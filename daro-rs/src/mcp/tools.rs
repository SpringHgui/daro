//! MCP 工具层：把 daro 的连接 / 目录 / 查询能力按连接名暴露给 agent。
//!
//! 每次调用都现读 `mcp.json` 和 `connections.json`：用户在界面里改档、改密码、
//! 加连接，下一句工具调用就按新值判。数据库口令只用来连库，既不出现在入参里，
//! 也不出现在任何返回里。
//!
//! 目录类调用丢到阻塞线程池，用只读档超时包住；`daro_execute_query` 另走一条
//! 独立连接并挂取消通道，超时会真正中断语句（SQL Server 驱动没有语句级中断，
//! 只能等它跑完再收回连接），慢查询也不会堵住后续工具调用。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::backend;
use crate::catalog;
use crate::mcp::{self, policy, store};
use crate::mock::{Connection, DbKind};
use crate::{mysql_driver, pg_driver, sqlite_driver, sqlserver_driver};

/// 一次工具调用期间的 active 计数，心跳里给界面看「agent 正在问库」。
struct Active(Arc<AtomicUsize>);

impl Active {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Active(counter)
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// 工具层内的错误：最终都以 `isError` 结果回给 agent，不走协议错误通道。
struct ToolError {
    code: &'static str,
    message: String,
    hint: &'static str,
}

impl ToolError {
    fn new(code: &'static str, message: impl Into<String>, hint: &'static str) -> Self {
        ToolError {
            code,
            message: message.into(),
            hint,
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        ToolError::new("INVALID_PARAMS", message, "检查参数名与取值后重试")
    }

    fn internal(message: impl Into<String>) -> Self {
        ToolError::new("INTERNAL", message, "这是 daro 内部错误，可稍后重试")
    }

    fn database(message: impl Into<String>) -> Self {
        ToolError::new(
            "DATABASE_ERROR",
            message,
            "数据库引擎拒绝了该语句；按 message 修正语句本身（对象名/语法/权限），与 MCP 权限设置无关",
        )
    }

    fn unreachable_kind(kind: DbKind) -> Self {
        ToolError::new(
            "DRIVER_UNSUPPORTED",
            format!("{} 连接暂不提供 SQL 工具（键值 / 文档浏览请在 daro 界面里操作）", kind.label()),
            "改用 MySQL / PostgreSQL / SQL Server / SQLite 连接",
        )
    }
}

fn tool_ok(value: Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(
        value.to_string(),
    )]))
}

fn tool_error(err: ToolError) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::error(vec![ContentBlock::text(
        json!({
            "error": {
                "code": err.code,
                "message": err.message,
                "hint": err.hint,
            }
        })
        .to_string(),
    )]))
}

