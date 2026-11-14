//! 「结构同步」：选两个库 / 模式，整体比较表、视图、实体化视图、函数、
//! 存储过程的结构，生成把目标端对齐到源端的增量脚本。界面照参考客户端的三步
//! 向导：第一页选源与目标并勾选比较范围，第二页逐对象看增量脚本与 DDL 对比，
//! 第三页确认部署脚本并按「开始」在目标端逐条执行。
//!
//! 比较与生成永远不碰目标库；只有部署页的「开始」会在目标端执行脚本。
//! 目标端多出的对象只给注释掉的 DROP，执行前会跳过这类纯注释语句；
//! 表内删除列照常出语句但带警示注释。支持 MySQL / PostgreSQL / SQLite
//! （[crate::read_design_meta] 覆盖的方言）；SQLite 没有例程，触发器与数据
//! 不参与比较。
//!
//! 表结构按库 / 模式整批取回（MySQL 7 条、PostgreSQL 4 条目录查询），
//! 不再逐表往返；视图定义与例程定义同样一次取回。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::backend::{self, Target};
use crate::catalog;
use crate::combo;
use crate::db_icons;
use crate::menu::{DropdownMenu as _, PopupMenuItem};
use crate::mock::{Connection, DbKind};
use crate::palette::Palette;
use crate::sql_preview;
use crate::ui::TITLE_H;
use crate::zed_icons;
use crate::zed_icons::ZedIcon;
use crate::{DaroApp, to_hsla, ui};

/// 侧栏标题行高（「源（同步自）」/「目标（同步到）」）
const SIDE_TITLE_H: f32 = 24.;
/// 字段块里的标签行高
const LABEL_H: f32 = 16.;
/// 选择框沿用查询窗口组合框的高度
const FIELD_H: f32 = crate::combo::TRIGGER_H;
/// 侧栏一行的块高：标签 + 选择框（面板紧贴选择框下沿）
const BLOCK_H: f32 = LABEL_H + FIELD_H;
/// 相邻字段块之间的间距
const BLOCK_GAP: f32 = 12.;
/// 下拉面板最大高
const MENU_MAX_H: f32 = 220.;

/// 比较分类：0 表 / 1 视图 / 2 实体化视图 / 3 函数 / 4 存储过程。
const CAT_TABLE: u8 = 0;
const CAT_VIEW: u8 = 1;
const CAT_MATVIEW: u8 = 2;
const CAT_FUNC: u8 = 3;
const CAT_PROC: u8 = 4;

fn supports(kind: DbKind) -> bool {
    matches!(kind, DbKind::MySql | DbKind::PostgreSql | DbKind::SQLite)
}

fn has_routines(kind: DbKind) -> bool {
    matches!(kind, DbKind::MySql | DbKind::PostgreSql)
}

/// 该引擎有没有这一类对象（首屏据此收放勾选项）。
fn cat_supported(kind: DbKind, cat: u8) -> bool {
    match cat {
        CAT_TABLE | CAT_VIEW => supports(kind),
        CAT_MATVIEW => kind == DbKind::PostgreSql,
        CAT_FUNC | CAT_PROC => has_routines(kind),
        _ => false,
    }
}

/// 参与比较的分类勾选。
#[derive(Clone, Copy, PartialEq, Eq)]
struct CompareOptions {
    tables: bool,
    views: bool,
    matviews: bool,
    funcs: bool,
    procs: bool,
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            tables: true,
            views: true,
            matviews: true,
            funcs: true,
            procs: true,
        }
    }
}

impl CompareOptions {
    fn get(self, cat: u8) -> bool {
        match cat {
            CAT_TABLE => self.tables,
            CAT_VIEW => self.views,
            CAT_MATVIEW => self.matviews,
            CAT_FUNC => self.funcs,
            CAT_PROC => self.procs,
            _ => false,
        }
    }

    fn toggle(&mut self, cat: u8) {
        let slot = match cat {
            CAT_TABLE => &mut self.tables,
            CAT_VIEW => &mut self.views,
            CAT_MATVIEW => &mut self.matviews,
            CAT_FUNC => &mut self.funcs,
            CAT_PROC => &mut self.procs,
            _ => return,
        };
        *slot = !*slot;
    }

    /// 实际参比的分类：勾选 ∩ 该引擎支持，顺序即脚本里的分节顺序。
    fn cats(self, kind: DbKind) -> Vec<u8> {
        [CAT_TABLE, CAT_VIEW, CAT_MATVIEW, CAT_FUNC, CAT_PROC]
            .into_iter()
            .filter(|cat| self.get(*cat) && cat_supported(kind, *cat))
            .collect()
    }
}

fn has_schema(kind: DbKind) -> bool {
    kind == DbKind::PostgreSql
}

/// 首屏「服务器版本」：一条查询取引擎版本，读不到返回 None（界面显示 `--`）。
fn query_version(conn: &Connection, db: &str, schema: &str) -> Option<String> {
    let tg = Target::of(conn, db, schema)?;
    let sql = match conn.kind {
        DbKind::MySql => "SELECT VERSION()",
        DbKind::PostgreSql => "SHOW server_version",
        DbKind::SQLite => "SELECT sqlite_version()",
        _ => return None,
    };
    let value = crate::meta_rows(&tg, sql)
        .ok()?
        .into_iter()
        .next()?
        .into_iter()
        .next()?
        .trim()
        .to_string();
    if value.is_empty() { None } else { Some(value) }
}

/// 「保存配置文件 / 加载配置文件」的 JSON 结构：源与目标的定位 + 比较范围。
#[derive(Serialize, Deserialize)]
struct ProfileFile {
    version: u32,
    source: ProfileSide,
    target: ProfileSide,
    tables: bool,
    views: bool,
    matviews: bool,
    funcs: bool,
    procs: bool,
}

#[derive(Serialize, Deserialize)]
struct ProfileSide {
    connection: String,
    db: String,
    schema: String,
}

fn ok_color(dark: bool) -> Hsla {
    to_hsla(if dark { 0x5cc98a } else { 0x1a7f37 })
}

fn bad_color(dark: bool) -> Hsla {
    to_hsla(if dark { 0xe06c75 } else { 0xcf222e })
}

/// 耗时口径 `mm:ss.cc`：2070 ms → `00:02.07`。
fn fmt_elapsed(ms: u128) -> String {
    let cs = ms / 10;
    format!("{:02}:{:02}.{:02}", cs / 6000, (cs / 100) % 60, cs % 100)
}

/// 失败语句在日志里的展示：跳过脚本头注释取第一行 SQL；ALTER 语句再补上
/// 第一处 ALTER COLUMN，否则只看到表名、定位不到是哪一列出的错。
fn failure_snippet(stmt: &str) -> Vec<String> {
    let sql_line = |l: &&str| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with("--")
    };
    let Some(first) = stmt.lines().find(sql_line) else {
        return Vec::new();
    };
    let mut out = vec![first.trim().to_string()];
    if let Some(col) = stmt
        .lines()
        .map(str::trim)
        .find(|l| l.to_ascii_uppercase().starts_with("ALTER COLUMN"))
    {
        out.push(col.to_string());
    }
    out
}

// ── 结果模型 ────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffStatus {
    New,
    Changed,
    Removed,
    Same,
}

impl DiffStatus {
    fn label(self) -> &'static str {
        match self {
            DiffStatus::New => "新增",
            DiffStatus::Changed => "修改",
            DiffStatus::Removed => "删除",
            DiffStatus::Same => "相同",
        }
    }

    fn ix(self) -> usize {
        match self {
            DiffStatus::New => 0,
            DiffStatus::Changed => 1,
            DiffStatus::Removed => 2,
            DiffStatus::Same => 3,
        }
    }

    fn color(self, dark: bool) -> Hsla {
        match self {
            DiffStatus::New => ok_color(dark),
            DiffStatus::Changed => to_hsla(Palette::of(dark).accent),
            DiffStatus::Removed => bad_color(dark),
            DiffStatus::Same => to_hsla(Palette::of(dark).muted_foreground),
        }
    }
}

/// 对象内部的一处差异项：列 / 主键 / 索引 / 外键 / 唯一键 / 检查 /
/// 表选项 / 视图或例程的定义。
#[derive(Clone, Copy, PartialEq, Eq)]
enum ItemKind {
    Column,
    PrimaryKey,
    Index,
    ForeignKey,
    Unique,
    Check,
    Option,
    Definition,
}

impl ItemKind {
    fn label(self) -> &'static str {
        match self {
            ItemKind::Column => "字段",
            ItemKind::PrimaryKey => "主键",
            ItemKind::Index => "索引",
            ItemKind::ForeignKey => "外键",
            ItemKind::Unique => "唯一键",
            ItemKind::Check => "检查",
            ItemKind::Option => "表选项",
            ItemKind::Definition => "定义",
        }
    }

    fn icon(self) -> ZedIcon {
        match self {
            ItemKind::Column => ZedIcon::Columns,
            ItemKind::PrimaryKey => ZedIcon::Key,
            ItemKind::Index => ZedIcon::Hash,
            ItemKind::ForeignKey => ZedIcon::Link,
            ItemKind::Unique => ZedIcon::Key,
            ItemKind::Check => ZedIcon::Check,
            ItemKind::Option => ZedIcon::Settings,
            ItemKind::Definition => ZedIcon::Code,
        }
    }
}

/// 比较页展开对象后的一行明细。
struct DiffItem {
    label: String,
    note: String,
    kind: ItemKind,
    status: DiffStatus,
}

struct ObjDiff {
    cat: u8,
    name: String,
    status: DiffStatus,
    detail: String,
    src_ddl: String,
    dst_ddl: String,
    script: String,
    items: Vec<DiffItem>,
}

impl ObjDiff {
    fn cat_label(&self) -> &'static str {
        cat_title(self.cat)
    }
}

struct CompareResult {
    objects: Vec<ObjDiff>,
    script: String,
    counts: [usize; 4],
    warnings: Vec<String>,
    src_label: String,
    dst_label: String,
    src_name: String,
    dst_name: String,
    src_place: String,
    dst_place: String,
}

/// 部署页「消息日志」的一行：一条语句 + 回执。
struct DeployEntry {
    ok: bool,
    stmt: String,
    msg: String,
    ms: u128,
}

/// 去掉行注释后还剩不剩 SQL：目标端多出的对象只生成 `-- DROP ...`，
/// 这种纯注释语句不往目标端发。
fn has_sql_code(stmt: &str) -> bool {
    stmt.lines().any(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with("--")
    })
}

/// 部署脚本按勾选对象拼装。外键约束全部挪到末尾统一添加：新建表之间互相
/// 引用时按名字排序会先建引用方（`approval_flow_versions` → `approval_flows`），
/// 直接执行报 42P01。
fn deploy_script_text(kind: DbKind, result: &CompareResult, checked: &HashSet<usize>) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut deferred: Vec<String> = Vec::new();
    for (ix, o) in result.objects.iter().enumerate() {
        if !checked.contains(&ix) || o.script.trim().is_empty() {
            continue;
        }
        let (body, mut fks) = defer_fk_constraints(kind, &o.script);
        deferred.append(&mut fks);
        parts.push(format!(
            "-- [{}] {}（{}）\n{}",
            o.status.label(),
            o.name,
            o.cat_label(),
            body.trim_end()
        ));
    }
    if parts.is_empty() {
        return String::new();
    }
    if !deferred.is_empty() {
        parts.push(format!(
            "-- 外键约束（等所有表建好后再加，避免依赖顺序）\n{}",
            deferred.join("\n")
        ));
    }
    format!(
        "-- daro 结构同步部署脚本（仅勾选对象）\n-- 源:   {}\n-- 目标: {}\n\n{}",
        result.src_label,
        result.dst_label,
        parts.join("\n\n")
    )
}

/// 把外键约束从脚本里拆出来：表内 `CONSTRAINT … FOREIGN KEY` 改写成
/// `ALTER TABLE … ADD …`，独立的 `ADD CONSTRAINT` 原样搬走，连带它们的
/// `COMMENT ON CONSTRAINT`。SQLite 不支持事后加约束，建表也不校验引用对象，
/// 原样返回。
pub(crate) fn defer_fk_constraints(kind: DbKind, script: &str) -> (String, Vec<String>) {
    if kind == DbKind::SQLite {
        return (script.to_string(), Vec::new());
    }
    let mut kept: Vec<String> = Vec::new();
    let mut deferred: Vec<String> = Vec::new();
    let mut names: HashSet<String> = HashSet::new();
    for stmt in crate::split_sql_statements(script) {
        // 拆分器把语句末尾的分号吃掉了，拼回去时补上，否则整段会被当成一条。
        let up = stmt.trim_start().to_ascii_uppercase();
        if up.starts_with("CREATE TABLE") || up.starts_with("CREATE UNLOGGED TABLE") {
            let (text, mut fks) = split_inline_fks(&stmt);
            for fk in &fks {
                if let Some(n) = constraint_name(fk) {
                    names.insert(n.to_ascii_uppercase());
                }
            }
            kept.push(format!("{text};"));
            deferred.append(&mut fks);
        } else if up.starts_with("ALTER TABLE")
            && up.contains("ADD CONSTRAINT")
            && up.contains("FOREIGN KEY")
        {
            if let Some(n) = constraint_name(&stmt) {
                names.insert(n.to_ascii_uppercase());
            }
            deferred.push(format!("{};", stmt.trim()));
        } else if up.starts_with("COMMENT ON CONSTRAINT")
            && constraint_name(&stmt).is_some_and(|n| names.contains(&n.to_ascii_uppercase()))
        {
            deferred.push(format!("{};", stmt.trim()));
        } else {
            kept.push(format!("{};", stmt.trim()));
        }
    }
    (kept.join("\n"), deferred)
}

/// 约束名（去掉方言引号）；`CONSTRAINT` 字样可能出现在注释里，但这里只在
/// 已判定为外键语句的片段上调。
fn constraint_name(sql: &str) -> Option<String> {
    let at = sql.to_ascii_uppercase().find("CONSTRAINT ")? + "CONSTRAINT ".len();
    let rest = sql[at..].trim_start_matches(['"', '`', '[']);
    let end = rest
        .find(|c: char| c.is_whitespace() || matches!(c, '"' | '`' | ']'))
        .unwrap_or(rest.len());
    (end > 0).then(|| rest[..end].to_string())
}

/// CREATE TABLE 表内以 `CONSTRAINT … FOREIGN KEY` 开头的项抽成独立的
/// `ALTER TABLE … ADD …`；没有外键项的原样返回。
fn split_inline_fks(stmt: &str) -> (String, Vec<String>) {
    let lines: Vec<&str> = stmt.lines().collect();
    let Some(open) = lines.iter().position(|l| l.trim_end().ends_with('(')) else {
        return (stmt.to_string(), Vec::new());
    };
    let Some(close) = lines.iter().rposition(|l| l.trim_start().starts_with(')')) else {
        return (stmt.to_string(), Vec::new());
    };
    if close <= open + 1 {
        return (stmt.to_string(), Vec::new());
    }
    let head = lines[open].trim_end().trim_end_matches('(').trim_end();
    let up = head.to_ascii_uppercase();
    let Some(at) = up.find(" TABLE ") else {
        return (stmt.to_string(), Vec::new());
    };
    let qual = head[at + " TABLE ".len()..].trim().to_string();
    let mut items: Vec<String> = Vec::new();
    let mut fks: Vec<String> = Vec::new();
    for raw in &lines[open + 1..close] {
        let item = raw.trim().trim_end_matches(',').to_string();
        let iu = item.to_ascii_uppercase();
        if iu.starts_with("CONSTRAINT ") && iu.contains(" FOREIGN KEY ") {
            fks.push(format!("ALTER TABLE {qual}\n  ADD {item};"));
        } else {
            items.push(item);
        }
    }
    if fks.is_empty() {
        return (stmt.to_string(), Vec::new());
    }
    let mut out = format!("{head} (");
    for (ix, item) in items.iter().enumerate() {
        out.push('\n');
        out.push_str("    ");
        out.push_str(item);
        if ix + 1 < items.len() {
            out.push(',');
        }
    }
    out.push('\n');
    out.push_str(lines[close]);
    (out, fks)
}

/// 比较页对象清单的分组口径。
#[derive(Clone, Copy, PartialEq, Eq)]
enum GroupMode {
    /// 按操作：要修改 / 要创建 / 要删除 / 无操作
    Op,
    /// 按对象类型：表 / 视图 / 实体化视图 / 函数 / 存储过程
    Category,
}

impl GroupMode {
    fn label(self) -> &'static str {
        match self {
            GroupMode::Op => "按操作分组",
            GroupMode::Category => "按对象类型分组",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Setup,
    Compare,
    Deploy,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Src,
    Dst,
}

impl Side {
    fn ix(self) -> usize {
        match self {
            Side::Src => 0,
            Side::Dst => 1,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Side::Src => "源（同步自）",
            Side::Dst => "目标（同步到）",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Conn,
    Db,
    Schema,
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Field::Conn => "连接:",
            Field::Db => "数据库:",
            Field::Schema => "模式:",
        }
    }

    fn top(self) -> f32 {
        match self {
            Field::Conn => 0.,
            Field::Db => BLOCK_H + BLOCK_GAP,
            Field::Schema => 2. * (BLOCK_H + BLOCK_GAP),
        }
    }
}

#[derive(Default)]
struct Picker {
    conn_ix: usize,
    db: String,
    schema: String,
    dbs: Vec<String>,
    schemas: Vec<String>,
    dbs_loading: bool,
    schemas_loading: bool,
}

/// 比较用的源 / 目标定位（后台线程拿到的全是自有字符串）。
#[derive(Clone)]
struct SideSpec {
    conn: Connection,
    db: String,
    schema: String,
}

impl SideSpec {
    fn label(&self) -> String {
        if self.schema.trim().is_empty() {
            format!("{}/{}", self.conn.name, self.db)
        } else {
            format!("{}/{}", self.conn.name, self.place())
        }
    }

    /// 摘要行第二行：`库` 或 `库.模式`。
    fn place(&self) -> String {
        if self.schema.trim().is_empty() {
            self.db.clone()
        } else {
            format!("{}.{}", self.db, self.schema)
        }
    }

    fn target(&self) -> Option<Target> {
        Target::of(&self.conn, &self.db, &self.schema)
    }
}

// ── 开窗与状态 ──────────────────────────────────────────────────

pub fn open_structure_compare(
    parent: Entity<DaroApp>,
    dark: bool,
    cx: &mut App,
) -> Option<Entity<StructureCompare>> {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(760.), px(500.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(1000.), px(660.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("结构同步".into());
    }
    match gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| StructureCompare::new(parent, dark, window, cx))
    }) {
        Ok((_, view)) => Some(view),
        Err(_) => {
            eprintln!("failed to open structure compare window");
            None
        }
    }
}

pub(crate) struct StructureCompare {
    dark: bool,
    _esc: Subscription,
    conns: Vec<Connection>,
    pickers: [Picker; 2],
    open: Option<(Side, Field)>,
    /// 打开下拉时挂进触发器的筛选输入（与查询页同一套组合框）
    combo_input: Entity<InputState>,
    combo_needle: String,
    combo_active: Option<usize>,
    step: Step,
    /// 首屏勾选的比较范围（表 / 视图 / 实体化视图 / 函数 / 存储过程）
    opts: CompareOptions,
    busy: bool,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<(usize, usize)>>,
    progress_done: usize,
    progress_total: usize,
    result: Option<CompareResult>,
    selected: Option<usize>,
    page: u8,
    /// 比较页勾选了哪些对象（部署脚本按勾选生成）
    checked: HashSet<usize>,
    /// 比较页展开了明细的对象
    expanded: HashSet<usize>,
    /// 收起的分组（键按分组口径拼）
    collapsed: HashSet<String>,
    group_mode: GroupMode,
    /// 页脚的一次性提示（配置已保存 / 已载入）
    notice: Option<String>,
    /// 载入配置文件后等待目录清单落地的目标库 / 模式
    pending: [Option<(String, String)>; 2],
    versions: [Option<String>; 2],
    version_gen: [u64; 2],
    needle: String,
    filter: Entity<InputState>,
    error: Option<String>,
    copied: bool,
    /// 部署页当前页签：0 部署脚本 / 1 消息日志
    deploy_tab: u8,
    /// 部署脚本正在目标端执行
    deploying: bool,
    /// 每条语句的回执（部署页「消息日志」）
    deploy_log: Vec<DeployEntry>,
    /// 因连接没开起来 / 首次报错而没执行的语句数
    deploy_skipped: usize,
    /// 整段部署耗时
    deploy_ms: u128,
    db_gen: [u64; 2],
    schema_gen: [u64; 2],
}

