//! 表 / 模式 / 库的 SQL 转储：生成可直接重放的 `.sql` 文件。
//!
//! 结构按方言取最可靠的一份：MySQL `SHOW CREATE TABLE`、SQLite `sqlite_master`、
//! PostgreSQL 走跨库粘贴那套目录反查（SERIAL 落成标识列）、SQL Server 用设计器
//! 同源拼装器。数据统一按 `INSERT` 批次写；外键从建表语句里抽出来放到文件末尾，
//! 免得表顺序依赖创建顺序。取数 / 写盘都在后台线程，进度与取消走共享标志。

use std::collections::HashSet;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::backend::Target;
use crate::mock::{self, DbKind};
use crate::{catalog, object_copy, pg_driver, structure_compare, table_copy};

/// 每条 INSERT 带多少行（与表数据导出同口径）。
const INSERT_BATCH: usize = 200;

/// 转储范围：单表 / 一个模式 / 整个库。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DumpScope {
    Table,
    Schema,
    Database,
}

#[derive(Clone)]
pub struct DumpRequest {
    pub conn: mock::Connection,
    pub db: String,
    /// PG / SQL Server 的模式；其它引擎留空。
    pub schema: String,
    pub scope: DumpScope,
    /// `scope == Table` 时的表名。
    pub table: String,
    /// false = 仅结构（不写 INSERT）。
    pub include_data: bool,
    pub path: String,
}

#[derive(Clone, Debug, Default)]
pub struct DumpProgress {
    /// 已处理对象数（表 + 视图 + 实体化视图）。
    pub done: usize,
    pub total: usize,
    pub current: String,
    pub rows: u64,
}

#[derive(Clone, Debug, Default)]
pub struct DumpOutcome {
    pub tables: usize,
    pub views: usize,
    pub rows: u64,
    pub bytes: u64,
    /// 逐条失败的表 / 视图（写进文件注释，也回给界面）。
    pub failed: Vec<String>,
}

#[derive(Debug)]
pub enum DumpError {
    Cancelled,
    Io(String),
    Failed(String),
}

impl From<String> for DumpError {
    fn from(e: String) -> Self {
        Self::Failed(e)
    }
}

impl std::fmt::Display for DumpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "已取消"),
            Self::Io(e) => write!(f, "写入失败：{e}"),
            Self::Failed(e) => write!(f, "{e}"),
        }
    }
}

/// 能出 SQL 转储的引擎（演示 / NoSQL / Access 只读都不在里面）。
pub fn supports(kind: DbKind) -> bool {
    matches!(
        kind,
        DbKind::SQLite | DbKind::PostgreSql | DbKind::MySql | DbKind::SqlServer
    )
}

struct Plan {
    schema: String,
    tables: Vec<String>,
    views: Vec<String>,
    matviews: Vec<String>,
}

fn schemas_of(req: &DumpRequest) -> Result<Vec<String>, String> {
    match req.scope {
        DumpScope::Database if catalog::has_schemas(req.conn.kind) => {
            let mut list = catalog::query_schemas(&req.conn, &req.db)?;
            if list.is_empty() {
                list.push(match req.conn.kind {
                    DbKind::SqlServer => "dbo".to_string(),
                    _ => "public".to_string(),
                });
            }
            Ok(list)
        }
        _ => Ok(vec![req.schema.clone()]),
    }
}

fn build_plans(req: &DumpRequest) -> Result<Vec<Plan>, String> {
    if req.scope == DumpScope::Table {
        if req.table.trim().is_empty() {
            return Err("没有点名要转储的表".into());
        }
        return Ok(vec![Plan {
            schema: req.schema.clone(),
            tables: vec![req.table.clone()],
            views: Vec::new(),
            matviews: Vec::new(),
        }]);
    }
    let mut plans = Vec::new();
    for schema in schemas_of(req)? {
        let tables = catalog::query_objects(&req.conn, &req.db, &schema, 0)?;
        let views = catalog::query_objects(&req.conn, &req.db, &schema, 1)?;
        let matviews = if req.conn.kind == DbKind::PostgreSql {
            catalog::query_objects(&req.conn, &req.db, &schema, 2)?
        } else {
            Vec::new()
        };
        plans.push(Plan {
            schema,
            tables,
            views,
            matviews,
        });
    }
    Ok(plans)
}