// ------------------------------------------------------------------ 参数

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ConnArgs {
    /// 连接名，见 daro_list_connections
    connection: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DatabaseArgs {
    /// 连接名，见 daro_list_connections
    connection: String,
    /// 数据库名，见 daro_list_databases
    database: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ListTablesArgs {
    /// 连接名，见 daro_list_connections
    connection: String,
    /// 数据库名，见 daro_list_databases
    database: String,
    /// 模式名；PostgreSQL 缺省 public，SQL Server 缺省 dbo
    #[serde(default)]
    schema: String,
    /// `table`（默认）或 `view`
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DescribeArgs {
    /// 连接名
    connection: String,
    /// 数据库名
    database: String,
    /// 模式名，可省略
    #[serde(default)]
    schema: String,
    /// 表名
    table: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ExecuteArgs {
    /// 连接名
    connection: String,
    /// 数据库名
    database: String,
    /// 模式名，可省略
    #[serde(default)]
    schema: String,
    /// 单条 SQL 语句（多条语句一律按完全访问处理）
    sql: String,
    /// 期望行数上限；缺省用策略默认值，超出硬上限自动夹取
    #[serde(default)]
    max_rows: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PreviewArgs {
    /// 连接名
    connection: String,
    /// 数据库名
    database: String,
    /// 模式名，可省略
    #[serde(default)]
    schema: String,
    /// 表名
    table: String,
    /// 行数，受策略夹取
    #[serde(default)]
    limit: Option<usize>,
    /// WHERE 片段（不含关键字）
    #[serde(default, rename = "where")]
    r#where: Option<String>,
    /// ORDER BY 片段（不含关键字），如 `id DESC, name`
    #[serde(default)]
    order_by: Option<String>,
    /// 是否额外 COUNT(*)（大表慎用）
    #[serde(default)]
    count: bool,
}

// ------------------------------------------------------------------ 服务

#[derive(Clone)]
pub struct DaroMcp {
    active: Arc<AtomicUsize>,
}

#[tool_router]
impl DaroMcp {
    pub fn new(active: Arc<AtomicUsize>) -> Self {
        DaroMcp { active }
    }

    #[tool(
        name = "daro_list_connections",
        description = "List saved daro connections with type/host/port/user. Passwords are never returned. Pick a connection from here before calling other tools. supported=false means the engine has no SQL tools for MCP (demo entries, MongoDB, Redis)."
    )]
    async fn daro_list_connections(&self) -> Result<CallToolResult, McpError> {
        let _active = Active::new(self.active.clone());
        let cfg = match mcp::current_config() {
            Ok(c) => c,
            Err(e) => return tool_error(ToolError::internal(e)),
        };
        if !cfg.enabled {
            return tool_error(disabled());
        }
        let all = match store::load_connections() {
            Ok(v) => v,
            Err(e) => return tool_error(ToolError::internal(e)),
        };
        store::sync_handles(&all);
        let items: Vec<Value> = all
            .iter()
            .filter(|c| cfg.visible_connection(&c.name))
            .map(|c| {
                json!({
                    "name": c.name,
                    "type": c.kind.label(),
                    "host": c.host,
                    "port": c.port,
                    "username": c.user,
                    "group": c.group,
                    "database": c.database,
                    "supported": sql_kind(c.kind) && !c.demo,
                    "passwordMissing": needs_password(c.kind) && c.password.is_empty(),
                    "demo": c.demo,
                })
            })
            .collect();
        tool_ok(json!({ "connections": items }))
    }

    #[tool(
        name = "daro_list_databases",
        description = "List databases reachable through one saved connection (SQLite lists main plus ATTACHed names)."
    )]
    async fn daro_list_databases(
        &self,
        Parameters(args): Parameters<ConnArgs>,
    ) -> Result<CallToolResult, McpError> {
        let _active = Active::new(self.active.clone());
        let cfg = match mcp::current_config() {
            Ok(c) => c,
            Err(e) => return tool_error(ToolError::internal(e)),
        };
        if !cfg.enabled {
            return tool_error(disabled());
        }
        let conn = match resolve(&cfg, &args.connection) {
            Ok(c) => c,
            Err(e) => return tool_error(e),
        };
        let result = blocking(policy::Mode::Readonly, move || match conn.kind {
            DbKind::SQLite => {
                sqlite_driver::ensure(&conn.name, &catalog::sqlite_path(&conn))?;
                Ok(sqlite_driver::list_databases(&conn.name))
            }
            DbKind::PostgreSql => {
                let p = backend::pg_params(&conn, "");
                let key = pg_driver::key(&conn.name, &p.dbname);
                pg_driver::ensure(&key, &p)?;
                pg_driver::try_list_databases(&key)
            }
            DbKind::MySql => mysql_driver::try_list_databases(&backend::mysql_params(&conn)),
            DbKind::SqlServer => {
                let p = backend::sqlserver_params(&conn, "");
                sqlserver_driver::try_list_databases(&conn.name, &p)
            }
            other => Err(format!("{} 不支持 SQL 工具", other.label())),
        })
        .await;
        match result {
            Ok(databases) => tool_ok(json!({ "databases": databases })),
            Err(e) => tool_error(e),
        }
    }

    #[tool(
        name = "daro_list_schemas",
        description = "List schemas of one database. PostgreSQL and SQL Server return their schemas; MySQL and SQLite legitimately return an empty list."
    )]
    async fn daro_list_schemas(
        &self,
        Parameters(args): Parameters<DatabaseArgs>,
    ) -> Result<CallToolResult, McpError> {
        let _active = Active::new(self.active.clone());
        let cfg = match mcp::current_config() {
            Ok(c) => c,
            Err(e) => return tool_error(ToolError::internal(e)),
        };
        if !cfg.enabled {
            return tool_error(disabled());
        }
        let conn = match resolve(&cfg, &args.connection) {
            Ok(c) => c,
            Err(e) => return tool_error(e),
        };
        let database = match require_database(&args.database) {
            Ok(db) => db,
            Err(e) => return tool_error(e),
        };
        let db_for_reply = database.clone();
        let result = blocking(policy::Mode::Readonly, move || match conn.kind {
            DbKind::PostgreSql => {
                let p = backend::pg_params(&conn, &database);
                let key = pg_driver::key(&conn.name, &p.dbname);
                pg_driver::ensure(&key, &p)?;
                pg_driver::try_list_schemas(&key)
            }
            DbKind::SqlServer => {
                let p = backend::sqlserver_params(&conn, &database);
                sqlserver_driver::try_list_schemas(&conn.name, &p)
            }
            // MySQL 库即模式；SQLite 没有模式层
            _ => Ok(Vec::new()),
        })
        .await;
        match result {
            Ok(schemas) => tool_ok(json!({ "database": db_for_reply, "schemas": schemas })),
            Err(e) => tool_error(e),
        }
    }

    #[tool(
        name = "daro_list_tables",
        description = "List tables (default) or views in a database/schema. Names only; use daro_describe_table for columns."
    )]
    async fn daro_list_tables(
        &self,
        Parameters(args): Parameters<ListTablesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let _active = Active::new(self.active.clone());
        let cfg = match mcp::current_config() {
            Ok(c) => c,
            Err(e) => return tool_error(ToolError::internal(e)),
        };
        if !cfg.enabled {
            return tool_error(disabled());
        }
        let conn = match resolve(&cfg, &args.connection) {
            Ok(c) => c,
            Err(e) => return tool_error(e),
        };
        let database = match require_database(&args.database) {
            Ok(db) => db,
            Err(e) => return tool_error(e),
        };
        let kind = match args.kind.as_deref().unwrap_or("table") {
            "table" => true,
            "view" => false,
            other => {
                return tool_error(ToolError::invalid(format!(
                    "kind 只能是 table / view，收到 {other:?}"
                )));
            }
        };
        let view = !kind;
        let schema = args.schema.clone();
        let kind_label = if view { "view" } else { "table" };
        let result = blocking(policy::Mode::Readonly, move || match conn.kind {
            DbKind::SQLite => {
                sqlite_driver::ensure(&conn.name, &catalog::sqlite_path(&conn))?;
                if view {
                    sqlite_driver::try_list_views(&conn.name, &database)
                } else {
                    sqlite_driver::try_list_tables(&conn.name, &database)
                }
            }
            DbKind::PostgreSql => {
                let p = backend::pg_params(&conn, &database);
                let key = pg_driver::key(&conn.name, &p.dbname);
                pg_driver::ensure(&key, &p)?;
                let schema = if schema.trim().is_empty() {
                    "public"
                } else {
                    schema.trim()
                };
                if view {
                    pg_driver::try_list_views(&key, schema)
                } else {
                    pg_driver::try_list_tables(&key, schema)
                }
            }
            DbKind::MySql => {
                let p = backend::mysql_params(&conn);
                if view {
                    mysql_driver::try_list_views(&p, &database)
                } else {
                    mysql_driver::try_list_tables(&p, &database)
                }
            }
            DbKind::SqlServer => {
                let p = backend::sqlserver_params(&conn, &database);
                let schema = if schema.trim().is_empty() {
                    "dbo"
                } else {
                    schema.trim()
                };
                if view {
                    sqlserver_driver::try_list_views(&conn.name, &p, schema)
                } else {
                    sqlserver_driver::try_list_tables(&conn.name, &p, schema)
                }
            }
            other => Err(format!("{} 不支持 SQL 工具", other.label())),
        })
        .await;
        match result {
            Ok(tables) => {
                let items: Vec<Value> = tables.iter().map(|t| json!({ "name": t })).collect();
                tool_ok(json!({ "kind": kind_label, "tables": items }))
            }
            Err(e) => tool_error(e),
        }
    }

    #[tool(
        name = "daro_describe_table",
        description = "Describe a table: column name, type, primaryKey and writable flags. Indexes and foreign keys are not exposed yet."
    )]
    async fn daro_describe_table(
        &self,
        Parameters(args): Parameters<DescribeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let _active = Active::new(self.active.clone());
        let cfg = match mcp::current_config() {
            Ok(c) => c,
            Err(e) => return tool_error(ToolError::internal(e)),
        };
        if !cfg.enabled {
            return tool_error(disabled());
        }
        let conn = match resolve(&cfg, &args.connection) {
            Ok(c) => c,
            Err(e) => return tool_error(e),
        };
        let database = match require_database(&args.database) {
            Ok(db) => db,
            Err(e) => return tool_error(e),
        };
        let table = args.table.trim().to_string();
        if table.is_empty() {
            return tool_error(ToolError::invalid("table 不能为空"));
        }
        let schema = args.schema.clone();
        let table_for_reply = table.clone();
        let result = blocking(policy::Mode::Readonly, move || match conn.kind {
            DbKind::SQLite => {
                sqlite_driver::ensure(&conn.name, &catalog::sqlite_path(&conn))?;
                sqlite_driver::try_table_columns(&conn.name, &database, &table)
            }
            DbKind::PostgreSql => {
                let p = backend::pg_params(&conn, &database);
                let key = pg_driver::key(&conn.name, &p.dbname);
                pg_driver::ensure(&key, &p)?;
                let schema = if schema.trim().is_empty() {
                    "public"
                } else {
                    schema.trim()
                };
                pg_driver::try_table_columns(&key, schema, &table)
            }
            DbKind::MySql => {
                mysql_driver::try_table_columns(&backend::mysql_params(&conn), &database, &table)
            }
            DbKind::SqlServer => {
                let p = backend::sqlserver_params(&conn, &database);
                let schema = if schema.trim().is_empty() {
                    "dbo"
                } else {
                    schema.trim()
                };
                sqlserver_driver::try_table_columns(&conn.name, &p, schema, &table)
            }
            other => Err(format!("{} 不支持 SQL 工具", other.label())),
        })
        .await;
        match result {
            Ok(cols) => {
                let columns: Vec<Value> = cols
                    .iter()
                    .map(|c| {
                        json!({
                            "name": c.name,
                            "type": c.ty,
                            "primaryKey": c.pk,
                            "writable": c.writable,
                        })
                    })
                    .collect();
                tool_ok(json!({ "table": table_for_reply, "columns": columns }))
            }
            Err(e) => tool_error(e),
        }
    }

    #[tool(
        name = "daro_execute_query",
        description = "Run one SQL statement and return its result. In readonly mode only read-only statements are allowed; INSERT and UPDATE/DELETE with a real WHERE need readWrite; DDL, unscoped writes and unrecognised statements need full access. maxRows is clamped by the configured hard cap."
    )]
    async fn daro_execute_query(
        &self,
        Parameters(args): Parameters<ExecuteArgs>,
    ) -> Result<CallToolResult, McpError> {
        let _active = Active::new(self.active.clone());
        let cfg = match mcp::current_config() {
            Ok(c) => c,
            Err(e) => return tool_error(ToolError::internal(e)),
        };
        if !cfg.enabled {
            return tool_error(disabled());
        }
        let conn = match resolve(&cfg, &args.connection) {
            Ok(c) => c,
            Err(e) => return tool_error(e),
        };
        let database = match require_database(&args.database) {
            Ok(db) => db,
            Err(e) => return tool_error(e),
        };
        let sql = args.sql.trim().to_string();
        if sql.is_empty() {
            return tool_error(ToolError::invalid("sql 不能为空"));
        }
        let class = policy::classify(&sql);
        let mode = cfg.default_mode;
        if !policy::allows(mode, class) {
            return tool_error(denied_sql(mode, class, &sql));
        }
        let limit = cfg.clamp_rows(args.max_rows);
        let schema = args.schema.clone();
        let Some(target) = backend::Target::of(&conn, &database, &schema) else {
            return tool_error(ToolError::database("该连接不能作为真库访问"));
        };
        let started = Instant::now();
        let result = run_cancellable(target, sql, limit, mode.timeout_secs()).await;
        let elapsed = started.elapsed().as_millis() as u64;
        match result {
            Ok(outcome) => {
                let mut body = json!({ "elapsedMs": elapsed });
                if let Some(page) = outcome.select {
                    let row_count = page.rows.len();
                    body["columns"] = columns_json(&page.columns);
                    body["rows"] = json!(page.rows);
                    body["rowCount"] = json!(row_count);
                    body["truncated"] = json!(page.truncated);
                } else {
                    body["affectedRows"] = json!(outcome.affected);
                }
                if !outcome.message.is_empty() {
                    body["message"] = json!(outcome.message);
                }
                tool_ok(body)
            }
            Err(e) => tool_error(e),
        }
    }

    #[tool(
        name = "daro_preview_table",
        description = "Preview table rows (server-side paging). Values are strings and SQL NULL shows as \"NULL\". where/orderBy take fragments without the keywords, e.g. where: \"status = 1\", orderBy: \"id DESC\"."
    )]
    async fn daro_preview_table(
        &self,
        Parameters(args): Parameters<PreviewArgs>,
    ) -> Result<CallToolResult, McpError> {
        let _active = Active::new(self.active.clone());
        let cfg = match mcp::current_config() {
            Ok(c) => c,
            Err(e) => return tool_error(ToolError::internal(e)),
        };
        if !cfg.enabled {
            return tool_error(disabled());
        }
        let conn = match resolve(&cfg, &args.connection) {
            Ok(c) => c,
            Err(e) => return tool_error(e),
        };
        let database = match require_database(&args.database) {
            Ok(db) => db,
            Err(e) => return tool_error(e),
        };
        let table = args.table.trim().to_string();
        if table.is_empty() {
            return tool_error(ToolError::invalid("table 不能为空"));
        }
        let where_sql = match clean_fragment(args.r#where.as_deref().unwrap_or(""), "WHERE") {
            Ok(w) => w,
            Err(e) => return tool_error(e),
        };
        let orders = match args.order_by.as_deref() {
            Some(raw) if !raw.trim().is_empty() => match parse_orders(raw) {
                Ok(o) => o,
                Err(e) => return tool_error(e),
            },
            _ => Vec::new(),
        };
        let limit = cfg.clamp_rows(args.limit);
        let want_count = args.count;
        let schema = args.schema.clone();
        let table_for_reply = table.clone();
        let result = blocking(policy::Mode::Readonly, move || {
            let target = backend::Target::of(&conn, &database, &schema)
                .ok_or_else(|| "该连接不能作为真库访问".to_string())?;
            let page = target.select_table(&table, where_sql.as_deref(), &orders, 0, limit)?;
            let total = if want_count {
                Some(target.count_table(&table, where_sql.as_deref())?)
            } else {
                None
            };
            Ok((page, total))
        })
        .await;
        match result {
            Ok((page, total)) => {
                let row_count = page.rows.len();
                let mut body = json!({
                    "table": table_for_reply,
                    "columns": columns_json(&page.columns),
                    "rows": page.rows,
                    "rowCount": row_count,
                    "truncated": page.truncated,
                });
                if let Some(total) = total {
                    body["totalRows"] = json!(total);
                }
                tool_ok(body)
            }
            Err(e) => tool_error(e),
        }
    }
}

