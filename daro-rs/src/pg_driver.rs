//! PostgreSQL 真实驱动（rust-postgres 同步 Client，NoTls）。
//!
//! 与 SQLite 档共用 `db_types` 的网格口径（列元数据 / 行身份 / 一页取数 /
//! 一条语句的回执），两处按 PostgreSQL 的现实改了形态：
//!
//! - **连接按库建**：一条 PG 连接只看得见自己那个库，所以注册表的键是
//!   「连接名 + 库名」，树上展开到哪个库才为哪个库建连。
//! - **行身份用主键**：PG 没有可依赖的稳定行号（ctid 会被 UPDATE / VACUUM
//!   改写），取数时按主键列的值拼行身份；没有主键的表不支撑改 / 删。
//!
//! 取数一律走文本协议：numeric / timestamptz / 数组 / jsonb 都按服务端给的
//! 文本原样展示，不逐类型解码。查询页用服务端游标取数（`DECLARE` +
//! `FETCH FORWARD`），只把前 N+1 行收进客户端，结果再大也不会整包进内存；
//! 游标装不下的语句（EXPLAIN / SHOW 等）退回 `simple_query`。行描述只给列名，
//! 所以查询页的列类型另问一次 `prepare`，问完立刻 `DEALLOCATE`。
//!
//! 写回按探针验过的路子：参数一律按 text 送、SQL 里显式二次转换
//! （`$1::text::bigint`）。直接把 `&str` 绑给数值位会被服务端报 WrongType。
//!
//! 事务：PG 在事务里报错后会把事务标成「已中止」，之后每条语句都连带失败，
//! 所以脚本报错撞上事务态就回滚收尾（SQLite 档是「出错继续」）。脚本里的
//! 事务态在驱动侧按关键字跟——`pg_stat_activity` 问自己那条 backend 恒是
//! active，问不出事务态。
//!
//! 暂不支持 TLS：服务端强制 SSL 时连接失败，错误原样上屏。

use std::collections::HashMap;
use std::error::Error as StdError;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

use postgres::types::ToSql;
use postgres::{Client, Config, Error, NoTls, SimpleQueryMessage, Transaction};

use crate::db_types::{ColMeta, ExecOutcome, GridChanges, RowKey, ScriptRun, SelectPage, StmtRun};

/// 查询页一次最多取回的行数（超出只标「已截断」，不无限吃内存）。
pub const QUERY_ROW_LIMIT: usize = 5000;

/// 建连上限（秒）：树展开 / 打开页签在 UI 线程同步建连，服务器不可达时
/// 不能让 TCP 默认超时把界面冻住十几秒。
const CONNECT_TIMEOUT_SECS: u64 = 5;

/// 目录类查询的上限（秒）：库 / 模式 / 对象 / 列元数据都是服务端秒级就该
/// 回来的小查询，但它们占的是驱动注册表那把全局锁——一条卡死的目录查询会
/// 连带堵住同进程所有取数。查询页的脚本不在此列（可中断，不设上限）。
const CATALOG_TIMEOUT_SECS: u64 = 5;

/// 与 SQLite 档同一口径：真 NULL 显示成字面 `NULL`。
const NULL_TEXT: &str = "NULL";

/// 建一条 PG 连接要的东西。`dbname` 决定这条连接看得见哪个库，所以树上
/// 每个库各是一份 Params。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Params {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub dbname: String,
}

impl Params {
    /// 同一台服务器换库（树上展开别的库时用）。
    pub fn for_db(&self, db: &str) -> Params {
        let mut p = self.clone();
        p.dbname = db.to_string();
        p
    }
}

/// 注册表键：连接名 + 库名（PG 一条连接只认一个库）。
pub fn key(conn: &str, db: &str) -> String {
    format!("{conn}\u{1}{db}")
}

/// 某连接名下全部键的公共前缀（断开 / 改名时按前缀一并关连接）。
pub fn prefix(conn: &str) -> String {
    format!("{conn}\u{1}")
}

/// 注册表：键 → 句柄。锁只护住这张表本身，**绝不跨网络 I/O 持有**：
/// 每条连接自带一把锁，谁用它谁锁它。UI 线程点「关闭连接」时只做一次
/// 表操作，不会被后台正在跑的建连 / 取数堵住（那曾把界面冻死）。
static CONNS: LazyLock<Mutex<HashMap<String, Arc<Mutex<Client>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 注册表里取一条句柄；持注册表锁的时间只够一次克隆。
fn handle(key: &str) -> Result<Arc<Mutex<Client>>, String> {
    CONNS
        .lock()
        .unwrap()
        .get(key)
        .cloned()
        .ok_or_else(|| "连接未打开".to_string())
}

fn connect(p: &Params) -> Result<Client, String> {
    base_config(p).connect(NoTls).map_err(|e| err_text(&e))
}

/// 连接参数 → rust-postgres 的 Config（测试要另造一条独立连接时也用它）。
fn base_config(p: &Params) -> Config {
    let mut cfg = Config::new();
    cfg.host(&p.host)
        .port(p.port)
        .user(&p.user)
        .password(&p.password);
    if !p.dbname.is_empty() {
        cfg.dbname(&p.dbname);
    }
    // 建连发生在 UI 线程（展开树 / 打开页签时惰性连），不能让它按 TCP
    // 默认超时冻住界面十几秒：到有上限就报错上屏
    cfg.connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS));
    cfg.application_name("daro-rs");
    cfg
}

/// 打开（或重开）一条库连接并登记。
pub fn open(key: &str, p: &Params) -> Result<(), String> {
    let client = connect(p)?;
    CONNS
        .lock()
        .unwrap()
        .insert(key.to_string(), Arc::new(Mutex::new(client)));
    Ok(())
}

pub fn is_open(key: &str) -> bool {
    CONNS.lock().unwrap().contains_key(key)
}

/// 惰性建连：树上展开到某个库才为该库开连接。
pub fn ensure(key: &str, p: &Params) -> Result<(), String> {
    if is_open(key) {
        return Ok(());
    }
    open(key, p)
}

/// 关一条库连接（树上收起某个库时）。只从注册表摘句柄，不等在飞的查询：
/// 那条查询手里还捏着 `Arc`，跑完自己释放。
pub fn close(key: &str) {
    let removed = CONNS.lock().unwrap().remove(key);
    drop(removed);
}

/// 关某连接名下所有库的连接（断开 / 删除 / 改名）。
pub fn close_prefix(conn: &str) {
    let head = prefix(conn);
    let removed: Vec<_> = {
        let mut map = CONNS.lock().unwrap();
        let keys: Vec<String> = map
            .keys()
            .filter(|k| k.starts_with(&head))
            .cloned()
            .collect();
        keys.iter().filter_map(|k| map.remove(k)).collect()
    };
    drop(removed);
}

/// 服务端没回话的失败（socket 断了 / 被掐 / 连不上）：出了这种错，那条句柄
/// 之后每次查询都会连带失败。服务端回了 SQL 错误（`DbError`）说明通道是通的，
/// 不算这一类。
fn is_transport(e: &Error) -> bool {
    e.as_db_error().is_none()
}

/// PostgreSQL 的错误要能读懂：rust-postgres 的 Display 只有「db error」，
/// 真正的信息在 DbError 里；传输层错误反过来只有一句
/// 「error communicating with the server」，真正的原因在 cause 里。
fn err_text(e: &Error) -> String {
    match e.as_db_error() {
        Some(db) => {
            let mut s = format!("{}（SQLSTATE {}）", db.message(), db.code().code());
            if let Some(d) = db.detail() {
                s.push_str("；");
                s.push_str(d);
            }
            if let Some(h) = db.hint() {
                s.push_str("；提示：");
                s.push_str(h);
            }
            s
        }
        None => match StdError::source(e) {
            Some(cause) => format!("{e}：{cause}"),
            None => e.to_string(),
        },
    }
}

/// 「测试连接」专用：连上去跑一条 SELECT 1，不注册。
/// 服务端接了 TCP 却不回语句时会一直等，所以同样套目录超时。
pub fn probe(p: &Params) -> Result<(), String> {
    let mut client = connect(p)?;
    client
        .simple_query(&with_catalog_timeout("SELECT 1"))
        .map(|_| ())
        .map_err(|e| err_text(&e))
}

/// 注册表里取出句柄、锁住这条连接执行 `f`；注册表锁不跨 I/O。
fn with_conn<T>(key: &str, f: impl FnOnce(&mut Client) -> Result<T, String>) -> Result<T, String> {
    let client = handle(key)?;
    let mut client = client.lock().unwrap();
    f(&mut client)
}

/// 把一条只读语句包成「事务 + `SET LOCAL statement_timeout`」的一批语句：
/// 超时随事务结束自动失效，不会漏给后续取数。`simple_query` 支持一次发多条，
/// 行结果只来自其中的那条 SELECT（其余是 CommandComplete，first_cols 会跳过）。
fn with_catalog_timeout(sql: &str) -> String {
    format!("BEGIN; SET LOCAL statement_timeout = '{CATALOG_TIMEOUT_SECS}s'; {sql}; COMMIT")
}

