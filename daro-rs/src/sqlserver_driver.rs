//! SQL Server 驱动：纯 Rust 的 TDS（tiberius），不依赖系统 ODBC 驱动。
//!
//! 方言口径：
//! - 标识符 `[name]`，内部 `]` 双写；字面量 `N'...'`，单引号双写
//! - 表名三段 `[库].[模式].[表]`；模式缺省 `dbo`
//! - 分页 `OFFSET n ROWS FETCH NEXT m ROWS ONLY`，没有 ORDER BY 时补
//!   `ORDER BY (SELECT NULL)`，否则 SQL Server 直接报错
//! - 认证：填了用户名走 SQL 认证，留空在 Windows 上走集成认证；macOS/Linux
//!   没有 Windows 身份，留空会直接报错，提示填账号
//! - 主机：命名实例写 `主机\实例名`（如 `localhost\SQLEXPRESS`），实例端口不
//!   固定，先问 SQL Browser（UDP 1434）；`主机,端口` 可以钉死端口跳过 Browser
//!
//! 驱动对上层是同步接口：内部统一在这个模块的运行时上跑 tiberius 的 async
//! 调用。调用点不能位于 tokio 运行时线程里（MCP 后台已用 spawn_blocking 隔开）。

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, LazyLock, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{Duration as ChronoDuration, NaiveDate};
use futures_util::TryStreamExt;
use tiberius::time::{
    Date as TibDate, DateTime as TibDateTime, DateTime2 as TibDateTime2,
    DateTimeOffset as TibDateTimeOffset, SmallDateTime, Time as TibTime,
};
use tiberius::{AuthMethod, Client, Column, ColumnData, ColumnType, Config, EncryptionLevel, QueryItem, Row, SqlBrowser};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::db_types::{ColMeta, ExecOutcome, GridChanges, ScriptRun, SelectPage, StmtRun};

/// 查询页一次取回的最大行数。
pub const QUERY_ROW_LIMIT: usize = 5000;
const NULL_TEXT: &str = "NULL";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Params {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub dbname: String,
}

/// 「主机」拆出来的连接目标。
#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    /// 命名实例名（`localhost\SQLEXPRESS` 里的 `SQLEXPRESS`），端口不固定，
    /// 要向 SQL Browser 问。
    pub instance: Option<String>,
    /// 写在主机里的端口（`host,1433` / `host:1433`），比表单端口优先。
    pub port: Option<u16>,
}

/// SQL Server 的主机写法比其它引擎多：命名实例的端口是装实例时随机分的，
/// 所以 `主机\实例` 才是日常写法，端口也可以直接钉在主机串里。
///
/// 认这几种：`host`、`host\实例`、`host,端口`、`host\实例,端口`、`host:端口`。
/// 端口和实例同时出现时端口赢、实例名忽略，与 SqlClient 一致。
pub fn split_host(raw: &str) -> Target {
    let raw = raw.trim();
    // IPv6 的字面量带冒号，只在「一个冒号且后面全是数字」时才当端口切
    let sep = raw
        .rsplit_once(',')
        .filter(|(_, p)| p.trim().parse::<u16>().is_ok())
        .or_else(|| {
            raw.split_once(':')
                .filter(|(h, p)| !h.is_empty() && !h.contains(':') && p.parse::<u16>().is_ok())
        });
    let (head, port) = match sep {
        Some((head, p)) => (head, p.trim().parse::<u16>().ok()),
        None => (raw, None),
    };
    let (host, instance) = match head.split_once('\\') {
        Some((h, i)) if !i.trim().is_empty() => (h.trim(), Some(i.trim().to_string())),
        _ => (head.trim(), None),
    };
    Target {
        host: host.to_string(),
        instance,
        port,
    }
}

type Conn = Client<Compat<TcpStream>>;

/// 所有连接共用的小运行时：`block_on` 只阻塞调用线程，网络读写由工作线程驱动。
static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .thread_name("daro-sqlserver")
        .build()
        .expect("初始化 SQL Server 运行时失败")
});

fn block_on<F: Future>(f: F) -> F::Output {
    RT.block_on(f)
}

static CONNS: LazyLock<Mutex<HashMap<String, Arc<Mutex<Conn>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn auth_method(p: &Params) -> Result<AuthMethod, String> {
    let user = p.user.trim();
    if !user.is_empty() {
        return Ok(AuthMethod::sql_server(user, &p.password));
    }
    #[cfg(windows)]
    {
        Ok(AuthMethod::Integrated)
    }
    #[cfg(not(windows))]
    {
        Err("Windows 集成认证只在 Windows 上可用；请填写 SQL Server 用户名和密码".to_string())
    }
}