#[tool_handler]
impl ServerHandler for DaroMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("daro", env!("CARGO_PKG_VERSION")))
            .with_instructions(mcp::INSTRUCTIONS)
    }
}

// ------------------------------------------------------------------ 判定与执行

fn disabled() -> ToolError {
    ToolError::new(
        "SERVICE_DISABLED",
        "MCP 服务未启用",
        "让用户在 daro 的「工具 ▸ 选项… ▸ MCP 服务」里打开服务",
    )
}

fn denied_sql(mode: policy::Mode, class: policy::SqlClass, sql: &str) -> ToolError {
    if mode == policy::Mode::ReadWrite && policy::is_broad_write(sql) {
        return ToolError::new(
            "WHERE_TOO_BROAD",
            "当前执行模式不允许全表 UPDATE/DELETE",
            "补一个有效的 WHERE 过滤条件后重试；确实要整表修改需在 daro 里切到「完全访问」",
        );
    }
    ToolError::new(
        "MODE_DENIED",
        format!(
            "当前执行模式（{}）不允许该操作，该语句至少需要 {}",
            mode.label(),
            class.minimum_mode().label()
        ),
        "只读 / 数据读写 / 完全访问在 daro 的「工具 ▸ 选项… ▸ MCP 服务」里切换",
    )
}