/// 一次转储。只在后台线程调用：目录查询、整表取数、写盘都可能跑很久。
pub fn run(
    req: &DumpRequest,
    progress: &mut dyn FnMut(DumpProgress),
    cancel: &AtomicBool,
) -> Result<DumpOutcome, DumpError> {
    if !supports(req.conn.kind) {
        return Err(DumpError::Failed(format!(
            "{} 连接不支持 SQL 转储",
            req.conn.kind.label()
        )));
    }
    if Target::of(&req.conn, &req.db, &req.schema).is_none() {
        return Err(DumpError::Failed("演示连接没有真库，无法转储".into()));
    }
    let plans = build_plans(req).map_err(DumpError::Failed)?;
    let path = req.path.trim();
    if path.is_empty() {
        return Err(DumpError::Failed("没有选择转储路径".into()));
    }
    let table_count: usize = plans.iter().map(|p| p.tables.len()).sum();
    let view_count: usize = plans.iter().map(|p| p.views.len() + p.matviews.len()).sum();
    // 有数据时每张表算两步（结构、数据），进度按步走。
    let total = if req.include_data {
        table_count * 2 + view_count
    } else {
        table_count + view_count
    };
    let mut sink = Sink::create(Path::new(path))?;
    let mut outcome = DumpOutcome::default();
    let mut done = 0usize;
    let mut deferred: Vec<String> = Vec::new();

    struct TableJob {
        plan: usize,
        table: String,
        pg_def: Option<table_copy::PgTable>,
    }

    let result = (|| -> Result<(), DumpError> {
        write_header(&mut sink, req, req.conn.kind)?;
        // 先结构、后数据：SQLite 的触发器在插入时立即执行，子表数据落地前
        // 父表必须已经建好（MySQL / PG 的视图、外键也一样）。
        let mut jobs: Vec<TableJob> = Vec::new();
        for (pi, plan) in plans.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(DumpError::Cancelled);
            }
            let tg = Target::of(&req.conn, &req.db, &plan.schema)
                .ok_or_else(|| DumpError::Failed("连不上目标库".into()))?;
            schema_header(&mut sink, &tg, &plan.schema)?;
            let mut pg = pg_session(&tg)?;
            for table in &plan.tables {
                if cancel.load(Ordering::Relaxed) {
                    return Err(DumpError::Cancelled);
                }
                progress(DumpProgress {
                    done,
                    total,
                    current: table.clone(),
                    rows: outcome.rows,
                });
                sink.line(&format!(
                    "--\n-- 表 {}\n--",
                    display_name(&plan.schema, table)
                ))?;
                let mut pg_def = None;
                match table_ddl(
                    &tg,
                    &mut pg,
                    &plan.schema,
                    table,
                    &mut deferred,
                    &mut pg_def,
                ) {
                    Ok(sql) => {
                        sink.str(&sql)?;
                        sink.line("")?;
                        outcome.tables += 1;
                        jobs.push(TableJob {
                            plan: pi,
                            table: table.clone(),
                            pg_def,
                        });
                    }
                    Err(e) => {
                        let text = format!("{}: {e}", display_name(&plan.schema, table));
                        sink.line(&format!("-- 跳过表：{text}"))?;
                        sink.line("")?;
                        outcome.failed.push(text);
                    }
                }
                done += 1;
            }
        }
        // 视图放在所有表之后：建视图时被引用的表必须存在。
        for plan in &plans {
            if cancel.load(Ordering::Relaxed) {
                return Err(DumpError::Cancelled);
            }
            let tg = Target::of(&req.conn, &req.db, &plan.schema)
                .ok_or_else(|| DumpError::Failed("连不上目标库".into()))?;
            schema_header(&mut sink, &tg, &plan.schema)?;
            for (cat, list) in [(1u8, &plan.views), (2u8, &plan.matviews)] {
                for view in list {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(DumpError::Cancelled);
                    }
                    progress(DumpProgress {
                        done,
                        total,
                        current: view.clone(),
                        rows: outcome.rows,
                    });
                    match view_ddl(&tg, view, cat as usize) {
                        Ok(Some(sql)) => {
                            sink.line(&format!(
                                "--\n-- {} {}\n--",
                                if cat == 2 { "实体化视图" } else { "视图" },
                                display_name(&plan.schema, view)
                            ))?;
                            sink.str(&sql)?;
                            if cat == 2 && matches!(tg, Target::Pg { .. }) {
                                sink.line(&format!(
                                    "REFRESH MATERIALIZED VIEW {};",
                                    tg.qualified_table(view)
                                ))?;
                            }
                            sink.line("")?;
                            outcome.views += 1;
                        }
                        Ok(None) => {
                            outcome.failed.push(format!(
                                "{}: 读不到定义",
                                display_name(&plan.schema, view)
                            ));
                        }
                        Err(e) => {
                            outcome
                                .failed
                                .push(format!("{}: {e}", display_name(&plan.schema, view)));
                        }
                    }
                    done += 1;
                }
            }
        }
        if req.include_data {
            for job in &jobs {
                if cancel.load(Ordering::Relaxed) {
                    return Err(DumpError::Cancelled);
                }
                let plan = &plans[job.plan];
                let tg = Target::of(&req.conn, &req.db, &plan.schema)
                    .ok_or_else(|| DumpError::Failed("连不上目标库".into()))?;
                progress(DumpProgress {
                    done,
                    total,
                    current: format!("{}（数据）", job.table),
                    rows: outcome.rows,
                });
                match dump_rows(
                    &mut sink,
                    req,
                    &tg,
                    &plan.schema,
                    &job.table,
                    job.pg_def.as_ref(),
                    cancel,
                    progress,
                    &mut outcome,
                    done,
                    total,
                ) {
                    Ok(()) => {}
                    Err(DumpError::Cancelled) => return Err(DumpError::Cancelled),
                    Err(e) => {
                        let text = format!("{}: {e}", display_name(&plan.schema, &job.table));
                        sink.line(&format!("-- 数据转储失败：{text}"))?;
                        sink.line("")?;
                        outcome.failed.push(text);
                    }
                }
                done += 1;
            }
        }
        // 外键统一挪到末尾：建表不受引用顺序约束。
        if !deferred.is_empty() {
            sink.line("--\n-- 外键约束\n--")?;
            for stmt in &deferred {
                sink.line(stmt)?;
            }
            sink.line("")?;
        }
        write_footer(&mut sink, req.conn.kind)?;
        Ok(())
    })();

    if let Err(e) = result {
        drop(sink);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    outcome.bytes = sink.bytes;
    sink.flush().map_err(|e| DumpError::Io(e.to_string()))?;
    progress(DumpProgress {
        done: total,
        total,
        current: String::new(),
        rows: outcome.rows,
    });
    Ok(outcome)
}

