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

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
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

/// 一行 SQL 里需要单独着色的字节区间。复用 [`crate::sql_highlight`] 的词法
/// 与配色；双引号 / 反引号 / 方括号引起来的是**标识符**（预览 DDL 里全是
/// 这类），词法器把它们归为 String，这里按参考界面口径回落正文色——单引号
/// 才是真字符串。回落正文色的区间不返回（默认样式就是正文色）。
pub fn line_runs(line: &str, dark: bool, fg: Hsla) -> Vec<(Range<usize>, Hsla)> {
    use crate::sql_highlight::{Kind, color_for, lex_sql};
    let mut out = Vec::new();
    for (range, kind) in lex_sql(line) {
        if range.is_empty() {
            continue;
        }
        let color = if kind == Kind::String && !line[range.start..].starts_with('\'') {
            fg
        } else {
            color_for(kind, dark)
        };
        if color != fg {
            out.push((range, color));
        }
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

// ── 可选中 / 复制的 SQL 代码块 ────────────────────────────────

/// 一块可选中、可复制的 SQL。整块排成一个 [`StyledText`]（词法配色走
/// highlight），行号单独排，改动行铺整行绿底；选区由 base 的 window 级
/// [`gpui_kit::base::TextSelection`] 管理，Ctrl+C 由 base Root 处理。
/// 元素本身按内容定高、按外层给的宽度定宽，横向 / 纵向滚动仍交给外层
/// 滚动容器。
pub struct SqlBlock {
    id: ElementId,
    text: SharedString,
    /// 正文样式：切片排版 / 上色按同一份，指标才和整块排版一致。
    style: TextStyle,
    changed: Vec<bool>,
    gutter_w: Pixels,
    width: Pixels,
    order: u64,
    dark: bool,
    fg: Hsla,
}

/// 每个选区切片的行数。base 的选中投影要对 run 里每个字符
/// `position_for_index`，而它按行线性扫；整块一个 run 的代价是
/// O(字符数 × 行数)，千行脚本拖选一下就是几百毫秒。按行切片后整块代价
/// 回到 O(字符数)，双/三击选词、复制的换行仍走同一套逻辑。
const RUN_LINES: usize = 16;

/// 选区的一个排版切片：一段行文字单独排版，供投影只扫自己这几行。
struct SqlRun {
    /// 片内文字。非末片末尾补一个 `\n`，复制时相邻两片才不会粘成一行。
    text: SharedString,
    /// 不含末尾补的 `\n` 的长度：铺高亮时截掉，免得画到下一行去。
    paint_len: usize,
    first_line: usize,
    lines: usize,
    /// 片内相对字节区间的配色：切片要自己画字，得带着高亮一起排。
    highlights: Vec<(Range<usize>, HighlightStyle)>,
}

impl SqlRun {
    fn bounds(&self, text_bounds: Bounds<Pixels>, line_height: Pixels) -> Bounds<Pixels> {
        Bounds::new(
            point(
                text_bounds.left(),
                text_bounds.top() + line_height * self.first_line as f32,
            ),
            size(text_bounds.size.width, line_height * self.lines as f32),
        )
    }
}

/// 造一块 SQL 代码区。`gutter_w` 为 0 时不出行号；`changed[i]` 为真的行
/// 铺绿底；`width` 是绿底铺满的行宽（与外层 `w` 同源）；`order` 是跨块
/// 选中时的文档序（左栏在前）。行高固定 20px、Consolas 12px，与逐行版本一致。
pub fn code_block(
    id: impl Into<ElementId>,
    lines: &[&str],
    changed: &[bool],
    gutter_w: f32,
    width: f32,
    order: u64,
    dark: bool,
    fg: Hsla,
) -> SqlBlock {
    let style = TextStyle {
        color: fg,
        font_family: "Consolas".into(),
        font_size: px(12.).into(),
        line_height: px(20.).into(),
        white_space: WhiteSpace::Nowrap,
        ..Default::default()
    };
    // 切片 / 上色 / 排版都推迟到元素状态里做：拖选时每次重绘都会重建这个
    // 元素，在这里切千行脚本的话，每帧光词法分析就要几毫秒。
    SqlBlock {
        id: id.into(),
        text: lines.join("\n").into(),
        style,
        changed: lines
            .iter()
            .enumerate()
            .map(|(i, _)| changed.get(i).copied().unwrap_or(false))
            .collect(),
        gutter_w: px(gutter_w),
        width: px(width),
        order,
        dark,
        fg,
    }
}

/// 选中文字时的排版口径：与逐行版本同字同高；`Nowrap` 让长句横向撑开，
/// 不在这里折行（折行会改变行号与行高的对应）。
fn block_text_refinement(fg: Hsla) -> TextStyleRefinement {
    TextStyleRefinement {
        color: Some(fg),
        font_family: Some("Consolas".into()),
        font_size: Some(px(12.).into()),
        line_height: Some(px(20.).into()),
        white_space: Some(WhiteSpace::Nowrap),
        ..Default::default()
    }
}

pub struct SqlBlockPrepaint {
    text_bounds: Bounds<Pixels>,
}

/// [`SqlBlock`] 跨帧保留的状态：选区参与者的句柄，加整块 / 切片 / 行号的
/// 排版缓存。这些只在文字、配色或缩放变化时重建，拖选时每帧只做投影与绘制。
pub struct SqlBlockState {
    handle: TextSelectionHandle,
    text: SharedString,
    scale: f32,
    /// 排版是按上一帧的配色做的；明暗 / 前景色变了要重排，否则高亮不跟主题。
    dark: bool,
    fg: Hsla,
    styled: StyledText,
    runs: Vec<SqlRun>,
    styled_runs: Vec<StyledText>,
    gutter: Vec<ShapedLine>,
    /// 上一帧投影出的区间：变了才请求重画（替代整段文字比较）。
    last_ranges: Vec<Option<Range<usize>>>,
}

impl SqlBlockState {
    fn stale(&self, block: &SqlBlock, scale: f32) -> bool {
        self.text.as_ref() != block.text.as_ref()
            || self.scale != scale
            || self.dark != block.dark
            || self.fg != block.fg
    }

    /// 整块切行、上色、排版一次。千行脚本每帧重排要十几毫秒，拖选时会直接
    /// 掉帧，所以只在文字 / 主题 / 缩放变化时跑。
    fn rebuild(&mut self, block: &SqlBlock, scale: f32, window: &mut Window, cx: &mut App) {
        let (highlights, runs) = slice_runs(block);
        self.styled =
            StyledText::new(block.text.clone()).with_default_highlights(&block.style, highlights);
        self.runs = runs;
        self.styled_runs.clear();
        window.with_text_style(Some(block_text_refinement(block.fg)), |window| {
            for i in 0..self.runs.len() {
                let mut styled = StyledText::new(self.runs[i].text.clone())
                    .with_default_highlights(&block.style, self.runs[i].highlights.iter().cloned());
                let (run_layout, ()) = styled.request_layout(None, None, window, cx);
                // 孤儿节点也要走一次排版，TextLayout 才有行位置可查。
                window.compute_layout(run_layout, AvailableSpace::min_size(), cx);
                self.styled_runs.push(styled);
            }
        });
        self.gutter.clear();
        if block.gutter_w > px(0.) {
            let gutter_style = TextStyle {
                color: gutter_color(block.dark),
                font_family: "Consolas".into(),
                font_size: px(12.).into(),
                line_height: px(20.).into(),
                ..Default::default()
            };
            let line_count = block.text.matches('\n').count() + 1;
            self.gutter = (1..=line_count)
                .map(|n| {
                    let text: SharedString = n.to_string().into();
                    let run = gutter_style.to_run(text.len());
                    window
                        .text_system()
                        .shape_line(text, px(12.), std::slice::from_ref(&run), None)
                })
                .collect();
        }
        self.text = block.text.clone();
        self.scale = scale;
        self.dark = block.dark;
        self.fg = block.fg;
        self.last_ranges.clear();
    }
}

/// 整块文字切行、上色，再按 [`RUN_LINES`] 切成选区排版单元。返回整块的
/// 全局高亮与切片元数据（片内高亮是相对切片起点的区间）。
fn slice_runs(block: &SqlBlock) -> (Vec<(Range<usize>, HighlightStyle)>, Vec<SqlRun>) {
    let lines: Vec<&str> = block.text.split('\n').collect();
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    let mut per_line: Vec<Vec<(Range<usize>, HighlightStyle)>> = Vec::with_capacity(lines.len());
    let mut offset = 0usize;
    for line in &lines {
        let mut line_highlights = Vec::new();
        for (range, color) in line_runs(line, block.dark, block.fg) {
            let range = offset + range.start..offset + range.end;
            let style = HighlightStyle {
                color: Some(color),
                ..Default::default()
            };
            highlights.push((range.clone(), style));
            line_highlights.push((range, style));
        }
        per_line.push(line_highlights);
        offset += line.len() + 1;
    }
    // 切片文字就是整段文字里连续的一截，行内高亮减掉这截的起点即可。
    let mut runs = Vec::new();
    let mut run_start = 0usize;
    for (i, part) in lines.chunks(RUN_LINES).enumerate() {
        let mut text = part.join("\n");
        let paint_len = text.len();
        if (i + 1) * RUN_LINES < lines.len() {
            text.push('\n');
        }
        let run_len = text.len();
        let first_line = i * RUN_LINES;
        let run_highlights = per_line[first_line..first_line + part.len()]
            .iter()
            .flatten()
            .map(|(range, style)| (range.start - run_start..range.end - run_start, *style))
            .collect();
        runs.push(SqlRun {
            text: text.into(),
            paint_len,
            first_line,
            lines: part.len(),
            highlights: run_highlights,
        });
        run_start += run_len;
    }
    (highlights, runs)
}

impl IntoElement for SqlBlock {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SqlBlock {
    type RequestLayoutState = Rc<RefCell<SqlBlockState>>;
    type PrepaintState = SqlBlockPrepaint;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // 选区参与者与排版缓存跨帧保留（文本会随设计器 / 部署结果变，跑表由 paint 更新）
        let state_rc = window.with_element_state(
            global_id.expect("SqlBlock must have a stable element id"),
            |retained: Option<Self::RequestLayoutState>, _| {
                let state = retained.unwrap_or_else(|| {
                    let handle = TextSelectionHandle::new(self.text.clone(), cx);
                    // 拖选时把焦点从输入框上挪开：否则 Ctrl+C 命中 "Input" 上下文的
                    // 复制动作（只拷编辑器选区、不向上传递），窗口选区的文本进不了剪贴板。
                    // 失焦后按键走窗口 Root 的 Copy，拷的就是这里选中的 SQL。
                    handle.focus_with(|window, cx| window.blur(cx), cx);
                    Rc::new(RefCell::new(SqlBlockState {
                        handle,
                        text: SharedString::default(),
                        scale: 0.,
                        dark: self.dark,
                        fg: self.fg,
                        styled: StyledText::new(""),
                        runs: Vec::new(),
                        styled_runs: Vec::new(),
                        gutter: Vec::new(),
                        last_ranges: Vec::new(),
                    }))
                });
                (state.clone(), state)
            },
        );
        let mut state = state_rc.borrow_mut();
        let scale = window.scale_factor();
        if state.stale(self, scale) {
            state.rebuild(self, scale, window, cx);
        }
        let (layout_id, ()) = window.with_text_style(Some(block_text_refinement(self.fg)), |window| {
            state
                .styled
                .request_layout(global_id, inspector_id, window, cx)
        });
        drop(state);
        (layout_id, state_rc)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let text_bounds = Bounds::new(
            point(bounds.left() + self.gutter_w, bounds.top()),
            size(
                (bounds.size.width - self.gutter_w).max(px(0.)),
                bounds.size.height,
            ),
        );
        let mut state = state.borrow_mut();
        let line_height = state.styled.layout().line_height();
        window.with_text_style(Some(block_text_refinement(self.fg)), |window| {
            state
                .styled
                .prepaint(global_id, inspector_id, text_bounds, &mut (), window, cx);
            for i in 0..state.runs.len() {
                let run_bounds = state.runs[i].bounds(text_bounds, line_height);
                state.styled_runs[i].prepaint(None, None, run_bounds, &mut (), window, cx);
            }
        });
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let registration = TextSelectionRegistration::new(hitbox, bounds)
            .with_document_order(self.order)
            .with_text_bounds(vec![text_bounds])
            .with_rendered_element(&state.handle, window, cx);
        state.handle.register(registration, window, cx);
        SqlBlockPrepaint { text_bounds }
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut state = state.borrow_mut();
        let line_height = state.styled.layout().line_height();
        // 滚动容器给出的裁剪框（窗口坐标）。长脚本整块上千行，不可见的部分
        // 每帧也发一遍绘制指令，拖选时一帧两遍，几千条绘制就是卡顿的来源。
        let clip = window.content_mask().bounds;
        // 改动行的绿底铺满整行（含行号槽），压在文字下面
        for (i, changed) in self.changed.iter().enumerate() {
            if !changed {
                continue;
            }
            let changed_line = Bounds::new(
                point(bounds.left(), bounds.top() + line_height * i as f32),
                size(self.width, line_height),
            );
            if !clip.intersects(&changed_line) {
                continue;
            }
            window.paint_quad(quad(changed_line, diff_green(self.dark)));
        }
        // 选区：每个切片各自投影出字节区间，绿底之上、文字之下铺高亮
        let runs: Vec<TextSelectionRun> = state
            .runs
            .iter()
            .zip(state.styled_runs.iter())
            .enumerate()
            .map(|(i, (run, styled))| {
                TextSelectionRun::new(
                    run.text.clone(),
                    styled.layout().clone(),
                    run.bounds(prepaint.text_bounds, line_height),
                )
                .with_document_order(i as u64)
            })
            .collect();
        let projection = state.handle.update_runs(&runs, cx);
        if projection.ranges() != state.last_ranges.as_slice() {
            state.last_ranges = projection.ranges().to_vec();
            window.refresh();
        }
        let selection = gpui_kit::base::Theme::global(cx).tokens.colors.selection;
        for (i, range) in projection.ranges().iter().enumerate() {
            let (Some(range), Some(styled)) = (range, state.styled_runs.get(i)) else {
                continue;
            };
            let end = range.end.min(state.runs[i].paint_len);
            let start = range.start.min(end);
            if start >= end {
                continue;
            }
            for quad_bounds in selection_bounds(styled.layout(), start..end) {
                window.paint_quad(quad(quad_bounds, selection));
            }
        }
        for (i, line) in state.gutter.iter().enumerate() {
            let gutter_y = bounds.top() + line_height * i as f32;
            if gutter_y + line_height < clip.top() || gutter_y > clip.bottom() {
                continue;
            }
            let _ = line.paint(
                point(bounds.left(), gutter_y),
                line_height,
                TextAlign::Left,
                Some(self.gutter_w),
                window,
                cx,
            );
        }
        // 只画与裁剪框相交的切片；切片内 16 行全画（一次排版是一次绘制单位）。
        for i in 0..state.runs.len() {
            let run_bounds = state.runs[i].bounds(prepaint.text_bounds, line_height);
            if !clip.intersects(&run_bounds) {
                continue;
            }
            let styled = &mut state.styled_runs[i];
            styled.paint(None, None, run_bounds, &mut (), &mut (), window, cx);
        }
    }
}

/// 一块纯色矩形。
fn quad(bounds: Bounds<Pixels>, color: Hsla) -> PaintQuad {
    PaintQuad {
        bounds,
        background: color.into(),
        corner_radii: Corners::default(),
        border_widths: Edges::default(),
        border_color: transparent_black(),
        border_style: BorderStyle::default(),
    }
}

/// 一段选中区间铺出来的矩形：同一行的取一行，跨行的补中间整宽 + 首尾两截。
fn selection_bounds(layout: &TextLayout, range: Range<usize>) -> Vec<Bounds<Pixels>> {
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return Vec::new();
    };
    let line_height = layout.line_height();
    let bounds = layout.bounds();
    if start.y == end.y {
        return vec![Bounds::from_corners(
            start,
            point(end.x, end.y + line_height),
        )];
    }
    let mut out = vec![Bounds::from_corners(
        start,
        point(bounds.right(), start.y + line_height),
    )];
    if end.y > start.y + line_height {
        out.push(Bounds::from_corners(
            point(bounds.left(), start.y + line_height),
            point(bounds.right(), end.y),
        ));
    }
    out.push(Bounds::from_corners(
        point(bounds.left(), end.y),
        point(end.x, end.y + line_height),
    ));
    out
}

#[cfg(test)]
mod sql_preview_tests {
    use super::{code_block, diff_lines, qualify, rule_template, slice_runs, trigger_template, RUN_LINES};
    use crate::mock::DbKind;
    use gpui_kit::hsla;

    /// 切片不能把语法高亮弄丢：把各片的相对高亮挪回整块坐标，要和整块
    /// 上色逐条对上，接缝两侧的那一行也要各自带着高亮。
    #[test]
    fn run_highlights_shift_back_to_global_offsets() {
        let lines: Vec<String> = (0..33).map(|i| format!("SELECT \"c{i}\" FROM t{i};")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let block = code_block(
            "seam-block",
            &refs,
            &[],
            0.,
            800.,
            0,
            false,
            hsla(0., 0., 0.1, 1.),
        );
        let (global, runs) = slice_runs(&block);
        assert!(!global.is_empty(), "SELECT / FROM 要上色");
        assert!(runs.len() >= 3, "33 行按每片 {RUN_LINES} 行切要出 3 片");
        let mut start = 0usize;
        let mut shifted = Vec::new();
        for run in &runs {
            assert_eq!(
                &block.text[start..start + run.paint_len],
                &run.text[..run.paint_len],
                "切片文字要是整块文字里连续的一截"
            );
            assert!(
                run.text.len() == run.paint_len || run.text.ends_with('\n'),
                "只有非末片才补一个换行"
            );
            for (range, style) in &run.highlights {
                assert!(
                    range.end <= run.paint_len,
                    "高亮不能画到补出来的换行上"
                );
                shifted.push((start + range.start..start + range.end, *style));
            }
            start += run.text.len();
        }
        assert_eq!(shifted, global, "片内高亮挪回整块坐标后要和整块上色一致");
        // 接缝：第二片首行正好是第 17 行，它的关键字高亮要带进第二片
        assert!(runs[0].text.ends_with('\n'));
        assert!(
            block.text[runs[0].text.len()..].starts_with("SELECT"),
            "第二片要从一整行开头切"
        );
        assert!(
            runs[1].highlights.iter().any(|(range, _)| range.start == 0),
            "接缝行的 SELECT 要带着高亮进第二片"
        );
    }

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

#[cfg(test)]
mod selection_tests {
    use gpui_kit::base::{TextSelection, TextSelectionLayer};
    use gpui_kit::component::scroll::ScrollableElement as _;
    use gpui_kit::{
        div, hsla, point, px, Context, InteractiveElement as _, IntoElement, Modifiers,
        MouseButton, MouseDownEvent, ParentElement as _, Render, ScrollDelta, ScrollEvent,
        ScrollHandle, ScrollSource, StatefulInteractiveElement as _, Styled as _, TestAppContext,
        TouchPhase, Window,
    };

    struct BlockView;

    impl Render for BlockView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(TextSelectionLayer)
                .child(
                    div().size_full().overflow_scrollbar().child(
                        div().flex().flex_col().w(px(400.)).child(super::code_block(
                            "sel-block",
                            &["alpha beta", "gamma delta"],
                            &[],
                            0.,
                            400.,
                            0,
                            false,
                            hsla(0., 0., 0.1, 1.),
                        )),
                    ),
                )
        }
    }

    #[gpui_kit::test]
    fn block_drag_selects_visible_text(cx: &mut TestAppContext) {
        let (_view, ctx) = cx.add_window_view(|_, _| BlockView);
        ctx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        ctx.simulate_mouse_down(
            point(px(2.), px(10.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        ctx.simulate_mouse_move(
            point(px(60.), px(30.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        ctx.simulate_mouse_up(
            point(px(60.), px(30.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        let picked = ctx.update(|window, cx| {
            let _ = window.draw(cx);
            TextSelection::selected_text(window, cx)
        });
        assert!(picked.contains("alpha beta"), "picked={picked:?}");
    }

    /// 带真实滚动句柄的竖滚视口：release 里 `debug_selector` 不登记，
    /// 只能从滚动句柄看内容有没有真的被顶上去。
    struct TallBlockView {
        scroll: ScrollHandle,
    }

    impl Render for TallBlockView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .id("scroll-inner")
                    .w(px(400.))
                    .h(px(60.))
                    .self_start()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(400.))
                        .child(super::code_block(
                            "scroll-block",
                            &[
                                "line one",
                                "line two",
                                "line three",
                                "line four",
                                "line five",
                                "line six",
                            ],
                            &[],
                            0.,
                            400.,
                            0,
                            false,
                            hsla(0., 0., 0.1, 1.),
                        )),
                ),
            )
        }
    }

    /// 32 行，横跨两个排版切片（切片 16 行）。
    struct ThirtyTwoLinesView;

    impl Render for ThirtyTwoLinesView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let lines: Vec<String> = (0..32).map(|i| format!("line {i:02}")).collect();
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            div().size_full().child(TextSelectionLayer).child(
                div().size_full().overflow_scrollbar().child(
                    div().flex().flex_col().w(px(400.)).child(super::code_block(
                        "lines-block",
                        &refs,
                        &[],
                        0.,
                        400.,
                        0,
                        false,
                        hsla(0., 0., 0.1, 1.),
                    )),
                ),
            )
        }
    }

    /// 选区切片是为了拖选不卡（每片只扫自己那几行），但复制出来的文字
    /// 必须还是原文：相邻行之间要有换行，切片接缝处也一样。
    #[gpui_kit::test]
    fn block_drag_copy_keeps_newlines_across_shared_runs(cx: &mut TestAppContext) {
        let (_view, ctx) = cx.add_window_view(|_, _| ThirtyTwoLinesView);
        ctx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        ctx.simulate_mouse_down(
            point(px(2.), px(4.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        for y in [100., 300., 630.] {
            ctx.simulate_mouse_move(
                point(px(300.), px(y)),
                Some(MouseButton::Left),
                Modifiers::default(),
            );
        }
        ctx.simulate_mouse_up(
            point(px(300.), px(630.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        let picked = ctx.update(|window, cx| {
            let _ = window.draw(cx);
            TextSelection::selected_text(window, cx)
        });
        assert!(picked.starts_with("line 00\nline 01"), "picked={picked:?}");
        assert!(
            picked.contains("line 15\nline 16"),
            "切片接缝处丢了换行：{picked:?}"
        );
        assert!(picked.contains("line 30\nline 31"), "picked={picked:?}");
    }

    /// 切片不能把双击选词弄丢：在第二片（第 20 行）双击 `20` 只选中这个词。
    #[gpui_kit::test]
    fn block_double_click_still_selects_word(cx: &mut TestAppContext) {
        let (_view, ctx) = cx.add_window_view(|_, _| ThirtyTwoLinesView);
        ctx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let position = point(px(38.), px(410.));
        ctx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        ctx.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
        let picked = ctx.update(|window, cx| {
            let _ = window.draw(cx);
            TextSelection::selected_text(window, cx)
        });
        assert_eq!(picked.trim(), "20", "picked={picked:?}");
    }

    /// 60px 高的视口里塞 6 行（120px）代码：滚轮要真的把内容顶上去，
    /// 否则「支持滚动」只是有个滚动条外壳。
    #[gpui_kit::test]
    fn block_scrolls_vertically_inside_wrapper(cx: &mut TestAppContext) {
        let scroll = ScrollHandle::new();
        let view_scroll = scroll.clone();
        let (_view, ctx) = cx.add_window_view(move |_, _| TallBlockView { scroll: view_scroll });
        // 先出一帧：滚动区要 layout + paint 之后才量得出可滚距离。
        ctx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(scroll.max_offset().y, px(60.), "六行正文比 60px 视口高出一截");
        // 负 delta 向下滚（沿用 gpui 的 ScrollEvent 约定）。
        ctx.simulate_event(ScrollEvent {
            position: point(px(30.), px(20.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-40.))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
            source: ScrollSource::Pointer,
        });
        ctx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(scroll.offset().y, px(-40.), "滚轮没把内容顶上去");
    }

}
