//! SQL Server 驱动：走系统 ODBC（与 Flutter 版 `sqlserver_driver.dart` 同一条路）。
//!
//! 方言口径：
//! - 标识符 `[name]`，内部 `]` 双写；字面量 `N'...'`，单引号双写
//! - 表名三段 `[库].[模式].[表]`；模式缺省 `dbo`
//! - 分页 `OFFSET n ROWS FETCH NEXT m ROWS ONLY`，没有 ORDER BY 时补
//!   `ORDER BY (SELECT NULL)`，否则 SQL Server 直接报错
//! - 认证：填了用户名走 SQL 认证，留空走 Windows 集成认证（Trusted_Connection）

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use odbc_api::{Connection, ConnectionOptions, Cursor, Environment, ResultSetMetadata};

use crate::db_types::{ColMeta, ExecOutcome, GridChanges, ScriptRun, SelectPage, StmtRun};

/// 查询页一次取回的最大行数。
pub const QUERY_ROW_LIMIT: usize = 5000;
const NULL_TEXT: &str = "NULL";
const QUERY_TIMEOUT_SECS: usize = 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Params {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub dbname: String,
}

static ENV: LazyLock<Result<&'static Environment, String>> = LazyLock::new(|| {
    Environment::new()
        .map(|env| &*Box::leak(Box::new(env)))
        .map_err(|e| format!("初始化 ODBC 失败：{e}"))
});

static CONNS: LazyLock<Mutex<HashMap<String, Arc<Mutex<Connection<'static>>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 与 Flutter 版一致的驱动查找顺序：先新的 Microsoft 驱动，最后是系统自带的
/// 旧 `SQL Server` 驱动。
const DRIVERS: [&str; 5] = [
    "ODBC Driver 18 for SQL Server",
    "ODBC Driver 17 for SQL Server",
    "ODBC Driver 13 for SQL Server",
    "SQL Server Native Client 11.0",
    "SQL Server",
];

fn connection_string(p: &Params, driver: &str) -> String {
    let host = p.host.trim();
    let port = if p.port == 0 { 1433 } else { p.port };
    let db = if p.dbname.trim().is_empty() {
        "master"
    } else {
        p.dbname.trim()
    };
    let auth = if p.user.trim().is_empty() {
        "Trusted_Connection=yes;".to_string()
    } else {
        format!(
            "UID={};PWD={};",
            esc_conn(p.user.trim()),
            esc_conn(&p.password)
        )
    };
    format!(
        "DRIVER={{{driver}}};SERVER={host},{port};DATABASE={db};\
         Encrypt=No;TrustServerCertificate=Yes;APP=daro;{auth}"
    )
}

/// 连接串里的分号 / 花括号要转义，否则密码里有 `;` 会截断参数。
fn esc_conn(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ';' => out.push_str("{;}"),
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            c => out.push(c),
        }
    }
    out
}

fn connect(p: &Params) -> Result<Connection<'static>, String> {
    let env = *ENV.as_ref().as_ref().map_err(|e| e.clone())?;
    let mut last = String::new();
    for driver in DRIVERS {
        let cs = connection_string(p, driver);
        match env.connect_with_connection_string(&cs, ConnectionOptions::default()) {
            Ok(conn) => return Ok(conn),
            Err(e) => last = format!("{driver}: {e}"),
        }
    }
    Err(format!(
        "SQL Server 连接失败（{last}）。请确认已安装 64 位 Microsoft ODBC Driver for SQL Server"
    ))
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