/// 目录 SQL 里的字符串字面量（`crate::sql_literal` 只转义、不包引号）。
fn sql_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn display_name(schema: &str, name: &str) -> String {
    if schema.is_empty() {
        name.to_string()
    } else {
        format!("{schema}.{name}")
    }
}

/// 模式分段头。PG 额外把 `search_path` 指到该模式：视图正文 / 默认值里常有不带
/// 模式限定的引用，落地时要按源库的模式解析。
fn schema_header(sink: &mut Sink, tg: &Target, schema: &str) -> Result<(), DumpError> {
    if schema.is_empty() {
        return Ok(());
    }
    sink.line(&format!("-- 模式 {schema}"))?;
    if matches!(tg, Target::Pg { .. }) {
        sink.line(&format!("SET search_path TO {};", tg.quote(schema)))?;
    }
    sink.line("")
}

fn pg_session(tg: &Target) -> Result<Option<pg_driver::CopySession>, DumpError> {
    match tg {
        Target::Pg { params, .. } => Ok(Some(
            pg_driver::CopySession::connect(params)
                .map_err(|e| DumpError::Failed(format!("连源库失败：{e}")))?,
        )),
        _ => Ok(None),
    }
}

struct Sink {
    out: BufWriter<std::fs::File>,
    bytes: u64,
}

impl Sink {
    fn create(path: &Path) -> Result<Self, DumpError> {
        let file = std::fs::File::create(path).map_err(|e| DumpError::Io(e.to_string()))?;
        Ok(Self {
            out: BufWriter::new(file),
            bytes: 0,
        })
    }

    fn str(&mut self, s: &str) -> Result<(), DumpError> {
        self.out
            .write_all(s.as_bytes())
            .map_err(|e| DumpError::Io(e.to_string()))?;
        self.bytes += s.len() as u64;
        Ok(())
    }

    fn line(&mut self, s: &str) -> Result<(), DumpError> {
        self.str(s)?;
        self.str("\n")
    }

    fn flush(&mut self) -> Result<(), DumpError> {
        self.out
            .flush()
            .map_err(|e| DumpError::Io(e.to_string()))
    }
}

fn write_header(
    sink: &mut Sink,
    req: &DumpRequest,
    kind: DbKind,
) -> Result<(), DumpError> {
    sink.line("-- daro-rs SQL 转储")?;
    sink.line(&format!("-- 连接：{}", req.conn.name))?;
    sink.line(&format!("-- 数据库：{}", req.db))?;
    match req.scope {
        DumpScope::Table => sink.line(&format!("-- 表：{}", display_name(&req.schema, &req.table)))?,
        DumpScope::Schema => sink.line(&format!("-- 模式：{}", req.schema))?,
        DumpScope::Database => sink.line("-- 范围：整个数据库")?,
    }
    sink.line(&format!(
        "-- 内容：{}",
        if req.include_data {
            "结构和数据"
        } else {
            "仅结构"
        }
    ))?;
    sink.line("")?;
    match kind {
        DbKind::MySql => {
            sink.line("SET NAMES utf8mb4;")?;
            sink.line("SET FOREIGN_KEY_CHECKS = 0;")?;
        }
        DbKind::PostgreSql => {
            sink.line("SET client_encoding = 'UTF8';")?;
            sink.line("SET standard_conforming_strings = on;")?;
            sink.line("SET check_function_bodies = false;")?;
            sink.line("SET client_min_messages = warning;")?;
        }
        DbKind::SQLite => {
            sink.line("PRAGMA foreign_keys = OFF;")?;
            sink.line("BEGIN TRANSACTION;")?;
        }
        DbKind::SqlServer => {
            sink.line("SET NOCOUNT ON;")?;
        }
        _ => {}
    }
    sink.line("")?;
    Ok(())
}

fn write_footer(sink: &mut Sink, kind: DbKind) -> Result<(), DumpError> {
    match kind {
        DbKind::MySql => sink.line("SET FOREIGN_KEY_CHECKS = 1;")?,
        DbKind::SQLite => sink.line("COMMIT;")?,
        _ => {}
    }
    sink.line("-- 完成")?;
    Ok(())
}

