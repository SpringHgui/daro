//! 角色 / 用户设计器的逻辑层：`pg_roles` 反查、行解析、`ALTER ROLE` 拼装，
//! 加上「成员属于 / 成员」两页的 `pg_auth_members` 反查与 `GRANT/REVOKE` 拼装。
//!
//! 口径来自参考界面的 PG 角色设计器截图（见 MIGRATION.md「角色设计器」）：
//! MySQL 那边叫「用户」、PG 这边叫「角色」，同一套界面，字段按 PG 的十个属性铺。
//! 口令从不回填，只把用户这次新输的那条带进 `ALTER ROLE`。

use crate::pg_driver::quote_ident;
use crate::sql_literal;

/// 设计器里一个角色的工作集。布尔位与 `pg_roles` 的列一一对应，数字与日期
/// 按界面原样存字符串（`-1` / 空串都是合法值，不做数值化）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoleModel {
    pub name: String,
    /// 角色 ID：目录里的 OID，只读展示
    pub oid: String,
    pub can_login: bool,
    pub superuser: bool,
    pub create_db: bool,
    pub create_role: bool,
    pub inherit: bool,
    pub replication: bool,
    pub bypass_rls: bool,
    /// 连接限制，`-1` = 不限
    pub conn_limit: String,
    /// 过期日期，空 = 永不过期
    pub valid_until: String,
    /// 现有口令的哈希类型（md5 / scram-sha-256），非超级用户读不到时为空
    pub pw_encryption: String,
    /// 用户这次新输的口令；两次输入一致才放行
    pub password: String,
    pub comment: String,
}

impl RoleModel {
    /// 演示档种子：照截图那条 `pg_read_all_data`（OID 6181，七个权限位里只有
    /// 继承权限为勾）。注意真库不一定这么答——PG 各版本给预定义角色的
    /// 继承位不同，所以「真连反查」那条只照目录说。
    pub fn demo_predefined(name: &str, oid: &str) -> Self {
        Self {
            name: name.to_string(),
            oid: oid.to_string(),
            inherit: true,
            conn_limit: "-1".to_string(),
            ..Default::default()
        }
    }

    /// 新建角色的初值：PG 的 `CREATE ROLE` 默认全是 NO*，界面上只有
    /// 继承权限按参考界面给成勾着。
    pub fn demo_new() -> Self {
        Self {
            inherit: true,
            conn_limit: "-1".to_string(),
            ..Default::default()
        }
    }
}

/// 反查一条角色的全部常规属性。`rolvaliduntil` 的 `infinity` 与 NULL 都解成
/// 空串——界面上「留空 = 永不过期」，写回时再补 `VALID UNTIL 'infinity'`。
pub fn role_meta_sql(name: &str) -> String {
    let lit = sql_literal(name);
    format!(
        "SELECT r.oid::text, r.rolcanlogin::text, r.rolsuper::text, \
                r.rolcreatedb::text, r.rolcreaterole::text, r.rolinherit::text, \
                r.rolreplication::text, r.rolbypassrls::text, r.rolconnlimit::text, \
                CASE WHEN r.rolvaliduntil IS NULL OR r.rolvaliduntil = 'infinity' \
                     THEN '' ELSE to_char(r.rolvaliduntil, 'YYYY-MM-DD HH24:MI:SS') END, \
                COALESCE(d.description, '') \
         FROM pg_roles r \
         LEFT JOIN pg_description d \
                ON d.objoid = r.oid \
               AND d.classoid = 'pg_catalog.pg_authid'::regclass \
               AND d.objsubid = 0 \
         WHERE r.rolname = '{lit}'"
    )
}

/// 口令哈希类型要读 `pg_authid`，PG 16 起非超级用户没权限——读不到就当空，
/// 下拉留白而不是编一个值。
pub fn pw_encryption_sql(name: &str) -> String {
    let lit = sql_literal(name);
    format!(
        "SELECT CASE \
                WHEN a.rolpassword IS NULL THEN '' \
                WHEN a.rolpassword LIKE 'SCRAM-SHA-256$%' \
                  OR a.rolpassword LIKE 'SCRAM-SHA-256-%' THEN 'scram-sha-256' \
                WHEN a.rolpassword LIKE 'md5%' THEN 'md5' \
                ELSE '其他' END \
         FROM pg_authid a WHERE a.rolname = '{lit}'"
    )
}