fn connect(p: &Params) -> Result<Conn, String> {
    let target = split_host(&p.host);
    if target.host.is_empty() {
        return Err("SQL Server 主机不能为空".to_string());
    }
    // 端口优先级：写在主机里的端口 → 表单端口 → 1433。走 Browser 时端口由
    // 实例自己报，这里的值用不上。
    let port = target
        .port
        .unwrap_or(if p.port == 0 { 1433 } else { p.port });
    let database = if p.dbname.trim().is_empty() {
        "master"
    } else {
        p.dbname.trim()
    };
    let auth = auth_method(p)?;
    block_on(async move {
        // 实例名和端口都从主机串里来：给了端口就直连，没给才问 Browser
        let instance = target.instance.filter(|_| target.port.is_none());
        let browser = instance.is_some();
        let mut cfg = Config::new();
        cfg.host(&target.host);
        if let Some(inst) = &instance {
            // 不设端口：tiberius 会拿 1434 去问 Browser，再用报回来的端口连
            cfg.instance_name(inst);
        } else {
            cfg.port(port);
        }
        cfg.database(database);
        cfg.application_name("daro");
        // 与原先连接串的 Encrypt=No;TrustServerCertificate=Yes 一致：
        // 只在登录阶段加密，证书不校验（内网实例多是自签名）。
        cfg.encryption(EncryptionLevel::Off);
        cfg.trust_cert();
        cfg.authentication(auth);
        let tcp = if browser {
            let inst = instance.clone().unwrap_or_default();
            TcpStream::connect_named(&cfg).await.map_err(|e| {
                format!(
                    "找不到命名实例 {}\\{inst}：{e}（确认 SQL Server Browser 服务已启动）",
                    target.host
                )
            })?
        } else {
            TcpStream::connect((target.host.as_str(), port))
                .await
                .map_err(|e| format!("连不上 {}:{port}：{e}", target.host))?
        };
        let _ = tcp.set_nodelay(true);
        Client::connect(cfg, tcp.compat_write())
            .await
            .map_err(|e| format!("SQL Server 登录失败：{e}"))
    })
}

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

pub fn ensure(key: &str, p: &Params) -> Result<(), String> {
    if CONNS.lock().unwrap().contains_key(key) {
        return Ok(());
    }
    open(key, p)
}

pub fn close(key: &str) {
    CONNS.lock().unwrap().remove(key);
}

pub fn close_prefix(prefix: &str) {
    let mut map = CONNS.lock().unwrap();
    map.retain(|k, _| !k.starts_with(prefix));
}

pub fn probe(p: &Params) -> Result<(), String> {
    let mut conn = connect(p)?;
    let _ = query_rows(&mut conn, "SELECT 1", 1)?;
    Ok(())
}

fn with_conn<T>(key: &str, p: &Params, f: impl FnOnce(&mut Conn) -> Result<T, String>) -> Result<T, String> {
    ensure(key, p)?;
    with_open_conn(key, f)
}

/// 只按缓存键执行：连接必须先 open / ensure 过（与其它驱动的 `execute_stmt` 同口径）。
fn with_open_conn<T>(key: &str, f: impl FnOnce(&mut Conn) -> Result<T, String>) -> Result<T, String> {
    let c = CONNS.lock().unwrap().get(key).cloned();
    let Some(c) = c else {
        return Err("SQL Server 连接已关闭".to_string());
    };
    let mut guard = c.lock().unwrap();
    match f(&mut guard) {
        Ok(v) => Ok(v),
        Err(e) => {
            if is_conn_broken(&e) {
                CONNS.lock().unwrap().remove(key);
            }
            Err(e)
        }
    }
}

/// 连接已被服务端/网络打断：下次 ensure 会重连，别继续用坏连接。
fn is_conn_broken(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("communication")
        || lower.contains("connection is broken")
        || lower.contains("connection reset")
        || lower.contains("connection closed")
        || lower.contains("broken pipe")
        || lower.contains("unexpected eof")
        || lower.contains("08s01")
        || lower.contains("not connected")
}

/// 服务端报错只把消息给用户看；`Token error: '...' on server ...` 那层包装
/// 对用户没意义。
fn db_err(e: tiberius::error::Error) -> String {
    match e {
        tiberius::error::Error::Server(err) => err.message().to_string(),
        other => other.to_string(),
    }
}

