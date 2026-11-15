//! Redis 驱动：连接 / 命名空间 / 键扫描 / 读值 / 写回。
//!
//! 口径照 Flutter 版 `redis_kv_driver.dart`：
//! - 每条连接按库缓存一条会话（SELECT 会改连接状态，不能共用）
//! - 键一律按二进制收，界面自己决定 UTF-8 还是 `HEX:` 显示
//! - 扫描只走 SCAN，一次调用只取一轮；游标为 0 表示遍历完
//! - 容器类型的读值有行数上限，超出标 `truncated`
//! - 字符串值累计取到 `STRING_FULL_LIMIT` 为止（阈值内一次取全），更大的每轮
//!   `STRING_PREVIEW` 按 offset 续读，未取全标 `truncated`

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use redis::{Connection, Value};

/// 一次值读取最多返回的条目数（hash / list / set / zset / stream）。
pub const VALUE_LIMIT: usize = 1000;
/// 累计载入越过取全阈值后，每轮续读的字节数。
pub const STRING_PREVIEW: usize = 64 * 1024;
/// 不超过这个字节数的字符串一次取全，可以直接编辑；更大的先给预览，
/// 由界面「载入更多」按 offset 续读。取全阈值定在 2MB：再大编辑器要卡。
pub const STRING_FULL_LIMIT: usize = 2 * 1024 * 1024;
/// 建连超时；命令读写超时。远端失联时不能把界面永久挂住。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct Params {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

struct Cached {
    conn: Mutex<Connection>,
}

static CONNS: LazyLock<Mutex<HashMap<String, Arc<Cached>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache_key(conn: &str, db: u8) -> String {
    format!("{conn}\u{1}db{db}")
}

fn connect(p: &Params, db: u8) -> Result<Connection, String> {
    let host = p.host.trim();
    if host.is_empty() {
        return Err("Redis 连接缺少主机地址".to_string());
    }
    let port = if p.port == 0 { 6379 } else { p.port };
    let uri = format!("redis://{host}:{port}");
    let client = redis::Client::open(uri).map_err(|e| format!("Redis 地址无效：{e}"))?;
    let mut conn = client
        .get_connection_with_timeout(CONNECT_TIMEOUT)
        .map_err(|e| format!("Redis 连接失败：{e}"))?;
    let _ = conn.set_read_timeout(Some(IO_TIMEOUT));
    let _ = conn.set_write_timeout(Some(IO_TIMEOUT));
    if !p.password.is_empty() {
        let mut cmd = redis::cmd("AUTH");
        if !p.user.trim().is_empty() {
            cmd.arg(p.user.trim());
        }
        cmd.arg(&p.password);
        cmd.query::<Value>(&mut conn)
            .map_err(|e| format!("Redis 认证失败：{e}"))?;
    }
    if db != 0 {
        redis::cmd("SELECT")
            .arg(db)
            .query::<Value>(&mut conn)
            .map_err(|e| format!("Redis 切换库 db{db} 失败：{e}"))?;
    }
    Ok(conn)
}

fn cached(conn: &str, p: &Params, db: u8) -> Result<Arc<Cached>, String> {
    let key = cache_key(conn, db);
    if let Some(c) = CONNS.lock().unwrap().get(&key) {
        return Ok(c.clone());
    }
    let fresh = Arc::new(Cached {
        conn: Mutex::new(connect(p, db)?),
    });
    CONNS.lock().unwrap().insert(key, fresh.clone());
    Ok(fresh)
}

/// 一条连接是否已经建过会话（任意库）。
pub fn is_open(conn: &str) -> bool {
    let prefix = format!("{conn}\u{1}");
    CONNS.lock().unwrap().keys().any(|k| k.starts_with(&prefix))
}

/// 打开连接：先连 db0 探一次；`list_namespaces` 随后会按需连别的库。
pub fn open(conn: &str, p: &Params) -> Result<(), String> {
    cached(conn, p, 0).map(|_| ())
}

/// 确保连接可用；句柄丢了就重连。
pub fn ensure(conn: &str, p: &Params) -> Result<(), String> {
    cached(conn, p, 0).map(|_| ())
}

pub fn close(conn: &str) {
    let prefix = format!("{conn}\u{1}");
    CONNS
        .lock()
        .unwrap()
        .retain(|k, _| !k.starts_with(&prefix));
}