fn sql_kind(kind: DbKind) -> bool {
    matches!(
        kind,
        DbKind::SQLite | DbKind::PostgreSql | DbKind::MySql | DbKind::SqlServer
    )
}

fn needs_password(kind: DbKind) -> bool {
    kind != DbKind::SQLite
}

fn require_database(raw: &str) -> Result<String, ToolError> {
    let db = raw.trim();
    if db.is_empty() {
        return Err(ToolError::invalid(
            "database 不能为空；先用 daro_list_databases 取库名",
        ));
    }
    Ok(db.to_string())
}

/// 连接可见性 → 真库 → SQL 引擎 → 密码，四道前置检查。
fn resolve(cfg: &mcp::Config, name: &str) -> Result<Connection, ToolError> {
    let all = store::load_connections().map_err(ToolError::internal)?;
    store::sync_handles(&all);
    if !cfg.visible_connection(name.trim()) {
        return Err(ToolError::new(
            "CONNECTION_NOT_VISIBLE",
            format!("连接 {name:?} 不存在或未授权给 MCP"),
            "先用 daro_list_connections 查看可用连接",
        ));
    }
    let conn = all
        .iter()
        .find(|c| c.name == name.trim())
        .or_else(|| all.iter().find(|c| c.name.eq_ignore_ascii_case(name.trim())))
        .cloned()
        .ok_or_else(|| {
            ToolError::new(
                "CONNECTION_NOT_VISIBLE",
                format!("连接 {name:?} 不存在或未授权给 MCP"),
                "先用 daro_list_connections 查看可用连接",
            )
        })?;
    if conn.demo {
        return Err(ToolError::new(
            "DRIVER_UNSUPPORTED",
            format!("连接 {:?} 是演示条目，只在 daro 界面里展示假数据", conn.name),
            "改用一条连真库的连接",
        ));
    }
    if !sql_kind(conn.kind) {
        return Err(ToolError::unreachable_kind(conn.kind));
    }
    if needs_password(conn.kind) && conn.password.is_empty() {
        return Err(ToolError::new(
            "PASSWORD_REQUIRED",
            format!("连接 {:?} 没有保存密码", conn.name),
            "让用户在 daro 里编辑该连接并保存密码后重试；agent 不能代传密码",
        ));
    }
    Ok(conn)
}