/// 单表结构。MySQL / SQLite 用库自己保存的原文，PG 走目录反查，SQL Server
/// 用设计器同源拼装器。外键语句从结果里拆出来交给 `deferred`。
fn table_ddl(
    tg: &Target,
    pg: &mut Option<pg_driver::CopySession>,
    schema: &str,
    table: &str,
    deferred: &mut Vec<String>,
    pg_def: &mut Option<table_copy::PgTable>,
) -> Result<String, String> {
    match tg {
        Target::MySql { .. } => {
            let rows = crate::meta_rows(
                tg,
                &format!("SHOW CREATE TABLE {}", tg.qualified_table(table)),
            )?;
            let ddl = rows
                .first()
                .and_then(|r| r.get(1))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "SHOW CREATE TABLE 没有返回建表语句".to_string())?;
            let ddl = table_copy::replace_name_after(&ddl, "TABLE", &tg.qualified_table(table))?;
            let (kept, fks) = structure_compare::defer_fk_constraints(DbKind::MySql, &ddl);
            deferred.extend(fks);
            Ok(ensure_semicolon(&kept))
        }
        Target::Sqlite { db, .. } => {
            let sql = format!(
                "SELECT COALESCE(sql, '') FROM {}.sqlite_master \
                 WHERE type = 'table' AND name = {}",
                crate::sqlite_ident(db),
                sql_str(table)
            );
            let ddl = crate::meta_rows(tg, &sql)?
                .first()
                .and_then(|r| r.first())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "sqlite_master 里没有这张表的建表语句".to_string())?;
            let ddl = table_copy::replace_name_after(&ddl, "TABLE", &tg.qualified_table(table))?;
            // 索引 / 触发器在 sqlite_master 里是独立行，CREATE TABLE 原文带不出来。
            let mut out = ensure_semicolon(&ddl);
            let extra = format!(
                "SELECT COALESCE(sql, '') FROM {}.sqlite_master \
                 WHERE type IN ('index', 'trigger') AND tbl_name = {} \
                   AND sql IS NOT NULL ORDER BY type, name",
                crate::sqlite_ident(db),
                sql_str(table)
            );
            for row in crate::meta_rows_or_empty(tg, &extra) {
                if let Some(sql) = row.first().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    out.push_str(&ensure_semicolon(sql));
                }
            }
            Ok(out)
        }
        Target::Pg { params, .. } => {
            if pg.is_none() {
                *pg = Some(
                    pg_driver::CopySession::connect(params)
                        .map_err(|e| format!("连源库失败：{e}"))?,
                );
            }
            let session = pg.as_mut().expect("上面刚建好 PG 会话");
            let def = table_copy::read_pg_table(session, schema, table)?;
            let sql = pg_table_script(schema, table, &def, deferred);
            *pg_def = Some(def);
            Ok(sql)
        }
        Target::SqlServer { .. } => {
            let ddl = crate::read_table_ddl(tg, schema, table)?;
            let (kept, fks) = structure_compare::defer_fk_constraints(DbKind::SqlServer, &ddl);
            deferred.extend(fks);
            Ok(ensure_semicolon(&kept))
        }
        _ => Err("这个引擎没有 SQL 转储".into()),
    }
}

fn pg_table_script(
    schema: &str,
    table: &str,
    def: &table_copy::PgTable,
    deferred: &mut Vec<String>,
) -> String {
    let qual = format!(
        "{}.{}",
        pg_driver::quote_ident(schema),
        pg_driver::quote_ident(table)
    );
    let mut out = table_copy::pg_create_table(schema, table, def);
    out.push(';');
    for (name, sql) in &def.constraints {
        out.push_str(&format!(
            "\nALTER TABLE {qual} ADD CONSTRAINT {} {sql};",
            pg_driver::quote_ident(name)
        ));
    }
    for (_, sql) in &def.indexes {
        out.push('\n');
        out.push_str(sql.trim_end_matches(';'));
        out.push(';');
    }
    if let Some(comment) = &def.comment {
        out.push_str(&format!(
            "\nCOMMENT ON TABLE {qual} IS {};",
            sql_str(comment)
        ));
    }
    for (col, comment) in &def.col_comments {
        out.push_str(&format!(
            "\nCOMMENT ON COLUMN {qual}.{} IS {};",
            pg_driver::quote_ident(col),
            sql_str(comment)
        ));
    }
    for fk in &def.fks {
        deferred.push(format!(
            "ALTER TABLE {qual} ADD CONSTRAINT {} {};",
            pg_driver::quote_ident(&fk.name),
            fk.def
        ));
    }
    out
}

fn view_ddl(tg: &Target, name: &str, cat: usize) -> Result<Option<String>, String> {
    let Some(ddl) = object_copy::fetch_ddl(tg, cat, name) else {
        return Ok(None);
    };
    let ddl = match tg {
        // MySQL / SQLite 的视图原文不带模式限定，补成可迁移的限定名。
        Target::MySql { .. } => {
            let ddl = strip_mysql_definer(&ddl);
            table_copy::replace_name_after(&ddl, "VIEW", &tg.qualified_table(name))?
        }
        Target::Sqlite { .. } => {
            table_copy::replace_name_after(&ddl, "VIEW", &tg.qualified_table(name))?
        }
        _ => ddl,
    };
    Ok(Some(ensure_semicolon(&ddl)))
}

