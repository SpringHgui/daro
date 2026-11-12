//! 表复制：对象页 Ctrl+C / Ctrl+V 与右键「复制表」的结构 + 数据搬运。
//!
//! 同一连接内优先走服务端原生语句（MySQL `CREATE TABLE LIKE`、PG `LIKE
//! INCLUDING ALL`、SQL Server `SELECT INTO`、SQLite 读 `sqlite_master` 建表 +
//! `INSERT … SELECT`）：结构、索引、默认值由数据库自己原样带过去，也不把行
//! 搬进内存。SQLite 跨文件时把源文件 ATTACH 到目标连接上，仍然是服务端复制。
//!
//! PostgreSQL 的库之间没有服务端访问通道，跨库 / 跨服务器时由客户端搬：
//! 从源库目录重建结构，数据走 COPY 文本流（见 `copy_pg`）。
//!
//! 表名规划（撞名时 `_copy` / `_copy_2`）与 SQLite 建表语句改写是纯函数，
//! 单测直接跑。

use std::collections::HashSet;

use crate::backend::Target;
use crate::mock::DbKind;
use crate::pg_driver;

/// 粘贴时带不带数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteMode {
    /// 结构和数据
    Data,
    /// 仅结构
    Structure,
}

impl PasteMode {
    pub fn label(self) -> &'static str {
        match self {
            PasteMode::Data => "结构和数据",
            PasteMode::Structure => "仅结构",
        }
    }

    pub fn with_data(self) -> bool {
        matches!(self, PasteMode::Data)
    }
}

/// 对象页 Ctrl+C 记住的表：源上下文 + 表名（按选中顺序）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableClip {
    pub conn: String,
    pub db: String,
    pub schema: String,
    pub tables: Vec<String>,
}

/// 一张表的落点：源表名 → 目标表名。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyItem {
    pub source: String,
    pub target: String,
}

/// 一次粘贴（可能多张表）的完整任务。后台线程只认这一份。
#[derive(Clone, Debug)]
pub struct CopyBatch {
    pub src: Target,
    pub dst: Target,
    /// SQLite 源库文件；跨文件粘贴时 ATTACH 用。
    pub src_path: Option<String>,
    pub dst_conn: String,
    pub dst_db: String,
    pub dst_schema: String,
    pub items: Vec<CopyItem>,
    pub mode: PasteMode,
}

/// 逐张复制，返回每张表的结果（顺序与 `items` 一致）。
pub fn run_batch(batch: &CopyBatch) -> Vec<(String, Result<(), String>)> {
    batch
        .items
        .iter()
        .map(|item| {
            (
                item.source.clone(),
                copy_table(
                    &batch.src,
                    &batch.dst,
                    &item.source,
                    &item.target,
                    batch.mode,
                    batch.src_path.as_deref(),
                ),
            )
        })
        .collect()
}

/// 源库已有表名 → 目标库落点。原名没被占用就用原名，否则依次
/// `_copy`、`_copy_2`…；比较按小写做，避免大小写不敏感的服务端起冲突。
/// `max_len` 是方言的表名长度上限，0 表示不限。
pub fn plan_names(existing: &[String], tables: &[String], max_len: usize) -> Vec<(String, String)> {
    let mut used: HashSet<String> = existing.iter().map(|n| n.to_lowercase()).collect();
    tables
        .iter()
        .map(|table| {
            let mut candidate = fit_name(table, "", max_len);
            let mut n = 1usize;
            while used.contains(&candidate.to_lowercase()) {
                let suffix = if n == 1 {
                    "_copy".to_string()
                } else {
                    format!("_copy_{n}")
                };
                candidate = fit_name(table, &suffix, max_len);
                n += 1;
            }
            used.insert(candidate.to_lowercase());
            (table.clone(), candidate)
        })
        .collect()
}

/// 各方言表名长度上限（0 = SQLite 不设限）。
pub fn name_limit(kind: DbKind) -> usize {
    match kind {
        DbKind::MySql => 64,
        DbKind::PostgreSql => 63,
        DbKind::SqlServer => 128,
        _ => 0,
    }
}

fn fit_name(base: &str, suffix: &str, max_len: usize) -> String {
    if max_len == 0 {
        return format!("{base}{suffix}");
    }
    let room = max_len.saturating_sub(suffix.chars().count()).max(1);
    let trimmed: String = base.chars().take(room).collect();
    format!("{trimmed}{suffix}")
}

/// 复制一张表。`src_path` 只在 SQLite 跨文件时用得到。
pub fn copy_table(
    src: &Target,
    dst: &Target,
    source: &str,
    target: &str,
    mode: PasteMode,
    src_path: Option<&str>,
) -> Result<(), String> {
    match (src, dst) {
        (Target::MySql { .. }, Target::MySql { .. }) => {
            require_same_connection(src, dst, "MySQL")?;
            let create = format!(
                "CREATE TABLE {} LIKE {}",
                dst.qualified_table(target),
                src.qualified_table(source)
            );
            dst.execute(&create, 1)?;
            if mode.with_data() {
                let insert = format!(
                    "INSERT INTO {} SELECT * FROM {}",
                    dst.qualified_table(target),
                    src.qualified_table(source)
                );
                if let Err(e) = dst.execute(&insert, 1) {
                    let _ = dst.execute(&format!("DROP TABLE {}", dst.qualified_table(target)), 1);
                    return Err(e);
                }
            }
            Ok(())
        }
        (
            Target::Pg {
                conn: src_conn,
                db: src_db,
                schema: src_schema,
                params: src_params,
            },
            Target::Pg {
                conn: dst_conn,
                db: dst_db,
                schema: dst_schema,
                params: dst_params,
            },
        ) => {
            // 同一条连接、同一个库才有服务端捷径；跨库 / 跨服务器只能客户端搬。
            if src_conn == dst_conn && src_db == dst_db {
                let create = format!(
                    "CREATE TABLE {} (LIKE {} INCLUDING ALL)",
                    dst.qualified_table(target),
                    src.qualified_table(source)
                );
                dst.execute(&create, 1)?;
                if mode.with_data() {
                    let insert = format!(
                        "INSERT INTO {} SELECT * FROM {}",
                        dst.qualified_table(target),
                        src.qualified_table(source)
                    );
                    if let Err(e) = dst.execute(&insert, 1) {
                        let _ =
                            dst.execute(&format!("DROP TABLE {}", dst.qualified_table(target)), 1);
                        return Err(e);
                    }
                }
                Ok(())
            } else {
                copy_pg(
                    src_params, src_schema, source, dst_params, dst_schema, target, mode,
                )
            }
        }
        (Target::SqlServer { .. }, Target::SqlServer { .. }) => {
            require_same_connection(src, dst, "SQL Server")?;
            let top = if mode.with_data() { "" } else { "TOP 0 " };
            let sql = format!(
                "SELECT {top}* INTO {} FROM {}",
                dst.qualified_table(target),
                src.qualified_table(source)
            );
            dst.execute(&sql, 1)?;
            Ok(())
        }
        (
            Target::Sqlite {
                db: src_db,
                path: src_file,
                ..
            },
            Target::Sqlite {
                db: dst_db,
                path: dst_file,
                ..
            },
        ) => copy_sqlite(
            dst, src_file, dst_file, src_db, dst_db, source, target, mode, src_path,
        ),
        _ => Err(format!(
            "暂不支持在 {} 与 {} 之间粘贴表",
            src.kind().label(),
            dst.kind().label()
        )),
    }
}

