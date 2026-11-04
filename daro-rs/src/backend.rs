//! UI 与真库驱动之间的分发层：页签 / 树只说「哪条连接的哪个库（哪个模式）」，
//! 走 SQLite 还是 PostgreSQL 在这里一次判定，免得每个取数、写回点都方言判断。
//!
//! mock 档（MySQL / SQL Server 等还没有真驱动）不从这里过：UI 保留各自的
//! 物化数据分支，`Target::of` 对这些引擎返回 None。
//!
//! 两个引擎的「库 / 模式」含义不同，落点形状也不同：
//! - SQLite：一个文件一条连接，库名是 `main` / ATTACH 进来的名字，没有模式层
//! - PostgreSQL：一条连接只看得见自己那个库，所以每个库各是一份连接；
//!   对象再由模式限定

use crate::catalog;
use crate::db_types::{ColMeta, ExecOutcome, GridChanges, ScriptRun, SelectPage};
use crate::mock::{Connection, DbKind};
use crate::{pg_driver, sqlite_driver};

/// 是否已有真驱动（决定树上要不要建连、页签要不要真取数）。
pub fn is_real(kind: DbKind) -> bool {
    matches!(kind, DbKind::SQLite | DbKind::PostgreSql)
}

/// 连接表单 → PG 档位。`db` 为空时兜到维护库 `postgres`（列库清单用的就是它）。
pub fn pg_params(conn: &Connection, db: &str) -> pg_driver::Params {
    let dbname = db.trim();
    pg_driver::Params {
        host: conn.host.trim().to_string(),
        port: if conn.port == 0 { 5432 } else { conn.port },
        user: conn.user.clone(),
        password: conn.password.clone(),
        dbname: if dbname.is_empty() {
            "postgres".to_string()
        } else {
            dbname.to_string()
        },
    }
}

/// 一个可寻址的真库落点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Sqlite {
        conn: String,
        db: String,
        path: String,
    },
    Pg {
        conn: String,
        db: String,
        schema: String,
        params: pg_driver::Params,
    },
}

impl Target {
    /// 由连接 + 页签的库 / 模式坐标得到；还没有真驱动的引擎返回 None。
    /// `schema` 传空时 PG 兜到 `public`。
    pub fn of(conn: &Connection, db: &str, schema: &str) -> Option<Target> {
        match conn.kind {
            DbKind::SQLite => Some(Target::Sqlite {
                conn: conn.name.clone(),
                db: db.to_string(),
                path: catalog::sqlite_path(conn),
            }),
            DbKind::PostgreSql => {
                let schema = schema.trim();
                Some(Target::Pg {
                    conn: conn.name.clone(),
                    db: db.to_string(),
                    schema: if schema.is_empty() {
                        "public".to_string()
                    } else {
                        schema.to_string()
                    },
                    params: pg_params(conn, db),
                })
            }
            _ => None,
        }
    }

    pub fn kind(&self) -> DbKind {
        match self {
            Target::Sqlite { .. } => DbKind::SQLite,
            Target::Pg { .. } => DbKind::PostgreSql,
        }
    }

    /// 驱动注册表的键（PG 每个库一条连接，键里必须带库名）。
    fn keyed(&self) -> String {
        match self {
            Target::Sqlite { conn, .. } => conn.clone(),
            Target::Pg { conn, db, .. } => pg_driver::key(conn, db),
        }
    }

    /// 驱动拼 SQL 用的限定名：SQLite 是库（ATTACH）名，PG 是模式名。
    fn space(&self) -> &str {
        match self {
            Target::Sqlite { db, .. } => db,
            Target::Pg { schema, .. } => schema,
        }
    }

    /// 每个真库操作前先走这一步：SQLite 确保文件句柄在，PG 按库惰性建连。
    fn ensure(&self) -> Result<(), String> {
        match self {
            Target::Sqlite { conn, path, .. } => sqlite_driver::ensure(conn, path),
            Target::Pg { params, .. } => pg_driver::ensure(&self.keyed(), params),
        }
    }

