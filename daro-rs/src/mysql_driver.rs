//! MySQL 真实驱动（`mysql` crate 的同步 `Conn`）。
//!
//! 与 SQLite / PostgreSQL 两档共用 `db_types` 的网格口径（列元数据 / 行身份 /
//! 一页取数 / 一条语句的回执），MySQL 这档改了形态的地方：
//!
//! - **一条连接看全部库**：MySQL 的连接不像 PG 那样只认一个库，登录后
//!   `库.表` 全限定就能访问任意库，所以注册表的键就是「连接名」一条。
//! - **没有模式层**：库下面直接是表 / 视图，树上不画模式节点（与 SQLite 同构）。
//! - **行身份用主键**：MySQL 没有可依赖的稳定行号，取数时按主键列的值拼行身份；
//!   没有主键的表不支撑改 / 删（与 PG 档同口径）。
//!
//! 取数一律走文本协议（`query_iter`）：数值 / 日期时间 / 十六进制都按服务端给的
//! 文本原样展示，不逐类型解码。行按协议逐条读，只保留前 `max_rows` 行走行数
//! 上限截断，剩余行由 crate 在收尾时读完丢弃（占网络、不占内存）；客户端内存
//! 始终只有「已保留的行 + 当前这一行」。行描述里就带列名与类型码，查询页不用
//! 另问一次列形。
//!
//! 写回走预处理语句 + 绑定参数：值一律按文本送，MySQL 自己按列类型转。
//! 不拼字面量，也就绕开了 MySQL 的反斜杠转义（`NO_BACKSLASH_ESCAPES` 模式下
//! 拼出来的字面量含义会变）这摊事。
//!
//! 认证：MySQL 8 默认的 `caching_sha2_password` 在明文通道上要走「向服务端
//! 要公钥 → 用公钥加密密码」这条流程，`mysql` crate 已内置，不用自己写。
//! 本驱动不启用 TLS（服务端强制 SSL 时连不上，错误原样上屏）。
//!
//! 取消：后台脚本的「停止」走 MySQL 的 `KILL QUERY <connection_id>` —— 另开
//! 一条连接发取消请求，正在跑的语句会被服务端打断（与 PG 档的 CancelToken
//! 同一口径，只是换了个服务端机制）。
//!
//! 超时：目录类查询（库 / 表 / 列元数据）另开一条短超时连接跑，卡死的目录
//! 查询不会占着注册表那把锁连带堵死取数；数据连接给足超时。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use mysql::consts::ColumnType;
use mysql::prelude::Queryable;
use mysql::{Column, Conn, OptsBuilder, Params as MParams, TxOpts, Value};

use crate::db_types::{ColMeta, ExecOutcome, GridChanges, RowKey, ScriptRun, SelectPage, StmtRun};

/// 查询页一次最多取回的行数（超出只标「已截断」，不无限吃内存）。
pub const QUERY_ROW_LIMIT: usize = 5000;

/// 建连上限（秒）：树展开 / 打开页签在 UI 线程同步建连，服务器不可达时
/// 不能让 TCP 默认超时把界面冻住十几秒。
const CONNECT_TIMEOUT_SECS: u64 = 5;

/// 目录类查询的上限（秒）：库 / 表 / 列元数据都是服务端秒级就该回来的小查询。
const CATALOG_TIMEOUT_SECS: u64 = 5;

/// 数据连接的上限（秒）：大表翻页 / 慢查询允许跑久一点。
const DATA_TIMEOUT_SECS: u64 = 30;

/// 与 SQLite / PG 两档同一口径：真 NULL 显示成字面 `NULL`。
const NULL_TEXT: &str = "NULL";

/// 建一条 MySQL 连接要的东西。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Params {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// 登录时的默认库（空 = 不选）。MySQL 一条连接能看全部库，目录 / 取数
    /// 都走 `库.表` 全限定，所以这里只是「登录后落在哪个库」。
    pub dbname: String,
}

/// 注册表：连接名 → 连接。一条连接看全部库，所以键里没有库名。
/// 锁只护住这张表本身，**绝不跨网络 I/O 持有**：每条连接自带一把锁，
/// 谁用它谁锁它。UI 线程点「关闭连接」时只做一次表操作，不会被后台正在跑的
/// 建连 / 取数堵住（那曾把界面冻死）。
static CONNS: LazyLock<Mutex<HashMap<String, Arc<Mutex<Conn>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 注册表里取一条连接；持注册表锁的时间只够一次克隆。
fn handle(key: &str) -> Result<Arc<Mutex<Conn>>, String> {
    CONNS
        .lock()
        .unwrap()
        .get(key)
        .cloned()
        .ok_or_else(|| "连接未打开".to_string())
}

/// 服务端错误要能读懂：mysql crate 的 Display 已经带了错误码与 SQLSTATE。
fn err_text(e: &mysql::Error) -> String {
    e.to_string()
}

/// 连接参数 → OptsBuilder。
///
/// `OptsBuilder` 的 setter 是「取走 self、还回新 self」的形态，所以只能
/// 一条链写完；条件项（默认库）单独接在链尾。
fn opts_builder(p: &Params, read_timeout: Duration) -> OptsBuilder {
    let b = OptsBuilder::default()
        .ip_or_hostname(Some(p.host.clone()))
        .tcp_port(p.port)
        .user(Some(p.user.clone()))
        .pass(Some(p.password.clone()))
        .read_timeout(Some(read_timeout))
        .write_timeout(Some(read_timeout))
        .tcp_connect_timeout(Some(Duration::from_secs(CONNECT_TIMEOUT_SECS)));
    if p.dbname.trim().is_empty() {
        b
    } else {
        b.db_name(Some(p.dbname.trim().to_string()))
    }
}

/// 连一条数据连接（超时给足）。
fn connect(p: &Params) -> Result<Conn, String> {
    let opts = opts_builder(p, Duration::from_secs(DATA_TIMEOUT_SECS));
    Conn::new(opts).map_err(|e| err_text(&e))
}

/// 连一条目录连接（超时短）：目录查询跑在它上面，卡死了也不占注册表的锁。
fn catalog_connect(p: &Params) -> Result<Conn, String> {
    let opts = opts_builder(p, Duration::from_secs(CATALOG_TIMEOUT_SECS));
    Conn::new(opts).map_err(|e| err_text(&e))
}

/// 打开（或重开）一条连接并登记。
pub fn open(key: &str, p: &Params) -> Result<(), String> {
    let conn = connect(p)?;
    CONNS
        .lock()
        .unwrap()
        .insert(key.to_string(), Arc::new(Mutex::new(conn)));
    Ok(())
}

pub fn is_open(key: &str) -> bool {
    CONNS.lock().unwrap().contains_key(key)
}

/// 惰性建连：树上展开到某个库才建连。
pub fn ensure(key: &str, p: &Params) -> Result<(), String> {
    if is_open(key) {
        return Ok(());
    }
    open(key, p)
}