fn require_same_connection(src: &Target, dst: &Target, engine: &str) -> Result<(), String> {
    if src.conn_name() != dst.conn_name() {
        return Err(format!(
            "{engine} 跨连接粘贴表暂不支持：请在同一个连接内跨数据库复制"
        ));
    }
    Ok(())
}

// ── PostgreSQL 跨库 / 跨连接粘贴 ─────────────────────────────────
//
// PG 一条连接只看得见一个库，服务端也没有跨库 DDL，所以跨库复制只能由
// 客户端做（参考客户端的 transfer 也是这条路）：从源库目录里读出列、默认值、
// 约束、索引、注释，在目标库建一张新表，再把行经 COPY 文本流搬过去。
// 两端各开一条临时连接，不占 UI 用的注册表句柄；目标库上的 DDL 与数据在
// 同一个事务里，任一步失败整表回滚，不会留下半成品。

struct PgCol {
    name: String,
    ty: String,
    collation: Option<String>,
    not_null: bool,
    default: Option<String>,
    identity: char,
    generated: char,
    serial: bool,
}

struct PgFk {
    name: String,
    def: String,
    ref_schema: String,
    ref_table: String,
    self_ref: bool,
}

struct PgTable {
    columns: Vec<PgCol>,
    constraints: Vec<(String, String)>,
    fks: Vec<PgFk>,
    indexes: Vec<(String, String)>,
    comment: Option<String>,
    col_comments: Vec<(String, String)>,
}

#[allow(clippy::too_many_arguments)]
fn copy_pg(
    src_params: &pg_driver::Params,
    src_schema: &str,
    source: &str,
    dst_params: &pg_driver::Params,
    dst_schema: &str,
    target: &str,
    mode: PasteMode,
) -> Result<(), String> {
    let mut src = pg_driver::CopySession::connect(src_params)
        .map_err(|e| format!("连源库失败：{e}"))?;
    let def = read_pg_table(&mut src, src_schema, source)?;
    let mut dst = pg_driver::CopySession::connect(dst_params)
        .map_err(|e| format!("连目标库失败：{e}"))?;
    dst.exec("BEGIN")?;
    let done = write_pg_table(
        &mut src,
        &mut dst,
        src_schema,
        source,
        dst_schema,
        target,
        &def,
        mode.with_data(),
    );
    match done {
        Ok(()) => dst
            .exec("COMMIT")
            .map_err(|e| format!("提交目标库事务失败：{e}")),
        Err(e) => {
            let _ = dst.exec("ROLLBACK");
            Err(e)
        }
    }
}

/// 从源库目录读出一张表的完整结构（只看普通表）。
fn read_pg_table(
    src: &mut pg_driver::CopySession,
    schema: &str,
    table: &str,
) -> Result<PgTable, String> {
    let rel = src.query(&format!(
        "SELECT c.oid::text, c.relkind::text FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = {} AND c.relname = {}",
        sql_literal(schema),
        sql_literal(table)
    ))?;
    let Some(row) = rel.first() else {
        return Err(format!("源库里找不到表「{table}」"));
    };
    let oid = cell(row, 0);
    if cell(row, 1) != "r" {
        return Err(format!("「{table}」不是普通表，暂不支持跨库粘贴"));
    }
    if oid.is_empty() {
        return Err(format!("读不到表「{table}」的目录信息"));
    }

    let qualified = format!(
        "{}.{}",
        pg_driver::quote_ident(schema),
        pg_driver::quote_ident(table)
    );
    let mut columns = Vec::new();
    for r in src.query(&format!(
        "SELECT a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod), \
                a.attnotnull::text, COALESCE(pg_get_expr(d.adbin, d.adrelid), ''), \
                a.attidentity::text, a.attgenerated::text, \
                COALESCE(pg_get_serial_sequence({}, a.attname), ''), \
                COALESCE(CASE WHEN a.attcollation <> t.typcollation \
                         THEN format('%I.%I', cn.nspname, co.collname) END, '') \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
         LEFT JOIN pg_catalog.pg_collation co ON co.oid = a.attcollation \
         LEFT JOIN pg_catalog.pg_namespace cn ON cn.oid = co.collnamespace \
         LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
         WHERE a.attrelid = {oid} AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
        sql_literal(&qualified)
    ))? {
        let default = cell(&r, 3);
        columns.push(PgCol {
            name: cell(&r, 0),
            ty: cell(&r, 1),
            not_null: cell(&r, 2) == "t",
            default: (!default.is_empty()).then_some(default),
            identity: cell(&r, 4).chars().next().unwrap_or(' '),
            generated: cell(&r, 5).chars().next().unwrap_or(' '),
            serial: !cell(&r, 6).is_empty(),
            collation: {
                let c = cell(&r, 7);
                (!c.is_empty()).then_some(c)
            },
        });
    }
    if columns.is_empty() {
        return Err(format!("「{table}」没有可复制的列"));
    }

    let mut constraints = Vec::new();
    let mut fks = Vec::new();
    for r in src.query(&format!(
        "SELECT con.conname, con.contype::text, pg_get_constraintdef(con.oid, false), \
                COALESCE(rn.nspname, ''), COALESCE(rc.relname, ''), \
                (con.confrelid = con.conrelid)::text \
         FROM pg_catalog.pg_constraint con \
         LEFT JOIN pg_catalog.pg_class rc ON rc.oid = con.confrelid \
         LEFT JOIN pg_catalog.pg_namespace rn ON rn.oid = rc.relnamespace \
         WHERE con.conrelid = {oid} AND con.contype IN ('p','u','c','f','x') \
         ORDER BY CASE con.contype WHEN 'p' THEN 0 WHEN 'u' THEN 1 WHEN 'x' THEN 2 \
                  WHEN 'c' THEN 3 ELSE 9 END, con.conname"
    ))? {
        let name = cell(&r, 0);
        let def = cell(&r, 2);
        if def.is_empty() {
            continue;
        }
        if cell(&r, 1) == "f" {
            fks.push(PgFk {
                name,
                def,
                ref_schema: cell(&r, 3),
                ref_table: cell(&r, 4),
                self_ref: cell(&r, 5) == "t",
            });
        } else {
            constraints.push((name, def));
        }
    }

    // 约束背后的索引由 ADD CONSTRAINT 自己建，这里只搬额外索引。
    let mut indexes = Vec::new();
    for r in src.query(&format!(
        "SELECT ic.relname, pg_get_indexdef(i.indexrelid) \
         FROM pg_catalog.pg_index i \
         JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid \
         WHERE i.indrelid = {oid} AND i.indisvalid AND NOT i.indisprimary \
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint c \
                           WHERE c.conindid = i.indexrelid) \
         ORDER BY ic.relname"
    ))? {
        indexes.push((cell(&r, 0), cell(&r, 1)));
    }

    let comment = src
        .query(&format!(
            "SELECT COALESCE(obj_description({oid}, 'pg_class'), '')"
        ))?
        .first()
        .map(|r| cell(r, 0))
        .filter(|c| !c.is_empty());
    let mut col_comments = Vec::new();
    for r in src.query(&format!(
        "SELECT a.attname, col_description(a.attrelid, a.attnum)::text \
         FROM pg_catalog.pg_attribute a \
         WHERE a.attrelid = {oid} AND a.attnum > 0 AND NOT a.attisdropped \
           AND col_description(a.attrelid, a.attnum) IS NOT NULL ORDER BY a.attnum"
    ))? {
        col_comments.push((cell(&r, 0), cell(&r, 1)));
    }

    Ok(PgTable {
        columns,
        constraints,
        fks,
        indexes,
        comment,
        col_comments,
    })
}