/// 「测试连接」：连上 db0，不注册进缓存。
pub fn probe(p: &Params) -> Result<(), String> {
    connect(p, 0).map(|_| ())
}

/// 跑一条命令；连接级错误（断线）把缓存摘掉，下次 ensure 会重连。
fn with_conn<T>(
    conn: &str,
    p: &Params,
    db: u8,
    f: impl FnOnce(&mut Connection) -> Result<T, String>,
) -> Result<T, String> {
    let cached = cached(conn, p, db)?;
    let mut guard = cached.conn.lock().unwrap();
    match f(&mut guard) {
        Ok(v) => Ok(v),
        Err(e) => {
            // 服务端业务错误不该扔掉连接；只有带 I/O / 连接字样的才摘
            let lower = e.to_ascii_lowercase();
            if lower.contains("connection")
                || lower.contains("broken pipe")
                || lower.contains("timed out")
                || lower.contains("reset")
            {
                CONNS.lock().unwrap().remove(&cache_key(conn, db));
            }
            Err(e)
        }
    }
}

fn err(e: redis::RedisError) -> String {
    e.to_string()
}

/// 一个库：`db0`、`db1`…；`keys` 来自 INFO keyspace（读不到就是 0）。
#[derive(Clone, Debug)]
pub struct Namespace {
    pub name: String,
    pub keys: usize,
}

/// 库数量：优先 CONFIG GET databases，其次 INFO keyspace 里最大的 dbN + 1。
pub fn list_namespaces(conn: &str, p: &Params) -> Result<Vec<Namespace>, String> {
    with_conn(conn, p, 0, |c| {
        let mut count = 1usize;
        if let Ok(v) = redis::cmd("CONFIG")
            .arg("GET")
            .arg("databases")
            .query::<Value>(c)
        {
            if let Some(n) = config_databases(&v) {
                count = n;
            }
        }
        let sizes = keyspace_sizes(c).unwrap_or_default();
        if count <= 1 {
            if let Some(max) = sizes.keys().max() {
                count = max + 1;
            }
        }
        Ok((0..count)
            .map(|i| Namespace {
                name: format!("db{i}"),
                keys: sizes.get(&i).copied().unwrap_or(0),
            })
            .collect())
    })
}

fn config_databases(v: &Value) -> Option<usize> {
    let Value::Array(items) = v else { return None };
    let mut it = items.iter();
    while let (Some(k), Some(val)) = (it.next(), it.next()) {
        if text_of(k).eq_ignore_ascii_case("databases") {
            return text_of(val).parse().ok().filter(|n: &usize| *n > 0);
        }
    }
    None
}

fn keyspace_sizes(c: &mut Connection) -> Option<HashMap<usize, usize>> {
    let info: String = redis::cmd("INFO").arg("keyspace").query(c).ok()?;
    let mut out = HashMap::new();
    for line in info.lines() {
        let Some((name, rest)) = line.split_once(':') else { continue };
        let Some(idx) = name.strip_prefix("db").and_then(|s| s.parse::<usize>().ok()) else {
            continue;
        };
        let keys = rest
            .split(',')
            .find_map(|kv| kv.strip_prefix("keys="))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        out.insert(idx, keys);
    }
    Some(out)
}

pub fn dbsize(conn: &str, p: &Params, db: u8) -> Result<usize, String> {
    with_conn(conn, p, db, |c| {
        redis::cmd("DBSIZE").query::<usize>(c).map_err(err)
    })
}

/// 键的显示文本：合法 UTF-8 原样显示，否则 `HEX:...`。
pub fn bytes_display(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => format!("HEX:{}", hex_upper(b)),
    }
}

fn hex_upper(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02X}"));
    }
    s
}

/// 界面输入的键名：`HEX:` 前缀按十六进制解析，其余按 UTF-8。
pub fn bytes_from_input(s: &str) -> Option<Vec<u8>> {
    if let Some(hex) = s.strip_prefix("HEX:") {
        let clean: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
        if clean.len() % 2 != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(clean.len() / 2);
        let bytes = clean.as_bytes();
        for i in (0..bytes.len()).step_by(2) {
            let hi = (bytes[i] as char).to_digit(16)?;
            let lo = (bytes[i + 1] as char).to_digit(16)?;
            out.push((hi * 16 + lo) as u8);
        }
        Some(out)
    } else {
        Some(s.as_bytes().to_vec())
    }
}