/// 关一条连接（断开 / 删除 / 改名）。只从注册表摘句柄，不等在飞的查询：
/// 那条查询手里还捏着 `Arc`，跑完自己释放。
pub fn close(key: &str) {
    let removed = CONNS.lock().unwrap().remove(key);
    drop(removed);
}

/// 「测试连接」专用：连上去跑一条 SELECT 1，不注册。
pub fn probe(p: &Params) -> Result<(), String> {
    let mut conn = catalog_connect(p)?;
    conn.query_drop("SELECT 1").map_err(|e| err_text(&e))
}

/// 注册表里取出句柄、锁住这条连接执行 `f`；注册表锁不跨 I/O。
fn with_conn<T>(key: &str, f: impl FnOnce(&mut Conn) -> Result<T, String>) -> Result<T, String> {
    let conn = handle(key)?;
    let mut conn = conn.lock().unwrap();
    f(&mut conn)
}

// ── SQL 片段 ─────────────────────────────────────────────────────

/// 引号包标识符（MySQL 口径：反引号，内部 ` 双写）。
pub fn quote_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// `库`.`表` 形态的全限定名。
pub fn qualify(db: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(db), quote_ident(table))
}

/// 单引号字面量（内部 ' 双写）。**只用于拼只读 SQL 的常量**（目录查询里
/// 拼库名 / 表名）；写回一律走绑定参数，不碰这摊反斜杠转义。
fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
}

/// MySQL 的类型名 → 数值列判定（右对齐 + 数值排序）。
/// 目录里 `DATA_TYPE` 给的是基础类型（int / decimal / double …）。
pub fn ty_is_numeric(ty: &str) -> bool {
    let t = ty.to_ascii_lowercase();
    matches!(
        t.as_str(),
        "int" | "integer" | "bigint" | "smallint" | "tinyint" | "mediumint" | "decimal"
            | "numeric" | "float" | "double" | "real" | "bit" | "year"
    )
}

/// 行数上限之类的整数夹到 MySQL 能装的范围。
fn cap(n: usize) -> u64 {
    (n as u64).min(i64::MAX as u64)
}

// ── 值 → 文本 ────────────────────────────────────────────────────

/// 一个 MySQL 值 → 网格里显示的文本。
///
/// 文本协议下绝大多数值已经是字节串；数值 / 日期时间由服务端按类型给出，
/// 这里按统一口径折成文本（与 SQLite / PG 两档的显示一致）。
pub fn value_text(v: &Value) -> String {
    match v {
        Value::NULL => NULL_TEXT.to_string(),
        Value::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => s.to_string(),
            Err(_) => format!("<BLOB {} 字节>", b.len()),
        },
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(d) => d.to_string(),
        Value::Date(y, mo, d, h, mi, s, micro) => {
            let date = format!("{y:04}-{mo:02}-{d:02}");
            if *h == 0 && *mi == 0 && *s == 0 && *micro == 0 {
                date
            } else {
                let time = format!("{h:02}:{mi:02}:{s:02}");
                if *micro == 0 {
                    format!("{date} {time}")
                } else {
                    format!("{date} {time}.{micro:06}")
                }
            }
        }
        Value::Time(neg, days, h, mi, s, micro) => {
            let total_hours = days * 24 + *h as u32;
            let sign = if *neg { "-" } else { "" };
            let base = format!("{sign}{total_hours:02}:{mi:02}:{s:02}");
            if *micro == 0 {
                base
            } else {
                format!("{base}.{micro:06}")
            }
        }
    }
}

/// 列名去重：`SELECT a, a` 会给出两个同名列，网格按名字筛选 / 排序要能区分
/// （与另两档同口径的 `name:n` 后缀）。
fn dedup_names(cols: Vec<ColMeta>) -> Vec<ColMeta> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = cols;
    for m in out.iter_mut() {
        let n = seen.entry(m.name.clone()).or_insert(0);
        if *n > 0 {
            m.name = format!("{}:{n}", m.name);
        }
        *n += 1;
    }
    out
}

// ── 目录清单 ─────────────────────────────────────────────────────

/// 目录类查询统一走短超时连接：跑完就把连接还回去。
fn catalog_rows(p: &Params, sql: &str) -> Result<Vec<Vec<String>>, String> {
    let mut conn = catalog_connect(p)?;
    let mut res = conn.query_iter(sql).map_err(|e| err_text(&e))?;
    let mut out = Vec::new();
    if let Some(mut set) = res.iter() {
        while let Some(r) = set.next() {
            let row = r.map_err(|e| err_text(&e))?;
            out.push(
                (0..row.len())
                    .map(|i| row.as_ref(i).map(value_text).unwrap_or_else(|| NULL_TEXT.to_string()))
                    .collect(),
            );
        }
    }
    Ok(out)
}

/// 目录类查询的单列文本清单。
fn catalog_list(p: &Params, sql: &str) -> Result<Vec<String>, String> {
    Ok(catalog_rows(p, sql)?
        .into_iter()
        .filter_map(|r| r.into_iter().next())
        .collect())
}

/// 服务器上的库清单。
pub fn try_list_databases(p: &Params) -> Result<Vec<String>, String> {
    catalog_list(p, "SHOW DATABASES")
}

/// 某库下的表清单（BASE TABLE）。
pub fn try_list_tables(p: &Params, db: &str) -> Result<Vec<String>, String> {
    let sql = format!(
        "SELECT TABLE_NAME FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = {} AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME",
        lit(db)
    );
    catalog_list(p, &sql)
}

/// 某库下的视图清单。
pub fn try_list_views(p: &Params, db: &str) -> Result<Vec<String>, String> {
    let sql = format!(
        "SELECT TABLE_NAME FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = {} AND TABLE_TYPE = 'VIEW' ORDER BY TABLE_NAME",
        lit(db)
    );
    catalog_list(p, &sql)
}

/// 某库下的函数 / 存储过程清单（同名重载在树上只出现一次）。
pub fn try_list_routines(p: &Params, db: &str) -> Result<Vec<String>, String> {
    let sql = format!(
        "SELECT DISTINCT ROUTINE_NAME FROM information_schema.ROUTINES \
         WHERE ROUTINE_SCHEMA = {} ORDER BY ROUTINE_NAME",
        lit(db)
    );
    catalog_list(p, &sql)
}

/// 表 / 视图的列元数据：类型文案走 `COLUMN_TYPE`（带长度 / 无符号），
/// 生成列标只读，主键列打标（写回就按这些列定位行）。
pub fn try_table_columns(p: &Params, db: &str, table: &str) -> Result<Vec<ColMeta>, String> {
    let sql = format!(
        "SELECT COLUMN_NAME, COLUMN_TYPE, DATA_TYPE, EXTRA, COLUMN_KEY \
         FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} ORDER BY ORDINAL_POSITION",
        lit(db),
        lit(table)
    );
    Ok(cols_from_rows(catalog_rows(p, &sql)?))
}