/// 在目标库落结构、搬数据、补索引 / 约束 / 注释。调用方保证已在事务里。
#[allow(clippy::too_many_arguments)]
fn write_pg_table(
    src: &mut pg_driver::CopySession,
    dst: &mut pg_driver::CopySession,
    src_schema: &str,
    source: &str,
    dst_schema: &str,
    target: &str,
    def: &PgTable,
    with_data: bool,
) -> Result<(), String> {
    let dst_table = pg_qualified(dst_schema, target);
    dst.exec(&pg_create_table(dst_schema, target, def))?;

    // 生成列由目标库自己算，不参与 COPY；整表都是生成列时没有可搬的行。
    let cols: Vec<String> = def
        .columns
        .iter()
        .filter(|c| c.generated != 's')
        .map(|c| pg_driver::quote_ident(&c.name))
        .collect();
    if with_data && !cols.is_empty() {
        let list = cols.join(", ");
        let from = format!(
            "COPY (SELECT {list} FROM {}.{}) TO STDOUT",
            pg_driver::quote_ident(src_schema),
            pg_driver::quote_ident(source)
        );
        let into = format!("COPY {dst_table} ({list}) FROM STDIN");
        dst.copy_from(src, &from, &into)?;
        reset_pg_sequences(dst, dst_schema, target, def)?;
    }

    // 约束 / 索引的名字在目标模式里重新取唯一的：主键 / 唯一约束背后那条
    // 索引与表共用 pg_class 命名空间，照搬源名会和刚贴上来的同名表撞。
    let mut used = pg_relation_names(dst, dst_schema)?;
    for (name, sql) in &def.constraints {
        let new_name = unique_pg_name(&mut used, &format!("{target}_{name}"));
        dst.exec(&format!(
            "ALTER TABLE {dst_table} ADD CONSTRAINT {} {sql}",
            pg_driver::quote_ident(&new_name)
        ))?;
    }

    // 额外索引：定义里的表名 / 索引名就地改写。
    for (old, sql) in &def.indexes {
        let new_name = unique_pg_name(&mut used, &format!("{target}_{old}"));
        // CREATE INDEX 的索引名不能带模式名（索引跟着表走），ON 后面可以带。
        let stmt = replace_name_after(sql, "INDEX", &pg_driver::quote_ident(&new_name))?;
        let stmt = replace_name_after(&stmt, "ON", &dst_table)?;
        dst.exec(&stmt)?;
    }

    for fk in &def.fks {
        if let Some(sql) = pg_fk_sql(dst, fk, src_schema, dst_schema, target)? {
            let new_name = unique_pg_name(&mut used, &format!("{target}_{}", fk.name));
            dst.exec(&format!(
                "ALTER TABLE {dst_table} ADD CONSTRAINT {} {sql}",
                pg_driver::quote_ident(&new_name)
            ))?;
        }
    }

    if let Some(text) = &def.comment {
        dst.exec(&format!(
            "COMMENT ON TABLE {dst_table} IS {}",
            sql_literal(text)
        ))?;
    }
    for (col, text) in &def.col_comments {
        dst.exec(&format!(
            "COMMENT ON COLUMN {dst_table}.{} IS {}",
            pg_driver::quote_ident(col),
            sql_literal(text)
        ))?;
    }
    Ok(())
}

fn pg_create_table(schema: &str, table: &str, def: &PgTable) -> String {
    let cols: Vec<String> = def.columns.iter().map(pg_column_sql).collect();
    format!(
        "CREATE TABLE {}.{} (\n    {}\n)",
        pg_driver::quote_ident(schema),
        pg_driver::quote_ident(table),
        cols.join(",\n    ")
    )
}

fn pg_column_sql(c: &PgCol) -> String {
    let mut s = format!("{} {}", pg_driver::quote_ident(&c.name), c.ty);
    if let Some(collation) = &c.collation {
        s.push_str(&format!(" COLLATE {collation}"));
    }
    if c.generated == 's' {
        // 生成列的表达式存放在 pg_attrdef 里，原样带上。
        s.push_str(&format!(
            " GENERATED ALWAYS AS ({}) STORED",
            c.default.as_deref().unwrap_or("NULL")
        ));
    } else if c.identity != ' ' || c.serial {
        // SERIAL 在目标库改成标识列：自动建序列，也不依赖源库那条序列名。
        s.push_str(if c.identity == 'a' {
            " GENERATED ALWAYS AS IDENTITY"
        } else {
            " GENERATED BY DEFAULT AS IDENTITY"
        });
    } else if let Some(default) = &c.default {
        s.push_str(&format!(" DEFAULT {default}"));
    }
    if c.not_null {
        s.push_str(" NOT NULL");
    }
    s
}

/// 标识列 / SERIAL 搬完数据后把序列推到「最大值 + 1」，下一条 INSERT 不撞号。
fn reset_pg_sequences(
    dst: &mut pg_driver::CopySession,
    schema: &str,
    table: &str,
    def: &PgTable,
) -> Result<(), String> {
    for c in &def.columns {
        if c.identity == ' ' && !c.serial {
            continue;
        }
        let qualified = pg_qualified(schema, table);
        dst.exec(&format!(
            "SELECT setval(pg_get_serial_sequence({}, {}), \
             COALESCE((SELECT MAX({}) FROM {qualified}), 0) + 1, false)",
            sql_literal(&qualified),
            sql_literal(&c.name),
            pg_driver::quote_ident(&c.name)
        ))?;
    }
    Ok(())
}

