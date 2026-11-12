//! 表 / 视图右键菜单的 DDL：语句拼装与文案。
//!
//! 各方言的引用符与「库 / 模式」限定名一律问 [Target]（它已经知道自己是
//! SQLite 的 ATTACH 库名、PG 的模式名还是 MySQL 的库名），这里只管动词本身。
//! 文案与关键字都按 [ObjKind] 出，表菜单和视图菜单因此共用同一条链路。

use crate::backend::Target;
use crate::mock::DbKind;

/// DDL 动的是哪一类对象。目录分类下标里只有这两类有可执行的 DDL 通路。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjKind {
    Table,
    View,
}

impl ObjKind {
    /// 分类下标 → 对象类型；函数 / 查询等还没有 DDL 通路，给 None 让上层置灰。
    pub fn from_cat(k: usize) -> Option<Self> {
        match k {
            0 => Some(ObjKind::Table),
            1 => Some(ObjKind::View),
            _ => None,
        }
    }

    /// 文案里的名词：「删除表」/「删除视图」、「新表名」/「新视图名」。
    pub fn noun(self) -> &'static str {
        match self {
            ObjKind::Table => "表",
            ObjKind::View => "视图",
        }
    }
}

/// 对象上可执行的四种结构 / 数据操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableOp {
    /// DROP TABLE / DROP VIEW
    Drop,
    /// DELETE FROM ——参考客户端的「清空表」，逐行删除，触发器照走、自增计数不动
    Clear,
    /// TRUNCATE TABLE —— 整表清空并复位自增；SQLite 无该语法，退成 DELETE FROM
    Truncate,
    /// ALTER TABLE … RENAME TO（MySQL 走 RENAME TABLE）
    Rename,
}

impl TableOp {
    /// 这一项在该类对象上到底做不做得了：视图没有自己的数据本体，清空 / 截断
    /// 无从谈起；改名各家都不认 `ALTER TABLE`（SQLite 实测直接回 `view vw may
    /// not be altered`，PG 要 `ALTER VIEW`、MySQL 能不能用 `RENAME TABLE` 没有
    /// 实测样本），重建视图体的链路还没做——三项一起挡在菜单外，不摆点了报错
    /// 的入口。
    pub fn applies_to(self, kind: ObjKind) -> bool {
        match self {
            TableOp::Drop => true,
            TableOp::Clear | TableOp::Truncate | TableOp::Rename => matches!(kind, ObjKind::Table),
        }
    }

    /// 菜单项与确认框标题用的动作名（照参考客户端的「删除表 / 删除视图」文案）。
    /// 返回 `&'static str`：弹窗标题那侧的接口就是这个签名，文案又是固定几条，
    /// 拼 `format!` 反而要把对话框签名一起改。
    pub fn label(self, kind: ObjKind) -> &'static str {
        match (self, kind) {
            (TableOp::Drop, ObjKind::Table) => "删除表",
            (TableOp::Drop, ObjKind::View) => "删除视图",
            // 清空 / 截断只在表上可达（applies_to 挡住了视图）
            (TableOp::Clear, _) => "清空表",
            (TableOp::Truncate, _) => "截断表",
            (TableOp::Rename, _) => "重命名",
        }
    }

    /// 失败弹窗标题。重命名在菜单里不带名词，标题要补上；其余三项与菜单同名。
    pub fn fail_title(self, kind: ObjKind) -> &'static str {
        match self {
            TableOp::Rename => match kind {
                ObjKind::Table => "重命名表",
                ObjKind::View => "重命名视图",
            },
            other => other.label(kind),
        }
    }

    /// 确认框正文：说清动的是哪张对象、会不会连结构一起没。
    pub fn confirm_text(self, table: &str, kind: ObjKind) -> String {
        let noun = kind.noun();
        match self {
            TableOp::Drop if kind == ObjKind::View => format!(
                "确实要删除视图「{table}」吗？\n视图定义会被移除，基表数据不受影响。"
            ),
            TableOp::Drop => format!(
                "确实要删除{noun}「{table}」吗？\n{noun}结构与其中全部数据都会被移除。"
            ),
            TableOp::Clear => format!("确实要清空{noun}「{table}」吗？\n其中全部数据将被删除。"),
            TableOp::Truncate => format!(
                "确实要截断{noun}「{table}」吗？\n全部数据将被删除，自增计数一并复位。"
            ),
            TableOp::Rename => String::new(),
        }
    }
}