/// 在阻塞线程池执行驱动调用，用当前档位的超时包住。
async fn blocking<T, F>(mode: policy::Mode, f: F) -> Result<T, ToolError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let secs = mode.timeout_secs();
    match tokio::time::timeout(
        Duration::from_secs(secs),
        tokio::task::spawn_blocking(f),
    )
    .await
    {
        Ok(Ok(Ok(v))) => Ok(v),
        Ok(Ok(Err(e))) => Err(ToolError::database(e)),
        Ok(Err(join)) => Err(ToolError::internal(format!("后台任务失败: {join}"))),
        Err(_) => Err(ToolError::new(
            "QUERY_TIMEOUT",
            format!("查询超过 {secs} 秒未返回"),
            "本地文件库（SQLite）无法中途取消，服务端可能仍在执行；稍后用更小的查询重试",
        )),
    }
}

/// 单语句执行：独立连接 + 可取消。超时后置取消标志并另起线程发中断请求，
/// 不再只是「放弃等待」。
async fn run_cancellable(
    target: backend::Target,
    stmt: String,
    limit: usize,
    secs: u64,
) -> Result<crate::db_types::ExecOutcome, ToolError> {
    let job = target.create_job();
    let worker = job.clone();
    let handle = tokio::task::spawn_blocking(move || target.run_statement(&worker, &stmt, limit));
    match tokio::time::timeout(Duration::from_secs(secs), handle).await {
        Ok(Ok(Ok(v))) => Ok(v),
        Ok(Ok(Err(e))) => Err(ToolError::database(e)),
        Ok(Err(join)) => Err(ToolError::internal(format!("后台任务失败: {join}"))),
        Err(_) => {
            // 中断要连服务器，可能阻塞；别占着异步工作线程
            job.request_stop();
            let breaker = job.clone();
            let _ = tokio::task::spawn_blocking(move || breaker.interrupt());
            Err(ToolError::new(
                "QUERY_TIMEOUT",
                format!("查询超过 {secs} 秒未返回，已请求取消"),
                "语句跑在独立连接上，不会再堵住后续工具调用；SQL Server 无语句级中断，会等该语句结束后收回连接。稍后用更小的查询重试",
            ))
        }
    }
}

