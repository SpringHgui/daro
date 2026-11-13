//! 对象级复制：视图 / 实体化视图 / 函数 / 查询 的「Ctrl+C 记住 → Ctrl+V 粘出副本」。
//!
//! 与 [crate::table_copy] 的表复制并列：表是「结构 + 数据」的服务端 / 客户端搬运；
//! 这里这几类对象没有行数据，复制就是「读它的 CREATE 语句、换个名再建一份」。
//! 查询（cat=4）是本地已保存的 SQL 片段，复制就是按新名字多存一条。
//!
//! DDL 读取按方言走各自系统表：SQLite 看 `sqlite_master`；MySQL `SHOW CREATE`；
//! PostgreSQL 视图读 `pg_views` / `pg_matviews`、函数读 `pg_get_functiondef`；
//! SQL Server 用 `OBJECT_DEFINITION`。取不到（如 SQLite 上的函数、MySQL 上的
//! 实体化视图）返回 None，上层退回原来的「只复制名字」。

use crate::backend::Target;
use crate::mock::DbKind;

/// 对象页 Ctrl+C 记住的非表对象：源上下文 + 分类 + 名字（按选中顺序）。
///
/// `cat` 取 [crate::main_rs::CATEGORIES] 下标：1 视图 / 2 实体化视图 /
/// 3 函数 / 4 查询。表（cat=0）走 [crate::table_copy::TableClip]，不进这里。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectClip {
    pub conn: String,
    pub db: String,
    pub schema: String,
    pub cat: usize,
    pub names: Vec<String>,
}

impl ObjectClip {
    /// 落点展示用：连接 / 库 / 模式。
    pub fn context(&self) -> String {
        if self.schema.is_empty() {
            format!("{}/{}", self.conn, self.db)
        } else {
            format!("{}/{}/{}", self.conn, self.db, self.schema)
        }
    }
}

/// 一次对象粘贴的任务描述：源 / 目标库定位 + 命名好的（源名 → 目标名）清单。
///
/// 真正执行延到帧尾的 `pump_object_copies`（在后台线程跑「读定义 + 建副本」），
/// 内存库则在 pump 里同步跑。这里只把任务挂起，避免占用 UI 线程。
#[derive(Clone)]
pub struct CopyJob {
    pub src: Target,
    pub dst: Target,
    pub items: Vec<(String, String)>,
    pub cat: usize,
    pub conn_name: String,
    pub db: String,
    pub schema: String,
}

/// 读取一个对象的完整 CREATE 语句（方言系统表）。取不到返回 None。
///
/// `cat` 含义同上；`name` 是对象的（不带模式前缀的）短名。返回的文本已经是
/// 可直接重命名后 `execute` 的整段 DDL（含 `CREATE … ` 头与结尾分号）。
pub fn fetch_ddl(tg: &Target, cat: usize, name: &str) -> Option<String> {
    match (tg, cat) {
        // SQLite 的视图与实体化视图都落在 sqlite_master 的 type='view' 里。
        (Target::Sqlite { .. }, 1) | (Target::Sqlite { .. }, 2) => {
            let sql = format!(
                "SELECT COALESCE(sql, '') FROM sqlite_master \
                 WHERE type = 'view' AND name = {}",
                sql_lit(name)
            );
            read_scalar(tg, &sql, 0)
        }
        // MySQL 视图：`SHOW CREATE VIEW` 第二列是整段 DDL。
        (Target::MySql { .. }, 1) => {
            let sql = format!("SHOW CREATE VIEW {}", tg.quote(name));
            read_scalar(tg, &sql, 1)
        }
        // MySQL 函数 / 存储过程：先试 FUNCTION，空了再试 PROCEDURE（同一名字空间
        // 不会两者兼有，先 FUNCTION 不为空就直接返回）。
        (Target::MySql { .. }, 3) => {
            if let Some(d) = read_scalar(tg, &format!("SHOW CREATE FUNCTION {}", tg.quote(name)), 1) {
                if !d.is_empty() {
                    return Some(d);
                }
            }
            read_scalar(tg, &format!("SHOW CREATE PROCEDURE {}", tg.quote(name)), 1)
        }
        // PostgreSQL 视图 / 实体化视图：`pg_views` / `pg_matviews` 的 definition
        // 只是 SELECT 正文（不带 CREATE 头），这里补上头与新名，改名交给 rename_ddl。
        (Target::Pg { schema, .. }, 1) | (Target::Pg { schema, .. }, 2) => {
            let table = if cat == 2 {
                "pg_matviews"
            } else {
                "pg_views"
            };
            let sql = format!(
                "SELECT COALESCE(definition, '') FROM {table} \
                 WHERE schemaname = {} AND viewname = {}",
                sql_lit(schema),
                sql_lit(name)
            );
            let def = read_scalar(tg, &sql, 0)?;
            if def.trim().is_empty() {
                return None;
            }
            let head = if cat == 2 {
                "CREATE MATERIALIZED VIEW"
            } else {
                "CREATE VIEW"
            };
            Some(format!("{} {} AS\n{def};", head, tg.qualified_table(name)))
        }
        // PostgreSQL 函数：pg_get_functiondef 直接给整段 CREATE OR REPLACE FUNCTION。
        (Target::Pg { schema, .. }, 3) => {
            let sql = format!(
                "SELECT COALESCE(pg_get_functiondef(oid), '') FROM pg_proc \
                 WHERE pronamespace = (SELECT oid FROM pg_namespace WHERE nspname = {}) \
                 AND proname = {} LIMIT 1",
                sql_lit(schema),
                sql_lit(name)
            );
            read_scalar(tg, &sql, 0)
        }
        // SQL Server 视图 / 函数 / 存储过程：OBJECT_DEFINITION 返回整段 CREATE，
        // 名字带 `[schema].[name]` 方括号限定。
        (Target::SqlServer { .. }, 1) | (Target::SqlServer { .. }, 3) => {
            let obj = format!(
                "{}.{}",
                tg.quote(tg.space()),
                tg.quote(name)
            );
            let sql = format!(
                "SELECT COALESCE(OBJECT_DEFINITION(OBJECT_ID({})), '')",
                sql_lit(&obj)
            );
            read_scalar(tg, &sql, 0)
        }
        // 其余引擎 / 分类没有可读取的定义（如 SQLite 上的函数、MySQL 上的实体化视图）。
        _ => None,
    }
}