fn text_of(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::BulkString(b) => String::from_utf8_lossy(b).into_owned(),
        Value::SimpleString(s) => s.clone(),
        Value::Okay => "OK".to_string(),
        Value::Double(f) => f.to_string(),
        Value::Boolean(b) => b.to_string(),
        _ => String::new(),
    }
}

fn opt_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::BulkString(b) => std::str::from_utf8(b).ok()?.parse().ok(),
        Value::SimpleString(s) => s.parse().ok(),
        _ => None,
    }
}

/// 键列表的一行。
#[derive(Clone, Debug)]
pub struct KeyMeta {
    pub key: Vec<u8>,
    pub rtype: String,
    pub ttl: i64,
}

impl KeyMeta {
    pub fn display(&self) -> String {
        bytes_display(&self.key)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ScanPage {
    pub keys: Vec<KeyMeta>,
    /// 0 = 遍历完；非 0 下次接着扫
    pub next: Option<u64>,
}

/// SCAN 一轮 + 类型/TTL 流水线。存储细节（编码、字节数）只在选中时取，
/// 见 [`read_value`]：一页几百个键都问 MEMORY USAGE 会拖慢整库。
pub fn scan(
    conn: &str,
    p: &Params,
    db: u8,
    cursor: u64,
    pattern: &str,
    rtype: &str,
    count: usize,
) -> Result<ScanPage, String> {
    with_conn(conn, p, db, |c| {
        let mut cmd = redis::cmd("SCAN");
        cmd.arg(cursor).arg("COUNT").arg(count.max(1));
        if !pattern.is_empty() {
            cmd.arg("MATCH").arg(pattern);
        }
        if !rtype.is_empty() {
            cmd.arg("TYPE").arg(rtype);
        }
        let (next, keys): (String, Vec<Vec<u8>>) = cmd.query(c).map_err(err)?;
        let next = next.parse::<u64>().unwrap_or(0);
        // 整页一次流水线发出：远端两次往返填满一页。逐键串着问是每键两次往返，
        // 200 键的一页在杭州那台库上要等十几秒。
        let mut pipe = redis::pipe();
        for key in &keys {
            pipe.cmd("TYPE").arg(key);
            pipe.cmd("TTL").arg(key);
        }
        let replies: Vec<Value> = pipe.query(c).map_err(err)?;
        let mut it = replies.into_iter();
        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            out.push(KeyMeta {
                rtype: pop_text(&mut it, "unknown"),
                ttl: pop_int(&mut it, -1),
                key,
            });
        }
        Ok(ScanPage {
            keys: out,
            next: (next != 0).then_some(next),
        })
    })
}

/// 流水线回包里取文本（简单串 / 批量串）；错误、Nil 或断流按 `def`。
fn pop_text(it: &mut impl Iterator<Item = Value>, def: &str) -> String {
    match it.next() {
        Some(Value::BulkString(b)) => String::from_utf8(b).unwrap_or_default(),
        Some(Value::SimpleString(s)) => s,
        _ => def.to_string(),
    }
}

/// 流水线回包里取整数；错误、Nil 或断流按 `def`。
fn pop_int(it: &mut impl Iterator<Item = Value>, def: i64) -> i64 {
    match it.next() {
        Some(Value::Int(n)) => n,
        _ => def,
    }
}

/// 值面板的一行。
#[derive(Clone, Debug)]
pub struct ValueRow {
    /// 字段名 / 下标 / 条目 ID；字符串类型的值行没有标签
    pub label: String,
    pub value: Vec<u8>,
    pub score: Option<f64>,
    /// list 的下标（写回 / 删除时用）
    pub index: Option<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct ValueRead {
    pub key: Vec<u8>,
    pub rtype: String,
    pub rows: Vec<ValueRow>,
    /// 容器条目总数（string 是字节数）
    pub total: Option<i64>,
    pub ttl: i64,
    pub truncated: bool,
    pub bytes: Option<i64>,
    /// OBJECT ENCODING，取不到为空
    pub encoding: String,
    /// MEMORY USAGE 的字节数，取不到为 0
    pub size: i64,
}

pub fn read_value(
    conn: &str,
    p: &Params,
    db: u8,
    key: &[u8],
    offset: usize,
    limit: usize,
) -> Result<ValueRead, String> {
    with_conn(conn, p, db, |c| {
        let rtype: String = redis::cmd("TYPE").arg(key).query(c).map_err(err)?;
        // TTL 与存储细节一次流水线问齐：列表页不再替每个键问 MEMORY USAGE
        let mut pipe = redis::pipe();
        pipe.cmd("TTL").arg(key);
        pipe.cmd("OBJECT").arg("ENCODING").arg(key);
        pipe.cmd("MEMORY").arg("USAGE").arg(key);
        let replies: Vec<Value> = pipe.query(c).unwrap_or_default();
        let mut it = replies.into_iter();
        let mut out = ValueRead {
            key: key.to_vec(),
            rtype: rtype.clone(),
            ttl: pop_int(&mut it, -1),
            encoding: pop_text(&mut it, ""),
            size: pop_int(&mut it, 0),
            ..Default::default()
        };
        match rtype.as_str() {
            "string" => {
                let total: i64 = redis::cmd("STRLEN").arg(key).query(c).map_err(err)?;
                out.total = Some(total);
                out.bytes = Some(total);
                // limit 只管容器条目数；字符串按 offset（字节偏移）取一块：
                // 累计没到取全阈值前一次给满（阈值内的值即一次取全），
                // 之后每轮 STRING_PREVIEW，由界面「载入更多」续读
                let want = STRING_FULL_LIMIT.saturating_sub(offset).max(STRING_PREVIEW);
                let end = offset + want - 1;
                let bytes: Vec<u8> = redis::cmd("GETRANGE")
                    .arg(key)
                    .arg(offset)
                    .arg(end)
                    .query(c)
                    .map_err(err)?;
                out.truncated = (offset + bytes.len()) < total as usize;
                out.rows.push(ValueRow {
                    label: String::new(),
                    value: bytes,
                    score: None,
                    index: None,
                });
            }
            "list" => {
                let total: i64 = redis::cmd("LLEN").arg(key).query(c).map_err(err)?;
                out.total = Some(total);
                let stop = offset as i64 + limit.max(1) as i64 - 1;
                let vals: Vec<Vec<u8>> = redis::cmd("LRANGE")
                    .arg(key)
                    .arg(offset)
                    .arg(stop)
                    .query(c)
                    .map_err(err)?;
                out.truncated = (offset + vals.len()) < total as usize;
                for (i, v) in vals.into_iter().enumerate() {
                    let ix = offset as i64 + i as i64;
                    out.rows.push(ValueRow {
                        label: ix.to_string(),
                        value: v,
                        score: None,
                        index: Some(ix),
                    });
                }
            }
            "hash" => {
                let total: i64 = redis::cmd("HLEN").arg(key).query(c).map_err(err)?;
                out.total = Some(total);
                let mut cursor = 0u64;
                while out.rows.len() < limit.max(1) {
                    let (next, flat): (String, Vec<Vec<u8>>) = redis::cmd("HSCAN")
                        .arg(key)
                        .arg(cursor)
                        .arg("COUNT")
                        .arg(limit.max(1))
                        .query(c)
                        .map_err(err)?;
                    let mut it = flat.into_iter();
                    while let (Some(f), Some(v)) = (it.next(), it.next()) {
                        out.rows.push(ValueRow {
                            label: bytes_display(&f),
                            value: v,
                            score: None,
                            index: None,
                        });
                        if out.rows.len() >= limit.max(1) {
                            break;
                        }
                    }
                    cursor = next.parse().unwrap_or(0);
                    if cursor == 0 {
                        break;
                    }
                }
                out.truncated = out.rows.len() < total as usize;
            }
            "set" => {
                let total: i64 = redis::cmd("SCARD").arg(key).query(c).map_err(err)?;
                out.total = Some(total);
                let mut cursor = 0u64;
                while out.rows.len() < limit.max(1) {
                    let (next, vals): (String, Vec<Vec<u8>>) = redis::cmd("SSCAN")
                        .arg(key)
                        .arg(cursor)
                        .arg("COUNT")
                        .arg(limit.max(1))
                        .query(c)
                        .map_err(err)?;
                    for v in vals {
                        out.rows.push(ValueRow {
                            label: bytes_display(&v),
                            value: v,
                            score: None,
                            index: None,
                        });
                        if out.rows.len() >= limit.max(1) {
                            break;
                        }
                    }
                    cursor = next.parse().unwrap_or(0);
                    if cursor == 0 {
                        break;
                    }
                }
                out.truncated = out.rows.len() < total as usize;
            }
            "zset" => {
                let total: i64 = redis::cmd("ZCARD").arg(key).query(c).map_err(err)?;
                out.total = Some(total);
                let stop = offset as i64 + limit.max(1) as i64 - 1;
                let flat: Vec<Vec<u8>> = redis::cmd("ZRANGE")
                    .arg(key)
                    .arg(offset)
                    .arg(stop)
                    .arg("WITHSCORES")
                    .query(c)
                    .map_err(err)?;
                let mut it = flat.into_iter();
                while let (Some(m), Some(s)) = (it.next(), it.next()) {
                    out.rows.push(ValueRow {
                        label: bytes_display(&m),
                        value: m,
                        score: String::from_utf8_lossy(&s).parse().ok(),
                        index: None,
                    });
                }
                out.truncated = (offset + out.rows.len()) < total as usize;
            }
            "stream" => {
                let total: i64 = redis::cmd("XLEN").arg(key).query(c).map_err(err)?;
                out.total = Some(total);
                let entries: Vec<Value> = redis::cmd("XRANGE")
                    .arg(key)
                    .arg("-")
                    .arg("+")
                    .arg("COUNT")
                    .arg(limit.max(1))
                    .query(c)
                    .map_err(err)?;
                for e in entries {
                    let Value::Array(mut pair) = e else { continue };
                    if pair.len() < 2 {
                        continue;
                    }
                    let fields = pair.pop().unwrap_or(Value::Nil);
                    let id = pair.pop().map(|v| text_of(&v)).unwrap_or_default();
                    let Value::Array(flat) = fields else { continue };
                    let mut parts = Vec::new();
                    let mut it = flat.into_iter();
                    while let (Some(f), Some(v)) = (it.next(), it.next()) {
                        parts.push(format!("{}={}", text_of(&f), text_of(&v)));
                    }
                    out.rows.push(ValueRow {
                        label: id,
                        value: parts.join("; ").into_bytes(),
                        score: None,
                        index: None,
                    });
                }
                out.truncated = out.rows.len() < total as usize;
            }
            "none" => return Err("键已不存在".to_string()),
            _ => return Err(format!("暂不支持的 Redis 类型：{rtype}")),
        }
        Ok(out)
    })
}

// ── 写回 ─────────────────────────────────────────────────────────

pub fn set_string(
    conn: &str,
    p: &Params,
    db: u8,
    key: &[u8],
    value: &[u8],
    ttl: Option<i64>,
) -> Result<(), String> {
    with_conn(conn, p, db, |c| {
        let mut cmd = redis::cmd("SET");
        cmd.arg(key).arg(value);
        match ttl {
            Some(sec) if sec > 0 => {
                cmd.arg("EX").arg(sec);
            }
            None => {
                cmd.arg("KEEPTTL");
            }
            _ => {}
        }
        cmd.query::<Value>(c).map_err(err).map(|_| ())
    })
}

pub fn delete_keys(conn: &str, p: &Params, db: u8, keys: &[Vec<u8>]) -> Result<usize, String> {
    if keys.is_empty() {
        return Ok(0);
    }
    with_conn(conn, p, db, |c| {
        let mut cmd = redis::cmd("DEL");
        for k in keys {
            cmd.arg(k);
        }
        cmd.query::<usize>(c).map_err(err)
    })
}

/// `ttl = None` → PERSIST；否则 EXPIRE。
pub fn set_ttl(
    conn: &str,
    p: &Params,
    db: u8,
    keys: &[Vec<u8>],
    ttl: Option<i64>,
) -> Result<usize, String> {
    with_conn(conn, p, db, |c| {
        let mut n = 0;
        for k in keys {
            let mut cmd = redis::cmd(if ttl.is_some() { "EXPIRE" } else { "PERSIST" });
            cmd.arg(k);
            if let Some(sec) = ttl {
                cmd.arg(sec.max(0));
            }
            if cmd.query::<i64>(c).unwrap_or(0) > 0 {
                n += 1;
            }
        }
        Ok(n)
    })
}

pub fn hash_write(
    conn: &str,
    p: &Params,
    db: u8,
    key: &[u8],
    set: &[(Vec<u8>, Vec<u8>)],
    remove: &[Vec<u8>],
) -> Result<(), String> {
    with_conn(conn, p, db, |c| {
        if !set.is_empty() {
            let mut cmd = redis::cmd("HSET");
            cmd.arg(key);
            for (f, v) in set {
                cmd.arg(f).arg(v);
            }
            cmd.query::<i64>(c).map_err(err)?;
        }
        if !remove.is_empty() {
            let mut cmd = redis::cmd("HDEL");
            cmd.arg(key);
            for f in remove {
                cmd.arg(f);
            }
            cmd.query::<i64>(c).map_err(err)?;
        }
        Ok(())
    })
}

pub fn list_write(
    conn: &str,
    p: &Params,
    db: u8,
    key: &[u8],
    set: &[(i64, Vec<u8>)],
    remove: &[i64],
    push: Option<&[u8]>,
) -> Result<(), String> {
    with_conn(conn, p, db, |c| {
        for (ix, v) in set {
            redis::cmd("LSET")
                .arg(key)
                .arg(*ix)
                .arg(v)
                .query::<Value>(c)
                .map_err(err)?;
        }
        if !remove.is_empty() {
            // 从大到小删，前面的下标才不会漂
            let mut idx = remove.to_vec();
            idx.sort_unstable();
            idx.reverse();
            // 用一个哨兵值占位再 LREM，等价于按下标删
            let sentinel = format!("\u{1}daro-del-{}", std::process::id());
            for i in idx {
                redis::cmd("LSET")
                    .arg(key)
                    .arg(i)
                    .arg(sentinel.as_bytes())
                    .query::<Value>(c)
                    .map_err(err)?;
                redis::cmd("LREM")
                    .arg(key)
                    .arg(1)
                    .arg(sentinel.as_bytes())
                    .query::<i64>(c)
                    .map_err(err)?;
            }
        }
        if let Some(v) = push {
            redis::cmd("RPUSH")
                .arg(key)
                .arg(v)
                .query::<i64>(c)
                .map_err(err)?;
        }
        Ok(())
    })
}

pub fn set_write(
    conn: &str,
    p: &Params,
    db: u8,
    key: &[u8],
    add: &[Vec<u8>],
    remove: &[Vec<u8>],
) -> Result<(), String> {
    with_conn(conn, p, db, |c| {
        if !add.is_empty() {
            let mut cmd = redis::cmd("SADD");
            cmd.arg(key);
            for m in add {
                cmd.arg(m);
            }
            cmd.query::<i64>(c).map_err(err)?;
        }
        if !remove.is_empty() {
            let mut cmd = redis::cmd("SREM");
            cmd.arg(key);
            for m in remove {
                cmd.arg(m);
            }
            cmd.query::<i64>(c).map_err(err)?;
        }
        Ok(())
    })
}

pub fn zset_write(
    conn: &str,
    p: &Params,
    db: u8,
    key: &[u8],
    add: &[(Vec<u8>, f64)],
    remove: &[Vec<u8>],
) -> Result<(), String> {
    with_conn(conn, p, db, |c| {
        if !add.is_empty() {
            let mut cmd = redis::cmd("ZADD");
            cmd.arg(key);
            for (m, s) in add {
                cmd.arg(*s).arg(m);
            }
            cmd.query::<i64>(c).map_err(err)?;
        }
        if !remove.is_empty() {
            let mut cmd = redis::cmd("ZREM");
            cmd.arg(key);
            for m in remove {
                cmd.arg(m);
            }
            cmd.query::<i64>(c).map_err(err)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_keeps_utf8_and_hexes_binary() {
        assert_eq!(bytes_display(b"user:1"), "user:1");
        assert_eq!(bytes_display(&[0xFF, 0x00, 0x1A]), "HEX:FF001A");
    }

    #[test]
    fn input_parses_plain_and_hex() {
        assert_eq!(bytes_from_input("abc"), Some(b"abc".to_vec()));
        assert_eq!(bytes_from_input("HEX:41 42"), Some(b"AB".to_vec()));
        assert_eq!(bytes_from_input("HEX:4"), None);
        assert_eq!(bytes_from_input("HEX:4G"), None);
    }

    #[test]
    fn cache_key_scopes_connection_and_db() {
        assert_ne!(cache_key("a", 0), cache_key("a", 1));
        assert_ne!(cache_key("a", 0), cache_key("b", 0));
    }

    /// 最小 RESP 服务端：只答键列表与字符串读写用到的命令，够把往返次数、分页、
    /// 保存口径跑在真 socket 上。一条连接持续服务，顺带验证驱动复用同一会话。
    /// 第二个返回值是「命令批次数」：一批 = 服务端一次读到的命令组，即往返次数。
    fn serve(
        keys: Vec<Vec<u8>>,
        seed: Vec<u8>,
    ) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read, Write};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let lst = std::net::TcpListener::bind("127.0.0.1:0").expect("占用本地端口");
        let port = lst.local_addr().unwrap().port();
        let batches = std::sync::Arc::new(AtomicUsize::new(0));
        let counted = batches.clone();
        std::thread::spawn(move || {
            let Ok(mut sock) = lst.accept().map(|(s, _)| s) else {
                return;
            };
            let mut value = seed;
            let mut buf: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 64 * 1024];
            loop {
                let mut handled = 0usize;
                while let Some((cmd, used)) = parse_command(&buf) {
                    buf.drain(..used);
                    handled += 1;
                    let out = reply(&cmd, &keys, &mut value);
                    if sock.write_all(&out).is_err() {
                        return;
                    }
                }
                if handled > 0 {
                    counted.fetch_add(1, Ordering::Relaxed);
                }
                match sock.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
        });
        (port, batches)
    }

    /// 单条命令的回包；未知命令一律 +OK（握手用）。
    fn reply(cmd: &[Vec<u8>], keys: &[Vec<u8>], value: &mut Vec<u8>) -> Vec<u8> {
        let name = cmd.first().map(|a| a.to_ascii_uppercase()).unwrap_or_default();
        let arg = |ix: usize| {
            cmd.get(ix)
                .and_then(|a| std::str::from_utf8(a).ok())
                .and_then(|s| s.parse::<i64>().ok())
        };
        match name.as_slice() {
            b"SCAN" => {
                let mut out = b"*2\r\n$1\r\n0\r\n".to_vec();
                out.extend_from_slice(format!("*{}\r\n", keys.len()).as_bytes());
                for k in keys {
                    out.extend_from_slice(&bulk(k));
                }
                out
            }
            b"TYPE" => b"+string\r\n".to_vec(),
            b"TTL" => b":3600\r\n".to_vec(),
            b"OBJECT" => bulk(b"raw"),
            b"MEMORY" | b"STRLEN" => int(value.len() as i64),
            b"GETRANGE" => {
                let from = arg(2).unwrap_or(0);
                let to = arg(3).unwrap_or(-1).min(value.len() as i64 - 1);
                let slice: &[u8] =
                    if from < 0 || to < from || from as usize >= value.len() {
                        &[]
                    } else {
                        &value[from as usize..=to as usize]
                    };
                bulk(slice)
            }
            // 第三参是要写入的值；KEEPTTL / EX 这里不建模，只记下负载
            b"SET" => {
                if let Some(v) = cmd.get(2) {
                    *value = v.clone();
                }
                b"+OK\r\n".to_vec()
            }
            _ => b"+OK\r\n".to_vec(),
        }
    }

    fn bulk(bytes: &[u8]) -> Vec<u8> {
        let mut out = format!("${}\r\n", bytes.len()).into_bytes();
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\r\n");
        out
    }

    fn int(n: i64) -> Vec<u8> {
        format!(":{n}\r\n").into_bytes()
    }

    /// 缓冲区开头若是一条完整命令，返回参数与占用字节数；不完整返回 None 等下次读。
    fn parse_command(buf: &[u8]) -> Option<(Vec<Vec<u8>>, usize)> {
        if buf.first() != Some(&b'*') {
            return None;
        }
        let (n, mut pos) = read_line_number(buf, 1)?;
        let mut args = Vec::with_capacity(n);
        for _ in 0..n {
            if buf.get(pos) != Some(&b'$') {
                return None;
            }
            let (len, next) = read_line_number(buf, pos + 1)?;
            if buf.len() < next + len + 2 {
                return None;
            }
            args.push(buf[next..next + len].to_vec());
            pos = next + len + 2;
        }
        Some((args, pos))
    }

    /// 从 `from` 起读一行十进制数，返回数值与行尾之后的位置。
    fn read_line_number(buf: &[u8], from: usize) -> Option<(usize, usize)> {
        let end = from + buf[from..].windows(2).position(|w| w == b"\r\n")?;
        let n: usize = std::str::from_utf8(&buf[from..end]).ok()?.parse().ok()?;
        Some((n, end + 2))
    }

    #[test]
    fn string_under_threshold_arrives_whole() {
        // 用户报的那个尺寸：两万像素的 JSON 配置，旧代码只给前 1000 字节
        let raw: Vec<u8> = (0..20445usize).map(|i| b'a' + (i % 26) as u8).collect();
        let (port, _) = serve(vec![b"BXConfig".to_vec()], raw.clone());
        let p = Params {
            host: "127.0.0.1".into(),
            port,
            user: String::new(),
            password: String::new(),
        };
        let v = read_value("verify-whole", &p, 0, b"BXConfig", 0, VALUE_LIMIT).unwrap();
        assert_eq!(v.total, Some(raw.len() as i64));
        assert!(!v.truncated, "取全阈值内的值不该再被截断");
        assert_eq!(v.rows[0].value, raw);
        assert_eq!(v.ttl, 3600);
        // 存储细节跟着值一起回，列表页就不用替每个键问了
        assert_eq!(v.encoding, "raw");
        assert_eq!(v.size, raw.len() as i64);
    }

    #[test]
    fn huge_string_pages_from_offset_until_whole() {
        let raw: Vec<u8> = (0..STRING_FULL_LIMIT + 70_000)
            .map(|i| (i % 251) as u8)
            .collect();
        let (port, _) = serve(vec![b"big".to_vec()], raw.clone());
        let p = Params {
            host: "127.0.0.1".into(),
            port,
            user: String::new(),
            password: String::new(),
        };
        let first = read_value("verify-page", &p, 0, b"big", 0, VALUE_LIMIT).unwrap();
        assert_eq!(first.rows[0].value.len(), STRING_FULL_LIMIT);
        assert!(first.truncated, "超过阈值只给到阈值，剩下靠载入更多");
        let mut got = first.rows[0].value.clone();
        while got.len() < raw.len() {
            let next =
                read_value("verify-page", &p, 0, b"big", got.len(), VALUE_LIMIT).unwrap();
            assert!(next.rows[0].value.len() <= STRING_PREVIEW);
            got.extend_from_slice(&next.rows[0].value);
            assert_eq!(next.truncated, got.len() < raw.len());
        }
        assert_eq!(got, raw);
    }

    #[test]
    fn save_writes_the_payload_back_byte_for_byte() {
        let (port, _) = serve(vec![b"cfg".to_vec()], vec![b'x'; 5000]);
        let p = Params {
            host: "127.0.0.1".into(),
            port,
            user: String::new(),
            password: String::new(),
        };
        // 界面压成紧凑 JSON 后交下来：超 i64 的整数与键序都得原样落库
        let payload = br#"{"a":1,"list":[1,2,3],"big":99999999999999999999}"#;
        set_string("verify-save", &p, 0, b"cfg", payload, None).unwrap();
        let v = read_value("verify-save", &p, 0, b"cfg", 0, VALUE_LIMIT).unwrap();
        assert_eq!(v.rows[0].value, payload.to_vec());
        assert!(!v.truncated);
    }

    #[test]
    fn key_page_costs_two_round_trips() {
        let keys: Vec<Vec<u8>> = (0..200).map(|i| format!("20261114_{i:04}").into_bytes()).collect();
        let (port, batches) = serve(keys.clone(), vec![b'x'; 10]);
        let p = Params {
            host: "127.0.0.1".into(),
            port,
            user: String::new(),
            password: String::new(),
        };
        let page = scan("verify-scan", &p, 0, 0, "", "", 200).unwrap();
        assert_eq!(page.keys.len(), 200);
        assert_eq!(page.next, None, "游标回 0 就是遍历完");
        assert!(
            page.keys.iter().all(|m| m.rtype == "string" && m.ttl == 3600),
            "整页类型/TTL 都得填上"
        );
        // 一页只该花两次往返：SCAN 一次、元数据流水线一次。逐键串问会是 401 次，
        // 杭州那台库上按 30ms 一个来回算就是十几秒。
        let got = batches.load(std::sync::atomic::Ordering::Relaxed);
        assert!(got <= 4, "往返批次 = {got}，元数据没走流水线？");
    }
}