/// 目录类查询（库 / 模式 / 对象 / 列元数据 / 补全）统一走这里。
/// 传输层失败会把那条句柄一并摘掉：留着它，这个库之后每次查询都连带失败，
/// 用户只能断开重连；摘掉后下一次 `ensure` 自然重连。
fn catalog_query(key: &str, sql: &str) -> Result<Vec<SimpleQueryMessage>, String> {
    let batch = with_catalog_timeout(sql);
    let client = handle(key)?;
    let err = {
        let mut client = client.lock().unwrap();
        match client.simple_query(&batch) {
            Ok(msgs) => return Ok(msgs),
            Err(e) => {
                // 批次里报错会把连接留在「事务已中止」态，不收掉后续语句全遭连带
                let _ = client.batch_execute("ROLLBACK");
                e
            }
        }
    };
    let text = err_text(&err);
    if is_transport(&err) {
        // 只摘掉自己这条：期间可能已被「断开 + 重开」换成了新句柄
        let mut map = CONNS.lock().unwrap();
        if map.get(key).is_some_and(|c| Arc::ptr_eq(c, &client)) {
            map.remove(key);
        }
    }
    Err(text)
}

/// 目录类查询的单列文本清单。
fn catalog_list(key: &str, sql: &str) -> Result<Vec<String>, String> {
    Ok(first_cols(&catalog_query(key, sql)?))
}

// ── SQL 片段 ─────────────────────────────────────────────────────

/// 引号包标识符（PG 与 SQLite 同形：双引号，内部 " 双写）。
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn qualify(schema: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(schema), quote_ident(table))
}

/// 单引号字面量（内部 ' 双写）。
fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// `'schema"."table'` 形态的标识符，喂给 `::regclass`。两段都加引号，
/// 免得大小写被 PG 折成小写、或对上了关键字。
fn regclass(schema: &str, table: &str) -> String {
    let dq = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    lit(&format!("{}.{}", dq(schema), dq(table)))
}

/// PostgreSQL 的类型名 → 数值列判定（右对齐 + 数值排序）。目录里的
/// `format_type` 文案（bigint / numeric(6,2)）和 prepare 给的类型名
/// （int8 / numeric）都覆盖。
fn ty_is_numeric(ty: &str) -> bool {
    let t = ty.to_ascii_uppercase();
    [
        "INT", "NUM", "DEC", "REAL", "FLOAT", "DOUBLE", "MONEY", "SERIAL", "OID",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// 行数上限之类的整数夹到 int8 能装的范围。
fn cap(n: usize) -> usize {
    n.min(i64::MAX as usize)
}

fn first_cols(msgs: &[SimpleQueryMessage]) -> Vec<String> {
    msgs.iter()
        .filter_map(|m| match m {
            SimpleQueryMessage::Row(r) => r.get(0).map(str::to_string),
            _ => None,
        })
        .collect()
}

// ── 目录清单 ─────────────────────────────────────────────────────

/// 服务器上的库清单（模板库不列，与参考界面一致）。
pub fn try_list_databases(key: &str) -> Result<Vec<String>, String> {
    catalog_list(
        key,
        "SELECT datname FROM pg_database WHERE NOT datistemplate ORDER BY 1",
    )
}

const SCHEMA_SQL: &str = "SELECT n.nspname FROM pg_namespace n \
                          WHERE n.nspname NOT LIKE 'pg\\_%' AND n.nspname <> 'information_schema' \
                          ORDER BY 1";

/// 某库的模式清单（系统模式不列）。
pub fn try_list_schemas(key: &str) -> Result<Vec<String>, String> {
    catalog_list(key, SCHEMA_SQL)
}

fn objects_sql(schema: &str, relkinds: &str) -> String {
    format!(
        "SELECT c.relname FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = {} AND c.relkind IN ({relkinds}) ORDER BY 1",
        lit(schema)
    )
}

/// 普通表 + 分区主表（relkind r / p）。
pub fn try_list_tables(key: &str, schema: &str) -> Result<Vec<String>, String> {
    catalog_list(key, &objects_sql(schema, "'r','p'"))
}

/// 视图清单。
pub fn try_list_views(key: &str, schema: &str) -> Result<Vec<String>, String> {
    catalog_list(key, &objects_sql(schema, "'v'"))
}

/// 物化视图清单。
pub fn try_list_matviews(key: &str, schema: &str) -> Result<Vec<String>, String> {
    catalog_list(key, &objects_sql(schema, "'m'"))
}

/// 函数 / 存储过程清单（同名重载在树上只出现一次）。
pub fn try_list_routines(key: &str, schema: &str) -> Result<Vec<String>, String> {
    let sql = format!(
        "SELECT DISTINCT p.proname FROM pg_proc p \
         JOIN pg_namespace n ON n.oid = p.pronamespace \
         WHERE n.nspname = {} AND p.prokind IN ('f','p') ORDER BY 1",
        lit(schema)
    );
    catalog_list(key, &sql)
}

/// 表 / 视图的列元数据：类型文案走 `format_type`，生成列与标识列标只读，
/// 主键列打标（写回就按这些列定位行）。
pub fn try_table_columns(key: &str, schema: &str, table: &str) -> Result<Vec<ColMeta>, String> {
    Ok(cols_from_msgs(&catalog_query(
        key,
        &table_columns_sql(schema, table),
    )?))
}

fn table_columns_sql(schema: &str, table: &str) -> String {
    format!(
        "SELECT a.attname, format_type(a.atttypid, a.atttypmod), a.attgenerated, \
                a.attidentity, coalesce(i.indisprimary, false) \
         FROM pg_attribute a \
         LEFT JOIN pg_index i ON i.indrelid = a.attrelid AND a.attnum = ANY(i.indkey) \
                                    AND i.indisprimary \
         WHERE a.attrelid = {}::regclass AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
        regclass(schema, table)
    )
}

/// 列元数据 SQL 的执行（取数路径用，不套目录超时：表页本身可中断）。
fn table_columns_raw(
    client: &mut Client,
    schema: &str,
    table: &str,
) -> Result<Vec<ColMeta>, String> {
    let msgs = client
        .simple_query(&table_columns_sql(schema, table))
        .map_err(|e| err_text(&e))?;
    Ok(cols_from_msgs(&msgs))
}

fn cols_from_msgs(msgs: &[SimpleQueryMessage]) -> Vec<ColMeta> {
    let mut cols = Vec::new();
    for m in msgs {
        let SimpleQueryMessage::Row(r) = m else { continue };
        let ty = r.get(1).unwrap_or("").to_string();
        let mut meta = ColMeta::new(r.get(0).unwrap_or(""), ty.clone(), ty_is_numeric(&ty));
        // GENERATED ALWAYS AS ... STORED / VIRTUAL 与 IDENTITY 都由服务端算，
        // 写回只能跳过
        let generated = matches!(r.get(2), Some("s") | Some("v"));
        let identity = matches!(r.get(3), Some("a") | Some("d"));
        meta.writable = !generated && !identity;
        meta.pk = r.get(4) == Some("t");
        cols.push(meta);
    }
    cols
}

/// 某模式下的「表 → 列」，查询编辑器补全用；一次查完，避免逐表往返。
pub fn all_columns_by_table(key: &str, schema: &str) -> Vec<(String, Vec<String>)> {
    let sql = format!(
        "SELECT c.relname::text, a.attname::text FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = {} AND c.relkind IN ('r','p','v','m','f') \
           AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY 1, a.attnum",
        lit(schema)
    );
    let Ok(msgs) = catalog_query(key, &sql) else {
        return Vec::new();
    };
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for msg in msgs {
        let SimpleQueryMessage::Row(row) = msg else {
            continue;
        };
        let (Some(table), Some(col)) = (row.get(0), row.get(1)) else {
            continue;
        };
        match out.last_mut() {
            Some((name, cols)) if name == table => cols.push(col.to_string()),
            _ => out.push((table.to_string(), vec![col.to_string()])),
        }
    }
    out
}


// ── 行取数 ───────────────────────────────────────────────────────

/// 表页取数的 SQL（状态栏也展示这条）。列名来自目录，顺序与 `cols` 一致。
///
/// 无排序时补主键排序：PG 不保证没有 ORDER BY 的行序，翻页就会重复 / 漏行。
pub fn select_table_sql(
    cols: &[ColMeta],
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
        qualify(schema, table),
        cap(limit),
        cap(offset)
    )
}

/// 表页取数：列元数据来自目录（带主键 / 只读标记），行身份按主键列的值拼；
/// 没有主键的表行身份为空向量，改 / 删会被拒。
#[allow(clippy::too_many_arguments)]
pub fn select_table(
    key: &str,
    schema: &str,
    table: &str,
    where_sql: Option<&str>,
    orders: &[(String, bool)],
    offset: usize,
    limit: usize,
) -> Result<SelectPage, String> {
    with_conn(key, |client| {
        let cols = table_columns_raw(client, schema, table)?;
        let pk: Vec<usize> = cols
            .iter()
            .enumerate()
            .filter(|(_, c)| c.pk)
            .map(|(i, _)| i)
            .collect();
        let sql = select_table_sql(&cols, schema, table, where_sql, orders, limit, offset);
        take_page(client, &sql, cols, &pk, usize::MAX)
    })
}

/// 当前筛选条件下的总行数（远程分页的「共 n 条」与末页判断）。
pub fn count_table(
    key: &str,
    schema: &str,
    table: &str,
    where_sql: Option<&str>,
) -> Result<usize, String> {
    with_conn(key, |client| {
        let where_part = where_sql.map(|w| format!(" WHERE {w}")).unwrap_or_default();
        let sql = format!("SELECT COUNT(*) FROM {}{where_part}", qualify(schema, table));
        let msgs = client
            .simple_query(&sql)
            .map_err(|e| err_text(&e))?;
        Ok(first_cols(&msgs)
            .first()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0))
    })
}