/// `SHOW CREATE VIEW` 会带上源库的 `DEFINER=\`user\`@\`host\``：换台机器重放时
/// 那个账号不存在会直接报错，转储里去掉（定义值本身不含空白）。
fn strip_mysql_definer(sql: &str) -> String {
    const KEY: &str = "DEFINER=";
    let Some(at) = sql.find(KEY) else {
        return sql.to_string();
    };
    let rest = &sql[at + KEY.len()..];
    let end = rest
        .find(char::is_whitespace)
        .map(|i| at + KEY.len() + i)
        .unwrap_or(sql.len());
    format!("{}{}", &sql[..at], sql[end..].trim_start())
}

fn ensure_semicolon(sql: &str) -> String {
    let trimmed = sql.trim_end();
    if trimmed.ends_with(';') {
        format!("{trimmed}\n")
    } else {
        format!("{trimmed};\n")
    }
}

#[allow(clippy::too_many_arguments)]
fn dump_rows(
    sink: &mut Sink,
    req: &DumpRequest,
    tg: &Target,
    schema: &str,
    table: &str,
    pg_def: Option<&table_copy::PgTable>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(DumpProgress),
    outcome: &mut DumpOutcome,
    done: usize,
    total: usize,
) -> Result<(), DumpError> {
    let skip = skipped_columns(tg, table, pg_def)?;
    let qual = tg.qualified_table(table);
    let mut indexes: Vec<usize> = Vec::new();
    let mut numeric: Vec<bool> = Vec::new();
    let mut col_list = String::new();
    let mut override_clause = "";
    let mut identity_on = false;
    let mut offset = 0usize;
    let mut wrote = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DumpError::Cancelled);
        }
        // 按页现取现写：转储不再把整表压进内存
        let page = crate::fetch_table_page(
            &req.conn,
            &req.db,
            schema,
            table,
            None,
            &[],
            offset,
            crate::REMOTE_PAGE_SIZE,
        )
        .map_err(DumpError::Failed)?;
        if page.rows.is_empty() {
            break;
        }
        if !wrote {
            // 第一页到手才知道列清单：跳过生成列后按原顺序拼 INSERT
            for (ix, col) in page.columns.iter().enumerate() {
                if skip.contains(&col.name) {
                    continue;
                }
                indexes.push(ix);
                numeric.push(col.numeric);
            }
            if indexes.is_empty() {
                return Ok(());
            }
            col_list = indexes
                .iter()
                .map(|&ix| tg.quote(&page.columns[ix].name))
                .collect::<Vec<_>>()
                .join(", ");
            if pg_identity_override(pg_def) {
                override_clause = " OVERRIDING SYSTEM VALUE";
            }
            let identity_insert = matches!(tg, Target::SqlServer { .. })
                && has_sqlserver_identity(tg, schema, table)?;
            if identity_insert {
                sink.line(&format!("SET IDENTITY_INSERT {qual} ON;"))?;
                identity_on = true;
            }
        }

        for batch in page.rows.chunks(INSERT_BATCH) {
            if cancel.load(Ordering::Relaxed) {
                return Err(DumpError::Cancelled);
            }
            let mut stmt =
                format!("INSERT INTO {qual} ({col_list}){override_clause} VALUES\n");
            for (row_ix, row) in batch.iter().enumerate() {
                let values = indexes
                    .iter()
                    .zip(&numeric)
                    .map(|(&ix, &num)| {
                        let cell = row.get(ix).map(|s| s.as_str()).unwrap_or("");
                        literal(cell, num, tg.kind())
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                stmt.push('(');
                stmt.push_str(&values);
                stmt.push(')');
                if row_ix + 1 == batch.len() {
                    stmt.push_str(";\n");
                } else {
                    stmt.push_str(",\n");
                }
            }
            sink.str(&stmt)?;
            outcome.rows += batch.len() as u64;
        }
        offset += page.rows.len();
        wrote = true;
        progress(DumpProgress {
            done,
            total,
            current: table.to_string(),
            rows: outcome.rows,
        });
    }
    if !wrote {
        return Ok(());
    }
    sink.line("")?;

    if identity_on {
        sink.line(&format!("SET IDENTITY_INSERT {qual} OFF;"))?;
    }
    if matches!(tg, Target::Pg { .. }) {
        reset_pg_sequences(sink, tg, table, pg_def)?;
    }
    Ok(())
}

/// 生成列（服务端计算）不能出现在 INSERT 里；自增 / 标识列要保留原值。
fn skipped_columns(
    tg: &Target,
    table: &str,
    pg_def: Option<&table_copy::PgTable>,
) -> Result<HashSet<String>, String> {
    if let Some(def) = pg_def {
        return Ok(def
            .columns
            .iter()
            .filter(|c| c.generated == 's' || c.generated == 'v')
            .map(|c| c.name.clone())
            .collect());
    }
    match tg {
        Target::MySql { db, .. } => Ok(crate::meta_rows_or_empty(
            tg,
            &format!(
                "SELECT COLUMN_NAME FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} \
                   AND EXTRA LIKE '%GENERATED%'",
                sql_str(db),
                sql_str(table)
            ),
        )
        .into_iter()
        .filter_map(|r| r.into_iter().next())
        .collect()),
        Target::Sqlite { db, .. } => Ok(crate::meta_rows_or_empty(
            tg,
            &format!(
                "SELECT name FROM pragma_table_xinfo({}, {}) WHERE hidden <> 0",
                sql_str(table),
                sql_str(db)
            ),
        )
        .into_iter()
        .filter_map(|r| r.into_iter().next())
        .collect()),
        Target::SqlServer { .. } => Ok(crate::meta_rows_or_empty(
            tg,
            &format!(
                "SELECT name FROM sys.columns \
                 WHERE object_id = OBJECT_ID({}) AND is_computed = 1",
                sql_str(&tg.qualified_table(table))
            ),
        )
        .into_iter()
        .filter_map(|r| r.into_iter().next())
        .collect()),
        _ => Ok(HashSet::new()),
    }
}