pub fn quote_ident(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

pub fn qualify(db: &str, schema: &str, table: &str) -> String {
    let schema = if schema.trim().is_empty() {
        "dbo"
    } else {
        schema.trim()
    };
    format!(
        "{}.{}.{}",
        quote_ident(db),
        quote_ident(schema),
        quote_ident(table)
    )
}

fn lit(s: &str) -> String {
    format!("N'{}'", s.replace('\'', "''"))
}

fn num_is_numeric(ty: &str) -> bool {
    matches!(
        ty.to_ascii_lowercase().as_str(),
        "bigint"
            | "int"
            | "smallint"
            | "tinyint"
            | "bit"
            | "decimal"
            | "numeric"
            | "money"
            | "smallmoney"
            | "float"
            | "real"
    )
}

// ── 目录查询 ─────────────────────────────────────────────────────

/// 跑一条查询：返回列元数据（来自结果集）与文本行；最多 `max_rows` 行。
fn query_rows(
    conn: &mut Conn,
    sql: &str,
    max_rows: usize,
) -> Result<(Vec<ColMeta>, Vec<Vec<String>>), String> {
    let page = take_page(conn, sql, Vec::new(), &[], max_rows)?;
    Ok((page.columns, page.rows))
}

/// 列名去重（查询页同名列不能把网格搞乱）。
fn dedup_names(mut cols: Vec<ColMeta>) -> Vec<ColMeta> {
    let mut seen = HashMap::<String, usize>::new();
    for c in cols.iter_mut() {
        let n = seen.entry(c.name.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            c.name = format!("{}_{}", c.name, *n);
        }
    }
    cols
}

fn describe_columns(cols: &[Column]) -> Vec<ColMeta> {
    let mut out = Vec::with_capacity(cols.len());
    for c in cols {
        let ty = type_label(c.column_type());
        let mut m = ColMeta::new(c.name().to_string(), ty.clone(), num_is_numeric(&ty));
        m.writable = false;
        out.push(m);
    }
    dedup_names(out)
}

fn type_label(t: ColumnType) -> String {
    match t {
        ColumnType::Bit | ColumnType::Bitn => "BIT",
        ColumnType::Int1 => "TINYINT",
        ColumnType::Int2 => "SMALLINT",
        ColumnType::Int4 | ColumnType::Intn => "INT",
        ColumnType::Int8 => "BIGINT",
        ColumnType::Money | ColumnType::Money4 => "MONEY",
        ColumnType::Float4 => "REAL",
        ColumnType::Float8 | ColumnType::Floatn => "FLOAT",
        ColumnType::Decimaln | ColumnType::Numericn => "DECIMAL",
        ColumnType::Datetime | ColumnType::Datetime4 | ColumnType::Datetimen => "DATETIME",
        ColumnType::Datetime2 => "DATETIME2",
        ColumnType::Daten => "DATE",
        ColumnType::Timen => "TIME",
        ColumnType::DatetimeOffsetn => "DATETIMEOFFSET",
        ColumnType::BigVarBin | ColumnType::BigBinary | ColumnType::Image => "VARBINARY",
        ColumnType::BigVarChar | ColumnType::BigChar => "VARCHAR",
        ColumnType::NVarchar | ColumnType::NChar => "NVARCHAR",
        ColumnType::Text => "TEXT",
        ColumnType::NText => "NTEXT",
        ColumnType::Xml => "XML",
        ColumnType::Guid => "UNIQUEIDENTIFIER",
        ColumnType::Null | ColumnType::Udt | ColumnType::SSVariant => "SQL_VARIANT",
    }
    .to_string()
}

/// 一格 → 与原来 ODBC 文本结果一致的字符串。
fn cell_text(cell: ColumnData<'_>) -> String {
    match cell {
        ColumnData::U8(v) => null_text(v),
        ColumnData::I16(v) => null_text(v),
        ColumnData::I32(v) => null_text(v),
        ColumnData::I64(v) => null_text(v),
        ColumnData::F32(v) => null_text(v),
        ColumnData::F64(v) => null_text(v),
        ColumnData::Bit(Some(true)) => "1".to_string(),
        ColumnData::Bit(Some(false)) => "0".to_string(),
        ColumnData::Bit(None) => null_s(),
        ColumnData::String(v) => match v {
            Some(s) => s.into_owned(),
            None => null_s(),
        },
        ColumnData::Guid(v) => match v {
            Some(g) => g.to_string().to_ascii_uppercase(),
            None => null_s(),
        },
        ColumnData::Binary(v) => match v {
            Some(b) => hex_text(&b),
            None => null_s(),
        },
        ColumnData::Numeric(v) => null_text(v),
        ColumnData::Xml(v) => match v {
            Some(x) => x.as_ref().to_string(),
            None => null_s(),
        },
        ColumnData::DateTime(v) => v.map(fmt_datetime).unwrap_or_else(null_s),
        ColumnData::SmallDateTime(v) => v.map(fmt_smalldatetime).unwrap_or_else(null_s),
        ColumnData::Time(v) => v.map(fmt_time).unwrap_or_else(null_s),
        ColumnData::Date(v) => v.map(fmt_date).unwrap_or_else(null_s),
        ColumnData::DateTime2(v) => v.map(fmt_datetime2).unwrap_or_else(null_s),
        ColumnData::DateTimeOffset(v) => v.map(fmt_datetimeoffset).unwrap_or_else(null_s),
    }
}

fn null_s() -> String {
    NULL_TEXT.to_string()
}

fn null_text<T: ToString>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(null_s)
}

fn hex_text(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    s.push_str("0x");
    for b in bytes {
        let _ = write!(s, "{b:02X}");
    }
    s
}

/// SQL Server 的日期起点：date 从 0001-01-01 起，datetime/smalldatetime 从
/// 1900-01-01 起（按天计）。
fn civil_date(epoch_year: i32, days: i64) -> String {
    let base = NaiveDate::from_ymd_opt(epoch_year, 1, 1).unwrap_or_default();
    base.checked_add_signed(ChronoDuration::days(days))
        .unwrap_or_default()
        .format("%Y-%m-%d")
        .to_string()
}

fn fmt_hms(nanos: u64, scale: usize) -> String {
    let secs = nanos / 1_000_000_000;
    let h = secs / 3600;
    let m = secs % 3600 / 60;
    let s = secs % 60;
    if scale == 0 {
        return format!("{h:02}:{m:02}:{s:02}");
    }
    let frac = (nanos % 1_000_000_000) / 10u64.pow(9 - scale as u32);
    format!("{h:02}:{m:02}:{s:02}.{frac:0w$}", w = scale)
}

fn fmt_datetime(v: TibDateTime) -> String {
    let days = v.days() as i64;
    // 1/300 秒一格，SQL Server 展示到毫秒。
    let nanos = v.seconds_fragments() as u64 * 1_000_000_000 / 300;
    format!("{} {}", civil_date(1900, days), fmt_hms(nanos, 3))
}

fn fmt_smalldatetime(v: SmallDateTime) -> String {
    let days = v.days() as i64;
    let nanos = v.seconds_fragments() as u64 * 60 * 1_000_000_000;
    format!("{} {}", civil_date(1900, days), fmt_hms(nanos, 0))
}

fn fmt_date(v: TibDate) -> String {
    civil_date(1, v.days() as i64)
}

fn fmt_time(v: TibTime) -> String {
    let scale = (v.scale() as usize).min(7);
    let nanos = v.increments() * 10u64.pow(9 - scale as u32);
    fmt_hms(nanos, scale)
}

fn fmt_datetime2(v: TibDateTime2) -> String {
    format!("{} {}", fmt_date(v.date()), fmt_time(v.time()))
}

