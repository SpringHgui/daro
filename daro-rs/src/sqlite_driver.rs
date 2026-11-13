//! SQLite 真实驱动（rusqlite，bundled 编译自带 SQLite，无需系统库）。
//!
//! daro-rs 接的第一个真驱动：目录清单（表 / 视图）、列元数据、行取数
//! （带 rowid，支撑网格的 UPDATE / DELETE）、任意语句执行。连接按名字
//! 注册进进程级表，UI 层只用连接名 + 文件路径说话。
//!
//! v1 口径：目录 / 取数 / 写回同步执行（本地文件瞬时完成，与 mock 档同一
//! 假设）；查询页的脚本另开一条连接在后台线程跑，界面不冻结、可中断。
//! NULL 渲染成字面 `NULL`；BLOB 渲染成 `<BLOB n 字节>`。
//!
//! 目录 / 取数都带库名（`main` = 主库，ATTACH 进来的库同名列在树上），
//! 写回按 rowid 定位并走参数绑定；生成列只读，视图整体拒绝写回。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use rusqlite::types::{Value, ValueRef};
use rusqlite::{InterruptHandle, OpenFlags, params, Connection as RawConn};

use crate::db_types::{ColMeta, ExecOutcome, GridChanges, RowKey, ScriptRun, SelectPage, StmtRun};

/// 查询页一次最多取回的行数（超出只标「已截断」，不无限吃内存）。
pub const QUERY_ROW_LIMIT: usize = 5000;

/// 外部进程同时写库时的等待时长；SQLite 默认 0，会立刻报 database is locked。
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// SQLite 类型亲和 → 数值列判定（照 affinity 规则的前缀口径）。
fn ty_is_numeric(ty: &str) -> bool {
    let t = ty.trim().to_ascii_uppercase();
    if t.is_empty() {
        return false; // 无类型列按文本走
    }
    ["INT", "REAL", "FLOA", "DOUB", "NUM", "DEC"]
        .iter()
        .any(|p| t.contains(p))
}

fn ty_is_blob(ty: &str) -> bool {
    ty.trim().to_ascii_uppercase().contains("BLOB")
}

/// 引号包标识符（SQLite 方言：双引号，内部 " 双写）。
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn qualify(db: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(db), quote_ident(table))
}

/// 注册表：连接名 → 库句柄。锁只护住这张表本身，**绝不跨查询持有**：
/// 每条连接自带一把锁，谁用它谁锁它。UI 线程点「关闭连接」时只做一次表操作，
/// 不会被后台正在跑的建连 / 取数堵住（那曾把界面冻死）。
static CONNS: LazyLock<Mutex<HashMap<String, Arc<Mutex<RawConn>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 注册表里取一条句柄；持注册表锁的时间只够一次克隆。
fn handle(key: &str) -> Result<Arc<Mutex<RawConn>>, String> {
    CONNS
        .lock()
        .unwrap()
        .get(key)
        .cloned()
        .ok_or_else(|| format!("连接「{key}」未打开"))
}

/// 打开（或重开）一个 SQLite 库文件。文件不存在会新建——与参考界面
/// 「连上空库再建表」的习惯一致；打不开（目录 / 权限 / 非库文件延迟到
/// 首条查询才报）在这里返回 Err。
///
/// 路径支持 SQLite URI（`file:…?mode=ro` 等）；打开后统一设 busy_timeout
/// 与 WAL，降低与外部进程同时读写时的锁冲突。
pub fn open(key: &str, path: &str) -> Result<(), String> {
    let raw = open_raw(path)?;
    CONNS
        .lock()
        .unwrap()
        .insert(key.to_string(), Arc::new(Mutex::new(raw)));
    Ok(())
}

/// 按 UI 连接的口径开一条裸连接（不落注册表）。
///
/// 路径支持 SQLite URI（`file:…?mode=ro` 等）；打开后统一设 busy_timeout
/// 与 WAL，降低与外部进程同时读写时的锁冲突。
pub fn open_raw(path: &str) -> Result<RawConn, String> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_URI;
    let raw = RawConn::open_with_flags(path, flags).map_err(|e| e.to_string())?;
    let _ = raw.busy_timeout(BUSY_TIMEOUT);
    raw.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| e.to_string())?;
    // 内存库没有 WAL；失败也不影响使用（网络盘 / 只读库会拒绝）
    if !is_memory_path(path) {
        let _ = raw.pragma_update(None, "journal_mode", "WAL");
    }
    Ok(raw)
}

/// 内存库路径：每条连接各是一份库，UI 不能为了后台执行另开一条。
pub fn is_memory_path(path: &str) -> bool {
    path == ":memory:" || path.contains("mode=memory")
}

/// 「测试连接」专用：只读打开并执行一条 SELECT 1，不注册、不建库。
/// 文件不存在直接报错——测试按钮不该顺手创建一个空库。
pub fn probe(path: &str) -> Result<(), String> {
    let raw = RawConn::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| e.to_string())?;
    raw.query_row("SELECT 1", [], |_| Ok(()))
        .map_err(|e| e.to_string())
}

/// 连接是否已在注册表里。UI 惰性建连：树展开 / 打表前先 ensure。
pub fn is_open(key: &str) -> bool {
    CONNS.lock().unwrap().contains_key(key)
}