impl StructureCompare {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let conns: Vec<Connection> = parent
            .read(cx)
            .conns
            .iter()
            .filter(|c| backend::is_real_conn(c) && supports(c.kind))
            .cloned()
            .collect();
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("筛选对象"));
        cx.subscribe(&filter, |this, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.needle = this.filter.read(cx).value().to_string();
                cx.notify();
            }
        })
        .detach();
        // 下拉筛选输入：set_value 不回发 Change，所以打开时回填当前值不会被当成筛选词
        let combo_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe(&combo_input, |this, input, event: &InputEvent, cx| {
            if let InputEvent::Change = event
                && this.open.is_some()
            {
                this.combo_needle = input.read(cx).value().trim().to_string();
                this.combo_active = None;
                cx.notify();
            }
        })
        .detach();
        // Esc：下拉展开时先收下拉（与查询页组合框一致），否则关窗
        let me = cx.entity().downgrade();
        let this_window = window.window_handle();
        let esc = cx.intercept_keystrokes(move |ev, window, cx| {
            if ev.keystroke.key != "escape" || window.window_handle() != this_window {
                return;
            }
            cx.stop_propagation();
            let Some(this) = me.upgrade() else {
                return;
            };
            if this.read(cx).open.is_some() {
                this.update(cx, |this, cx| this.close_picker(cx));
            } else {
                window.remove_window();
            }
        });
        let mut this = Self {
            dark,
            _esc: esc,
            conns,
            pickers: [Picker::default(), Picker::default()],
            open: None,
            combo_input,
            combo_needle: String::new(),
            combo_active: None,
            step: Step::Setup,
            opts: CompareOptions::default(),
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new((0, 0))),
            progress_done: 0,
            progress_total: 0,
            result: None,
            selected: None,
            page: 0,
            checked: HashSet::new(),
            expanded: HashSet::new(),
            collapsed: HashSet::new(),
            group_mode: GroupMode::Op,
            notice: None,
            pending: [None, None],
            versions: [None, None],
            version_gen: [0, 0],
            needle: String::new(),
            filter,
            error: None,
            copied: false,
            deploy_tab: 0,
            deploying: false,
            deploy_log: Vec::new(),
            deploy_skipped: 0,
            deploy_ms: 0,
            db_gen: [0, 0],
            schema_gen: [0, 0],
        };
        let n = this.conns.len();
        if n > 0 {
            this.pickers[0].conn_ix = 0;
            this.pickers[1].conn_ix = if n > 1 { 1 } else { 0 };
            cx.defer_in(window, |this, _window, cx| {
                this.reload_dbs(Side::Src, cx);
                this.reload_dbs(Side::Dst, cx);
            });
        }
        this
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn picker(&self, side: Side) -> &Picker {
        &self.pickers[side.ix()]
    }

    fn picker_mut(&mut self, side: Side) -> &mut Picker {
        &mut self.pickers[side.ix()]
    }

    fn conn(&self, side: Side) -> Option<&Connection> {
        self.conns.get(self.picker(side).conn_ix)
    }

    /// 无头用例读回两侧选中的连接名与库名。
    #[cfg(test)]
    pub(crate) fn debug_sides(&self) -> [(String, String); 2] {
        [Side::Src, Side::Dst].map(|side| {
            (
                self.conn(side).map(|c| c.name.clone()).unwrap_or_default(),
                self.picker(side).db.clone(),
            )
        })
    }

    /// 无头用例读回比较范围勾选（按 CAT_* 顺序）。
    #[cfg(test)]
    pub(crate) fn debug_opts(&self) -> [bool; 5] {
        [
            self.opts.tables,
            self.opts.views,
            self.opts.matviews,
            self.opts.funcs,
            self.opts.procs,
        ]
    }

    /// 无头用例直接改比较范围，绕开下拉（菜单项会挡住触发按钮的点击坐标）。
    #[cfg(test)]
    pub(crate) fn debug_set_scope(&mut self, tables: bool, views: bool) {
        self.opts.tables = tables;
        self.opts.views = views;
    }

    /// 无头用例读回部署回执：语句数、成功数、是否还在执行。
    #[cfg(test)]
    pub(crate) fn debug_deploy(&self) -> (usize, usize, bool) {
        (
            self.deploy_log.len(),
            self.deploy_log.iter().filter(|e| e.ok).count(),
            self.deploying,
        )
    }

    /// 无头用例读回下拉的临时状态：是否展开、筛选词、键盘高亮、输入框文字。
    #[cfg(test)]
    pub(crate) fn debug_combo(&self, cx: &gpui::App) -> (bool, String, Option<usize>, String) {
        (
            self.open.is_some(),
            self.combo_needle.clone(),
            self.combo_active,
            self.combo_input.read(cx).value().to_string(),
        )
    }

    fn reload_dbs(&mut self, side: Side, cx: &mut Context<Self>) {
        let Some(conn) = self.conn(side).cloned() else {
            return;
        };
        self.db_gen[side.ix()] += 1;
        let ticket = self.db_gen[side.ix()];
        {
            let picker = self.picker_mut(side);
            picker.dbs.clear();
            picker.dbs_loading = true;
            picker.db.clear();
            picker.schemas.clear();
            picker.schema.clear();
            picker.schemas_loading = false;
        }
        self.close_combo();
        self.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { catalog::query_databases(&conn) })
                .await;
            let _ = this.update(cx, |this, cx| this.dbs_loaded(side, ticket, res, cx));
        })
        .detach();
    }

    fn dbs_loaded(
        &mut self,
        side: Side,
        ticket: u64,
        res: Result<Vec<String>, String>,
        cx: &mut Context<Self>,
    ) {
        if ticket != self.db_gen[side.ix()] {
            return;
        }
        match res {
            Ok(list) => {
                let default = self
                    .conn(side)
                    .map(|c| c.database.clone())
                    .unwrap_or_default();
                let wanted = self.pending[side.ix()].clone();
                let picker = self.picker_mut(side);
                picker.dbs_loading = false;
                picker.dbs = list;
                picker.db = match wanted.as_ref() {
                    Some((db, _)) if picker.dbs.iter().any(|d| d == db) => db.clone(),
                    _ if picker.dbs.iter().any(|d| d == &default) => default,
                    _ => picker.dbs.first().cloned().unwrap_or_default(),
                };
                self.reload_schemas(side, cx);
                self.reload_version(side, cx);
            }
            Err(e) => {
                let picker = self.picker_mut(side);
                picker.dbs_loading = false;
                picker.dbs.clear();
                self.error = Some(format!("读取数据库清单失败：{e}"));
            }
        }
        cx.notify();
    }

    fn reload_schemas(&mut self, side: Side, cx: &mut Context<Self>) {
        let Some(conn) = self.conn(side).cloned() else {
            return;
        };
        if !has_schema(conn.kind) {
            let picker = self.picker_mut(side);
            picker.schemas.clear();
            picker.schema.clear();
            picker.schemas_loading = false;
            self.pending[side.ix()] = None;
            return;
        }
        let db = self.picker(side).db.clone();
        if db.is_empty() {
            return;
        }
        self.schema_gen[side.ix()] += 1;
        let ticket = self.schema_gen[side.ix()];
        self.picker_mut(side).schemas_loading = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { catalog::query_schemas(&conn, &db) })
                .await;
            let _ = this.update(cx, |this, cx| this.schemas_loaded(side, ticket, res, cx));
        })
        .detach();
    }

    fn schemas_loaded(
        &mut self,
        side: Side,
        ticket: u64,
        res: Result<Vec<String>, String>,
        cx: &mut Context<Self>,
    ) {
        if ticket != self.schema_gen[side.ix()] {
            return;
        }
        let wanted = self.pending[side.ix()].take().map(|(_, schema)| schema);
        let picker = self.picker_mut(side);
        picker.schemas_loading = false;
        match res {
            Ok(list) => {
                picker.schemas = list;
                picker.schema = match wanted {
                    Some(s) if !s.trim().is_empty() && picker.schemas.iter().any(|x| x == &s) => s,
                    _ if picker.schemas.iter().any(|s| s == "public") => "public".to_string(),
                    _ => picker.schemas.first().cloned().unwrap_or_default(),
                };
            }
            Err(e) => {
                picker.schemas.clear();
                picker.schema.clear();
                self.error = Some(format!("读取模式清单失败：{e}"));
            }
        }
        cx.notify();
    }

    /// 连接信息里的「服务器版本」：引擎一条查询，读不到就显示 `--`。
    fn reload_version(&mut self, side: Side, cx: &mut Context<Self>) {
        let Some(conn) = self.conn(side).cloned() else {
            return;
        };
        let db = self.picker(side).db.clone();
        let schema = self.picker(side).schema.clone();
        self.version_gen[side.ix()] += 1;
        let ticket = self.version_gen[side.ix()];
        self.versions[side.ix()] = None;
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { query_version(&conn, &db, &schema) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if ticket == this.version_gen[side.ix()] {
                    this.versions[side.ix()] = res;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn swap_sides(&mut self, cx: &mut Context<Self>) {
        let [a, b] = &mut self.pickers;
        std::mem::swap(a, b);
        // 换代计数跟着换：对调前还在路上的目录请求落地时会因编号不符被丢掉
        self.db_gen.swap(0, 1);
        self.schema_gen.swap(0, 1);
        self.close_combo();
        self.error = None;
        cx.notify();
    }

    fn side_specs(&self) -> Option<(SideSpec, SideSpec)> {
        let spec = |side: Side| -> Option<SideSpec> {
            let conn = self.conn(side)?.clone();
            let picker = self.picker(side);
            if picker.db.trim().is_empty() {
                return None;
            }
            if has_schema(conn.kind) && picker.schema.trim().is_empty() {
                return None;
            }
            Some(SideSpec {
                conn,
                db: picker.db.clone(),
                schema: picker.schema.clone(),
            })
        };
        Some((spec(Side::Src)?, spec(Side::Dst)?))
    }

    fn start_compare(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some((src, dst)) = self.side_specs() else {
            self.error = Some("请先为源和目标各选好数据库（PostgreSQL 还要选模式）".into());
            cx.notify();
            return;
        };
        if src.conn.kind != dst.conn.kind {
            self.error = Some("源和目标必须是同一种数据库引擎".into());
            cx.notify();
            return;
        }
        if src.conn.name == dst.conn.name && src.db == dst.db && src.schema == dst.schema {
            self.error = Some("源与目标是同一个库，换个目标再比较".into());
            cx.notify();
            return;
        }
        if src.target().is_none() || dst.target().is_none() {
            self.error = Some("该连接没有可用的查询驱动".into());
            cx.notify();
            return;
        }
        let opts = self.opts;
        if opts.cats(src.conn.kind).is_empty() {
            self.error = Some("请至少勾选一类要比较的对象".into());
            cx.notify();
            return;
        }
        self.error = None;
        self.busy = true;
        self.copied = false;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.progress = Arc::new(Mutex::new((0, 0)));
        self.progress_done = 0;
        self.progress_total = 0;
        cx.notify();
        let cancel = self.cancel.clone();
        let progress = self.progress.clone();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { compare_objects(&src, &dst, opts, &progress, &cancel) })
                .await;
            let _ = this.update(cx, |this, cx| this.compare_done(res, cx));
        })
        .detach();
        self.track_progress(cx);
    }

    fn track_progress(&mut self, cx: &mut Context<Self>) {
        let progress = self.progress.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let mut stop = true;
                let applied = this.update(cx, |this, cx| {
                    stop = !this.busy;
                    let (done, total) = *progress.lock().unwrap();
                    this.progress_done = done;
                    this.progress_total = total;
                    cx.notify();
                });
                if applied.is_err() || stop {
                    break;
                }
            }
        })
        .detach();
    }

    fn compare_done(&mut self, res: Result<CompareResult, String>, cx: &mut Context<Self>) {
        self.busy = false;
        match res {
            Ok(r) => {
                // 默认勾选所有有改动的对象；无差异的也列出来，但默认不部署
                self.checked = r
                    .objects
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| o.status != DiffStatus::Same)
                    .map(|(ix, _)| ix)
                    .collect();
                self.selected = r.objects.iter().position(|o| o.status != DiffStatus::Same);
                self.result = Some(r);
                self.step = Step::Compare;
                self.expanded.clear();
                self.collapsed.clear();
                self.page = 0;
                self.deploy_tab = 0;
                self.deploy_log.clear();
                self.deploy_skipped = 0;
                self.deploy_ms = 0;
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    fn back_to_setup(&mut self, cx: &mut Context<Self>) {
        self.step = Step::Setup;
        self.error = None;
        self.copied = false;
        cx.notify();
    }

    fn toggle_checked(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.checked.remove(&ix) {
            self.checked.insert(ix);
        }
        self.copied = false;
        cx.notify();
    }

    fn all_checked(&self, indices: &[usize]) -> bool {
        !indices.is_empty() && indices.iter().all(|ix| self.checked.contains(ix))
    }

    fn toggle_group_checked(&mut self, indices: &[usize], cx: &mut Context<Self>) {
        let all = self.all_checked(indices);
        for ix in indices {
            if all {
                self.checked.remove(ix);
            } else {
                self.checked.insert(*ix);
            }
        }
        self.copied = false;
        cx.notify();
    }

    /// 部署脚本：只拼勾选对象（目标端多出的对象脚本本身就是注释掉的 DROP）。
    fn deploy_script(&self) -> String {
        let Some(result) = self.result.as_ref() else {
            return String::new();
        };
        let kind = self
            .side_specs()
            .map(|(src, _)| src.conn.kind)
            .unwrap_or(DbKind::PostgreSql);
        deploy_script_text(kind, result, &self.checked)
    }

    /// 部署脚本里真正要执行的语句：跳过目标端多出的对象那种纯注释的
    /// `-- DROP ...`，以及拼接时带进来的空行。
    fn deploy_statements(&self) -> Vec<String> {
        crate::split_sql_statements(&self.deploy_script())
            .into_iter()
            .filter(|stmt| has_sql_code(stmt))
            .collect()
    }

    fn copy_deploy_script(&mut self, cx: &mut Context<Self>) {
        let text = self.deploy_script();
        if text.trim().is_empty() {
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.copied = true;
        cx.notify();
    }

    fn next_to_deploy(&mut self, cx: &mut Context<Self>) {
        if self.result.is_none() {
            return;
        }
        self.copied = false;
        self.deploy_tab = 0;
        self.step = Step::Deploy;
        cx.notify();
    }

    fn back_to_compare(&mut self, cx: &mut Context<Self>) {
        self.step = Step::Compare;
        self.error = None;
        cx.notify();
    }

    /// 在目标端逐条执行部署脚本；回执攒进「消息日志」，界面先切到日志页。
    fn start_deploy(&mut self, cx: &mut Context<Self>) {
        if self.deploying {
            return;
        }
        let Some((_, dst)) = self.side_specs() else {
            self.error = Some("目标库信息已失效，请重新比较".into());
            cx.notify();
            return;
        };
        let Some(tg) = dst.target() else {
            self.error = Some("目标连接没有可用的查询驱动".into());
            cx.notify();
            return;
        };
        let stmts = self.deploy_statements();
        if stmts.is_empty() {
            self.error = Some("没有要执行的语句：先在比较页勾选对象".into());
            cx.notify();
            return;
        }
        self.error = None;
        self.deploying = true;
        self.deploy_log.clear();
        self.deploy_skipped = 0;
        self.deploy_ms = 0;
        self.deploy_tab = 1;
        cx.notify();
        let total = stmts.len();
        cx.spawn(async move |this, cx| {
            let (run, _) = cx
                .background_executor()
                .spawn(async move {
                    let job = tg.create_job();
                    tg.run_script(&job, &stmts, false, None)
                })
                .await;
            let _ = this.update(cx, |this, cx| this.deploy_done(run, total, cx));
        })
        .detach();
    }

    fn deploy_done(
        &mut self,
        run: crate::db_types::ScriptRun,
        total: usize,
        cx: &mut Context<Self>,
    ) {
        self.deploying = false;
        self.deploy_ms = run.total_ms;
        if let Some(e) = run.open_error {
            self.error = Some(e);
            self.deploy_skipped = total;
            cx.notify();
            return;
        }
        // 语句文本按执行顺序与回执一一对应；run 里只有跑到的那些
        let stmts = self.deploy_statements();
        let executed = run.runs.len().min(stmts.len());
        self.deploy_log = run
            .runs
            .into_iter()
            .zip(stmts.iter().take(executed))
            .map(|(r, stmt)| DeployEntry {
                ok: r.result.is_ok(),
                stmt: stmt.clone(),
                msg: match &r.result {
                    Ok(o) if !o.message.trim().is_empty() => o.message.clone(),
                    Ok(o) => format!("影响 {} 行", o.affected),
                    Err(e) => e.clone(),
                },
                ms: r.elapsed_ms,
            })
            .collect();
        self.deploy_skipped = total.saturating_sub(executed);
        if run.cancelled && self.deploy_skipped == 0 {
            self.deploy_skipped = total.saturating_sub(executed);
        }
        if let Some(f) = self.deploy_log.iter().find(|e| !e.ok) {
            self.error = Some(format!("部署有失败语句：{}", f.msg));
        } else {
            self.notice = Some(format!(
                "部署完成：{executed} 条语句，{} ms",
                self.deploy_ms
            ));
        }
        cx.notify();
    }

    fn save_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((src, dst)) = self.side_specs() else {
            self.error = Some("先把源和目标选好再保存配置文件".into());
            cx.notify();
            return;
        };
        let data = ProfileFile {
            version: 1,
            source: ProfileSide {
                connection: src.conn.name.clone(),
                db: src.db.clone(),
                schema: src.schema.clone(),
            },
            target: ProfileSide {
                connection: dst.conn.name.clone(),
                db: dst.db.clone(),
                schema: dst.schema.clone(),
            },
            tables: self.opts.tables,
            views: self.opts.views,
            matviews: self.opts.matviews,
            funcs: self.opts.funcs,
            procs: self.opts.procs,
        };
        let dialog = rfd::AsyncFileDialog::new()
            .set_title("保存结构同步配置")
            .add_filter("结构同步配置", &["json"])
            .set_file_name("结构同步.json");
        ui::save_file(window, dialog, cx, move |this, _window, cx, path| {
            let written = serde_json::to_string_pretty(&data)
                .map_err(|e| e.to_string())
                .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()));
            match written {
                Ok(()) => {
                    this.error = None;
                    this.notice = Some(format!("配置已保存：{}", path.display()));
                }
                Err(e) => this.error = Some(format!("保存配置文件失败：{e}")),
            }
            cx.notify();
        });
    }

    fn load_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = rfd::AsyncFileDialog::new()
            .set_title("加载结构同步配置")
            .add_filter("结构同步配置", &["json"]);
        ui::pick_file(window, dialog, cx, |this, _window, cx, path| {
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) => {
                    this.error = Some(format!("读取配置文件失败：{e}"));
                    cx.notify();
                    return;
                }
            };
            let data: ProfileFile = match serde_json::from_str(&text) {
                Ok(data) => data,
                Err(e) => {
                    this.error = Some(format!("配置文件格式不对：{e}"));
                    cx.notify();
                    return;
                }
            };
            let find = |name: &str| this.conns.iter().position(|c| c.name == name);
            let (Some(src_ix), Some(dst_ix)) =
                (find(&data.source.connection), find(&data.target.connection))
            else {
                this.error = Some("配置文件里的连接不在当前连接列表里".into());
                cx.notify();
                return;
            };
            this.pickers[0].conn_ix = src_ix;
            this.pickers[1].conn_ix = dst_ix;
            this.opts = CompareOptions {
                tables: data.tables,
                views: data.views,
                matviews: data.matviews,
                funcs: data.funcs,
                procs: data.procs,
            };
            this.pending = [
                Some((data.source.db, data.source.schema)),
                Some((data.target.db, data.target.schema)),
            ];
            this.step = Step::Setup;
            this.error = None;
            this.notice = Some(format!("已载入配置：{}", path.display()));
            this.reload_dbs(Side::Src, cx);
            this.reload_dbs(Side::Dst, cx);
        });
    }

    // ── 首屏：选源与目标 ────────────────────────────────────────

    fn setup_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let dark = self.dark;
        if self.conns.is_empty() {
            return v_flex()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .p(px(24.))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(fg)
                        .child("还没有可用于结构比较的连接"),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(muted)
                        .child("结构比较需要 MySQL / PostgreSQL / SQLite 的真实连接；演示连接不连库，不参与比较。"),
                )
                .child(
                    ui::secondary_button("sc-empty-close", "关闭")
                        .on_click(cx.listener(|_, _, window, _| window.remove_window())),
                )
                .into_any_element();
        }
        let cols = h_flex()
            .flex_1()
            .min_h_0()
            .gap(px(14.))
            // h_flex 默认垂直居中，两栏字段数不同会高低错开
            .items_stretch()
            .child(self.side_column(Side::Src, cx))
            .child(self.swap_button(cx))
            .child(self.side_column(Side::Dst, cx));
        let kind = self.conn(Side::Src).map(|c| c.kind);
        let busy = self.busy;
        let progress = if self.progress_total > 0 {
            format!("比较中… {}/{}", self.progress_done, self.progress_total)
        } else {
            "比较中…".to_string()
        };
        let (status, status_color) = if let Some(e) = self.error.as_ref() {
            (e.clone(), bad_color(dark))
        } else if let Some(n) = self.notice.as_ref() {
            (n.clone(), muted)
        } else {
            (
                "脚本只供复核，daro 不会自动执行；触发器与数据不参与比较。".to_string(),
                muted,
            )
        };
        let mut footer = ui::footer_bar(dark)
            .child(
                ui::secondary_button("sc-save-profile", "保存配置文件")
                    .on_click(cx.listener(|this, _, window, cx| this.save_profile(window, cx))),
            )
            .child(
                ui::secondary_button("sc-load-profile", "加载配置文件")
                    .on_click(cx.listener(|this, _, window, cx| this.load_profile(window, cx))),
            )
            .child(self.options_button(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(11.))
                    .text_color(status_color)
                    .truncate()
                    .child(status),
            );
        if busy {
            footer = footer
                .child(
                    ui::secondary_button("sc-cancel", "取消").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.cancel.store(true, Ordering::Relaxed);
                            cx.notify();
                        }),
                    ),
                )
                .child(ui::primary_button_enabled(
                    "sc-run", progress, dark, false, cx,
                ));
        } else {
            footer = footer.child(
                ui::primary_button_enabled(
                    "sc-run",
                    "比较",
                    dark,
                    self.side_specs().is_some()
                        && kind.is_some_and(|k| !self.opts.cats(k).is_empty()),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.start_compare(cx))),
            );
        }
        v_flex()
            .id("sc-setup")
            .test_support()
            .flex_1()
            .min_h_0()
            .p(px(16.))
            .gap(px(12.))
            // 点页面空白收起字段下拉（触发框与面板自己拦冒泡）
            .on_click(cx.listener(|this, _, _, cx| {
                if this.open.is_some() {
                    this.close_picker(cx);
                }
            }))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(self.setup_summary()),
            )
            .child(cols)
            .child(footer)
            .into_any_element()
    }

    /// 首屏顶部的源 → 目标摘要（库名与模式随选择实时更新）。
    fn setup_summary(&self) -> AnyElement {
        let conn_name = |side: Side| {
            self.conn(side)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "未选择".to_string())
        };
        let place = |side: Side| {
            let picker = self.picker(side);
            let kind = self.conn(side).map(|c| c.kind);
            if picker.db.trim().is_empty() {
                "未选择数据库".to_string()
            } else if kind.is_some_and(has_schema) && !picker.schema.trim().is_empty() {
                format!("{}.{}", picker.db, picker.schema)
            } else {
                picker.db.clone()
            }
        };
        self.summary_row(
            &conn_name(Side::Src),
            &place(Side::Src),
            &conn_name(Side::Dst),
            &place(Side::Dst),
        )
    }

    /// 源（绿）→ 目标（蓝）两段式摘要，首屏与比较页共用。
    fn summary_row(
        &self,
        src_name: &str,
        src_place: &str,
        dst_name: &str,
        dst_place: &str,
    ) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let dst_color = to_hsla(if self.dark { 0x6cb2ff } else { 0x2f6fd0 });
        let side_el = |name: &str, place: &str, color: Hsla| -> AnyElement {
            h_flex()
                .gap(px(6.))
                .items_center()
                .child(
                    Icon::new(ZedIcon::Database)
                        .with_size(px(16.))
                        .text_color(color),
                )
                .child(
                    v_flex()
                        .gap(px(0.))
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(fg)
                                .child(name.to_string()),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(muted)
                                .child(place.to_string()),
                        ),
                )
                .into_any_element()
        };
        h_flex()
            .gap(px(10.))
            .items_center()
            .child(side_el(src_name, src_place, ok_color(self.dark)))
            .child(zed_icons::ic(ZedIcon::ArrowRight, 14., muted))
            .child(side_el(dst_name, dst_place, dst_color))
            .into_any_element()
    }

    fn swap_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        div()
            .w(px(30.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("sc-swap")
                    .test_support()
                    .w(px(26.))
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .border(px(1.))
                    .border_color(to_hsla(p.dialog_line))
                    .cursor_pointer()
                    .hover(|el| el.bg(to_hsla(p.hover)))
                    .on_click(cx.listener(|this, _, _, cx| this.swap_sides(cx)))
                    .child(
                        Icon::new(ZedIcon::ArrowRightLeft)
                            .with_size(px(13.))
                            .text_color(to_hsla(p.muted_foreground)),
                    ),
            )
            .into_any_element()
    }

    /// 页脚「选项」：比较范围按源连接的引擎收放（两边引擎必须一致，比较时再校验）。
    fn options_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let kind = self.conn(Side::Src).map(|c| c.kind);
        let opts = self.opts;
        let me = cx.entity();
        let base = Button::new("sc-options")
            .ghost()
            .with_size(gpui_kit::component::Size::XSmall)
            .h(px(26.))
            .px(px(10.))
            .gap(px(5.))
            .rounded(px(4.))
            .border(px(1.))
            .border_color(to_hsla(if kind.is_some() {
                p.button_border
            } else {
                p.border_soft
            }))
            .text_size(px(12.))
            .text_color(if kind.is_some() { fg } else { muted })
            .child(div().child("选项"))
            .child(zed_icons::ic(ZedIcon::ChevronDown, 10., muted));
        let Some(kind) = kind else {
            return base.opacity(0.5).into_any_element();
        };
        base.cursor_pointer()
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                for cat in [CAT_TABLE, CAT_VIEW, CAT_MATVIEW, CAT_FUNC, CAT_PROC] {
                    if !cat_supported(kind, cat) {
                        continue;
                    }
                    let me = me.clone();
                    menu = menu.item(
                        PopupMenuItem::new(cat_title(cat))
                            .checked(opts.get(cat))
                            .on_click(move |_, _, cx| {
                                me.update(cx, |this, cx| {
                                    this.opts.toggle(cat);
                                    this.error = None;
                                    this.notice = None;
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    fn side_column(&self, side: Side, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let conn_kind = self.conn(side).map(|c| c.kind);
        let mut col = v_flex()
            .id(if side == Side::Src {
                "sc-src"
            } else {
                "sc-dst"
            })
            .test_support()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .flex_none()
                    .h(px(SIDE_TITLE_H))
                    .text_size(px(12.5))
                    .text_color(fg)
                    .child(side.title()),
            );
        for field in [Field::Conn, Field::Db, Field::Schema] {
            if field == Field::Schema && !conn_kind.map(has_schema).unwrap_or(false) {
                continue;
            }
            col = col.child(self.picker_field(side, field, cx));
        }
        col = col.child(self.info_box(side));
        // 面板底下的说明行，撑满剩余高度，让底部按钮落在页脚
        let note = match conn_kind {
            Some(DbKind::MySql) => "MySQL：库下面直接是表和视图。",
            Some(DbKind::PostgreSql) => "PostgreSQL：按模式比较，默认 public。",
            Some(DbKind::SQLite) => "SQLite：比较连接指向的库文件。",
            _ => "",
        };
        col = col.child(
            div()
                .flex_1()
                .min_h_0()
                .pt(px(8.))
                .text_size(px(11.))
                .text_color(muted)
                .child(note),
        );
        // 下拉面板放在最后画，盖住信息块与下方内容
        if let Some((open_side, open_field)) = self.open
            && open_side == side
        {
            col = col.child(self.picker_popup(side, open_field, cx));
        }
        col.into_any_element()
    }

    /// 连接信息块：云名称 / 项目名称 / 连接类型 / 连接名称 / 主机 / 端口 / 服务器版本。
    fn info_box(&self, side: Side) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let conn = self.conn(side);
        let text = |v: Option<String>| v.unwrap_or_else(|| "--".to_string());
        let host = conn.map(|c| match c.kind {
            DbKind::SQLite => "本地文件".to_string(),
            _ if c.host.trim().is_empty() => "--".to_string(),
            _ => c.host.clone(),
        });
        let port = conn.map(|c| match c.kind {
            _ if c.port == 0 => "--".to_string(),
            _ => c.port.to_string(),
        });
        let rows = [
            ("云名称", "--".to_string()),
            (
                "项目名称",
                conn.and_then(|c| (!c.group.trim().is_empty()).then(|| c.group.clone()))
                    .unwrap_or_else(|| "--".to_string()),
            ),
            (
                "连接类型",
                conn.map(|c| kind_title(c.kind).to_string())
                    .unwrap_or_else(|| "--".to_string()),
            ),
            ("连接名称", text(conn.map(|c| c.name.clone()))),
            ("主机", text(host)),
            ("端口", text(port)),
            ("服务器版本", text(self.versions[side.ix()].clone())),
        ];
        let count = rows.len();
        v_flex()
            .flex_none()
            .mt(px(10.))
            .rounded(px(4.))
            .border(px(1.))
            .border_color(to_hsla(p.dialog_line))
            .bg(to_hsla(p.background))
            .children(rows.into_iter().enumerate().map(|(ix, (label, value))| {
                h_flex()
                    .h(px(23.))
                    .px(px(8.))
                    .items_center()
                    .gap(px(8.))
                    .when(ix + 1 < count, |el| {
                        el.border_b(px(1.)).border_color(to_hsla(p.border_soft))
                    })
                    .child(
                        div()
                            .w(px(72.))
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(muted)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(11.5))
                            .text_color(fg)
                            .truncate()
                            .child(value),
                    )
            }))
            .into_any_element()
    }

    fn picker_field(&self, side: Side, field: Field, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let muted = to_hsla(p.muted_foreground);
        let picker = self.picker(side);
        let (icon, text) = match field {
            Field::Conn => (
                self.conn(side)
                    .map(|c| db_icons::engine_bar(c.kind, 14.))
                    .unwrap_or_else(|| div().w(px(16.)).h(px(16.)).into_any_element()),
                self.conn(side)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "选择连接".into()),
            ),
            Field::Db => {
                if picker.dbs_loading {
                    (
                        div().w(px(16.)).h(px(16.)).into_any_element(),
                        "载入中…".to_string(),
                    )
                } else if picker.db.is_empty() {
                    (
                        div().w(px(16.)).h(px(16.)).into_any_element(),
                        "选择数据库".to_string(),
                    )
                } else {
                    (db_icons::database(14.), picker.db.clone())
                }
            }
            Field::Schema => {
                let icon = crate::art::icon(crate::art::Glyph::Schema, 14., muted);
                if picker.schemas_loading {
                    (
                        div().w(px(16.)).h(px(16.)).into_any_element(),
                        "载入中…".to_string(),
                    )
                } else if picker.schema.is_empty() {
                    (
                        div().w(px(16.)).h(px(16.)).into_any_element(),
                        "选择模式".to_string(),
                    )
                } else {
                    (icon, picker.schema.clone())
                }
            }
        };
        let open = self.open == Some((side, field));
        let field_ix = field as usize;
        v_flex()
            .id(format!("sc-{}-{}", side.ix(), field_ix))
            .test_support()
            .flex_none()
            .h(px(BLOCK_H + BLOCK_GAP))
            .child(
                div()
                    .h(px(LABEL_H))
                    .text_size(px(11.))
                    .line_height(px(LABEL_H))
                    .text_color(muted)
                    .child(field.label()),
            )
            .child(combo::trigger(
                combo::TriggerSpec {
                    id: format!("sc-{}-{}-trigger", side.ix(), field_ix),
                    width: None,
                    icon,
                    label: text,
                    open,
                    // 打开时正文换成筛选输入框，与查询页组合框完全同一套
                    input: open.then(|| self.combo_input.clone()),
                },
                self.combo_colors(),
                cx,
                // 展开时点正文是给输入框落光标，箭头负责开 / 关
                move |this, window, cx| {
                    if !open {
                        this.toggle_picker(side, field, window, cx);
                    }
                    cx.stop_propagation();
                },
                move |this, window, cx| {
                    if open {
                        this.close_picker(cx);
                    } else {
                        this.toggle_picker(side, field, window, cx);
                    }
                    cx.stop_propagation();
                },
                move |this, ev: &KeyDownEvent, _window, cx| match ev.keystroke.key.as_str() {
                    "escape" => {
                        this.close_picker(cx);
                        cx.stop_propagation();
                    }
                    "enter" => {
                        this.combo_commit(side, field, cx);
                        cx.stop_propagation();
                    }
                    "up" => {
                        this.combo_move(side, field, false, cx);
                        cx.stop_propagation();
                    }
                    "down" => {
                        this.combo_move(side, field, true, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                },
            ))
            .into_any_element()
    }

    /// 组合框展开：清筛选词、高亮当前值，并把当前值回填进触发器里的输入框
    /// （全选，键入即替换），焦点交给输入框——与查询页工具栏下拉同一流程。
    fn toggle_picker(
        &mut self,
        side: Side,
        field: Field,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open == Some((side, field)) {
            self.close_picker(cx);
            return;
        }
        self.open = Some((side, field));
        self.combo_needle.clear();
        let current = self.picker_value(side, field);
        self.combo_active = self
            .combo_candidates(side, field)
            .iter()
            .position(|n| *n == current);
        self.combo_input.update(cx, |s, cx| {
            s.set_value(current, window, cx);
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        cx.notify();
    }

    fn close_picker(&mut self, cx: &mut Context<Self>) {
        self.close_combo();
        cx.notify();
    }

    /// 收起下拉并清掉临时筛选状态（换连接、点空白、点选候选项都走这里）。
    fn close_combo(&mut self) {
        self.open = None;
        self.combo_needle.clear();
        self.combo_active = None;
    }

    fn picker_value(&self, side: Side, field: Field) -> String {
        match field {
            Field::Conn => self.conn(side).map(|c| c.name.clone()).unwrap_or_default(),
            Field::Db => self.picker(side).db.clone(),
            Field::Schema => self.picker(side).schema.clone(),
        }
    }

    /// 完整候选清单（筛选前的连接名 / 库名 / 模式名）。
    fn combo_candidates(&self, side: Side, field: Field) -> Vec<String> {
        match field {
            Field::Conn => self.conns.iter().map(|c| c.name.clone()).collect(),
            Field::Db => self.picker(side).dbs.clone(),
            Field::Schema => self.picker(side).schemas.clone(),
        }
    }

    fn combo_filtered(&self, side: Side, field: Field) -> Vec<String> {
        let names = self.combo_candidates(side, field);
        if self.combo_needle.is_empty() {
            names
        } else {
            names
                .into_iter()
                .filter(|n| crate::contains_ci(n, &self.combo_needle))
                .collect()
        }
    }

    /// 方向键在筛选后的候选里移动高亮；还没高亮时从首 / 尾进入。
    fn combo_move(&mut self, side: Side, field: Field, down: bool, cx: &mut Context<Self>) {
        let n = self.combo_filtered(side, field).len();
        if n == 0 {
            return;
        }
        self.combo_active = Some(match (self.combo_active, down) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1).min(n - 1),
            (Some(i), false) => i.saturating_sub(1),
        });
        cx.notify();
    }

    /// 回车确认：高亮项优先，否则取筛选后的第一项；空列表只收起。
    fn combo_commit(&mut self, side: Side, field: Field, cx: &mut Context<Self>) {
        let names = self.combo_filtered(side, field);
        let name = self
            .combo_active
            .and_then(|i| names.get(i).cloned())
            .or_else(|| {
                if self.combo_active.is_none() {
                    names.first().cloned()
                } else {
                    None
                }
            });
        match name {
            Some(name) => self.pick(side, field, &name, cx),
            None => self.close_picker(cx),
        }
    }

    /// 首屏字段下拉沿用查询页的组合框配色。
    fn combo_colors(&self) -> combo::Colors {
        let p = self.pal();
        combo::Colors {
            trigger_bg: to_hsla(p.button_face),
            menu_bg: to_hsla(p.dialog_surface),
            text: to_hsla(p.foreground),
            muted: to_hsla(p.muted_foreground),
            edge: to_hsla(p.field_border),
            focus: to_hsla(p.field_focus),
            selected: to_hsla(p.selection),
            hover: to_hsla(p.hover),
            highlight: to_hsla(p.accent).opacity(0.35),
        }
    }

    fn picker_popup(&self, side: Side, field: Field, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let muted = to_hsla(p.muted_foreground);
        let colors = self.combo_colors();
        let all = self.combo_candidates(side, field);
        let mut items: Vec<(String, Option<AnyElement>)> = match field {
            Field::Conn => self
                .conns
                .iter()
                .map(|c| (c.name.clone(), Some(db_icons::engine_bar(c.kind, 14.))))
                .collect(),
            Field::Db => self
                .picker(side)
                .dbs
                .iter()
                .map(|d| (d.clone(), Some(db_icons::database(14.))))
                .collect(),
            Field::Schema => self
                .picker(side)
                .schemas
                .iter()
                .map(|s| {
                    (
                        s.clone(),
                        Some(crate::art::icon(crate::art::Glyph::Schema, 14., muted)),
                    )
                })
                .collect(),
        };
        // 键入即过滤：大小写不敏感的子串匹配，与查询页组合框同口径
        if !self.combo_needle.is_empty() {
            items.retain(|(label, _)| crate::contains_ci(label, &self.combo_needle));
        }
        let loading = self.picker(side).dbs_loading || self.picker(side).schemas_loading;
        let empty = items.is_empty();
        if empty {
            let hint = if loading {
                "载入中…"
            } else if all.is_empty() {
                "（没有可选项）"
            } else {
                "（没有匹配项）"
            };
            items.push((hint.to_string(), None));
        }
        let rows: Vec<AnyElement> = items
            .into_iter()
            .enumerate()
            .map(|(ix, (label, icon))| {
                let label_for_click = label.clone();
                combo::row(
                    combo::RowSpec {
                        id: format!("sc-pop-{}-{}", side.ix(), label),
                        label: label.clone(),
                        icon,
                        enabled: !empty,
                        highlight: if empty {
                            None
                        } else {
                            crate::find_ci(&label, &self.combo_needle)
                        },
                        active: !empty && self.combo_active == Some(ix),
                        hint: None,
                    },
                    colors,
                    cx,
                    move |this, _, cx| {
                        if !empty {
                            this.pick(side, field, &label_for_click, cx);
                        }
                    },
                )
            })
            .collect();
        // 面板按行数定高并封顶：外层给确定高度，滚动容器才知道自己在哪裁切，
        // 否则候选一多就整串画到页面上、和底下的说明 / 页脚叠在一起
        let height = (rows.len() as f32 * combo::ROW_H + 2. * combo::PAD_Y + 1.).min(MENU_MAX_H);
        // 外层只负责定位：绝对定位不能挂在滚动容器上（Scrollable 只搬运尺寸样式）
        div()
            .id(format!("sc-pop-{}-{}", side.ix(), field as usize))
            .test_support()
            .absolute()
            // 侧栏内容原点：标题占一行，字段块在它下面；面板压在触发框下沿
            .top(px(SIDE_TITLE_H + field.top() + BLOCK_H))
            .left(px(0.))
            .right(px(0.))
            .h(px(height))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(combo::panel(rows, combo::PanelFit::Fill, colors))
            .into_any_element()
    }

    fn pick(&mut self, side: Side, field: Field, label: &str, cx: &mut Context<Self>) {
        self.close_combo();
        self.error = None;
        match field {
            Field::Conn => {
                let Some(ix) = self.conns.iter().position(|c| c.name == label) else {
                    return;
                };
                if self.picker(side).conn_ix == ix && !self.picker(side).db.is_empty() {
                    cx.notify();
                    return;
                }
                self.picker_mut(side).conn_ix = ix;
                self.reload_dbs(side, cx);
            }
            Field::Db => {
                if self.picker(side).db == label {
                    cx.notify();
                    return;
                }
                self.picker_mut(side).db = label.to_string();
                self.picker_mut(side).schemas.clear();
                self.picker_mut(side).schema.clear();
                self.reload_schemas(side, cx);
            }
            Field::Schema => {
                self.picker_mut(side).schema = label.to_string();
                cx.notify();
            }
        }
    }

    // ── 第二页：对象清单 + 脚本 / DDL ──────────────────────────

    fn compare_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let line = to_hsla(p.dialog_line);
        let dark = self.dark;
        let Some(result) = self.result.as_ref() else {
            return div().flex_1().into_any_element();
        };
        let (src_name, src_place, dst_name, dst_place) = (
            result.src_name.clone(),
            result.src_place.clone(),
            result.dst_name.clone(),
            result.dst_place.clone(),
        );
        let warnings = result.warnings.clone();
        let mode = self.group_mode;
        let me = cx.entity();
        let mode_btn = Button::new("sc-group-mode")
            .ghost()
            .with_size(gpui_kit::component::Size::XSmall)
            .h(px(24.))
            .px(px(8.))
            .gap(px(5.))
            .rounded(px(4.))
            .border(px(1.))
            .border_color(to_hsla(p.button_border))
            .text_size(px(11.5))
            .text_color(fg)
            .cursor_pointer()
            .child(div().child(mode.label()))
            .child(zed_icons::ic(ZedIcon::ChevronDown, 10., muted))
            .dropdown_menu(move |menu, _, _| {
                let me_op = me.clone();
                let me_cat = me.clone();
                menu.item(
                    PopupMenuItem::new("按操作分组")
                        .checked(mode == GroupMode::Op)
                        .on_click(move |_, _, cx| {
                            me_op.update(cx, |this, cx| {
                                this.group_mode = GroupMode::Op;
                                cx.notify();
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new("按对象类型分组")
                        .checked(mode == GroupMode::Category)
                        .on_click(move |_, _, cx| {
                            me_cat.update(cx, |this, cx| {
                                this.group_mode = GroupMode::Category;
                                cx.notify();
                            });
                        }),
                )
            });
        let top = h_flex()
            .flex_none()
            .w_full()
            .min_w_0()
            .h(px(38.))
            .px(px(10.))
            .gap(px(8.))
            .items_center()
            .border_b(px(1.))
            .border_color(line)
            .child(div().w(px(220.)).flex_none().child(mode_btn))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .justify_center()
                    .child(self.summary_row(&src_name, &src_place, &dst_name, &dst_place)),
            )
            .child(
                div().w(px(220.)).flex_none().flex().justify_end().child(
                    div().w(px(170.)).h(px(24.)).child(
                        Input::new(&self.filter)
                            .id("sc-filter")
                            .appearance(false)
                            .text_size(px(11.5))
                            .h(px(24.)),
                    ),
                ),
            );
        let header = h_flex()
            .flex_none()
            .w_full()
            .min_w_0()
            .h(px(26.))
            .px(px(10.))
            .gap(px(6.))
            .items_center()
            .border_t(px(1.))
            .border_color(line)
            .bg(to_hsla(p.tab_face))
            .child(
                div()
                    .flex_1()
                    .text_size(px(10.5))
                    .text_color(muted)
                    .child("源对象"),
            )
            .child(
                div()
                    .w(px(120.))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .text_size(px(10.5))
                    .text_color(muted)
                    .child("操作"),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(10.5))
                    .text_color(muted)
                    .child("目标对象"),
            );
        let rows = self.group_rows(cx);
        let all_row = self.all_row(cx);
        let list = div()
            .id("sc-object-scroll")
            .test_support()
            .flex_1()
            .w_full()
            .min_w_0()
            .min_h_0()
            .overflow_y_scrollbar()
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .children(std::iter::once(all_row))
                    .children(rows),
            );
        let busy = self.busy;
        let progress = if self.progress_total > 0 {
            format!("比较中… {}/{}", self.progress_done, self.progress_total)
        } else {
            "比较中…".to_string()
        };
        let (status, status_color) = if let Some(e) = self.error.as_ref() {
            (e.clone(), bad_color(dark))
        } else if !warnings.is_empty() {
            (warnings.join("；"), muted)
        } else if let Some(n) = self.notice.as_ref() {
            (n.clone(), muted)
        } else {
            (
                "目标端多出的对象只给注释掉的 DROP；下一步确认脚本后可在目标端执行。".to_string(),
                muted,
            )
        };
        let mut footer = ui::footer_bar(dark)
            .w_full()
            .min_w_0()
            .child(
                ui::secondary_button("sc-save-profile", "保存配置文件")
                    .on_click(cx.listener(|this, _, window, cx| this.save_profile(window, cx))),
            )
            .child(
                ui::secondary_button("sc-load-profile", "加载配置文件")
                    .on_click(cx.listener(|this, _, window, cx| this.load_profile(window, cx))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(11.))
                    .text_color(status_color)
                    .truncate()
                    .child(status),
            );
        if busy {
            footer = footer
                .child(
                    ui::secondary_button("sc-cancel", "取消").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.cancel.store(true, Ordering::Relaxed);
                            cx.notify();
                        }),
                    ),
                )
                .child(ui::primary_button_enabled(
                    "sc-copy", progress, dark, false, cx,
                ));
        } else {
            footer = footer
                .child(
                    ui::secondary_button("sc-back", "上一步")
                        .on_click(cx.listener(|this, _, _, cx| this.back_to_setup(cx))),
                )
                .child(
                    ui::secondary_button("sc-rerun", "重新比较")
                        .on_click(cx.listener(|this, _, _, cx| this.start_compare(cx))),
                )
                .child(
                    ui::primary_button_enabled(
                        "sc-next", "下一步", dark, true, cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.next_to_deploy(cx))),
                );
        }
        v_flex()
            .id("sc-compare")
            .test_support()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .child(top)
            .child(header)
            .child(list)
            .child(self.diff_pane(cx))
            .child(footer)
            .into_any_element()
    }

    // ── 第三页：部署（脚本全文 / 执行回执）─────────────────────────

    fn deploy_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let line = to_hsla(p.dialog_line);
        let accent = to_hsla(p.accent);
        let dark = self.dark;
        let Some(result) = self.result.as_ref() else {
            return div().flex_1().into_any_element();
        };
        let (src_name, src_place) = (result.src_name.clone(), result.src_place.clone());
        let (dst_name, dst_place) = (result.dst_name.clone(), result.dst_place.clone());
        let tab = |ix: u8, label: &'static str| {
            let active = self.deploy_tab == ix;
            div()
                .id(format!("sc-deploy-tab-{ix}"))
                .test_support()
                .h_full()
                .px(px(14.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(if active { fg } else { muted })
                .when(active, |el| el.border_b(px(2.)).border_color(accent))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.deploy_tab = ix;
                    cx.notify();
                }))
                .child(label)
        };
        let tabs = h_flex()
            .flex_none()
            .w_full()
            .min_w_0()
            .h(px(34.))
            .px(px(8.))
            .gap(px(2.))
            .items_end()
            .border_b(px(1.))
            .border_color(line)
            .bg(to_hsla(p.tab_face))
            .child(tab(0, "部署脚本"))
            .child(tab(1, "消息日志"));
        let summary = h_flex()
            .flex_none()
            .w_full()
            .min_w_0()
            .h(px(60.))
            .items_center()
            .justify_center()
            .child(self.summary_row(&src_name, &src_place, &dst_name, &dst_place));
        let dst_color = to_hsla(if dark { 0x6cb2ff } else { 0x2f6fd0 });
        let copied = self.copied;
        let header = h_flex()
            .flex_none()
            .w_full()
            .min_w_0()
            .h(px(42.))
            .px(px(12.))
            .gap(px(8.))
            .items_center()
            .border_t(px(1.))
            .border_b(px(1.))
            .border_color(line)
            .bg(to_hsla(p.tab_face))
            .child(
                Icon::new(ZedIcon::Database)
                    .with_size(px(18.))
                    .text_color(dst_color),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(fg)
                            .child(dst_name.clone()),
                    )
                    .child(
                        h_flex()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(muted)
                                    .child(dst_place.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(muted)
                                    .child("（以下脚本将在此服务器上运行）"),
                            ),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("sc-deploy-copy")
                    .test_support()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(6.))
                    .py(px(3.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|el| el.bg(to_hsla(p.hover)))
                    .text_size(px(11.))
                    .text_color(accent)
                    .on_click(cx.listener(|this, _, _, cx| this.copy_deploy_script(cx)))
                    .child(zed_icons::ic(ZedIcon::Copy, 12., accent))
                    .child(if copied { "已复制" } else { "复制脚本" }),
            );
        let busy = self.deploying;
        let stmts = self.deploy_statements();
        let status = if let Some(e) = self.error.as_ref() {
            (e.clone(), bad_color(dark))
        } else if busy {
            ("正在目标端执行…".to_string(), muted)
        } else if !self.deploy_log.is_empty() {
            // 计数与耗时都在日志页顶栏，页脚不再重复
            (String::new(), muted)
        } else if let Some(n) = self.notice.as_ref() {
            (n.clone(), muted)
        } else {
            (
                "脚本只在点「开始」后于目标端逐条执行；失败即停，回执见「消息日志」。".to_string(),
                muted,
            )
        };
        // 执行完右端换成「关闭」：开始按钮此时没有意义，参考客户端也是关窗收尾
        let tail = if !busy && !self.deploy_log.is_empty() {
            ui::secondary_button("sc-close", "关闭")
                .on_click(cx.listener(|_, _, window, _| window.remove_window()))
        } else {
            ui::primary_button_enabled(
                "sc-start",
                if busy { "执行中…" } else { "开始" },
                dark,
                !busy && !stmts.is_empty(),
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| this.start_deploy(cx)))
        };
        let footer = ui::footer_bar(dark)
            .w_full()
            .min_w_0()
            .child(
                ui::secondary_button("sc-save-profile", "保存配置文件")
                    .on_click(cx.listener(|this, _, window, cx| this.save_profile(window, cx))),
            )
            .child(
                ui::secondary_button("sc-load-profile", "加载配置文件")
                    .on_click(cx.listener(|this, _, window, cx| this.load_profile(window, cx))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(11.))
                    .text_color(status.1)
                    .truncate()
                    .child(status.0),
            )
            .child(
                ui::secondary_button("sc-deploy-back", "上一步")
                    .on_click(cx.listener(|this, _, _, cx| this.back_to_compare(cx))),
            )
            .child(
                ui::secondary_button("sc-rerun", "重新比较")
                    .on_click(cx.listener(|this, _, _, cx| this.start_compare(cx))),
            )
            .child(tail);
        let body = match self.deploy_tab {
            0 => self.deploy_script_body(),
            _ => self.deploy_log_body(stmts.len()),
        };
        let mut page = v_flex()
            .id("sc-deploy")
            .test_support()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .child(tabs)
            .child(summary);
        // 目标服务器行和「复制脚本」只属于脚本页；日志页顶部换成执行计数
        if self.deploy_tab == 0 {
            page = page.child(header);
        }
        page.child(body).child(footer).into_any_element()
    }

    fn deploy_script_body(&self) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let dark = self.dark;
        let script = self.deploy_script();
        if script.trim().is_empty() {
            return v_flex()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(muted)
                        .child("没有勾选要部署的对象：回到比较页勾选后再来。"),
                )
                .into_any_element();
        }
        let lines: Vec<&str> = script.lines().collect();
        let w = sql_preview::code_lines_w(&lines, 0.);
        div()
            .id("sc-deploy-script")
            .test_support()
            .flex_1()
            .w_full()
            .min_w_0()
            .min_h_0()
            .p(px(8.))
            .child(
                div().size_full().self_start().overflow_scrollbar().child(
                    v_flex().w(px(w)).child(sql_preview::code_block(
                        "sc-deploy-script-sql",
                        &lines,
                        &[],
                        0.,
                        w,
                        0,
                        dark,
                        fg,
                    )),
                ),
            )
            .into_any_element()
    }

    fn deploy_log_body(&self, total: usize) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let dark = self.dark;
        let done = self.deploy_log.len();
        let ok = self.deploy_log.iter().filter(|e| e.ok).count();
        let failed = done - ok;
        let pct = if total > 0 {
            done as f64 * 100.0 / total as f64
        } else {
            0.0
        };
        let stat = |label: &'static str, value: String, color: Hsla| {
            h_flex()
                .gap(px(8.))
                .child(
                    div()
                        .w(px(34.))
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(muted)
                        .child(label),
                )
                .child(div().text_size(px(11.)).text_color(color).child(value))
        };
        let stats = v_flex()
            .id("sc-deploy-stats")
            .test_support()
            .flex_none()
            .w_full()
            .min_w_0()
            .px(px(12.))
            .py(px(8.))
            .gap(px(2.))
            .border_b(px(1.))
            .border_color(to_hsla(p.dialog_line))
            .child(stat(
                "进度",
                format!("{done}/{total} ({pct:.1}%)"),
                fg,
            ))
            .child(stat(
                "成功",
                ok.to_string(),
                if ok > 0 { ok_color(dark) } else { muted },
            ))
            .child(stat(
                "错误",
                failed.to_string(),
                if failed > 0 { bad_color(dark) } else { muted },
            ))
            .child(stat("时间", fmt_elapsed(self.deploy_ms), fg));
        if self.deploy_log.is_empty() {
            let mut empty = v_flex()
                .w_full()
                .min_w_0()
                .flex_1()
                .min_h_0()
                .child(stats);
            if self.deploying {
                empty = empty.child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .items_center()
                        .justify_center()
                        .child(div().text_size(px(11.5)).text_color(muted).child("正在目标端执行…")),
                );
            }
            return empty.into_any_element();
        }
        let log_line = |text: String, color: Hsla| {
            div()
                .w_full()
                .min_w_0()
                .px(px(12.))
                .py(px(2.))
                .text_size(px(11.5))
                .text_color(color)
                .child(text)
                .into_any_element()
        };
        let mut lines: Vec<AnyElement> = vec![log_line("--Start--".into(), fg)];
        for (i, e) in self.deploy_log.iter().enumerate() {
            if e.ok {
                continue;
            }
            lines.push(log_line(format!("第 {} 条执行失败：{}", i + 1, e.msg), bad_color(dark)));
            for line in failure_snippet(&e.stmt) {
                lines.push(log_line(line, bad_color(dark)));
            }
        }
        if self.deploy_skipped > 0 {
            lines.push(log_line(
                format!("未执行 {} 条", self.deploy_skipped),
                bad_color(dark),
            ));
        }
        lines.push(log_line(format!("Executed Query: {done}"), fg));
        lines.push(log_line("--End--".into(), fg));
        v_flex()
            .id("sc-deploy-log")
            .test_support()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .child(stats)
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .flex_1()
                    .min_h_0()
                    .py(px(6.))
                    .overflow_y_scrollbar()
                    .child(v_flex().w_full().min_w_0().children(lines)),
            )
            .into_any_element()
    }

    fn filtered(&self) -> Vec<usize> {
        let Some(result) = self.result.as_ref() else {
            return Vec::new();
        };
        let needle = self.needle.trim().to_ascii_lowercase();
        result
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| needle.is_empty() || o.name.to_ascii_lowercase().contains(&needle))
            .map(|(ix, _)| ix)
            .collect()
    }

    /// 三列清单：顶部「全部对象」全选行 + 按当前分组口径的分组。
    fn group_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let indices = self.filtered();
        let mut out: Vec<AnyElement> = Vec::new();
        match self.group_mode {
            GroupMode::Op => {
                for status in [
                    DiffStatus::Changed,
                    DiffStatus::New,
                    DiffStatus::Removed,
                    DiffStatus::Same,
                ] {
                    let group: Vec<usize> = indices
                        .iter()
                        .copied()
                        .filter(|ix| self.object(*ix).is_some_and(|o| o.status == status))
                        .collect();
                    if group.is_empty() {
                        continue;
                    }
                    let title = match status {
                        DiffStatus::Changed => "要修改的对象",
                        DiffStatus::New => "要创建的对象",
                        DiffStatus::Removed => "要删除的对象",
                        DiffStatus::Same => "无操作",
                    };
                    let icon = op_icon(status);
                    out.push(self.group_block(
                        &format!("op-{}", status.ix()),
                        title,
                        icon,
                        status.color(self.dark),
                        &group,
                        cx,
                    ));
                }
            }
            GroupMode::Category => {
                for cat in [CAT_TABLE, CAT_VIEW, CAT_MATVIEW, CAT_FUNC, CAT_PROC] {
                    let group: Vec<usize> = indices
                        .iter()
                        .copied()
                        .filter(|ix| self.object(*ix).is_some_and(|o| o.cat == cat))
                        .collect();
                    if group.is_empty() {
                        continue;
                    }
                    out.push(self.group_block(
                        &format!("cat-{cat}"),
                        cat_title(cat),
                        cat_icon(cat),
                        fg,
                        &group,
                        cx,
                    ));
                }
            }
        }
        out
    }

    fn object(&self, ix: usize) -> Option<&ObjDiff> {
        self.result.as_ref().and_then(|r| r.objects.get(ix))
    }

    fn all_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let indices = self.filtered();
        let all = self.all_checked(&indices);
        let total = indices.len();
        let selected = indices
            .iter()
            .filter(|ix| self.checked.contains(ix))
            .count();
        let me = cx.entity();
        h_flex()
            .id("sc-all")
            .test_support()
            .h(px(30.))
            .flex_none()
            .px(px(10.))
            .gap(px(6.))
            .items_center()
            .border_t(px(1.))
            .border_color(to_hsla(p.border_soft))
            .bg(to_hsla(p.dialog_surface))
            .child(ui::checkbox(
                "sc-all-check",
                "",
                all,
                !indices.is_empty(),
                self.dark,
                move |_, _, cx| {
                    cx.stop_propagation();
                    let list = indices.clone();
                    me.update(cx, |this, cx| this.toggle_group_checked(&list, cx));
                },
            ))
            .child(div().text_size(px(12.)).text_color(fg).child("全部对象"))
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(10.5))
                    .text_color(muted)
                    .child(format!("已选择 {selected} 个 (共 {total} 个)")),
            )
            .into_any_element()
    }

    /// 一个可折叠分组：表头行 + 展开后的对象行（对象行再可展开明细）。
    fn group_block(
        &self,
        key: &str,
        title: &str,
        icon: ZedIcon,
        color: Hsla,
        indices: &[usize],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let collapsed = self.collapsed.contains(key);
        let all = self.all_checked(indices);
        let total = indices.len();
        let selected = indices
            .iter()
            .filter(|ix| self.checked.contains(ix))
            .count();
        let key_owned = key.to_string();
        let me = cx.entity();
        let check_id = format!("sc-group-check-{key}");
        let list = indices.to_vec();
        let mut block = v_flex()
            .id(format!("sc-group-{key}"))
            .test_support()
            .flex_none()
            .child(
                h_flex()
                    .id(format!("sc-group-head-{key}"))
                    .h(px(30.))
                    .flex_none()
                    .px(px(10.))
                    .gap(px(6.))
                    .items_center()
                    .border_t(px(1.))
                    .border_color(to_hsla(p.border_soft))
                    .bg(to_hsla(p.dialog_surface))
                    .cursor_pointer()
                    .hover(|el| el.bg(to_hsla(p.hover)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.collapsed.remove(&key_owned) {
                            this.collapsed.insert(key_owned.clone());
                        }
                        cx.notify();
                    }))
                    .child(zed_icons::ic(
                        if collapsed {
                            ZedIcon::ChevronRight
                        } else {
                            ZedIcon::ChevronDown
                        },
                        10.,
                        muted,
                    ))
                    .child(ui::checkbox(
                        check_id,
                        "",
                        all,
                        !indices.is_empty(),
                        self.dark,
                        move |_, _, cx| {
                            cx.stop_propagation();
                            let list = list.clone();
                            me.update(cx, |this, cx| this.toggle_group_checked(&list, cx));
                        },
                    ))
                    .child(zed_icons::ic(icon, 12., color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(fg)
                            .child(title.to_string()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(10.5))
                            .text_color(muted)
                            .child(format!("已选择 {selected} 个 (共 {total} 个)")),
                    ),
            );
        if !collapsed {
            for &ix in indices {
                for row in self.object_rows(ix, cx) {
                    block = block.child(row);
                }
            }
        }
        block.into_any_element()
    }

    /// 对象行 + （展开时）明细行；三列依次是源对象 / 操作图标 / 目标对象。
    fn object_rows(&self, ix: usize, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(obj) = self.object(ix) else {
            return Vec::new();
        };
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let dark = self.dark;
        let checked = self.checked.contains(&ix);
        let expanded = self.expanded.contains(&ix);
        let selected = self.selected == Some(ix);
        let src_present = obj.status != DiffStatus::Removed;
        let dst_present = obj.status != DiffStatus::New;
        let name = obj.name.clone();
        let cat = obj.cat;
        let status = obj.status;
        let items_len = obj.items.len();
        let me_check = cx.entity();
        let me_expand = cx.entity();
        let row = h_flex()
            .id(format!("sc-obj-{ix}"))
            .test_support()
            .h(px(28.))
            .flex_none()
            .px(px(10.))
            .gap(px(6.))
            .items_center()
            .border_t(px(1.))
            .border_color(to_hsla(p.border_soft))
            .bg(if selected {
                to_hsla(p.selection)
            } else {
                to_hsla(p.dialog_surface)
            })
            .cursor_pointer()
            .hover(|el| el.bg(to_hsla(p.hover)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = Some(ix);
                this.copied = false;
                cx.notify();
            }))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(6.))
                    .items_center()
                    .child(ui::checkbox(
                        format!("sc-check-{ix}"),
                        "",
                        checked,
                        true,
                        dark,
                        move |_, _, cx| {
                            cx.stop_propagation();
                            me_check.update(cx, |this, cx| this.toggle_checked(ix, cx));
                        },
                    ))
                    .when(items_len > 0, |el| {
                        el.child(
                            div()
                                .id(format!("sc-expand-{ix}"))
                                .test_support()
                                .w(px(14.))
                                .h(px(14.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if !this.expanded.remove(&ix) {
                                        this.expanded.insert(ix);
                                    }
                                    cx.notify();
                                }))
                                .child(zed_icons::ic(
                                    if expanded {
                                        ZedIcon::ChevronDown
                                    } else {
                                        ZedIcon::ChevronRight
                                    },
                                    10.,
                                    muted,
                                )),
                        )
                    })
                    .when(src_present, |el| {
                        el.child(zed_icons::ic(cat_icon(cat), 13., muted))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_color(if src_present { fg } else { muted })
                            .truncate()
                            .child(if src_present {
                                name.clone()
                            } else {
                                "（无）".to_string()
                            }),
                    ),
            )
            .child(
                div()
                    .w(px(120.))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(zed_icons::ic(op_icon(status), 12., status.color(dark))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .text_color(if dst_present { fg } else { muted })
                    .truncate()
                    .child(if dst_present {
                        name.clone()
                    } else {
                        "（无）".to_string()
                    }),
            );
        let mut out = vec![row.into_any_element()];
        if expanded {
            for (n, item) in obj.items.iter().enumerate() {
                out.push(
                    h_flex()
                        .id(format!("sc-item-{ix}-{n}"))
                        .test_support()
                        .h(px(23.))
                        .flex_none()
                        .px(px(10.))
                        .gap(px(6.))
                        .items_center()
                        .border_t(px(1.))
                        .border_color(to_hsla(p.border_soft))
                        .bg(to_hsla(p.dialog_surface))
                        .child(div().w(px(58.)).flex_none())
                        .child(zed_icons::ic(item.kind.icon(), 11., muted))
                        .child(
                            div()
                                .w(px(150.))
                                .flex_none()
                                .text_size(px(11.5))
                                .text_color(fg)
                                .truncate()
                                .child(item.label.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(10.5))
                                .text_color(muted)
                                .truncate()
                                .child(item.note.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(10.))
                                .text_color(item.status.color(dark))
                                .child(item.status.label()),
                        )
                        .into_any_element(),
                );
            }
        }
        out
    }

    /// 底部页签：DDL 比较 / 部署脚本。
    fn diff_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let line = to_hsla(p.dialog_line);
        let accent = to_hsla(p.accent);
        let page = self.page;
        let tab = |ix: u8, label: &'static str| {
            let active = page == ix;
            div()
                .id(format!("sc-tab-{ix}"))
                .test_support()
                .h_full()
                .px(px(12.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(if active { fg } else { muted })
                .when(active, |el| el.border_b(px(2.)).border_color(accent))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = ix;
                    cx.notify();
                }))
                .child(label)
        };
        let scope = if page == 1 {
            format!("已勾选 {} 个对象", self.checked.len())
        } else {
            match self.selected.and_then(|ix| self.object(ix)) {
                Some(o) => format!("{}（{}）", o.name, o.cat_label()),
                None => "全部差异对象".to_string(),
            }
        };
        let head = h_flex()
            .id("sc-ddl-head")
            .test_support()
            .flex_none()
            .w_full()
            .min_w_0()
            .h(px(30.))
            .px(px(8.))
            .gap(px(2.))
            .items_end()
            .border_t(px(1.))
            .border_color(line)
            .bg(to_hsla(p.tab_face))
            .child(tab(0, "DDL 比较"))
            .child(tab(1, "部署脚本"))
            .child(div().flex_1())
            .child(
                div()
                    .pb(px(6.))
                    .pr(px(4.))
                    .text_size(px(10.5))
                    .text_color(muted)
                    .child(scope),
            );
        let body = if page == 0 {
            self.ddl_body()
        } else {
            self.script_body()
        };
        v_flex()
            .flex_none()
            .w_full()
            .min_w_0()
            .bg(to_hsla(p.dialog_surface))
            .child(head)
            .child(body)
            .into_any_element()
    }

    fn ddl_body(&self) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let dark = self.dark;
        let Some(obj) = self.selected.and_then(|ix| self.object(ix)) else {
            return v_flex()
                .id("sc-ddl")
                .test_support()
                .h(px(200.))
                .flex_none()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(muted)
                        .child("在上方清单里选一个对象，查看源与目标的 DDL 对比。"),
                )
                .into_any_element();
        };
        let before = if obj.src_ddl.trim().is_empty() {
            "-- 该端没有这个对象".to_string()
        } else {
            obj.src_ddl.clone()
        };
        let after = if obj.dst_ddl.trim().is_empty() {
            "-- 该端没有这个对象".to_string()
        } else {
            obj.dst_ddl.clone()
        };
        let src_title = if obj.src_ddl.trim().is_empty() {
            format!("源: （没有 {}）", obj.name)
        } else {
            format!("源: {}", obj.name)
        };
        let dst_title = if obj.dst_ddl.trim().is_empty() {
            format!("目标: （没有 {}）", obj.name)
        } else {
            format!("目标: {}", obj.name)
        };
        let b: Vec<&str> = before.lines().collect();
        let a: Vec<&str> = after.lines().collect();
        let (chg_b, chg_a) = sql_preview::diff_lines(&before, &after);
        let gutter_w = 28.;
        let w_b = sql_preview::code_lines_w(&b, gutter_w);
        let w_a = sql_preview::code_lines_w(&a, gutter_w);
        let pane = |id: &'static str, rows: &[&str], chg: &[bool], w: f32, order: u64| {
            div()
                .id(id)
                .test_support()
                .flex_1()
                .min_w_0()
                .h_full()
                .min_h_0()
                .child(
                    div().size_full().self_start().overflow_scrollbar().child(
                        v_flex().w(px(w)).child(sql_preview::code_block(
                            ElementId::Name(format!("{id}-sql").into()),
                            rows,
                            chg,
                            gutter_w,
                            w,
                            order,
                            dark,
                            fg,
                        )),
                    ),
                )
        };
        v_flex()
            .id("sc-ddl")
            .test_support()
            .w_full()
            .min_w_0()
            .h(px(200.))
            .flex_none()
            .child(
                h_flex()
                    .flex_none()
                    .w_full()
                    .min_w_0()
                    .px(px(10.))
                    .pt(px(4.))
                    .pb(px(2.))
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(muted)
                            .child(src_title),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(muted)
                            .child(dst_title),
                    ),
            )
            .child(
                h_flex()
                    .id("sc-ddl-body")
                    .test_support()
                    .flex_1()
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .px(px(10.))
                    .pb(px(6.))
                    .gap(px(10.))
                    .child(pane("sc-ddl-src", &b, &chg_b, w_b, 0))
                    .child(pane("sc-ddl-dst", &a, &chg_a, w_a, 1)),
            )
            .into_any_element()
    }

    fn script_body(&self) -> AnyElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let dark = self.dark;
        let script = self.deploy_script();
        let with_text = if script.trim().is_empty() {
            "-- 没有勾选要部署的对象；在上方清单勾选后，这里会拼出部署脚本。".to_string()
        } else {
            script
        };
        let lines: Vec<&str> = with_text.lines().collect();
        let w = sql_preview::code_lines_w(&lines, 0.);
        div()
            .id("sc-script")
            .test_support()
            .w_full()
            .min_w_0()
            .h(px(200.))
            .flex_none()
            .p(px(8.))
            .child(
                div().size_full().self_start().overflow_scrollbar().child(
                    v_flex().w(px(w)).child(sql_preview::code_block(
                        "sc-script-sql",
                        &lines,
                        &[],
                        0.,
                        w,
                        0,
                        dark,
                        fg,
                    )),
                ),
            )
            .into_any_element()
    }
}