fn fmt_datetimeoffset(v: TibDateTimeOffset) -> String {
    let (sign, mins) = if v.offset() < 0 {
        ('-', v.offset().unsigned_abs())
    } else {
        ('+', v.offset() as u16)
    };
    let dt2 = fmt_datetime2(v.datetime2());
    format!("{dt2}{sign}{:02}:{:02}", mins / 60, mins % 60)
}

pub fn try_list_databases(key: &str, p: &Params) -> Result<Vec<String>, String> {
    let sql = "SELECT name FROM sys.databases ORDER BY name";
    with_conn(key, p, |c| {
        let (_, rows) = query_rows(c, sql, 10_000)?;
        Ok(rows.into_iter().filter_map(|r| r.into_iter().next()).collect())
    })
}

/// 系统视图挂到参数指定的库：连接按连接名缓存，缓存的 DATABASE 未必是
/// 调用方当前展开的库。
fn db_prefix(db: &str) -> String {
    let db = db.trim();
    if db.is_empty() {
        String::new()
    } else {
        format!("{}.", quote_ident(db))
    }
}

pub fn try_list_schemas(key: &str, p: &Params) -> Result<Vec<String>, String> {
    let sql = format!(
        "SELECT name FROM {}sys.schemas \
         WHERE schema_id < 16384 AND name NOT IN ('guest','INFORMATION_SCHEMA','sys') \
         ORDER BY name",
        db_prefix(&p.dbname)
    );
    with_conn(key, p, |c| {
        let (_, rows) = query_rows(c, &sql, 10_000)?;
        Ok(rows.into_iter().filter_map(|r| r.into_iter().next()).collect())
    })
}

pub fn try_list_tables(key: &str, p: &Params, schema: &str) -> Result<Vec<String>, String> {
    list_objects(key, p, schema, "BASE TABLE")
}

pub fn try_list_views(key: &str, p: &Params, schema: &str) -> Result<Vec<String>, String> {
    list_objects(key, p, schema, "VIEW")
}

fn list_objects(key: &str, p: &Params, schema: &str, kind: &str) -> Result<Vec<String>, String> {
    let schema = if schema.trim().is_empty() { "dbo" } else { schema };
    let sql = format!(
        "SELECT TABLE_NAME FROM {}INFORMATION_SCHEMA.TABLES \
         WHERE TABLE_SCHEMA = {} AND TABLE_TYPE = {} ORDER BY TABLE_NAME",
        db_prefix(&p.dbname),
        lit(schema),
        lit(kind)
    );
    with_conn(key, p, |c| {
        let (_, rows) = query_rows(c, &sql, 10_000)?;
        Ok(rows.into_iter().filter_map(|r| r.into_iter().next()).collect())
    })
}

pub fn try_list_routines(key: &str, p: &Params, schema: &str) -> Result<Vec<String>, String> {
    let schema = if schema.trim().is_empty() { "dbo" } else { schema };
    let sql = format!(
        "SELECT ROUTINE_NAME FROM {}INFORMATION_SCHEMA.ROUTINES \
         WHERE ROUTINE_SCHEMA = {} ORDER BY ROUTINE_NAME",
        db_prefix(&p.dbname),
        lit(schema)
    );
    with_conn(key, p, |c| {
        let (_, rows) = query_rows(c, &sql, 10_000)?;
        Ok(rows.into_iter().filter_map(|r| r.into_iter().next()).collect())
    })
}

pub fn try_table_columns(
    key: &str,
    p: &Params,
    schema: &str,
    table: &str,
) -> Result<Vec<ColMeta>, String> {
    let schema = if schema.trim().is_empty() { "dbo" } else { schema };
    let dp = db_prefix(&p.dbname);
    let sql = format!(
        "SELECT COLUMN_NAME, DATA_TYPE FROM {dp}INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} ORDER BY ORDINAL_POSITION",
        lit(schema),
        lit(table)
    );
    let pk_sql = format!(
        "SELECT k.COLUMN_NAME FROM {dp}INFORMATION_SCHEMA.TABLE_CONSTRAINTS t \
           JOIN {dp}INFORMATION_SCHEMA.KEY_COLUMN_USAGE k \
             ON k.CONSTRAINT_NAME = t.CONSTRAINT_NAME \
            AND k.TABLE_SCHEMA = t.TABLE_SCHEMA AND k.TABLE_NAME = t.TABLE_NAME \
         WHERE t.CONSTRAINT_TYPE = 'PRIMARY KEY' \
           AND t.TABLE_SCHEMA = {} AND t.TABLE_NAME = {}",
        lit(schema),
        lit(table)
    );
    with_conn(key, p, |c| {
        let (_, rows) = query_rows(c, &sql, 10_000)?;
        let (_, pk_rows) = query_rows(c, &pk_sql, 10_000)?;
        let pks: Vec<String> = pk_rows
            .into_iter()
            .filter_map(|r| r.into_iter().next())
            .collect();
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let name = r.first()?.clone();
                let ty = r.get(1).cloned().unwrap_or_default();
                let mut m = ColMeta::new(name.clone(), ty.to_uppercase(), num_is_numeric(&ty));
                m.pk = pks.iter().any(|p| p.eq_ignore_ascii_case(&name));
                Some(m)
            })
            .collect())
    })
}

