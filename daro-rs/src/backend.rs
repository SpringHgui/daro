//! UI 与真库驱动之间的分发层：页签 / 树只说「哪条连接的哪个库（哪个模式）」，
//! 走 SQLite 还是 PostgreSQL 在这里一次判定，免得每个取数、写回点都方言判断。
//!
//! 没有真驱动的引擎不从这里过：UI 保留各自的物化数据分支，`Target::of`
//! 对这些引擎返回 None（Redis 是键值库，走自己的 KV 页签，也不进 Target）。
//!
//! 各引擎的「库 / 模式」含义不同，落点形状也不同：
//! - SQLite：一个文件一条连接，库名是 `main` / ATTACH 进来的名字，没有模式层
//! - PostgreSQL：一条连接只看得见自己那个库，所以每个库各是一份连接；
//!   对象再由模式限定
//! - SQL Server：一条连接看全部库，取数走三段 `[库].[模式].[表]`
//! - MongoDB：一条连接看全部库，库下面直接是集合 / 视图

use crate::catalog;
use crate::db_types::{ColMeta, ExecOutcome, GridChanges, ScriptRun, SelectPage};
use crate::mock::{Connection, DbKind};
use crate::{
    access_driver, mongo_driver, mysql_driver, pg_driver, redis_driver, sqlite_driver,
    sqlserver_driver,
};

/// 是否已有真驱动（决定树上要不要建连、页签要不要真取数）。
pub fn is_real(kind: DbKind) -> bool {
    matches!(
        kind,
        DbKind::SQLite
            | DbKind::Access
            | DbKind::PostgreSql
            | DbKind::MySql
            | DbKind::SqlServer
            | DbKind::MongoDb
            | DbKind::Redis
    )
}

/// 引擎层是否只读（MongoDB 走游标写回，Access 是纯 Rust 解析文件页）。
/// 网格编辑、建表 / 改表、导入、复制粘贴全部按这道门收放。
pub fn is_read_only(kind: DbKind) -> bool {
    matches!(kind, DbKind::MongoDb | DbKind::Access)
}

/// 这条连接走不走真库。
///
/// 引擎有真驱动 ≠ 这条连接连真库：标了 `demo` 的演示条目只提供假数据，
/// 树照样能展开看结构，但不会去连那台演示主机。
pub fn is_real_conn(conn: &Connection) -> bool {
    is_real(conn.kind) && !conn.demo
}