impl Render for StructureCompare {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        v_flex()
            .id("structure-compare-root")
            .test_support()
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .child(
                TitleBar::new().h(px(TITLE_H)).border_b_0().child(
                    h_flex()
                        .h_full()
                        .items_center()
                        .gap(px(6.))
                        .pl(px(4.))
                        .child(div().text_xs().text_color(fg).child("结构同步")),
                ),
            )
            .child(
                ui::dialog_surface(self.dark)
                    // 内容面默认是块级 div，子页面的 flex_1 不会生效，先声明成竖向 flex
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(match self.step {
                        Step::Setup => self.setup_page(cx),
                        Step::Compare => self.compare_page(cx),
                        Step::Deploy => self.deploy_page(cx),
                    }),
            )
    }
}

// ── 比较引擎（后台线程）─────────────────────────────────────────

struct TableDiff {
    head: Vec<String>,
    mid: Vec<String>,
    tail: Vec<String>,
    changes: Vec<String>,
    notes: Vec<String>,
}

fn sql_lit(s: &str) -> String {
    s.replace('\'', "''")
}

fn norm_sql(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn col_name(c: &crate::DesignCol) -> String {
    c.name.trim().to_string()
}

fn col_def(kind: DbKind, c: &crate::DesignCol) -> String {
    let q = |n: &str| crate::design_quote(kind, n);
    let mut seg = format!("{} {}", q(c.name.trim()), crate::design_full_type(c));
    if !c.collation.trim().is_empty()
        && !c.collation.eq_ignore_ascii_case("default")
        && kind != DbKind::SQLite
    {
        seg.push_str(&format!(" COLLATE {}", q(c.collation.trim())));
    }
    if c.not_null {
        seg.push_str(" NOT NULL");
    }
    if kind == DbKind::MySql && c.auto_inc {
        seg.push_str(" AUTO_INCREMENT");
    }
    if kind == DbKind::PostgreSql && !c.identity.trim().is_empty() {
        seg.push_str(&format!(" GENERATED {} AS IDENTITY", c.identity.trim()));
    }
    if !c.default.trim().is_empty() {
        seg.push_str(&format!(" DEFAULT {}", c.default.trim()));
    }
    if kind == DbKind::MySql && !c.comment.trim().is_empty() {
        seg.push_str(&format!(" COMMENT '{}'", sql_lit(c.comment.trim())));
    }
    seg
}

fn create_script(kind: DbKind, schema: &str, table: &str, meta: &crate::DesignMeta) -> String {
    if kind == DbKind::SQLite {
        return sqlite_create_script(table, meta, true);
    }
    let is_mysql = kind == DbKind::MySql;
    let mut sql = crate::design_create_sql_at(
        kind,
        schema,
        table,
        &meta.options.owner,
        &meta.cols,
        &meta.indexes,
        &meta.fks,
        &meta.uniques,
        &meta.checks,
        &meta.options,
        &[],
        &[],
    );
    // 设计器的建表拼装器只把默认值写进 MySQL 的行内定义；PG / SQLite
    // 的默认值是独立 ALTER，补在 CREATE 之后才不会丢。
    if !is_mysql {
        let qual = crate::design_qualify(kind, schema, table);
        for c in &meta.cols {
            if c.default.trim().is_empty() {
                continue;
            }
            sql.push_str(&format!(
                "\nALTER TABLE {qual}\n  ALTER COLUMN {} SET DEFAULT {};",
                crate::design_quote(kind, c.name.trim()),
                c.default.trim()
            ));
        }
    }
    sql
}

fn sqlite_create_script(table: &str, meta: &crate::DesignMeta, with_indexes: bool) -> String {
    let q = |n: &str| crate::design_quote(DbKind::SQLite, n);
    let pk: Vec<&crate::DesignCol> = meta.cols.iter().filter(|c| c.pk).collect();
    let auto_inline =
        pk.len() == 1 && pk[0].auto_inc && pk[0].ty.trim().eq_ignore_ascii_case("integer");
    let mut lines: Vec<String> = Vec::new();
    for c in &meta.cols {
        let mut seg = format!("    {} {}", q(c.name.trim()), crate::design_full_type(c));
        if auto_inline && c.pk {
            seg.push_str(" PRIMARY KEY AUTOINCREMENT");
        } else if c.not_null {
            seg.push_str(" NOT NULL");
        }
        if !c.default.trim().is_empty() {
            seg.push_str(&format!(" DEFAULT {}", c.default.trim()));
        }
        lines.push(seg);
    }
    if !auto_inline && !pk.is_empty() {
        lines.push(format!(
            "    PRIMARY KEY ({})",
            pk.iter()
                .map(|c| q(c.name.trim()))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for fk in &meta.fks {
        if let Some(clause) = crate::design_fk_sql(false, fk) {
            lines.push(format!("    {clause}"));
        }
    }
    for uk in &meta.uniques {
        if let Some(clause) = crate::design_unique_sql(false, uk) {
            lines.push(format!("    {clause}"));
        }
    }
    for ck in &meta.checks {
        if let Some(clause) = crate::design_check_sql(false, ck) {
            lines.push(format!("    {clause}"));
        }
    }
    let mut sql = format!("CREATE TABLE {} (\n{}\n);", q(table), lines.join(",\n"));
    if with_indexes {
        for idx in &meta.indexes {
            if let Some(stmt) = crate::design_index_sql_at(false, "", table, idx, &meta.cols) {
                sql.push('\n');
                sql.push_str(&stmt);
            }
        }
    }
    sql
}

fn view_create(kind: DbKind, schema: &str, name: &str, cat: u8, body: &str) -> String {
    let qual = crate::design_qualify(kind, schema, name);
    let head = if kind == DbKind::PostgreSql && cat == 2 {
        "CREATE MATERIALIZED VIEW"
    } else {
        "CREATE VIEW"
    };
    let tail = if kind == DbKind::PostgreSql && cat == 2 {
        "\nWITH DATA;"
    } else {
        ";"
    };
    format!(
        "{head} {qual} AS\n{}{tail}",
        body.trim().trim_end_matches(';')
    )
}

fn view_drop(kind: DbKind, schema: &str, name: &str, cat: u8, commented: bool) -> String {
    let qual = crate::design_qualify(kind, schema, name);
    let stmt = if kind == DbKind::PostgreSql && cat == 2 {
        format!("DROP MATERIALIZED VIEW IF EXISTS {qual};")
    } else {
        format!("DROP VIEW IF EXISTS {qual};")
    };
    if commented {
        format!("-- {stmt}")
    } else {
        stmt
    }
}

fn mysql_view_body(body: &str, src_db: &str, dst_db: &str) -> String {
    if src_db.is_empty() || dst_db.is_empty() || src_db == dst_db {
        return body.to_string();
    }
    body.replace(&format!("`{}`.", src_db), &format!("`{}`.", dst_db))
}

fn same_table(a: &crate::DesignMeta, b: &crate::DesignMeta) -> bool {
    let mut ca = a.cols.clone();
    let mut cb = b.cols.clone();
    ca.sort_by(|x, y| x.name.cmp(&y.name));
    cb.sort_by(|x, y| x.name.cmp(&y.name));
    if ca != cb {
        return false;
    }
    let idx_of = |m: &crate::DesignMeta| -> Vec<String> {
        let mut v: Vec<String> = m.indexes.iter().map(|i| format!("{:?}", i)).collect();
        v.sort();
        v
    };
    if idx_of(a) != idx_of(b) {
        return false;
    }
    let fks_of = |m: &crate::DesignMeta| -> Vec<String> {
        let mut v: Vec<String> = m.fks.iter().map(|f| format!("{:?}", f)).collect();
        v.sort();
        v
    };
    if fks_of(a) != fks_of(b) {
        return false;
    }
    let pairs_of = |m: &crate::DesignMeta| -> Vec<String> {
        let mut v: Vec<String> = m
            .uniques
            .iter()
            .chain(m.checks.iter())
            .map(|p| format!("{}={}", p.name.trim(), p.value.trim()))
            .collect();
        v.sort();
        v
    };
    if pairs_of(a) != pairs_of(b) {
        return false;
    }
    a.options.comment.trim() == b.options.comment.trim()
}

fn tally(changes: &mut Vec<String>, label: &str, plus: usize, minus: usize, changed: usize) {
    if plus == 0 && minus == 0 && changed == 0 {
        return;
    }
    let mut seg = format!("{label} ");
    let mut bits: Vec<String> = Vec::new();
    if plus > 0 {
        bits.push(format!("+{plus}"));
    }
    if minus > 0 {
        bits.push(format!("-{minus}"));
    }
    if changed > 0 {
        bits.push(format!("~{changed}"));
    }
    seg.push_str(&bits.join(" "));
    changes.push(seg);
}

fn table_diff(
    kind: DbKind,
    schema: &str,
    table: &str,
    src: &crate::DesignMeta,
    dst: &crate::DesignMeta,
) -> TableDiff {
    let is_mysql = kind == DbKind::MySql;
    let qual = crate::design_qualify(kind, schema, table);
    let q = |n: &str| crate::design_quote(kind, n);
    let alter = |frag: &str| format!("ALTER TABLE {qual}\n  {frag};");
    let mut d = TableDiff {
        head: Vec::new(),
        mid: Vec::new(),
        tail: Vec::new(),
        changes: Vec::new(),
        notes: Vec::new(),
    };

    // 索引：先算出要删 / 要建的，放到列操作前后
    let idx_name = |idx: &crate::DesignIndex| -> String {
        let n = idx.name.trim();
        if !n.is_empty() {
            return n.to_string();
        }
        let fields: Vec<_> = idx
            .fields
            .iter()
            .filter(|f| !f.name.trim().is_empty())
            .collect();
        crate::design_index_auto_name(is_mysql, table, &fields)
    };
    let drop_index = |name: &str| -> String {
        match kind {
            DbKind::MySql => alter(&format!("DROP INDEX {}", q(name))),
            DbKind::PostgreSql if !schema.trim().is_empty() => {
                format!("DROP INDEX {}.{};", q(schema.trim()), q(name))
            }
            _ => format!("DROP INDEX {};", q(name)),
        }
    };
    let (mut idx_add, mut idx_drop, mut idx_chg) = (0usize, 0usize, 0usize);
    for idx in &src.indexes {
        if crate::design_index_blank(idx) {
            continue;
        }
        let name = idx_name(idx);
        match dst.indexes.iter().find(|b| idx_name(b) == name) {
            None => {
                if let Some(stmt) =
                    crate::design_index_sql_at(is_mysql, schema, table, idx, &src.cols)
                {
                    d.tail.push(stmt);
                    idx_add += 1;
                }
            }
            Some(b) if b == idx => {}
            Some(_) => {
                d.head.push(drop_index(&name));
                if let Some(stmt) =
                    crate::design_index_sql_at(is_mysql, schema, table, idx, &src.cols)
                {
                    d.tail.push(stmt);
                    idx_chg += 1;
                }
            }
        }
    }
    for b in &dst.indexes {
        if crate::design_index_blank(b) {
            continue;
        }
        let name = idx_name(b);
        if src.indexes.iter().any(|f| idx_name(f) == name) {
            continue;
        }
        d.head.push(drop_index(&name));
        idx_drop += 1;
    }
    tally(&mut d.changes, "索引", idx_add, idx_drop, idx_chg);

    // 字段
    let same_col = |c: &crate::DesignCol, b: &crate::DesignCol| {
        // 只动了主键勾由表级主键段负责，这里不算列改动
        let mut x = c.clone();
        x.pk = b.pk;
        x == *b
    };
    let (mut col_add, mut col_drop, mut col_chg) = (0usize, 0usize, 0usize);
    for c in &src.cols {
        let name = col_name(c);
        if name.is_empty() {
            continue;
        }
        match dst.cols.iter().find(|b| col_name(b) == name) {
            None => {
                d.mid
                    .push(alter(&format!("ADD COLUMN {}", col_def(kind, c))));
                col_add += 1;
                if c.not_null && c.default.trim().is_empty() {
                    d.notes
                        .push(format!("新增非空列 {name} 没有默认值，空表才能直接执行"));
                }
            }
            Some(b) if same_col(c, b) => {}
            Some(b) => {
                col_chg += 1;
                if kind == DbKind::MySql {
                    d.mid
                        .push(alter(&format!("MODIFY COLUMN {}", col_def(kind, c))));
                } else {
                    match crate::design_col_alter(kind, &qual, c, b) {
                        Ok(stmts) => d.mid.extend(stmts),
                        Err(e) => d.notes.push(e),
                    }
                }
            }
        }
    }
    for b in &dst.cols {
        let name = col_name(b);
        if name.is_empty() || src.cols.iter().any(|c| col_name(c) == name) {
            continue;
        }
        // 删列会丢数据：语句照出，但前面挂一条警示
        d.mid.push(alter(&format!("DROP COLUMN {}", q(&name))));
        d.notes.push(format!("删除列 {name} 会丢掉该列数据"));
        col_drop += 1;
    }
    tally(&mut d.changes, "字段", col_add, col_drop, col_chg);

    // 主键
    let pk_of = |m: &crate::DesignMeta| -> Vec<String> {
        m.cols
            .iter()
            .filter(|c| c.pk && !c.name.trim().is_empty())
            .map(|c| c.name.trim().to_string())
            .collect()
    };
    let (pk_now, pk_base) = (pk_of(src), pk_of(dst));
    if pk_now != pk_base {
        d.changes.push("主键".into());
        if pk_base.is_empty() {
            d.mid.push(alter(&format!(
                "ADD PRIMARY KEY ({})",
                pk_now.iter().map(|n| q(n)).collect::<Vec<_>>().join(", ")
            )));
        } else if is_mysql {
            d.head.push(alter("DROP PRIMARY KEY"));
            if !pk_now.is_empty() {
                d.mid.push(alter(&format!(
                    "ADD PRIMARY KEY ({})",
                    pk_now.iter().map(|n| q(n)).collect::<Vec<_>>().join(", ")
                )));
            }
        } else {
            d.notes
                .push("PostgreSQL 改主键需要原约束名，这里没有取到，请手工处理".to_string());
        }
    }

    // 外键
    let (mut fk_add, mut fk_drop, mut fk_chg) = (0usize, 0usize, 0usize);
    for fk in &src.fks {
        let name = fk.name.trim();
        if name.is_empty() {
            continue;
        }
        match dst.fks.iter().find(|b| b.name.trim() == name) {
            None => {
                if let Some(clause) = crate::design_fk_sql(is_mysql, fk) {
                    d.tail.push(alter(&format!("ADD {clause}")));
                    fk_add += 1;
                }
            }
            Some(b) if b == fk => {}
            Some(_) => {
                d.head.push(alter(&format!(
                    "DROP {}",
                    if is_mysql {
                        format!("FOREIGN KEY {}", q(name))
                    } else {
                        format!("CONSTRAINT {}", q(name))
                    }
                )));
                if let Some(clause) = crate::design_fk_sql(is_mysql, fk) {
                    d.tail.push(alter(&format!("ADD {clause}")));
                    fk_chg += 1;
                }
            }
        }
    }
    for b in &dst.fks {
        let name = b.name.trim();
        if name.is_empty() || src.fks.iter().any(|f| f.name.trim() == name) {
            continue;
        }
        d.head.push(alter(&format!(
            "DROP {}",
            if is_mysql {
                format!("FOREIGN KEY {}", q(name))
            } else {
                format!("CONSTRAINT {}", q(name))
            }
        )));
        fk_drop += 1;
    }
    tally(&mut d.changes, "外键", fk_add, fk_drop, fk_chg);

    // 唯一键
    let (mut uk_add, mut uk_drop, mut uk_chg) = (0usize, 0usize, 0usize);
    for uk in &src.uniques {
        let name = uk.name.trim();
        if name.is_empty() {
            continue;
        }
        match dst.uniques.iter().find(|b| b.name.trim() == name) {
            None => {
                if let Some(clause) = crate::design_unique_sql(is_mysql, uk) {
                    d.tail.push(alter(&format!("ADD {clause}")));
                    uk_add += 1;
                }
            }
            Some(b) if b.value.trim() == uk.value.trim() => {}
            Some(_) => {
                d.head.push(alter(&format!(
                    "DROP {}",
                    if is_mysql {
                        format!("INDEX {}", q(name))
                    } else {
                        format!("CONSTRAINT {}", q(name))
                    }
                )));
                if let Some(clause) = crate::design_unique_sql(is_mysql, uk) {
                    d.tail.push(alter(&format!("ADD {clause}")));
                    uk_chg += 1;
                }
            }
        }
    }
    for b in &dst.uniques {
        let name = b.name.trim();
        if name.is_empty() || src.uniques.iter().any(|u| u.name.trim() == name) {
            continue;
        }
        d.head.push(alter(&format!(
            "DROP {}",
            if is_mysql {
                format!("INDEX {}", q(name))
            } else {
                format!("CONSTRAINT {}", q(name))
            }
        )));
        uk_drop += 1;
    }
    tally(&mut d.changes, "唯一键", uk_add, uk_drop, uk_chg);

    // 检查约束
    let (mut ck_add, mut ck_drop, mut ck_chg) = (0usize, 0usize, 0usize);
    for ck in &src.checks {
        let name = ck.name.trim();
        if name.is_empty() {
            continue;
        }
        match dst.checks.iter().find(|b| b.name.trim() == name) {
            None => {
                if let Some(clause) = crate::design_check_sql(kind == DbKind::PostgreSql, ck) {
                    d.tail.push(alter(&format!("ADD {clause}")));
                    ck_add += 1;
                }
            }
            Some(b) if b.value.trim() == ck.value.trim() => {}
            Some(_) => {
                d.head.push(alter(&format!("DROP CONSTRAINT {}", q(name))));
                if let Some(clause) = crate::design_check_sql(kind == DbKind::PostgreSql, ck) {
                    d.tail.push(alter(&format!("ADD {clause}")));
                    ck_chg += 1;
                }
            }
        }
    }
    for b in &dst.checks {
        let name = b.name.trim();
        if name.is_empty() || src.checks.iter().any(|c| c.name.trim() == name) {
            continue;
        }
        d.head.push(alter(&format!("DROP CONSTRAINT {}", q(name))));
        ck_drop += 1;
    }
    tally(&mut d.changes, "检查", ck_add, ck_drop, ck_chg);

    // 表注释：MySQL 行内 ALTER，PG 走 COMMENT ON
    if src.options.comment.trim() != dst.options.comment.trim() {
        d.changes.push("表注释".into());
        let lit = if src.options.comment.trim().is_empty() {
            "NULL".to_string()
        } else {
            format!("'{}'", sql_lit(src.options.comment.trim()))
        };
        if is_mysql {
            d.tail.push(format!(
                "ALTER TABLE {qual} COMMENT = {};",
                if src.options.comment.trim().is_empty() {
                    "''".to_string()
                } else {
                    lit
                }
            ));
        } else if kind == DbKind::PostgreSql {
            d.tail.push(format!("COMMENT ON TABLE {qual} IS {lit};"));
        }
    }
    d
}

fn sqlite_rebuild(table: &str, src: &crate::DesignMeta, dst: &crate::DesignMeta) -> String {
    let q = |n: &str| crate::design_quote(DbKind::SQLite, n);
    let tmp = format!("{table}__daro_new");
    let common: Vec<String> = src
        .cols
        .iter()
        .map(|c| c.name.trim().to_string())
        .filter(|n| dst.cols.iter().any(|b| b.name.trim() == n))
        .collect();
    let mut sql = String::new();
    sql.push_str("-- SQLite 不能直接改列 / 加约束，按源端结构重建这张表\n");
    sql.push_str("PRAGMA foreign_keys=OFF;\nBEGIN;\n");
    sql.push_str(&format!("DROP TABLE IF EXISTS {};\n", q(&tmp)));
    sql.push_str(&sqlite_create_script(&tmp, src, false));
    sql.push('\n');
    if !common.is_empty() {
        let list = common.iter().map(|n| q(n)).collect::<Vec<_>>().join(", ");
        sql.push_str(&format!(
            "INSERT INTO {} ({list}) SELECT {list} FROM {};\n",
            q(&tmp),
            q(table)
        ));
    }
    sql.push_str(&format!("DROP TABLE {};\n", q(table)));
    sql.push_str(&format!(
        "ALTER TABLE {} RENAME TO {};\n",
        q(&tmp),
        q(table)
    ));
    // 索引名在 SQLite 里是整库唯一的：老表和临时表同时存在时建不上，
    // 等改名之后再按源端索引补建。
    for idx in &src.indexes {
        if let Some(stmt) = crate::design_index_sql_at(false, "", table, idx, &src.cols) {
            sql.push_str(&stmt);
            sql.push('\n');
        }
    }
    sql.push_str("COMMIT;\nPRAGMA foreign_keys=ON;");
    sql
}

/// SQLite 只差索引时不必重建表：列 / 主键 / 外键 / 唯一键 / 检查约束都一样就够。
fn sqlite_shape_same(a: &crate::DesignMeta, b: &crate::DesignMeta) -> bool {
    let cols_of = |m: &crate::DesignMeta| {
        let mut v = m.cols.clone();
        v.sort_by(|x, y| x.name.cmp(&y.name));
        v
    };
    if cols_of(a) != cols_of(b) {
        return false;
    }
    let fks_of = |m: &crate::DesignMeta| {
        let mut v: Vec<String> = m.fks.iter().map(|f| format!("{:?}", f)).collect();
        v.sort();
        v
    };
    if fks_of(a) != fks_of(b) {
        return false;
    }
    let pairs_of = |m: &crate::DesignMeta| {
        let mut v: Vec<String> = m
            .uniques
            .iter()
            .chain(m.checks.iter())
            .map(|p| format!("{}={}", p.name.trim(), p.value.trim()))
            .collect();
        v.sort();
        v
    };
    pairs_of(a) == pairs_of(b)
}

fn table_script(
    kind: DbKind,
    schema: &str,
    table: &str,
    src: &crate::DesignMeta,
    dst: &crate::DesignMeta,
) -> (String, Vec<String>, Vec<String>) {
    if kind == DbKind::SQLite && !sqlite_shape_same(src, dst) {
        let mut notes = vec!["结构有改动，按 SQLite 重建流程处理".to_string()];
        for c in &src.cols {
            let name = c.name.trim();
            if name.is_empty() || dst.cols.iter().any(|b| b.name.trim() == name) {
                continue;
            }
            if c.not_null && c.default.trim().is_empty() {
                notes.push(format!("新增非空列 {name} 没有默认值，旧行搬过去会失败"));
            }
        }
        for b in &dst.cols {
            let name = b.name.trim();
            if name.is_empty() || src.cols.iter().any(|c| c.name.trim() == name) {
                continue;
            }
            notes.push(format!("重建后不再有列 {name}，该列数据会丢失"));
        }
        return (sqlite_rebuild(table, src, dst), notes, Vec::new());
    }
    let d = table_diff(kind, schema, table, src, dst);
    let mut out = String::new();
    for stmt in d.head.iter().chain(d.mid.iter()).chain(d.tail.iter()) {
        out.push_str(stmt);
        out.push('\n');
    }
    (out.trim_end().to_string(), d.changes, d.notes)
}

fn failed_obj(cat: u8, name: &str, msg: &str) -> ObjDiff {
    ObjDiff {
        cat,
        name: name.to_string(),
        status: DiffStatus::Changed,
        detail: format!("读取失败：{msg}"),
        src_ddl: String::new(),
        dst_ddl: String::new(),
        script: format!("-- 读取失败，未生成语句：{msg}"),
        items: Vec::new(),
    }
}

/// 索引名：没填时按方言默认口径补（与 table_diff 里的同一套规则）。
fn index_name_of(is_mysql: bool, table: &str, idx: &crate::DesignIndex) -> String {
    let name = idx.name.trim();
    if !name.is_empty() {
        return name.to_string();
    }
    let fields: Vec<_> = idx
        .fields
        .iter()
        .filter(|f| !f.name.trim().is_empty())
        .collect();
    crate::design_index_auto_name(is_mysql, table, &fields)
}

fn col_note(c: &crate::DesignCol) -> String {
    let mut s = crate::design_full_type(c);
    if c.not_null {
        s.push_str(" · 非空");
    }
    if !c.default.trim().is_empty() {
        s.push_str(&format!(" · 默认 {}", c.default.trim()));
    }
    s
}

fn index_note(idx: &crate::DesignIndex) -> String {
    let fields: Vec<String> = idx
        .fields
        .iter()
        .filter(|f| !f.name.trim().is_empty())
        .map(|f| {
            let mut n = f.name.trim().to_string();
            if !f.sort.trim().is_empty() {
                n.push(' ');
                n.push_str(f.sort.trim());
            }
            n
        })
        .collect();
    let mut s = fields.join(", ");
    if idx.unique {
        s.push_str(" · 唯一");
    }
    s
}

fn fk_note(fk: &crate::DesignForeignKey) -> String {
    let mut target = fk.ref_table.trim().to_string();
    if !fk.ref_schema.trim().is_empty() {
        target = format!("{}.{target}", fk.ref_schema.trim());
    }
    format!(
        "{} → {}.{}",
        fk.columns.trim(),
        target,
        fk.ref_columns.trim()
    )
}

fn options_note(o: &crate::DesignOptions) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !o.comment.trim().is_empty() {
        parts.push(format!("备注 {}", o.comment.trim()));
    }
    if !o.fillfactor.trim().is_empty() {
        parts.push(format!("填充因子 {}", o.fillfactor.trim()));
    }
    if !o.tablespace.trim().is_empty() {
        parts.push(format!("表空间 {}", o.tablespace.trim()));
    }
    if !o.owner.trim().is_empty() {
        parts.push(format!("属主 {}", o.owner.trim()));
    }
    if o.unlogged {
        parts.push("不记录".to_string());
    }
    if !o.inherit.trim().is_empty() {
        parts.push(format!("继承 {}", o.inherit.trim()));
    }
    if !o.cluster.trim().is_empty() {
        parts.push(format!("集群 {}", o.cluster.trim()));
    }
    if parts.is_empty() {
        "默认选项".to_string()
    } else {
        parts.join(" · ")
    }
}

fn pk_names(m: &crate::DesignMeta) -> Vec<String> {
    m.cols
        .iter()
        .filter(|c| c.pk && !c.name.trim().is_empty())
        .map(|c| c.name.trim().to_string())
        .collect()
}

/// 同一张表两端都在时的明细：字段 / 主键 / 唯一键 / 检查 / 外键 / 索引 / 表选项。
fn diff_table_items(
    is_mysql: bool,
    table: &str,
    src: &crate::DesignMeta,
    dst: &crate::DesignMeta,
) -> Vec<DiffItem> {
    let mut out: Vec<DiffItem> = Vec::new();
    for c in &src.cols {
        let name = col_name(c);
        if name.is_empty() {
            continue;
        }
        let status = match dst.cols.iter().find(|b| col_name(b) == name) {
            None => DiffStatus::New,
            Some(b) => {
                // 只动了主键勾由主键行负责
                let mut x = c.clone();
                x.pk = b.pk;
                if x == *b {
                    DiffStatus::Same
                } else {
                    DiffStatus::Changed
                }
            }
        };
        out.push(DiffItem {
            label: name,
            note: col_note(c),
            kind: ItemKind::Column,
            status,
        });
    }
    for b in &dst.cols {
        let name = col_name(b);
        if name.is_empty() || src.cols.iter().any(|c| col_name(c) == name) {
            continue;
        }
        out.push(DiffItem {
            label: name,
            note: col_note(b),
            kind: ItemKind::Column,
            status: DiffStatus::Removed,
        });
    }

    let (pk_src, pk_dst) = (pk_names(src), pk_names(dst));
    if !pk_src.is_empty() || !pk_dst.is_empty() {
        let status = if pk_src == pk_dst {
            DiffStatus::Same
        } else if pk_dst.is_empty() {
            DiffStatus::New
        } else if pk_src.is_empty() {
            DiffStatus::Removed
        } else {
            DiffStatus::Changed
        };
        let list = if pk_src.is_empty() { &pk_dst } else { &pk_src };
        out.push(DiffItem {
            label: "主键".into(),
            note: list.join(", "),
            kind: ItemKind::PrimaryKey,
            status,
        });
    }

    {
        let mut pairs = |src: &[crate::DesignPair], dst: &[crate::DesignPair], kind: ItemKind| {
            for a in src {
                if a.name.trim().is_empty() {
                    continue;
                }
                let status = match dst.iter().find(|b| b.name.trim() == a.name.trim()) {
                    None => DiffStatus::New,
                    Some(b)
                        if a.value.trim() == b.value.trim()
                            && a.comment.trim() == b.comment.trim()
                            && a.no_inherit == b.no_inherit =>
                    {
                        DiffStatus::Same
                    }
                    Some(_) => DiffStatus::Changed,
                };
                out.push(DiffItem {
                    label: a.name.trim().to_string(),
                    note: a.value.trim().to_string(),
                    kind,
                    status,
                });
            }
            for b in dst {
                if b.name.trim().is_empty() || src.iter().any(|a| a.name.trim() == b.name.trim()) {
                    continue;
                }
                out.push(DiffItem {
                    label: b.name.trim().to_string(),
                    note: b.value.trim().to_string(),
                    kind,
                    status: DiffStatus::Removed,
                });
            }
        };
        pairs(&src.uniques, &dst.uniques, ItemKind::Unique);
        pairs(&src.checks, &dst.checks, ItemKind::Check);
    }

    for fk in &src.fks {
        if fk.name.trim().is_empty() {
            continue;
        }
        let status = match dst.fks.iter().find(|b| b.name.trim() == fk.name.trim()) {
            None => DiffStatus::New,
            Some(b) if b == fk => DiffStatus::Same,
            Some(_) => DiffStatus::Changed,
        };
        out.push(DiffItem {
            label: fk.name.trim().to_string(),
            note: fk_note(fk),
            kind: ItemKind::ForeignKey,
            status,
        });
    }
    for b in &dst.fks {
        if b.name.trim().is_empty() || src.fks.iter().any(|f| f.name.trim() == b.name.trim()) {
            continue;
        }
        out.push(DiffItem {
            label: b.name.trim().to_string(),
            note: fk_note(b),
            kind: ItemKind::ForeignKey,
            status: DiffStatus::Removed,
        });
    }

    for idx in &src.indexes {
        if crate::design_index_blank(idx) {
            continue;
        }
        let name = index_name_of(is_mysql, table, idx);
        let status = match dst
            .indexes
            .iter()
            .find(|b| index_name_of(is_mysql, table, b) == name)
        {
            None => DiffStatus::New,
            Some(b) if b == idx => DiffStatus::Same,
            Some(_) => DiffStatus::Changed,
        };
        out.push(DiffItem {
            label: name,
            note: index_note(idx),
            kind: ItemKind::Index,
            status,
        });
    }
    for b in &dst.indexes {
        if crate::design_index_blank(b) {
            continue;
        }
        let name = index_name_of(is_mysql, table, b);
        if src
            .indexes
            .iter()
            .any(|idx| index_name_of(is_mysql, table, idx) == name)
        {
            continue;
        }
        out.push(DiffItem {
            label: name,
            note: index_note(b),
            kind: ItemKind::Index,
            status: DiffStatus::Removed,
        });
    }

    out.push(DiffItem {
        label: "表选项".into(),
        note: options_note(&src.options),
        kind: ItemKind::Option,
        status: if src.options == dst.options {
            DiffStatus::Same
        } else {
            DiffStatus::Changed
        },
    });
    out
}

/// 只有一端存在的表：列 / 主键 / 约束 / 索引 / 表选项全按同一状态列出。
fn meta_items(
    is_mysql: bool,
    table: &str,
    meta: &crate::DesignMeta,
    status: DiffStatus,
) -> Vec<DiffItem> {
    let mut out: Vec<DiffItem> = Vec::new();
    for c in &meta.cols {
        let name = col_name(c);
        if name.is_empty() {
            continue;
        }
        out.push(DiffItem {
            label: name,
            note: col_note(c),
            kind: ItemKind::Column,
            status,
        });
    }
    let pk = pk_names(meta);
    if !pk.is_empty() {
        out.push(DiffItem {
            label: "主键".into(),
            note: pk.join(", "),
            kind: ItemKind::PrimaryKey,
            status,
        });
    }
    for u in &meta.uniques {
        if u.name.trim().is_empty() {
            continue;
        }
        out.push(DiffItem {
            label: u.name.trim().to_string(),
            note: u.value.trim().to_string(),
            kind: ItemKind::Unique,
            status,
        });
    }
    for ck in &meta.checks {
        if ck.name.trim().is_empty() {
            continue;
        }
        out.push(DiffItem {
            label: ck.name.trim().to_string(),
            note: ck.value.trim().to_string(),
            kind: ItemKind::Check,
            status,
        });
    }
    for fk in &meta.fks {
        if fk.name.trim().is_empty() {
            continue;
        }
        out.push(DiffItem {
            label: fk.name.trim().to_string(),
            note: fk_note(fk),
            kind: ItemKind::ForeignKey,
            status,
        });
    }
    for idx in &meta.indexes {
        if crate::design_index_blank(idx) {
            continue;
        }
        out.push(DiffItem {
            label: index_name_of(is_mysql, table, idx),
            note: index_note(idx),
            kind: ItemKind::Index,
            status,
        });
    }
    out.push(DiffItem {
        label: "表选项".into(),
        note: options_note(&meta.options),
        kind: ItemKind::Option,
        status,
    });
    out
}

fn table_items(
    is_mysql: bool,
    table: &str,
    src: Option<&crate::DesignMeta>,
    dst: Option<&crate::DesignMeta>,
) -> Vec<DiffItem> {
    match (src, dst) {
        (Some(a), Some(b)) => diff_table_items(is_mysql, table, a, b),
        (Some(a), None) => meta_items(is_mysql, table, a, DiffStatus::New),
        (None, Some(b)) => meta_items(is_mysql, table, b, DiffStatus::Removed),
        (None, None) => Vec::new(),
    }
}

fn record(
    objects: &mut Vec<ObjDiff>,
    counts: &mut [usize; 4],
    progress: &Arc<Mutex<(usize, usize)>>,
    done: &mut usize,
    total: usize,
    mut obj: ObjDiff,
) {
    // 视图 / 例程 / 读取失败只给一条「定义」明细，表由 table_items 填
    if obj.items.is_empty() {
        obj.items = vec![DiffItem {
            label: obj.cat_label().to_string(),
            note: obj.detail.clone(),
            kind: ItemKind::Definition,
            status: obj.status,
        }];
    }
    counts[obj.status.ix()] += 1;
    objects.push(obj);
    *done += 1;
    *progress.lock().unwrap() = (*done, total);
}

// ── 批量读取：把「每表 N 条查询」压成「每库 N 条查询」─────────────

type MetaLoad = Result<crate::DesignMeta, String>;

fn sorted_keys(map: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut v: Vec<String> = map.keys().cloned().collect();
    v.sort();
    v
}

fn meta_lookup<'a>(
    map: &'a HashMap<String, MetaLoad>,
    name: &str,
) -> Result<Option<&'a crate::DesignMeta>, String> {
    match map.get(name) {
        Some(Ok(m)) => Ok(Some(m)),
        Some(Err(e)) => Err(e.clone()),
        None => Err("没有读到表结构".to_string()),
    }
}

/// 目录查询可能返回上万行（整库列 / 约束），比单表查询的 10k 上限放宽。
fn meta_rows_wide(tg: &Target, sql: &str) -> Result<Vec<Vec<String>>, String> {
    let out = tg.execute(sql, 200_000)?;
    Ok(out.select.map(|p| p.rows).unwrap_or_default())
}

/// 把带表名的行按表名分组，行里去掉首列（表名）。
fn split_by_first(rows: Vec<Vec<String>>) -> HashMap<String, Vec<Vec<String>>> {
    let mut out: HashMap<String, Vec<Vec<String>>> = HashMap::new();
    for mut row in rows {
        if row.is_empty() {
            continue;
        }
        let name = row.remove(0);
        out.entry(name).or_default().push(row);
    }
    out
}

/// 表结构：MySQL 整库 7 条查询；SQLite 没有批量 pragma 的稳妥写法，
/// 保持逐表读（库文件在本地，往返代价小）。
fn load_table_metas(
    tg: &Target,
    kind: DbKind,
    side: &SideSpec,
    names: &[String],
) -> Result<HashMap<String, MetaLoad>, String> {
    if names.is_empty() {
        return Ok(HashMap::new());
    }
    match kind {
        DbKind::MySql => load_mysql_metas(tg, &side.db),
        DbKind::PostgreSql => load_pg_metas(tg, &side.schema),
        _ => Ok(names
            .iter()
            .map(|n| (n.clone(), crate::read_design_meta(tg, n)))
            .collect()),
    }
}

struct MySqlRows {
    cols: Vec<Vec<String>>,
    cons: Vec<Vec<String>>,
    kcu: Vec<Vec<String>>,
    rules: Vec<Vec<String>>,
    checks: Vec<Vec<String>>,
    indexes: Vec<Vec<String>>,
    comment: String,
}

fn load_mysql_metas(tg: &Target, db: &str) -> Result<HashMap<String, MetaLoad>, String> {
    let dbl = sql_lit(db);
    let cols = split_by_first(meta_rows_wide(
        tg,
        &format!(
            "SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, \
             COALESCE(COLUMN_DEFAULT, ''), COALESCE(COLUMN_COMMENT, ''), \
             COALESCE(COLLATION_NAME, ''), COALESCE(EXTRA, ''), COALESCE(COLUMN_KEY, '') \
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = '{dbl}' \
             ORDER BY TABLE_NAME, ORDINAL_POSITION"
        ),
    )?);
    let cons = split_by_first(meta_rows_wide(
        tg,
        &format!(
            "SELECT TABLE_NAME, CONSTRAINT_NAME, CONSTRAINT_TYPE \
             FROM information_schema.TABLE_CONSTRAINTS WHERE TABLE_SCHEMA = '{dbl}' \
             ORDER BY TABLE_NAME, CONSTRAINT_NAME"
        ),
    )?);
    let kcu = split_by_first(meta_rows_wide(
        tg,
        &format!(
            "SELECT TABLE_NAME, CONSTRAINT_NAME, COLUMN_NAME, \
             COALESCE(REFERENCED_TABLE_NAME, ''), COALESCE(REFERENCED_COLUMN_NAME, ''), \
             ORDINAL_POSITION \
             FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = '{dbl}' \
             ORDER BY TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION"
        ),
    )?);
    // 旧版 MariaDB 没有 REFERENTIAL_CONSTRAINTS / CHECK_CONSTRAINTS：读不到时
    // 退化成「没有这一项」，与其他目录项读失败的口径一致
    let rules = split_by_first(crate::meta_rows_or_empty(
        tg,
        &format!(
            "SELECT TABLE_NAME, CONSTRAINT_NAME, DELETE_RULE, UPDATE_RULE \
             FROM information_schema.REFERENTIAL_CONSTRAINTS \
             WHERE CONSTRAINT_SCHEMA = '{dbl}'"
        ),
    ));
    let checks = split_by_first(crate::meta_rows_or_empty(
        tg,
        &format!(
            "SELECT tc.TABLE_NAME, tc.CONSTRAINT_NAME, cc.CHECK_CLAUSE \
             FROM information_schema.TABLE_CONSTRAINTS tc \
             JOIN information_schema.CHECK_CONSTRAINTS cc \
               ON cc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA \
              AND cc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME \
             WHERE tc.TABLE_SCHEMA = '{dbl}' AND tc.CONSTRAINT_TYPE = 'CHECK' \
             ORDER BY tc.TABLE_NAME, tc.CONSTRAINT_NAME"
        ),
    ));
    let indexes = split_by_first(meta_rows_wide(
        tg,
        &format!(
            "SELECT TABLE_NAME, INDEX_NAME, COALESCE(COLUMN_NAME, ''), NON_UNIQUE, \
             INDEX_TYPE, COALESCE(COLLATION, ''), COALESCE(SUB_PART, '') \
             FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = '{dbl}' AND INDEX_NAME <> 'PRIMARY' \
             ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX"
        ),
    )?);
    // 表清单以 TABLES 为准：COLUMNS 里还混着视图的列
    let tables = meta_rows_wide(
        tg,
        &format!(
            "SELECT TABLE_NAME, COALESCE(TABLE_COMMENT, '') FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = '{dbl}' AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME"
        ),
    )?;
    let mut out = HashMap::new();
    for row in tables {
        let Some(name) = row.first().cloned() else {
            continue;
        };
        let rows = MySqlRows {
            cols: cols.get(&name).cloned().unwrap_or_default(),
            cons: cons.get(&name).cloned().unwrap_or_default(),
            kcu: kcu.get(&name).cloned().unwrap_or_default(),
            rules: rules.get(&name).cloned().unwrap_or_default(),
            checks: checks.get(&name).cloned().unwrap_or_default(),
            indexes: indexes.get(&name).cloned().unwrap_or_default(),
            comment: row.get(1).cloned().unwrap_or_default(),
        };
        out.insert(name, Ok(mysql_meta_from_rows(rows)));
    }
    Ok(out)
}

/// 解析口径与 `read_mysql_design` 逐项一致，只是行是从整库结果里按表切出来的。
fn mysql_meta_from_rows(r: MySqlRows) -> crate::DesignMeta {
    let mut meta = crate::DesignMeta::default();
    for row in r.cols {
        if row.len() < 8 {
            continue;
        }
        let (ty, len, dec) = crate::split_column_type(&row[1]);
        let extra = row[6].to_ascii_lowercase();
        let mut default = row[3].clone();
        if let Some(pos) = extra.find("on update ") {
            if !default.trim().is_empty() {
                default.push_str(" ON UPDATE ");
                default.push_str(row[6][pos + 10..].trim().to_ascii_uppercase().as_str());
            }
        }
        meta.cols.push(crate::DesignCol {
            name: row[0].clone(),
            ty,
            len,
            dec,
            not_null: row[2].eq_ignore_ascii_case("NO"),
            pk: row[7].eq_ignore_ascii_case("PRI"),
            auto_inc: extra.contains("auto_increment"),
            comment: row[4].clone(),
            default,
            collation: row[5].clone(),
            identity: String::new(),
        });
    }

    let mut local: HashMap<String, Vec<String>> = HashMap::new();
    let mut ref_cols: HashMap<String, Vec<String>> = HashMap::new();
    let mut ref_table: HashMap<String, String> = HashMap::new();
    for row in r.kcu {
        if row.len() < 5 {
            continue;
        }
        local
            .entry(row[0].clone())
            .or_default()
            .push(row[1].clone());
        if !row[2].is_empty() {
            ref_table
                .entry(row[0].clone())
                .or_insert_with(|| row[2].clone());
            ref_cols
                .entry(row[0].clone())
                .or_default()
                .push(row[3].clone());
        }
    }
    let mut fk_names: Vec<String> = Vec::new();
    let mut unique_names: HashSet<String> = HashSet::new();
    for row in r.cons {
        if row.len() < 2 {
            continue;
        }
        match row[1].as_str() {
            "UNIQUE" => {
                unique_names.insert(row[0].clone());
                meta.uniques.push(crate::DesignPair {
                    name: row[0].clone(),
                    value: local.get(&row[0]).map(|v| v.join(", ")).unwrap_or_default(),
                    ..Default::default()
                });
            }
            "FOREIGN KEY" => fk_names.push(row[0].clone()),
            _ => {}
        }
    }

    if !fk_names.is_empty() {
        let mut del = HashMap::new();
        let mut upd = HashMap::new();
        for row in r.rules {
            if row.len() < 3 {
                continue;
            }
            del.insert(row[0].clone(), row[1].clone());
            upd.insert(row[0].clone(), row[2].clone());
        }
        for name in fk_names {
            meta.fks.push(crate::DesignForeignKey {
                name: name.clone(),
                columns: local.get(&name).map(|v| v.join(", ")).unwrap_or_default(),
                ref_schema: String::new(),
                ref_table: ref_table.get(&name).cloned().unwrap_or_default(),
                ref_columns: ref_cols
                    .get(&name)
                    .map(|v| v.join(", "))
                    .unwrap_or_default(),
                on_delete: del
                    .get(&name)
                    .map(|s| s.to_ascii_uppercase())
                    .unwrap_or_else(|| "NO ACTION".into()),
                on_update: upd
                    .get(&name)
                    .map(|s| s.to_ascii_uppercase())
                    .unwrap_or_else(|| "NO ACTION".into()),
                comment: String::new(),
            });
        }
    }

    for row in r.checks {
        if row.len() < 2 {
            continue;
        }
        meta.checks.push(crate::DesignPair {
            name: row[0].clone(),
            value: crate::strip_redundant_parens(&row[1]).to_string(),
            ..Default::default()
        });
    }

    let ty_of = |name: &str| -> String {
        meta.cols
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.ty.clone())
            .unwrap_or_default()
    };
    let expr_idx: HashSet<String> = r
        .indexes
        .iter()
        .filter(|row| row.len() >= 2 && !row[0].is_empty() && row[1].is_empty())
        .map(|row| row[0].clone())
        .collect();
    let mut idx_order: Vec<String> = Vec::new();
    let mut idx_map: HashMap<String, crate::DesignIndex> = HashMap::new();
    for row in r.indexes {
        if row.len() < 5 {
            continue;
        }
        let name = row[0].clone();
        if name.is_empty() || unique_names.contains(&name) || expr_idx.contains(&name) {
            continue;
        }
        let entry = idx_map.entry(name.clone()).or_insert_with(|| {
            idx_order.push(name.clone());
            let t = row[3].trim().to_ascii_uppercase();
            let special = t == "FULLTEXT" || t == "SPATIAL";
            crate::DesignIndex {
                name,
                method: if special { String::new() } else { t.clone() },
                kind: if special { t } else { String::new() },
                ..Default::default()
            }
        });
        let mut f = crate::IndexField::make(&row[1], &ty_of(&row[1]));
        f.collate_schema.clear();
        f.collation.clear();
        f.opclass_schema.clear();
        f.opclass.clear();
        if row.len() >= 6 {
            f.sub_part = row[5].trim().to_string();
        }
        if row[4].eq_ignore_ascii_case("D") {
            f.sort = "DESC".into();
        }
        entry.fields.push(f);
        if row[2] == "0" {
            entry.unique = true;
        }
    }
    meta.indexes = idx_order
        .into_iter()
        .filter_map(|n| idx_map.remove(&n))
        .collect();
    meta.options.comment = r.comment;
    meta
}