pub fn all_columns_by_table(key: &str, p: &Params, schema: &str) -> Vec<(String, Vec<String>)> {
    let schema = if schema.trim().is_empty() { "dbo" } else { schema };
    let sql = format!(
        "SELECT TABLE_NAME, COLUMN_NAME FROM {}INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME, ORDINAL_POSITION",
        db_prefix(&p.dbname),
        lit(schema)
    );
    let Ok(rows) = with_conn(key, p, |c| query_rows(c, &sql, 100_000).map(|(_, r)| r)) else {
        return Vec::new();
    };
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for r in rows {
        let (Some(table), Some(col)) = (r.first(), r.get(1)) else {
            continue;
        };
        match out.last_mut() {
            Some((t, cols)) if t == table => cols.push(col.clone()),
            _ => out.push((table.clone(), vec![col.clone()])),
        }
    }
    out
}

// ── 取数 ─────────────────────────────────────────────────────────

/// 表页取数的 SQL（状态栏也展示这一条）。SQL Server 的 OFFSET/FETCH 必须带
/// ORDER BY；用户没选排序时用 `ORDER BY (SELECT NULL)` 占位（行序无保证，
/// 与 Flutter 版一致）。
pub fn select_table_sql(
    cols: &[ColMeta],
    db: &str,
    schema: &str,
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
        " ORDER BY (SELECT NULL)".to_string()
    } else {
        let cols: Vec<String> = keys
            .iter()
            .map(|(c, asc)| format!("{} {}", quote_ident(c), if *asc { "ASC" } else { "DESC" }))
            .collect();
        format!(" ORDER BY {}", cols.join(", "))
    };
    let where_part = where_sql.map(|w| format!(" WHERE {w}")).unwrap_or_default();
    format!(
        "SELECT {list} FROM {}{where_part}{order_part} OFFSET {} ROWS FETCH NEXT {} ROWS ONLY",
        qualify(db, schema, table),
        offset.max(0),
        limit.max(1)
    )
}

#[allow(clippy::too_many_arguments)]
pub fn select_table(
    key: &str,
    p: &Params,
    db: &str,
    schema: &str,
    table: &str,
    where_sql: Option<&str>,
    orders: &[(String, bool)],
    offset: usize,
    limit: usize,
) -> Result<SelectPage, String> {
    let cols = try_table_columns(key, p, schema, table)?;
    let pk: Vec<usize> = cols
        .iter()
        .enumerate()
        .filter(|(_, c)| c.pk)
        .map(|(i, _)| i)
        .collect();
    let sql = select_table_sql(&cols, db, schema, table, where_sql, orders, limit, offset);
    with_conn(key, p, |c| take_page(c, &sql, cols, &pk, usize::MAX))
}

fn take_page(
    conn: &mut Conn,
    sql: &str,
    columns: Vec<ColMeta>,
    pk: &[usize],
    max_rows: usize,
) -> Result<SelectPage, String> {
    block_on(async {
        let mut stream = conn.simple_query(sql).await.map_err(db_err)?;
        let columns = if columns.is_empty() {
            let described = stream.columns().await.map_err(db_err)?;
            describe_columns(described.unwrap_or(&[]))
        } else {
            columns
        };
        let mut rows = Vec::new();
        let mut keys = Vec::new();
        let mut truncated = false;
        while let Some(item) = stream.try_next().await.map_err(db_err)? {
            let QueryItem::Row(row) = item else {
                continue;
            };
            if row.result_index() != 0 {
                continue;
            }
            if rows.len() >= max_rows {
                truncated = true;
                break;
            }
            let vals = row_text(row);
            keys.push(
                pk.iter()
                    .filter_map(|i| vals.get(*i).cloned())
                    .collect::<Vec<_>>(),
            );
            rows.push(vals);
        }
        Ok(SelectPage {
            columns,
            rows,
            keys,
            truncated,
        })
    })
}

pub fn count_table(
    key: &str,
    p: &Params,
    db: &str,
    schema: &str,
    table: &str,
    where_sql: Option<&str>,
) -> Result<usize, String> {
    let where_part = where_sql.map(|w| format!(" WHERE {w}")).unwrap_or_default();
    let sql = format!(
        "SELECT COUNT_BIG(*) FROM {}{where_part}",
        qualify(db, schema, table)
    );
    with_conn(key, p, |c| {
        let (_, rows) = query_rows(c, &sql, 1)?;
        Ok(rows
            .first()
            .and_then(|r| r.first())
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0))
    })
}

pub fn query_is_select(stmt: &str) -> bool {
    let s = stmt.trim_start();
    let head = s.get(..6).unwrap_or("").to_ascii_lowercase();
    head == "select" || head == "with" || head.starts_with("exec") || head.starts_with("show")
}

pub fn execute_stmt(key: &str, stmt: &str, max_rows: usize) -> Result<ExecOutcome, String> {
    with_open_conn(key, |c| execute_on(c, stmt, max_rows))
}

fn row_text(row: Row) -> Vec<String> {
    row.into_iter().map(cell_text).collect()
}

/// tiberius 的结果流只暴露列与行，不带 Done 里的影响行数，所以批处理里没有
/// 结果集时再问一次 `@@ROWCOUNT`。
async fn affected_rows(conn: &mut Conn) -> Result<usize, String> {
    let stream = conn
        .simple_query("SELECT @@ROWCOUNT")
        .await
        .map_err(db_err)?;
    let row = stream.into_row().await.map_err(db_err)?;
    let n = row
        .and_then(|r| r.into_iter().next())
        .map(cell_text)
        .unwrap_or_default();
    Ok(n.trim().parse().unwrap_or(0))
}

/// 只取影响行数（写回/事务控制用）。`execute` 直接读 Done 里的行数，不占
/// 结果流，也不用额外往返。
fn run_affected(conn: &mut Conn, sql: &str) -> Result<usize, String> {
    block_on(async {
        let res = conn.execute(sql, &[]).await.map_err(db_err)?;
        Ok(res.rows_affected().iter().sum::<u64>() as usize)
    })
}