/// 把 DDL 里第一处对象名（可能带模式前缀）换成目标限定名。
///
/// 按 DDL 内容挑关键字：含 `MATERIALIZED VIEW` / ` VIEW ` → `VIEW`；含 `PROCEDURE`
/// → `PROCEDURE`；含 `FUNCTION` → `FUNCTION`。命中后改写紧跟其后的（可选点号连接的）
/// 限定名为 `tg_dst.qualified_table(newname)`，保留原语句其余部分。
pub fn rename_ddl(ddl: &str, newname: &str, tg_dst: &Target) -> Result<String, String> {
    let upper = ddl.to_ascii_uppercase();
    let kw = if upper.contains("MATERIALIZED VIEW") {
        "VIEW"
    } else if upper.contains(" VIEW ") || upper.contains("CREATE VIEW") {
        "VIEW"
    } else if upper.contains("PROCEDURE") {
        "PROCEDURE"
    } else if upper.contains("FUNCTION") {
        "FUNCTION"
    } else {
        return Err(format!(
            "无法从 DDL 识别对象类型：{}",
            ellipsize(ddl, 60)
        ));
    };
    let kw_at = find_keyword(ddl, kw)
        .ok_or_else(|| format!("DDL 里找不到关键字 {kw}：{}", ellipsize(ddl, 60)))?;
    // 关键字之后的限定名：先一个标识符，紧跟点号再一个标识符（模式.对象）。
    let mut i = skip_ws(ddl, kw_at + kw.len());
    let Some((s, e)) = ident_span(ddl, i) else {
        return Err(format!("DDL 里 {kw} 后面没有对象名：{}", ellipsize(ddl, 60)));
    };
    let mut start = s;
    let mut end = e;
    let bytes = ddl.as_bytes();
    let after = skip_ws(ddl, e);
    if bytes.get(after) == Some(&b'.') {
        let j = skip_ws(ddl, after + 1);
        if let Some((s2, e2)) = ident_span(ddl, j) {
            start = s;
            end = e2;
        }
    }
    // 用「命名空间.新名」重建：MySQL 是 库.表、PG / SQL Server 是 模式.对象，
    // 统一不带「库」前缀（SQL Server 的 DDL 里对象名只到 模式.对象 两级，
    // 多出的库前缀反而会让 CREATE 指向错库甚至语法报警）。
    let replacement = format!("{}{}", tg_dst.qualified_prefix(), tg_dst.quote(newname));
    Ok(format!("{}{}{}", &ddl[..start], replacement, &ddl[end..]))
}

/// 执行一批对象复制：逐个读源定义、改目标名、在目标库建副本。返回每张源的
/// 结果（顺序与 `items` 一致）。整段可在后台线程跑（[Target] 是 Send）。
pub fn run_copies(
    src: &Target,
    dst: &Target,
    items: &[(String, String)],
    cat: usize,
) -> Vec<(String, Result<(), String>)> {
    items
        .iter()
        .map(|(source, target)| {
            let res = fetch_ddl(src, cat, source)
                .ok_or_else(|| {
                    format!("读不到「{source}」的定义（该引擎 / 分类暂不支持复制）")
                })
                .and_then(|ddl| rename_ddl(&ddl, target, dst))
                .and_then(|stmt| dst.execute(&stmt, 1).map(|_| ()));
            (source.clone(), res)
        })
        .collect()
}

/// 执行一条查询并把指定列的标量取回来（取不到 / 空 / 出错都返回 None）。
fn read_scalar(tg: &Target, sql: &str, col: usize) -> Option<String> {
    let out = tg.execute(sql, 10).ok()?;
    out.select
        .and_then(|p| p.rows.into_iter().next())
        .and_then(|r| r.into_iter().nth(col))
        .filter(|s| !s.trim().is_empty())
}