/// 连接表单里的密码（空串即无密码）。Access 的加密库走这个字段。
pub fn conn_password(conn: &Connection) -> Option<String> {
    (!conn.password.is_empty()).then(|| conn.password.clone())
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

/// 连接表单 → MySQL 档位。MySQL 一条连接能看全部库，`dbname` 只是登录时
/// 落在哪个库，取数一律走 `库.表` 全限定。
pub fn mysql_params(conn: &Connection) -> mysql_driver::Params {
    mysql_driver::Params {
        host: conn.host.trim().to_string(),
        port: if conn.port == 0 { 3306 } else { conn.port },
        user: conn.user.clone(),
        password: conn.password.clone(),
        dbname: conn.database.trim().to_string(),
    }
}

/// 连接表单 → SQL Server 档位。登录库留空由驱动兜到 `master`。
pub fn sqlserver_params(conn: &Connection, db: &str) -> sqlserver_driver::Params {
    let dbname = if db.trim().is_empty() {
        conn.database.trim().to_string()
    } else {
        db.trim().to_string()
    };
    sqlserver_driver::Params {
        host: conn.host.trim().to_string(),
        port: if conn.port == 0 { 1433 } else { conn.port },
        user: conn.user.clone(),
        password: conn.password.clone(),
        dbname,
    }
}

/// 连接表单 → MongoDB 档位。URI 的认证库固定 `admin`（与 Flutter 版一致）。
pub fn mongo_params(conn: &Connection) -> mongo_driver::Params {
    mongo_driver::Params {
        host: conn.host.trim().to_string(),
        port: if conn.port == 0 { 27017 } else { conn.port },
        user: conn.user.clone(),
        password: conn.password.clone(),
    }
}

/// 连接表单 → Redis 档位（库序号由 KV 页签自己选）。
pub fn redis_params(conn: &Connection) -> redis_driver::Params {
    redis_driver::Params {
        host: conn.host.trim().to_string(),
        port: if conn.port == 0 { 6379 } else { conn.port },
        user: conn.user.clone(),
        password: conn.password.clone(),
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
    /// Access 档只有一个库文件：`db` 坐标（文件名的库节点）不参与寻址，
    /// 取数直接按连接锁住的 `PageReader` 走。
    Access {
        conn: String,
        path: String,
        password: Option<String>,
    },
    Pg {
        conn: String,
        db: String,
        schema: String,
        params: pg_driver::Params,
    },
    MySql {
        conn: String,
        db: String,
        params: mysql_driver::Params,
    },
    SqlServer {
        conn: String,
        db: String,
        schema: String,
        params: sqlserver_driver::Params,
    },
    Mongo {
        conn: String,
        db: String,
        params: mongo_driver::Params,
    },
}

impl Target {
    /// 由连接 + 页签的库 / 模式坐标得到；还没有真驱动的引擎返回 None。
    /// `schema` 传空时 PG 兜到 `public`。
    pub fn of(conn: &Connection, db: &str, schema: &str) -> Option<Target> {
        // 演示条目只给假数据，不落真库
        if conn.demo {
            return None;
        }
        match conn.kind {
            DbKind::SQLite => Some(Target::Sqlite {
                conn: conn.name.clone(),
                db: db.to_string(),
                path: catalog::sqlite_path(conn),
            }),
            DbKind::Access => Some(Target::Access {
                conn: conn.name.clone(),
                path: catalog::access_path(conn),
                password: conn_password(conn),
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
            // MySQL 没有模式层：库下面直接是表 / 视图
            DbKind::MySql => {
                let db = db.trim();
                let mut params = mysql_params(conn);
                // 查询页的库下拉优先于连接表单里的默认库：后台脚本另开连接跑，
                // 不带库名的表名要靠登录默认库才能解析。
                if !db.is_empty() {
                    params.dbname = db.to_string();
                }
                Some(Target::MySql {
                    conn: conn.name.clone(),
                    db: db.to_string(),
                    params,
                })
            }
            DbKind::SqlServer => Some(Target::SqlServer {
                conn: conn.name.clone(),
                db: db.to_string(),
                schema: if schema.trim().is_empty() {
                    "dbo".to_string()
                } else {
                    schema.trim().to_string()
                },
                params: sqlserver_params(conn, db),
            }),
            DbKind::MongoDb => Some(Target::Mongo {
                conn: conn.name.clone(),
                db: db.to_string(),
                params: mongo_params(conn),
            }),
            // Redis 没有表 / SQL：键浏览走自己的页签
            _ => None,
        }
    }

    pub fn kind(&self) -> DbKind {
        match self {
            Target::Sqlite { .. } => DbKind::SQLite,
            Target::Access { .. } => DbKind::Access,
            Target::Pg { .. } => DbKind::PostgreSql,
            Target::MySql { .. } => DbKind::MySql,
            Target::SqlServer { .. } => DbKind::SqlServer,
            Target::Mongo { .. } => DbKind::MongoDb,
        }
    }

    /// 驱动注册表的键（PG 每个库一条连接，键里必须带库名；
    /// MySQL 一条连接看全部库，键就是连接名）。
    fn keyed(&self) -> String {
        match self {
            Target::Sqlite { conn, .. } | Target::Access { conn, .. } => conn.clone(),
            Target::Pg { conn, db, .. } => pg_driver::key(conn, db),
            Target::MySql { conn, .. } | Target::SqlServer { conn, .. } | Target::Mongo { conn, .. } => {
                conn.clone()
            }
        }
    }

    /// 驱动拼 SQL 用的限定名：SQLite 是库（ATTACH）名，PG 是模式名，
    /// MySQL 是库名，SQL Server 是模式名，MongoDB 是库名。
    pub(crate) fn space(&self) -> &str {
        match self {
            Target::Sqlite { db, .. } => db,
            // Access 没有库 / 模式限定：表名就是表名
            Target::Access { .. } => "",
            Target::Pg { schema, .. } => schema,
            Target::MySql { db, .. } => db,
            Target::SqlServer { schema, .. } => schema,
            Target::Mongo { db, .. } => db,
        }
    }

    /// 该方言的标识符引用形式（MySQL 反引号，SQL Server 方括号，其余双引号，
    /// 内部同名符双写）。
    pub fn quote(&self, ident: &str) -> String {
        match self {
            Target::MySql { .. } => format!("`{}`", ident.replace('`', "``")),
            Target::SqlServer { .. } => sqlserver_driver::quote_ident(ident),
            // Access 的标识符用方括号，内部 `]` 双写；只用于状态栏展示的 SQL
            Target::Access { .. } => format!("[{}]", ident.replace(']', "]]")),
            _ => format!("\"{}\"", ident.replace('"', "\"\"")),
        }
    }

    /// 限定名的前缀部分（含末尾的点）；没有前缀时是空串。
    pub fn qualified_prefix(&self) -> String {
        let space = self.space();
        if space.is_empty() {
            String::new()
        } else {
            format!("{}.", self.quote(space))
        }
    }

    /// DDL 用的表限定名（`库.表` / `模式.表` / SQL Server 三段），与取数侧
    /// 同一套方言口径。
    pub fn qualified_table(&self, table: &str) -> String {
        match self {
            Target::SqlServer { db, schema, .. } => sqlserver_driver::qualify(db, schema, table),
            _ => format!("{}{}", self.qualified_prefix(), self.quote(table)),
        }
    }

    /// 每个真库操作前先走这一步：SQLite 确保文件句柄在，PG 按库惰性建连，
    /// MySQL / SQL Server / MongoDB 按连接惰性建连。
    fn ensure(&self) -> Result<(), String> {
        match self {
            Target::Sqlite { conn, path, .. } => sqlite_driver::ensure(conn, path),
            Target::Access {
                conn, path, password, ..
            } => access_driver::ensure(conn, path, password.as_deref()),
            Target::Pg { params, .. } => pg_driver::ensure(&self.keyed(), params),
            Target::MySql { params, .. } => mysql_driver::ensure(&self.keyed(), params),
            Target::SqlServer { params, .. } => sqlserver_driver::ensure(&self.keyed(), params),
            Target::Mongo { params, .. } => mongo_driver::ensure(&self.keyed(), params),
        }
    }

    /// 一次取回的最大行数（各驱动里定，这里只转发）。
    pub fn row_limit(&self) -> usize {
        match self {
            Target::Sqlite { .. } => sqlite_driver::QUERY_ROW_LIMIT,
            Target::Access { .. } => access_driver::QUERY_ROW_LIMIT,
            Target::Pg { .. } => pg_driver::QUERY_ROW_LIMIT,
            Target::MySql { .. } => mysql_driver::QUERY_ROW_LIMIT,
            Target::SqlServer { .. } => sqlserver_driver::QUERY_ROW_LIMIT,
            Target::Mongo { .. } => mongo_driver::QUERY_ROW_LIMIT,
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
            Target::MySql { params, .. } => mysql_driver::select_table(
                &self.keyed(),
                params,
                self.space(),
                table,
                where_sql,
                orders,
                offset,
                limit,
            ),
            Target::SqlServer {
                db, schema, params, ..
            } => sqlserver_driver::select_table(
                &self.keyed(),
                params,
                db,
                schema,
                table,
                where_sql,
                orders,
                offset,
                limit,
            ),
            Target::Mongo { db, params, .. } => mongo_driver::select_docs(
                &self.keyed(),
                params,
                db,
                table,
                where_sql,
                orders,
                offset,
                limit,
            ),
            // Access 没有可下推的 WHERE / ORDER BY：页签走整表物化 + 本地筛选
            // 排序（见界面 `load_table_page`），这里只按偏移交付数据。
            Target::Access { conn, .. } => access_driver::select_page(conn, table, offset, limit),
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
            Target::MySql { .. } => {
                mysql_driver::count_table(&self.keyed(), self.space(), table, where_sql)
            }
            Target::SqlServer {
                db, schema, params, ..
            } => sqlserver_driver::count_table(
                &self.keyed(),
                params,
                db,
                schema,
                table,
                where_sql,
            ),
            Target::Mongo { db, params, .. } => {
                mongo_driver::count_docs(&self.keyed(), params, db, table)
            }
            // 行数取表头缓存；本地筛选下的计数由界面按物化行算
            Target::Access { conn, .. } => access_driver::count_rows(conn, table),
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
            Target::MySql { .. } => mysql_driver::select_table_sql(
                cols,
                self.space(),
                table,
                where_sql,
                orders,
                limit,
                offset,
            ),
            Target::SqlServer {
                db, schema, ..
            } => sqlserver_driver::select_table_sql(
                cols, db, schema, table, where_sql, orders, limit, offset,
            ),
            Target::Mongo { db, .. } => {
                mongo_driver::page_sql(db, table, orders, limit, offset)
            }
            // Access 不跑 SQL：状态栏只展示等价的「整表读取」
            Target::Access { .. } => format!("SELECT * FROM {}", self.qualified_table(table)),
        }
    }

    /// 该库（该模式）下「表 → 列」，SQL 编辑器补全用。一次目录查询取回，
    /// 替代旧版「全库扁名列名」——那样没法做别名 / 限定名补全。
    pub fn columns_by_table(&self) -> Vec<(String, Vec<String>)> {
        if self.ensure().is_err() {
            return Vec::new();
        }
        match self {
            Target::Sqlite { conn, .. } => {
                sqlite_driver::all_columns_by_table(conn, self.space())
            }
            Target::Pg { .. } => pg_driver::all_columns_by_table(&self.keyed(), self.space()),
            Target::MySql { params, .. } => {
                mysql_driver::all_columns_by_table(params, self.space())
            }
            Target::SqlServer {
                schema, params, ..
            } => sqlserver_driver::all_columns_by_table(&self.keyed(), params, schema),
            Target::Access { conn, .. } => access_driver::all_columns_by_table(conn),
            // MongoDB 没有固定列：补全只能按集合名做，列名留给网格按文档现算
            Target::Mongo { .. } => Vec::new(),
        }
    }

    /// 「解释」实际下发的语句。SQLite 的 `EXPLAIN` 出的是字节码，可读计划
    /// 要 `EXPLAIN QUERY PLAN`；MySQL / PG 直接用 `EXPLAIN`。用户已经自己写了
    /// EXPLAIN 就原样执行，不叠第二层。
    pub fn explain_sql(&self, stmt: &str) -> String {
        let stmt = stmt.trim();
        let already = stmt
            .get(..7)
            .is_some_and(|head| head.eq_ignore_ascii_case("explain"));
        if already {
            return stmt.to_string();
        }
        match self {
            Target::Sqlite { .. } => format!("EXPLAIN QUERY PLAN {stmt}"),
            Target::SqlServer { .. } => format!("SET SHOWPLAN_ALL ON; {stmt}"),
            // Access 不执行 SQL，没有可解释的执行计划
            Target::Access { .. } | Target::Mongo { .. } => stmt.to_string(),
            Target::Pg { .. } | Target::MySql { .. } => format!("EXPLAIN {stmt}"),
        }
    }

    /// 执行一条语句（查询页 / 建表等）。MongoDB 没有 SQL 语句这一层。
    pub fn execute(&self, stmt: &str, max_rows: usize) -> Result<ExecOutcome, String> {
        self.ensure()?;
        match self {
            Target::Sqlite { conn, .. } => sqlite_driver::execute_stmt(conn, stmt, max_rows),
            Target::Pg { .. } => pg_driver::execute_stmt(&self.keyed(), stmt, max_rows),
            Target::MySql { .. } => {
                mysql_driver::execute_stmt(&self.keyed(), self.space(), stmt, max_rows)
            }
            Target::SqlServer { .. } => sqlserver_driver::execute_stmt(&self.keyed(), stmt, max_rows),
            Target::Mongo { .. } => Err("MongoDB 不支持 SQL 语句执行，请使用集合浏览".to_string()),
            Target::Access { .. } => {
                Err("Access 连接为只读，不支持执行 SQL 语句".to_string())
            }
        }
    }

    /// 网格工作集写回。MongoDB / Access 只读。
    pub fn apply(&self, ch: &GridChanges<'_>) -> Result<String, String> {
        self.ensure()?;
        match self {
            Target::Sqlite { conn, .. } => sqlite_driver::apply_changes(conn, self.space(), ch),
            Target::Pg { .. } => pg_driver::apply_changes(&self.keyed(), self.space(), ch),
            Target::MySql { .. } => mysql_driver::apply_changes(&self.keyed(), self.space(), ch),
            Target::SqlServer {
                db, schema, params, ..
            } => sqlserver_driver::apply_changes(&self.keyed(), params, db, schema, ch),
            Target::Mongo { .. } => Err("MongoDB 连接为只读，暂不支持写回".to_string()),
            Target::Access { .. } => Err("Access 连接为只读，暂不支持写回".to_string()),
        }
    }

    /// 是否适合丢到后台线程跑整段脚本。SQLite 的 `:memory:` 库只活在本进程
    /// 那条连接上，另开连接查不到内容，只能同步跑；其余远程服务器恒后台。
    pub fn background_ok(&self) -> bool {
        match self {
            Target::Sqlite { path, .. } => !sqlite_driver::is_memory_path(path),
            Target::Access { .. } | Target::Mongo { .. } => false,
            _ => true,
        }
    }

    pub fn create_job(&self) -> Job {
        match self {
            Target::Sqlite { .. } => Job::Sqlite(sqlite_driver::create_job()),
            Target::Pg { .. } => Job::Pg(pg_driver::create_job()),
            Target::MySql { params, .. } => Job::MySql(mysql_driver::create_job(params)),
            Target::SqlServer { params, .. } => {
                Job::SqlServer(sqlserver_driver::create_job(params))
            }
            Target::Access { .. } | Target::Mongo { .. } => Job::Unsupported,
        }
    }

    /// 后台跑整段脚本（自己开连接，不占用 UI 线程那条）。
    /// `continue_on_error` 对应运行下拉的「遇到错误时继续」：false 时第一条
    /// 报错就收手，后面的语句不再执行。
    pub fn run_script(
        &self,
        job: &Job,
        stmts: &[String],
        continue_on_error: bool,
    ) -> ScriptRun {
        match (self, job) {
            (Target::Sqlite { path, .. }, Job::Sqlite(j)) => sqlite_driver::run_script(
                j,
                path,
                stmts,
                sqlite_driver::QUERY_ROW_LIMIT,
                continue_on_error,
            ),
            (Target::Pg { params, .. }, Job::Pg(j)) => pg_driver::run_script(
                j,
                params,
                stmts,
                pg_driver::QUERY_ROW_LIMIT,
                continue_on_error,
            ),
            (Target::MySql { params, .. }, Job::MySql(j)) => mysql_driver::run_script(
                j,
                params,
                stmts,
                mysql_driver::QUERY_ROW_LIMIT,
                continue_on_error,
            ),
            (Target::SqlServer { .. }, Job::SqlServer(j)) => {
                sqlserver_driver::run_script(
                    j,
                    stmts,
                    sqlserver_driver::QUERY_ROW_LIMIT,
                    continue_on_error,
                )
            }
            // Access 只读：脚本入口在功能区就被置灰，这里兜住直接调用
            (Target::Access { .. }, _) => ScriptRun {
                runs: Vec::new(),
                open_error: Some("Access 连接为只读，不支持运行 SQL 脚本".to_string()),
                cancelled: false,
                rolled_back: false,
                aborted_tx: false,
                total_ms: 0,
            },
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
            Target::Pg { .. }
            | Target::MySql { .. }
            | Target::SqlServer { .. }
            | Target::Mongo { .. }
            | Target::Access { .. } => false,
        }
    }
}

/// 一次后台脚本的取消通道：各档的中断机制不同（SQLite 打自己那条连接，
/// PG 另开连接发 cancel 请求，MySQL 另开连接发 KILL QUERY，SQL Server 只在
/// 语句之间收手），UI 只需要「停止」按钮点下去。
#[derive(Clone)]
pub enum Job {
    Sqlite(sqlite_driver::Job),
    Pg(pg_driver::Job),
    MySql(mysql_driver::Job),
    SqlServer(sqlserver_driver::Job),
    /// 该引擎不支持后台脚本（MongoDB）
    Unsupported,
}

impl Job {
    /// 只置取消标志：不碰网络，UI 线程直接调用也安全。语句之间的检查靠它。
    pub fn request_stop(&self) {
        match self {
            Job::Sqlite(j) => j.request_stop(),
            Job::Pg(j) => j.request_stop(),
            Job::MySql(j) => j.request_stop(),
            Job::SqlServer(j) => j.request_stop(),
            Job::Unsupported => {}
        }
    }

    /// 掐断正在跑的那条语句：MySQL / PG 要另开连接发请求，可能阻塞，
    /// 调用方应放到后台线程。
    pub fn interrupt(&self) {
        match self {
            Job::Sqlite(j) => j.interrupt(),
            Job::Pg(j) => j.interrupt(),
            Job::MySql(j) => j.interrupt(),
            Job::SqlServer(j) => j.interrupt(),
            Job::Unsupported => {}
        }
    }

    /// 置标志并中断（测试 / 同步场景用）。
    pub fn cancel(&self) {
        self.request_stop();
        self.interrupt();
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
        DbKind::Access => access_driver::close(conn),
        DbKind::PostgreSql => pg_driver::close_prefix(conn),
        DbKind::MySql => mysql_driver::close(conn),
        DbKind::SqlServer => sqlserver_driver::close_prefix(conn),
        DbKind::MongoDb => mongo_driver::close(conn),
        DbKind::Redis => redis_driver::close(conn),
    }
}

/// 连接改名：SQLite 把活句柄跟着改名迁移；PG 的连接按「连接名 + 库」建，
/// 旧的直接关，重连时按新名字重建；MySQL 一条连接看全部库，同理关掉重建。
pub fn rename_connection(old_name: &str, new_name: &str, kind: DbKind, path: &str) {
    match kind {
        DbKind::SQLite => {
            if sqlite_driver::is_open(old_name) {
                sqlite_driver::close(old_name);
                let _ = sqlite_driver::open(new_name, path);
            }
        }
        // Access 句柄不带参数：关掉即可，下次取数按新名字惰性重开
        DbKind::Access => access_driver::close(old_name),
        DbKind::PostgreSql => pg_driver::close_prefix(old_name),
        DbKind::MySql => mysql_driver::close(old_name),
        DbKind::SqlServer => sqlserver_driver::close_prefix(old_name),
        DbKind::MongoDb => mongo_driver::close(old_name),
        DbKind::Redis => redis_driver::close(old_name),
    }
}

/// 打开连接（树上点开 / 连接对话框「确定」时）：SQLite 开文件，PG 连配置里
/// 指定的库；失败原样报出去，UI 负责上屏。
pub fn open_connection(conn: &Connection) -> Result<(), String> {
    // 演示条目不连真库：树照常展开，展开出来的清单是假数据
    if conn.demo {
        return Ok(());
    }
    match conn.kind {
        DbKind::SQLite => {
            let path = catalog::sqlite_path(conn);
            if path.is_empty() {
                return Err("连接没有库文件路径".to_string());
            }
            sqlite_driver::open(&conn.name, &path)
        }
        DbKind::Access => {
            let path = catalog::access_path(conn);
            if path.is_empty() {
                return Err("连接没有数据库文件路径".to_string());
            }
            access_driver::open(&conn.name, &path, conn_password(conn).as_deref())
        }
        DbKind::PostgreSql => {
            let params = pg_params(conn, &conn.database);
            let key = pg_driver::key(&conn.name, &params.dbname);
            pg_driver::open(&key, &params)
        }
        DbKind::MySql => mysql_driver::open(&conn.name, &mysql_params(conn)),
        DbKind::SqlServer => {
            sqlserver_driver::open(&conn.name, &sqlserver_params(conn, &conn.database))
        }
        DbKind::MongoDb => mongo_driver::open(&conn.name, &mongo_params(conn)),
        DbKind::Redis => redis_driver::open(&conn.name, &redis_params(conn)),
    }
}

/// 「测试连接」：连上去跑一条探针语句，不注册进注册表。
pub fn probe(conn: &Connection) -> Result<(), String> {
    if conn.demo {
        return Err("这是演示连接，不连真实数据库".to_string());
    }
    match conn.kind {
        DbKind::SQLite => sqlite_driver::probe(&catalog::sqlite_path(conn)),
        DbKind::Access => {
            access_driver::probe(&catalog::access_path(conn), conn_password(conn).as_deref())
        }
        DbKind::PostgreSql => pg_driver::probe(&pg_params(conn, &conn.database)),
        DbKind::MySql => mysql_driver::probe(&mysql_params(conn)),
        DbKind::SqlServer => sqlserver_driver::probe(&sqlserver_params(conn, &conn.database)),
        DbKind::MongoDb => mongo_driver::probe(&mongo_params(conn)),
        DbKind::Redis => redis_driver::probe(&redis_params(conn)),
    }
}