fn execute_on(conn: &mut Conn, stmt: &str, max_rows: usize) -> Result<ExecOutcome, String> {
    block_on(async {
        let mut stream = conn.simple_query(stmt).await.map_err(db_err)?;
        let described = stream.columns().await.map_err(db_err)?;
        let grid = described.map(describe_columns);
        let Some(columns) = grid else {
            drop(stream);
            let affected = affected_rows(conn).await?;
            return Ok(ExecOutcome {
                select: None,
                affected,
                message: format!("{affected} rows affected"),
            });
        };
        let mut rows = Vec::new();
        let mut truncated = false;
        while let Some(item) = stream.try_next().await.map_err(db_err)? {
            let QueryItem::Row(row) = item else {
                continue;
            };
            if row.result_index() != 0 {
                continue;
            }
            if rows.len() >= max_rows {
                truncated = true;
                break;
            }
            rows.push(row_text(row));
        }
        let msg = if truncated {
            format!("结果超过 {max_rows} 行，已截断")
        } else {
            format!("{} 行", rows.len())
        };
        Ok(ExecOutcome {
            select: Some(SelectPage {
                columns,
                rows,
                keys: Vec::new(),
                truncated,
            }),
            affected: 0,
            message: msg,
        })
    })
}

// ── 写回 ─────────────────────────────────────────────────────────

/// 单元格 → SQL 字面量。`NULL` 是空值哨兵；数值列不加引号。
fn literal(meta: &ColMeta, v: &str) -> String {
    if v == NULL_TEXT {
        return "NULL".to_string();
    }
    let unquoted = v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'');
    let val = if unquoted {
        v[1..v.len() - 1].replace("''", "'")
    } else {
        v.to_string()
    };
    if meta.numeric && !unquoted {
        let t = val.trim();
        if t.parse::<f64>().is_ok() {
            return t.to_string();
        }
        if t.is_empty() {
            return "NULL".to_string();
        }
    }
    lit(&val)
}

/// 视图不能直接编辑。
fn is_view(conn: &mut Conn, db: &str, schema: &str, table: &str) -> Result<bool, String> {
    let schema = if schema.trim().is_empty() { "dbo" } else { schema };
    let sql = format!(
        "SELECT TABLE_TYPE FROM {}INFORMATION_SCHEMA.TABLES \
         WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {}",
        db_prefix(db),
        lit(schema),
        lit(table)
    );
    let (_, rows) = query_rows(conn, &sql, 1)?;
    Ok(rows
        .first()
        .and_then(|r| r.first())
        .map(|t| t.eq_ignore_ascii_case("VIEW"))
        .unwrap_or(false))
}