struct PgRows {
    cols: Vec<Vec<String>>,
    cons: Vec<Vec<String>>,
    indexes: Vec<Vec<String>>,
    options: Option<Vec<String>>,
}

fn load_pg_metas(tg: &Target, schema: &str) -> Result<HashMap<String, MetaLoad>, String> {
    let sch = sql_lit(schema);
    let cols = split_by_first(meta_rows_wide(
        tg,
        &format!(
            "SELECT cl.relname, a.attname, format_type(a.atttypid, a.atttypmod), \
             CASE WHEN a.attnotnull THEN 't' ELSE 'f' END, \
             COALESCE(pg_get_expr(d.adbin, d.adrelid), ''), \
             COALESCE(col_description(a.attrelid, a.attnum), ''), \
             COALESCE((SELECT collname FROM pg_collation WHERE oid = a.attcollation), ''), \
             a.attidentity \
             FROM pg_attribute a \
             JOIN pg_class cl ON cl.oid = a.attrelid \
             JOIN pg_namespace nc ON nc.oid = cl.relnamespace \
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
             WHERE nc.nspname = '{sch}' AND cl.relkind IN ('r', 'p') \
               AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY cl.relname, a.attnum"
        ),
    )?);
    let cons = split_by_first(meta_rows_wide(
        tg,
        &format!(
            "SELECT cl.relname, con.conname, con.contype, \
             COALESCE((SELECT string_agg(pa.attname, ',' ORDER BY k.ord) \
               FROM generate_subscripts(con.conkey, 1) AS k(ord) \
               JOIN pg_attribute pa ON pa.attrelid = con.conrelid \
                AND pa.attnum = con.conkey[k.ord]), ''), \
             COALESCE((SELECT string_agg(ra.attname, ',' ORDER BY k.ord) \
               FROM generate_subscripts(con.confkey, 1) AS k(ord) \
               JOIN pg_attribute ra ON ra.attrelid = con.confrelid \
                AND ra.attnum = con.confkey[k.ord]), ''), \
             COALESCE((SELECT rn.nspname FROM pg_class rc \
               JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
               WHERE rc.oid = con.confrelid), ''), \
             COALESCE((SELECT rc.relname FROM pg_class rc WHERE rc.oid = con.confrelid), ''), \
             con.confdeltype, con.confupdtype, \
             COALESCE(CASE WHEN con.contype = 'c' THEN pg_get_constraintdef(con.oid) END, '') \
             FROM pg_constraint con \
             JOIN pg_class cl ON cl.oid = con.conrelid \
             JOIN pg_namespace nc ON nc.oid = cl.relnamespace \
             WHERE nc.nspname = '{sch}' AND cl.relkind IN ('r', 'p') \
             ORDER BY cl.relname, con.conname"
        ),
    )?);
    let indexes = split_by_first(meta_rows_wide(
        tg,
        &format!(
            "SELECT t.relname, ic.relname, am.amname, a.attname, \
             CASE WHEN (i.indoption[k.ord] & 1) = 1 THEN 'DESC' ELSE 'ASC' END, \
             CASE WHEN (i.indoption[k.ord] & 2) = 2 THEN 'NULLS FIRST' ELSE 'NULLS LAST' END, \
             CASE WHEN i.indisunique THEN 't' ELSE 'f' END \
             FROM pg_index i \
             JOIN pg_class ic ON ic.oid = i.indexrelid \
             JOIN pg_am am ON am.oid = ic.relam \
             JOIN pg_class t ON t.oid = i.indrelid \
             JOIN pg_namespace n ON n.oid = t.relnamespace \
             CROSS JOIN LATERAL generate_subscripts(i.indkey, 1) AS k(ord) \
             JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = i.indkey[k.ord] \
             WHERE n.nspname = '{sch}' AND t.relkind IN ('r', 'p') \
               AND NOT i.indisprimary \
               AND i.indexprs IS NULL \
               AND NOT EXISTS (SELECT 1 FROM pg_constraint c WHERE c.conindid = i.indexrelid) \
             ORDER BY t.relname, ic.relname, k.ord"
        ),
    )?);
    let mut options: HashMap<String, Vec<String>> = HashMap::new();
    for row in meta_rows_wide(
        tg,
        &format!(
            "SELECT c.relname, COALESCE(obj_description(c.oid, 'pg_class'), ''), \
             COALESCE((SELECT split_part(o, '=', 2) FROM unnest(c.reloptions) AS o \
               WHERE o LIKE 'fillfactor=%' LIMIT 1), ''), \
             COALESCE((SELECT ts.spcname FROM pg_tablespace ts \
               WHERE ts.oid = c.reltablespace), ''), \
             c.relpersistence, COALESCE(pg_get_userbyid(c.relowner), ''), \
             COALESCE((SELECT string_agg(pn.nspname || '.' || pc.relname, ', ') \
               FROM pg_inherits ih \
               JOIN pg_class pc ON pc.oid = ih.inhparent \
               JOIN pg_namespace pn ON pn.oid = pc.relnamespace \
               WHERE ih.inhrelid = c.oid), ''), \
             COALESCE((SELECT ic.relname FROM pg_index ci \
               JOIN pg_class ic ON ic.oid = ci.indexrelid \
               WHERE ci.indrelid = c.oid AND ci.indisclustered LIMIT 1), '') \
             FROM pg_class c \
             JOIN pg_namespace nc ON nc.oid = c.relnamespace \
             WHERE nc.nspname = '{sch}' AND c.relkind IN ('r', 'p')"
        ),
    )? {
        let Some(name) = row.first().cloned() else {
            continue;
        };
        options.insert(name, row.into_iter().skip(1).collect());
    }
    let mut out = HashMap::new();
    for name in options.keys() {
        let rows = PgRows {
            cols: cols.get(name).cloned().unwrap_or_default(),
            cons: cons.get(name).cloned().unwrap_or_default(),
            indexes: indexes.get(name).cloned().unwrap_or_default(),
            options: options.get(name).cloned(),
        };
        out.insert(name.clone(), Ok(pg_meta_from_rows(rows)));
    }
    Ok(out)
}