/// 关一条库连接。只从注册表摘句柄，不等在飞的查询：那条查询手里还捏着
/// `Arc`，跑完自己释放。
pub fn close(key: &str) {
    let removed = CONNS.lock().unwrap().remove(key);
    drop(removed);
}

/// 惰性建连：没开过就开。path 为空当内存库（测试用）。
pub fn ensure(key: &str, path: &str) -> Result<(), String> {
    if is_open(key) {
        return Ok(());
    }
    open(key, path)
}

/// 测试台专用：拿到原始连接造数据 / 断言。UI 层不依赖。
#[cfg(test)]
pub(crate) fn with_raw<T>(key: &str, f: impl FnOnce(&RawConn) -> T) -> Option<T> {
    let raw = CONNS.lock().unwrap().get(key).cloned()?;
    Some(f(&raw.lock().unwrap()))
}

/// 注册表里取出句柄、锁住这条连接执行 `f`；注册表锁不跨查询。
fn with_conn<T>(key: &str, f: impl FnOnce(&RawConn) -> Result<T, String>) -> Result<T, String> {
    let raw = handle(key)?;
    let raw = raw.lock().unwrap();
    f(&raw)
}

/// 当前连接是否不在自动提交态（有未提交事务）。脚本失败后 UI 用它收尾。
pub fn in_transaction(key: &str) -> bool {
    with_conn(key, |raw| Ok(!raw.is_autocommit())).unwrap_or(false)
}

/// 有悬挂事务就回滚；返回是否真的回滚过。
pub fn rollback_if_open(key: &str) -> bool {
    with_conn(key, |raw| {
        if raw.is_autocommit() {
            return Ok(false);
        }
        raw.execute_batch("ROLLBACK").map_err(|e| e.to_string())?;
        Ok(true)
    })
    .unwrap_or(false)
}

// ── 目录清单 ─────────────────────────────────────────────────────

/// 连接里可见的库清单（主库 + ATTACH 进来的库），`PRAGMA database_list` 顺序。
pub fn list_databases(key: &str) -> Vec<String> {
    with_conn(key, |raw| {
        let mut stmt = raw
            .prepare("PRAGMA database_list")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .filter(|n| n != "temp")
            .collect();
        Ok(rows)
    })
    .unwrap_or_default()
}

/// 用户表清单（排除 sqlite_ 内部表 / 影子表，虚拟表保留），名称排序。
pub fn list_tables(key: &str, db: &str) -> Vec<String> {
    try_list_tables(key, db).unwrap_or_default()
}

/// 视图清单，名称排序。
pub fn list_views(key: &str, db: &str) -> Vec<String> {
    try_list_views(key, db).unwrap_or_default()
}

/// 带错误的表清单（目录层要能上屏真实失败，而不是静默空树）。
pub fn try_list_tables(key: &str, db: &str) -> Result<Vec<String>, String> {
    objects_of_type(key, db, "table")
}

/// 带错误的视图清单。
pub fn try_list_views(key: &str, db: &str) -> Result<Vec<String>, String> {
    objects_of_type(key, db, "view")
}