pub fn apply_changes(
    key: &str,
    p: &Params,
    db: &str,
    schema: &str,
    ch: &GridChanges<'_>,
) -> Result<String, String> {
    with_conn(key, p, |c| {
        if is_view(c, db, schema, ch.table)? {
            return Err(format!("「{}」是视图，不能直接编辑；请修改基表", ch.table));
        }
        let pk: Vec<&ColMeta> = ch.columns.iter().filter(|c| c.pk).collect();
        if (!ch.updates.is_empty() || !ch.deletes.is_empty()) && pk.is_empty() {
            return Err("该表没有主键，不支持编辑和删除写回".to_string());
        }
        let where_part = |vals: &[String]| -> Result<String, String> {
            if vals.len() != pk.len() {
                return Err("行标识与当前主键不匹配，请刷新页签".to_string());
            }
            Ok(pk
                .iter()
                .zip(vals)
                .map(|(c, v)| {
                    if *v == NULL_TEXT {
                        format!("{} IS NULL", quote_ident(&c.name))
                    } else {
                        format!("{} = {}", quote_ident(&c.name), literal(c, v))
                    }
                })
                .collect::<Vec<_>>()
                .join(" AND "))
        };
        let table = qualify(db, schema, ch.table);
        let mut touched = 0usize;
        run_affected(c, "BEGIN TRANSACTION")?;
        let result = (|| -> Result<(), String> {
            for (key_vals, col, val) in ch.updates {
                let Some(meta) = ch.columns.get(*col) else {
                    continue;
                };
                if !meta.writable {
                    return Err(format!("列「{}」不能写回", meta.name));
                }
                let sql = format!(
                    "UPDATE {table} SET {} = {} WHERE {}",
                    quote_ident(&meta.name),
                    literal(meta, val),
                    where_part(key_vals)?
                );
                let n = run_affected(c, &sql)?;
                if n == 0 {
                    return Err(
                        "按主键没找到该行（可能已被其它客户端改动或删除），请刷新".to_string()
                    );
                }
                touched += n;
            }
            for key_vals in ch.deletes {
                let sql = format!("DELETE FROM {table} WHERE {}", where_part(key_vals)?);
                let n = run_affected(c, &sql)?;
                if n == 0 {
                    return Err("按主键没找到该行（可能已被其它客户端删除），请刷新".to_string());
                }
                touched += n;
            }
            if !ch.inserts.is_empty() {
                let idx: Vec<usize> = ch
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.writable)
                    .map(|(i, _)| i)
                    .collect();
                let names = idx
                    .iter()
                    .map(|i| quote_ident(&ch.columns[*i].name))
                    .collect::<Vec<_>>()
                    .join(", ");
                for row in ch.inserts {
                    let vals = idx
                        .iter()
                        .map(|i| {
                            row.get(*i)
                                .map(|v| literal(&ch.columns[*i], v))
                                .unwrap_or_else(|| "NULL".to_string())
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let sql = format!("INSERT INTO {table} ({names}) VALUES ({vals})");
                    touched += run_affected(c, &sql)?;
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                run_affected(c, "COMMIT TRANSACTION")?;
                Ok(format!("{touched} rows affected"))
            }
            Err(e) => {
                let _ = run_affected(c, "ROLLBACK TRANSACTION");
                Err(e)
            }
        }
    })
}

// ── 后台脚本 ─────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Job {
    params: Params,
    dbname: String,
    stop: Arc<AtomicBool>,
}

impl Job {
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn interrupt(&self) {
        self.request_stop();
    }

    pub fn cancel(&self) {
        self.request_stop();
    }
}

pub fn create_job(p: &Params) -> Job {
    Job {
        params: p.clone(),
        dbname: p.dbname.clone(),
        stop: Arc::new(AtomicBool::new(false)),
    }
}

/// 后台跑整段脚本：自己开一条连接，按 `continue_on_error` 决定是否在
/// 第一条报错后收手。停止只在语句之间生效（驱动层不发取消请求）。
pub fn run_script(
    job: &Job,
    stmts: &[String],
    max_rows: usize,
    continue_on_error: bool,
) -> ScriptRun {
    let started = std::time::Instant::now();
    let mut runs = Vec::new();
    let conn = connect(&job.params);
    let mut conn = match conn {
        Ok(c) => c,
        Err(e) => {
            return ScriptRun {
                runs: Vec::new(),
                open_error: Some(e),
                cancelled: false,
                rolled_back: false,
                aborted_tx: false,
                total_ms: started.elapsed().as_millis(),
            }
        }
    };
    let _ = &job.dbname;
    let mut cancelled = false;
    for stmt in stmts {
        if job.stop.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let t0 = std::time::Instant::now();
        let result = execute_on(&mut conn, stmt, max_rows);
        let failed = result.is_err();
        runs.push(StmtRun {
            elapsed_ms: t0.elapsed().as_millis(),
            result,
        });
        if failed && !continue_on_error {
            break;
        }
    }
    ScriptRun {
        runs,
        open_error: None,
        cancelled,
        rolled_back: false,
        aborted_tx: false,
        total_ms: started.elapsed().as_millis(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ident_quoting_escapes_brackets() {
        assert_eq!(quote_ident("a]b"), "[a]]b]");
    }

    #[test]
    fn qualify_defaults_schema_to_dbo() {
        assert_eq!(qualify("db", "", "t"), "[db].[dbo].[t]");
        assert_eq!(qualify("db", "sales", "t"), "[db].[sales].[t]");
    }

    #[test]
    fn db_prefix_qualifies_system_views() {
        assert_eq!(db_prefix(""), "");
        assert_eq!(db_prefix("  "), "");
        assert_eq!(db_prefix("sales]db"), "[sales]]db].");
    }

    #[test]
    fn datetime_text_matches_sql_server_display() {
        assert_eq!(
            fmt_datetime(TibDateTime::new(0, 0)),
            "1900-01-01 00:00:00.000"
        );
        // 1/300 秒一格，展示截到毫秒
        assert_eq!(
            fmt_datetime(TibDateTime::new(0, 1)),
            "1900-01-01 00:00:00.003"
        );
        assert_eq!(
            fmt_datetime(TibDateTime::new(0, 300)),
            "1900-01-01 00:00:01.000"
        );
        let base = NaiveDate::from_ymd_opt(1900, 1, 1).unwrap();
        let day = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let days = day.signed_duration_since(base).num_days() as i32;
        assert_eq!(
            fmt_datetime(TibDateTime::new(days, 0)),
            "2024-01-02 00:00:00.000"
        );
    }

    #[test]
    fn smalldatetime_date_and_time_text() {
        assert_eq!(
            fmt_smalldatetime(SmallDateTime::new(0, 90)),
            "1900-01-01 01:30:00"
        );
        assert_eq!(fmt_date(TibDate::new(0)), "0001-01-01");
        let days = NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .signed_duration_since(NaiveDate::from_ymd_opt(1, 1, 1).unwrap())
            .num_days() as u32;
        assert_eq!(fmt_date(TibDate::new(days)), "2024-01-02");
        assert_eq!(
            fmt_time(TibTime::new(45_296 * 10_000_000, 7)),
            "12:34:56.0000000"
        );
        assert_eq!(
            fmt_time(TibTime::new(45_296_1234567, 7)),
            "12:34:56.1234567"
        );
        assert_eq!(
            fmt_datetimeoffset(TibDateTimeOffset::new(
                TibDateTime2::new(TibDate::new(0), TibTime::new(0, 7)),
                -330
            )),
            "0001-01-01 00:00:00.0000000-05:30"
        );
    }

    #[test]
    fn cell_text_matches_driver_text() {
        assert_eq!(cell_text(ColumnData::Bit(Some(true))), "1");
        assert_eq!(cell_text(ColumnData::Bit(Some(false))), "0");
        assert_eq!(cell_text(ColumnData::I32(None)), "NULL");
        assert_eq!(
            cell_text(ColumnData::String(Some("中文".into()))),
            "中文"
        );
        let bytes = ColumnData::Binary(Some(std::borrow::Cow::Borrowed(&[0xDE, 0xAD])));
        assert_eq!(cell_text(bytes), "0xDEAD");
        let guid = tiberius::Uuid::from_u128(0x6f9619ff_8b86_d011_b42d_00c04fc964ff);
        assert_eq!(
            cell_text(ColumnData::Guid(Some(guid))),
            "6F9619FF-8B86-D011-B42D-00C04FC964FF"
        );
    }

    #[test]
    fn type_labels_match_sql_server_names() {
        assert_eq!(type_label(ColumnType::Datetime2), "DATETIME2");
        assert_eq!(type_label(ColumnType::NVarchar), "NVARCHAR");
        assert_eq!(type_label(ColumnType::Guid), "UNIQUEIDENTIFIER");
        assert!(num_is_numeric(&type_label(ColumnType::Decimaln)));
        assert!(!num_is_numeric(&type_label(ColumnType::NVarchar)));
    }

    fn target(host: &str, instance: Option<&str>, port: Option<u16>) -> Target {
        Target {
            host: host.to_string(),
            instance: instance.map(str::to_string),
            port,
        }
    }

    #[test]
    fn host_splits_named_instance_and_port() {
        assert_eq!(split_host(r"localhost\SQLEXPRESS"), target("localhost", Some("SQLEXPRESS"), None));
        assert_eq!(split_host(r".\SQLEXPRESS"), target(".", Some("SQLEXPRESS"), None));
        assert_eq!(split_host(r"10.0.0.5\MSSQL2019,14330"), target("10.0.0.5", Some("MSSQL2019"), Some(14330)));
        assert_eq!(split_host("db.example.com"), target("db.example.com", None, None));
        // 端口两种写法都认：`,` 是 SQL Server 的传统写法，`:` 照顾从连接串抄过来的习惯
        assert_eq!(split_host("db.example.com,51433"), target("db.example.com", None, Some(51433)));
        assert_eq!(split_host("db.example.com:51433"), target("db.example.com", None, Some(51433)));
        assert_eq!(split_host("  localhost\\SQLEXPRESS  "), target("localhost", Some("SQLEXPRESS"), None));
    }

    #[test]
    fn host_without_valid_port_keeps_whole_string() {
        // 端口位不是数字就别切，整串当主机名
        assert_eq!(split_host("db.example.com:abc"), target("db.example.com:abc", None, None));
        // 端口超范围同样不切
        assert_eq!(split_host("db.example.com:70000"), target("db.example.com:70000", None, None));
        // IPv6 字面量里全是冒号，不能当成端口
        assert_eq!(split_host("::1"), target("::1", None, None));
        assert_eq!(split_host("[::1]"), target("[::1]", None, None));
        // 实例名为空时不认成命名实例
        assert_eq!(split_host("localhost\\"), target("localhost\\", None, None));
        assert_eq!(split_host(""), target("", None, None));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// 本机 docker 起的 SQL Server 2022（Linux 容器没有 SQL Browser 服务，
    /// 命名实例那条要额外起一个假的 Browser 监听 UDP 1434）：
    ///
    /// ```text
    /// docker run -d --name daro-mssql -e ACCEPT_EULA=Y `
    ///   -e MSSQL_SA_PASSWORD='Daro!Test2026' -p 14330:1433 `
    ///   mcr.microsoft.com/mssql/server:2022-latest
    /// ```
    fn env_params() -> Params {
        let get = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        Params {
            host: get("DARO_MSSQL_HOST", "127.0.0.1"),
            port: get("DARO_MSSQL_PORT", "14330").parse().unwrap_or(14330),
            user: get("DARO_MSSQL_USER", "sa"),
            password: get("DARO_MSSQL_PASSWORD", "Daro!Test2026"),
            dbname: String::new(),
        }
    }

    #[test]
    #[ignore = "需要 docker 里跑着的 SQL Server"]
    fn docker_login_and_list_databases() {
        let p = env_params();
        probe(&p).expect("直连端口登录 docker SQL Server 失败");
        let mut c = connect(&p).unwrap();
        let (_, rows) = query_rows(&mut c, "SELECT name FROM sys.databases ORDER BY name", 100).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r[0].as_str()).collect();
        for db in ["master", "tempdb"] {
            assert!(names.contains(&db), "系统库要在清单里：{names:?}");
        }
    }

    /// 命名实例：端口不固定，必须靠 SQL Browser 报回来。夹具把实例名报成上面
    /// 那个容器的映射端口，所以这里故意把表单端口填成错的 1433，验证走 Browser
    /// 时表单端口不参与。
    ///
    /// 实例名默认 `MSSQLSERVER`：Linux 容器里只有这一个实例名能过服务器的
    /// prelogin 校验；连真 Windows 命名实例时用 `DARO_MSSQL_INSTANCE=SQLEXPRESS`。
    #[test]
    #[ignore = "需要 docker SQL Server + 监听 UDP 1434 的假 Browser"]
    fn named_instance_port_comes_from_browser() {
        let mut p = env_params();
        let instance = std::env::var("DARO_MSSQL_INSTANCE").unwrap_or_else(|_| "MSSQLSERVER".to_string());
        p.host = format!("{}\\{}", p.host, instance);
        p.port = 1433;
        probe(&p).unwrap_or_else(|e| panic!("命名实例 {instance} 应通过 SQL Browser 解析出真实端口：{e}"));
    }

    /// 主机串里钉了端口就不问 Browser，直接连（与 SqlClient 同口径）。
    #[test]
    #[ignore = "需要 docker 里跑着的 SQL Server"]
    fn port_in_host_skips_browser() {
        let mut p = env_params();
        let port = p.port;
        p.host = format!("{}\\NOPE,{}", p.host, port);
        p.port = 1433;
        probe(&p).expect("主机里给了端口，实例名应被忽略并直连该端口");
    }
}