fn pg_identity_override(def: Option<&table_copy::PgTable>) -> bool {
    def.is_some_and(|d| {
        d.columns
            .iter()
            .any(|c| c.identity != ' ' || c.serial)
    })
}

fn has_sqlserver_identity(
    tg: &Target,
    schema: &str,
    table: &str,
) -> Result<bool, String> {
    let Target::SqlServer { db, .. } = tg else {
        return Ok(false);
    };
    let qual = format!(
        "{}.{}.{}",
        tg.quote(db),
        tg.quote(schema),
        tg.quote(table)
    );
    Ok(crate::meta_rows_or_empty(
        tg,
        &format!(
            "SELECT name FROM sys.columns \
             WHERE object_id = OBJECT_ID({}) AND is_identity = 1",
            sql_str(&qual)
        ),
    )
    .into_iter()
    .any(|r| r.first().is_some_and(|s| !s.is_empty())))
}

/// 显式写入了标识列的值，序列要推到「最大值 + 1」，否则下一条 INSERT 撞号。
fn reset_pg_sequences(
    sink: &mut Sink,
    tg: &Target,
    table: &str,
    def: Option<&table_copy::PgTable>,
) -> Result<(), DumpError> {
    let Some(def) = def else {
        return Ok(());
    };
    let qual = match tg {
        Target::Pg { schema, .. } => format!(
            "{}.{}",
            pg_driver::quote_ident(schema),
            pg_driver::quote_ident(table)
        ),
        _ => return Ok(()),
    };
    for col in &def.columns {
        if col.identity == ' ' && !col.serial {
            continue;
        }
        sink.line(&format!(
            "SELECT setval(pg_get_serial_sequence({}, {}), \
             COALESCE((SELECT MAX({}) FROM {qual}), 0) + 1, false);",
            sql_str(&qual),
            sql_str(&col.name),
            pg_driver::quote_ident(&col.name)
        ))?;
    }
    Ok(())
}