/// 反查行 → 模型；列数不足（目录查询被截断 / 换了版本）返回 None。
pub fn parse_role_row(r: &[String]) -> Option<RoleModel> {
    if r.len() < 11 {
        return None;
    }
    let flag = |i: usize| r[i].as_str() == "t";
    Some(RoleModel {
        oid: r[0].clone(),
        can_login: flag(1),
        superuser: flag(2),
        create_db: flag(3),
        create_role: flag(4),
        inherit: flag(5),
        replication: flag(6),
        bypass_rls: flag(7),
        conn_limit: r[8].clone(),
        valid_until: r[9].clone(),
        comment: r[10].clone(),
        ..Default::default()
    })
}

/// 保存前的校验：拦在保存按钮上，与表设计器同一口径。`confirm` 是确认框的
/// 原文，它不进模型，所以在这里单独比一次。连接限制留空当「不限」，
/// 与 [limit_sql] 的兜底同口径。
pub fn validate(now: &RoleModel, confirm: &str) -> Option<&'static str> {
    if now.name.trim().is_empty() {
        return Some("角色名称不能为空");
    }
    if !now.password.is_empty() && now.password != confirm {
        return Some("两次输入的密码不一致");
    }
    if !now.conn_limit.trim().is_empty() && limit_number(&now.conn_limit).is_none() {
        return Some("连接限制要填整数");
    }
    None
}

/// 改口令时参考界面先选哈希算法：PG 的开关是 `password_encryption`，
/// 必须排在同一条会话里 `PASSWORD` 之前才作用到这条口令上。
fn pw_encryption_stmt(enc: &str) -> Option<String> {
    (!enc.is_empty()).then(|| format!("SET password_encryption = '{}'", sql_literal(enc)))
}

/// 十一个布尔 / 数值属性里动了哪些，就拼进同一条 `ALTER ROLE ... WITH`；
/// 名称、注释各自单独一条（PG 没有把它们塞进 WITH 的语法）。
pub fn alter_statements(base: &RoleModel, now: &RoleModel) -> Vec<String> {
    let mut out = Vec::new();
    if !base.name.is_empty() && base.name != now.name && !now.name.is_empty() {
        out.push(format!(
            "ALTER ROLE {} RENAME TO {}",
            quote_ident(&base.name),
            quote_ident(&now.name)
        ));
    }
    let target = if now.name.is_empty() { &base.name } else { &now.name };
    let mut with: Vec<String> = Vec::new();
    let mut flag = |on: bool, was: bool, up: &str, down: &str| {
        if on != was {
            with.push(if on { up.to_string() } else { down.to_string() });
        }
    };
    flag(now.can_login, base.can_login, "LOGIN", "NOLOGIN");
    flag(now.superuser, base.superuser, "SUPERUSER", "NOSUPERUSER");
    flag(now.create_db, base.create_db, "CREATEDB", "NOCREATEDB");
    flag(now.create_role, base.create_role, "CREATEROLE", "NOCREATEROLE");
    flag(now.inherit, base.inherit, "INHERIT", "NOINHERIT");
    flag(now.replication, base.replication, "REPLICATION", "NOREPLICATION");
    flag(now.bypass_rls, base.bypass_rls, "BYPASSRLS", "NOBYPASSRLS");
    if now.conn_limit != base.conn_limit {
        with.push(format!("CONNECTION LIMIT {}", limit_sql(&now.conn_limit)));
    }
    if now.valid_until != base.valid_until {
        with.push(format!("VALID UNTIL '{}'", sql_literal(&until_sql(&now.valid_until))));
    }
    if !now.password.is_empty() {
        if let Some(s) = pw_encryption_stmt(&now.pw_encryption) {
            out.push(s);
        }
        with.push(format!("PASSWORD '{}'", sql_literal(&now.password)));
    }
    if !with.is_empty() {
        out.push(format!("ALTER ROLE {} WITH {}", quote_ident(target), with.join(" ")));
    }
    if base.comment != now.comment {
        let v = if now.comment.is_empty() {
            "NULL".to_string()
        } else {
            format!("'{}'", sql_literal(&now.comment))
        };
        out.push(format!("COMMENT ON ROLE {} IS {}", quote_ident(target), v));
    }
    out
}