fn with_conn<T>(key: &str, p: &Params, f: impl FnOnce(&mut Connection<'static>) -> Result<T, String>) -> Result<T, String> {
    ensure(key, p)?;
    with_open_conn(key, f)
}

/// 只按缓存键执行：连接必须先 open / ensure 过（与其它驱动的 `execute_stmt` 同口径）。
fn with_open_conn<T>(key: &str, f: impl FnOnce(&mut Connection<'static>) -> Result<T, String>) -> Result<T, String> {
    let c = CONNS.lock().unwrap().get(key).cloned();
    let Some(c) = c else {
        return Err("SQL Server 连接已关闭".to_string());
    };
    let mut guard = c.lock().unwrap();
    match f(&mut guard) {
        Ok(v) => Ok(v),
        Err(e) => {
            let lower = e.to_ascii_lowercase();
            if lower.contains("communication")
                || lower.contains("connection is broken")
                || lower.contains("08s01")
                || lower.contains("not connected")
            {
                CONNS.lock().unwrap().remove(key);
            }
            Err(e)
        }
    }
}

fn odbc_err(e: odbc_api::Error) -> String {
    e.to_string()
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
    conn: &mut Connection<'static>,
    sql: &str,
    max_rows: usize,
) -> Result<(Vec<ColMeta>, Vec<Vec<String>>), String> {
    let mut stmt = conn.preallocate().map_err(odbc_err)?;
    let Some(mut cursor) = stmt.execute(sql, ()).map_err(odbc_err)? else {
        return Ok((Vec::new(), Vec::new()));
    };
    let ncols = cursor.num_result_cols().map_err(odbc_err)?;
    let mut columns = Vec::new();
    for i in 1..=ncols {
        let name = cursor.col_name(i as u16).unwrap_or_else(|_| format!("col{i}"));
        let ty = cursor
            .col_data_type(i as u16)
            .map(type_label)
            .unwrap_or_default();
        let mut meta = ColMeta::new(name, ty.clone(), num_is_numeric(&ty));
        meta.writable = false;
        columns.push(meta);
    }
    let columns = dedup_names(columns);
    let mut rows = Vec::new();
    while let Some(mut row) = cursor.next_row().map_err(odbc_err)? {
        if rows.len() >= max_rows {
            break;
        }
        let mut vals = Vec::with_capacity(columns.len());
        for i in 1..=ncols {
            let mut buf = Vec::new();
            let present = row.get_text(i as u16, &mut buf).map_err(odbc_err)?;
            if present {
                vals.push(String::from_utf8_lossy(&buf).into_owned());
            } else {
                vals.push(NULL_TEXT.to_string());
            }
        }
        rows.push(vals);
    }
    Ok((columns, rows))
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

fn type_label(t: odbc_api::handles::DataType) -> String {
    use odbc_api::handles::DataType as D;
    match t {
        D::Char { .. } => "CHAR",
        D::WChar { .. } => "NCHAR",
        D::Varchar { .. } => "VARCHAR",
        D::WVarchar { .. } => "NVARCHAR",
        D::LongVarchar { .. } | D::WLongVarchar { .. } => "TEXT",
        D::Numeric { .. } | D::Decimal { .. } => "DECIMAL",
        D::Integer => "INT",
        D::SmallInt => "SMALLINT",
        D::BigInt => "BIGINT",
        D::TinyInt => "TINYINT",
        D::Bit => "BIT",
        D::Float { .. } => "FLOAT",
        D::Real => "REAL",
        D::Double => "DOUBLE",
        D::Date => "DATE",
        D::Time { .. } => "TIME",
        D::Timestamp { .. } => "DATETIME",
        D::Varbinary { .. } | D::Binary { .. } | D::LongVarbinary { .. } => "VARBINARY",
        _ => "SQL_VARIANT",
    }
    .to_string()
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
    conn: &mut Connection<'static>,
    sql: &str,
    columns: Vec<ColMeta>,
    pk: &[usize],
    max_rows: usize,
) -> Result<SelectPage, String> {
    let mut stmt = conn.preallocate().map_err(odbc_err)?;
    let Some(mut cursor) = stmt.execute(sql, ()).map_err(odbc_err)? else {
        return Ok(SelectPage {
            columns,
            rows: Vec::new(),
            keys: Vec::new(),
            truncated: false,
        });
    };
    let ncols = cursor.num_result_cols().map_err(odbc_err)?;
    let columns = if columns.is_empty() {
        let mut described = Vec::new();
        for i in 1..=ncols {
            let name = cursor.col_name(i as u16).unwrap_or_else(|_| format!("col{i}"));
            let ty = cursor
                .col_data_type(i as u16)
                .map(type_label)
                .unwrap_or_default();
            let mut m = ColMeta::new(name, ty.clone(), num_is_numeric(&ty));
            m.writable = false;
            described.push(m);
        }
        dedup_names(described)
    } else {
        columns
    };
    let mut rows = Vec::new();
    let mut keys = Vec::new();
    let mut truncated = false;
    while let Some(mut row) = cursor.next_row().map_err(odbc_err)? {
        if rows.len() >= max_rows {
            truncated = true;
            break;
        }
        let mut vals = Vec::with_capacity(columns.len());
        for i in 1..=ncols {
            let mut buf = Vec::new();
            let present = row.get_text(i as u16, &mut buf).map_err(odbc_err)?;
            if present {
                vals.push(String::from_utf8_lossy(&buf).into_owned());
            } else {
                vals.push(NULL_TEXT.to_string());
            }
        }
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

fn execute_on(
    conn: &mut Connection<'static>,
    stmt: &str,
    max_rows: usize,
) -> Result<ExecOutcome, String> {
    let mut st = conn.preallocate().map_err(odbc_err)?;
    let cursor = st.execute(stmt, ()).map_err(odbc_err)?;
    if let Some(mut cursor) = cursor {
        let ncols = cursor.num_result_cols().map_err(odbc_err)?;
        let mut columns = Vec::new();
        for i in 1..=ncols {
            let name = cursor.col_name(i as u16).unwrap_or_else(|_| format!("col{i}"));
            let ty = cursor
                .col_data_type(i as u16)
                .map(type_label)
                .unwrap_or_default();
            let mut m = ColMeta::new(name, ty.clone(), num_is_numeric(&ty));
            m.writable = false;
            columns.push(m);
        }
        let columns = dedup_names(columns);
        let mut rows = Vec::new();
        let mut truncated = false;
        while let Some(mut row) = cursor.next_row().map_err(odbc_err)? {
            if rows.len() >= max_rows {
                truncated = true;
                break;
            }
            let mut vals = Vec::with_capacity(columns.len());
            for i in 1..=ncols {
                let mut buf = Vec::new();
                let present = row.get_text(i as u16, &mut buf).map_err(odbc_err)?;
                if present {
                    vals.push(String::from_utf8_lossy(&buf).into_owned());
                } else {
                    vals.push(NULL_TEXT.to_string());
                }
            }
            rows.push(vals);
        }
        let msg = if truncated {
            format!("结果超过 {max_rows} 行，已截断")
        } else {
            format!("{} 行", rows.len())
        };
        return Ok(ExecOutcome {
            select: Some(SelectPage {
                columns,
                rows,
                keys: Vec::new(),
                truncated,
            }),
            affected: 0,
            message: msg,
        });
    }
    drop(cursor);
    let affected = st.row_count().map_err(odbc_err)?.unwrap_or(0);
    Ok(ExecOutcome {
        select: None,
        affected,
        message: format!("{affected} rows affected"),
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
fn is_view(
    conn: &mut Connection<'static>,
    db: &str,
    schema: &str,
    table: &str,
) -> Result<bool, String> {
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
        execute_on(c, "BEGIN TRANSACTION", 1)?;
        let run = |c: &mut Connection<'static>, sql: &str| -> Result<usize, String> {
            let mut st = c.preallocate().map_err(odbc_err)?;
            let cursor = st.execute(sql, ()).map_err(odbc_err)?;
            drop(cursor);
            Ok(st.row_count().map_err(odbc_err)?.unwrap_or(0))
        };
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
                let n = run(c, &sql)?;
                if n == 0 {
                    return Err(
                        "按主键没找到该行（可能已被其它客户端改动或删除），请刷新".to_string()
                    );
                }
                touched += n;
            }
            for key_vals in ch.deletes {
                let sql = format!("DELETE FROM {table} WHERE {}", where_part(key_vals)?);
                let n = run(c, &sql)?;
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
                    touched += run(c, &sql)?;
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                execute_on(c, "COMMIT TRANSACTION", 1)?;
                Ok(format!("{touched} rows affected"))
            }
            Err(e) => {
                let _ = execute_on(c, "ROLLBACK TRANSACTION", 1);
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
/// 第一条报错后收手。停止只在语句之间生效（ODBC 不发取消请求）。
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
    fn connection_string_escapes_password() {
        let p = Params {
            host: "h".into(),
            port: 1433,
            user: "sa".into(),
            password: "a;b}c".into(),
            dbname: String::new(),
        };
        let cs = connection_string(&p, "ODBC Driver 18 for SQL Server");
        assert!(cs.contains("PWD=a{;}b}}c;"), "{cs}");
        assert!(cs.contains("DATABASE=master;"), "{cs}");
    }
}