/// 单元格 → SQL 字面量。数值列不加引号（保留 varchar 里的 `007`），
/// MySQL 还要转义反斜杠，否则以 `\` 结尾的字符串会把收尾单引号吃掉。
fn literal(cell: &str, numeric: bool, kind: DbKind) -> String {
    if crate::export_format::is_null(cell) {
        return "NULL".into();
    }
    let t = cell.trim();
    if !t.is_empty()
        && (t.eq_ignore_ascii_case("true")
            || t.eq_ignore_ascii_case("false")
            || (numeric && (t.parse::<i64>().is_ok() || t.parse::<f64>().is_ok())))
    {
        return t.to_string();
    }
    let mut value = if kind == DbKind::MySql {
        cell.replace('\\', "\\\\")
    } else {
        cell.to_string()
    };
    value = value.replace('\'', "''");
    format!("'{value}'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_sql_engines_support_dump() {
        for kind in [
            DbKind::SQLite,
            DbKind::PostgreSql,
            DbKind::MySql,
            DbKind::SqlServer,
        ] {
            assert!(supports(kind), "{} 应能转储", kind.label());
        }
        for kind in [DbKind::Access, DbKind::MongoDb, DbKind::Redis] {
            assert!(!supports(kind), "{} 没有 SQL 转储", kind.label());
        }
        for conn in mock::connections() {
            if conn.demo || !supports(conn.kind) {
                assert!(!crate::sql_dump_ready(&conn), "演示 / 非 SQL 连接不能出假转储");
            }
        }
    }

    #[test]
    fn numeric_columns_keep_digits_verbatim() {
        assert_eq!(literal("007", true, DbKind::PostgreSql), "007");
        assert_eq!(literal("007", false, DbKind::PostgreSql), "'007'");
        assert_eq!(literal("NULL", true, DbKind::MySql), "NULL");
        assert_eq!(literal("true", false, DbKind::SQLite), "true");
        assert_eq!(literal("it's", false, DbKind::PostgreSql), "'it''s'");
    }

    #[test]
    fn mysql_view_ddl_drops_the_source_definer() {
        let sql = "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`%` SQL SECURITY DEFINER \
                   VIEW `v` AS select 1 AS `x`";
        assert_eq!(
            strip_mysql_definer(sql),
            "CREATE ALGORITHM=UNDEFINED SQL SECURITY DEFINER VIEW `v` AS select 1 AS `x`"
        );
        assert_eq!(strip_mysql_definer("CREATE VIEW v AS SELECT 1"), "CREATE VIEW v AS SELECT 1");
    }

    #[test]
    fn mysql_literals_escape_the_backslash_first() {
        assert_eq!(literal(r"a\b", false, DbKind::MySql), r"'a\\b'");
        // 以反斜杠结尾的值不能把收尾引号转义掉
        assert_eq!(literal("tail\\", false, DbKind::MySql), "'tail\\\\'");
    }

    fn fixture(tag: &str) -> mock::Connection {
        let (conn, _path) = crate::ui_test_support::sqlite_fixture(
            tag,
            &[
                "CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT, code TEXT)",
                "CREATE TABLE child (id INTEGER PRIMARY KEY, \
                 pid INTEGER REFERENCES parent(id), note TEXT DEFAULT 'x')",
                "CREATE TABLE empty_t (id INTEGER)",
                "CREATE INDEX child_pid ON child(pid)",
                "CREATE TRIGGER child_touch AFTER INSERT ON child BEGIN \
                 UPDATE parent SET code = code WHERE id = NEW.pid; END",
                "CREATE VIEW parent_view AS SELECT id, name FROM parent",
                "INSERT INTO parent (id, name, code) VALUES \
                 (1, 'a', '007'), (2, 'it''s', NULL), (3, '末\\尾', '9')",
                "INSERT INTO child (id, pid, note) VALUES (1, 1, 'n')",
            ],
        );
        conn
    }

    fn request(conn: mock::Connection, tag: &str) -> DumpRequest {
        DumpRequest {
            conn,
            db: "main".into(),
            schema: String::new(),
            scope: DumpScope::Database,
            table: String::new(),
            include_data: true,
            path: std::env::temp_dir()
                .join(format!("daro_rs_{tag}_{}.sql", std::process::id()))
                .display()
                .to_string(),
        }
    }

    /// 整库转储 → 语句原样跑进新库：表 / 索引 / 触发器 / 视图 / 数据都要回来。
    #[test]
    fn sqlite_database_dump_replays_into_a_fresh_file() {
        let req = request(fixture("dump_db"), "dump_db_out");
        let path = std::path::PathBuf::from(&req.path);
        let _ = std::fs::remove_file(&path);
        let cancel = AtomicBool::new(false);

        let outcome = run(&req, &mut |_| {}, &cancel).expect("整库转储要成功");
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        assert_eq!(outcome.tables, 3, "三张表都进档");
        assert_eq!(outcome.views, 1);
        assert_eq!(outcome.rows, 4, "两张有数据的表共 4 行");
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("CREATE TABLE \"main\".\"parent\""), "{text}");
        assert!(text.contains("CREATE INDEX child_pid ON child(pid)"), "{text}");
        assert!(text.contains("CREATE TRIGGER child_touch"), "{text}");
        assert!(text.contains("CREATE VIEW \"main\".\"parent_view\""), "{text}");
        assert!(text.contains("INSERT INTO \"main\".\"parent\""), "{text}");
        assert!(!text.contains("INSERT INTO \"main\".\"empty_t\""), "空表不出 INSERT");
        assert!(!text.contains("-- 跳过表"), "{text}");

        let dst = std::env::temp_dir()
            .join(format!("daro_rs_dump_db_replay_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dst);
        let key = format!("SQLite dump replay {}", std::process::id());
        crate::sqlite_driver::open(&key, &dst.to_string_lossy()).unwrap();
        for stmt in crate::split_sql_statements(&text) {
            // 纯注释段（文件收尾的 `-- 完成`）没有可执行的 SQL。
            if !stmt.lines().any(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with("--")
            }) {
                continue;
            }
            crate::sqlite_driver::execute_stmt(&key, &stmt, 1)
                .unwrap_or_else(|e| panic!("转储语句跑不通：{e}\n{stmt}"));
        }
        let back = crate::sqlite_driver::select_sql(
            &key,
            "SELECT id, name, code FROM parent ORDER BY id",
            100,
        )
        .unwrap();
        assert_eq!(
            back.rows,
            vec![
                vec!["1".to_string(), "a".into(), "007".into()],
                vec!["2".to_string(), "it's".into(), "NULL".into()],
                vec!["3".to_string(), "末\\尾".into(), "9".into()],
            ],
            "文本原样回来：前导零不丢、单引号 / 反斜杠不串"
        );
        let idx = crate::sqlite_driver::select_sql(
            &key,
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name = 'child_pid'",
            10,
        )
        .unwrap();
        assert_eq!(idx.rows.len(), 1, "索引随档重建");
        crate::sqlite_driver::close(&key);
        let _ = std::fs::remove_file(&dst);
        let _ = std::fs::remove_file(&path);
    }

    /// 真 PG：整库转储 → 语句跑进一个临时库，标识列 / 生成列 / 序列 / 视图
    /// / 实体化视图都要能原样重放。
    #[test]
    #[ignore = "打 datawalk 开发库，需要网络可达"]
    fn pg_database_dump_replays_into_a_scratch_database() {
        let Some((_guard, p)) = pg_driver::tests::pg_fixture() else {
            eprintln!("[pg] skip：读不到 datawalk 连接串");
            return;
        };
        let conn = mock::Connection::new(
            "dump-pg",
            DbKind::PostgreSql,
            p.host.clone(),
            p.port,
            p.dbname.clone(),
            p.user.clone(),
            p.password.clone(),
            "",
            false,
        );
        let mut req = request(conn.clone(), "dump_pg_out");
        req.db = p.dbname.clone();
        req.schema = "public".into();
        let path = std::path::PathBuf::from(&req.path);
        let _ = std::fs::remove_file(&path);

        let cancel = AtomicBool::new(false);
        let outcome = run(&req, &mut |_| {}, &cancel).expect("PG 整库转储要成功");
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        assert!(outcome.tables >= 6, "夹具表都该进档：{}", outcome.tables);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("SET search_path TO \"public\";"), "{text}");
        assert!(
            text.contains("CREATE TABLE \"public\".\"member\"")
                && text.contains("GENERATED ALWAYS AS IDENTITY"),
            "标识列要落成 IDENTITY：{text}"
        );
        assert!(text.contains("OVERRIDING SYSTEM VALUE"), "{text}");
        assert!(text.contains("setval(pg_get_serial_sequence("), "{text}");
        assert!(
            text.contains("GENERATED ALWAYS AS ((a * 2)) STORED"),
            "生成列连表达式一起带：{text}"
        );
        assert!(text.contains("INSERT INTO \"public\".\"gen\" (\"k\", \"a\")"), "{text}");
        assert!(text.contains("CREATE VIEW \"public\".\"v_member\""), "{text}");
        assert!(text.contains("CREATE MATERIALIZED VIEW \"public\".\"mv_member\""), "{text}");
        assert!(
            text.contains("REFRESH MATERIALIZED VIEW \"public\".\"mv_member\";"),
            "{text}"
        );

        // 重放进临时库：建库 → 逐条执行 → 比对数据与序列。
        let admin = "dump-pg-admin";
        let replay_db = "daro_dump_replay";
        pg_driver::open(admin, &p.for_db("postgres")).expect("连 postgres 库失败");
        pg_driver::execute_stmt(
            admin,
            &format!("DROP DATABASE IF EXISTS {replay_db} WITH (FORCE)"),
            1,
        )
        .unwrap();
        pg_driver::execute_stmt(admin, &format!("CREATE DATABASE {replay_db}"), 1).unwrap();
        pg_driver::close(admin);

        let replay = "dump-pg-replay";
        pg_driver::open(replay, &p.for_db(replay_db)).expect("连临时库失败");
        for stmt in crate::split_sql_statements(&text) {
            if !stmt.lines().any(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with("--")
            }) {
                continue;
            }
            pg_driver::execute_stmt(replay, &stmt, 1)
                .unwrap_or_else(|e| panic!("PG 转储语句跑不通：{e}\n{stmt}"));
        }
        let src_key = "dump-pg-src";
        pg_driver::open(src_key, &p.for_db(&p.dbname)).expect("连测试库失败");
        let src = pg_driver::select_table(src_key, "public", "member", None, &[], 0, 100).unwrap();
        let back = pg_driver::select_table(replay, "public", "member", None, &[], 0, 100).unwrap();
        assert_eq!(back.columns, src.columns);
        assert_eq!(back.rows, src.rows, "数据逐行原样回来");
        let generated =
            pg_driver::select_sql(replay, "SELECT b FROM gen ORDER BY k", 100).unwrap();
        assert_eq!(
            generated.rows,
            vec![vec!["42".to_string()], vec!["44".to_string()]],
            "生成列由目标库重新算"
        );
        let seq = pg_driver::select_sql(
            replay,
            "SELECT nextval(pg_get_serial_sequence('public.member', 'id'))",
            10,
        )
        .unwrap();
        assert_eq!(seq.rows, vec![vec!["4".to_string()]], "序列要从最大值 + 1 接着走");
        pg_driver::close(src_key);
        pg_driver::close(replay);
        pg_driver::open(admin, &p.for_db("postgres")).unwrap();
        pg_driver::execute_stmt(
            admin,
            &format!("DROP DATABASE IF EXISTS {replay_db} WITH (FORCE)"),
            1,
        )
        .unwrap();
        pg_driver::close(admin);
        let _ = std::fs::remove_file(&path);
    }

    /// 单表范围只写点名的表；仅结构不写 INSERT；取消要把半成品删掉。
    #[test]
    fn table_scope_is_isolated_and_cancel_removes_partial_file() {
        let mut req = request(fixture("dump_tbl"), "dump_tbl_out");
        req.scope = DumpScope::Table;
        req.table = "parent".into();
        req.include_data = false;
        let path = std::path::PathBuf::from(&req.path);
        let _ = std::fs::remove_file(&path);
        let cancel = AtomicBool::new(false);

        let outcome = run(&req, &mut |_| {}, &cancel).expect("单表转储要成功");
        assert_eq!((outcome.tables, outcome.views, outcome.rows), (1, 0, 0));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("CREATE TABLE \"main\".\"parent\""), "{text}");
        assert!(!text.contains("CREATE TABLE \"main\".\"child\""), "别的表不能混进来");
        assert!(!text.contains("INSERT INTO"), "仅结构不出数据");
        let _ = std::fs::remove_file(&path);

        req.include_data = true;
        let cancel = AtomicBool::new(true);
        let err = run(&req, &mut |_| {}, &cancel).expect_err("取消要报错");
        assert!(matches!(err, DumpError::Cancelled), "{err}");
        assert!(!path.exists(), "取消后不留半成品");
        let _ = std::fs::remove_file(&path);
    }
}