fn pg_meta_from_rows(r: PgRows) -> crate::DesignMeta {
    let mut meta = crate::DesignMeta::default();
    for row in r.cols {
        if row.len() < 7 {
            continue;
        }
        let (ty, len, dec) = crate::split_column_type(&row[1]);
        meta.cols.push(crate::DesignCol {
            name: row[0].clone(),
            ty,
            len,
            dec,
            not_null: row[2] == "t",
            pk: false,
            auto_inc: false,
            comment: row[4].clone(),
            default: row[3].clone(),
            collation: row[5].clone(),
            identity: match row[6].as_str() {
                "a" => "ALWAYS".into(),
                "d" => "BY DEFAULT".into(),
                _ => String::new(),
            },
        });
    }

    let mut pk_cols: HashSet<String> = HashSet::new();
    for row in r.cons {
        if row.len() < 9 {
            continue;
        }
        let name = row[0].clone();
        let cols = row[2].clone();
        match row[1].as_str() {
            "p" => pk_cols.extend(cols.split(',').map(|s| s.trim().to_string())),
            "u" => meta.uniques.push(crate::DesignPair {
                name,
                value: cols,
                ..Default::default()
            }),
            "f" => meta.fks.push(crate::DesignForeignKey {
                name,
                columns: cols,
                ref_schema: row[4].clone(),
                ref_table: row[5].clone(),
                ref_columns: row[3].clone(),
                on_delete: crate::fk_action(&row[6]),
                on_update: crate::fk_action(&row[7]),
                comment: String::new(),
            }),
            "c" => {
                let mut def = row[8].clone();
                let no_inherit = def.trim_end().to_ascii_uppercase().ends_with("NO INHERIT");
                if no_inherit {
                    def.truncate(def.trim_end().len() - "NO INHERIT".len());
                }
                meta.checks.push(crate::DesignPair {
                    name,
                    value: crate::pg_check_expr(&def),
                    no_inherit,
                    ..Default::default()
                });
            }
            _ => {}
        }
    }
    if !pk_cols.is_empty() {
        for c in &mut meta.cols {
            c.pk = pk_cols.contains(&c.name);
        }
    }

    let ty_of = |name: &str| -> String {
        meta.cols
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.ty.clone())
            .unwrap_or_default()
    };
    let mut idx_order: Vec<String> = Vec::new();
    let mut idx_map: HashMap<String, crate::DesignIndex> = HashMap::new();
    for row in r.indexes {
        if row.len() < 6 {
            continue;
        }
        let name = row[0].clone();
        if name.is_empty() || row[2].is_empty() {
            continue;
        }
        let entry = idx_map.entry(name.clone()).or_insert_with(|| {
            idx_order.push(name.clone());
            crate::DesignIndex {
                name,
                method: row[1].clone(),
                unique: row[5] == "t",
                ..Default::default()
            }
        });
        let mut f = crate::IndexField::make(&row[2], &ty_of(&row[2]));
        f.sort = row[3].clone();
        f.nulls = row[4].clone();
        entry.fields.push(f);
    }
    meta.indexes = idx_order
        .into_iter()
        .filter_map(|n| idx_map.remove(&n))
        .collect();

    if let Some(row) = r.options {
        if row.len() >= 7 {
            meta.options.comment = row[0].clone();
            meta.options.fillfactor = row[1].clone();
            meta.options.tablespace = row[2].clone();
            meta.options.unlogged = row[3] == "u";
            meta.options.owner = row[4].clone();
            meta.options.inherit = row[5].clone();
            meta.options.cluster = row[6].clone();
        }
    }
    meta
}