/// 任意 SELECT 语句的结果（查询页）。
pub fn select_sql(key: &str, stmt: &str, max_rows: usize) -> Result<SelectPage, String> {
    with_conn(key, |client| {
        let cols = describe_columns(client, stmt);
        take_page(client, stmt, cols, &[], max_rows)
    })
}

/// 跑一条 SELECT 装配成一页：值全按服务端给的文本原样收。
/// `columns` 为空时（prepare 也问不到类型的语句）退化成只认行描述里的列名。
///
/// 查询页（`max_rows` 有限）走服务端游标：`FETCH` 只把前 `max_rows + 1` 行
/// 收进客户端，结果再大也不会整包塞进内存。
fn take_page(
    client: &mut Client,
    sql: &str,
    columns: Vec<ColMeta>,
    pk: &[usize],
    max_rows: usize,
) -> Result<SelectPage, String> {
    if max_rows != usize::MAX
        && let Some(page) = take_page_streamed(client, sql, columns.clone(), pk, max_rows)?
    {
        return Ok(page);
    }
    let msgs = client.simple_query(sql).map_err(|e| err_text(&e))?;
    Ok(page_from_messages(&msgs, columns, pk, max_rows))
}

/// 游标取数：语句能在游标里跑就只 FETCH `max_rows + 1` 行，返回 Some；
/// DECLARE 被服务端拒绝（EXPLAIN / SHOW / 带写操作的 CTE 等）时返回 None，
/// 交给整包路径——DECLARE 失败时语句还没执行，重跑不会重复产生副作用。
fn take_page_streamed(
    client: &mut Client,
    sql: &str,
    columns: Vec<ColMeta>,
    pk: &[usize],
    max_rows: usize,
) -> Result<Option<SelectPage>, String> {
    static NEXT_CURSOR: AtomicU64 = AtomicU64::new(1);
    let cursor = format!("daro_page_{}", NEXT_CURSOR.fetch_add(1, Ordering::Relaxed));
    let savepoint = format!("{cursor}_sp");
    // 连接可能正处在用户脚本的事务里：有事务就建保存点，没有就自己开一个；
    // 收尾都回到进来时的状态（ROLLBACK 只撤本次取数）。
    let in_tx = client
        .batch_execute(&format!("SAVEPOINT {savepoint}"))
        .is_ok();
    if !in_tx {
        client.batch_execute("BEGIN").map_err(|e| err_text(&e))?;
    }
    if client
        .batch_execute(&format!("DECLARE {cursor} NO SCROLL CURSOR FOR {sql}"))
        .is_err()
    {
        end_stream_scope(client, in_tx, &savepoint);
        return Ok(None);
    }
    let msgs = client.simple_query(&format!(
        "FETCH FORWARD {} FROM {cursor}",
        max_rows.saturating_add(1)
    ));
    end_stream_scope(client, in_tx, &savepoint);
    let msgs = msgs.map_err(|e| err_text(&e))?;
    Ok(Some(page_from_messages(&msgs, columns, pk, max_rows)))
}

/// 退出取数作用域：在用户事务里回滚到保存点并释放，否则回滚本次开的临时事务。
fn end_stream_scope(client: &mut Client, in_tx: bool, savepoint: &str) {
    if in_tx {
        let _ = client.batch_execute(&format!("ROLLBACK TO SAVEPOINT {savepoint}"));
        let _ = client.batch_execute(&format!("RELEASE SAVEPOINT {savepoint}"));
    } else {
        let _ = client.batch_execute("ROLLBACK");
    }
}