fn sql_lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn ellipsize(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
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

/// 解析一个（可能带引号 / 方括号的）标识符，返回其在 `sql` 中的起止下标。
fn ident_span(sql: &str, i: usize) -> Option<(usize, usize)> {
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
                        continue;
                    }
                    return Some((i, j + 1));
                }
                j += 1;
            }
            None
        }
        b'[' => {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b']' {
                j += 1;
            }
            (j < bytes.len()).then_some((i, j + 1))
        }
        _ => {
            let mut j = i;
            while j < bytes.len()
                && !bytes[j].is_ascii_whitespace()
                && !matches!(bytes[j], b'(' | b')' | b',' | b';' | b'.')
            {
                j += 1;
            }
            (j > i).then_some((i, j))
        }
    }
}

fn skip_ws(sql: &str, mut i: usize) -> usize {
    let bytes = sql.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// 该连接是否支持把这一分类的对象粘成副本（用于菜单置灰判定）。
pub fn object_copy_ready(conn: &crate::mock::Connection, cat: usize) -> bool {
    match cat {
        // 查询是本地保存的 SQL 片段，不依赖真库连接。
        4 => true,
        // 视图 / 实体化视图 / 函数：要读定义再重建，需要真库连接且引擎有这条路。
        1 | 2 | 3 => {
            crate::backend::is_real_conn(conn)
                && !crate::backend::is_read_only(conn.kind)
                && matches!(
                    conn.kind,
                    DbKind::SQLite | DbKind::PostgreSql | DbKind::MySql | DbKind::SqlServer
                )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_view_ddl_gets_renamed() {
        let tg = Target::MySql {
            conn: "c".into(),
            db: "shop".into(),
            params: crate::mysql_driver::Params {
                host: "h".into(),
                port: 0,
                user: "u".into(),
                password: "".into(),
                dbname: "shop".into(),
            },
        };
        let ddl = "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`%` SQL SECURITY DEFINER \
                   VIEW `v_orders` AS select `id` from `orders`";
        let renamed = rename_ddl(ddl, "v_orders_copy", &tg).unwrap();
        assert!(
            renamed.contains("VIEW `shop`.`v_orders_copy` AS"),
            "{renamed}"
        );
        assert!(renamed.contains("from `orders`"), "正文里的表名不能被误改");
    }

    #[test]
    fn pg_materialized_view_ddl_gets_renamed_with_schema() {
        let tg = Target::Pg {
            conn: "c".into(),
            db: "shop".into(),
            schema: "crm".into(),
            params: crate::pg_driver::Params {
                host: "h".into(),
                port: 0,
                user: "u".into(),
                password: "".into(),
                dbname: "shop".into(),
            },
        };
        let ddl = "CREATE MATERIALIZED VIEW \"crm\".\"mv\" AS SELECT 1";
        let renamed = rename_ddl(ddl, "mv_copy", &tg).unwrap();
        assert_eq!(
            renamed,
            "CREATE MATERIALIZED VIEW \"crm\".\"mv_copy\" AS SELECT 1"
        );
    }

    #[test]
    fn pg_function_ddl_gets_renamed_keeping_body() {
        let tg = Target::Pg {
            conn: "c".into(),
            db: "shop".into(),
            schema: "public".into(),
            params: crate::pg_driver::Params {
                host: "h".into(),
                port: 0,
                user: "u".into(),
                password: "".into(),
                dbname: "shop".into(),
            },
        };
        let ddl = "CREATE OR REPLACE FUNCTION \"public\".\"add2\"(a integer) \
                   RETURNS integer LANGUAGE sql AS $fn$ SELECT a + 2 $fn$";
        let renamed = rename_ddl(ddl, "add2_copy", &tg).unwrap();
        assert!(
            renamed.starts_with("CREATE OR REPLACE FUNCTION \"public\".\"add2_copy\"("),
            "{renamed}"
        );
        assert!(renamed.contains("SELECT a + 2"), "函数体不应被改动");
    }

    #[test]
    fn sqlserver_procedure_ddl_gets_renamed() {
        let tg = Target::SqlServer {
            conn: "c".into(),
            db: "shop".into(),
            schema: "dbo".into(),
            params: crate::sqlserver_driver::Params {
                host: "h".into(),
                port: 0,
                user: "u".into(),
                password: "".into(),
                dbname: "shop".into(),
            },
        };
        let ddl = "CREATE PROCEDURE [dbo].[sp_x] @a int AS SELECT @a";
        let renamed = rename_ddl(ddl, "sp_x_copy", &tg).unwrap();
        assert!(
            renamed.starts_with("CREATE PROCEDURE [dbo].[sp_x_copy]"),
            "{renamed}"
        );
    }

    #[test]
    fn ddl_without_recognizable_object_is_rejected() {
        let tg = Target::MySql {
            conn: "c".into(),
            db: "shop".into(),
            params: crate::mysql_driver::Params {
                host: "h".into(),
                port: 0,
                user: "u".into(),
                password: "".into(),
                dbname: "shop".into(),
            },
        };
        assert!(rename_ddl("SELECT 1", "x", &tg).is_err());
    }
}
