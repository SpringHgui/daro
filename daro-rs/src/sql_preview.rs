//! 表设计器「SQL 预览」页的逻辑层：行级 diff、「更改」增量脚本、行内高亮。
//!
//! 参考口径（`cameras @daowei_dev.public` 屏实测）：
//!
//! - 「DDL」子页：「之前： / 之后：」两栏并排行号对比。改动行整行铺绿底
//!   （取样 `#D6F0EA`），行号灰 `#A9A9A9`；两栏各自滚动，不插占位行对齐——
//!   标准行级 LCS 的着色就能复现参考图（改一行 + 插一行 → 左一行绿、右两行绿）。
//! - 「更改」子页：增量脚本 = `ALTER TABLE … ADD/DROP …` + `CREATE/DROP INDEX …`，
//!   末尾固定追加触发器 / 规则的生成段——设计器里一条触发器都没有时照出
//!   `CREATE TRIGGER BEFORE ON … EXECUTE PROCEDURE ;` 空模板（参考界面原样），
//!   规则段只有 PostgreSQL 出；右上角复制按钮把脚本抄进剪贴板。
//! - 新增的排除约束行即使名称 / 元素还空着也出形状：`EXCLUDE USING btree`。

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::mock::DbKind;
use crate::{DesignerState, to_hsla};

// ── 取色（cameras 屏取样）─────────────────────────────────────

/// 改动行的绿底。暗色下压深一档，保持色相。
pub fn diff_green(dark: bool) -> Hsla {
    if dark {
        to_hsla(0x2c5040)
    } else {
        to_hsla(0xd6f0ea)
    }
}

/// 行号灰。
pub fn gutter_color(dark: bool) -> Hsla {
    if dark {
        to_hsla(0x6e6e6e)
    } else {
        to_hsla(0xa9a9a9)
    }
}

/// 带模式限定的表名：PG / SQL Server 出 `"public"."cameras"`，MySQL 出
/// `` `db`.`table` ``；没有模式层（或设计师没带出模式）只引表名。
pub fn qualify(kind: DbKind, schema: &str, table: &str) -> String {
    let q = |n: &str| {
        if matches!(kind, DbKind::MySql) {
            format!("`{n}`")
        } else {
            format!("\"{n}\"")
        }
    };
    if schema.is_empty() {
        q(table)
    } else {
        format!("{}.{}", q(schema), q(table))
    }
}

// ── 行级 diff ─────────────────────────────────────────────────