/// 新建角色：属性全量写。
pub fn create_statements(now: &RoleModel) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut with: Vec<String> = Vec::new();
    let mut flag = |on: bool, up: &str| {
        if on {
            with.push(up.to_string());
        }
    };
    flag(now.can_login, "LOGIN");
    flag(now.superuser, "SUPERUSER");
    flag(now.create_db, "CREATEDB");
    flag(now.create_role, "CREATEROLE");
    flag(now.inherit, "INHERIT");
    flag(now.replication, "REPLICATION");
    flag(now.bypass_rls, "BYPASSRLS");
    with.push(format!("CONNECTION LIMIT {}", limit_sql(&now.conn_limit)));
    with.push(format!("VALID UNTIL '{}'", sql_literal(&until_sql(&now.valid_until))));
    if !now.password.is_empty() {
        out.extend(pw_encryption_stmt(&now.pw_encryption));
        with.push(format!("PASSWORD '{}'", sql_literal(&now.password)));
    }
    out.push(format!(
        "CREATE ROLE {} WITH {}",
        quote_ident(&now.name),
        with.join(" ")
    ));
    if !now.comment.is_empty() {
        out.push(format!(
            "COMMENT ON ROLE {} IS '{}'",
            quote_ident(&now.name),
            sql_literal(&now.comment)
        ));
    }
    out
}

/// 「已修改」判定：口令是一次性输入，填了又清空不该让保存按钮一直亮着，
/// 所以只在「这次填了口令」时算改动，其余按属性集比。
/// 加密方式同理——它不改目录，只决定新口令怎么存，没填口令时不算改动。
pub fn is_dirty(base: &RoleModel, now: &RoleModel) -> bool {
    if !now.password.is_empty() {
        return true;
    }
    let mut clean = now.clone();
    clean.pw_encryption = base.pw_encryption.clone();
    let mut was = base.clone();
    was.password.clear();
    clean != was
}

/// 连接限制认整数（可带负号），空按不限。
fn limit_number(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let body = t.strip_prefix('-').unwrap_or(t);
    (!body.is_empty() && body.chars().all(|c| c.is_ascii_digit())).then(|| t.to_string())
}

fn limit_sql(s: &str) -> String {
    limit_number(s).unwrap_or_else(|| "-1".to_string())
}

/// 过期日期空 = 永不过期，PG 的写法是 `'infinity'`。
fn until_sql(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        "infinity".to_string()
    } else {
        t.to_string()
    }
}

/// 「成员属于」/「成员」网格里的一行：集群里的一个角色 + 两个勾选。
///
/// 口径照参考界面：两页都列出**全部**角色（不是只列有关系的那些），
/// 勾选表示关系是否存在，所以取消勾选就是发 REVOKE。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemberRow {
    pub name: String,
    /// 「授予」列：这层关系在不在
    pub granted: bool,
    /// 「管理选项」列：`WITH ADMIN OPTION`
    pub admin: bool,
}

/// 成员关系的两个方向。PG 的目录只记「member 属于 roleid」这一条边，
/// 两页是同一张表正着查和反着查。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberDir {
    /// 列出当前角色的父角色（`GRANT 列表项 TO 当前角色`）
    MemberOf,
    /// 列出当前角色的子角色（`GRANT 当前角色 TO 列表项`）
    Members,
}

/// `pg_auth_members` 记「这一层关系能不能再授给别人」的那一列，PG16 起叫
/// `admin_option`，之前叫 `admoption`。SQL 是解析期就定死列名的，写任何一个
/// 都会在另一台服务器上直接报「列不存在」，所以由调用方先从目录问出来再传进来。
pub const ADMIN_COL_PROBE: &str = "SELECT attname \
                                   FROM pg_attribute \
                                   WHERE attrelid = 'pg_catalog.pg_auth_members'::regclass \
                                     AND attname IN ('admin_option', 'admoption')";

/// 探测结果 → 列名。认不出的形态一律按 PG16 的写法兜底：真取不到列，报错也是
/// 「列不存在」这种看得懂的话，比这里悄悄换一列去查更诚实。
pub fn admin_col_from_probe(rows: &[Vec<String>]) -> &'static str {
    match rows.first().and_then(|r| r.first()).map(|s| s.as_str()) {
        Some("admoption") => "admoption",
        _ => "admin_option",
    }
}