/// 视图 / 实体化视图定义：每端一条查询取回全部正文。
fn load_views(
    tg: &Target,
    kind: DbKind,
    side: &SideSpec,
    cat: u8,
) -> Result<HashMap<String, String>, String> {
    let sql = match kind {
        DbKind::PostgreSql => {
            let sch = sql_lit(&side.schema);
            if cat == CAT_MATVIEW {
                format!(
                    "SELECT matviewname, COALESCE(definition, '') FROM pg_matviews \
                     WHERE schemaname = '{sch}'"
                )
            } else {
                format!(
                    "SELECT viewname, COALESCE(definition, '') FROM pg_views \
                     WHERE schemaname = '{sch}'"
                )
            }
        }
        DbKind::MySql => format!(
            "SELECT TABLE_NAME, COALESCE(VIEW_DEFINITION, '') \
             FROM information_schema.views WHERE TABLE_SCHEMA = '{}'",
            sql_lit(&side.db)
        ),
        DbKind::SQLite => format!(
            "SELECT name, COALESCE(sql, '') FROM {}.sqlite_master WHERE type = 'view'",
            crate::sqlite_ident(&side.db)
        ),
        _ => return Ok(HashMap::new()),
    };
    let mut out = HashMap::new();
    for row in meta_rows_wide(tg, &sql)? {
        let Some(name) = row.first() else {
            continue;
        };
        let mut body = row.get(1).cloned().unwrap_or_default();
        if kind == DbKind::SQLite {
            body = crate::strip_create_view(&body);
        }
        out.insert(name.clone(), body.trim().to_string());
    }
    Ok(out)
}