/// 标准行级 LCS。返回 `(之前各行是否属于改动块, 之后各行是否属于改动块)`：
/// 改 / 删的行在「之前」侧标 true，改 / 增的行在「之后」侧标 true。
/// mock 档的 DDL 在百行级，O(m·n) 的 DP 表足够。
pub fn diff_lines(before: &str, after: &str) -> (Vec<bool>, Vec<bool>) {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0u32; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ca = vec![true; m];
    let mut cb = vec![true; n];
    let (mut i, mut j) = (0usize, 0usize);
    while i < m && j < n {
        if a[i] == b[j] {
            ca[i] = false;
            cb[j] = false;
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (ca, cb)
}

// ── 行内高亮 ──────────────────────────────────────────────────

/// 一行 SQL 的行内高亮 span。复用 [`crate::sql_highlight`] 的词法与配色；
/// 双引号 / 反引号 / 方括号引起来的是**标识符**（预览 DDL 里全是这类），
/// 词法器把它们归为 String，这里按参考界面口径回落正文色——单引号才是真字符串。
pub fn sql_code_spans(line: &str, dark: bool, fg: Hsla) -> Vec<AnyElement> {
    use crate::sql_highlight::{Kind, color_for, lex_sql};
    let span = |text: &str, color: Hsla| {
        div()
            .flex_none()
            .text_size(px(12.))
            .font_family("Consolas")
            .text_color(color)
            .child(text.to_string())
            .into_any_element()
    };
    let mut out: Vec<AnyElement> = Vec::new();
    let mut pos = 0usize;
    for (range, kind) in lex_sql(line) {
        if range.start > pos {
            out.push(span(&line[pos..range.start], fg));
        }
        let color = if kind == Kind::String && !line[range.start..].starts_with('\'') {
            fg
        } else {
            color_for(kind, dark)
        };
        out.push(span(&line[range.clone()], color));
        pos = range.end;
    }
    if pos < line.len() {
        out.push(span(&line[pos..], fg));
    }
    out
}

// ── 「更改」增量脚本 ──────────────────────────────────────────

/// 「更改」子页的增量脚本：拿工作集和保存基线逐组比，新增的出
/// `ALTER TABLE … ADD …` / `CREATE INDEX …`，删掉的出 `DROP …`；
/// 字段按名字比（改类型的 ALTER 动作接真库后再补，mock 档不出），
/// 约束同名即算未变。末尾按参考界面口径追加触发器 / 规则的生成段。
/// 没有任何改动时返回空串（页面留白，不显示复制按钮）。
pub fn design_change_sql(kind: DbKind, schema: &str, table: &str, ds: &DesignerState) -> String {
    if !crate::design_dirty(ds) {
        return String::new();
    }
    let is_mysql = matches!(kind, DbKind::MySql);
    let is_pg = matches!(kind, DbKind::PostgreSql);
    let qual = qualify(kind, schema, table);
    let q = |n: &str| {
        if is_mysql {
            format!("`{n}`")
        } else {
            format!("\"{n}\"")
        }
    };
    let alter = |frag: &str| format!("ALTER TABLE {qual}\n  {frag};");
    let mut stmts: Vec<String> = Vec::new();

    // 字段：按名字比，新增出 ADD COLUMN，删掉的出 DROP COLUMN
    for c in &ds.cols {
        if c.name.trim().is_empty() || ds.baseline.iter().any(|b| b.name == c.name) {
            continue;
        }
        stmts.push(alter(&format!(
            "ADD COLUMN {}",
            crate::design_col_sql(is_mysql, c)
        )));
    }
    for b in &ds.baseline {
        if b.name.trim().is_empty() || ds.cols.iter().any(|c| c.name == b.name) {
            continue;
        }
        stmts.push(alter(&format!("DROP COLUMN {}", q(b.name.trim()))));
    }
    // 外键 / 唯一键 / 检查：同名即未变；被引用的模式已随模型限定
    for fk in &ds.fks {
        let Some(clause) = crate::design_fk_sql(is_mysql, fk) else {
            continue;
        };
        if ds.baseline_fks.iter().any(|b| b.name.trim() == fk.name.trim()) {
            continue;
        }
        stmts.push(alter(&format!("ADD {clause}")));
    }
    for b in &ds.baseline_fks {
        if crate::design_fk_sql(is_mysql, b).is_none()
            || ds.fks.iter().any(|f| f.name.trim() == b.name.trim())
        {
            continue;
        }
        stmts.push(alter(&format!("DROP CONSTRAINT {}", q(b.name.trim()))));
    }
    for uk in &ds.uniques {
        let Some(clause) = crate::design_unique_sql(is_mysql, uk) else {
            continue;
        };
        if ds
            .baseline_uniques
            .iter()
            .any(|b| b.name.trim() == uk.name.trim())
        {
            continue;
        }
        stmts.push(alter(&format!("ADD {clause}")));
    }
    for b in &ds.baseline_uniques {
        if crate::design_unique_sql(is_mysql, b).is_none()
            || ds.uniques.iter().any(|f| f.name.trim() == b.name.trim())
        {
            continue;
        }
        stmts.push(alter(&format!("DROP CONSTRAINT {}", q(b.name.trim()))));
    }
    for ck in &ds.checks {
        let Some(clause) = crate::design_check_sql(matches!(kind, DbKind::PostgreSql), ck) else {
            continue;
        };
        if ds
            .baseline_checks
            .iter()
            .any(|b| b.name.trim() == ck.name.trim())
        {
            continue;
        }
        stmts.push(alter(&format!("ADD {clause}")));
    }
    for b in &ds.baseline_checks {
        if crate::design_check_sql(matches!(kind, DbKind::PostgreSql), b).is_none()
            || ds.checks.iter().any(|f| f.name.trim() == b.name.trim())
        {
            continue;
        }
        stmts.push(alter(&format!("DROP CONSTRAINT {}", q(b.name.trim()))));
    }
    // 排除：PG 专属，和 CREATE TABLE 的口径一致（空行也出形状）。
    // 种子空白行不算数：按名字配对且每条基线只消费一次，多出来的行
    // （哪怕还是空行）就是新增——参考界面的更改脚本对加一行空排除
    // 照样出 `ADD EXCLUDE USING btree`。
    if is_pg {
        let mut taken = vec![false; ds.baseline_excludes.len()];
        for ex in &ds.excludes {
            let name = ex.name.trim();
            let hit = ds
                .baseline_excludes
                .iter()
                .enumerate()
                .find(|(i, b)| !taken[*i] && b.name.trim() == name)
                .map(|(i, _)| i);
            match hit {
                Some(i) => taken[i] = true,
                None => {
                    if let Some(clause) = crate::design_exclude_sql(ex) {
                        stmts.push(alter(&format!("ADD {clause}")));
                    }
                }
            }
        }
        for (i, b) in ds.baseline_excludes.iter().enumerate() {
            if taken[i] {
                continue;
            }
            let name = b.name.trim();
            if name.is_empty() || ds.excludes.iter().any(|f| f.name.trim() == name) {
                continue;
            }
            stmts.push(alter(&format!("DROP CONSTRAINT {}", q(name))));
        }
    }
    // 索引：新增出整句 CREATE INDEX（带并发 / 表空间等属性），删除出 DROP INDEX
    for idx in &ds.indexes {
        if idx.name.trim().is_empty()
            || ds
                .baseline_indexes
                .iter()
                .any(|b| b.name.trim() == idx.name.trim())
        {
            continue;
        }
        if let Some(stmt) = crate::design_index_sql_at(is_mysql, schema, table, idx, &ds.cols) {
            stmts.push(stmt);
        }
    }
    for b in &ds.baseline_indexes {
        if b.name.trim().is_empty()
            || ds
                .indexes
                .iter()
                .any(|f| f.name.trim() == b.name.trim())
        {
            continue;
        }
        let name = b.name.trim();
        let drop = if !is_mysql && !schema.is_empty() {
            format!("DROP INDEX \"{schema}\".\"{name}\";")
        } else {
            format!("DROP INDEX {};", q(name))
        };
        stmts.push(drop);
    }
    // 触发器 / 规则段：参考界面的更改脚本永远带——有真实行出生成的语句，
    // 没有（或行还空着）就出空模板；规则只有 PostgreSQL 有。
    if matches!(kind, DbKind::MySql | DbKind::PostgreSql | DbKind::SqlServer) {
        let trig = crate::design_triggers_sql_at(kind, schema, table, &ds.triggers);
        if trig.is_empty() {
            let t = trigger_template(kind, schema, table, ds.triggers.first());
            if !t.is_empty() {
                stmts.push(t);
            }
        } else {
            stmts.extend(trig);
        }
    }
    if is_pg {
        let rules: Vec<String> = ds
            .rules
            .iter()
            .filter_map(|r| crate::design_rule_sql(r, ""))
            .collect();
        if rules.is_empty() {
            stmts.push(rule_template(kind, schema, table, ds.rules.first()));
        } else {
            stmts.extend(rules);
        }
    }
    stmts.join("\n\n")
}

/// 空触发器模板（PG 形态，参考界面原样）：
/// `CREATE TRIGGER BEFORE ON "public"."cameras"` + `FOR EACH ROW` +
/// `EXECUTE PROCEDURE ;`。设计器里有草稿行时行上的时机 / 函数跟着走。
fn trigger_template(kind: DbKind, schema: &str, table: &str, row: Option<&crate::DesignTrigger>) -> String {
    let bare = crate::DesignTrigger {
        name: String::new(),
        timing: "BEFORE".into(),
        // 参考图里事件段是空的：CREATE TRIGGER BEFORE ON …（没有 INSERT）
        events: String::new(),
        for_each: "行".into(),
        function: String::new(),
        body: String::new(),
            ..Default::default()
        };
    let t = row.unwrap_or(&bare);
    let timing = if t.timing.trim().is_empty() {
        "BEFORE"
    } else {
        t.timing.trim()
    };
    let qual = qualify(kind, schema, table);
    match kind {
        DbKind::PostgreSql => {
            let each = if t.for_each.trim().starts_with('语') {
                "STATEMENT"
            } else {
                "ROW"
            };
            let mut head = format!("CREATE TRIGGER {timing}");
            let events = t.events.trim();
            if !events.is_empty() {
                head.push(' ');
                head.push_str(events);
            }
            head.push_str(&format!(" ON {qual}"));
            let f = t.function.trim();
            let tail = if f.is_empty() {
                "EXECUTE PROCEDURE ;".to_string()
            } else {
                format!("EXECUTE PROCEDURE {f}();")
            };
            format!("{head}\nFOR EACH {each}\n{tail}")
        }
        DbKind::MySql => {
            let ev = t
                .events
                .split(',')
                .map(str::trim)
                .find(|s| !s.is_empty())
                .unwrap_or("");
            let mut head = format!("CREATE TRIGGER {timing}");
            if !ev.is_empty() {
                head.push(' ');
                head.push_str(ev);
            }
            format!("{head} ON {qual} FOR EACH ROW {};", t.body.trim())
        }
        DbKind::SqlServer => {
            let timing = if timing == "INSTEAD OF" { "INSTEAD OF" } else { "AFTER" };
            format!(
                "CREATE TRIGGER {qual} {timing} {} AS {};",
                t.events.split(',').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", "),
                t.body.trim()
            )
        }
        _ => String::new(),
    }
}

/// 空规则模板（参考界面原样）：`CREATE RULE  AS ON INSERT TO "public"."cameras" DO NOTHING;`
/// ——名字空着所以 RULE 后面是两个空格，事件回落 INSERT，动作回落 DO NOTHING。
fn rule_template(kind: DbKind, schema: &str, table: &str, row: Option<&crate::DesignRule>) -> String {
    let name = row.map(|r| r.name.trim()).unwrap_or("");
    let event = row
        .map(|r| r.event.trim())
        .filter(|e| !e.is_empty())
        .unwrap_or("INSERT");
    let qual = qualify(kind, schema, table);
    let mut sql = format!("CREATE RULE {name} AS ON {event} TO {qual}");
    if let Some(r) = row {
        let qual_cond = r.qualification.trim();
        if !qual_cond.is_empty() {
            sql.push_str(&format!(" WHERE ({qual_cond})"));
        }
    }
    let action = row.map(|r| r.action.trim().to_uppercase()).unwrap_or_default();
    match action.as_str() {
        "ALSO" | "INSTEAD" => {
            let stmt = row.map(|r| r.statement.trim()).unwrap_or("");
            if stmt.is_empty() {
                sql.push_str(" DO NOTHING");
            } else {
                sql.push_str(&format!(" DO {action} {stmt}"));
            }
        }
        _ => sql.push_str(" DO NOTHING"),
    }
    sql.push(';');
    sql
}

// ── 渲染辅助（预览页两栏共用的行构件）────────────────────────

/// 预览代码区一行的内容宽度（行号槽另加）：只用于横向滚动的总量估算，
/// 与列宽自适应同一口径。
pub fn code_lines_w(lines: &[&str], gutter_w: f32) -> f32 {
    lines
        .iter()
        .map(|l| crate::est_text_w(l, 12.))
        .fold(0., f32::max)
        + gutter_w
        + 22.
}

/// diff 面板里的一行：行号槽 + 行内高亮，改动行整行铺绿底。
pub fn diff_row(
    n: usize,
    line: &str,
    changed: bool,
    w: f32,
    gutter_w: f32,
    dark: bool,
    fg: Hsla,
) -> AnyElement {
    h_flex()
        .h(px(20.))
        .w(px(w))
        .flex_none()
        .when(changed, |s| s.bg(diff_green(dark)))
        .child(
            div()
                .w(px(gutter_w))
                .pr(px(10.))
                .flex_none()
                .text_size(px(12.))
                .font_family("Consolas")
                .text_color(gutter_color(dark))
                .child(n.to_string()),
        )
        .child(h_flex().flex_none().children(sql_code_spans(line, dark, fg)))
        .into_any_element()
}

/// 「更改」脚本区的一行：没有行号、没有绿底。
pub fn plain_row(line: &str, w: f32, dark: bool, fg: Hsla) -> AnyElement {
    h_flex()
        .h(px(20.))
        .w(px(w))
        .flex_none()
        .child(h_flex().flex_none().children(sql_code_spans(line, dark, fg)))
        .into_any_element()
}

#[cfg(test)]
mod sql_preview_tests {
    use super::{diff_lines, qualify, rule_template, trigger_template};
    use crate::mock::DbKind;

    /// 参考图那屏的 diff 形态：末尾约束行补了个逗号（改）+ 插入一行 EXCLUDE
    /// → 左侧一行绿、右侧两行绿，其余行全不变。
    #[test]
    fn diff_marks_modified_and_inserted_like_reference() {
        let before = "A\nB\nC (\"id\")\n);\n";
        let after = "A\nB\nC (\"id\"),\nEXCLUDE USING btree\n);\n";
        let (cb, ca) = diff_lines(before, after);
        assert_eq!(cb, vec![false, false, true, false]);
        assert_eq!(ca, vec![false, false, true, true, false]);
    }

    #[test]
    fn diff_clean_when_identical() {
        let (cb, ca) = diff_lines("x\ny\n", "x\ny\n");
        assert!(cb.iter().all(|c| !c) && ca.iter().all(|c| !c));
    }

    /// PG 空触发器模板（参考界面原样，注意事件段为空、EXECUTE PROCEDURE 后空函数）
    #[test]
    fn trigger_template_matches_reference_shape() {
        let t = trigger_template(
            DbKind::PostgreSql,
            "public",
            "cameras",
            None,
        );
        assert_eq!(
            t,
            "CREATE TRIGGER BEFORE ON \"public\".\"cameras\"\nFOR EACH ROW\nEXECUTE PROCEDURE ;"
        );
    }

    /// 空规则模板：名字空着 → RULE 后两个空格，事件回落 INSERT
    #[test]
    fn rule_template_matches_reference_shape() {
        let t = rule_template(DbKind::PostgreSql, "public", "cameras", None);
        assert_eq!(
            t,
            "CREATE RULE  AS ON INSERT TO \"public\".\"cameras\" DO NOTHING;"
        );
    }

    #[test]
    fn qualify_adds_schema_for_pg_but_not_mysql() {
        assert_eq!(qualify(DbKind::PostgreSql, "public", "cameras"), "\"public\".\"cameras\"");
        assert_eq!(qualify(DbKind::MySql, "db", "t"), "`db`.`t`");
        assert_eq!(qualify(DbKind::PostgreSql, "", "cameras"), "\"cameras\"");
    }
}