/// 把 simple 协议的消息装成一张结果页（超出 `max_rows` 标截断）。
fn page_from_messages(
    msgs: &[SimpleQueryMessage],
    columns: Vec<ColMeta>,
    pk: &[usize],
    max_rows: usize,
) -> SelectPage {
    let mut columns = columns;
    if columns.is_empty() {
        let described = msgs.iter().find_map(|m| match m {
            SimpleQueryMessage::RowDescription(cs) => Some(
                cs.iter()
                    .map(|c| {
                        let mut m = ColMeta::new(c.name(), "", false);
                        m.writable = false;
                        m
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        });
        columns = dedup_names(described.unwrap_or_default());
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut keys: Vec<RowKey> = Vec::new();
    let mut truncated = false;
    for m in msgs {
        let SimpleQueryMessage::Row(r) = m else { continue };
        if rows.len() >= max_rows {
            truncated = true;
            break;
        }
        let row: Vec<String> = (0..columns.len())
            .map(|i| r.get(i).unwrap_or(NULL_TEXT).to_string())
            .collect();
        keys.push(pk.iter().filter_map(|i| row.get(*i)).cloned().collect());
        rows.push(row);
    }
    SelectPage {
        columns,
        rows,
        keys,
        truncated,
    }
}

/// 查询页结果的列形。simple 协议的行描述只有列名，类型要另问一次 prepare。
/// rust-postgres 不公开预备语句的名字，问完只能整条连接一起清
/// （`DEALLOCATE ALL`）——本驱动只在取数 / 写回时临时 prepare，不缓存语句，
/// 全清不会误伤别处；写回那一头本来也是收尾统一 DEALLOCATE ALL。
fn describe_columns(client: &mut Client, sql: &str) -> Vec<ColMeta> {
    let Ok(st) = client.prepare(sql) else {
        return Vec::new();
    };
    let cols: Vec<ColMeta> = st
        .columns()
        .iter()
        .map(|c| {
            let ty = c.type_().name().to_string();
            let numeric = ty_is_numeric(&ty);
            let mut m = ColMeta::new(c.name().to_string(), ty, numeric);
            m.writable = false;
            m
        })
        .collect();
    let _ = client.batch_execute("DEALLOCATE ALL");
    dedup_names(cols)
}

/// 列名去重：`SELECT a, a` 会给出两个同名列，网格按名字筛选 / 排序要能区分
/// （与 SQLite 档同口径的 `name:n` 后缀）。
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

/// 顶层出结果的语句粗判（与 SQLite 档同一套启发式，方言词换成 PG 有的）。
pub fn query_is_select(stmt: &str) -> bool {
    let head = stmt
        .trim_start()
        .trim_start_matches('(')
        .trim_start()
        .to_ascii_lowercase();
    ["select", "with", "explain", "values", "table", "show"]
        .iter()
        .any(|p| head.starts_with(p))
}

// ── 语句执行 ─────────────────────────────────────────────────────

/// 执行一条语句（调用方已按分号切好）。SELECT 出结果页，其余出影响行数。
pub fn execute_stmt(key: &str, stmt: &str, max_rows: usize) -> Result<ExecOutcome, String> {
    with_conn(key, |client| execute_on(client, stmt, max_rows))
}

/// 同 `execute_stmt`，但直接吃连接：后台脚本线程自己开连接，不碰注册表锁。
pub fn execute_on(client: &mut Client, stmt: &str, max_rows: usize) -> Result<ExecOutcome, String> {
    let trimmed = stmt.trim();
    if query_is_select(trimmed) {
        let cols = describe_columns(client, trimmed);
        let page = take_page(client, trimmed, cols, &[], max_rows)?;
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
        let msgs = client
            .simple_query(trimmed)
            .map_err(|e| err_text(&e))?;
        let affected: usize = msgs
            .iter()
            .filter_map(|m| match m {
                SimpleQueryMessage::CommandComplete(n) => Some(*n as usize),
                _ => None,
            })
            .sum();
        Ok(ExecOutcome {
            select: None,
            affected,
            message: format!("Query OK, {affected} rows affected"),
        })
    }
}

// ── 后台脚本执行（查询页「运行」）───────────────────────────────

/// 一次后台脚本的取消通道。PG 的中断走服务端：`CancelToken` 带着这条
/// backend 的 pid + secret，另开一条连接发取消请求，正在跑的语句会回
/// 「canceling statement due to user request」。
#[derive(Clone)]
pub struct Job {
    id: u64,
    cancel: Arc<AtomicBool>,
}

impl Job {
    /// 只置标志：不碰服务器，UI 线程直接调用也安全。
    pub fn request_stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// 另开一条连接发取消请求；要连服务器，可能阻塞，放到后台线程调。
    pub fn interrupt(&self) {
        // 先摘出令牌再发请求：cancel_query 要连服务器，抱着锁等不划算
        let token = RUNNING_JOBS.lock().unwrap().get(&self.id).cloned();
        if let Some(token) = token {
            let _ = token.cancel_query(NoTls);
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
pub fn create_job() -> Job {
    let id = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
    Job {
        id,
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

static RUNNING_JOBS: LazyLock<Mutex<HashMap<u64, postgres::CancelToken>>> =
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

/// PG 事务中止后的连带报错。
fn tx_aborted(msg: &str) -> bool {
    msg.contains("current transaction is aborted")
}

/// 逐条执行脚本，支持「停止」；`continue_on_error=false` 时第一条报错就收手。
///
/// 与其它档的差异：报错撞上事务时 PG 已把事务标成中止，后续语句必然
/// 连带失败，所以这里无论是否勾「遇到错误时继续」都回滚并停下
/// （`aborted_tx`），而不是一条条攒假错误。
pub fn run_script(
    job: &Job,
    p: &Params,
    stmts: &[String],
    max_rows: usize,
    continue_on_error: bool,
) -> ScriptRun {
    let started = Instant::now();
    let mut client = match connect(p) {
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
        .insert(job.id, client.cancel_token());
    let mut runs = Vec::with_capacity(stmts.len());
    let mut cancelled = false;
    let mut rolled_back = false;
    let mut aborted_tx = false;
    let mut tx_open = false;
    for stmt in stmts {
        if job.is_cancelled() {
            cancelled = true;
            break;
        }
        let sw = Instant::now();
        let result = execute_on(&mut client, stmt, max_rows);
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
        if let Some(msg) = err {
            if tx_open || tx_aborted(&msg) {
                let _ = client.batch_execute("ROLLBACK");
                tx_open = false;
                rolled_back = true;
                aborted_tx = true;
                cancelled = true;
                break;
            }
            if !continue_on_error {
                break;
            }
        }
    }
    // 脚本停在 BEGIN 里没提交：别把事务挂在连接上（连接虽然后续即弃，
    // 但中止态会污染同一脚本剩下的语句，UI 也要据此提示）
    if tx_open {
        rolled_back = client.batch_execute("ROLLBACK").is_ok();
    }
    RUNNING_JOBS.lock().unwrap().remove(&job.id);
    ScriptRun {
        runs,
        open_error: None,
        cancelled,
        rolled_back,
        aborted_tx,
        total_ms: started.elapsed().as_millis(),
    }
}

// ── 网格写回（apply）────────────────────────────────────────────

/// 单元格文本 → 绑定值（全部按 text 送，None 即 SQL NULL）。约定与 SQLite 档同：
/// 字面 `NULL` → NULL；单引号包住 → 去引号的纯文本；数值列空串 → NULL。
fn value_for(meta: &ColMeta, v: &str) -> Option<String> {
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

/// `$n::text::<列类型>`：参数按 text 送、服务端再显式转成目标类型。
/// 类型文案来自 `format_type`，只认字母 / 数字 / 空格 / 括号 / 逗号 / 下划线，
/// 其余形态（理论上不该出现）退化成 `::text` 让 PG 自己按赋值转换。
fn cast_expr(n: usize, ty: &str) -> String {
    let clean = !ty.is_empty()
        && ty
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '(' | ')' | ',' | '[' | ']'));
    if clean {
        format!("${n}::text::{ty}")
    } else {
        format!("${n}::text")
    }
}

/// 按 text 送参数执行一条写语句，返回影响行数。
/// `&str` 语句在服务端是预备语句，整批写完统一 DEALLOCATE（见 apply_changes）。
fn exec_bound(
    tx: &mut Transaction<'_>,
    sql: &str,
    args: &[Option<String>],
) -> Result<usize, String> {
    let refs: Vec<&(dyn ToSql + Sync)> = args
        .iter()
        .map(|a| a as &(dyn ToSql + Sync))
        .collect();
    tx.execute(sql, &refs).map(|n| n as usize).map_err(|e| err_text(&e))
}

/// 主键列（顺序即行身份里各段的顺序）。
fn pk_cols(columns: &[ColMeta]) -> Vec<&ColMeta> {
    columns.iter().filter(|c| c.pk).collect()
}

/// 按主键定位的 WHERE 片段；`from` 是主键值对应的起始参数号。
fn pk_where(pk: &[&ColMeta], from: usize) -> String {
    pk.iter()
        .enumerate()
        .map(|(i, c)| format!("{} = {}", quote_ident(&c.name), cast_expr(from + i, &c.ty)))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// 把网格工作集写回真库：UPDATE / DELETE 按主键定位，INSERT 只写可写列，
/// 全部包在一个事务里，任何一条失败整体回滚。
///
/// 影响行数为 0 视为「这行已经不是当初那行了」（别的客户端改过 / 删过），
/// 直接报错回滚，而不是默默少写一行。
pub fn apply_changes(key: &str, schema: &str, ch: &GridChanges<'_>) -> Result<String, String> {
    with_conn(key, |client| {
        let kinds = client
            .simple_query(&format!(
                "SELECT c.relkind FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = {} AND c.relname = {}",
                lit(schema),
                lit(ch.table)
            ))
            .map_err(|e| err_text(&e))?;
        match first_cols(&kinds).into_iter().next().as_deref() {
            None => {
                return Err(format!(
                    "「{schema}.{}」已不存在（结构被改过？请刷新）",
                    ch.table
                ));
            }
            Some("v") | Some("m") => {
                return Err(format!(
                    "「{}」是视图 / 物化视图，不能直接编辑；请修改基表",
                    ch.table
                ));
            }
            _ => {}
        }
        let pk = pk_cols(ch.columns);
        if (!ch.updates.is_empty() || !ch.deletes.is_empty()) && pk.is_empty() {
            return Err("该表没有主键，不支持编辑和删除写回".to_string());
        }
        // UPDATE 的 $1 是列值，主键从 $2 起；DELETE 没有列值，主键从 $1 起
        let update_where = pk_where(&pk, 2);
        let delete_where = pk_where(&pk, 1);
        let mut tx = client.transaction().map_err(|e| err_text(&e))?;
        let mut touched = 0usize;
        for (key_vals, col, val) in ch.updates {
            let Some(meta) = ch.columns.get(*col) else {
                continue;
            };
            if !meta.writable {
                return Err(format!(
                    "列「{}」是生成列 / 标识列，不能写回",
                    meta.name
                ));
            }
            if key_vals.len() != pk.len() {
                return Err("行标识与当前主键不匹配，请刷新页签".to_string());
            }
            let sql = format!(
                "UPDATE {} SET {} = {} WHERE {update_where}",
                qualify(schema, ch.table),
                quote_ident(&meta.name),
                cast_expr(1, &meta.ty)
            );
            let mut args: Vec<Option<String>> = vec![value_for(meta, val)];
            args.extend(key_vals.iter().map(|v| Some(v.clone())));
            let n = exec_bound(&mut tx, &sql, &args)?;
            if n == 0 {
                return Err("按主键没找到该行（可能已被其它客户端改动或删除），请刷新".to_string());
            }
            touched += n;
        }
        for key_vals in ch.deletes {
            if key_vals.len() != pk.len() {
                return Err("行标识与当前主键不匹配，请刷新页签".to_string());
            }
            let sql = format!(
                "DELETE FROM {} WHERE {delete_where}",
                qualify(schema, ch.table)
            );
            let args: Vec<Option<String>> = key_vals.iter().map(|v| Some(v.clone())).collect();
            let n = exec_bound(&mut tx, &sql, &args)?;
            if n == 0 {
                return Err("按主键没找到该行（可能已被其它客户端删除），请刷新".to_string());
            }
            touched += n;
        }
        if !ch.inserts.is_empty() {
            // 只写可写列：生成列 / 标识列由服务端算，写它反而报错
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
                let holders: Vec<String> = idx
                    .iter()
                    .enumerate()
                    .filter_map(|(n, i)| ch.columns.get(*i).map(|c| cast_expr(n + 1, &c.ty)))
                    .collect();
                let args: Vec<Option<String>> = idx
                    .iter()
                    .filter_map(|i| ch.columns.get(*i).map(|c| (*i, c)))
                    .map(|(i, c)| value_for(c, row.get(i).map(String::as_str).unwrap_or("")))
                    .collect();
                let sql = if names.is_empty() {
                    format!("INSERT INTO {} DEFAULT VALUES", qualify(schema, ch.table))
                } else {
                    format!(
                        "INSERT INTO {} ({}) VALUES ({})",
                        qualify(schema, ch.table),
                        names.join(", "),
                        holders.join(", ")
                    )
                };
                exec_bound(&mut tx, &sql, &args)?;
                touched += 1;
            }
        }
        tx.commit().map_err(|e| err_text(&e))?;
        // 上面每条写语句都在服务端留下了预备语句，一次清干净
        let _ = client.batch_execute("DEALLOCATE ALL");
        Ok(format!("已写回 {touched} 行"))
    })
}

// ── 跨库复制表用的独立会话 ────────────────────────────────────────

/// 跨库 / 跨连接复制表时自己开一条连接：不占注册表里的句柄。复制一张大表
/// 可能搬很久，不能把 UI 正在用的那条连接锁住。
///
/// PG 没有「跨库查询」这类服务端语法，跨库复制只能由客户端做：从源库读结构、
/// 在目标库建表，再经 COPY 文本流把行搬过去（与参考客户端的 transfer 同路）。
pub struct CopySession {
    client: Client,
}

impl CopySession {
    pub fn connect(p: &Params) -> Result<CopySession, String> {
        connect(p).map(|client| CopySession { client })
    }

    /// 执行一条语句（BEGIN / COMMIT / DDL / COMMENT），不取结果。
    pub fn exec(&mut self, sql: &str) -> Result<(), String> {
        self.client.batch_execute(sql).map_err(|e| err_text(&e))
    }

    /// 简单查询，所有列按文本取回（NULL 变空串；调用方自己用 COALESCE 区分）。
    pub fn query(&mut self, sql: &str) -> Result<Vec<Vec<String>>, String> {
        let msgs = self.client.simple_query(sql).map_err(|e| err_text(&e))?;
        Ok(msgs
            .into_iter()
            .filter_map(|m| match m {
                SimpleQueryMessage::Row(r) => {
                    Some((0..r.len()).map(|i| r.get(i).unwrap_or("").to_string()).collect())
                }
                _ => None,
            })
            .collect())
    }

    /// 把 `from`（`COPY … TO STDOUT`）的文本流原样灌进 `into`
    /// （`COPY … FROM STDIN`）。两端都走文本格式，不逐类型解码。
    pub fn copy_from(
        &mut self,
        src: &mut CopySession,
        from: &str,
        into: &str,
    ) -> Result<u64, String> {
        let mut reader = src.client.copy_out(from).map_err(|e| err_text(&e))?;
        let mut writer = self.client.copy_in(into).map_err(|e| err_text(&e))?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("从源库读数据失败：{e}"))?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buf[..n])
                .map_err(|e| format!("写入目标库失败：{e}"))?;
        }
        writer.finish().map_err(|e| err_text(&e))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn key_pairs_conn_with_database_and_prefix_covers_all_dbs() {
        assert_eq!(key("pg", "daro_pg_test"), "pg\u{1}daro_pg_test");
        assert_eq!(prefix("pg"), "pg\u{1}");
        assert!(key("pg", "any").starts_with(&prefix("pg")));
        // 连接名里带分隔符也不串号：另一台同名服务器的键不会落进同一前缀
        assert!(!key("pg\u{1}x", "y").starts_with(&prefix("pg\u{1}z")));
    }

    #[test]
    fn format_type_names_and_prepared_names_both_count_as_numeric() {
        // 目录里 format_type 的文案
        for ty in ["bigint", "integer", "smallint", "numeric(6,2)", "double precision", "real"] {
            assert!(ty_is_numeric(ty), "{ty} 应判为数值列");
        }
        // prepare 回来的内部类型名
        for ty in ["int8", "int4", "numeric", "float8"] {
            assert!(ty_is_numeric(ty), "{ty} 应判为数值列");
        }
        for ty in ["text", "boolean", "date", "timestamptz", "jsonb", "uuid", "bytea"] {
            assert!(!ty_is_numeric(ty), "{ty} 不该判为数值列");
        }
    }

    #[test]
    fn casts_are_double_stepped_from_text_and_guarded_on_type_text() {
        // 正常形态：先声明参数是 text，再显式转成目标类型
        assert_eq!(cast_expr(2, "bigint"), "$2::text::bigint");
        assert_eq!(cast_expr(1, "timestamp with time zone"), "$1::text::timestamp with time zone");
        assert_eq!(cast_expr(3, "integer[]"), "$3::text::integer[]");
        // 类型文案不认识时退回 ::text，交给 PG 按赋值转换
        assert_eq!(cast_expr(1, ""), "$1::text");
        assert_eq!(cast_expr(1, "int; DROP TABLE x"), "$1::text");
    }

    #[test]
    fn cell_text_maps_to_null_quoted_and_blank_like_sqlite() {
        let num = ColMeta::new("score", "numeric(6,2)", true);
        let txt = ColMeta::new("name", "text", false);
        assert_eq!(value_for(&num, "NULL"), None);
        assert_eq!(value_for(&num, ""), None); // 数值列空串 = NULL
        assert_eq!(value_for(&num, "60.5").as_deref(), Some("60.5"));
        assert_eq!(value_for(&txt, "").as_deref(), Some("")); // 文本列空串保留
        assert_eq!(value_for(&txt, "'NULL'").as_deref(), Some("NULL")); // 引号内是字面文本
        assert_eq!(value_for(&txt, "'  留白  '").as_deref(), Some("  留白  "));
        // 只有整格被引号包住才是「文本里带引号」的表示，内部 '' 还原成 '
        assert_eq!(value_for(&txt, "'O''B'").as_deref(), Some("O'B"));
        assert_eq!(value_for(&txt, "O''B").as_deref(), Some("O''B"));
    }

    #[test]
    fn table_sql_pushes_where_order_and_pages_by_pk_when_unsorted() {
        let cols = vec![
            {
                let mut c = ColMeta::new("id", "bigint", true);
                c.pk = true;
                c
            },
            ColMeta::new("name", "text", false),
        ];
        // 形参顺序与 SQLite 档一致：limit 在前、offset 在后
        let sql = select_table_sql(&cols, "public", "member", None, &[], 20, 10);
        assert_eq!(
            sql,
            "SELECT \"id\", \"name\" FROM \"public\".\"member\" ORDER BY \"id\" ASC LIMIT 20 OFFSET 10"
        );
        // 无排序时补主键排序，翻页才不会重复 / 漏行；ORDER BY 必须在 LIMIT 之前
        let sorted = select_table_sql(
            &cols,
            "public",
            "member",
            Some("\"score\" > 80"),
            &[("score".to_string(), false)],
            0,
            100,
        );
        assert!(sorted.contains("WHERE \"score\" > 80"), "{sorted}");
        assert!(sorted.contains("ORDER BY \"score\" DESC"), "{sorted}");
        assert!(!sorted.contains("\"id\" ASC"), "显式排序优先：{sorted}");
    }

    #[test]
    fn identifiers_and_literals_escape() {
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(lit("it's"), "'it''s'");
        // regclass 两段都加引号，大小写才不被折成小写
        assert_eq!(regclass("Public", "MyTable"), "'\"Public\".\"MyTable\"'");
        assert_eq!(qualify("public", "member"), "\"public\".\"member\"");
    }

    #[test]
    fn duplicate_result_columns_get_index_suffix() {
        let cols = vec![
            ColMeta::new("id", "int4", true),
            ColMeta::new("id", "int4", true),
            ColMeta::new("id", "int4", true),
        ];
        let out = dedup_names(cols);
        let names: Vec<&str> = out.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["id", "id:1", "id:2"]);
    }

    #[test]
    fn tx_state_follows_pg_keywords_not_just_begin() {
        assert_eq!(tx_toggle("BEGIN"), Some(true));
        assert_eq!(tx_toggle("  start transaction "), Some(true));
        assert_eq!(tx_toggle("COMMIT WORK"), Some(false));
        assert_eq!(tx_toggle("END"), Some(false));
        assert_eq!(tx_toggle("ROLLBACK"), Some(false));
        // 回滚到保存点：事务还在
        assert_eq!(tx_toggle("ROLLBACK TO SAVEPOINT sp"), None);
        assert_eq!(tx_toggle("SELECT 1"), None);
    }

    #[test]
    fn result_shaped_statements_recognised_in_pg_dialect() {
        for s in ["SELECT 1", " with x as (select 1) select * from x", "EXPLAIN SELECT 1", "VALUES (1)", "SHOW server_version", "(SELECT 1)"] {
            assert!(query_is_select(s), "{s}");
        }
        for s in ["INSERT INTO t VALUES (1)", "CREATE TABLE t(a int)", "UPDATE t SET a = 1"] {
            assert!(!query_is_select(s), "{s}");
        }
    }

    // ── 真机（datawalk 开发服务器 + 独立测试库）──────────────────

    /// 独立测试库名：每轮先删后建，绝不碰业务库。
    pub(crate) const TEST_DB: &str = "daro_pg_test";

    /// 连接凭据从 datawalk 的开发配置就地读：不进命令行、不进本仓库副本。
    pub(crate) fn datawalk_params() -> Option<Params> {
        let path =
            "G:/Github/datawalk/datawalk-business-api/src/QiYun.WebApi.Business/appsettings.Development.json";
        let text = std::fs::read_to_string(path).ok()?;
        let at = text.find("\"Core\"")?;
        let after = &text[at..];
        let start = after.find(": \"")? + 3;
        let conn = &after[start..][..after[start..].find('"')?];
        let mut out = Params {
            host: String::new(),
            port: 5432,
            user: String::new(),
            password: String::new(),
            dbname: String::new(),
        };
        for part in conn.split(';') {
            let Some((k, v)) = part.split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim().to_ascii_lowercase().as_str() {
                "host" => out.host = v.to_string(),
                "port" => out.port = v.parse().unwrap_or(5432),
                "username" => out.user = v.to_string(),
                "password" => out.password = v.to_string(),
                _ => {}
            }
        }
        (!out.host.is_empty()).then_some(out)
    }

    /// 真机用例串行跑：共用同一台服务器上的同一个测试库。
    static PG_LOCK: Mutex<()> = Mutex::new(());

    /// 先删后建独立测试库并铺夹具，返回测试库参数。
    /// 夹具形态刻意覆盖驱动的每个边界：标识列主键、生成列、无主键表、
    /// 复合主键、视图 / 物化视图、函数、以及各种类型。
    pub(crate) fn reset_test_db() -> Option<Params> {
        let p = datawalk_params()?;
        let admin = "pg-test-admin";
        open(admin, &p.for_db("postgres")).expect("连不上 postgres 库（TLS 或凭据问题）");
        let drop = format!("DROP DATABASE IF EXISTS {TEST_DB} WITH (FORCE)");
        if execute_stmt(admin, &drop, 1).is_err() {
            // PG 13 以下不认 WITH (FORCE)：先踢掉占用会话再删
            let _ = execute_stmt(
                admin,
                &format!(
                    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{TEST_DB}'"
                ),
                1,
            );
            execute_stmt(admin, &format!("DROP DATABASE IF EXISTS {TEST_DB}"), 1).ok();
        }
        execute_stmt(admin, &format!("CREATE DATABASE {TEST_DB}"), 1).expect("建独立测试库失败");
        close(admin);

        let tp = p.for_db(TEST_DB);
        let k = key("pg-fixture", TEST_DB);
        open(&k, &tp).expect("连测试库失败");
        for stmt in [
            "CREATE TABLE member(id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, \
                      name text NOT NULL, score numeric(6,2), joined timestamptz, note text)",
            "INSERT INTO member(name, score, joined, note) VALUES \
                ('阿离', 91.5, '2026-01-02 03:04:05+08', NULL), \
                ('北辰', 77.0, '2026-02-03 04:05:06+08', '带''引号'), \
                ('青梧', NULL, NULL, 'utf8 中文 ok')",
            "CREATE TABLE gen(k int PRIMARY KEY, a int, b int GENERATED ALWAYS AS (a * 2) STORED)",
            "INSERT INTO gen(k, a) VALUES (1, 21), (2, 22)",
            "CREATE TABLE nopath(x text, y text)",
            "INSERT INTO nopath VALUES ('a', 'b'), ('a', 'b')",
            "CREATE TABLE pair(a int, b int, v text, PRIMARY KEY(a, b))",
            "INSERT INTO pair VALUES (1, 1, '甲'), (1, 2, '乙')",
            // 堆序（30,10,20）刻意与主键序相反：缺了隐式 ORDER BY 主键，
            // 顺序稳定的翻页就会露馅
            "CREATE TABLE shuffle(id int PRIMARY KEY, label text)",
            "INSERT INTO shuffle(id, label) VALUES (30, '三十'), (10, '十'), (20, '二十')",
            "CREATE TABLE wide(id int PRIMARY KEY, flag bool, d date, bin bytea, doc jsonb, \
                            u uuid, nums int[], big numeric(20,6), ts timestamp)",
            "CREATE VIEW v_member AS SELECT id, name FROM member WHERE score > 80",
            "CREATE MATERIALIZED VIEW mv_member AS SELECT id, name FROM member",
            "CREATE FUNCTION add2(x int) RETURNS int AS 'SELECT x + 2' LANGUAGE sql",
        ] {
            execute_stmt(&k, stmt, 1).unwrap_or_else(|e| panic!("夹具建表失败：{e}\n{stmt}"));
        }
        close(&k);
        Some(tp)
    }

    /// 给别的模块的真机用例用：同一把锁 + 重建测试库，返回测试库参数。
    /// 读不到开发库连接串时返回 None（调用方自行跳过）。
    pub(crate) fn pg_fixture() -> Option<(std::sync::MutexGuard<'static, ()>, Params)> {
        let guard = PG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let p = reset_test_db()?;
        Some((guard, p))
    }

    /// 真机用例的公共开场：拿锁 → 重建库 → 注册一条 UI 口径的连接。
    fn pg_setup() -> (std::sync::MutexGuard<'static, ()>, Params, String) {
        let guard = PG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let p = match reset_test_db() {
            Some(p) => p,
            None => {
                eprintln!("[pg] skip：读不到 datawalk 连接串");
                return (guard, Params { host: String::new(), port: 0, user: String::new(), password: String::new(), dbname: String::new() }, String::new());
            }
        };
        let k = key("真机测试", TEST_DB);
        close(&k);
        ensure(&k, &p).expect("连测试库失败");
        (guard, p, k)
    }

    #[test]
    #[ignore = "打 datawalk 开发库，需要网络可达"]
    fn transport_failure_evicts_the_dead_handle_so_the_next_call_reconnects() {
        let (_g, p, k) = pg_setup();
        try_list_schemas(&k).expect("开场连接应该是活的");

        // 从另一条连接把这个测试库的会话全踢掉，模拟服务端单方面断链
        // （真实场景就是网络抖一下 / DBA kill / 防火墙掐Idle）
        let kicker = key("pg-kicker", "postgres");
        ensure(&kicker, &p.for_db("postgres")).expect("连 postgres 库失败");
        let kick = format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{TEST_DB}' AND pid <> pg_backend_pid()"
        );
        catalog_list(&kicker, &kick).expect("踢不掉占用测试库的会话");
        close(&kicker);

        let e = try_list_schemas(&k).expect_err("被踢掉的句柄该报错");
        assert!(
            e.contains("error communicating with the server") || e.contains("connection closed"),
            "传输层错误要能读懂：{e}"
        );
        assert!(!is_open(&k), "坏句柄必须出局：留在注册表里这个库之后每次都失败");

        ensure(&k, &p).expect("下次用的时候应该重连上");
        assert!(try_list_schemas(&k).unwrap().contains(&"public".to_string()));
    }

    #[test]
    #[ignore = "打 datawalk 开发库，需要网络可达"]
    fn pg_catalog_reads_databases_schemas_objects_and_columns() {
        let (_g, _p, k) = pg_setup();
        let dbs = try_list_databases(&k).unwrap();
        assert!(dbs.iter().any(|d| d == TEST_DB), "独立测试库要在清单里：{dbs:?}");
        assert!(!dbs.iter().any(|d| d == "template1"), "模板库不列");

        let schemas = try_list_schemas(&k).unwrap();
        assert!(schemas.contains(&"public".to_string()), "{schemas:?}");
        assert!(!schemas.iter().any(|s| s.starts_with("pg_")), "系统模式不列");

        assert_eq!(
            try_list_tables(&k, "public").unwrap(),
            vec!["gen", "member", "nopath", "pair", "shuffle", "wide"]
        );
        assert_eq!(try_list_views(&k, "public").unwrap(), vec!["v_member"]);
        assert_eq!(try_list_matviews(&k, "public").unwrap(), vec!["mv_member"]);
        assert!(
            try_list_routines(&k, "public").unwrap().contains(&"add2".to_string()),
            "函数要出现在例程清单"
        );

        let cols = try_table_columns(&k, "public", "member").unwrap();
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["id", "name", "score", "joined", "note"]);
        assert!(cols[0].pk, "id 是主键");
        assert!(cols[0].numeric && cols[2].numeric, "bigint / numeric 右对齐");
        assert!(!cols[1].numeric && !cols[3].numeric);
        assert!(!cols[0].writable, "GENERATED ALWAYS AS IDENTITY 不能写");
        assert!(cols[1].writable);

        let gen_cols = try_table_columns(&k, "public", "gen").unwrap();
        assert!(
            !gen_cols.iter().find(|c| c.name == "b").unwrap().writable,
            "生成列只读"
        );
        assert_eq!(gen_cols[0].ty, "integer");

        // 补全用的「表 → 列」
        let by_table = all_columns_by_table(&k, "public");
        let all: Vec<&String> = by_table.iter().flat_map(|(_, cols)| cols.iter()).collect();
        assert!(all.iter().any(|c| *c == "name") && all.iter().any(|c| *c == "flag"));
    }

    #[test]
    #[ignore = "打 datawalk 开发库，需要网络可达"]
    fn pg_table_page_returns_text_values_keys_and_pushdown() {
        let (_g, _p, k) = pg_setup();
        let page = select_table(&k, "public", "member", None, &[], 0, 100).unwrap();
        assert_eq!(page.rows.len(), 3);
        // 文本协议：numeric 保持服务端给的两位小数，NULL 是字面量
        assert_eq!(page.rows[0][1], "阿离");
        assert_eq!(page.rows[0][2], "91.50");
        assert_eq!(page.rows[2][2], "NULL");
        assert_eq!(page.rows[2][4], "utf8 中文 ok");
        assert_eq!(page.rows[1][4], "带'引号");
        // 行身份 = 主键列值；标识列由服务端发号，具体值不预设
        assert!(page.keys.iter().all(|key| key.len() == 1 && !key[0].is_empty()));
        // 无排序时按主键排，翻页顺序稳定
        assert_eq!(page.rows[0][1], "阿离", "按 id 升序");

        // shuffle 的插入顺序（堆序 30,10,20）与主键序相反：
        // 少了隐式 ORDER BY，这条就会按堆序吐回来
        let shuffled = select_table(&k, "public", "shuffle", None, &[], 0, 10).unwrap();
        assert_eq!(
            shuffled.rows.iter().map(|r| r[1].as_str()).collect::<Vec<_>>(),
            vec!["十", "二十", "三十"],
            "无排序时补主键排序，不是原样吐堆序"
        );

        let page = select_table(&k, "public", "member", None, &[], 1, 1).unwrap();
        assert_eq!(page.rows.len(), 1, "第二页只有一行");
        assert_eq!(count_table(&k, "public", "member", None).unwrap(), 3);

        let filtered = select_table(
            &k,
            "public",
            "member",
            Some("\"score\" > 80"),
            &[("score".to_string(), false)],
            0,
            10,
        )
        .unwrap();
        assert_eq!(filtered.rows.len(), 1);
        assert_eq!(filtered.rows[0][1], "阿离");
        assert_eq!(
            count_table(&k, "public", "member", Some("\"score\" > 80")).unwrap(),
            1,
            "COUNT 也带同样的筛选"
        );
        // ILIKE（筛选面板里 PG 走的不区分大小写算子）
        let ci = select_table(&k, "public", "member", Some("\"note\" ILIKE 'UTF8%'"), &[], 0, 10)
            .unwrap();
        assert_eq!(ci.rows.len(), 1);

        // 无主键表：行身份为空向量，改 / 删会被拒
        let none = select_table(&k, "public", "nopath", None, &[], 0, 10).unwrap();
        assert_eq!(none.rows.len(), 2);
        assert!(none.keys.iter().all(|key| key.is_empty()));

        // 复合主键：身份有两段，顺序按列序
        let pair = select_table(&k, "public", "pair", None, &[], 0, 10).unwrap();
        assert_eq!(pair.keys, vec![vec!["1".to_string(), "1".to_string()], vec!["1".to_string(), "2".to_string()]]);
    }

    /// 导出的 SQL 档要能在真库跑回来：表页拿到的文本值 → INSERT（非 MySQL 引号）
    /// → 灌进另一张表 → 逐行比对。反引号方言在这一步会直接被 PG 拒掉。
    #[test]
    #[ignore = "打 datawalk 开发库，需要网络可达"]
    fn pg_exported_sql_insert_loads_back_into_server() {
        let (_g, _p, k) = pg_setup();
        let page = select_table(&k, "public", "member", None, &[], 0, 100).unwrap();
        let cols: Vec<String> = page.columns.iter().map(|c| c.name.clone()).collect();
        let sql = crate::table_export::sql_insert("member_copy", &cols, &page.rows, false)
            .expect("有行就该出 INSERT");
        assert!(sql.contains("\"member_copy\""), "PG 档用双引号：{sql}");
        assert!(!sql.contains('`'), "不能漏出 MySQL 反引号：{sql}");

        execute_stmt(
            &k,
            "CREATE TABLE member_copy(id bigint, name text, score numeric(6,2), \
             joined timestamptz, note text)",
            1,
        )
        .unwrap();
        // 一次执行整批（多语句串走 batch_execute 口径：逐条切给 execute_stmt）
        for stmt in crate::split_sql_statements(&sql) {
            execute_stmt(&k, &stmt, 100).unwrap_or_else(|e| panic!("导出的 INSERT 跑不通：{e}\n{stmt}"));
        }
        let back = select_table(&k, "public", "member_copy", None, &[], 0, 100).unwrap();
        assert_eq!(back.rows, page.rows, "文本值原样导回来，类型也都接得住");
    }

    #[test]
    #[ignore = "打 datawalk 开发库，需要网络可达"]
    fn pg_write_back_updates_deletes_inserts_and_boundaries() {
        let (_g, _p, k) = pg_setup();
        let cols = try_table_columns(&k, "public", "member").unwrap();
        let page = select_table(&k, "public", "member", None, &[], 0, 10).unwrap();
        let first = page.keys[0].clone();

        // 改一列 + 删一行 + 新增一行，一次应用
        let ch = GridChanges {
            table: "member",
            columns: &cols,
            updates: &[(first.clone(), 1, "阿离改".to_string()), (first.clone(), 2, "88.25".to_string())],
            deletes: vec![page.keys[2].clone()].leak(),
            inserts: &[vec![
                "0".to_string(), // 标识列，驱动应跳过
                "新兄弟".to_string(),
                "66.5".to_string(),
                "NULL".to_string(),
                "'NULL'".to_string(),
            ]],
        };
        let msg = apply_changes(&k, "public", &ch).unwrap();
        assert!(msg.contains("已写回"), "{msg}");
        let after = select_table(&k, "public", "member", None, &[], 0, 10).unwrap();
        let names: Vec<&str> = after.rows.iter().map(|r| r[1].as_str()).collect();
        assert_eq!(names, vec!["阿离改", "北辰", "新兄弟"]);
        assert_eq!(after.rows[0][2], "88.25", "numeric 按 text 送也能落对");
        assert_eq!(after.rows[2][2], "66.50");
        assert_eq!(after.rows[2][3], "NULL", "数值 / 时间列空串按 NULL");
        assert_eq!(after.rows[2][4], "NULL", "引号包住的 NULL 是字面文本");

        // 生成列拒写
        let gen_cols = try_table_columns(&k, "public", "gen").unwrap();
        let gen_page = select_table(&k, "public", "gen", None, &[], 0, 10).unwrap();
        let err = apply_changes(
            &k,
            "public",
            &GridChanges {
                table: "gen",
                columns: &gen_cols,
                updates: &[(gen_page.keys[0].clone(), 2, "999".to_string())],
                deletes: &[],
                inserts: &[],
            },
        )
        .unwrap_err();
        assert!(err.contains("生成列"), "{err}");

        // 复合主键定位：只改到那一行
        let pair = try_table_columns(&k, "public", "pair").unwrap();
        apply_changes(
            &k,
            "public",
            &GridChanges {
                table: "pair",
                columns: &pair,
                updates: &[(vec!["1".to_string(), "2".to_string()], 2, "改中".to_string())],
                deletes: &[],
                inserts: &[],
            },
        )
        .unwrap();
        let after = select_table(&k, "public", "pair", None, &[], 0, 10).unwrap();
        assert_eq!(after.rows[0][2], "甲");
        assert_eq!(after.rows[1][2], "改中");

        // 无主键表：改 / 删整体拒绝；纯新增仍可
        let nopath = try_table_columns(&k, "public", "nopath").unwrap();
        let err = apply_changes(
            &k,
            "public",
            &GridChanges {
                table: "nopath",
                columns: &nopath,
                updates: &[(vec![], 0, "z".to_string())],
                deletes: &[],
                inserts: &[],
            },
        )
        .unwrap_err();
        assert!(err.contains("没有主键"), "{err}");
        apply_changes(
            &k,
            "public",
            &GridChanges {
                table: "nopath",
                columns: &nopath,
                updates: &[],
                deletes: &[],
                inserts: &[vec!["c".to_string(), "d".to_string()]],
            },
        )
        .unwrap();
        assert_eq!(count_table(&k, "public", "nopath", None).unwrap(), 3);

        // 视图 / 物化视图不能写
        let view = try_table_columns(&k, "public", "v_member").unwrap();
        let err = apply_changes(
            &k,
            "public",
            &GridChanges {
                table: "v_member",
                columns: &view,
                updates: &[],
                deletes: &[],
                inserts: &[vec!["9".to_string(), "x".to_string()]],
            },
        )
        .unwrap_err();
        assert!(err.contains("视图"), "{err}");

        // 行被别的客户端删掉后按主键写回：不能默默少写
        let cfg = base_config(&cfg_for(&k).unwrap());
        let mut foreign = cfg.connect(NoTls).unwrap();
        foreign
            .simple_query(&format!(
                "DELETE FROM member WHERE id = {}",
                page.keys[1][0].replace(|c: char| !c.is_ascii_digit(), "")
            ))
            .unwrap();
        drop(foreign);
        // 同一批里既有一行能改到、又有一行已被别人删掉：报错，且能改到的那行也不留
        let err = apply_changes(
            &k,
            "public",
            &GridChanges {
                table: "member",
                columns: &cols,
                updates: &[
                    (page.keys[0].clone(), 1, "跟着一起回滚".to_string()),
                    (page.keys[1].clone(), 1, "没人".to_string()),
                ],
                deletes: &[],
                inserts: &[],
            },
        )
        .unwrap_err();
        assert!(err.contains("没找到该行"), "{err}");
        let after = select_table(&k, "public", "member", None, &[], 0, 10).unwrap();
        let names: Vec<String> = after.rows.iter().map(|r| r[1].clone()).collect();
        assert_eq!(names, vec!["阿离改", "新兄弟"], "别人删掉的那行不是我们删的");
        assert_eq!(after.rows[0][1], "阿离改", "失败整体回滚：本批成功的写也不留");

        // 各类型都按 text 送一遍
        let wide = try_table_columns(&k, "public", "wide").unwrap();
        apply_changes(
            &k,
            "public",
            &GridChanges {
                table: "wide",
                columns: &wide,
                updates: &[],
                deletes: &[],
                inserts: &[vec![
                    "7".to_string(),
                    "true".to_string(),
                    "2026-11-04".to_string(),
                    "\\x48656c6c6f".to_string(),
                    r#"{"k": [1, "中文"], "n": null}"#.to_string(),
                    "11111111-2222-3333-4444-555555555555".to_string(),
                    "{1,2,3}".to_string(),
                    "12345678901234.567890".to_string(),
                    "2026-11-04 10:20:30".to_string(),
                ]],
            },
        )
        .unwrap();
        let got = select_table(&k, "public", "wide", None, &[], 0, 10).unwrap();
        assert_eq!(got.rows[0][1], "t");
        assert_eq!(got.rows[0][2], "2026-11-04");
        assert_eq!(got.rows[0][3], "\\x48656c6c6f", "bytea 以十六进制文本展示");
        assert!(got.rows[0][4].contains("中文"));
        assert_eq!(got.rows[0][6], "{1,2,3}");
        assert_eq!(got.rows[0][7], "12345678901234.567890");
        assert_eq!(got.rows[0][8], "2026-11-04 10:20:30");

        // 送进非法文本：PG 的报错要上屏，不能被吞
        let err = apply_changes(
            &k,
            "public",
            &GridChanges {
                table: "wide",
                columns: &wide,
                updates: &[],
                deletes: &[],
                inserts: &[vec![
                    "不是数".to_string(),
                    "true".to_string(),
                    "2026-11-04".to_string(),
                    "NULL".to_string(),
                    "NULL".to_string(),
                    "NULL".to_string(),
                    "NULL".to_string(),
                    "NULL".to_string(),
                    "NULL".to_string(),
                ]],
            },
        )
        .unwrap_err();
        assert!(err.contains("invalid input syntax"), "{err}");
        assert!(err.contains("SQLSTATE"), "错误要带 SQLSTATE：{err}");
        close(&k);
    }

    #[test]
    #[ignore = "打 datawalk 开发库，需要网络可达"]
    fn pg_execute_stmt_covers_select_types_counts_and_errors() {
        let (_g, _p, k) = pg_setup();
        let out = execute_stmt(&k, "SELECT id, name, score FROM member ORDER BY id", 100).unwrap();
        let sel = out.select.expect("SELECT 要出结果页");
        assert_eq!(sel.rows.len(), 3);
        assert_eq!(sel.columns[0].name, "id");
        assert_eq!(sel.columns[0].ty, "int8", "列类型走 prepare 问回来");
        assert!(sel.columns[0].numeric);
        assert!(!sel.columns[1].numeric);
        assert!(!sel.columns[0].writable, "查询页结果不支撑写回");
        assert!(out.message.is_empty());

        // 截断只按上限标注，不无限吃内存
        let out = execute_stmt(&k, "SELECT * FROM generate_series(1, 10) AS g", 4).unwrap();
        let sel = out.select.unwrap();
        assert_eq!(sel.rows.len(), 4);
        assert!(sel.truncated);
        assert!(out.message.contains("仅显示前 4 行"), "{}", out.message);

        let out = execute_stmt(&k, "UPDATE member SET note = 'x' WHERE score IS NULL", 100).unwrap();
        assert_eq!(out.affected, 1);
        assert!(out.message.contains("1 rows affected"));

        let out = execute_stmt(&k, "CREATE TABLE extra(a int)", 100).unwrap();
        assert_eq!(out.affected, 0, "DDL 没有行数");

        // 报错形态：服务端的消息 + SQLSTATE
        let err = execute_stmt(&k, "SELECT * FROM nope", 100).unwrap_err();
        assert!(err.contains("nope"), "{err}");
        assert!(err.contains("42P01"), "缺 SQLSTATE：{err}");

        //  Dollar-quoted 函数体（切句由 UI 负责，驱动只管一条）
        let out = execute_stmt(
            &k,
            "CREATE FUNCTION body() RETURNS int AS $fn$\nBEGIN RETURN 1; END;\n$fn$ LANGUAGE plpgsql",
            100,
        )
        .unwrap();
        assert_eq!(out.affected, 0);
        assert!(try_list_routines(&k, "public").unwrap().contains(&"body".to_string()));
        close(&k);
    }

    #[test]
    #[ignore = "打 datawalk 开发库，需要网络可达"]
    fn pg_script_continues_past_errors_but_aborts_inside_transaction() {
        let (_g, p, _k) = pg_setup();
        let run = run_script(
            &create_job(),
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
        assert!(!run.aborted_tx);

        // 事务里报错：PG 把事务标成中止，之后每条都会连带失败，
        // 所以就地回滚并停下，而不是攒一串假错误
        let run = run_script(
            &create_job(),
            &p,
            &[
                "BEGIN".to_string(),
                "INSERT INTO pair VALUES (5, 5, '进事务')".to_string(),
                "SELECT * FROM nope".to_string(),
                "SELECT 3".to_string(),
            ],
            QUERY_ROW_LIMIT,
            false,
        );
        assert_eq!(run.runs.len(), 3, "第 4 条不该再跑");
        assert!(run.aborted_tx, "事务中止要单独报出去");
        assert!(run.cancelled, "剩余语句未执行");
        assert!(run.rolled_back, "中止的事务被回滚");
        assert_eq!(
            count_table(&key("真机测试", TEST_DB), "public", "pair", None).unwrap_or(0),
            2,
            "回滚只回事务内的，夹具的两行还在"
        );

        // 脚本停在 BEGIN 里没提交：收尾回滚，不把事务挂在连接上
        let run = run_script(
            &create_job(),
            &p,
            &[
                "BEGIN".to_string(),
                "INSERT INTO pair VALUES (6, 6, '没提交')".to_string(),
            ],
            QUERY_ROW_LIMIT,
            false,
        );
        assert!(run.rolled_back && !run.aborted_tx);
        assert_eq!(
            count_table(&key("真机测试", TEST_DB), "public", "pair", None).unwrap(),
            2
        );

        // 事务正常提交后，写入对下一条语句可见
        let run = run_script(
            &create_job(),
            &p,
            &[
                "BEGIN".to_string(),
                "INSERT INTO pair VALUES (7, 7, '提交')".to_string(),
                "COMMIT".to_string(),
            ],
            QUERY_ROW_LIMIT,
            false,
        );
        assert!(run.runs.iter().all(|r| r.result.is_ok()));
        assert!(!run.rolled_back);
        assert_eq!(
            count_table(&key("真机测试", TEST_DB), "public", "pair", None).unwrap(),
            3
        );
    }

    #[test]
    #[ignore = "打 datawalk 开发库，需要网络可达"]
    fn pg_stop_cancels_the_running_statement() {
        let (_g, p, _k) = pg_setup();
        let job = create_job();
        let stmts = vec!["SELECT pg_sleep(20)".to_string(), "SELECT 1".to_string()];
        let worker_p = p.clone();
        let bg = job.clone();
        let h = std::thread::spawn(move || run_script(&bg, &worker_p, &stmts, QUERY_ROW_LIMIT, true));
        // 让第一条语句真的跑起来再点「停止」
        std::thread::sleep(std::time::Duration::from_millis(600));
        job.cancel();
        let run = h.join().unwrap();
        assert!(run.cancelled, "停止标志要落到回执");
        let msg = run
            .runs
            .first()
            .and_then(|r| r.result.as_ref().err().cloned())
            .unwrap_or_default();
        assert!(
            msg.contains("canceling statement due to user request"),
            "正在跑的语句要被服务端中断：{msg}"
        );
        assert_eq!(run.runs.len(), 1, "后面的语句不再跑");
    }

    /// 测试用：从注册表键反查该库的连接参数（造第二条独立连接验证并发写）。
    fn cfg_for(_k: &str) -> Option<Params> {
        datawalk_params().map(|p| p.for_db(TEST_DB))
    }
}
