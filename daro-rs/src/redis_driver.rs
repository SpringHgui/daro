//! Redis 驱动：连接 / 命名空间 / 键扫描 / 读值 / 写回。
//!
//! 口径照 Flutter 版 `redis_kv_driver.dart`：
//! - 每条连接按库缓存一条会话（SELECT 会改连接状态，不能共用）
//! - 键一律按二进制收，界面自己决定 UTF-8 还是 `HEX:` 显示
//! - 扫描只走 SCAN，一次调用只取一轮；游标为 0 表示遍历完
//! - 容器类型的读值有行数上限，超出标 `truncated`

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use redis::{Connection, Value};

/// 一次值读取最多返回的条目数（hash / list / set / zset / stream）。
pub const VALUE_LIMIT: usize = 1000;
/// 字符串值一次最多预览的字节数（与 Flutter 版的 64KB 对齐）。
pub const STRING_PREVIEW: usize = 64 * 1024;
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
    pub encoding: String,
    pub size: i64,
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

/// SCAN 一轮 + 元数据流水线。
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
        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            out.push(meta_of(c, key));
        }
        Ok(ScanPage {
            keys: out,
            next: (next != 0).then_some(next),
        })
    })
}

fn meta_of(c: &mut Connection, key: Vec<u8>) -> KeyMeta {
    let rtype = redis::cmd("TYPE")
        .arg(&key)
        .query::<String>(c)
        .unwrap_or_else(|_| "unknown".to_string());
    let ttl = redis::cmd("TTL")
        .arg(&key)
        .query::<i64>(c)
        .unwrap_or(-1);
    let encoding = redis::cmd("OBJECT")
        .arg("ENCODING")
        .arg(&key)
        .query::<String>(c)
        .unwrap_or_default();
    let size = redis::cmd("MEMORY")
        .arg("USAGE")
        .arg(&key)
        .query::<i64>(c)
        .unwrap_or(0);
    KeyMeta {
        key,
        rtype,
        ttl,
        encoding,
        size,
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
        let ttl = redis::cmd("TTL").arg(key).query::<i64>(c).unwrap_or(-1);
        let mut out = ValueRead {
            key: key.to_vec(),
            rtype: rtype.clone(),
            ttl,
            ..Default::default()
        };
        match rtype.as_str() {
            "string" => {
                let total: i64 = redis::cmd("STRLEN").arg(key).query(c).map_err(err)?;
                out.total = Some(total);
                out.bytes = Some(total);
                let end = if limit == 0 {
                    STRING_PREVIEW.saturating_sub(1)
                } else {
                    limit.saturating_sub(1)
                };
                let bytes: Vec<u8> = redis::cmd("GETRANGE")
                    .arg(key)
                    .arg(offset)
                    .arg(end)
                    .query(c)
                    .map_err(err)?;
                out.truncated = (bytes.len() as i64) < total;
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
}