#[derive(Default)]
struct RoutineDefs {
    funcs: HashMap<String, Vec<String>>,
    procs: HashMap<String, Vec<String>>,
}

/// 例程定义：PG 一条 `pg_get_functiondef` 查询全取；MySQL 只有
/// `SHOW CREATE` 能给出带参数签名的完整语句，按名字逐个取。
fn load_routines(
    tg: &Target,
    kind: DbKind,
    side: &SideSpec,
    opts: CompareOptions,
) -> Result<RoutineDefs, String> {
    let mut out = RoutineDefs::default();
    if !has_routines(kind) {
        return Ok(out);
    }
    if kind == DbKind::PostgreSql {
        let kinds = match (opts.funcs, opts.procs) {
            (true, true) => "'f','p'",
            (true, false) => "'f'",
            (false, true) => "'p'",
            (false, false) => return Ok(out),
        };
        let sql = format!(
            "SELECT p.proname, p.prokind::text, COALESCE(pg_get_functiondef(p.oid), '') \
             FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = '{}' AND p.prokind IN ({kinds}) \
             ORDER BY p.proname, p.oid",
            sql_lit(&side.schema)
        );
        for row in meta_rows_wide(tg, &sql)? {
            let (Some(name), Some(code)) = (row.first(), row.get(1)) else {
                continue;
            };
            let def = row.get(2).cloned().unwrap_or_default().trim().to_string();
            let bucket = if code == "p" {
                &mut out.procs
            } else {
                &mut out.funcs
            };
            bucket.entry(name.clone()).or_default().push(def);
        }
    } else {
        let sql = format!(
            "SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES \
             WHERE ROUTINE_SCHEMA = '{}' ORDER BY ROUTINE_NAME",
            sql_lit(&side.db)
        );
        let mut wanted: Vec<(String, u8)> = Vec::new();
        for row in crate::meta_rows(tg, &sql)? {
            let (Some(name), Some(ty)) = (row.first(), row.get(1)) else {
                continue;
            };
            let cat = match ty.to_ascii_uppercase().as_str() {
                "FUNCTION" => CAT_FUNC,
                "PROCEDURE" => CAT_PROC,
                _ => continue,
            };
            if opts.get(cat) {
                wanted.push((name.clone(), cat));
            }
        }
        for (name, cat) in wanted {
            let def = mysql_routine_ddl(tg, &side.db, cat, &name);
            let bucket = if cat == CAT_PROC {
                &mut out.procs
            } else {
                &mut out.funcs
            };
            bucket.entry(name).or_default().push(def);
        }
    }
    for defs in out.funcs.values_mut().chain(out.procs.values_mut()) {
        defs.sort();
        defs.dedup();
    }
    Ok(out)
}

fn mysql_routine_ddl(tg: &Target, db: &str, cat: u8, name: &str) -> String {
    let kw = if cat == CAT_PROC {
        "PROCEDURE"
    } else {
        "FUNCTION"
    };
    let qual = format!(
        "{}.{}",
        crate::design_quote(DbKind::MySql, db),
        crate::design_quote(DbKind::MySql, name)
    );
    // SHOW CREATE FUNCTION / PROCEDURE 的列序是 Function / sql_mode / Create ...，
    // 按内容挑以 CREATE 开头的那列，不依赖 MySQL / MariaDB 的列下标
    crate::meta_rows(tg, &format!("SHOW CREATE {kw} {qual}"))
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|row| {
            row.into_iter().find(|v| {
                v.trim_start()
                    .get(..6)
                    .is_some_and(|h| h.eq_ignore_ascii_case("create"))
            })
        })
        .unwrap_or_default()
}

fn ensure_semi(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() || t.ends_with(';') {
        t.to_string()
    } else {
        format!("{t};")
    }
}

/// 源端定义里的模式限定换成目标端（PG 的 `pg_get_functiondef` 带模式名）。
fn pg_qualifier_swap(def: &str, from: &str, to: &str) -> String {
    let (from, to) = (from.trim(), to.trim());
    if from.is_empty() || from == to {
        return def.to_string();
    }
    def.replace(&format!("\"{from}\"."), &format!("\"{to}\"."))
        .replace(&format!("{from}."), &format!("{to}."))
}

/// 例程定义改写到目标端并拼接：比较与增量脚本共用同一份文本。
fn routine_retarget(
    kind: DbKind,
    defs: &[String],
    src_schema: &str,
    dst_schema: &str,
    src_db: &str,
    dst_db: &str,
) -> String {
    defs.iter()
        .map(|d| match kind {
            DbKind::PostgreSql => pg_qualifier_swap(d, src_schema, dst_schema),
            DbKind::MySql => mysql_view_body(d, src_db, dst_db),
            _ => d.clone(),
        })
        .map(|d| ensure_semi(&d))
        .collect::<Vec<_>>()
        .join("\n")
}

fn routine_text(defs: &[String]) -> String {
    defs.iter()
        .map(|d| ensure_semi(d))
        .collect::<Vec<_>>()
        .join("\n")
}

fn routine_drop(
    kind: DbKind,
    schema: &str,
    db: &str,
    cat: u8,
    name: &str,
    commented: bool,
) -> String {
    let kw = if cat == CAT_PROC {
        "PROCEDURE"
    } else {
        "FUNCTION"
    };
    let qual = match kind {
        DbKind::PostgreSql => crate::design_qualify(kind, schema, name),
        DbKind::MySql if !db.trim().is_empty() => format!(
            "{}.{}",
            crate::design_quote(kind, db),
            crate::design_quote(kind, name)
        ),
        _ => crate::design_quote(kind, name),
    };
    let stmt = format!("DROP {kw} IF EXISTS {qual};");
    if commented {
        format!("-- {stmt}")
    } else {
        stmt
    }
}

fn compare_objects(
    src: &SideSpec,
    dst: &SideSpec,
    opts: CompareOptions,
    progress: &Arc<Mutex<(usize, usize)>>,
    cancel: &Arc<AtomicBool>,
) -> Result<CompareResult, String> {
    let src_tg = src.target().ok_or("源连接没有可用的查询驱动")?;
    let dst_tg = dst.target().ok_or("目标连接没有可用的查询驱动")?;
    let kind = src.conn.kind;
    let cats = opts.cats(kind);
    if cats.is_empty() {
        return Err("请至少勾选一类要比较的对象".to_string());
    }
    let mut objects: Vec<ObjDiff> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut counts = [0usize; 4];

    let mut src_lists = list_cats(src, &cats)?;
    let mut dst_lists = list_cats(dst, &cats)?;
    // 例程按函数 / 存储过程分开列，清单随定义一起取回
    let src_routines = load_routines(&src_tg, kind, src, opts)?;
    let dst_routines = load_routines(&dst_tg, kind, dst, opts)?;
    for (cat, listing) in [
        (CAT_FUNC, &src_routines.funcs),
        (CAT_PROC, &src_routines.procs),
    ] {
        if cats.contains(&cat) {
            src_lists[cat as usize] = sorted_keys(listing);
        }
    }
    for (cat, listing) in [
        (CAT_FUNC, &dst_routines.funcs),
        (CAT_PROC, &dst_routines.procs),
    ] {
        if cats.contains(&cat) {
            dst_lists[cat as usize] = sorted_keys(listing);
        }
    }
    // 表结构 / 视图定义整批读回：不再每张表、每个视图各来回一次
    let src_tables = if cats.contains(&CAT_TABLE) {
        load_table_metas(&src_tg, kind, src, &src_lists[CAT_TABLE as usize])?
    } else {
        HashMap::new()
    };
    let dst_tables = if cats.contains(&CAT_TABLE) {
        load_table_metas(&dst_tg, kind, dst, &dst_lists[CAT_TABLE as usize])?
    } else {
        HashMap::new()
    };
    let src_views = if cats.contains(&CAT_VIEW) {
        load_views(&src_tg, kind, src, CAT_VIEW)?
    } else {
        HashMap::new()
    };
    let dst_views = if cats.contains(&CAT_VIEW) {
        load_views(&dst_tg, kind, dst, CAT_VIEW)?
    } else {
        HashMap::new()
    };
    let src_mvs = if cats.contains(&CAT_MATVIEW) {
        load_views(&src_tg, kind, src, CAT_MATVIEW)?
    } else {
        HashMap::new()
    };
    let dst_mvs = if cats.contains(&CAT_MATVIEW) {
        load_views(&dst_tg, kind, dst, CAT_MATVIEW)?
    } else {
        HashMap::new()
    };
    let total: usize = cats
        .iter()
        .map(|c| {
            let s = src_lists.get(*c as usize).map(Vec::len).unwrap_or(0);
            let t = dst_lists.get(*c as usize).map(Vec::len).unwrap_or(0);
            s + t
        })
        .sum();
    *progress.lock().unwrap() = (0, total);
    let mut done = 0usize;
    // 目标端可能缺的属主：按目标连接的用户名粗筛，只提示一次，避免每张新表都刷一条
    let mut missing_owners: Vec<String> = Vec::new();

    for &cat in &cats {
        let empty: Vec<String> = Vec::new();
        let src_names = src_lists.get(cat as usize).unwrap_or(&empty);
        let dst_names = dst_lists.get(cat as usize).unwrap_or(&empty);
        let mut names: Vec<String> = src_names.iter().chain(dst_names.iter()).cloned().collect();
        names.sort();
        names.dedup();
        for name in names {
            if cancel.load(Ordering::Relaxed) {
                return Err("已取消".to_string());
            }
            let in_src = src_names.contains(&name);
            let in_dst = dst_names.contains(&name);
            if cat >= CAT_FUNC {
                // 函数 / 存储过程：整段定义比较；PG 的定义里带模式名，
                // 比之前先把源端模式换成目标端，否则跨模式比较恒判不同
                let (src_map, dst_map) = if cat == CAT_PROC {
                    (&src_routines.procs, &dst_routines.procs)
                } else {
                    (&src_routines.funcs, &dst_routines.funcs)
                };
                let a = if in_src {
                    Some(src_map.get(&name).cloned().unwrap_or_default())
                } else {
                    None
                };
                let b = if in_dst {
                    Some(dst_map.get(&name).cloned().unwrap_or_default())
                } else {
                    None
                };
                let obj = match (a, b) {
                    (Some(a), None) => {
                        if a.is_empty() {
                            warnings.push(format!("{name} 读不到源端定义（可能缺权限）"));
                            failed_obj(cat, &name, "读不到定义（可能缺权限）")
                        } else {
                            ObjDiff {
                                cat,
                                name: name.clone(),
                                status: DiffStatus::New,
                                detail: "源端有、目标端没有".into(),
                                src_ddl: routine_text(&a),
                                dst_ddl: String::new(),
                                // 脚本在目标端执行，限定名换到目标端
                                script: routine_retarget(
                                    kind,
                                    &a,
                                    &src.schema,
                                    &dst.schema,
                                    &src.db,
                                    &dst.db,
                                ),
                                items: Vec::new(),
                            }
                        }
                    }
                    (None, Some(b)) => ObjDiff {
                        cat,
                        name: name.clone(),
                        status: DiffStatus::Removed,
                        detail: "目标端多出、源端没有".into(),
                        src_ddl: String::new(),
                        dst_ddl: routine_text(&b),
                        script: format!(
                            "-- 目标端多出的{}；确认不再需要后取消下面一行的注释\n{}",
                            cat_title(cat),
                            routine_drop(kind, &dst.schema, &dst.db, cat, &name, true)
                        ),
                        items: Vec::new(),
                    },
                    (Some(a), Some(b)) => {
                        if a.is_empty() || b.is_empty() {
                            warnings.push(format!("{name} 有一端读不到定义（可能缺权限）"));
                            failed_obj(cat, &name, "读不到定义（可能缺权限）")
                        } else {
                            let src_ddl = routine_text(&a);
                            let dst_ddl = routine_text(&b);
                            let src_in_dst = routine_retarget(
                                kind,
                                &a,
                                &src.schema,
                                &dst.schema,
                                &src.db,
                                &dst.db,
                            );
                            if norm_sql(&src_in_dst) == norm_sql(&dst_ddl) {
                                ObjDiff {
                                    cat,
                                    name: name.clone(),
                                    status: DiffStatus::Same,
                                    detail: "定义一致".into(),
                                    src_ddl,
                                    dst_ddl,
                                    script: String::new(),
                                    items: Vec::new(),
                                }
                            } else {
                                // PG 支持 CREATE OR REPLACE；MySQL 要先 DROP
                                let script = if kind == DbKind::PostgreSql {
                                    src_in_dst
                                } else {
                                    format!(
                                        "{}\n{}",
                                        routine_drop(kind, &dst.schema, &dst.db, cat, &name, false),
                                        src_in_dst
                                    )
                                };
                                ObjDiff {
                                    cat,
                                    name: name.clone(),
                                    status: DiffStatus::Changed,
                                    detail: "定义不同".into(),
                                    src_ddl,
                                    dst_ddl,
                                    script,
                                    items: Vec::new(),
                                }
                            }
                        }
                    }
                    (None, None) => continue,
                };
                record(&mut objects, &mut counts, progress, &mut done, total, obj);
                continue;
            }
            if cat == CAT_TABLE {
                // 单个对象读失败不中断整轮比较，记一条带原因的差异项
                let src_meta = if in_src {
                    match meta_lookup(&src_tables, &name) {
                        Ok(m) => m,
                        Err(e) => {
                            warnings.push(format!("表 {name} 结构读取失败：{e}"));
                            record(
                                &mut objects,
                                &mut counts,
                                progress,
                                &mut done,
                                total,
                                failed_obj(cat, &name, &e),
                            );
                            continue;
                        }
                    }
                } else {
                    None
                };
                let dst_meta = if in_dst {
                    match meta_lookup(&dst_tables, &name) {
                        Ok(m) => m,
                        Err(e) => {
                            warnings.push(format!("表 {name} 结构读取失败：{e}"));
                            record(
                                &mut objects,
                                &mut counts,
                                progress,
                                &mut done,
                                total,
                                failed_obj(cat, &name, &e),
                            );
                            continue;
                        }
                    }
                } else {
                    None
                };
                let items = table_items(kind == DbKind::MySql, &name, src_meta, dst_meta);
                let mut obj = match (src_meta, dst_meta) {
                    (Some(a), None) => {
                        let owner = a.options.owner.trim();
                        if !owner.is_empty()
                            && !owner.eq_ignore_ascii_case(dst.conn.user.trim())
                            && !missing_owners.iter().any(|o| o == owner)
                        {
                            missing_owners.push(owner.to_string());
                        }
                        ObjDiff {
                            cat,
                            name: name.clone(),
                            status: DiffStatus::New,
                            detail: "源端有、目标端没有".into(),
                            src_ddl: create_script(kind, &src.schema, &name, &a),
                            dst_ddl: String::new(),
                            // 脚本在目标端执行，模式名要按目标端限定
                            script: create_script(kind, &dst.schema, &name, &a),
                            items: Vec::new(),
                        }
                    }
                    (None, Some(b)) => ObjDiff {
                        cat,
                        name: name.clone(),
                        status: DiffStatus::Removed,
                        detail: "目标端多出、源端没有".into(),
                        src_ddl: String::new(),
                        dst_ddl: create_script(kind, &dst.schema, &name, &b),
                        script: format!(
                            "-- 目标端多出的表；确认不再需要后取消下面一行的注释\n-- {};",
                            format!(
                                "DROP TABLE {}",
                                crate::design_qualify(kind, &dst.schema, &name)
                            )
                        ),
                        items: Vec::new(),
                    },
                    (Some(a), Some(b)) => {
                        let src_ddl = create_script(kind, &src.schema, &name, &a);
                        let dst_ddl = create_script(kind, &dst.schema, &name, &b);
                        if same_table(&a, &b) {
                            ObjDiff {
                                cat,
                                name: name.clone(),
                                status: DiffStatus::Same,
                                detail: "结构一致".into(),
                                src_ddl,
                                dst_ddl,
                                script: String::new(),
                                items: Vec::new(),
                            }
                        } else {
                            let (script, changes, notes) =
                                table_script(kind, &dst.schema, &name, &a, &b);
                            for n in &notes {
                                warnings.push(format!("{name}：{n}"));
                            }
                            ObjDiff {
                                cat,
                                name: name.clone(),
                                status: DiffStatus::Changed,
                                detail: changes.join(" / "),
                                src_ddl,
                                dst_ddl,
                                script,
                                items: Vec::new(),
                            }
                        }
                    }
                    (None, None) => continue,
                };
                obj.items = items;
                record(&mut objects, &mut counts, progress, &mut done, total, obj);
                continue;
            }

            // 视图 / 实体化视图：按定义正文比较（定义已整批取回）
            let (src_defs, dst_defs) = if cat == CAT_MATVIEW {
                (&src_mvs, &dst_mvs)
            } else {
                (&src_views, &dst_views)
            };
            let a = if in_src {
                Some(src_defs.get(&name).cloned().unwrap_or_default())
            } else {
                None
            };
            let b = if in_dst {
                Some(dst_defs.get(&name).cloned().unwrap_or_default())
            } else {
                None
            };
            let obj = match (a, b) {
                (Some(a), None) => {
                    if a.trim().is_empty() {
                        warnings.push(format!("{name} 读不到源端视图定义（可能缺权限）"));
                        failed_obj(cat, &name, "读不到视图定义（可能缺权限）")
                    } else {
                        let body = if kind == DbKind::MySql {
                            mysql_view_body(&a, &src.db, &dst.db)
                        } else {
                            a.clone()
                        };
                        let ddl = view_create(kind, &src.schema, &name, cat, &body);
                        ObjDiff {
                            cat,
                            name: name.clone(),
                            status: DiffStatus::New,
                            detail: "源端有、目标端没有".into(),
                            src_ddl: ddl,
                            dst_ddl: String::new(),
                            // 脚本在目标端执行，模式名要按目标端限定
                            script: view_create(kind, &dst.schema, &name, cat, &body),
                            items: Vec::new(),
                        }
                    }
                }
                (None, Some(b)) => ObjDiff {
                    cat,
                    name: name.clone(),
                    status: DiffStatus::Removed,
                    detail: "目标端多出、源端没有".into(),
                    src_ddl: String::new(),
                    dst_ddl: view_create(kind, &dst.schema, &name, cat, &b),
                    script: view_drop(kind, &dst.schema, &name, cat, true),
                    items: Vec::new(),
                },
                (Some(a), Some(b)) => {
                    let body = if kind == DbKind::MySql {
                        mysql_view_body(&a, &src.db, &dst.db)
                    } else {
                        a.clone()
                    };
                    // MySQL 的定义正文带着源库名，替换成目标库后再比，
                    // 否则同一个视图只因库名不同会被误判成「定义不同」
                    let src_ddl = view_create(kind, &src.schema, &name, cat, &body);
                    let dst_ddl = view_create(kind, &dst.schema, &name, cat, &b);
                    if body.trim().is_empty() || b.trim().is_empty() {
                        warnings.push(format!("{name} 有一端读不到视图定义（可能缺权限）"));
                        failed_obj(cat, &name, "读不到视图定义（可能缺权限）")
                    } else if norm_sql(&body) == norm_sql(&b) {
                        ObjDiff {
                            cat,
                            name: name.clone(),
                            status: DiffStatus::Same,
                            detail: "定义一致".into(),
                            src_ddl,
                            dst_ddl,
                            script: String::new(),
                            items: Vec::new(),
                        }
                    } else {
                        let script = format!(
                            "{}\n{}",
                            view_drop(kind, &dst.schema, &name, cat, false),
                            view_create(kind, &dst.schema, &name, cat, &body)
                        );
                        ObjDiff {
                            cat,
                            name: name.clone(),
                            status: DiffStatus::Changed,
                            detail: "定义不同".into(),
                            src_ddl,
                            dst_ddl,
                            script,
                            items: Vec::new(),
                        }
                    }
                }
                (None, None) => continue,
            };
            record(&mut objects, &mut counts, progress, &mut done, total, obj);
        }
    }

    // 同名对象在两端类型不同（表 ↔ 视图）：脚本会撞名，单独提示
    let names_of = |list: &Vec<Vec<String>>, cat: usize| -> Vec<String> {
        list.get(cat).cloned().unwrap_or_default()
    };
    for name in names_of(&src_lists, 0) {
        if names_of(&dst_lists, 1).contains(&name) {
            warnings.push(format!(
                "{name} 在源端是表、目标端是视图，需先处理目标端对象"
            ));
        }
    }
    for name in names_of(&dst_lists, 0) {
        if names_of(&src_lists, 1).contains(&name) {
            warnings.push(format!(
                "{name} 在目标端是表、源端是视图，需先处理目标端对象"
            ));
        }
    }
    // 同名例程在两端类型不同（函数 ↔ 存储过程）：PG 共用一套名字空间
    for name in src_routines
        .funcs
        .keys()
        .filter(|n| dst_routines.procs.contains_key(*n))
    {
        warnings.push(format!(
            "{name} 在源端是函数、目标端是存储过程，需先处理目标端对象"
        ));
    }
    for name in src_routines
        .procs
        .keys()
        .filter(|n| dst_routines.funcs.contains_key(*n))
    {
        warnings.push(format!(
            "{name} 在源端是存储过程、目标端是函数，需先处理目标端对象"
        ));
    }
    if !missing_owners.is_empty() {
        warnings.push(format!(
            "新建表的脚本会带上 OWNER TO {}；目标端没有这些角色时需先建角色或改写语句",
            missing_owners.join(" / ")
        ));
    }

    let src_label = src.label();
    let dst_label = dst.label();
    let src_name = src.conn.name.clone();
    let dst_name = dst.conn.name.clone();
    let src_place = src.place();
    let dst_place = dst.place();
    let mut script = String::new();
    script.push_str("-- daro 结构同步脚本\n");
    script.push_str(&format!("-- 源:   {src_label}\n"));
    script.push_str(&format!("-- 目标: {dst_label}\n"));
    script.push_str("-- 说明: 把目标端调整为源端结构，供人工复核；daro 不会自动执行。\n");
    let scope: Vec<&str> = cats.iter().map(|c| cat_title(*c)).collect();
    script.push_str(&format!(
        "-- 范围: {}；触发器、数据不参与比较。\n",
        scope.join(" / ")
    ));
    script.push_str("-- 注意: 目标端多出的对象只给注释掉的 DROP，确认后再取消注释。\n");
    let mut any = false;
    for &cat in &cats {
        let title = cat_title(cat);
        let picked: Vec<&ObjDiff> = objects
            .iter()
            .filter(|o| o.cat == cat && o.status != DiffStatus::Same)
            .collect();
        if picked.is_empty() {
            continue;
        }
        any = true;
        script.push_str(&format!("\n-- ── {title} ──\n"));
        for obj in picked {
            script.push_str(&format!("-- [{}] {}\n", obj.status.label(), obj.name));
            if obj.script.trim().is_empty() {
                script.push_str("-- 没有可执行的语句\n");
            } else {
                script.push_str(obj.script.trim_end());
                script.push('\n');
            }
        }
    }
    if !any {
        script.push_str("\n-- 两个库的结构完全一致，无需同步。\n");
    }

    Ok(CompareResult {
        objects,
        script,
        counts,
        warnings,
        src_label,
        dst_label,
        src_name,
        dst_name,
        src_place,
        dst_place,
    })
}