/// 外键只在被引用表于目标库存在时重建；否则跳过（不因一张子表把整次复制拖垮）。
fn pg_fk_sql(
    dst: &mut pg_driver::CopySession,
    fk: &PgFk,
    src_schema: &str,
    dst_schema: &str,
    target: &str,
) -> Result<Option<String>, String> {
    if fk.self_ref {
        return Ok(Some(replace_reference(
            &fk.def,
            &pg_qualified(dst_schema, target),
        )?));
    }
    let mut candidates: Vec<(&str, &str)> = Vec::new();
    if fk.ref_schema == src_schema {
        candidates.push((dst_schema, &fk.ref_table));
    }
    candidates.push((&fk.ref_schema, &fk.ref_table));
    for (schema, table) in candidates {
        if !schema.is_empty() && pg_relation_exists(dst, schema, table)? {
            return Ok(Some(replace_reference(
                &fk.def,
                &pg_qualified(schema, table),
            )?));
        }
    }
    Ok(None)
}

fn pg_relation_exists(
    dst: &mut pg_driver::CopySession,
    schema: &str,
    table: &str,
) -> Result<bool, String> {
    let rows = dst.query(&format!(
        "SELECT to_regclass({}) IS NOT NULL",
        sql_literal(&pg_qualified(schema, table))
    ))?;
    Ok(rows.first().is_some_and(|r| cell(r, 0) == "t"))
}

/// 目标模式里已被占用的 pg_class 名字（表 / 索引 / 序列 / 视图都算）。
fn pg_relation_names(
    dst: &mut pg_driver::CopySession,
    schema: &str,
) -> Result<HashSet<String>, String> {
    let rows = dst.query(&format!(
        "SELECT c.relname FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = {}",
        sql_literal(schema)
    ))?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.into_iter().next())
        .map(|n| n.to_lowercase())
        .collect())
}

fn unique_pg_name(used: &mut HashSet<String>, base: &str) -> String {
    let mut name = fit_name(base, "", 63);
    let mut n = 2usize;
    while used.contains(&name.to_lowercase()) {
        name = fit_name(base, &format!("_{n}"), 63);
        n += 1;
    }
    used.insert(name.to_lowercase());
    name
}

fn pg_qualified(schema: &str, table: &str) -> String {
    format!(
        "{}.{}",
        pg_driver::quote_ident(schema),
        pg_driver::quote_ident(table)
    )
}

fn cell(row: &[String], i: usize) -> String {
    row.get(i).cloned().unwrap_or_default()
}

/// 把约束定义里 `REFERENCES` 后面的（可带模式名的）表名换成新落点。
fn replace_reference(sql: &str, replacement: &str) -> Result<String, String> {
    let kw = find_keyword(sql, "REFERENCES")
        .ok_or_else(|| format!("外键定义里找不到 REFERENCES：{sql}"))?;
    let start = skip_ws(sql, kw + "REFERENCES".len());
    let (_, first_end) = parse_ident(sql, start)
        .ok_or_else(|| format!("REFERENCES 后面没有表名：{sql}"))?;
    let dot = skip_ws(sql, first_end);
    let end = if sql.as_bytes().get(dot) == Some(&b'.') {
        let after = skip_ws(sql, dot + 1);
        parse_ident(sql, after)
            .ok_or_else(|| format!("REFERENCES 后面的模式名不完整：{sql}"))?
            .1
    } else {
        first_end
    };
    Ok(format!("{}{replacement}{}", &sql[..start], &sql[end..]))
}

/// SQLite：同文件直接用源库名；跨文件把源文件 ATTACH 到目标连接上。
/// 表结构取 `sqlite_master` 里的原始建表语句（默认值 / AUTOINCREMENT /
/// WITHOUT ROWID 都原样保留），索引按同一表名重新生成唯一索引名。
#[allow(clippy::too_many_arguments)]
fn copy_sqlite(
    dst: &Target,
    src_file: &str,
    dst_file: &str,
    src_db: &str,
    dst_db: &str,
    source: &str,
    target: &str,
    mode: PasteMode,
    attach_path: Option<&str>,
) -> Result<(), String> {
    let same_file = !src_file.is_empty() && src_file == dst_file;
    let alias = if same_file {
        None
    } else {
        if src_db != "main" {
            return Err(format!(
                "只能跨文件复制 main 库里的表（「{source}」在附加库「{src_db}」里）"
            ));
        }
        let path = attach_path
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .unwrap_or(src_file);
        if path.is_empty() {
            return Err("源 SQLite 连接没有库文件路径".to_string());
        }
        let alias = unique_attach_alias(dst)?;
        let stmt = format!(
            "ATTACH DATABASE {} AS {}",
            sql_literal(path),
            dst.quote(&alias)
        );
        dst.execute(&stmt, 1)?;
        Some(alias)
    };
    let src_space = alias.as_deref().unwrap_or(src_db);
    let target_table = dst.qualified_table(target);
    // 只有本函数建出来的目标表才在失败时回删；同名表本来就在的话不能连锅端
    let mut created = false;

    let result = (|| -> Result<(), String> {
        let ddl = query_one(
            dst,
            &format!(
                "SELECT COALESCE(sql, '') FROM {}.sqlite_master \
                 WHERE type = 'table' AND name = {}",
                dst.quote(src_space),
                sql_literal(source)
            ),
        )?;
        if ddl.trim().is_empty() {
            return Err(format!("「{source}」不是表，或没有可复制的建表语句"));
        }
        // CREATE TABLE … AS SELECT 的原文里带着取数：照抄一遍会把数据也带过去
        // （跨文件时源表还不在目标库里，直接失败）。改成只取列形状，数据另走
        // 下面的 INSERT … SELECT。
        let create = if create_populates_rows(&ddl) {
            format!(
                "CREATE TABLE {target_table} AS SELECT * FROM {}.{} WHERE 0",
                dst.quote(src_space),
                dst.quote(source)
            )
        } else {
            replace_name_after(&ddl, "TABLE", &target_table)?
        };
        dst.execute(&create, 1)?;
        created = true;

        // 索引跟着表走；原名在目标库可能已被占用，统一加目标表名前缀。
        let mut used: HashSet<String> = query_rows(
            dst,
            &format!(
                "SELECT name FROM {}.sqlite_master \
                 WHERE type = 'index' AND name IS NOT NULL",
                dst.quote(dst_db)
            ),
        )?
        .into_iter()
        .filter_map(|r| r.into_iter().next())
        .map(|n| n.to_lowercase())
        .collect();
        let indexes = query_rows(
            dst,
            &format!(
                "SELECT name, COALESCE(sql, '') FROM {}.sqlite_master \
                 WHERE type = 'index' AND tbl_name = {} AND sql IS NOT NULL \
                 ORDER BY name",
                dst.quote(src_space),
                sql_literal(source)
            ),
        )?;
        for row in indexes {
            let Some(old_name) = row.first() else {
                continue;
            };
            let Some(sql) = row.get(1) else { continue };
            if sql.trim().is_empty() {
                continue;
            }
            let new_name = unique_sqlite_name(&mut used, &format!("{target}_{old_name}"));
            // SQLite 的索引名可以带库前缀，但 ON 后面的表名必须是裸名，
            // 写成 "main"."t" 会直接语法错。
            let index_name = format!("{}.{}", dst.quote(dst_db), dst.quote(&new_name));
            let stmt = replace_name_after(sql, "INDEX", &index_name)?;
            let stmt = replace_name_after(&stmt, "ON", &dst.quote(target))?;
            dst.execute(&stmt, 1)?;
        }

        if mode.with_data() {
            let insert = format!(
                "INSERT INTO {target_table} SELECT * FROM {}.{}",
                dst.quote(src_space),
                dst.quote(source)
            );
            dst.execute(&insert, 1)?;
        }
        Ok(())
    })();

    if let Some(alias) = alias {
        let _ = dst.execute(&format!("DETACH DATABASE {}", dst.quote(&alias)), 1);
    }
    if result.is_err() && created {
        let _ = dst.execute(&format!("DROP TABLE {target_table}"), 1);
    }
    result
}