    /// 一次取回的最大行数（两档同值，各自驱动里定，这里只转发）。
    pub fn row_limit(&self) -> usize {
        match self {
            Target::Sqlite { .. } => sqlite_driver::QUERY_ROW_LIMIT,
            Target::Pg { .. } => pg_driver::QUERY_ROW_LIMIT,
        }
    }

    /// 表页取数（WHERE / ORDER BY / LIMIT / OFFSET 全部下推）。
    pub fn select_table(
        &self,
        table: &str,
        where_sql: Option<&str>,
        orders: &[(String, bool)],
        offset: usize,
        limit: usize,
    ) -> Result<SelectPage, String> {
        self.ensure()?;
        match self {
            Target::Sqlite { conn, .. } => sqlite_driver::select_table(
                conn,
                self.space(),
                table,
                where_sql,
                orders,
                offset,
                limit,
            ),
            Target::Pg { .. } => pg_driver::select_table(
                &self.keyed(),
                self.space(),
                table,
                where_sql,
                orders,
                offset,
                limit,
            ),
        }
    }

    /// 当前筛选条件下的总行数。
    pub fn count_table(&self, table: &str, where_sql: Option<&str>) -> Result<usize, String> {
        self.ensure()?;
        match self {
            Target::Sqlite { conn, .. } => {
                sqlite_driver::count_table(conn, self.space(), table, where_sql)
            }
            Target::Pg { .. } => pg_driver::count_table(&self.keyed(), self.space(), table, where_sql),
        }
    }

    /// 表页实际执行的 SQL（状态栏展示用）。SQLite 用 `SELECT *`，列清单不入库。
    pub fn page_sql(
        &self,
        cols: &[ColMeta],
        table: &str,
        where_sql: Option<&str>,
        orders: &[(String, bool)],
        limit: usize,
        offset: usize,
    ) -> String {
        match self {
            Target::Sqlite { db, .. } => {
                sqlite_driver::select_table_sql(db, table, where_sql, orders, limit, offset)
            }
            Target::Pg { .. } => pg_driver::select_table_sql(
                cols,
                self.space(),
                table,
                where_sql,
                orders,
                limit,
                offset,
            ),
        }
    }

    /// 该库（该模式）下全部表的列名，SQL 编辑器补全用。
    pub fn column_names(&self) -> Vec<String> {
        if self.ensure().is_err() {
            return Vec::new();
        }
        match self {
            Target::Sqlite { conn, .. } => sqlite_driver::all_column_names(conn, self.space()),
            Target::Pg { .. } => pg_driver::all_column_names(&self.keyed(), self.space()),
        }
    }

    /// 执行一条语句（查询页 / 建表等）。
    pub fn execute(&self, stmt: &str, max_rows: usize) -> Result<ExecOutcome, String> {
        self.ensure()?;
        match self {
            Target::Sqlite { conn, .. } => sqlite_driver::execute_stmt(conn, stmt, max_rows),
            Target::Pg { .. } => pg_driver::execute_stmt(&self.keyed(), stmt, max_rows),
        }
    }

    /// 网格工作集写回。
    pub fn apply(&self, ch: &GridChanges<'_>) -> Result<String, String> {
        self.ensure()?;
        match self {
            Target::Sqlite { conn, .. } => sqlite_driver::apply_changes(conn, self.space(), ch),
            Target::Pg { .. } => pg_driver::apply_changes(&self.keyed(), self.space(), ch),
        }
    }

    /// 是否适合丢到后台线程跑整段脚本。SQLite 的 `:memory:` 库只活在本进程
    /// 那条连接上，另开连接查不到内容，只能同步跑；PG 是远程服务器，恒后台。
    pub fn background_ok(&self) -> bool {
        match self {
            Target::Sqlite { path, .. } => !sqlite_driver::is_memory_path(path),
            Target::Pg { .. } => true,
        }
    }

    pub fn create_job(&self) -> Job {
        match self {
            Target::Sqlite { .. } => Job::Sqlite(sqlite_driver::create_job()),
            Target::Pg { .. } => Job::Pg(pg_driver::create_job()),
        }
    }