/// 目录里的对象清单（表 / 视图 / 实体化视图）；例程由 [load_routines] 填。
fn list_cats(side: &SideSpec, cats: &[u8]) -> Result<Vec<Vec<String>>, String> {
    let mut out: Vec<Vec<String>> = vec![Vec::new(); 5];
    for &cat in cats {
        if cat > CAT_MATVIEW {
            continue;
        }
        let mut list = catalog::query_objects(&side.conn, &side.db, &side.schema, cat as usize)
            .map_err(|e| format!("读取{}清单失败：{e}", cat_title(cat)))?;
        list.sort();
        list.dedup();
        out[cat as usize] = list;
    }
    Ok(out)
}

fn cat_title(cat: u8) -> &'static str {
    match cat {
        CAT_VIEW => "视图",
        CAT_MATVIEW => "实体化视图",
        CAT_FUNC => "函数",
        CAT_PROC => "存储过程",
        _ => "表",
    }
}

fn cat_icon(cat: u8) -> ZedIcon {
    match cat {
        CAT_VIEW => ZedIcon::Eye,
        CAT_MATVIEW => ZedIcon::Layers,
        CAT_FUNC | CAT_PROC => ZedIcon::Code,
        _ => ZedIcon::Table,
    }
}

fn kind_title(kind: DbKind) -> &'static str {
    match kind {
        DbKind::MySql => "MySQL",
        DbKind::PostgreSql => "PostgreSQL",
        DbKind::SQLite => "SQLite",
        DbKind::SqlServer => "SQL Server",
        DbKind::Access => "Access",
        DbKind::MongoDb => "MongoDB",
        DbKind::Redis => "Redis",
    }
}

fn op_icon(status: DiffStatus) -> ZedIcon {
    match status {
        DiffStatus::Changed => ZedIcon::ArrowRight,
        DiffStatus::New => ZedIcon::Plus,
        DiffStatus::Removed => ZedIcon::Close,
        DiffStatus::Same => ZedIcon::Dash,
    }
}

#[cfg(test)]
mod tests {
    // 不用 `use super::*`：模块顶上的 `gpui_kit::*` 里有个 `test` 属性宏，
    // glob 进来会把 `#[test]` 换成它，展开直接撞递归上限。
    use super::{
        CAT_FUNC, CAT_PROC, CAT_VIEW, CompareOptions, create_script, failure_snippet, fmt_elapsed,
        routine_drop, routine_retarget, routine_text, same_table, table_script, view_create,
        view_drop,
    };
    use crate::mock::DbKind;
    use crate::{DesignCol, DesignIndex, DesignMeta, DesignPair};

    fn col(name: &str, ty: &str, len: &str, not_null: bool) -> DesignCol {
        DesignCol {
            name: name.into(),
            ty: ty.into(),
            len: len.into(),
            not_null,
            ..Default::default()
        }
    }

    #[test]
    fn same_table_ignores_column_order() {
        let mut a = DesignMeta::default();
        a.cols = vec![
            col("id", "int", "", true),
            col("name", "varchar", "64", false),
        ];
        let mut b = DesignMeta::default();
        b.cols = vec![
            col("name", "varchar", "64", false),
            col("id", "int", "", true),
        ];
        assert!(same_table(&a, &b));
        b.cols[0].len = "128".into();
        assert!(!same_table(&a, &b), "长度变了要算修改");
    }

    #[test]
    fn elapsed_renders_mm_ss_cc() {
        assert_eq!(fmt_elapsed(0), "00:00.00");
        assert_eq!(fmt_elapsed(2070), "00:02.07");
        assert_eq!(fmt_elapsed(65_430), "01:05.43");
        assert_eq!(fmt_elapsed(3_600_000), "60:00.00");
    }

    #[test]
    fn failure_snippet_skips_script_header() {
        let stmt = "-- daro 结构同步部署脚本（仅勾选对象）\n-- 源: a/b\n\n\
                    ALTER TABLE \"public\".\"access_rules\"\n  \
                    ALTER COLUMN \"decision\" TYPE integer USING CAST(\"decision\" AS integer);";
        assert_eq!(
            failure_snippet(stmt),
            [
                "ALTER TABLE \"public\".\"access_rules\"",
                "ALTER COLUMN \"decision\" TYPE integer USING CAST(\"decision\" AS integer);",
            ]
        );
        assert_eq!(
            failure_snippet("DROP INDEX \"public\".\"ix\";"),
            ["DROP INDEX \"public\".\"ix\";"]
        );
    }

    #[test]
    fn mysql_diff_adds_and_modifies_columns() {
        let mut src = DesignMeta::default();
        src.cols = vec![
            col("id", "int", "", true),
            col("name", "varchar", "128", false),
            col("extra", "int", "", false),
        ];
        let mut dst = DesignMeta::default();
        dst.cols = vec![
            col("id", "int", "", true),
            col("name", "varchar", "64", false),
        ];
        let (script, changes, _) = table_script(DbKind::MySql, "", "t", &src, &dst);
        assert!(script.contains("ADD COLUMN `extra` int"), "{script}");
        assert!(
            script.contains("MODIFY COLUMN `name` varchar(128)"),
            "{script}"
        );
        assert!(changes.iter().any(|c| c == "字段 +1 ~1"), "{changes:?}");
    }

    #[test]
    fn pg_diff_changes_default_and_nullability() {
        let mut src = DesignMeta::default();
        let mut c = col("note", "text", "", true);
        c.default = "'x'::text".into();
        src.cols = vec![c];
        let mut dst = DesignMeta::default();
        dst.cols = vec![col("note", "text", "", false)];
        let (script, _, notes) = table_script(DbKind::PostgreSql, "public", "t", &src, &dst);
        assert!(script.contains("SET NOT NULL"), "{script}");
        assert!(script.contains("SET DEFAULT 'x'::text"), "{script}");
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn pg_create_keeps_defaults() {
        let mut meta = DesignMeta::default();
        let mut c = col("flag", "boolean", "", false);
        c.default = "false".into();
        meta.cols = vec![c];
        let sql = create_script(DbKind::PostgreSql, "public", "t", &meta);
        assert!(sql.starts_with("CREATE TABLE \"public\".\"t\""), "{sql}");
        assert!(sql.contains("SET DEFAULT false"), "{sql}");
    }

    #[test]
    fn sqlite_rebuild_copies_common_columns() {
        let mut src = DesignMeta::default();
        src.cols = vec![
            col("id", "INTEGER", "", true),
            col("added", "TEXT", "", false),
        ];
        let mut dst = DesignMeta::default();
        dst.cols = vec![
            col("id", "INTEGER", "", true),
            col("gone", "TEXT", "", false),
        ];
        let (script, _, _) = table_script(DbKind::SQLite, "main", "t", &src, &dst);
        assert!(script.contains("CREATE TABLE \"t__daro_new\""), "{script}");
        assert!(
            script.contains("INSERT INTO \"t__daro_new\" (\"id\") SELECT \"id\" FROM \"t\""),
            "{script}"
        );
        assert!(
            script.contains("ALTER TABLE \"t__daro_new\" RENAME TO \"t\""),
            "{script}"
        );
    }

    #[test]
    fn index_add_and_drop_are_generated() {
        let mut src = DesignMeta::default();
        src.cols = vec![col("id", "int", "", true)];
        src.indexes = vec![DesignIndex {
            name: "idx_id".into(),
            fields: vec![crate::IndexField::make("id", "int")],
            ..Default::default()
        }];
        let mut dst = DesignMeta::default();
        dst.cols = vec![col("id", "int", "", true)];
        dst.indexes = vec![DesignIndex {
            name: "idx_old".into(),
            fields: vec![crate::IndexField::make("id", "int")],
            ..Default::default()
        }];
        let (script, changes, _) = table_script(DbKind::MySql, "", "t", &src, &dst);
        assert!(script.contains("CREATE INDEX `idx_id`"), "{script}");
        assert!(script.contains("DROP INDEX `idx_old`"), "{script}");
        assert!(changes.iter().any(|c| c == "索引 +1 -1"), "{changes:?}");
    }

    #[test]
    fn unique_and_check_changes_rebuild_constraints() {
        let mut src = DesignMeta::default();
        src.cols = vec![col("a", "int", "", false)];
        src.uniques = vec![DesignPair {
            name: "uq_a".into(),
            value: "a".into(),
            ..Default::default()
        }];
        src.checks = vec![DesignPair {
            name: "ck_a".into(),
            value: "a > 0".into(),
            ..Default::default()
        }];
        let mut dst = DesignMeta::default();
        dst.cols = vec![col("a", "int", "", false)];
        dst.uniques = vec![DesignPair {
            name: "uq_a".into(),
            value: "a, a".into(),
            ..Default::default()
        }];
        let (script, _, _) = table_script(DbKind::PostgreSql, "public", "t", &src, &dst);
        assert!(script.contains("DROP CONSTRAINT \"uq_a\""), "{script}");
        assert!(
            script.contains("ADD CONSTRAINT \"uq_a\" UNIQUE (\"a\")"),
            "{script}"
        );
        assert!(
            script.contains("ADD CONSTRAINT ck_a CHECK (a > 0)"),
            "{script}"
        );
    }

    #[test]
    fn sqlite_index_only_change_skips_rebuild() {
        let mut src = DesignMeta::default();
        src.cols = vec![col("id", "INTEGER", "", true)];
        src.indexes = vec![DesignIndex {
            name: "idx_a".into(),
            fields: vec![crate::IndexField::make("id", "INTEGER")],
            ..Default::default()
        }];
        let mut dst = DesignMeta::default();
        dst.cols = vec![col("id", "INTEGER", "", true)];
        let (script, changes, _) = table_script(DbKind::SQLite, "main", "t", &src, &dst);
        assert!(!script.contains("__daro_new"), "{script}");
        assert!(script.contains("CREATE INDEX \"idx_a\""), "{script}");
        assert!(changes.iter().any(|c| c == "索引 +1"), "{changes:?}");
    }

    #[test]
    fn mysql_pk_only_change_does_not_modify_column() {
        let mut src = DesignMeta::default();
        let mut c = col("id", "int", "", true);
        c.pk = true;
        src.cols = vec![c];
        let mut dst = DesignMeta::default();
        dst.cols = vec![col("id", "int", "", true)];
        let (script, changes, _) = table_script(DbKind::MySql, "", "t", &src, &dst);
        assert!(!script.contains("MODIFY COLUMN"), "{script}");
        assert!(script.contains("ADD PRIMARY KEY (`id`)"), "{script}");
        assert_eq!(changes, vec!["主键".to_string()]);
    }

    #[test]
    fn view_ddl_and_drop_shape() {
        let ddl = view_create(DbKind::PostgreSql, "public", "v", 1, " SELECT 1;");
        assert_eq!(ddl, "CREATE VIEW \"public\".\"v\" AS\nSELECT 1;");
        let drop = view_drop(DbKind::PostgreSql, "public", "v", 1, true);
        assert_eq!(drop, "-- DROP VIEW IF EXISTS \"public\".\"v\";");
        let mv = view_create(DbKind::PostgreSql, "public", "mv", 2, "select 1");
        assert!(mv.starts_with("CREATE MATERIALIZED VIEW"), "{mv}");
        assert!(mv.ends_with("WITH DATA;"), "{mv}");
    }

    #[test]
    fn compare_scope_follows_engine_and_checks() {
        let all = CompareOptions::default();
        assert_eq!(all.cats(DbKind::PostgreSql), vec![0, 1, 2, 3, 4]);
        // MySQL 没有实体化视图
        assert_eq!(all.cats(DbKind::MySql), vec![0, 1, 3, 4]);
        // SQLite 没有实体化视图与例程，勾了也只会出表和视图
        assert_eq!(all.cats(DbKind::SQLite), vec![0, 1]);
        let mut picked = all;
        picked.toggle(CAT_FUNC);
        picked.toggle(CAT_PROC);
        assert_eq!(picked.cats(DbKind::PostgreSql), vec![0, 1, 2]);
        picked.toggle(CAT_VIEW);
        assert!(!picked.get(CAT_VIEW));
        assert_eq!(picked.cats(DbKind::MySql), vec![0]);
    }

    #[test]
    fn pg_routine_definition_is_rewritten_to_target_schema() {
        let defs = vec![
            "CREATE OR REPLACE FUNCTION public.f() RETURNS void\n LANGUAGE sql\nAS $function$ SELECT 1 $function$"
                .to_string(),
            "CREATE OR REPLACE FUNCTION \"public\".\"g\"(a integer) RETURNS integer\n LANGUAGE sql\nAS $function$ SELECT a $function$"
                .to_string(),
        ];
        let out = routine_retarget(DbKind::PostgreSql, &defs, "public", "cmp", "dev", "cmp");
        assert!(out.contains("FUNCTION cmp.f()"), "{out}");
        assert!(out.contains("FUNCTION \"cmp\".\"g\"(a integer)"), "{out}");
        assert!(!out.contains("public."), "{out}");
        // PG 用 CREATE OR REPLACE，不需要先 DROP
        assert!(!out.contains("DROP FUNCTION"), "{out}");
        assert!(out.ends_with("$function$;"), "每段定义补齐分号：{out}");
    }

    #[test]
    fn mysql_routine_rename_needs_drop_first() {
        let drop = routine_drop(DbKind::MySql, "", "shop", CAT_PROC, "p", false);
        assert_eq!(drop, "DROP PROCEDURE IF EXISTS `shop`.`p`;");
        assert_eq!(
            routine_drop(DbKind::MySql, "", "shop", CAT_FUNC, "f", true),
            "-- DROP FUNCTION IF EXISTS `shop`.`f`;"
        );
        let text = routine_retarget(
            DbKind::MySql,
            &["CREATE FUNCTION f() RETURNS int RETURN 1".into()],
            "",
            "",
            "dev",
            "shop",
        );
        assert!(text.ends_with("RETURN 1;"), "{text}");
        assert_eq!(routine_text(&["SELECT 1".to_string()]), "SELECT 1;");
    }

    // ── 真 PG 库：跨模式比较（打 datawalk 开发库，平时忽略） ─────────

    #[test]
    #[ignore = "打 datawalk 开发库，需要网络可达"]
    fn pg_real_schema_compare_reads_metadata_and_qualifies_script_with_target_schema() {
        use super::{DiffStatus, SideSpec, compare_objects};
        use crate::mock::Connection;
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex};

        let Some((_guard, p)) = crate::pg_driver::tests::pg_fixture() else {
            eprintln!("[pg] skip：读不到 datawalk 连接串");
            return;
        };
        // 目标模式与 public 同名表少列，另带一个只在目标端出现的表和视图
        let mut seed = crate::pg_driver::CopySession::connect(&p).unwrap();
        for stmt in [
            "CREATE SCHEMA cmp",
            "CREATE TABLE cmp.member(id bigint, extra text)",
            "CREATE TABLE cmp.only_here(x int)",
            "CREATE VIEW cmp.v_member AS SELECT id FROM cmp.member",
        ] {
            seed.exec(stmt).unwrap_or_else(|e| panic!("{e}\n{stmt}"));
        }

        struct CloseKeys(Vec<String>);
        impl Drop for CloseKeys {
            fn drop(&mut self) {
                for k in &self.0 {
                    crate::pg_driver::close(k);
                }
            }
        }
        let _keys = CloseKeys(vec!["sc-pg-src".into(), "sc-pg-dst".into()]);
        let side = |name: &str, schema: &str| SideSpec {
            conn: Connection::new(
                name,
                DbKind::PostgreSql,
                &p.host,
                p.port,
                &p.dbname,
                &p.user,
                &p.password,
                "",
                true,
            ),
            db: p.dbname.clone(),
            schema: schema.to_string(),
        };
        let progress = Arc::new(Mutex::new((0usize, 0usize)));
        let cancel = Arc::new(AtomicBool::new(false));

        let tables_only = CompareOptions {
            tables: true,
            views: true,
            matviews: true,
            funcs: false,
            procs: false,
        };
        let r = compare_objects(
            &side("sc-pg-src", "public"),
            &side("sc-pg-dst", "cmp"),
            tables_only,
            &progress,
            &cancel,
        )
        .expect("真库比较失败");
        let obj = |cat: u8, name: &str| {
            r.objects
                .iter()
                .find(|o| o.cat == cat && o.name == name)
                .unwrap_or_else(|| {
                    panic!(
                        "缺少差异项 {cat}/{name}：{:?}",
                        r.objects
                            .iter()
                            .map(|o| (&o.name, o.status.label()))
                            .collect::<Vec<_>>()
                    )
                })
        };
        assert!(matches!(obj(0, "member").status, DiffStatus::Changed));
        assert!(matches!(obj(0, "only_here").status, DiffStatus::Removed));
        assert!(matches!(obj(0, "gen").status, DiffStatus::New));
        assert!(matches!(obj(1, "v_member").status, DiffStatus::Changed));
        assert!(
            matches!(obj(2, "mv_member").status, DiffStatus::New),
            "mv_member 状态 {}；警示：{:?}",
            obj(2, "mv_member").status.label(),
            r.warnings
        );
        assert!(
            r.warnings.iter().any(|w| w.contains("member")),
            "{:?}",
            r.warnings
        );

        // 脚本在目标模式执行：括号里必须是 cmp 而不是 public
        assert!(
            r.script.contains("ALTER TABLE \"cmp\".\"member\""),
            "{}",
            r.script
        );
        assert!(
            r.script.contains("ADD COLUMN \"name\" text"),
            "{}",
            r.script
        );
        assert!(
            r.script.contains("-- DROP TABLE \"cmp\".\"only_here\";"),
            "{}",
            r.script
        );
        assert!(
            r.script.contains("CREATE TABLE \"cmp\".\"gen\""),
            "{}",
            r.script
        );
        assert!(
            r.script
                .contains("DROP VIEW IF EXISTS \"cmp\".\"v_member\";"),
            "{}",
            r.script
        );
        assert!(
            r.script.contains("CREATE VIEW \"cmp\".\"v_member\""),
            "{}",
            r.script
        );
        assert!(
            r.script
                .contains("CREATE MATERIALIZED VIEW \"cmp\".\"mv_member\""),
            "{}",
            r.script
        );
        assert!(
            !r.script.contains("\"public\".\""),
            "脚本不能把对象建到源模式：{}",
            r.script
        );

        // 同一模式自比：所有对象应判为一致，脚本只留说明
        let same = compare_objects(
            &side("sc-pg-src", "public"),
            &side("sc-pg-dst", "public"),
            tables_only,
            &progress,
            &cancel,
        )
        .expect("自比较失败");
        assert_eq!(
            *progress.lock().unwrap(),
            (8, 16),
            "进度应走完 8 个对象（源+目标共 16）"
        );
        assert!(
            same.objects
                .iter()
                .all(|o| matches!(o.status, DiffStatus::Same)),
            "{:?}",
            same.objects
                .iter()
                .map(|o| (&o.name, o.status.label()))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            same.counts[3], 8,
            "6 表 + 1 视图 + 1 实体化视图：{:?}",
            same.counts
        );
        assert!(same.script.contains("结构完全一致"), "{}", same.script);
    }

    #[test]
    fn deploy_moves_foreign_keys_after_all_tables() {
        use super::defer_fk_constraints;

        let sql = "CREATE TABLE \"public\".\"child\" (
    \"id\" bigint NOT NULL,
    \"p\" bigint NOT NULL,
    PRIMARY KEY (\"id\"),
    CONSTRAINT \"fk_child_p\" FOREIGN KEY (\"p\") REFERENCES \"public\".\"parent\" (\"id\")
);
ALTER TABLE \"public\".\"parent\"
  ADD CONSTRAINT \"fk_parent_self\" FOREIGN KEY (\"x\") REFERENCES \"public\".\"child\" (\"id\");";
        let (body, fks) = defer_fk_constraints(DbKind::PostgreSql, sql);
        assert!(!body.contains("FOREIGN KEY"), "建表里不能再留外键：{body}");
        assert!(body.contains("PRIMARY KEY (\"id\")"), "其它表内约束保留：{body}");
        assert!(body.contains(");"), "语句终止符要补回来：{body}");
        assert_eq!(fks.len(), 2, "{fks:#?}");
        assert!(
            fks[0].starts_with("ALTER TABLE \"public\".\"child\"
  ADD CONSTRAINT \"fk_child_p\" FOREIGN KEY"),
            "新建表的外键改成事后 ADD：{}",
            fks[0]
        );
        assert!(fks[1].contains("fk_parent_self"), "{fks:#?}");
    }

    #[test]
    fn deploy_fk_comment_follows_its_constraint() {
        use super::defer_fk_constraints;

        let sql = "CREATE TABLE \"c\" (
    id integer,
    CONSTRAINT \"fk\" FOREIGN KEY (id) REFERENCES p(id)
);
COMMENT ON CONSTRAINT \"fk\" ON \"public\".\"c\" IS 'x';
COMMENT ON CONSTRAINT \"uk\" ON \"public\".\"c\" IS 'y';";
        let (body, fks) = defer_fk_constraints(DbKind::PostgreSql, sql);
        assert!(!body.contains("FOREIGN KEY"), "{body}");
        assert!(
            body.contains("COMMENT ON CONSTRAINT \"uk\""),
            "唯一键的注释不跟着外键走：{body}"
        );
        assert_eq!(fks.len(), 2, "{fks:#?}");
        assert!(fks[1].starts_with("COMMENT ON CONSTRAINT \"fk\""), "{fks:#?}");
    }

    #[test]
    fn deploy_keeps_foreign_keys_inline_for_sqlite() {
        use super::defer_fk_constraints;

        let sql = "CREATE TABLE \"c\" (
    id integer,
    CONSTRAINT \"fk\" FOREIGN KEY (id) REFERENCES p(id)
);";
        let (body, fks) = defer_fk_constraints(DbKind::SQLite, sql);
        assert_eq!(body, sql, "SQLite 不支持事后加约束，原样保留");
        assert!(fks.is_empty());
    }

    #[test]
    fn deploy_script_puts_foreign_keys_in_a_trailing_section() {
        use super::deploy_script_text;

        let mk = |name: &str, script: &str| super::ObjDiff {
            cat: super::CAT_TABLE,
            name: name.into(),
            status: super::DiffStatus::New,
            detail: String::new(),
            src_ddl: String::new(),
            dst_ddl: String::new(),
            script: script.into(),
            items: Vec::new(),
        };
        let result = super::CompareResult {
            objects: vec![
                mk(
                    "child",
                    "CREATE TABLE \"public\".\"child\" (
    id bigint,
    CONSTRAINT \"fk\" FOREIGN KEY (id) REFERENCES \"public\".\"parent\"(id)
);",
                ),
                mk(
                    "parent",
                    "CREATE TABLE \"public\".\"parent\" (
    id bigint
);",
                ),
            ],
            script: String::new(),
            counts: [1, 0, 0, 0],
            warnings: Vec::new(),
            src_label: "s".into(),
            dst_label: "d".into(),
            src_name: "s".into(),
            dst_name: "d".into(),
            src_place: "s".into(),
            dst_place: "d".into(),
        };
        let checked: std::collections::HashSet<usize> = [0, 1].into_iter().collect();
        let text = deploy_script_text(DbKind::PostgreSql, &result, &checked);
        let parent = text.find("\"parent\" (").expect("父表建表在脚本里");
        let fk = text.find("-- 外键约束").expect("外键段落存在");
        assert!(fk > parent, "外键段落必须排在所有建表之后：{text}");
        assert!(text[fk..].contains("ADD CONSTRAINT \"fk\" FOREIGN KEY"), "{text}");
    }
}