/// 反查全部角色及其与当前角色的关系。用「列出全部角色 + 两条 EXISTS」而不是
/// 先查关系再补名字，是为了让「没有任何成员关系」时仍然列出全部角色——界面
/// 要能往里勾；EXISTS 而不是 JOIN 是因为 PG16 起同一对角色可以按不同授权人
/// 有多条边，JOIN 会把一行摊成多行。
/// 当前角色自己排除在外：截图那 16 行就是全集群减去 `pg_read_all_data`，
/// PG 也不允许 `GRANT x TO x`。
pub fn member_sql(dir: MemberDir, name: &str, admin_col: &str) -> String {
    let lit = sql_literal(name);
    // 当前角色固定站在哪一侧：MemberOf 时它是 member（列表项是父），
    // Members 时它是 roleid（列表项是子）。
    let (self_col, other_col) = match dir {
        MemberDir::MemberOf => ("member", "roleid"),
        MemberDir::Members => ("roleid", "member"),
    };
    let edge = format!(
        "m.{self_col} = (SELECT oid FROM pg_roles WHERE rolname = '{lit}') \
                AND m.{other_col} = r.oid"
    );
    format!(
        "SELECT r.rolname, \
                EXISTS (SELECT 1 FROM pg_auth_members m WHERE {edge})::text, \
                EXISTS (SELECT 1 FROM pg_auth_members m \
                         WHERE {edge} AND m.{admin_col})::text \
         FROM pg_roles r \
         WHERE r.rolname <> '{lit}' \
         ORDER BY r.rolname"
    )
}

/// 目录行 → 网格行；列数不足的行直接跳过（宁可少一行，不编一行）。
pub fn parse_member_rows(rows: &[Vec<String>]) -> Vec<MemberRow> {
    rows.iter()
        .filter(|r| r.len() >= 3)
        .map(|r| MemberRow {
            name: r[0].clone(),
            granted: r[1].as_str() == "t",
            admin: r[2].as_str() == "t",
        })
        .collect()
}

/// 演示档：全部列出、全部不勾——和截图里 `pg_read_all_data` 的样子一致，
/// 不替目录编关系。
pub fn demo_member_rows(names: &[&str]) -> Vec<MemberRow> {
    names
        .iter()
        .map(|n| MemberRow {
            name: n.to_string(),
            ..Default::default()
        })
        .collect()
}

/// 两行列表按名字对齐后比勾选，供「已修改」判定用。
fn membership_set(rows: &[MemberRow]) -> std::collections::BTreeSet<(&str, bool, bool)> {
    rows.iter()
        .map(|r| (r.name.as_str(), r.granted, r.admin))
        .collect()
}

pub fn membership_dirty(base: &[MemberRow], now: &[MemberRow]) -> bool {
    membership_set(base) != membership_set(now)
}

fn find_member<'a>(rows: &'a [MemberRow], name: &str) -> Option<&'a MemberRow> {
    rows.iter().find(|r| r.name == name)
}

/// 成员差异 → GRANT / REVOKE。先撤后授：加一条边可能要求先有别的边，
/// 反过来撤不会把还没建的关系牵连进去。
pub fn membership_statements(
    target: &str,
    dir: MemberDir,
    base: &[MemberRow],
    now: &[MemberRow],
) -> Vec<String> {
    if target.is_empty() {
        return Vec::new();
    }
    let mut grants = Vec::new();
    let mut revokes = Vec::new();
    for r in now {
        let was = find_member(base, &r.name).cloned().unwrap_or_default();
        // (被授的角色, 被授予的角色)：MemberOf 时列表项是父，Members 时列表项是子
        let (parent, member) = match dir {
            MemberDir::MemberOf => (quote_ident(&r.name), quote_ident(target)),
            MemberDir::Members => (quote_ident(target), quote_ident(&r.name)),
        };
        if !was.granted && r.granted {
            grants.push(if r.admin {
                format!("GRANT {parent} TO {member} WITH ADMIN OPTION")
            } else {
                format!("GRANT {parent} TO {member}")
            });
        } else if was.granted && !r.granted {
            revokes.push(format!("REVOKE {parent} FROM {member}"));
        } else if was.granted && r.granted && was.admin != r.admin {
            // 只动管理选项：关系还在，撤的 / 补的是 ADMIN OPTION 这一层
            if r.admin {
                grants.push(format!("GRANT {parent} TO {member} WITH ADMIN OPTION"));
            } else {
                revokes.push(format!("REVOKE ADMIN OPTION FOR {parent} FROM {member}"));
            }
        }
    }
    // 界面上被删掉的行（目录里有、工作集没有）按取消勾选处理
    for w in base {
        if find_member(now, &w.name).is_none() && w.granted {
            let (parent, member) = match dir {
                MemberDir::MemberOf => (quote_ident(&w.name), quote_ident(target)),
                MemberDir::Members => (quote_ident(target), quote_ident(&w.name)),
            };
            revokes.push(format!("REVOKE {parent} FROM {member}"));
        }
    }
    revokes.extend(grants);
    revokes
}