    /// 后台跑整段脚本（自己开连接，不占用 UI 线程那条）。
    pub fn run_script(&self, job: &Job, stmts: &[String]) -> ScriptRun {
        match (self, job) {
            (Target::Sqlite { path, .. }, Job::Sqlite(j)) => {
                sqlite_driver::run_script(j, path, stmts, sqlite_driver::QUERY_ROW_LIMIT)
            }
            (Target::Pg { params, .. }, Job::Pg(j)) => {
                pg_driver::run_script(j, params, stmts, pg_driver::QUERY_ROW_LIMIT)
            }
            // 页签挂的任务和后端不配套（改过连接类型没重开）：当作没跑成
            _ => ScriptRun {
                runs: Vec::new(),
                open_error: Some("连接类型已变更，请重新执行".to_string()),
                cancelled: false,
                rolled_back: false,
                aborted_tx: false,
                total_ms: 0,
            },
        }
    }

    /// 同步跑完一段脚本后收尾：连接还挂在半路事务里就回滚，免得下一次取数 /
    /// 写回撞上未提交事务。PG 的脚本事务态在脚本线程里跟，UI 这条连接没有
    /// 跨语句事务，恒 false。
    pub fn rollback_open_tx(&self) -> bool {
        match self {
            Target::Sqlite { conn, .. } => {
                sqlite_driver::in_transaction(conn) && sqlite_driver::rollback_if_open(conn)
            }
            Target::Pg { .. } => false,
        }
    }
}

/// 一次后台脚本的取消通道：两档的中断机制不同（SQLite 打自己那条连接，
/// PG 另开连接发 cancel 请求），UI 只需要「停止」按钮点下去。
#[derive(Clone)]
pub enum Job {
    Sqlite(sqlite_driver::Job),
    Pg(pg_driver::Job),
}

impl Job {
    pub fn cancel(&self) {
        match self {
            Job::Sqlite(j) => j.cancel(),
            Job::Pg(j) => j.cancel(),
        }
    }
}

// ── 连接生命周期 ─────────────────────────────────────────────────

/// 断开 / 删除连接：关掉该连接名下的全部真库句柄（PG 是每个库各一条）。
pub fn close_connection(conn: &Connection) {
    close_named(&conn.name, conn.kind);
}

/// 按「连接名 + 引擎」关句柄：编辑连接时参数已经换过，旧句柄只能按旧名字、
/// 旧引擎找。
pub fn close_named(conn: &str, kind: DbKind) {
    match kind {
        DbKind::SQLite => sqlite_driver::close(conn),
        DbKind::PostgreSql => pg_driver::close_prefix(conn),
        _ => {}
    }
}

/// 连接改名：SQLite 把活句柄跟着改名迁移；PG 的连接按「连接名 + 库」建，
/// 旧的直接关，重连时按新名字重建。
pub fn rename_connection(old_name: &str, new_name: &str, kind: DbKind, path: &str) {
    match kind {
        DbKind::SQLite => {
            if sqlite_driver::is_open(old_name) {
                sqlite_driver::close(old_name);
                let _ = sqlite_driver::open(new_name, path);
            }
        }
        DbKind::PostgreSql => pg_driver::close_prefix(old_name),
        _ => {}
    }
}

/// 打开连接（树上点开 / 连接对话框「确定」时）：SQLite 开文件，PG 连配置里
/// 指定的库；失败原样报出去，UI 负责上屏。
pub fn open_connection(conn: &Connection) -> Result<(), String> {
    match conn.kind {
        DbKind::SQLite => {
            let path = catalog::sqlite_path(conn);
            if path.is_empty() {
                return Err("连接没有库文件路径".to_string());
            }
            sqlite_driver::open(&conn.name, &path)
        }
        DbKind::PostgreSql => {
            let params = pg_params(conn, &conn.database);
            let key = pg_driver::key(&conn.name, &params.dbname);
            pg_driver::open(&key, &params)
        }
        _ => Ok(()),
    }
}

/// 「测试连接」：连上去跑一条探针语句，不注册进注册表。
pub fn probe(conn: &Connection) -> Result<(), String> {
    match conn.kind {
        DbKind::SQLite => sqlite_driver::probe(&catalog::sqlite_path(conn)),
        DbKind::PostgreSql => pg_driver::probe(&pg_params(conn, &conn.database)),
        _ => Err("该引擎还没有真驱动".to_string()),
    }
}