/// 列元数据的行 → `ColMeta`。
fn cols_from_rows(rows: Vec<Vec<String>>) -> Vec<ColMeta> {
    let mut cols = Vec::with_capacity(rows.len());
    for r in rows {
        if r.len() < 5 {
            continue;
        }
        let name = r[0].clone();
        let ty = r[1].clone();
        let data_ty = r[2].clone();
        let extra = r[3].to_ascii_lowercase();
        let mut meta = ColMeta::new(name, ty.clone(), ty_is_numeric(&data_ty));
        // STORED / VIRTUAL GENERATED 列由服务端算，写回只能跳过
        meta.writable = !extra.contains("generated");
        // 自增列由服务端赋值：INSERT 跳过它（空串插进去会报类型错），
        // UPDATE 也一并禁掉，免得用户改出自增序列的洞
        if extra.contains("auto_increment") {
            meta.writable = false;
        }
        meta.pk = r[4].eq_ignore_ascii_case("PRI");
        cols.push(meta);
    }
    cols
}

/// 某库下的「表 → 列」，查询编辑器补全用；一次查完。
pub fn all_columns_by_table(p: &Params, db: &str) -> Vec<(String, Vec<String>)> {
    let sql = format!(
        "SELECT TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME, ORDINAL_POSITION",
        lit(db)
    );
    let Ok(rows) = catalog_rows(p, &sql) else {
        return Vec::new();
    };
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for row in rows {
        let (Some(table), Some(col)) = (row.first(), row.get(1)) else {
            continue;
        };
        match out.last_mut() {
            Some((name, cols)) if name == table => cols.push(col.clone()),
            _ => out.push((table.clone(), vec![col.clone()])),
        }
    }
    out
}