/// 当前成员关系的全集（不是差异）：DDL 两栏要把这个角色「长什么样」完整重建
/// 一遍，得连已经存在的边一起铺。
fn grant_lines(target: &str, dir: MemberDir, rows: &[MemberRow]) -> Vec<String> {
    rows.iter()
        .filter(|r| r.granted)
        .map(|r| {
            let (parent, member) = match dir {
                MemberDir::MemberOf => (quote_ident(&r.name), quote_ident(target)),
                MemberDir::Members => (quote_ident(target), quote_ident(&r.name)),
            };
            if r.admin {
                format!("GRANT {parent} TO {member} WITH ADMIN OPTION")
            } else {
                format!("GRANT {parent} TO {member}")
            }
        })
        .collect()
}

/// 「SQL 预览 → DDL」两栏的正文：属性 + 注释 + 两页成员边，从同一套拼装器重建
/// 一份完整定义。基线与工作集各出一份才有共同口径——与表 / 视图设计器同一路线，
/// 正文不是从库里抄的原文，是按当前字段重拼的。名字还没填就返回空串，
/// 别拼半条 `CREATE ROLE  WITH …` 出去。
pub fn ddl_script(model: &RoleModel, member_of: &[MemberRow], members: &[MemberRow]) -> String {
    if model.name.is_empty() {
        return String::new();
    }
    let mut out = create_statements(model).join("\n");
    let edges: Vec<String> = grant_lines(&model.name, MemberDir::MemberOf, member_of)
        .into_iter()
        .chain(grant_lines(&model.name, MemberDir::Members, members))
        .collect();
    for stmt in edges {
        out.push('\n');
        out.push_str(&stmt);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mrow(name: &str, granted: bool, admin: bool) -> MemberRow {
        MemberRow {
            name: name.to_string(),
            granted,
            admin,
        }
    }

    #[test]
    fn member_sql_lists_every_role_in_both_directions() {
        let up = member_sql(MemberDir::MemberOf, "pg_read_all_data", "admin_option");
        let down = member_sql(MemberDir::Members, "pg_read_all_data", "admin_option");
        // 列全部角色靠 pg_roles 打底，关系侧两条 EXISTS——PG16 起同一对角色
        // 可以按不同授权人有多条边，JOIN 会把一行摊成多行
        assert!(up.contains("FROM pg_roles r"));
        assert_eq!(up.matches("EXISTS (SELECT 1 FROM pg_auth_members m").count(), 2);
        assert!(up.contains("m.member = (SELECT oid FROM pg_roles WHERE rolname = 'pg_read_all_data')"));
        assert!(up.contains("AND m.roleid = r.oid"));
        assert!(down.contains("m.roleid = (SELECT oid FROM pg_roles WHERE rolname = 'pg_read_all_data')"));
        assert!(down.contains("AND m.member = r.oid"));
        // 自己不出现在自己的成员表里（截图那 16 行就是全集群减去当前角色）
        assert!(up.contains("WHERE r.rolname <> 'pg_read_all_data'"));
        assert!(up.ends_with("ORDER BY r.rolname"));
    }

    #[test]
    fn member_sql_follows_the_admin_option_column_name() {
        // PG15 及以前那台只认 admoption，写死 admin_option 会「列不存在」
        let old = member_sql(MemberDir::MemberOf, "app", "admoption");
        assert!(old.contains("AND m.admoption"));
        assert!(!old.contains("admin_option"));
    }

    #[test]
    fn admin_col_probe_maps_only_the_two_known_spellings() {
        assert_eq!(
            admin_col_from_probe(&[vec!["admoption".to_string()]]),
            "admoption"
        );
        assert_eq!(
            admin_col_from_probe(&[vec!["admin_option".to_string()]]),
            "admin_option"
        );
        // 问不出东西（无权限、目录异常）就按 PG16 的写法兜底，让报错说实话
        assert_eq!(admin_col_from_probe(&[]), "admin_option");
        assert_eq!(
            admin_col_from_probe(&[vec!["别的".to_string()]]),
            "admin_option"
        );
        assert!(ADMIN_COL_PROBE.contains("'admin_option', 'admoption'"));
    }

    #[test]
    fn member_sql_escapes_the_role_name() {
        let s = member_sql(MemberDir::MemberOf, "it's", "admin_option");
        assert!(s.contains("'it''s'"), "名字里的单引号要转义：{s}");
    }

    #[test]
    fn parse_member_rows_skips_short_rows() {
        let rows = vec![
            vec!["pg_monitor".into(), "t".into(), "f".into()],
            vec!["半行".into()],
            vec!["postgres".into(), "f".into(), "t".into()],
        ];
        let got = parse_member_rows(&rows);
        assert_eq!(
            got,
            vec![mrow("pg_monitor", true, false), mrow("postgres", false, true)]
        );
    }

    #[test]
    fn membership_grant_reads_parent_to_child() {
        let base = vec![mrow("pg_monitor", false, false)];
        let now = vec![mrow("pg_monitor", true, false)];
        assert_eq!(
            membership_statements("daro_app", MemberDir::MemberOf, &base, &now),
            vec!["GRANT \"pg_monitor\" TO \"daro_app\"".to_string()]
        );
        // 反方向：当前角色站在父位
        assert_eq!(
            membership_statements("daro_app", MemberDir::Members, &base, &now),
            vec!["GRANT \"daro_app\" TO \"pg_monitor\"".to_string()]
        );
    }

    #[test]
    fn membership_admin_option_has_its_own_pair_of_statements() {
        let base = vec![mrow("pg_monitor", true, false)];
        let on = vec![mrow("pg_monitor", true, true)];
        assert_eq!(
            membership_statements("daro_app", MemberDir::MemberOf, &base, &on),
            vec!["GRANT \"pg_monitor\" TO \"daro_app\" WITH ADMIN OPTION".to_string()]
        );
        assert_eq!(
            membership_statements("daro_app", MemberDir::MemberOf, &on, &base),
            vec!["REVOKE ADMIN OPTION FOR \"pg_monitor\" FROM \"daro_app\"".to_string()]
        );
        // 勾掉授予 = 整条关系撤掉，不带 ADMIN OPTION 前缀
        let off = vec![mrow("pg_monitor", false, false)];
        assert_eq!(
            membership_statements("daro_app", MemberDir::MemberOf, &on, &off),
            vec!["REVOKE \"pg_monitor\" FROM \"daro_app\"".to_string()]
        );
    }

    #[test]
    fn membership_revokes_run_before_grants() {
        let base = vec![mrow("pg_monitor", true, false), mrow("postgres", false, false)];
        let now = vec![mrow("pg_monitor", false, false), mrow("postgres", true, false)];
        let got = membership_statements("daro_app", MemberDir::MemberOf, &base, &now);
        assert_eq!(
            got,
            vec![
                "REVOKE \"pg_monitor\" FROM \"daro_app\"".to_string(),
                "GRANT \"postgres\" TO \"daro_app\"".to_string(),
            ]
        );
        // 没动 = 空脚本
        assert!(membership_statements("daro_app", MemberDir::MemberOf, &now, &now).is_empty());
        // 名字还没落库的新角色不拼语句
        assert!(membership_statements("", MemberDir::MemberOf, &base, &now).is_empty());
    }

    #[test]
    fn membership_dirty_compares_by_name_not_order() {
        let a = vec![mrow("x", true, false), mrow("y", false, false)];
        let b = vec![mrow("y", false, false), mrow("x", true, false)];
        assert!(!membership_dirty(&a, &b), "同一组关系换个顺序不算改动");
        assert!(membership_dirty(&a, &[mrow("x", true, true), mrow("y", false, false)]));
    }

    /// 截图里那条 `pg_read_all_data`：只勾「继承权限」，连接限制 -1，无口令。
    fn base() -> RoleModel {
        RoleModel::demo_predefined("pg_read_all_data", "6181")
    }

    fn row(oid: &str, flags: &str, limit: &str, until: &str, comment: &str) -> Vec<String> {
        let mut r = vec![oid.to_string()];
        r.extend(flags.chars().map(|c| c.to_string()));
        r.extend([limit.to_string(), until.to_string(), comment.to_string()]);
        r
    }

    #[test]
    fn meta_sql_filters_by_name_and_blanks_infinity() {
        let sql = role_meta_sql("pg_read_all_data");
        assert!(sql.ends_with("WHERE r.rolname = 'pg_read_all_data'"), "{sql}");
        // infinity 与 NULL 都折成空串：界面上留空就是永不过期
        assert!(sql.contains("WHEN r.rolvaliduntil IS NULL OR r.rolvaliduntil = 'infinity' THEN ''"));
        assert!(sql.contains("COALESCE(d.description, '')"));
        // 名字里的单引号双写，别把查询拼断
        assert!(role_meta_sql("o'brien").contains("= 'o''brien'"));
        // 真机踩过一次：继承位在 pg_roles 里叫 rolinherit，不是 rolcaninherit
        assert!(sql.contains("r.rolinherit::text"));
        assert!(!sql.contains("rolcaninherit"));
    }

    #[test]
    fn password_hash_probe_falls_back_to_labels() {
        let sql = pw_encryption_sql("app");
        assert!(sql.contains("WHEN a.rolpassword IS NULL THEN ''"));
        assert!(sql.contains("'scram-sha-256'"));
        assert!(sql.contains("ELSE '其他'"));
    }

    #[test]
    fn parse_row_maps_bools_and_keeps_display_strings() {
        let m = parse_role_row(&row("42", "tftftff", "5", "2030-01-01 00:00:00", "只读账号"))
            .expect("十一列应解析成功");
        assert_eq!(m.oid, "42");
        assert!(m.can_login && !m.superuser && m.create_db && !m.create_role);
        assert!(m.inherit && !m.replication && !m.bypass_rls);
        assert_eq!(m.conn_limit, "5");
        assert_eq!(m.valid_until, "2030-01-01 00:00:00");
        assert_eq!(m.comment, "只读账号");
        // 截断的行宁可当没读到，也不把缺的列当 false 铺出去
        assert!(parse_role_row(&row("42", "tftftf", "5", "", "")).is_none());
    }

    #[test]
    fn validate_blocks_name_password_and_limit() {
        let mut empty = base();
        empty.name = "  ".to_string();
        assert_eq!(validate(&empty, ""), Some("角色名称不能为空"));

        let mut pw = base();
        pw.password = "s3cret".to_string();
        assert_eq!(validate(&pw, "s3cret2"), Some("两次输入的密码不一致"));
        // 口令两遍一致就放行
        assert_eq!(validate(&pw, "s3cret"), None);

        let mut bad = base();
        bad.conn_limit = "unlimited".to_string();
        assert_eq!(validate(&bad, ""), Some("连接限制要填整数"));
        // 空 = 不限，跟 -1 一样放行
        bad.conn_limit = "".to_string();
        assert_eq!(validate(&bad, ""), None);
    }

    #[test]
    fn alter_only_carries_what_changed() {
        let mut now = base();
        now.inherit = false;
        now.conn_limit = "5".to_string();
        assert_eq!(
            alter_statements(&base(), &now),
            vec!["ALTER ROLE \"pg_read_all_data\" WITH NOINHERIT CONNECTION LIMIT 5".to_string()]
        );
        // 什么都没动 = 空脚本
        assert!(alter_statements(&base(), &base()).is_empty());
    }

    #[test]
    fn rename_and_comment_are_their_own_statements() {
        let mut now = base();
        now.name = "pg_read_all".to_string();
        now.comment = "读数".to_string();
        assert_eq!(
            alter_statements(&base(), &now),
            vec![
                "ALTER ROLE \"pg_read_all_data\" RENAME TO \"pg_read_all\"".to_string(),
                "COMMENT ON ROLE \"pg_read_all\" IS '读数'".to_string()
            ]
        );
        // 注释清掉 = IS NULL（不是空串）
        now.comment = String::new();
        let base_commented = RoleModel {
            comment: "读数".to_string(),
            ..base()
        };
        assert_eq!(
            alter_statements(&base_commented, &now)[1],
            "COMMENT ON ROLE \"pg_read_all\" IS NULL"
        );
    }

    #[test]
    fn password_change_writes_the_encryption_switch_first() {
        let mut now = base();
        now.password = "p'w".to_string();
        now.pw_encryption = "scram-sha-256".to_string();
        assert_eq!(
            alter_statements(&base(), &now),
            vec![
                "SET password_encryption = 'scram-sha-256'".to_string(),
                "ALTER ROLE \"pg_read_all_data\" WITH PASSWORD 'p''w'".to_string()
            ]
        );
        // 读不到哈希类型时不编：只发 PASSWORD
        now.pw_encryption = String::new();
        assert_eq!(
            alter_statements(&base(), &now),
            vec!["ALTER ROLE \"pg_read_all_data\" WITH PASSWORD 'p''w'".to_string()]
        );
    }

    #[test]
    fn blank_valid_until_writes_infinity() {
        let mut now = base();
        now.valid_until = "2030-01-01 00:00:00".to_string();
        assert_eq!(
            alter_statements(&base(), &now)[0],
            "ALTER ROLE \"pg_read_all_data\" WITH VALID UNTIL '2030-01-01 00:00:00'"
        );
        // 反过来清成永不过期
        let was = now.clone();
        now.valid_until = String::new();
        assert_eq!(
            alter_statements(&was, &now)[0],
            "ALTER ROLE \"pg_read_all_data\" WITH VALID UNTIL 'infinity'"
        );
    }

    #[test]
    fn create_writes_the_whole_attribute_set() {
        let mut now = RoleModel::demo_new();
        now.name = "daro_app".to_string();
        now.can_login = true;
        now.comment = "应用账号".to_string();
        assert_eq!(
            create_statements(&now),
            vec![
                "CREATE ROLE \"daro_app\" WITH LOGIN INHERIT CONNECTION LIMIT -1 VALID UNTIL 'infinity'".to_string(),
                "COMMENT ON ROLE \"daro_app\" IS '应用账号'".to_string()
            ]
        );
        // 口令 + 加密档位在 CREATE 之前也要先设好
        now.password = "pw".to_string();
        now.pw_encryption = "md5".to_string();
        assert_eq!(create_statements(&now)[0], "SET password_encryption = 'md5'");
        assert!(create_statements(&now)[1].contains("PASSWORD 'pw'"));
    }

    #[test]
    fn dirty_ignores_one_shot_password_fields() {
        // 填了又清空：不该一直亮着保存
        let mut now = base();
        now.password = String::new();
        assert!(!is_dirty(&base(), &now));
        now.password = "pw".to_string();
        assert!(is_dirty(&base(), &now), "有新口令就是改动");
        // 没填口令时换加密档位不算改动（它不改目录）
        let mut switched = base();
        switched.pw_encryption = "md5".to_string();
        assert!(!is_dirty(&base(), &switched));
        // 真改了属性还是要算
        switched.inherit = false;
        assert!(is_dirty(&base(), &switched));
        // OID 只读，但目录值本身变了一样得亮（改名不算脏？改的是 name，照亮）
        let mut reoid = base();
        reoid.oid = "999".to_string();
        assert!(is_dirty(&base(), &reoid));
    }

    #[test]
    fn ddl_script_rebuilds_attributes_and_both_membership_edges() {
        let m = RoleModel {
            name: "daro_app".to_string(),
            can_login: true,
            ..Default::default()
        };
        let up = vec![mrow("pg_read_all_data", true, false), mrow("pg_monitor", false, false)];
        let down = vec![mrow("daro_web", true, true)];
        let ddl = ddl_script(&m, &up, &down);
        assert!(
            ddl.lines()
                .next()
                .unwrap_or_default()
                .starts_with("CREATE ROLE \"daro_app\" WITH LOGIN"),
            "第一行是完整属性：{ddl}"
        );
        assert!(ddl.contains("GRANT \"pg_read_all_data\" TO \"daro_app\""));
        assert!(!ddl.contains("pg_monitor"), "没授予的边不进 DDL");
        assert!(ddl.contains("GRANT \"daro_app\" TO \"daro_web\" WITH ADMIN OPTION"));
        // 名字还没填就别拼半条 CREATE ROLE 出去
        assert_eq!(ddl_script(&RoleModel::default(), &up, &down), "");
        // 两栏要能比出差异：工作集多一个超级用户位，正文就得跟着变
        let mut now = m.clone();
        now.superuser = true;
        assert_ne!(ddl, ddl_script(&now, &up, &down));
    }
}