fn columns_json(cols: &[crate::db_types::ColMeta]) -> Value {
    let items: Vec<Value> = cols
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "type": c.ty,
                "numeric": c.numeric,
            })
        })
        .collect();
    Value::Array(items)
}

/// WHERE 片段：允许子句内容，但不允许拼出第二条语句或注释掉后续子句。
fn clean_fragment(raw: &str, keyword: &str) -> Result<Option<String>, ToolError> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let t = strip_prefix_ci(t, keyword);
    if t.contains(';') || t.contains("--") || t.contains("/*") {
        return Err(ToolError::invalid(format!(
            "{} 片段里不能带分号或注释",
            keyword
        )));
    }
    Ok(Some(t.to_string()))
}

fn strip_prefix_ci<'a>(s: &'a str, keyword: &str) -> &'a str {
    let Some(prefix) = s.get(..keyword.len()) else {
        return s;
    };
    if prefix.eq_ignore_ascii_case(keyword) {
        let rest = &s[keyword.len()..];
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return rest.trim_start();
        }
    }
    s
}

/// `ORDER BY` 片段 → 驱动要的 (列名, 升序) 列表。
fn parse_orders(raw: &str) -> Result<Vec<(String, bool)>, ToolError> {
    let raw = strip_prefix_ci(raw.trim(), "ORDER BY");
    if raw.contains(';') || raw.contains("--") || raw.contains("/*") {
        return Err(ToolError::invalid("orderBy 片段里不能带分号或注释"));
    }
    let mut out = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut tokens: Vec<&str> = part.split_whitespace().collect();
        let mut asc = true;
        if let Some(last) = tokens.last() {
            if last.eq_ignore_ascii_case("desc") {
                asc = false;
                tokens.pop();
            } else if last.eq_ignore_ascii_case("asc") {
                tokens.pop();
            }
        }
        if tokens.is_empty() {
            return Err(ToolError::invalid(format!("orderBy 片段 {part:?} 缺列名")));
        }
        out.push((tokens.join(" "), asc));
    }
    Ok(out)
}