/// 建表语句自己会不会把行也带进去（`CREATE TABLE x AS SELECT …` / `AS WITH …`）。
fn create_populates_rows(ddl: &str) -> bool {
    let mut from = 0usize;
    while let Some(i) = find_keyword(&ddl[from..], "AS").map(|i| i + from) {
        let j = skip_ws(ddl, i + 2);
        if word_at(ddl, j, "SELECT") || word_at(ddl, j, "WITH") {
            return true;
        }
        from = i + 2;
    }
    false
}

fn word_at(sql: &str, i: usize, word: &str) -> bool {
    sql.get(i..i + word.len())
        .is_some_and(|s| s.eq_ignore_ascii_case(word))
        && sql
            .as_bytes()
            .get(i + word.len())
            .is_none_or(|b| !is_ident_byte(*b))
}

/// 目标连接上还没被占用的 ATTACH 别名。
fn unique_attach_alias(dst: &Target) -> Result<String, String> {    let used: HashSet<String> = query_rows(dst, "PRAGMA database_list")?
        .into_iter()
        .filter_map(|r| r.get(1).cloned())
        .map(|n| n.to_lowercase())
        .collect();
    let mut n = 1usize;
    loop {
        let alias = if n == 1 {
            "daro_copy_src".to_string()
        } else {
            format!("daro_copy_src_{n}")
        };
        if !used.contains(&alias.to_lowercase()) {
            return Ok(alias);
        }
        n += 1;
    }
}

fn unique_sqlite_name(used: &mut HashSet<String>, base: &str) -> String {
    let mut name = base.to_string();
    let mut n = 2usize;
    while used.contains(&name.to_lowercase()) {
        name = format!("{base}_{n}");
        n += 1;
    }
    used.insert(name.to_lowercase());
    name
}

fn query_rows(tg: &Target, sql: &str) -> Result<Vec<Vec<String>>, String> {
    let out = tg.execute(sql, 10_000)?;
    Ok(out.select.map(|page| page.rows).unwrap_or_default())
}

fn query_one(tg: &Target, sql: &str) -> Result<String, String> {
    Ok(query_rows(tg, sql)?
        .into_iter()
        .next()
        .and_then(|r| r.into_iter().next())
        .unwrap_or_default())
}

fn sql_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// 把 SQL 里第一个 `keyword`（引号外、整词）后面的标识符换成 `replacement`。
/// `TABLE` / `INDEX` 后面会跳过可选的 `IF NOT EXISTS`。
fn replace_name_after(sql: &str, keyword: &str, replacement: &str) -> Result<String, String> {
    let kw =
        find_keyword(sql, keyword).ok_or_else(|| format!("DDL 里找不到关键字 {keyword}：{sql}"))?;
    let mut i = skip_ws(sql, kw + keyword.len());
    if keyword.eq_ignore_ascii_case("TABLE") || keyword.eq_ignore_ascii_case("INDEX") {
        i = skip_if_not_exists(sql, i);
    }
    let (start, end) =
        parse_ident(sql, i).ok_or_else(|| format!("DDL 里 {keyword} 后面没有标识符：{sql}"))?;
    Ok(format!("{}{}{}", &sql[..start], replacement, &sql[end..]))
}

/// 引号外的整词定位：跳过单引号 / 双引号 / 反引号 / [方括号] 里的内容。
fn find_keyword(sql: &str, keyword: &str) -> Option<usize> {
    let bytes = sql.as_bytes();
    let kw = keyword.as_bytes();
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(close) = quote {
            if b == close {
                if close != b']' && i + 1 < bytes.len() && bytes[i + 1] == b {
                    i += 2;
                    continue;
                }
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' | b'`' => {
                quote = Some(b);
                i += 1;
            }
            b'[' => {
                quote = Some(b']');
                i += 1;
            }
            _ => {
                if i + kw.len() <= bytes.len()
                    && sql[i..i + kw.len()].eq_ignore_ascii_case(keyword)
                    && (i == 0 || !is_ident_byte(bytes[i - 1]))
                    && (i + kw.len() == bytes.len() || !is_ident_byte(bytes[i + kw.len()]))
                {
                    return Some(i);
                }
                i += 1;
            }
        }
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

fn skip_ws(sql: &str, mut i: usize) -> usize {
    let bytes = sql.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn skip_if_not_exists(sql: &str, start: usize) -> usize {
    let mut i = start;
    for word in ["IF", "NOT", "EXISTS"] {
        i = skip_ws(sql, i);
        let end = i + word.len();
        if end > sql.len() || !sql[i..end].eq_ignore_ascii_case(word) {
            return start;
        }
        let after = sql.as_bytes().get(end).copied();
        if after.is_some_and(is_ident_byte) {
            return start;
        }
        i = end;
    }
    skip_ws(sql, i)
}

fn parse_ident(sql: &str, i: usize) -> Option<(usize, usize)> {
    let bytes = sql.as_bytes();
    if i >= bytes.len() {
        return None;
    }
    match bytes[i] {
        q @ (b'"' | b'\'' | b'`') => {
            let mut j = i + 1;
            while j < bytes.len() {
                if bytes[j] == q {
                    if j + 1 < bytes.len() && bytes[j + 1] == q {
                        j += 2;
                    } else {
                        return Some((i, j + 1));
                    }
                } else {
                    j += 1;
                }
            }
            None
        }
        b'[' => {
            let mut j = i + 1;
            while j < bytes.len() {
                if bytes[j] == b']' {
                    return Some((i, j + 1));
                }
                j += 1;
            }
            None
        }
        _ => {
            let mut j = i;
            while j < bytes.len()
                && !bytes[j].is_ascii_whitespace()
                && !matches!(bytes[j], b'(' | b')' | b',' | b';')
            {
                j += 1;
            }
            (j > i).then_some((i, j))
        }
    }
}