// ── 行取数 ───────────────────────────────────────────────────────
/// 表页取数的 SQL（状态栏也展示这条）。列名来自目录，顺序与 `cols` 一致。
///
/// 无排序时补主键排序：MySQL 不保证没有 ORDER BY 的行序，翻页就会重复 / 漏行。
pub fn select_table_sql(
    cols: &[ColMeta],
    db: &str,
    table: &str,
    where_sql: Option<&str>,
    orders: &[(String, bool)],
    limit: usize,
    offset: usize,
) -> String {
    let list = if cols.is_empty() {
        "*".to_string()
    } else {
        cols.iter()
            .map(|c| quote_ident(&c.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let keys: Vec<(String, bool)> = if orders.is_empty() {
        cols.iter()
            .filter(|c| c.pk)
            .map(|c| (c.name.clone(), true))
            .collect()
    } else {
        orders.to_vec()
    };
    let order_part = if keys.is_empty() {
        String::new()
    } else {
        let cols: Vec<String> = keys
            .iter()
            .map(|(c, asc)| format!("{} {}", quote_ident(c), if *asc { "ASC" } else { "DESC" }))
            .collect();
        format!(" ORDER BY {}", cols.join(", "))
    };
    let where_part = where_sql.map(|w| format!(" WHERE {w}")).unwrap_or_default();
    format!(
        "SELECT {list} FROM {}{where_part}{order_part} LIMIT {} OFFSET {}",
        qualify(db, table),
        cap(limit),
        cap(offset)
    )
}

/// 表页取数：列元数据来自目录（带主键 / 只读标记），行身份按主键列的值拼；
/// 没有主键的表行身份为空向量，改 / 删会被拒。
#[allow(clippy::too_many_arguments)]
pub fn select_table(
    key: &str,
    p: &Params,
    db: &str,
    table: &str,
    where_sql: Option<&str>,
    orders: &[(String, bool)],
    offset: usize,
    limit: usize,
) -> Result<SelectPage, String> {
    let cols = try_table_columns(p, db, table)?;
    let pk: Vec<usize> = cols
        .iter()
        .enumerate()
        .filter(|(_, c)| c.pk)
        .map(|(i, _)| i)
        .collect();
    let sql = select_table_sql(&cols, db, table, where_sql, orders, limit, offset);
    with_conn(key, |conn| take_page(conn, &sql, cols, &pk, usize::MAX))
}

/// 当前筛选条件下的总行数（远程分页的「共 n 条」与末页判断）。
pub fn count_table(
    key: &str,
    db: &str,
    table: &str,
    where_sql: Option<&str>,
) -> Result<usize, String> {
    with_conn(key, |conn| {
        let where_part = where_sql.map(|w| format!(" WHERE {w}")).unwrap_or_default();
        let sql = format!("SELECT COUNT(*) FROM {}{where_part}", qualify(db, table));
        let mut res = conn.query_iter(sql).map_err(|e| err_text(&e))?;
        let mut out = 0usize;
        if let Some(mut set) = res.iter() {
            if let Some(Ok(row)) = set.next() {
                if let Some(v) = row.as_ref(0) {
                    out = value_text(v).trim().parse().unwrap_or(0);
                }
            }
        }
        Ok(out)
    })
}

/// 跑一条 SELECT 装配成一页：值全按服务端给的文本原样收。
/// `columns` 为空时（查询页）用行描述里的列名 / 类型码；
/// 非空时（表页）用目录给的元数据（带主键 / 只读标记）。
fn take_page(
    conn: &mut Conn,
    sql: &str,
    columns: Vec<ColMeta>,
    pk: &[usize],
    max_rows: usize,
) -> Result<SelectPage, String> {
    let mut res = conn.query_iter(sql).map_err(|e| err_text(&e))?;
    let described: Vec<ColMeta> = res
        .columns()
        .as_ref()
        .iter()
        .map(|c| {
            let mut m = ColMeta::new(
                c.name_str().to_string(),
                type_label(c),
                c.column_type().is_numeric_type(),
            );
            m.writable = false;
            m
        })
        .collect();
    let columns = if columns.is_empty() {
        dedup_names(described)
    } else {
        columns
    };
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut keys: Vec<RowKey> = Vec::new();
    let mut truncated = false;
    if let Some(mut set) = res.iter() {
        while let Some(r) = set.next() {
            if rows.len() >= max_rows {
                truncated = true;
                break;
            }
            let row = r.map_err(|e| err_text(&e))?;
            let vals: Vec<String> = (0..columns.len())
                .map(|i| row.as_ref(i).map(value_text).unwrap_or_else(|| NULL_TEXT.to_string()))
                .collect();
            keys.push(pk.iter().filter_map(|i| vals.get(*i)).cloned().collect());
            rows.push(vals);
        }
    }
    Ok(SelectPage {
        columns,
        rows,
        keys,
        truncated,
    })
}

/// 列类型码 → 展示用的类型文案（查询页的列头）。
fn type_label(c: &Column) -> String {
    let t = match c.column_type() {
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => "DECIMAL",
        ColumnType::MYSQL_TYPE_TINY => "TINYINT",
        ColumnType::MYSQL_TYPE_SHORT => "SMALLINT",
        ColumnType::MYSQL_TYPE_INT24 => "MEDIUMINT",
        ColumnType::MYSQL_TYPE_LONG => "INT",
        ColumnType::MYSQL_TYPE_LONGLONG => "BIGINT",
        ColumnType::MYSQL_TYPE_FLOAT => "FLOAT",
        ColumnType::MYSQL_TYPE_DOUBLE => "DOUBLE",
        ColumnType::MYSQL_TYPE_NULL => "NULL",
        ColumnType::MYSQL_TYPE_TIMESTAMP | ColumnType::MYSQL_TYPE_TIMESTAMP2 => "TIMESTAMP",
        ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE => "DATE",
        ColumnType::MYSQL_TYPE_TIME | ColumnType::MYSQL_TYPE_TIME2 => "TIME",
        ColumnType::MYSQL_TYPE_DATETIME | ColumnType::MYSQL_TYPE_DATETIME2 => "DATETIME",
        ColumnType::MYSQL_TYPE_YEAR => "YEAR",
        ColumnType::MYSQL_TYPE_VARCHAR => "VARCHAR",
        ColumnType::MYSQL_TYPE_VAR_STRING => "VARCHAR",
        ColumnType::MYSQL_TYPE_STRING => "CHAR",
        ColumnType::MYSQL_TYPE_BIT => "BIT",
        ColumnType::MYSQL_TYPE_JSON => "JSON",
        ColumnType::MYSQL_TYPE_ENUM => "ENUM",
        ColumnType::MYSQL_TYPE_SET => "SET",
        ColumnType::MYSQL_TYPE_TINY_BLOB => "TINYBLOB",
        ColumnType::MYSQL_TYPE_MEDIUM_BLOB => "MEDIUMBLOB",
        ColumnType::MYSQL_TYPE_LONG_BLOB => "LONGBLOB",
        ColumnType::MYSQL_TYPE_BLOB => "BLOB",
        ColumnType::MYSQL_TYPE_GEOMETRY => "GEOMETRY",
        _ => "VARIABLE",
    };
    t.to_string()
}

/// 顶层出结果的语句粗判（与另两档同一套启发式，方言词换成 MySQL 有的）。
pub fn query_is_select(stmt: &str) -> bool {
    let head = stmt
        .trim_start()
        .trim_start_matches('(')
        .trim_start()
        .to_ascii_lowercase();
    [
        "select", "with", "explain", "values", "table", "show", "desc", "describe",
    ]
    .iter()
    .any(|p| head.starts_with(p))
}

// ── 语句执行 ─────────────────────────────────────────────────────

/// 把连接的当前库切到 `db`（空串不动）。MySQL 一条连接看全部库，登录时的
/// 默认库不会跟着查询页的库下拉走；用户写不带库名的表名时得先 `USE`。
fn use_db(conn: &mut Conn, db: &str) -> Result<(), String> {
    let db = db.trim();
    if db.is_empty() {
        return Ok(());
    }
    conn.query_drop(format!("USE {}", quote_ident(db)))
        .map_err(|e| err_text(&e))
}

/// 执行一条语句（调用方已按分号切好）。SELECT 出结果页，其余出影响行数。
/// `db` 是页签当前选的库：注册连接是跨库共用的，每次执行前先对准。
pub fn execute_stmt(
    key: &str,
    db: &str,
    stmt: &str,
    max_rows: usize,
) -> Result<ExecOutcome, String> {
    with_conn(key, |conn| {
        use_db(conn, db)?;
        execute_on(conn, stmt, max_rows)
    })
}

/// 同 `execute_stmt`，但直接吃连接：后台脚本线程自己开连接，不碰注册表锁。
pub fn execute_on(conn: &mut Conn, stmt: &str, max_rows: usize) -> Result<ExecOutcome, String> {
    let trimmed = stmt.trim();
    if query_is_select(trimmed) {
        let page = take_page(conn, trimmed, Vec::new(), &[], max_rows)?;
        let message = if page.truncated {
            format!("结果超过 {max_rows} 行，仅显示前 {max_rows} 行")
        } else {
            String::new()
        };
        Ok(ExecOutcome {
            select: Some(page),
            affected: 0,
            message,
        })
    } else {
        let res = conn.query_iter(trimmed).map_err(|e| err_text(&e))?;
        let affected = res.affected_rows() as usize;
        Ok(ExecOutcome {
            select: None,
            affected,
            message: format!("Query OK, {affected} rows affected"),
        })
    }
}

/// MCP / 单语句后台执行：自己开一条连接并登记服务端连接号，超时用
/// `KILL QUERY` 从服务端打断；连接用完即弃，不占注册表那条。
#[allow(dead_code)] // 只有 MCP 后台（lib）调用，UI 那份拷贝用不到
pub fn execute_job(
    job: &Job,
    p: &Params,
    db: &str,
    stmt: &str,
    max_rows: usize,
) -> Result<ExecOutcome, String> {
    if job.is_cancelled() {
        return Err("查询已取消".to_string());
    }
    let mut conn = connect(p)?;
    use_db(&mut conn, db)?;
    RUNNING_JOBS
        .lock()
        .unwrap()
        .insert(job.id, conn.connection_id());
    let result = execute_on(&mut conn, stmt, max_rows);
    RUNNING_JOBS.lock().unwrap().remove(&job.id);
    result
}

// ── 后台脚本执行（查询页「运行」）───────────────────────────────

/// 一次后台脚本的取消通道。MySQL 的中断走服务端：`KILL QUERY <id>` 打断
/// 正在跑的语句；另开一条连接发这个请求。
#[derive(Clone)]
pub struct Job {
    id: u64,
    cancel: Arc<AtomicBool>,
    /// 另开连接发 KILL QUERY 时要用同一套连接参数。
    params: Params,
}

impl Job {
    /// 只置标志：不碰服务器，UI 线程直接调用也安全。
    pub fn request_stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// 另开一条连接发 `KILL QUERY`；要连服务器，可能阻塞，放到后台线程调。
    pub fn interrupt(&self) {
        // 先摘出连接号再发请求：cancel 要连服务器，抱着锁等不划算
        let cid = RUNNING_JOBS.lock().unwrap().get(&self.id).copied();
        if let Some(cid) = cid {
            if let Ok(mut c) = connect(&self.params) {
                let _ = c.query_drop(format!("KILL QUERY {cid}"));
            }
        }
    }

    pub fn cancel(&self) {
        self.request_stop();
        self.interrupt();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// 分配一个后台任务号（尚未挂连接）。
pub fn create_job(params: &Params) -> Job {
    let id = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
    Job {
        id,
        cancel: Arc::new(AtomicBool::new(false)),
        params: params.clone(),
    }
}

/// 后台任务号 → 服务端连接号。
static RUNNING_JOBS: LazyLock<Mutex<HashMap<u64, u32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

/// 脚本里的事务开关（按语句首关键字跟）：`Some(true)` 开、`Some(false)` 关。
/// `ROLLBACK TO SAVEPOINT` 不算关——事务还在。
fn tx_toggle(stmt: &str) -> Option<bool> {
    let head = stmt.trim_start().to_ascii_lowercase();
    let mut words = head.split_whitespace();
    let first = words.next().unwrap_or("");
    match first {
        "begin" | "start" => Some(true),
        "commit" | "end" => Some(false),
        // 回滚到保存点：事务还在，既不算开也不算关
        "rollback" if words.next() == Some("to") => None,
        "rollback" => Some(false),
        _ => None,
    }
}

/// 逐条执行脚本，支持「停止」；`continue_on_error=false` 时第一条报错就收手。
///
/// 自己开一条连接，不占用 UI 线程那条。MySQL 报错后事务不会进入「已中止」
/// 态，勾了「遇到错误时继续」后续语句照常能跑；脚本停在 BEGIN 里没提交时
/// 收尾回滚。
pub fn run_script(
    job: &Job,
    p: &Params,
    stmts: &[String],
    max_rows: usize,
    continue_on_error: bool,
) -> ScriptRun {
    let started = Instant::now();
    let mut conn = match connect(p) {
        Ok(c) => c,
        Err(e) => {
            return ScriptRun {
                runs: Vec::new(),
                open_error: Some(e),
                cancelled: false,
                rolled_back: false,
                aborted_tx: false,
                total_ms: started.elapsed().as_millis(),
            };
        }
    };
    RUNNING_JOBS
        .lock()
        .unwrap()
        .insert(job.id, conn.connection_id());
    let mut runs = Vec::with_capacity(stmts.len());
    let mut cancelled = false;
    let mut rolled_back = false;
    let mut tx_open = false;
    for stmt in stmts {
        if job.is_cancelled() {
            cancelled = true;
            break;
        }
        let sw = Instant::now();
        let result = execute_on(&mut conn, stmt, max_rows);
        let err = result.as_ref().err().cloned();
        runs.push(StmtRun {
            elapsed_ms: sw.elapsed().as_millis(),
            result,
        });
        match tx_toggle(stmt) {
            Some(on) if err.is_none() => tx_open = on,
            Some(false) => tx_open = false,
            _ => {}
        }
        if err.is_some() && !continue_on_error {
            break;
        }
    }
    // 脚本停在 BEGIN 里没提交：别把事务挂在连接上
    if tx_open {
        rolled_back = conn.query_drop("ROLLBACK").is_ok();
    }
    RUNNING_JOBS.lock().unwrap().remove(&job.id);
    ScriptRun {
        runs,
        open_error: None,
        cancelled,
        rolled_back,
        aborted_tx: false,
        total_ms: started.elapsed().as_millis(),
    }
}

// ── 网格写回（apply）────────────────────────────────────────────

/// 单元格文本 → 绑定值。约定与另两档同：
/// 字面 `NULL` → NULL；单引号包住 → 去引号的纯文本；数值列空串 → NULL。
pub fn value_for(meta: &ColMeta, v: &str) -> Option<String> {
    if v == NULL_TEXT {
        return None;
    }
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        return Some(v[1..v.len() - 1].replace("''", "'"));
    }
    if meta.numeric && v.trim().is_empty() {
        return None;
    }
    Some(v.to_string())
}

/// 绑定值 → MySQL 参数。
fn bind(meta: &ColMeta, v: &str) -> Value {
    match value_for(meta, v) {
        None => Value::NULL,
        Some(s) => {
            if meta.numeric {
                // 数值列按数值送，别让服务端再猜一次类型
                if let Ok(i) = s.trim().parse::<i64>() {
                    return Value::Int(i);
                }
                if let Ok(f) = s.trim().parse::<f64>() {
                    return Value::Double(f);
                }
            }
            Value::from(s)
        }
    }
}

/// 主键列（顺序即行身份里各段的顺序）。
fn pk_cols(columns: &[ColMeta]) -> Vec<&ColMeta> {
    columns.iter().filter(|c| c.pk).collect()
}

/// 按主键定位的 WHERE 片段（`?` 是位置参数）。
fn pk_where(pk: &[&ColMeta]) -> String {
    pk.iter()
        .map(|c| format!("{} = ?", quote_ident(&c.name)))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// 目标是不是视图 / 物化视图（视图不能直接编辑）。
fn is_view(conn: &mut Conn, db: &str, table: &str) -> Result<bool, String> {
    let sql = format!(
        "SELECT TABLE_TYPE FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {}",
        lit(db),
        lit(table)
    );
    let mut res = conn.query_iter(sql).map_err(|e| err_text(&e))?;
    let mut kind = None;
    if let Some(mut set) = res.iter() {
        if let Some(Ok(row)) = set.next() {
            kind = row.as_ref(0).map(value_text);
        }
    }
    Ok(matches!(kind.as_deref(), Some("VIEW") | Some("SYSTEM VIEW")))
}

/// 把网格工作集写回真库：UPDATE / DELETE 按主键定位，INSERT 只写可写列，
/// 全部包在一个事务里，任何一条失败整体回滚。
///
/// 影响行数为 0 视为「这行已经不是当初那行了」（别的客户端改过 / 删过），
/// 直接报错回滚，而不是默默少写一行。
pub fn apply_changes(key: &str, db: &str, ch: &GridChanges<'_>) -> Result<String, String> {
    with_conn(key, |conn| {
        if is_view(conn, db, ch.table)? {
            return Err(format!(
                "「{}」是视图，不能直接编辑；请修改基表",
                ch.table
            ));
        }
        let pk = pk_cols(ch.columns);
        if (!ch.updates.is_empty() || !ch.deletes.is_empty()) && pk.is_empty() {
            return Err("该表没有主键，不支持编辑和删除写回".to_string());
        }
        let update_where = pk_where(&pk);
        let delete_where = pk_where(&pk);
        let mut tx = conn.start_transaction(TxOpts::default()).map_err(|e| err_text(&e))?;
        let mut touched = 0usize;
        for (key_vals, col, val) in ch.updates {
            let Some(meta) = ch.columns.get(*col) else {
                continue;
            };
            if !meta.writable {
                return Err(format!("列「{}」是生成列 / 自增列，不能写回", meta.name));
            }
            if key_vals.len() != pk.len() {
                return Err("行标识与当前主键不匹配，请刷新页签".to_string());
            }
            let sql = format!(
                "UPDATE {} SET {} = ? WHERE {update_where}",
                qualify(db, ch.table),
                quote_ident(&meta.name)
            );
            let mut args: Vec<Value> = vec![bind(meta, val)];
            args.extend(key_vals.iter().map(|v| Value::from(v.clone())));
            let n = tx
                .exec_iter(sql, MParams::Positional(args))
                .map_err(|e| err_text(&e))?
                .affected_rows() as usize;
            if n == 0 {
                return Err("按主键没找到该行（可能已被其它客户端改动或删除），请刷新".to_string());
            }
            touched += n;
        }
        for key_vals in ch.deletes {
            if key_vals.len() != pk.len() {
                return Err("行标识与当前主键不匹配，请刷新页签".to_string());
            }
            let sql = format!("DELETE FROM {} WHERE {delete_where}", qualify(db, ch.table));
            let args: Vec<Value> = key_vals.iter().map(|v| Value::from(v.clone())).collect();
            let n = tx
                .exec_iter(sql, MParams::Positional(args))
                .map_err(|e| err_text(&e))?
                .affected_rows() as usize;
            if n == 0 {
                return Err("按主键没找到该行（可能已被其它客户端删除），请刷新".to_string());
            }
            touched += n;
        }
        if !ch.inserts.is_empty() {
            // 只写可写列：生成列 / 自增列由服务端算，写它反而报错
            let idx: Vec<usize> = ch
                .columns
                .iter()
                .enumerate()
                .filter(|(_, c)| c.writable)
                .map(|(i, _)| i)
                .collect();
            let names: Vec<String> = idx
                .iter()
                .filter_map(|i| ch.columns.get(*i))
                .map(|c| quote_ident(&c.name))
                .collect();
            for row in ch.inserts {
                let args: Vec<Value> = idx
                    .iter()
                    .filter_map(|i| ch.columns.get(*i).map(|c| (*i, c)))
                    .map(|(i, c)| bind(c, row.get(i).map(String::as_str).unwrap_or("")))
                    .collect();
                let sql = if names.is_empty() {
                    format!("INSERT INTO {} () VALUES ()", qualify(db, ch.table))
                } else {
                    format!(
                        "INSERT INTO {} ({}) VALUES ({})",
                        qualify(db, ch.table),
                        names.join(", "),
                        names.iter().map(|_| "?").collect::<Vec<_>>().join(", ")
                    )
                };
                tx.exec_iter(sql, MParams::Positional(args))
                    .map_err(|e| err_text(&e))?;
                touched += 1;
            }
        }
        tx.commit().map_err(|e| err_text(&e))?;
        Ok(format!("已写回 {touched} 行"))
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn p() -> Params {
        Params {
            host: "127.0.0.1".into(),
            port: 3306,
            user: "root".into(),
            password: String::new(),
            dbname: String::new(),
        }
    }

    #[test]
    fn identifiers_use_backticks_and_double_the_inner_one() {
        assert_eq!(quote_ident("user"), "`user`");
        assert_eq!(quote_ident("a`b"), "`a``b`");
        assert_eq!(qualify("bx11", "bx_cart"), "`bx11`.`bx_cart`");
        // 库名带引号时也只影响标识符，不影响 SQL 结构
        assert_eq!(qualify("d`b", "t"), "`d``b`.`t`");
    }

    #[test]
    fn literals_escape_quote_and_backslash() {
        assert_eq!(lit("it's"), "'it''s'");
        // MySQL 默认把反斜杠当转义符，不双写就会被吃掉
        assert_eq!(lit("a\\b"), "'a\\\\b'");
    }

    #[test]
    fn numeric_types_are_recognised() {
        for ty in ["int", "bigint", "tinyint", "decimal", "double", "numeric", "float"] {
            assert!(ty_is_numeric(ty), "{ty} 应判为数值列");
        }
        for ty in ["varchar", "text", "date", "datetime", "json", "blob"] {
            assert!(!ty_is_numeric(ty), "{ty} 不该判为数值列");
        }
    }

    #[test]
    fn values_render_as_grid_text() {
        assert_eq!(value_text(&Value::NULL), "NULL");
        assert_eq!(value_text(&Value::Bytes(b"hi".to_vec())), "hi");
        assert_eq!(value_text(&Value::Int(-3)), "-3");
        assert_eq!(value_text(&Value::UInt(9)), "9");
        assert_eq!(value_text(&Value::Double(1.5)), "1.5");
        // 非 UTF-8 的字节打二进制占位，不吞掉
        assert_eq!(value_text(&Value::Bytes(vec![0xff, 0x00])), "<BLOB 2 字节>");
        // 日期时间：没有时分秒时只给日期
        assert_eq!(value_text(&Value::Date(2026, 3, 4, 0, 0, 0, 0)), "2026-03-04");
        assert_eq!(
            value_text(&Value::Date(2026, 3, 4, 11, 22, 33, 0)),
            "2026-03-04 11:22:33"
        );
        // TIME 的天数折进小时
        assert_eq!(value_text(&Value::Time(false, 1, 2, 3, 4, 0)), "26:03:04");
    }

    #[test]
    fn cell_text_maps_to_null_quoted_and_blank() {
        let num = ColMeta::new("score", "decimal(6,2)", true);
        let txt = ColMeta::new("name", "varchar(64)", false);
        assert_eq!(value_for(&num, "NULL"), None);
        assert_eq!(value_for(&num, ""), None); // 数值列空串 = NULL
        assert_eq!(value_for(&num, "60.5").as_deref(), Some("60.5"));
        assert_eq!(value_for(&txt, "").as_deref(), Some("")); // 文本列空串保留
        assert_eq!(value_for(&txt, "'NULL'").as_deref(), Some("NULL"));
        assert_eq!(value_for(&txt, "'O''B'").as_deref(), Some("O'B"));
    }

    #[test]
    fn numeric_cells_bind_as_numbers_not_text() {
        let num = ColMeta::new("n", "int", true);
        let txt = ColMeta::new("s", "varchar(10)", false);
        assert!(matches!(bind(&num, "42"), Value::Int(42)));
        assert!(matches!(bind(&num, "1.5"), Value::Double(_)));
        assert!(matches!(bind(&txt, "42"), Value::Bytes(_)));
        assert!(matches!(bind(&num, "NULL"), Value::NULL));
    }

    #[test]
    fn table_sql_pushes_where_order_and_pages_by_pk_when_unsorted() {
        let cols = vec![
            {
                let mut c = ColMeta::new("id", "bigint", true);
                c.pk = true;
                c
            },
            ColMeta::new("name", "varchar(64)", false),
        ];
        let sql = select_table_sql(&cols, "bx11", "member", None, &[], 20, 10);
        assert_eq!(
            sql,
            "SELECT `id`, `name` FROM `bx11`.`member` ORDER BY `id` ASC LIMIT 20 OFFSET 10"
        );
        let sorted = select_table_sql(
            &cols,
            "bx11",
            "member",
            Some("`score` > 80"),
            &[("score".to_string(), false)],
            100,
            0,
        );
        assert!(sorted.contains("WHERE `score` > 80"), "{sorted}");
        assert!(sorted.contains("ORDER BY `score` DESC"), "{sorted}");
        assert!(!sorted.contains("`id` ASC"), "显式排序优先：{sorted}");
    }

    #[test]
    fn column_meta_flags_generated_autoincrement_and_pk() {
        let rows = vec![
            vec![
                "id".into(),
                "bigint".into(),
                "bigint".into(),
                "auto_increment".into(),
                "PRI".into(),
            ],
            vec![
                "qty".into(),
                "int".into(),
                "int".into(),
                String::new(),
                String::new(),
            ],
            vec![
                "total".into(),
                "decimal(10,2)".into(),
                "decimal".into(),
                "STORED GENERATED".into(),
                String::new(),
            ],
        ];
        let cols = cols_from_rows(rows);
        assert_eq!(cols.len(), 3);
        assert!(cols[0].pk);
        assert!(!cols[0].writable, "自增列不可写");
        assert!(cols[1].writable);
        assert!(!cols[1].pk);
        assert!(cols[2].numeric);
        assert!(!cols[2].writable, "生成列不可写");
    }

    #[test]
    fn select_detection_covers_mysql_result_statements() {
        assert!(query_is_select("SELECT 1"));
        assert!(query_is_select("  with x as (select 1) select * from x"));
        assert!(query_is_select("SHOW TABLES"));
        assert!(query_is_select("DESC t"));
        assert!(!query_is_select("INSERT INTO t VALUES (1)"));
        assert!(!query_is_select("UPDATE t SET a = 1"));
        assert!(!query_is_select("CREATE TABLE t (a int)"));
    }

    #[test]
    fn tx_toggle_follows_statement_keywords() {
        assert_eq!(tx_toggle("BEGIN"), Some(true));
        assert_eq!(tx_toggle("start transaction"), Some(true));
        assert_eq!(tx_toggle("COMMIT"), Some(false));
        assert_eq!(tx_toggle("ROLLBACK"), Some(false));
        assert_eq!(tx_toggle("ROLLBACK TO SAVEPOINT s"), None);
        assert_eq!(tx_toggle("SELECT 1"), None);
    }

    #[test]
    fn job_cancel_sets_flag_even_without_a_running_connection() {
        let job = create_job(&p());
        assert!(!job.is_cancelled());
        job.cancel();
        assert!(job.is_cancelled());
    }
}

/// 真机联调用：需要一台可达的 MySQL。连接参数走环境变量，
/// 默认打本机 3306 的 root（无密码）。
#[cfg(test)]
mod live_tests {
    use super::*;

    const TEST_DB: &str = "daro_mysql_test";

    /// 环境变量 → 连接参数。
    fn env_params() -> Params {
        Params {
            host: std::env::var("DARO_MYSQL_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: std::env::var("DARO_MYSQL_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3306),
            user: std::env::var("DARO_MYSQL_USER").unwrap_or_else(|_| "root".to_string()),
            password: std::env::var("DARO_MYSQL_PASSWORD").unwrap_or_default(),
            dbname: String::new(),
        }
    }

    /// 收尾把测试库删掉：留下的库会让下次跑撞上「库已存在」。
    struct Guard {
        p: Params,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let mut p = self.p.clone();
            p.dbname = String::new();
            if let Ok(mut c) = catalog_connect(&p) {
                let _ = c.query_drop(format!("DROP DATABASE IF EXISTS `{TEST_DB}`"));
            }
        }
    }

    /// 建测试库 + 表，返回参数与收尾句柄。
    fn setup() -> (Guard, Params) {
        let mut p = env_params();
        p.dbname = String::new();
        let mut c = catalog_connect(&p).expect("连 MySQL 失败");
        c.query_drop(format!("DROP DATABASE IF EXISTS `{TEST_DB}`"))
            .expect("清旧测试库");
        c.query_drop(format!("CREATE DATABASE `{TEST_DB}`")).expect("建测试库");

        let mut p2 = env_params();
        p2.dbname = TEST_DB.to_string();
        let mut c2 = catalog_connect(&p2).expect("连测试库失败");
        c2.query_drop(
            "CREATE TABLE member (
                 id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
                 name VARCHAR(64),
                 score DECIMAL(6,2),
                 joined DATETIME,
                 note TEXT
             ) ENGINE=InnoDB",
        )
        .expect("建表");
        c2.query_drop(
            "INSERT INTO member (name, score, joined, note) VALUES
                 ('阿离', 91.50, '2026-03-04 11:22:33', 'utf8 中文 ok'),
                 ('老张', 60.25, '2026-03-05 00:00:00', NULL),
                 ('带引号', 12.00, '2026-03-06 08:00:00', '带''引号')",
        )
        .expect("灌数据");
        c2.query_drop("CREATE VIEW v_member AS SELECT id, name FROM member")
            .expect("建视图");
        (Guard { p: p.clone() }, p2)
    }

    #[test]
    #[ignore = "需要一台可达的 MySQL"]
    fn mysql_catalog_reads_databases_tables_views_and_columns() {
        let (_g, p) = setup();
        let dbs = try_list_databases(&p).unwrap();
        assert!(dbs.iter().any(|d| d == TEST_DB), "测试库要在清单里：{dbs:?}");

        assert_eq!(try_list_tables(&p, TEST_DB).unwrap(), vec!["member"]);
        assert_eq!(try_list_views(&p, TEST_DB).unwrap(), vec!["v_member"]);

        let cols = try_table_columns(&p, TEST_DB, "member").unwrap();
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["id", "name", "score", "joined", "note"]);
        assert!(cols[0].pk, "id 是主键");
        assert!(!cols[0].writable, "自增列不能写");
        assert!(cols[0].numeric, "bigint 右对齐");
        assert!(cols[2].numeric, "decimal 右对齐");
        assert!(!cols[1].numeric && !cols[3].numeric);
        assert!(cols[1].writable && cols[4].writable);

        let by_table = all_columns_by_table(&p, TEST_DB);
        let all: Vec<&String> = by_table.iter().flat_map(|(_, cols)| cols.iter()).collect();
        assert!(all.iter().any(|c| *c == "name") && all.iter().any(|c| *c == "note"));
    }

    #[test]
    #[ignore = "需要一台可达的 MySQL"]
    fn mysql_table_page_returns_text_values_keys_and_pushdown() {
        let (_g, p) = setup();
        let key = "真机测试";
        open(key, &p).expect("建连");

        let page = select_table(key, &p, TEST_DB, "member", None, &[], 0, 100).unwrap();
        assert_eq!(page.rows.len(), 3);
        // 文本协议：decimal 保持服务端给的两位小数，NULL 是字面量
        assert_eq!(page.rows[0][1], "阿离");
        assert_eq!(page.rows[0][2], "91.50");
        assert_eq!(page.rows[1][3], "NULL");
        assert_eq!(page.rows[0][3], "2026-03-04 11:22:33");
        assert_eq!(page.rows[0][4], "utf8 中文 ok");
        assert_eq!(page.rows[2][4], "带'引号");
        // 行身份 = 主键列值
        assert!(page.keys.iter().all(|k| k.len() == 1 && !k[0].is_empty()));

        // 无排序时按主键排，翻页顺序稳定
        assert_eq!(page.rows[0][1], "阿离", "按 id 升序");

        // WHERE / ORDER BY 下推
        let filtered = select_table(
            key,
            &p,
            TEST_DB,
            "member",
            Some("`score` > 50"),
            &[("score".to_string(), false)],
            0,
            100,
        )
        .unwrap();
        assert_eq!(filtered.rows.len(), 2);
        assert_eq!(filtered.rows[0][1], "老张", "降序");

        // 总行数
        assert_eq!(count_table(key, TEST_DB, "member", None).unwrap(), 3);
        assert_eq!(
            count_table(key, TEST_DB, "member", Some("`score` > 50")).unwrap(),
            2
        );
        close(key);
    }

    #[test]
    #[ignore = "需要一台可达的 MySQL"]
    fn mysql_apply_changes_updates_inserts_and_deletes() {
        let (_g, p) = setup();
        let key = "真机测试";
        open(key, &p).expect("建连");
        let cols = try_table_columns(&p, TEST_DB, "member").unwrap();

        let page = select_table(key, &p, TEST_DB, "member", None, &[], 0, 100).unwrap();
        let target = page.keys[0].clone();

        // 改值
        let ch = GridChanges {
            table: "member",
            columns: &cols,
            updates: &[(target.clone(), 1, "阿离改".to_string())],
            deletes: &[],
            inserts: &[],
        };
        let msg = apply_changes(key, TEST_DB, &ch).unwrap();
        assert!(msg.contains("1"), "{msg}");
        let after = select_table(key, &p, TEST_DB, "member", None, &[], 0, 100).unwrap();
        assert_eq!(after.rows[0][1], "阿离改");

        // 插一行（自增列不写，由服务端发号）
        let new_row = vec![
            "新行".to_string(),
            "5.50".to_string(),
            "2026-04-01 00:00:00".to_string(),
            "备注".to_string(),
        ];
        let ch = GridChanges {
            table: "member",
            columns: &cols,
            updates: &[],
            deletes: &[],
            inserts: &[new_row],
        };
        apply_changes(key, TEST_DB, &ch).unwrap();
        let after = select_table(key, &p, TEST_DB, "member", None, &[], 0, 100).unwrap();
        assert_eq!(after.rows.len(), 4);
        assert_eq!(after.rows[3][1], "新行");

        // 删一行
        let ch = GridChanges {
            table: "member",
            columns: &cols,
            updates: &[],
            deletes: &[after.keys[3].clone()],
            inserts: &[],
        };
        apply_changes(key, TEST_DB, &ch).unwrap();
        let after = select_table(key, &p, TEST_DB, "member", None, &[], 0, 100).unwrap();
        assert_eq!(after.rows.len(), 3);

        // 视图不能直接编辑
        let view_cols = try_table_columns(&p, TEST_DB, "v_member").unwrap();
        let ch = GridChanges {
            table: "v_member",
            columns: &view_cols,
            updates: &[],
            deletes: &[],
            inserts: &[],
        };
        let err = apply_changes(key, TEST_DB, &ch).unwrap_err();
        assert!(err.contains("视图"), "{err}");
        close(key);
    }

    #[test]
    #[ignore = "需要一台可达的 MySQL"]
    fn mysql_execute_stmt_covers_select_counts_and_errors() {
        let (_g, p) = setup();
        let key = "真机测试";
        // 连接表单没填默认库、库由查询页下拉选：注册连接上必须先 USE，
        // 否则不带库名的表名会撞 ERROR 1046。
        let mut p_nodb = p.clone();
        p_nodb.dbname = String::new();
        open(key, &p_nodb).expect("建连");

        let out = execute_stmt(
            key,
            TEST_DB,
            "SELECT id, name, score FROM member ORDER BY id",
            100,
        )
        .unwrap();
        let sel = out.select.expect("SELECT 要出结果页");
        assert_eq!(sel.rows.len(), 3);
        assert_eq!(sel.columns[0].name, "id");
        assert!(sel.columns[0].numeric, "bigint 右对齐");
        assert!(!sel.columns[1].numeric);
        assert!(!sel.columns[0].writable, "查询页结果不支撑写回");

        let out = execute_stmt(key, TEST_DB, "SELECT * FROM member", 2).unwrap();
        let sel = out.select.unwrap();
        assert_eq!(sel.rows.len(), 2);
        assert!(sel.truncated);
        assert!(out.message.contains("仅显示前 2 行"), "{}", out.message);

        let out = execute_stmt(
            key,
            TEST_DB,
            "UPDATE member SET note = 'x' WHERE score IS NULL",
            100,
        )
        .unwrap();
        assert_eq!(out.affected, 1);
        assert!(out.message.contains("1 rows affected"));

        // 报错形态：服务端消息 + 错误码
        let err = execute_stmt(key, TEST_DB, "SELECT * FROM nope", 100).unwrap_err();
        assert!(err.contains("nope"), "{err}");

        // SHOW / DESC 也是出结果的语句
        let out = execute_stmt(key, TEST_DB, "SHOW TABLES", 100).unwrap();
        assert!(out.select.is_some(), "SHOW 要出结果页");
        close(key);
    }

    #[test]
    #[ignore = "需要一台可达的 MySQL"]
    fn mysql_script_continues_past_errors_and_rolls_back_open_tx() {
        let (_g, p) = setup();
        let run = run_script(
            &create_job(&p),
            &p,
            &[
                "SELECT 1".to_string(),
                "SELECT * FROM nope".to_string(),
                "SELECT 2".to_string(),
            ],
            QUERY_ROW_LIMIT,
            true,
        );
        assert!(run.open_error.is_none(), "{:?}", run.open_error);
        assert_eq!(run.runs.len(), 3, "事务外的报错不拦后面的语句");
        assert!(run.runs[1].result.is_err());
        assert!(!run.cancelled);
        // MySQL 报错后事务不会进入「已中止」态，后续语句照常能跑
        assert!(!run.aborted_tx);

        // 脚本停在 BEGIN 里没提交：收尾回滚，不把事务挂在连接上
        let run = run_script(
            &create_job(&p),
            &p,
            &[
                "BEGIN".to_string(),
                "INSERT INTO member (name) VALUES ('进事务')".to_string(),
            ],
            QUERY_ROW_LIMIT,
            false,
        );
        assert!(run.rolled_back, "没收尾的事务被回滚");
    }
}