/// 拼出该方言下这条操作的语句。`new_name` 只有重命名用得到，其余传空串。
pub fn statement(tg: &Target, op: TableOp, table: &str, new_name: &str, kind: ObjKind) -> String {
    let name = tg.qualified_table(table);
    match (op, kind) {
        (TableOp::Drop, ObjKind::Table) => format!("DROP TABLE {name}"),
        (TableOp::Drop, ObjKind::View) => format!("DROP VIEW {name}"),
        // 清空 / 截断只在表上可达（applies_to 挡住了视图），语句照表拼
        (TableOp::Clear, _) => format!("DELETE FROM {name}"),
        // SQLite 没有 TRUNCATE：DELETE 是官方等价写法（自增计数复位需要
        // 再动 sqlite_sequence，这里不偷偷多做，行为差异写进文档）
        (TableOp::Truncate, _) if tg.kind() == DbKind::SQLite => format!("DELETE FROM {name}"),
        (TableOp::Truncate, _) => format!("TRUNCATE TABLE {name}"),
        // 改名三方言都用表的写法：MySQL 的 RENAME TABLE、PG 与 SQLite 的
        // ALTER TABLE … RENAME TO 对视图同样生效（视图在目录里就是一条名字）
        (TableOp::Rename, _) => {
            let to = tg.quote(new_name);
            match tg.kind() {
                // MySQL 的 RENAME TABLE 两侧都要带库名，否则可能落到连接默认的库里
                DbKind::MySql => format!("RENAME TABLE {name} TO {}{to}", tg.qualified_prefix()),
                _ => format!("ALTER TABLE {name} RENAME TO {to}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::Connection;

    fn target(kind: DbKind, db: &str, schema: &str) -> Target {
        let conn = Connection::new("t", kind, "h", 0, db, "u", "p", "", true);
        Target::of(&conn, db, schema).expect("真连接要有落点")
    }

    #[test]
    fn drop_clear_truncate_are_qualified_per_dialect() {
        let mysql = target(DbKind::MySql, "shop", "");
        assert_eq!(
            statement(&mysql, TableOp::Drop, "orders", "", ObjKind::Table),
            "DROP TABLE `shop`.`orders`"
        );
        assert_eq!(
            statement(&mysql, TableOp::Clear, "orders", "", ObjKind::Table),
            "DELETE FROM `shop`.`orders`"
        );
        assert_eq!(
            statement(&mysql, TableOp::Truncate, "orders", "", ObjKind::Table),
            "TRUNCATE TABLE `shop`.`orders`"
        );

        let pg = target(DbKind::PostgreSql, "shop", "crm");
        assert_eq!(
            statement(&pg, TableOp::Drop, "orders", "", ObjKind::Table),
            r#"DROP TABLE "crm"."orders""#
        );
        // 模式为空时 Target 兜到 public
        let pg_default = target(DbKind::PostgreSql, "shop", "");
        assert_eq!(
            statement(&pg_default, TableOp::Truncate, "orders", "", ObjKind::Table),
            r#"TRUNCATE TABLE "public"."orders""#
        );

        let sqlite = target(DbKind::SQLite, "main", "");
        assert_eq!(
            statement(&sqlite, TableOp::Drop, "orders", "", ObjKind::Table),
            r#"DROP TABLE "main"."orders""#
        );
        // SQLite 没有 TRUNCATE 语法，截断退成 DELETE（不能拼出跑不动的语句）
        assert_eq!(
            statement(&sqlite, TableOp::Truncate, "orders", "", ObjKind::Table),
            r#"DELETE FROM "main"."orders""#
        );
    }

    #[test]
    fn view_drop_uses_the_view_keyword_in_every_dialect() {
        let mysql = target(DbKind::MySql, "shop", "");
        assert_eq!(
            statement(&mysql, TableOp::Drop, "v_orders", "", ObjKind::View),
            "DROP VIEW `shop`.`v_orders`"
        );
        let pg = target(DbKind::PostgreSql, "shop", "crm");
        assert_eq!(
            statement(&pg, TableOp::Drop, "v_orders", "", ObjKind::View),
            r#"DROP VIEW "crm"."v_orders""#
        );
        let sqlite = target(DbKind::SQLite, "main", "");
        assert_eq!(
            statement(&sqlite, TableOp::Drop, "v_orders", "", ObjKind::View),
            r#"DROP VIEW "main"."v_orders""#
        );
        // 视图没有数据本体：清空 / 截断在视图上不可达，菜单据此置灰
        assert!(!TableOp::Clear.applies_to(ObjKind::View));
        assert!(!TableOp::Truncate.applies_to(ObjKind::View));
        // 视图改名要重建视图体，各家语法没有实测样本 → 一并置灰
        assert!(!TableOp::Rename.applies_to(ObjKind::View));
        assert!(TableOp::Drop.applies_to(ObjKind::View));
        // 文案跟着对象类型走
        assert_eq!(TableOp::Drop.label(ObjKind::View), "删除视图");
        assert!(TableOp::Drop
            .confirm_text("v_orders", ObjKind::View)
            .contains("视图定义会被移除"));
    }

    #[test]
    fn rename_keeps_the_target_in_its_own_database() {
        let mysql = target(DbKind::MySql, "shop", "");
        assert_eq!(
            statement(&mysql, TableOp::Rename, "orders", "orders_new", ObjKind::Table),
            "RENAME TABLE `shop`.`orders` TO `shop`.`orders_new`"
        );
        let pg = target(DbKind::PostgreSql, "shop", "crm");
        assert_eq!(
            statement(&pg, TableOp::Rename, "orders", "orders_new", ObjKind::Table),
            r#"ALTER TABLE "crm"."orders" RENAME TO "orders_new""#
        );
        let sqlite = target(DbKind::SQLite, "main", "");
        assert_eq!(
            statement(&sqlite, TableOp::Rename, "orders", "orders_new", ObjKind::Table),
            r#"ALTER TABLE "main"."orders" RENAME TO "orders_new""#
        );
        // 视图改名走同一写法（三方言的这条语句都认视图）
        assert_eq!(
            statement(&mysql, TableOp::Rename, "v_a", "v_b", ObjKind::View),
            "RENAME TABLE `shop`.`v_a` TO `shop`.`v_b`"
        );
        assert_eq!(
            statement(&sqlite, TableOp::Rename, "v_a", "v_b", ObjKind::View),
            r#"ALTER TABLE "main"."v_a" RENAME TO "v_b""#
        );
    }

    #[test]
    fn identifiers_with_the_quote_character_are_doubled() {
        // 反引号 / 双引号在标识符里要双写，否则拼出的语句能把后面的内容当语句跑
        let mysql = target(DbKind::MySql, "sh`op", "");
        assert_eq!(
            statement(&mysql, TableOp::Drop, "or`ders", "", ObjKind::Table),
            "DROP TABLE `sh``op`.`or``ders`"
        );
        let pg = target(DbKind::PostgreSql, "shop", "cr\"m");
        assert_eq!(
            statement(&pg, TableOp::Rename, "or\"ders", "new\"name", ObjKind::View),
            r#"ALTER TABLE "cr""m"."or""ders" RENAME TO "new""name""#
        );
    }
}