/// `Target` 只在 SQLite 跨文件时用得到源文件路径；这里拿到的是连接名。
trait TargetNames {
    fn conn_name(&self) -> &str;
}

impl TargetNames for Target {
    fn conn_name(&self) -> &str {
        match self {
            Target::Sqlite { conn, .. }
            | Target::Access { conn, .. }
            | Target::Pg { conn, .. }
            | Target::MySql { conn, .. }
            | Target::SqlServer { conn, .. }
            | Target::Mongo { conn, .. } => conn,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plan_keeps_free_names_and_suffixes_collisions() {
        let existing = names(&["orders", "Users"]);
        let plan = plan_names(&existing, &names(&["orders", "users", "items"]), 64);
        assert_eq!(
            plan,
            vec![
                ("orders".to_string(), "orders_copy".to_string()),
                ("users".to_string(), "users_copy".to_string()),
                ("items".to_string(), "items".to_string()),
            ]
        );
    }

    #[test]
    fn plan_bumps_suffix_until_free_and_handles_duplicates() {
        let existing = names(&["t", "t_copy", "t_copy_2"]);
        let plan = plan_names(&existing, &names(&["t", "t"]), 0);
        assert_eq!(plan[0].1, "t_copy_3");
        assert_eq!(plan[1].1, "t_copy_4");
    }

    #[test]
    fn plan_truncates_to_dialect_limit() {
        let base = "a".repeat(63);
        // 没撞名就原样用，哪怕没顶满上限
        let plan = plan_names(&[], &[base.clone()], 64);
        assert_eq!(plan[0].1, base);
        // 撞名要加后缀：总长压到 64 以内
        let plan = plan_names(&[base.clone()], &[base], 64);
        assert_eq!(plan[0].1.chars().count(), 64);
        assert!(plan[0].1.ends_with("_copy"));
    }

    #[test]
    fn create_table_name_is_rewritten_in_place() {
        let ddl = "CREATE TABLE \"member\" (id INTEGER, name TEXT)";
        assert_eq!(
            replace_name_after(ddl, "TABLE", "\"main\".\"member_copy\"").unwrap(),
            "CREATE TABLE \"main\".\"member_copy\" (id INTEGER, name TEXT)"
        );
        let ddl = "CREATE TABLE IF NOT EXISTS member (id INTEGER)";
        assert_eq!(
            replace_name_after(ddl, "TABLE", "\"x\"").unwrap(),
            "CREATE TABLE IF NOT EXISTS \"x\" (id INTEGER)"
        );
        // 列名 / 默认值里的 TABLE 字样不当关键字
        let ddl = "CREATE TABLE bad (note TEXT DEFAULT 'CREATE TABLE nope', table_id INTEGER)";
        assert_eq!(
            replace_name_after(ddl, "TABLE", "\"main\".\"copy\"").unwrap(),
            "CREATE TABLE \"main\".\"copy\" (note TEXT DEFAULT 'CREATE TABLE nope', table_id INTEGER)"
        );
    }

    #[test]
    fn index_sql_rewrites_name_and_table() {
        let sql = "CREATE UNIQUE INDEX \"ix_name\" ON member(name)";
        let sql = replace_name_after(sql, "INDEX", "\"member_copy_ix_name\"").unwrap();
        let sql = replace_name_after(&sql, "ON", "\"main\".\"member_copy\"").unwrap();
        assert_eq!(
            sql,
            "CREATE UNIQUE INDEX \"member_copy_ix_name\" ON \"main\".\"member_copy\"(name)"
        );
    }

    #[test]
    fn quoted_identifiers_with_escaped_quotes_survive_rewrite() {
        let ddl = "CREATE TABLE \"we\"\"ird\" (a INTEGER)";
        assert_eq!(
            replace_name_after(ddl, "TABLE", "\"main\".\"copy\"").unwrap(),
            "CREATE TABLE \"main\".\"copy\" (a INTEGER)"
        );
    }

    fn pg_col(name: &str, ty: &str) -> PgCol {
        PgCol {
            name: name.to_string(),
            ty: ty.to_string(),
            collation: None,
            not_null: false,
            default: None,
            identity: ' ',
            generated: ' ',
            serial: false,
        }
    }

    #[test]
    fn pg_create_table_covers_identity_serial_generated_and_collation() {
        let mut id = pg_col("id", "bigint");
        id.not_null = true;
        id.identity = 'a';
        let mut legacy = pg_col("legacy", "integer");
        legacy.not_null = true;
        legacy.serial = true;
        legacy.default = Some("nextval('\"public\".\"t_legacy_seq\"'::regclass)".to_string());
        let mut email = pg_col("email", "text");
        email.not_null = true;
        email.default = Some("'x'::text".to_string());
        email.collation = Some("\"public\".\"C\"".to_string());
        let mut doubled = pg_col("double_it", "integer");
        doubled.generated = 's';
        doubled.default = Some("(id * 2)".to_string());
        let def = PgTable {
            columns: vec![id, legacy, email, doubled],
            constraints: vec![],
            fks: vec![],
            indexes: vec![],
            comment: None,
            col_comments: vec![],
        };
        assert_eq!(
            pg_create_table("归档", "T", &def),
            "CREATE TABLE \"归档\".\"T\" (\n    \
             \"id\" bigint GENERATED ALWAYS AS IDENTITY NOT NULL,\n    \
             \"legacy\" integer GENERATED BY DEFAULT AS IDENTITY NOT NULL,\n    \
             \"email\" text COLLATE \"public\".\"C\" DEFAULT 'x'::text NOT NULL,\n    \
             \"double_it\" integer GENERATED ALWAYS AS ((id * 2)) STORED\n)"
        );
    }

    #[test]
    fn pg_fk_reference_rewrite_handles_quoted_schema_and_trailing_actions() {
        assert_eq!(
            replace_reference(
                "FOREIGN KEY (a) REFERENCES public.t(id)",
                "\"b\".\"t_copy\""
            )
            .unwrap(),
            "FOREIGN KEY (a) REFERENCES \"b\".\"t_copy\"(id)"
        );
        assert_eq!(
            replace_reference("FOREIGN KEY (a) REFERENCES t(id) ON DELETE CASCADE", "\"b\".\"c\"")
                .unwrap(),
            "FOREIGN KEY (a) REFERENCES \"b\".\"c\"(id) ON DELETE CASCADE"
        );
        assert_eq!(
            replace_reference(
                "FOREIGN KEY (a, b) REFERENCES \"we\"\"ird\".\"t bl\"(a, b) MATCH FULL",
                "\"b\".\"c\""
            )
            .unwrap(),
            "FOREIGN KEY (a, b) REFERENCES \"b\".\"c\"(a, b) MATCH FULL"
        );
    }

    #[test]
    fn pg_index_names_stay_unique_and_within_identifier_limit() {
        let mut used: HashSet<String> = ["t_ix".to_string(), "t_ix_2".to_string()]
            .into_iter()
            .collect();
        assert_eq!(unique_pg_name(&mut used, "t_ix"), "t_ix_3");
        let long = "x".repeat(70);
        let name = unique_pg_name(&mut HashSet::new(), &long);
        assert_eq!(name.chars().count(), 63);
        // 撞名时后缀要把总长压回 63 以内
        let mut used: HashSet<String> = [long.chars().take(63).collect()].into_iter().collect();
        let name = unique_pg_name(&mut used, &long);
        assert_eq!(name.chars().count(), 63);
        assert!(name.ends_with("_2"));
    }

    // ── 真 SQLite 库：结构 + 数据 + 索引确实搬过去了 ──────────────

    fn temp_db(tag: &str) -> String {
        let path = std::env::temp_dir()
            .join(format!("daro_copy_{tag}_{}.db", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_file(&path);
        path
    }

    fn conn(name: &str, path: &str) -> crate::mock::Connection {
        crate::mock::Connection::new(name, DbKind::SQLite, "", 0, path, "", "", "", false)
    }

    fn seed(name: &str, path: &str) {
        crate::sqlite_driver::open(name, path).unwrap();
        for stmt in [
            "CREATE TABLE member (\
                id INTEGER PRIMARY KEY AUTOINCREMENT, \
                name TEXT NOT NULL DEFAULT 'anon', \
                score REAL\
             )",
            "CREATE INDEX ix_member_name ON member(name)",
            "INSERT INTO member(name, score) VALUES ('a', 1.5), ('b', 2.5)",
        ] {
            crate::sqlite_driver::execute_stmt(name, stmt, 1).unwrap();
        }
    }

    fn scalar(name: &str, sql: &str) -> String {
        crate::sqlite_driver::execute_stmt(name, sql, 10)
            .unwrap()
            .select
            .and_then(|p| p.rows.into_iter().next())
            .and_then(|r| r.into_iter().next())
            .unwrap_or_default()
    }

    #[test]
    fn sqlite_copy_brings_structure_data_and_indexes() {
        let path = temp_db("same");
        let name = "复制表同文件";
        seed(name, &path);
        let c = conn(name, &path);
        let tg = Target::of(&c, "main", "").unwrap();

        copy_table(&tg, &tg, "member", "member_copy", PasteMode::Data, None).unwrap();
        assert_eq!(scalar(name, "SELECT count(*) FROM main.member_copy"), "2");
        assert_eq!(
            scalar(name, "SELECT group_concat(name, ',') FROM main.member_copy"),
            "a,b"
        );
        // AUTOINCREMENT / 默认值这些建表细节来自 sqlite_master 原文
        assert!(
            scalar(
                name,
                "SELECT sql FROM main.sqlite_master WHERE name = 'member_copy'"
            )
            .contains("AUTOINCREMENT")
        );
        // 索引跟着复制，并换了不撞车的名字
        assert_eq!(
            scalar(
                name,
                "SELECT name FROM main.sqlite_master \
                 WHERE type = 'index' AND tbl_name = 'member_copy'"
            ),
            "member_copy_ix_member_name"
        );

        // 仅结构：表建出来、索引在，行数为 0
        copy_table(
            &tg,
            &tg,
            "member",
            "member_shape",
            PasteMode::Structure,
            None,
        )
        .unwrap();
        assert_eq!(scalar(name, "SELECT count(*) FROM main.member_shape"), "0");
        assert_eq!(
            scalar(
                name,
                "SELECT count(*) FROM main.sqlite_master \
                 WHERE type = 'index' AND tbl_name = 'member_shape'"
            ),
            "1"
        );

        crate::sqlite_driver::close(name);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sqlite_copy_across_files_attaches_the_source() {
        let src_path = temp_db("xsrc");
        let dst_path = temp_db("xdst");
        let src_name = "复制源文件";
        seed(src_name, &src_path);
        let src_conn = conn(src_name, &src_path);
        let dst_conn = conn("复制目标文件", &dst_path);
        let src = Target::of(&src_conn, "main", "").unwrap();
        let dst = Target::of(&dst_conn, "main", "").unwrap();

        copy_table(
            &src,
            &dst,
            "member",
            "member_copy",
            PasteMode::Data,
            Some(&src_path),
        )
        .unwrap();
        assert_eq!(
            scalar("复制目标文件", "SELECT count(*) FROM main.member_copy"),
            "2"
        );
        assert_eq!(
            scalar(
                "复制目标文件",
                "SELECT group_concat(name, ',') FROM main.member_copy"
            ),
            "a,b"
        );
        assert_eq!(
            scalar(
                "复制目标文件",
                "SELECT name FROM main.sqlite_master \
                 WHERE type = 'index' AND tbl_name = 'member_copy'"
            ),
            "member_copy_ix_member_name"
        );
        // 源文件没被改：原表还在，且没有多出副本
        assert_eq!(scalar(src_name, "SELECT count(*) FROM main.member"), "2");
        assert_eq!(
            scalar(
                src_name,
                "SELECT count(*) FROM main.sqlite_master WHERE name = 'member_copy'"
            ),
            "0"
        );

        crate::sqlite_driver::close(src_name);
        crate::sqlite_driver::close("复制目标文件");
        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&dst_path);
    }

    #[test]
    fn sqlite_copy_of_a_ctas_table_does_not_double_rows() {
        let src_path = temp_db("ctas_src");
        let dst_path = temp_db("ctas_dst");
        let name = "复制CTAS源";
        crate::sqlite_driver::open(name, &src_path).unwrap();
        for stmt in [
            "CREATE TABLE src(id INTEGER, v TEXT)",
            "INSERT INTO src VALUES (1, 'a'), (2, 'b')",
            "CREATE TABLE snap AS SELECT * FROM src",
        ] {
            crate::sqlite_driver::execute_stmt(name, stmt, 1).unwrap();
        }
        let src = Target::of(&conn(name, &src_path), "main", "").unwrap();
        let dst = Target::of(&conn("复制CTAS目标", &dst_path), "main", "").unwrap();
        copy_table(
            &src,
            &dst,
            "snap",
            "snap_copy",
            PasteMode::Data,
            Some(&src_path),
        )
        .unwrap();
        // 建表语句原文里带着 SELECT，重放一次会把数据抄两遍
        assert_eq!(
            scalar("复制CTAS目标", "SELECT count(*) FROM main.snap_copy"),
            "2"
        );
        assert_eq!(
            scalar("复制CTAS目标", "SELECT sum(id) FROM main.snap_copy"),
            "3"
        );

        crate::sqlite_driver::close(name);
        crate::sqlite_driver::close("复制CTAS目标");
        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&dst_path);
    }

    #[test]
    fn sqlite_copy_rolls_back_the_half_made_table_on_failure() {
        let path = temp_db("fail");
        let name = "复制表失败回滚";
        seed(name, &path);
        let c = conn(name, &path);
        let tg = Target::of(&c, "main", "").unwrap();
        // 目标名已存在 → 建表那一步就撞名；既不能留下半张表，也不能
        // 把用户原来的表当成「半成品」删掉
        crate::sqlite_driver::execute_stmt(name, "CREATE TABLE taken(a INTEGER)", 1).unwrap();
        crate::sqlite_driver::execute_stmt(name, "INSERT INTO taken VALUES (7)", 1).unwrap();
        let err = copy_table(&tg, &tg, "member", "taken", PasteMode::Data, None).unwrap_err();
        assert!(!err.is_empty());
        assert_eq!(
            scalar(name, "SELECT count(*) FROM main.taken"),
            "1",
            "撞名失败不能清空已存在的目标表"
        );

        crate::sqlite_driver::close(name);
        let _ = std::fs::remove_file(&path);
    }

    // ── 真 PG 库：跨库复制（打 datawalk 开发库，平时忽略） ───────────

    fn pg_scalar(s: &mut pg_driver::CopySession, sql: &str) -> String {
        s.query(sql)
            .unwrap()
            .first()
            .map(|r| cell(r, 0))
            .unwrap_or_default()
    }

    #[test]
    #[ignore = "打 datawalk 开发库，需要网络可达"]
    fn pg_cross_database_copy_rebuilds_structure_data_and_constraints() {
        use crate::pg_driver::CopySession;

        let Some((_guard, src_params)) = crate::pg_driver::tests::pg_fixture() else {
            eprintln!("[pg] skip：读不到 datawalk 连接串");
            return;
        };
        let target_db = "daro_pg_copy_target";
        let admin_params = src_params.for_db("postgres");
        let mut admin = CopySession::connect(&admin_params).unwrap();
        let _ = admin.exec(&format!(
            "DROP DATABASE IF EXISTS {target_db} WITH (FORCE)"
        ));
        admin.exec(&format!("CREATE DATABASE {target_db}")).unwrap();
        struct DropDb(pg_driver::Params);
        impl Drop for DropDb {
            fn drop(&mut self) {
                if let Ok(mut s) = CopySession::connect(&self.0) {
                    let _ = s.exec(
                        "DROP DATABASE IF EXISTS daro_pg_copy_target WITH (FORCE)",
                    );
                }
            }
        }
        let _clean = DropDb(admin_params);

        // 源库补两张表：一张带唯一索引 / 注释 / 默认值，一张自引用外键。
        let mut seed = CopySession::connect(&src_params).unwrap();
        for stmt in [
            "CREATE TABLE public.copy_probe(\
                id bigint GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, \
                email text NOT NULL UNIQUE, nick text DEFAULT 'x'::text, tag text)",
            "CREATE UNIQUE INDEX copy_probe_tag_ix ON public.copy_probe (lower(tag))",
            "COMMENT ON TABLE public.copy_probe IS '跨库复制探针'",
            "COMMENT ON COLUMN public.copy_probe.nick IS '昵称'",
            "INSERT INTO public.copy_probe(email, tag) VALUES ('A@B', 'Aa'), ('c@d', 'Cc')",
            "CREATE TABLE public.copy_tree(\
                id int PRIMARY KEY, parent_id int REFERENCES public.copy_tree(id))",
            "INSERT INTO public.copy_tree VALUES (1, NULL), (2, 1)",
        ] {
            seed.exec(stmt).unwrap_or_else(|e| panic!("{e}\n{stmt}"));
        }

        let src = Target::Pg {
            conn: "pg-src".to_string(),
            db: src_params.dbname.clone(),
            schema: "public".to_string(),
            params: src_params.clone(),
        };
        let dst = Target::Pg {
            conn: "pg-dst".to_string(),
            db: target_db.to_string(),
            schema: "public".to_string(),
            params: src_params.for_db(target_db),
        };
        for table in ["member", "gen", "copy_probe", "copy_tree"] {
            copy_table(&src, &dst, table, table, PasteMode::Data, None)
                .unwrap_or_else(|e| panic!("复制 {table} 失败：{e}"));
        }
        copy_table(
            &src,
            &dst,
            "member",
            "member_shape",
            PasteMode::Structure,
            None,
        )
        .unwrap();

        let mut d = CopySession::connect(&src_params.for_db(target_db)).unwrap();
        // 数据、生成列、仅结构
        assert_eq!(pg_scalar(&mut d, "SELECT count(*)::text FROM public.member"), "3");
        assert_eq!(
            pg_scalar(&mut d, "SELECT count(*)::text FROM public.member_shape"),
            "0"
        );
        assert_eq!(
            pg_scalar(
                &mut d,
                "SELECT string_agg(b::text, ',' ORDER BY k) FROM public.gen"
            ),
            "42,44"
        );
        // 标识列序列推到最大值之后，继续插入不撞号
        assert_eq!(
            pg_scalar(
                &mut d,
                "INSERT INTO public.member(name) VALUES ('新') RETURNING id::text"
            ),
            "4"
        );
        assert_eq!(
            pg_scalar(
                &mut d,
                "INSERT INTO public.copy_probe(email) VALUES ('e@f') RETURNING id::text"
            ),
            "3"
        );
        assert_eq!(
            pg_scalar(&mut d, "SELECT nick FROM public.copy_probe WHERE email = 'e@f'"),
            "x"
        );
        // 唯一约束 + 额外索引 + 表 / 列注释
        assert_eq!(
            pg_scalar(
                &mut d,
                "SELECT count(*)::text FROM pg_indexes \
                 WHERE schemaname = 'public' AND tablename = 'copy_probe'"
            ),
            "3"
        );
        assert_eq!(
            pg_scalar(
                &mut d,
                "SELECT obj_description('public.copy_probe'::regclass, 'pg_class')"
            ),
            "跨库复制探针"
        );
        assert_eq!(
            pg_scalar(
                &mut d,
                "SELECT col_description('public.copy_probe'::regclass, 3)"
            ),
            "昵称"
        );
        // 自引用外键跟着表名改写，且真的生效
        assert_eq!(
            pg_scalar(
                &mut d,
                "SELECT count(*)::text FROM pg_constraint \
                 WHERE conrelid = 'public.copy_tree'::regclass AND contype = 'f'"
            ),
            "1"
        );
        d.exec("INSERT INTO public.copy_tree VALUES (3, 2)").unwrap();
        assert!(d.exec("INSERT INTO public.copy_tree VALUES (4, 99)").is_err());
    }
}