fn objects_of_type(key: &str, db: &str, ty: &str) -> Result<Vec<String>, String> {
    with_conn(key, |raw| {
        // table_list 能区分 shadow（FTS 影子表）与 virtual，比 sqlite_master 干净
        let sql = "SELECT name FROM pragma_table_list \
                   WHERE schema = ?1 AND (type = ?2 OR (?2 = 'table' AND type = 'virtual')) \
                   AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name";
        let mut stmt = raw.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![db, ty], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

/// 表的列元数据（PRAGMA table_xinfo：含生成列，隐藏列不展示）。
pub fn table_columns(key: &str, db: &str, table: &str) -> Vec<ColMeta> {
    try_table_columns(key, db, table).unwrap_or_default()
}

/// 带错误的列元数据。
pub fn try_table_columns(key: &str, db: &str, table: &str) -> Result<Vec<ColMeta>, String> {
    with_conn(key, |raw| table_columns_raw(raw, db, table))
}

fn table_columns_raw(raw: &RawConn, db: &str, table: &str) -> Result<Vec<ColMeta>, String> {
    let mut stmt = raw
        .prepare(&format!(
            "PRAGMA {}.table_xinfo({})",
            quote_ident(db),
            quote_ident(table)
        ))
        .map_err(|e| e.to_string())?;
    let cols = stmt
        .query_map([], |r| {
            let name: String = r.get("name")?;
            let ty: String = r.get("type").unwrap_or_default();
            let hidden: i64 = r.get("hidden").unwrap_or(0);
            let numeric = ty_is_numeric(&ty);
            Ok((name, ty, numeric, hidden))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|c| c.ok())
        .filter(|(_, _, _, hidden)| *hidden != 1) // 虚拟表隐藏列不进 SELECT *
        .map(|(name, ty, numeric, hidden)| {
            let mut m = ColMeta::new(name, ty, numeric);
            m.writable = hidden == 0;
            m
        })
        .collect();
    Ok(cols)
}

/// 库里「表 → 列」，查询编辑器补全用（SQLite 没有目录表，逐表 PRAGMA）。
pub fn all_columns_by_table(key: &str, db: &str) -> Vec<(String, Vec<String>)> {
    list_tables(key, db)
        .into_iter()
        .map(|t| {
            let cols = table_columns(key, db, &t)
                .into_iter()
                .map(|c| c.name)
                .collect();
            (t, cols)
        })
        .collect()
}

// ── 行取数 ───────────────────────────────────────────────────────

/// 表页取数的 SQL（状态栏也用它展示真实语句）。
pub fn select_table_sql(
    db: &str,
    table: &str,
    where_sql: Option<&str>,
    orders: &[(String, bool)],
    limit: usize,
    offset: usize,
) -> String {
    let order_part = order_part(orders);
    let where_part = where_sql.map(|w| format!(" WHERE {w}")).unwrap_or_default();
    format!(
        "SELECT * FROM {}{where_part}{order_part} LIMIT {limit} OFFSET {offset}",
        qualify(db, table)
    )
}

fn order_part(orders: &[(String, bool)]) -> String {
    match orders {
        [] => String::new(),
        keys => {
            let cols: Vec<String> = keys
                .iter()
                .map(|(c, asc)| {
                    format!("{} {}", quote_ident(c), if *asc { "ASC" } else { "DESC" })
                })
                .collect();
            format!(" ORDER BY {}", cols.join(", "))
        }
    }
}

/// 打开表的取数：`SELECT <rowid>, * FROM table`（带 WHERE / ORDER BY /
/// LIMIT / OFFSET）。表里恰好有同名列时换 `_rowid_` / `oid` 避让；
/// WITHOUT ROWID / 虚拟表退化成普通 SELECT（rowid 全 0）。
#[allow(clippy::too_many_arguments)]
pub fn select_table(
    key: &str,
    db: &str,
    table: &str,
    where_sql: Option<&str>,
    orders: &[(String, bool)],
    offset: usize,
    limit: usize,
) -> Result<SelectPage, String> {
    with_conn(key, |raw| {
        let alias = rowid_alias(raw, db, table);
        let where_part = where_sql.map(|w| format!(" WHERE {w}")).unwrap_or_default();
        let order_part = order_part(orders);
        let base = format!(
            " FROM {}{where_part}{order_part} LIMIT {limit} OFFSET {offset}",
            qualify(db, table)
        );
        if let Some(alias) = alias {
            let sql = format!("SELECT {alias} AS \"__daro_rowid__\", *{base}");
            if let Ok(page) = fetch_rows_impl(raw, &sql, true, None) {
                return Ok(page);
            }
        }
        // WITHOUT ROWID / 虚拟表：退回不带 rowid 的取数
        let sql = format!("SELECT *{base}");
        fetch_rows_impl(raw, &sql, false, None)
    })
}

/// 当前筛选条件下的总行数（远程分页的「共 n 条」与末页判断）。
pub fn count_table(
    key: &str,
    db: &str,
    table: &str,
    where_sql: Option<&str>,
) -> Result<usize, String> {
    with_conn(key, |raw| {
        let where_part = where_sql.map(|w| format!(" WHERE {w}")).unwrap_or_default();
        let sql = format!("SELECT COUNT(*) FROM {}{where_part}", qualify(db, table));
        raw.query_row(&sql, [], |r| r.get::<_, i64>(0))
            .map(|n| n.max(0) as usize)
            .map_err(|e| e.to_string())
    })
}

/// 任意 SELECT 语句的结果（查询页）。列名重复时自动去重后缀；
/// 超过 `max_rows` 行就截断并置 `truncated`。
pub fn select_sql(key: &str, stmt: &str, max_rows: usize) -> Result<SelectPage, String> {
    with_conn(key, |raw| fetch_rows_impl(raw, stmt, false, Some(max_rows)))
}

/// 值渲染：NULL → `NULL`，BLOB → `<BLOB n 字节>`，其余原样。
pub fn render_value(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Null => "NULL".to_string(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
        ValueRef::Blob(b) => format!("<BLOB {} 字节>", b.len()),
    }
}

// ── 写路径 ───────────────────────────────────────────────────────

/// 执行一条语句（调用方已按分号切好）。SELECT 出结果页，其余出影响行数。
pub fn execute_stmt(key: &str, stmt: &str, max_rows: usize) -> Result<ExecOutcome, String> {
    with_conn(key, |raw| execute_on(raw, stmt, max_rows))
}

/// 同 `execute_stmt`，但直接吃连接：后台脚本线程自己开连接，不碰注册表锁。
pub fn execute_on(raw: &RawConn, stmt: &str, max_rows: usize) -> Result<ExecOutcome, String> {
    let trimmed = stmt.trim();
    if query_is_select(trimmed) {
        let page = fetch_rows_impl(raw, trimmed, false, Some(max_rows))?;
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
        let affected = raw.execute(trimmed, []).map_err(|e| e.to_string())?;
        Ok(ExecOutcome {
            select: None,
            affected,
            message: format!("Query OK, {affected} rows affected"),
        })
    }
}

/// MCP / 单语句后台执行：文件库自己开一条连接，跑完即弃；`:memory:` 只活在
/// 进程内那条共享连接上，只能借用它，靠 InterruptHandle 中途打断。
#[allow(dead_code)] // 只有 MCP 后台（lib）调用，UI 那份拷贝用不到
pub fn execute_job(
    key: &str,
    path: &str,
    job: &Job,
    stmt: &str,
    max_rows: usize,
) -> Result<ExecOutcome, String> {
    if job.is_cancelled() {
        return Err("查询已取消".to_string());
    }
    if is_memory_path(path) {
        ensure(key, path)?;
        let raw = handle(key)?;
        let raw = raw.lock().unwrap();
        RUNNING_JOBS
            .lock()
            .unwrap()
            .insert(job.id, raw.get_interrupt_handle());
        let result = execute_on(&raw, stmt, max_rows);
        RUNNING_JOBS.lock().unwrap().remove(&job.id);
        return result;
    }
    let raw = open_raw(path)?;
    RUNNING_JOBS
        .lock()
        .unwrap()
        .insert(job.id, raw.get_interrupt_handle());
    let result = execute_on(&raw, stmt, max_rows);
    RUNNING_JOBS.lock().unwrap().remove(&job.id);
    result
}

// ── 后台脚本执行（查询页「运行」）───────────────────────────────

/// 一次后台脚本的取消通道。UI 侧持有它：点「停止」先置标志（下一条语句
/// 不再执行），再 interrupt 正在跑的那条语句（SQLite 立刻回 SQLITE_INTERRUPT）。
/// 任务号由 `create_job` 分配，句柄要等工作线程开完连接才挂得上，
/// 所以两段都写成「有则用，没有就只置标志」。
#[derive(Clone)]
pub struct Job {
    id: u64,
    cancel: Arc<AtomicBool>,
}

impl Job {
    /// 只置标志：下一条语句开跑前收手，不碰连接。
    pub fn request_stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// 立刻打断正在执行的语句（SQLite 回 SQLITE_INTERRUPT）。
    pub fn interrupt(&self) {
        if let Some(h) = RUNNING_JOBS.lock().unwrap().get(&self.id) {
            h.interrupt();
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

/// 逐条执行脚本，支持「停止」；`continue_on_error=false` 时第一条报错就收手。
///
/// 自己开一条到同一文件的连接：注册表连接归 UI 线程，长查询借它会连同
/// 界面、目录读取一起排队。写完即提交，UI 侧下一次取数自然看得见。
pub fn run_script(
    job: &Job,
    path: &str,
    stmts: &[String],
    max_rows: usize,
    continue_on_error: bool,
) -> ScriptRun {
    let started = Instant::now();
    let raw = match open_raw(path) {
        Ok(raw) => raw,
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
        .insert(job.id, raw.get_interrupt_handle());
    let mut runs = Vec::with_capacity(stmts.len());
    let mut had_error = false;
    let mut cancelled = false;
    for stmt in stmts {
        if job.is_cancelled() {
            cancelled = true;
            break;
        }
        let sw = Instant::now();
        let result = execute_on(&raw, stmt, max_rows);
        let failed = result.is_err();
        if failed {
            had_error = true;
        }
        runs.push(StmtRun {
            elapsed_ms: sw.elapsed().as_millis(),
            result,
        });
        if failed && !continue_on_error {
            break;
        }
    }
    let rolled_back = had_error && !raw.is_autocommit() && raw.execute_batch("ROLLBACK").is_ok();
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

static RUNNING_JOBS: LazyLock<Mutex<HashMap<u64, InterruptHandle>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

fn fetch_rows_impl(
    raw: &RawConn,
    sql: &str,
    first_col_is_rowid: bool,
    max_rows: Option<usize>,
) -> Result<SelectPage, String> {
    // 与 with_conn 版本同体，只是直接吃连接（查询页走这里，避免嵌套锁）
    let mut stmt = raw.prepare(sql).map_err(|e| e.to_string())?;
    let ncols = stmt.column_count();
    let decls: Vec<Option<String>> = stmt
        .columns()
        .map(|c| c.decl_type().map(str::to_string))
        .collect();
    let mut columns: Vec<ColMeta> = Vec::with_capacity(ncols);
    let mut seen: HashMap<String, usize> = HashMap::new();
    for i in 0..ncols {
        if first_col_is_rowid && i == 0 {
            continue;
        }
        let name = stmt
            .column_name(i)
            .map_err(|e| e.to_string())?
            .to_string();
        let ty = decls.get(i).cloned().flatten().unwrap_or_default();
        let numeric = ty_is_numeric(&ty);
        let mut uniq = name.clone();
        if let Some(n) = seen.get_mut(&name) {
            *n += 1;
            uniq = format!("{name}:{n}");
        } else {
            seen.insert(name, 0);
        }
        columns.push(ColMeta::new(uniq, ty, numeric));
    }
    let mut rows = Vec::new();
    let mut keys: Vec<RowKey> = Vec::new();
    let mut truncated = false;
    let mut rows_iter = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(r) = rows_iter.next().map_err(|e| e.to_string())? {
        if max_rows.is_some_and(|n| rows.len() >= n) {
            truncated = true;
            break;
        }
        let mut row = Vec::with_capacity(columns.len());
        for i in 0..ncols {
            if first_col_is_rowid && i == 0 {
                continue;
            }
            row.push(render_value(r.get_ref(i).map_err(|e| e.to_string())?));
        }
        if first_col_is_rowid {
            let rid = r.get_ref(0).map_err(|e| e.to_string())?;
            keys.push(match rid {
                ValueRef::Integer(i) => vec![i.to_string()],
                _ => Vec::new(),
            });
        }
        rows.push(row);
    }
    if !first_col_is_rowid {
        keys = vec![Vec::new(); rows.len()];
    }
    Ok(SelectPage {
        columns,
        rows,
        keys,
        truncated,
    })
}

/// 顶层 SELECT 粗判（WITH / EXPLAIN / PRAGMA 也当出结果处理）。
pub fn query_is_select(stmt: &str) -> bool {
    let head = stmt
        .trim_start()
        .trim_start_matches('(')
        .trim_start()
        .to_ascii_lowercase();
    head.starts_with("select")
        || head.starts_with("with")
        || head.starts_with("explain")
        || head.starts_with("pragma")
}

// ── 网格写回（apply）────────────────────────────────────────────

pub fn apply_changes(key: &str, db: &str, ch: &GridChanges<'_>) -> Result<String, String> {
    with_conn(key, |raw| {
        let object_type: Option<String> = raw
            .query_row(
                &format!(
                    "SELECT type FROM {}.sqlite_master WHERE name = ?1",
                    quote_ident(db)
                ),
                params![ch.table],
                |r| r.get(0),
            )
            .ok();
        if object_type.as_deref() == Some("view") {
            return Err(format!(
                "「{}」是视图，不能直接编辑；请修改基表",
                ch.table
            ));
        }
        let writable = writable_map(raw, db, ch.table)?;
        let alias = rowid_alias(raw, db, ch.table);
        if (!ch.updates.is_empty() || !ch.deletes.is_empty()) && alias.is_none() {
            return Err("该表没有可用的 rowid，不支持编辑 / 删除写回".to_string());
        }
        let alias = alias.unwrap_or_else(|| "rowid".to_string());
        let tx = raw.unchecked_transaction().map_err(|e| e.to_string())?;
        let mut touched = 0usize;
        for (key, col, val) in ch.updates {
            let Some(rid) = rowid_of(key) else {
                return Err("该表无 rowid，不支持编辑写回".to_string());
            };
            let Some(meta) = ch.columns.get(*col) else {
                continue;
            };
            if !writable.get(&meta.name).copied().unwrap_or(true) {
                return Err(format!("列「{}」是生成列 / 隐藏列，不能写回", meta.name));
            }
            let sql = format!(
                "UPDATE {} SET {} = ?1 WHERE {} = ?2",
                qualify(db, ch.table),
                quote_ident(&meta.name),
                alias
            );
            tx.execute(&sql, params![value_for(meta, val)?, rid])
                .map_err(|e| e.to_string())?;
            touched += 1;
        }
        for key in ch.deletes {
            let Some(rid) = rowid_of(key) else {
                return Err("该表无 rowid，不支持删除写回".to_string());
            };
            let sql = format!(
                "DELETE FROM {} WHERE {} = ?1",
                qualify(db, ch.table),
                alias
            );
            touched += tx.execute(&sql, params![rid]).map_err(|e| e.to_string())?;
        }
        if !ch.inserts.is_empty() {
            // 只写可写列；生成列由 SQLite 自算，隐藏列不参与
            let idx: Vec<usize> = ch
                .columns
                .iter()
                .enumerate()
                .filter(|(_, c)| writable.get(&c.name).copied().unwrap_or(true))
                .map(|(i, _)| i)
                .collect();
            let collist: Vec<String> = idx
                .iter()
                .filter_map(|i| ch.columns.get(*i).map(|c| quote_ident(&c.name)))
                .collect();
            for row in ch.inserts {
                let placeholders: Vec<String> =
                    (1..=idx.len()).map(|i| format!("?{i}")).collect();
                let values: Vec<Value> = idx
                    .iter()
                    .map(|i| {
                        ch.columns
                            .get(*i)
                            .map(|m| value_for(m, row.get(*i).map(String::as_str).unwrap_or("")))
                            .unwrap_or(Ok(Value::Null))
                    })
                    .collect::<Result<_, String>>()?;
                let sql = if collist.is_empty() {
                    format!("INSERT INTO {} DEFAULT VALUES", qualify(db, ch.table))
                } else {
                    format!(
                        "INSERT INTO {} ({}) VALUES ({})",
                        qualify(db, ch.table),
                        collist.join(", "),
                        placeholders.join(", ")
                    )
                };
                tx.execute(&sql, rusqlite::params_from_iter(values))
                    .map_err(|e| e.to_string())?;
                touched += 1;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(format!("已写回 {touched} 行"))
    })
}

/// SQLite 的行身份 = 单元素 rowid 文本；空身份（无 rowid 表）或非数字都定位不到行。
fn rowid_of(key: &RowKey) -> Option<i64> {
    key.first().and_then(|s| s.parse().ok())
}

/// rowid 在表中的可用名字：列名占用 `rowid` 时依次退到 `_rowid_` / `oid`；
/// 三个都被占（或 WITHOUT ROWID）就没有可用 rowid。
fn rowid_alias(raw: &RawConn, db: &str, table: &str) -> Option<String> {
    let names: Vec<String> = raw
        .prepare(&format!(
            "PRAGMA {}.table_xinfo({})",
            quote_ident(db),
            quote_ident(table)
        ))
        .ok()?
        .query_map([], |r| r.get::<_, String>(1))
        .ok()?
        .filter_map(|r| r.ok())
        .collect();
    ["rowid", "_rowid_", "oid"]
        .into_iter()
        .find(|cand| !names.iter().any(|n| n.eq_ignore_ascii_case(cand)))
        .map(str::to_string)
}

/// 列名 → 是否可写（生成列 hidden=2/3、隐藏列 hidden=1 均不可写）。
fn writable_map(raw: &RawConn, db: &str, table: &str) -> Result<HashMap<String, bool>, String> {
    let mut stmt = raw
        .prepare(&format!(
            "PRAGMA {}.table_xinfo({})",
            quote_ident(db),
            quote_ident(table)
        ))
        .map_err(|e| e.to_string())?;
    let map = stmt
        .query_map([], |r| {
            let name: String = r.get("name")?;
            let hidden: i64 = r.get("hidden").unwrap_or(0);
            Ok((name, hidden == 0))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(map)
}

/// 单元格文本 → 绑定值。约定：
/// - 字面 `NULL` → SQL NULL
/// - 单引号包住的值（`'NULL'` / `'  文本  '`）→ 去引号后的纯文本
/// - 数值列且能解析 → Integer / Real；空串 → NULL
/// - 其余 → Text
fn value_for(meta: &ColMeta, v: &str) -> Result<Value, String> {
    if v == "NULL" {
        return Ok(Value::Null);
    }
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        return Ok(Value::Text(v[1..v.len() - 1].replace("''", "'")));
    }
    if meta.numeric {
        let t = v.trim();
        if t.is_empty() {
            return Ok(Value::Null);
        }
        if let Ok(i) = t.parse::<i64>() {
            return Ok(Value::Integer(i));
        }
        if let Ok(f) = t.parse::<f64>() {
            return Ok(Value::Real(f));
        }
    }
    if ty_is_blob(&meta.ty) && v.starts_with("<BLOB ") {
        return Err(format!(
            "BLOB 列「{}」暂不支持文本编辑（当前值是二进制占位）",
            meta.name
        ));
    }
    Ok(Value::Text(v.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: &str = "unit";

    // 单测共用 key=unit 这一条内存连接，必须串行跑；也避免一条用例
    // panic 时把 CONNS 锁打毒后连带其它用例。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn setup() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        close(K);
        open(K, ":memory:").unwrap();
        with_raw(K, |raw| {
            raw.execute_batch(
                r#"
                CREATE TABLE member (id INTEGER PRIMARY KEY, name TEXT NOT NULL, score REAL, joined_at INTEGER);
                INSERT INTO member VALUES (1, '阿离', 91.5, 1700000000);
                INSERT INTO member VALUES (2, '北辰', 77.0, 1700000100);
                INSERT INTO member VALUES (3, '青梧', NULL, 1700000200);
                CREATE VIEW v_top AS SELECT id, name FROM member WHERE score > 80;
                CREATE TABLE norowid (a TEXT PRIMARY KEY, b TEXT) WITHOUT ROWID;
                INSERT INTO norowid VALUES ('x', 'y');
                CREATE TABLE gen (id INTEGER PRIMARY KEY, a INTEGER, b INTEGER GENERATED ALWAYS AS (a * 2) STORED);
                INSERT INTO gen (a) VALUES (5);
                CREATE TABLE shadow_named ("rowid" TEXT, v TEXT);
                INSERT INTO shadow_named VALUES ('fake', 'ok');
                "#,
            )
            .unwrap();
        });
        guard
    }

    /// 测试用行身份：单元素 rowid 文本（与 select_table 带回的形态一致）。
    fn k(id: i64) -> RowKey {
        vec![id.to_string()]
    }

    #[test]
    fn catalog_lists_sorted_tables_and_views() {
        let _guard = setup();
        assert_eq!(
            list_tables(K, "main"),
            vec!["gen", "member", "norowid", "shadow_named"]
        );
        assert_eq!(list_views(K, "main"), vec!["v_top"]);
        close(K);
    }

    #[test]
    fn columns_carry_numeric_and_writable_flags() {
        let _guard = setup();
        let cols = table_columns(K, "main", "member");
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["id", "name", "score", "joined_at"]);
        assert!(cols[0].numeric); // INTEGER
        assert!(!cols[1].numeric); // TEXT
        assert!(cols[2].numeric); // REAL
        assert!(cols.iter().all(|c| c.writable));

        let gen_cols = table_columns(K, "main", "gen");
        let b = gen_cols.iter().find(|c| c.name == "b").unwrap();
        assert!(!b.writable, "生成列不可写");
        close(K);
    }

    #[test]
    fn select_table_returns_real_rows_with_rowids() {
        let _guard = setup();
        let page = select_table(K, "main", "member", None, &[], 0, 100).unwrap();
        assert_eq!(page.rows.len(), 3);
        assert_eq!(page.keys, vec![vec!["1".to_string()], vec!["2".to_string()], vec!["3".to_string()]]);
        assert_eq!(page.rows[2][1], "青梧");
        assert_eq!(page.rows[2][2], "NULL"); // NULL 渲染成字面量
        close(K);
    }

    #[test]
    fn select_table_pages_and_counts() {
        let _guard = setup();
        let page = select_table(K, "main", "member", None, &[], 1, 1).unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0][1], "北辰");
        assert_eq!(page.keys, vec![vec!["2".to_string()]]);
        assert_eq!(count_table(K, "main", "member", None).unwrap(), 3);
        let filtered = count_table(K, "main", "member", Some("\"score\" > 80")).unwrap();
        assert_eq!(filtered, 1);
        close(K);
    }

    #[test]
    fn select_table_without_rowid_falls_back() {
        let _guard = setup();
        let page = select_table(K, "main", "norowid", None, &[], 0, 100).unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0][0], "x");
        assert!(page.keys.iter().all(|k| k.is_empty()));
        close(K);
    }

    #[test]
    fn select_table_avoids_shadowed_rowid() {
        let _guard = setup();
        let page = select_table(K, "main", "shadow_named", None, &[], 0, 10).unwrap();
        assert_eq!(page.columns.len(), 2);
        assert_eq!(page.rows[0][0], "fake");
        assert!(!page.keys[0].is_empty(), "同名列不应遮蔽隐藏 rowid");
        close(K);
    }

    #[test]
    fn where_and_order_flow_into_sql() {
        let _guard = setup();
        let page = select_table(
            K,
            "main",
            "member",
            Some("\"score\" > 80"),
            &[("score".to_string(), false)],
            0,
            100,
        )
        .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0][1], "阿离");
        close(K);
    }

    #[test]
    fn execute_stmt_select_and_write() {
        let _guard = setup();
        let out = execute_stmt(K, "SELECT name FROM member ORDER BY id", QUERY_ROW_LIMIT).unwrap();
        let sel = out.select.unwrap();
        assert_eq!(sel.columns[0].name, "name");
        assert_eq!(sel.rows.len(), 3);
        assert!(!sel.truncated);

        let out = execute_stmt(K, "DELETE FROM member WHERE id = 3", QUERY_ROW_LIMIT).unwrap();
        assert_eq!(out.affected, 1);
        assert!(out.message.contains("1 rows affected"));

        let err = execute_stmt(K, "SELECT nope FROM member", QUERY_ROW_LIMIT).unwrap_err();
        assert!(err.contains("nope"));
        close(K);
    }

    #[test]
    fn select_sql_truncates_at_limit() {
        let _guard = setup();
        let page = select_sql(K, "SELECT * FROM member", 2).unwrap();
        assert_eq!(page.rows.len(), 2);
        assert!(page.truncated);
        close(K);
    }

    #[test]
    fn apply_changes_updates_inserts_deletes_atomically() {
        let _guard = setup();
        let cols = table_columns(K, "main", "member");
        let ch = GridChanges {
            table: "member",
            columns: &cols,
            updates: &[(k(1), 1, "阿离改".to_string()), (k(2), 2, "60.5".to_string())],
            deletes: &[k(3)],
            inserts: &[vec![
                "4".to_string(),
                "新成员".to_string(),
                "88".to_string(),
                "1700000300".to_string(),
            ]],
        };
        let msg = apply_changes(K, "main", &ch).unwrap();
        assert!(msg.contains("4 行"));
        let page = select_table(K, "main", "member", None, &[], 0, 100).unwrap();
        assert_eq!(page.rows.len(), 3);
        assert_eq!(page.rows[0][1], "阿离改");
        assert_eq!(page.rows[1][2], "60.5");
        assert_eq!(page.rows[2][1], "新成员");
        close(K);
    }

    #[test]
    fn apply_rejects_views_and_rowidless_tables() {
        let _guard = setup();
        let cols = table_columns(K, "main", "norowid");
        let ch = GridChanges {
            table: "norowid",
            columns: &cols,
            updates: &[(Vec::new(), 0, "z".to_string())],
            deletes: &[],
            inserts: &[],
        };
        assert!(apply_changes(K, "main", &ch).is_err());

        let cols = table_columns(K, "main", "v_top");
        let ch = GridChanges {
            table: "v_top",
            columns: &cols,
            updates: &[],
            deletes: &[],
            inserts: &[vec!["9".to_string(), "x".to_string()]],
        };
        let err = apply_changes(K, "main", &ch).unwrap_err();
        assert!(err.contains("视图"));
        close(K);
    }

    #[test]
    fn insert_skips_generated_columns() {
        let _guard = setup();
        let cols = table_columns(K, "main", "gen");
        let ch = GridChanges {
            table: "gen",
            columns: &cols,
            updates: &[],
            deletes: &[],
            // 第三格（生成列 b）即使是垃圾也不写
            inserts: &[vec!["2".to_string(), "99".to_string(), "垃圾".to_string()]],
        };
        apply_changes(K, "main", &ch).unwrap();
        let page = select_table(K, "main", "gen", Some("\"id\" = 2"), &[], 0, 10).unwrap();
        assert_eq!(page.rows[0][1], "99");
        assert_eq!(page.rows[0][2], "198");
        close(K);
    }

    #[test]
    fn empty_null_and_quoted_literals_roundtrip() {
        let _guard = setup();
        let cols = table_columns(K, "main", "member");
        let ch = GridChanges {
            table: "member",
            columns: &cols,
            updates: &[],
            deletes: &[],
            inserts: &[
                vec![
                    "5".to_string(),
                    String::new(),
                    String::new(),
                    "1700000300".to_string(),
                ],
                vec![
                    "6".to_string(),
                    "O'Brien".to_string(),
                    "50".to_string(),
                    "0".to_string(),
                ],
                vec![
                    "7".to_string(),
                    "'NULL'".to_string(),
                    "'  留白  '".to_string(),
                    "0".to_string(),
                ],
            ],
        };
        apply_changes(K, "main", &ch).unwrap();
        let page = select_table(K, "main", "member", Some("\"id\" >= 5"), &[], 0, 10).unwrap();
        assert_eq!(page.rows[0][1], ""); // 文本空串保留
        assert_eq!(page.rows[0][2], "NULL"); // 数值列空串按 NULL 落库
        assert_eq!(page.rows[1][1], "O'Brien"); // 单引号双写
        assert_eq!(page.rows[2][1], "NULL"); // 字面字符串 NULL
        assert_eq!(page.rows[2][2], "  留白  "); // 引号内空白保留
        close(K);
    }

    #[test]
    fn transaction_helpers_recover_after_failure() {
        let _guard = setup();
        assert!(!in_transaction(K));
        execute_stmt(K, "BEGIN", QUERY_ROW_LIMIT).unwrap();
        assert!(in_transaction(K));
        assert!(rollback_if_open(K));
        assert!(!in_transaction(K));
        assert!(!rollback_if_open(K));
        close(K);
    }

    #[test]
    fn reopen_replaces_cached_connection() {
        let _guard = setup();
        open(K, ":memory:").unwrap(); // 重开同一 key：替换而非报错
        assert!(execute_stmt(K, "SELECT 1", QUERY_ROW_LIMIT).is_ok());
        close(K);
    }

    fn tmp_db(tag: &str) -> String {
        let db = std::env::temp_dir().join(format!("daro_rs_{tag}_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        db.to_string_lossy().to_string()
    }

    fn rows_of(run: &ScriptRun, ix: usize) -> Vec<Vec<String>> {
        match &run.runs[ix].result {
            Ok(o) => o.select.as_ref().expect("SELECT 应有结果页").rows.clone(),
            Err(e) => panic!("第 {} 条应当成功：{e}", ix + 1),
        }
    }

    #[test]
    fn run_script_continues_past_errors_and_rolls_back_dangling_tx() {
        let _guard = setup();
        let path = tmp_db("run_tx");
        let stmts: Vec<String> = [
            "CREATE TABLE r(a INTEGER PRIMARY KEY)",
            "INSERT INTO r VALUES (1)",
            "BEGIN",
            "INSERT INTO r VALUES (2)",
            "SELECT * FROM nope",
            "SELECT 1",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let run = run_script(&create_job(), &path, &stmts, QUERY_ROW_LIMIT, true);
        assert!(run.open_error.is_none());
        assert!(!run.cancelled);
        assert_eq!(run.runs.len(), 6, "一条报错不拦后面的语句");
        assert!(run.runs[4].result.is_err(), "不存在的表要报错而不是空结果");
        assert!(run.rolled_back, "BEGIN 挂着的半成品事务被回滚");
        let check = run_script(
            &create_job(),
            &path,
            &["SELECT count(*) FROM r".to_string()],
            QUERY_ROW_LIMIT,
            true,
        );
        assert_eq!(rows_of(&check, 0), vec![vec!["1".to_string()]], "回滚只回事务内的");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn run_script_stops_at_first_error_unless_continue_chosen() {
        let _guard = setup();
        let path = tmp_db("run_stop");
        let stmts: Vec<String> = [
            "CREATE TABLE e(a INTEGER)",
            "SELECT * FROM nope",
            "INSERT INTO e VALUES (1)",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let run = run_script(&create_job(), &path, &stmts, QUERY_ROW_LIMIT, false);
        assert!(run.open_error.is_none());
        assert!(!run.cancelled, "遇错收手不是用户停止");
        assert_eq!(run.runs.len(), 2, "第一条报错后不再跑后面的语句");
        assert!(run.runs[1].result.is_err());
        let check = run_script(
            &create_job(),
            &path,
            &["SELECT count(*) FROM e".to_string()],
            QUERY_ROW_LIMIT,
            false,
        );
        assert_eq!(
            rows_of(&check, 0),
            vec![vec!["0".to_string()]],
            "后面的 INSERT 确实没执行"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn run_script_cancelled_before_first_statement_runs_nothing() {
        let _guard = setup();
        let path = tmp_db("run_cancel");
        let job = create_job();
        job.cancel();
        assert!(job.is_cancelled());
        let stmts: Vec<String> = ["CREATE TABLE s(a INTEGER)", "INSERT INTO s VALUES (1)"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let run = run_script(&job, &path, &stmts, QUERY_ROW_LIMIT, true);
        assert!(run.cancelled);
        assert!(run.runs.is_empty(), "取消后不留半截回执");
        let check = run_script(
            &create_job(),
            &path,
            &["SELECT name FROM sqlite_master WHERE name = 's'".to_string()],
            QUERY_ROW_LIMIT,
            true,
        );
        assert!(rows_of(&check, 0).is_empty(), "语句确实一条没跑");
        let _ = std::fs::remove_file(&path);
    }

    /// 关闭连接不能等正在跑的查询：注册表锁只在查表时短暂持有，句柄各看各的。
    /// 旧实现里 UI 线程在「打开过程中」点关闭会卡在注册表锁上，界面冻死。
    #[test]
    fn close_does_not_wait_for_in_flight_query() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        const KEY: &str = "close-race";
        close(KEY);
        open(KEY, ":memory:").unwrap();
        let held = handle(KEY).unwrap();
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let _query = held.lock().unwrap();
            locked_tx.send(()).unwrap();
            let _ = release_rx.recv();
        });
        locked_rx.recv().unwrap();

        let started = Instant::now();
        close(KEY);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "关闭连接被在飞的查询拖住了"
        );
        assert!(!is_open(KEY), "关闭后注册表里不该还留着句柄");

        release_tx.send(()).unwrap();
        worker.join().unwrap();
    }
}
