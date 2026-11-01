// gpui 的 test 宏展开深度超过默认 128（仅 cfg(test) 编译时用到）
#![recursion_limit = "512"]

mod art;
mod conn_dialog;
mod dock;
mod export_format;
mod table_export;
mod table_export_dialog;
mod export_result_dialog;
mod filter;
mod filter_value_dialog;
mod mock;
mod navicat_export_dialog;
mod sql_completion;
mod sql_highlight;
mod sql_preview;
mod ui;
mod navicat_import_dialog;
mod navicat_ncx;
mod palette;
mod prompt_dialog;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use gpui_kit::assets::{AllAssets, IconName};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{
    Editor, EditorState, Input, InputEvent, InputState, Textarea, TextareaState,
};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _, DropdownButton},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

actions!(daro, [TreeRename, TreeCancelRename, RefreshView, DeleteSelected, Save]);

use conn_dialog::{ConnDialogMode, open_connection_dialog, open_new_connection};
use mock::{Connection, DbKind, GROUPS, connections};
use palette::Palette;
use prompt_dialog::{open_confirm, open_text_prompt};

/// 行高：Navicat Lite 里左侧树是 20px，中部对象列表更密（约 16.5px），分别取值
/// 树行高。Navicat Lite 实测：行距 40 物理像素 @200% = 20 逻辑像素
/// （截图 nv_base.png 连续行文字带 334/374/414… 间距 40）。2026-10-31 前
/// 这里写 17.5，比参考工具密 12.5%，已按实测改回 20。
const ROW_H: f32 = 20.;
const LIST_ROW_H: f32 = 17.;
const LEFT_W: f32 = 250.;
const RIGHT_W: f32 = 315.;
const RIBBON_H: f32 = 62.;
const TITLE_H: f32 = 24.;
const MENU_H: f32 = 22.;

// ── 数据页密度档（照 Navicat 数据页 1:1：工具条 27 / 表头两行 20+17 / 行 20）──
/// 数据页工具条高（图标 + 文字）
const T_TOOLBAR_H: f32 = 28.;
/// 表头「列名」行高
const T_HEAD_NAME_H: f32 = 21.;
/// 表头「类型」行高
const T_HEAD_TYPE_H: f32 = 18.;
/// 数据行高
const T_ROW_H: f32 = 22.;
/// 行首槽宽（只放当前行蓝条 / 新增行标记，没有行号）
const T_GUTTER_W: f32 = 9.;
/// 底部记录导航条高
const T_BAR_H: f32 = 27.;

/// 数据页一页多少行。Navicat 默认 1000，mock 每表只有 8..48 行，按 1000 分页
/// 永远只有一页、分页钮永远灰着，mock 档先取 10 让分页交互真实可走；
/// 真数据接入后改回 1000（或做成可配，Navicat 齿轮里就有页大小）。
const PAGE_SIZE: usize = 10;

// ── 设计器密度档 ──────────────────────────────────────────
// 照 Flutter 版 table_designer_page.dart 的实测常量整批搬来，每个值都有
// 参考工具出处（见 Flutter 仓 AGENTS.md「设计器密度档」），别凭手感调。
const D_ROW_EXTENT: f32 = 21.; // 字段行高（RowExtent）
const D_HEADER_H: f32 = 30.; // 表头高
const D_GUTTER_W: f32 = 12.; // 行首「当前行」槽
const D_ACCENT_BAR_W: f32 = 2.; // 槽右端 / 表头下划线的强调条
const D_HEADER_INSET_TOP: f32 = 5.; // 表头淡蓝块的内缩
const D_TOOLBAR_H: f32 = 29.;
const D_TABBAR_H: f32 = 21.;
const D_TOOLSEP_W: f32 = 17.; // 工具栏分隔块（两侧留白含 1px 竖线）
const D_ICON_BTN_W: f32 = 25.; // 14 图标 + 2×5.5 水平留白
const D_PROP_SPLITTER_H: f32 = 3.; // 网格与属性面板之间可拖动的缝
const D_PROP_ROW_H: f32 = 26.;
const D_PROP_PAD_L: f32 = 11.;
const D_PROP_LABEL_W: f32 = 190.; // 控件列自 201 起，与网格第一列(200)对齐
const D_PROP_FIELD_W: f32 = 300.;
/// 字段网格列宽：名称 / 类型 / 长度 / 小数点 / 不是 null / 键 / 注释
const D_FIELD_WIDTHS: [f32; 7] = [200., 120., 60., 60., 60., 60., 300.];
/// 索引子页主表格列宽：名称 / 字段 / 索引方法 / 唯一键 / 并发 / 注释
/// （照参考工具 1:1 量：273/589/185/112/148/314 物理像素 ÷ 1.75 后按窗口宽度压一档）
const D_INDEX_WIDTHS: [f32; 6] = [148., 300., 96., 58., 80., 168.];
/// 索引「字段」浮层（参考工具点「字段」格弹出的小表格）整体宽
/// （七列 700 + 每列 1px 网格线，留 10px 余量免得最后一列被裁）
const D_IDX_POPUP_W: f32 = 710.;
/// 浮层七列列宽：名称 / 排序规则模式 / 排序规则 / 运算符类别模式 / 运算符类别 /
/// 排序顺序 / Nulls 排序（参考图 216/214/148/184/112/148/314 物理像素 ÷ 1.75 折算，
/// 再按 12px 中文表头 / 带下拉箭头的格子调平，合计 700）
const D_IDX_POPUP_COLS: [f32; 7] = [100., 104., 72., 92., 78., 74., 180.];
/// 浮层表头高（参考 48 物理像素 ÷ 1.75）
const D_IDX_POPUP_HEAD_H: f32 = 27.;
/// 浮层表格区固定高（参考图表格区约 240 逻辑像素，只有两三行也撑这么高）
const D_IDX_POPUP_BODY_H: f32 = 240.;
/// 浮层底栏高（↑↓+− + 确定/取消）
const D_IDX_POPUP_FOOT_H: f32 = 40.;
/// 外键子页主表格列宽：名称 / 字段 / 被引用的模式 / 被引用的表 / 被引用的字段 /
/// 删除时 / 更新时 / 注释（参考工具 1:1 量：180/260×4/160×2/214 物理像素 ÷ 1.75 =
/// 103/148×4/91×2/122，合计 999；设计器窗格默认 876 扣掉行首槽只剩 864，
/// 按索引页同款口径整体压一档 0.865）
const D_FK_WIDTHS: [f32; 8] = [89., 128., 128., 128., 128., 79., 79., 105.];
/// 外键子页底部属性区（参考工具那三行：符合全部 / 可延迟 / 延迟）：
/// 行距 30（52 物理像素 ÷ 1.75）、控件列自 230 起（403 物理像素 ÷ 1.75）、
/// 下拉框 342×20（598×36 物理像素 ÷ 1.75）、整块高 198（346 物理像素 ÷ 1.75，
/// 行下的留白属于这块面板 —— 参考图属性行与状态条之间还有一段白）
const D_FK_PROP_ROW_H: f32 = 30.;
const D_FK_PROP_LABEL_W: f32 = 219.;
const D_FK_PROP_FIELD_W: f32 = 342.;
const D_FK_PROPS_H: f32 = 198.;
/// 外键页行首槽 / 表头下划线的强调条色（参考取样 #4DA0E1，比当前格的
/// #0078D4 浅一档 —— 参考图里这是两个不同的蓝）
const D_FK_BAR: u32 = 0x4DA0E1;
/// 唯一键子页主表格列宽：名称 / 字段 / 注释（参考工具 1:1 量：300/800/512
/// 物理像素 ÷ 1.75 = 171/457/293，合计 921；窗格 876 扣行首槽放不下，
/// 按外键页同款口径压一档 0.938）
const D_UNIQUE_WIDTHS: [f32; 3] = [161., 429., 274.];
/// 检查子页主表格列宽：名称 / 表达式 / 无继承 / 注释（参考工具 1:1 量：
/// 300/800/160/558+ 物理像素 ÷ 1.75 = 171/457/91/注释被窗口裁切，
/// 按窗格宽压 0.848）
const D_CHECK_WIDTHS: [f32; 4] = [145., 387., 77., 254.];
/// 排除子页主表格列宽：名称 / 索引方法 / 元素 / 注释（参考工具 1:1 量：
/// 300/200/800/600 物理像素 ÷ 1.75 = 171/114/457/343，合计 1085 → 窗格放不下
/// 压 0.79）
const D_EXCLUDE_WIDTHS: [f32; 4] = [135., 90., 361., 271.];
/// 规则子页属性区（位置 / 定义两行）：行距 38（参考图 67 物理像素 ÷ 1.75）、
/// 标签列 203、字段 357×20（字段自 x=214 起，比外键属性区更靠左也更宽）
const D_RL_PROP_ROW_H: f32 = 38.;
const D_RL_PROP_LABEL_W: f32 = 203.;
const D_RL_PROP_FIELD_W: f32 = 357.;
/// 规则子页主表格列宽：名称 / 事件 / 代替运行 / 启用 / 注释（参考工具 1:1 量：
/// 500/160/160/100/600 物理像素 ÷ 1.75 = 286/91/91/57/343，合计 868，
/// 窗格 876 扣行首槽刚好放下，注释削 4）
const D_RULE_WIDTHS: [f32; 5] = [285., 90., 91., 56., 338.];
/// 触发器子页主表格列宽：名称 / 给每个 / 触发 / 插入 / 更新 / 删除 / 截断 /
/// 更新字段 / 启用 / 注释（参考工具 1:1 量：240/200/262/120/120/120/120/200/100/
/// 600+ 物理像素 ÷ 1.75 = 137/114/150/69/69/69/69/114/57/343，合计 1208 超出
/// 窗格 —— 整体横向滚动，与参考工具同款裁切）
const D_TRIGGER_WIDTHS: [f32; 10] = [
    137., 114., 150., 69., 69., 69., 69., 114., 57., 343.,
];
/// 外键「删除时 / 更新时」下拉的候选（SQL 标准五种引用动作）
const FK_ACTIONS: [&str; 5] = ["RESTRICT", "NO ACTION", "CASCADE", "SET NULL", "SET DEFAULT"];
/// 子标签条（与 Flutter 版 TabControl 的 11 个标签一致）
const D_SUBTABS: [&str; 11] = [
    "字段", "索引", "外键", "唯一键", "检查", "排除", "规则", "触发器", "选项", "注释", "SQL 预览",
];
/// 设计器动作图标色（Flutter 版实测：绿加 / 红减 / 金钥匙）
const D_ICON_ADD: u32 = 0x45C300;
const D_ICON_DANGER: u32 = 0xFF4A37;
const D_ICON_KEY: u32 = 0xF5AA00;

// ── 查询页密度档（排期7）──────────────────────────────────
// 尺寸取 Flutter 版 query_page.dart 照 Navicat Lite 实测的量，别凭手感调。
const Q_CONNBAR_H: f32 = 30.; // 运行上下文栏高
const Q_TRIGGER_W: f32 = 160.; // 连接 / 库 / 模式下拉触发器宽
/// 参考工具三个下拉不等宽：连接最宽、模式最窄
const Q_CONN_W: f32 = 240.;
const Q_DB_W: f32 = 205.;
const Q_SCHEMA_W: f32 = 165.;
const Q_TRIGGER_H: f32 = 22.; // 触发器高
const Q_PANEL_H: f32 = 300.; // 结果面板默认高（Flutter 版可拖，mock 档先固定）
const Q_HEADER_H: f32 = 26.; // 结果面板头（状态文案行）
const Q_TABBAR_H: f32 = 21.; // 结果页签条（同设计器密度）
const Q_ROWNO_W: f32 = 24.; // 行号列宽（同表数据页）
const Q_ROW_H: f32 = 17.5; // 结果网格行高（同表数据页）
const Q_GRID_HEADER_H: f32 = 22.; // 结果网格单行表头（列名，无类型行）
/// 运行上下文功能强调色（主题无关，Flutter 版照 Navicat 实测：蓝三角 / 浅红方块 / 浅蓝图标）
const Q_RUN_BLUE: u32 = 0x0092ff;
const Q_STOP_IDLE: u32 = 0xF79E95;
const Q_EXPLAIN_BLUE: u32 = 0x50A0F0;

const CATEGORIES: [(&str, art::Glyph); 5] = [
    ("表", art::Glyph::Table),
    ("视图", art::Glyph::View),
    ("实体化视图", art::Glyph::MatView),
    ("函数", art::Glyph::Function),
    ("查询", art::Glyph::Query),
];

/// (图标, 标签, 图标色, 打开的分类, 是否带下拉箭头, 是否压 Ent 角标)
const RIBBON: [(art::Glyph, &str, u32, Option<usize>, bool, bool); 13] = [
    (art::Glyph::RibbonConn, "连接", 0x6b7785, None, false, false),
    (art::Glyph::RibbonNewQuery, "新建查询", 0x2f8fd8, None, false, false),
    (art::Glyph::RibbonTable, "表", 0x2f8fd8, Some(0), false, false),
    (art::Glyph::RibbonView, "视图", 0x2f8fd8, Some(1), false, false),
    (art::Glyph::RibbonMatView, "实体化视图", 0x2f8fd8, Some(2), false, false),
    (art::Glyph::RibbonFunction, "函数", 0x2f8fd8, Some(3), false, false),
    (art::Glyph::RibbonUser, "角色", 0xd9a441, None, false, false),
    (art::Glyph::RibbonTools, "其它", 0x7a8b99, None, true, false),
    (art::Glyph::RibbonQuery, "查询", 0x2f8fd8, Some(4), false, false),
    (art::Glyph::RibbonBackup, "备份", 0x5c7fa8, None, false, true),
    (art::Glyph::RibbonAutoRun, "自动运行", 0xe08a3c, None, false, true),
    (art::Glyph::RibbonModel, "模型", 0x8a76c8, None, false, true),
    (art::Glyph::RibbonBi, "BI", 0xd05b5b, None, false, true),
];

const MENUS: &[(&str, &[&str])] = &[
    ("文件", &["新建连接…", "新建查询", "导入连接…", "导出连接…", "退出"]),
    ("编辑", &["撤销", "重做", "复制", "粘贴", "删除"]),
    ("查看", &["刷新", "大型图标", "列表", "详细信息", "显示右侧面板", "切换主题"]),
    ("收藏夹", &["添加当前对象", "管理收藏夹"]),
    ("工具", &["数据传输…", "结构同步…", "备份…", "自动运行…", "命令行界面…"]),
    ("窗口", &["最小化", "全屏"]),
    ("帮助", &["关于 daro-rs", "问题反馈"]),
];

/// 导入向导交回主窗口的一条连接（字段已按 Navicat 导出口径归一）。
pub struct ImportItem {
    pub name: String,
    pub kind: DbKind,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: String,
    pub group: String,
}

/// 一次导入的结果，供向导回显汇总文案。
pub struct ImportOutcome {
    pub imported: usize,
    pub renamed: usize,
    pub new_groups: Vec<String>,
}

/// gpui 的 Windows 后端不处理 WM_LBUTTONDBLCLK（只处理非客户区的），
/// 所以真实双击时 click_count 永远是 1。双击只能自己按「同一行 + 间隔」判，
/// 500ms 取 Windows 默认双击间隔。
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Act {
    /// 「我的连接」根节点
    Root,
    Group(usize),
    Conn(usize),
    /// (连接, 库)
    Db(usize, usize),
    /// (连接, 库, 模式) —— 只有 PG / SQL Server 有这一层
    Schema(usize, usize, usize),
    /// (连接, 库, 模式, 分类)。没有模式层的引擎把模式记成 NO_SCHEMA。
    Cat(usize, usize, usize, usize),
    /// (连接, 库, 模式, 分类, 对象)
    Obj(usize, usize, usize, usize, usize),
    /// (连接, 库, 模式, 分类, 对象, 字段) —— 表 / 视图展开后的字段行
    Col(usize, usize, usize, usize, usize, usize),
}

/// 树里固定五层，但 MySQL 系没有「模式」这一层；用一个哨兵值占住索引位，
/// 这样分类和对象节点不必按引擎分叉出两套元组形状。
const NO_SCHEMA: usize = usize::MAX;

impl Act {
    fn conn(&self) -> Option<usize> {
        match *self {
            Act::Conn(i) | Act::Db(i, _) | Act::Schema(i, _, _) | Act::Cat(i, _, _, _)
            | Act::Obj(i, _, _, _, _) | Act::Col(i, _, _, _, _, _) => Some(i),
            Act::Group(_) | Act::Root => None,
        }
    }

    fn db(&self) -> Option<usize> {
        match *self {
            Act::Db(_, d) | Act::Schema(_, d, _) | Act::Cat(_, d, _, _)
            | Act::Obj(_, d, _, _, _) | Act::Col(_, d, _, _, _, _) => Some(d),
            _ => None,
        }
    }

    fn schema(&self) -> Option<usize> {
        match *self {
            Act::Schema(_, _, s) => Some(s),
            Act::Cat(_, _, s, _) | Act::Obj(_, _, s, _, _) | Act::Col(_, _, s, _, _, _)
                if s != NO_SCHEMA =>
            {
                Some(s)
            }
            _ => None,
        }
    }

    fn cat(&self) -> Option<usize> {
        match *self {
            Act::Cat(_, _, _, c) | Act::Obj(_, _, _, c, _) | Act::Col(_, _, _, c, _, _) => Some(c),
            _ => None,
        }
    }

    /// 字段行归到所属对象：右侧面板、对象列表都按对象口径取数。
    fn obj(&self) -> Option<usize> {
        match *self {
            Act::Obj(_, _, _, _, o) | Act::Col(_, _, _, _, o, _) => Some(o),
            _ => None,
        }
    }
}

/// 树节点的展开态按「连接名 + 各级标签」记：索引会随增删连接漂移，名字不会。
/// 树节点的稳定 id。层级最深有六段，元组的 From 只到四，所以一律拼成名字。
fn rid(path: &str) -> ElementId {
    ElementId::Name(path.to_string().into())
}

fn node_key(conn_name: &str, sub: &str) -> String {
    format!("{conn_name}\u{1}{sub}")
}

/// 删光连接后各面板仍要能渲染：拿这个空连接当兜底，而不是在每个取值点判 Option。
static EMPTY_CONN: mock::Connection = mock::Connection {
    name: String::new(),
    kind: DbKind::MySql,
    host: String::new(),
    port: 0,
    database: String::new(),
    user: String::new(),
    password: String::new(),
    group: String::new(),
    connected: false,
};

/// 落盘状态（connections.json）。只存 Navicat 重启后会还原的东西：
/// 连接配置、分组、树展开态、主题、视图模式；选中项与打开的页签不存。
#[derive(Serialize, Deserialize)]
struct Store {
    version: u32,
    dark: bool,
    list_view: bool,
    show_info: bool,
    collapsed_root: bool,
    groups: Vec<String>,
    collapsed_groups: Vec<String>,
    starred: Vec<String>,
    conn_colors: HashMap<String, u8>,
    expanded: Vec<String>,
    visited_dbs: Vec<String>,
    connections: Vec<mock::Connection>,
}

/// 数据目录：`DARO_DATA_DIR` 可覆盖（开发 / 验证用），默认走用户本地数据目录。
fn store_path() -> PathBuf {
    match std::env::var("DARO_DATA_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("daro-rs"),
    }
    .join("connections.json")
}

fn load_store() -> Option<Store> {
    let data = std::fs::read_to_string(store_path()).ok()?;
    match serde_json::from_str(&data) {
        Ok(store) => Some(store),
        Err(e) => {
            eprintln!("[daro] connections.json 损坏，按默认状态启动：{e}");
            None
        }
    }
}

/// 树里给连接上色的七种标签色（Navicat「颜色」子菜单）
const CONN_COLORS: [(&str, u32); 7] = [
    ("红", 0xd9534f),
    ("橙", 0xe8913f),
    ("黄", 0xe8c33f),
    ("绿", 0x4caf50),
    ("蓝", 0x3d7fd0),
    ("紫", 0x8a6cc8),
    ("灰", 0x8d99a6),
];

#[derive(Clone, Copy)]
enum ViewAct {
    Grid,
    List,
    Info,
}

pub(crate) fn to_hsla(v: u32) -> Hsla {
    rgb(v).into()
}

/// 把 `a` 按 `t`（白色的权重）往 `b` 上混，派生页签 / 行号这类中间档。
/// 估算一段文字在给定字号下的宽度（ASCII 约 0.56em、CJK 1em）。
/// 只用于列宽自适应，差几个像素无所谓。
fn est_text_w(s: &str, size: f32) -> f32 {
    s.chars()
        .map(|c| if (c as u32) < 0x2e80 { 0.58 } else { 1.0 })
        .sum::<f32>()
        * size
}

fn blend(a: u32, b: u32, t: f32) -> u32 {
    let mix = |x: u32, y: u32| {
        let v = (x as f32 * (1. - t) + y as f32 * t).round() as u32;
        v.min(255)
    };
    (mix(a >> 16 & 0xff, b >> 16 & 0xff) << 16)
        | (mix(a >> 8 & 0xff, b >> 8 & 0xff) << 8)
        | mix(a & 0xff, b & 0xff)
}

// ── 复制为 SQL / HTML ──────────────────────────────────────────

/// 复制选定行为 INSERT 语句。Navicat 口径：单行一条 INSERT，多行合并成
/// 一条 INSERT ... VALUES (...), (...), ...；列名用方言引号。
fn copy_as_insert(kind: DbKind, db: &str, schema: &str, table: &str, cols: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let qcol = |name: &str| match kind {
        DbKind::MySql | DbKind::MongoDb | DbKind::Redis => format!("`{name}`"),
        _ => format!("\"{name}\""),
    };
    let qtable = match kind {
        DbKind::MySql | DbKind::MongoDb | DbKind::Redis => format!("`{db}`.`{table}`"),
        _ => format!("\"{schema}\".\"{table}\""),
    };
    let col_list = cols.iter().map(|c| qcol(c)).collect::<Vec<_>>().join(", ");
    let val_rows: Vec<String> = rows
        .iter()
        .map(|r| {
            let vals = r
                .iter()
                .map(|v| {
                    if v == "NULL" {
                        "NULL".to_string()
                    } else {
                        format!("'{}'", v.replace('\'', "''"))
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("({vals})")
        })
        .collect();
    format!("INSERT INTO {qtable} ({col_list}) VALUES {}", val_rows.join(", "))
}

/// 复制选定行为 UPDATE 语句。Navicat 口径：每行一条 UPDATE，WHERE 用主键
/// （mock 档第一列当主键）。
fn copy_as_update(kind: DbKind, db: &str, schema: &str, table: &str, cols: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() || cols.is_empty() {
        return String::new();
    }
    let qcol = |name: &str| match kind {
        DbKind::MySql | DbKind::MongoDb | DbKind::Redis => format!("`{name}`"),
        _ => format!("\"{name}\""),
    };
    let qtable = match kind {
        DbKind::MySql | DbKind::MongoDb | DbKind::Redis => format!("`{db}`.`{table}`"),
        _ => format!("\"{schema}\".\"{table}\""),
    };
    let pk = cols[0];
    rows.iter()
        .map(|r| {
            let sets = cols
                .iter()
                .skip(1)
                .zip(r.iter().skip(1))
                .map(|(c, v)| {
                    let val = if v == "NULL" {
                        "NULL".to_string()
                    } else {
                        format!("'{}'", v.replace('\'', "''"))
                    };
                    format!("{} = {val}", qcol(c))
                })
                .collect::<Vec<_>>()
                .join(", ");
            let pk_val = if r[0] == "NULL" {
                "NULL".to_string()
            } else {
                format!("'{}'", r[0].replace('\'', "''"))
            };
            format!("UPDATE {qtable} SET {sets} WHERE {} = {pk_val}", qcol(pk))
        })
        .collect::<Vec<_>>()
        .join(";\n")
}

/// 复制选定行为 SELECT 语句。Navicat 口径：SELECT * FROM table WHERE pk IN (...);
/// 多行用 IN 列表，单行用 =。
fn copy_as_select(kind: DbKind, db: &str, schema: &str, table: &str, cols: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() || cols.is_empty() {
        return String::new();
    }
    let qcol = |name: &str| match kind {
        DbKind::MySql | DbKind::MongoDb | DbKind::Redis => format!("`{name}`"),
        _ => format!("\"{name}\""),
    };
    let qtable = match kind {
        DbKind::MySql | DbKind::MongoDb | DbKind::Redis => format!("`{db}`.`{table}`"),
        _ => format!("\"{schema}\".\"{table}\""),
    };
    let pk = cols[0];
    let where_clause = if rows.len() == 1 {
        let v = if rows[0][0] == "NULL" {
            "NULL".to_string()
        } else {
            format!("'{}'", rows[0][0].replace('\'', "''"))
        };
        format!("{} = {v}", qcol(pk))
    } else {
        let vals = rows
            .iter()
            .map(|r| {
                if r[0] == "NULL" {
                    "NULL".to_string()
                } else {
                    format!("'{}'", r[0].replace('\'', "''"))
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} IN ({vals})", qcol(pk))
    };
    format!("SELECT * FROM {qtable} WHERE {where_clause}")
}

/// 复制选定行为 HTML 表格。Navicat 口径：<table> 带边框，第一行是表头。
fn copy_as_html(cols: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let header = cols
        .iter()
        .map(|c| format!("<th>{c}</th>"))
        .collect::<Vec<_>>()
        .join("");
    let body = rows
        .iter()
        .map(|r| {
            let cells = r
                .iter()
                .map(|v| format!("<td>{}</td>", v.replace('<', "&lt;").replace('>', "&gt;")))
                .collect::<Vec<_>>()
                .join("");
            format!("<tr>{cells}</tr>")
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("<table border=\"1\">\n<tr>{header}</tr>\n{body}\n</table>")
}

/// 全局键位与动作：F2 改名 / Esc 取消（改名或格子编辑）/ F5 刷新当前视图。
/// main 与 headless 测试共用这条装配，保证「绑定 → 全局动作 → defer 进窗口」
/// 的链路两边一致（键盘派发发生在 win.update 内部，监听器里直接 update 会
/// 重入失败，所以一律 defer 到本轮 effect 冲刷后再进窗口）。
fn wire_global_keys(root: Entity<DaroApp>, win: AnyWindowHandle, cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("f2", TreeRename, None),
        KeyBinding::new("escape", TreeCancelRename, None),
        KeyBinding::new("f5", RefreshView, None),
        KeyBinding::new("delete", DeleteSelected, None),
        KeyBinding::new("ctrl-s", Save, None),
        // 补全弹层（gpui 编辑器的 ShowCompletions 动作；编辑器只挂了
        // on_action 处理器，键位要在应用层绑上）
        KeyBinding::new("ctrl-space", gpui_kit::component::input::ShowCompletions, None),
    ]);
    let root_rename = root.clone();
    cx.on_action::<TreeRename>(move |_, cx| {
        let root = root_rename.clone();
        cx.defer(move |cx| {
            let _ = win.update(cx, |_, window, cx| {
                root.update(cx, |this, cx| this.start_rename(window, cx));
            });
        });
    });
    let root_cancel = root.clone();
    cx.on_action::<TreeCancelRename>(move |_, cx| {
        let root = root_cancel.clone();
        cx.defer(move |cx| {
            let _ = win.update(cx, |_, _, cx| {
                root.update(cx, |this, cx| {
                    // Esc 一鱼两吃：树在就地改名时取消改名；数据页在
                    // 编辑格子时丢弃编辑（两者实际不会同时打开）
                    this.cancel_rename(cx);
                    this.cancel_cell_edit(cx);
                });
            });
        });
    });
    let root_refresh = root.clone();
    cx.on_action::<RefreshView>(move |_, cx| {
        let root = root_refresh.clone();
        cx.defer(move |cx| {
            let _ = win.update(cx, |_, _, cx| {
                root.update(cx, |this, cx| this.refresh_view(cx));
            });
        });
    });
    let root_delete = root.clone();
    cx.on_action::<DeleteSelected>(move |_, cx| {
        let root = root_delete.clone();
        cx.defer(move |cx| {
            let _ = win.update(cx, |_, _, cx| {
                root.update(cx, |this, cx| this.delete_selected(cx));
            });
        });
    });
    let root_save = root.clone();
    cx.on_action::<Save>(move |_, cx| {
        let root = root_save.clone();
        cx.defer(move |cx| {
            let _ = win.update(cx, |_, window, cx| {
                root.update(cx, |this, cx| this.save_current(window, cx));
            });
        });
    });
}

fn main() {
    // 设计器整页 + 下拉浮层的渲染栈很深，Windows 主线程默认 1MB 栈会 overflow；
    // 把 UI 跑在一个 16MB 栈的线程上（消息循环跟着这个线程走）。
    std::thread::Builder::new()
        .name("daro-ui".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(ui_main)
        .expect("spawn ui thread")
        .join()
        .expect("ui thread");
}

fn ui_main() {
    let app = gpui_kit::application().with_assets(AllAssets);

    app.run(move |cx| {
        gpui_kit::init(cx);

        let mut options = TitleBar::window_options();
        options.window_min_size = Some(size(px(1080.), px(700.)));
        options.window_bounds = Some(WindowBounds::centered(size(px(1440.), px(900.)), cx));
        if let Some(titlebar) = options.titlebar.as_mut() {
            titlebar.title = Some("daro".into());
        }

        let (win, root) =
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| DaroApp::new(window, cx))
            })
            .expect("failed to open window");

        // 树行不持有焦点，元素级 on_action 在无焦点时不进派发路径
        // （dispatch_path 只有根节点），所以 F2 / Esc 挂在全局动作监听上，
        // 经窗口句柄桥进窗口上下文（改名编辑器要 window 才能建 + 聚焦）。
        // open_window 的注册根视图是 base::Root，不能 downcast 回 DaroApp，
        // 只能拿 Entity<DaroApp> 在窗口更新闭包里套一层 update。
        wire_global_keys(root.clone(), win, cx);

        // DARO_DEBUG 自动化冒烟：不依赖真实键盘 / 前台（锁屏下也能跑），
        // 用 dispatch_keystroke 走「键位绑定 → 全局动作 → 内联编辑器 → 提交/取消」全链路。
        if std::env::var("DARO_DEBUG").is_ok() {
            let root = root.clone();
            let root_step = root.clone();
            let step = move |cx: &mut AsyncApp,
                             f: fn(&mut Window, &mut App, &Entity<DaroApp>)|
                  -> bool {
                win.update(cx, |_, window, cx| {
                    f(window, cx, &root_step);
                })
                .is_ok()
            };
            cx.spawn(async move |cx| {
                async fn wait(cx: &AsyncApp, ms: u64) {
                    cx.background_executor()
                        .timer(Duration::from_millis(ms))
                        .await;
                }
                // 选中第一个连接再走后续键序
                step(cx, |_, cx, root| {
                    root.update(cx, |this, cx| {
                        if !this.conns.is_empty() {
                            this.sel = Act::Conn(0);
                        }
                        cx.notify();
                    });
                });
                eprintln!("[dbg] auto: sel=Conn(0)");
                wait(cx, 500).await;
                win.update(cx, |_, window, _| {
                    eprintln!(
                        "[dbg] auto: bindings_for_action(TreeRename)={} RefreshView={}",
                        window.bindings_for_action(&TreeRename).len(),
                        window.bindings_for_action(&RefreshView).len()
                    );
                    for b in window.bindings_for_action(&TreeRename) {
                        let keys: Vec<String> = b
                            .keystrokes()
                            .iter()
                            .map(|k| k.key().to_string())
                            .collect();
                        eprintln!("[dbg] auto: TreeRename binding keys={keys:?}");
                    }
                })
                .ok();
                step(cx, |window, cx, _| {
                    window.dispatch_keystroke(Keystroke {
                        modifiers: Modifiers::default(),
                        key: "f2".into(),
                        key_char: None,
                    }, cx);
                });
                eprintln!("[dbg] auto: f2 dispatched");
                wait(cx, 2500).await;
                win.update(cx, |_, _, cx| {
                    root.update(cx, |this, cx| {
                        let val = this
                            .rename_input
                            .as_ref()
                            .map(|i| i.read(cx).value().to_string());
                        eprintln!(
                            "[dbg] auto: after f2 renaming={:?} input={:?}",
                            this.renaming, val
                        );
                    });
                })
                .ok();
                step(cx, |window, cx, _| {
                    window.dispatch_keystroke(Keystroke {
                        modifiers: Modifiers::default(),
                        key: "x".into(),
                        key_char: Some("X".into()),
                    }, cx);
                });
                wait(cx, 400).await;
                win.update(cx, |_, _, cx| {
                    root.update(cx, |this, cx| {
                        let val = this
                            .rename_input
                            .as_ref()
                            .map(|i| i.read(cx).value().to_string());
                        eprintln!("[dbg] auto: after type input={:?}", val);
                    });
                })
                .ok();
                step(cx, |window, cx, _| {
                    window.dispatch_keystroke(Keystroke {
                        modifiers: Modifiers::default(),
                        key: "enter".into(),
                        key_char: None,
                    }, cx);
                });
                wait(cx, 500).await;
                win.update(cx, |_, _, cx| {
                    root.update(cx, |this, _| {
                        eprintln!(
                            "[dbg] auto: after enter name={:?} renaming={:?}",
                            this.conns.first().map(|c| c.name.clone()),
                            this.renaming
                        );
                    });
                })
                .ok();
                step(cx, |window, cx, _| {
                    window.dispatch_keystroke(Keystroke {
                        modifiers: Modifiers::default(),
                        key: "f2".into(),
                        key_char: None,
                    }, cx);
                });
                wait(cx, 500).await;
                step(cx, |window, cx, _| {
                    window.dispatch_keystroke(Keystroke {
                        modifiers: Modifiers::default(),
                        key: "escape".into(),
                        key_char: None,
                    }, cx);
                });
                wait(cx, 500).await;
                win.update(cx, |_, _, cx| {
                    root.update(cx, |this, _| {
                        eprintln!(
                            "[dbg] auto: after f2+esc name={:?} renaming={:?}",
                            this.conns.first().map(|c| c.name.clone()),
                            this.renaming
                        );
                    });
                })
                .ok();
                eprintln!("[dbg] auto: done");

                // 分组改名：F2 → 输入 → Enter，验证 conn.group / collapsed_groups 迁移
                step(cx, |_, cx, root| {
                    root.update(cx, |this, cx| {
                        if !this.groups.is_empty() {
                            this.sel = Act::Group(0);
                        }
                        cx.notify();
                    });
                });
                eprintln!("[dbg] auto: sel=Group(0)");
                wait(cx, 400).await;
                step(cx, |window, cx, _| {
                    window.dispatch_keystroke(Keystroke {
                        modifiers: Modifiers::default(),
                        key: "f2".into(),
                        key_char: None,
                    }, cx);
                });
                wait(cx, 600).await;
                win.update(cx, |_, _, cx| {
                    root.update(cx, |this, cx| {
                        let val = this
                            .rename_input
                            .as_ref()
                            .map(|i| i.read(cx).value().to_string());
                        eprintln!(
                            "[dbg] auto: group f2 renaming={:?} input={:?}",
                            this.renaming, val
                        );
                    });
                })
                .ok();
                step(cx, |window, cx, _| {
                    window.dispatch_keystroke(Keystroke {
                        modifiers: Modifiers::default(),
                        key: "g".into(),
                        key_char: Some("G".into()),
                    }, cx);
                });
                step(cx, |window, cx, _| {
                    window.dispatch_keystroke(Keystroke {
                        modifiers: Modifiers::default(),
                        key: "enter".into(),
                        key_char: None,
                    }, cx);
                });
                wait(cx, 500).await;
                win.update(cx, |_, _, cx| {
                    root.update(cx, |this, _| {
                        eprintln!(
                            "[dbg] auto: group after enter groups={:?} conns={:?} renaming={:?}",
                            this.groups,
                            this.conns.iter().map(|c| c.group.clone()).collect::<Vec<_>>(),
                            this.renaming
                        );
                    });
                })
                .ok();
                eprintln!("[dbg] auto: done2");
            })
            .detach();
        }
    });
}

/// 打开的数据表标签页。双击树里的表 / 视图节点追加，0 号位是常驻「对象」页。
/// 连接与库都按名字记（索引会随增删连接漂移），连接删了页签就地消失。
#[derive(Clone)]
struct OpenTab {
    conn_name: String,
    db: &'static str,
    schema: usize,
    cat: usize,
    obj: usize,
    /// 网格里当前格（行 × 列）：整行铺选中色，当前格铺 accent 反白
    cell: (usize, usize),
    /// 设计页签：内容走 [DaroApp::designer_page]，页签标题带「 (设计)」
    design: bool,
    /// 设计器工作状态；design=false 时恒为 None。
    /// did 只在设计页签上非零，编辑器写回靠它定位页签（下标会随关页漂移）。
    did: u64,
    dstate: Option<DesignerState>,
    /// 查询页签：内容走 [DaroApp::query_page]，与 design 互斥。
    query: bool,
    /// 查询页签的 id（编辑器写回 / 运行按钮定位页签用，同 did 的理由）。
    qid: u64,
    qstate: Option<QueryState>,
    /// 数据页列头排序：Some((列下标, 升序?))。cell.0 在排序态指**显示序位**
    /// （排序后的第 n 行），分页 / 选区 / 状态栏因此不用改口径。
    sort: Option<(usize, bool)>,
    /// 数据页签的取数结果（可编辑工作集）；设计 / 查询页签恒空。
    /// mock 档把它物化成真实二维表：编辑 / 增删改的是这份副本而不是
    /// 每帧现算的 mock_cell，行为与 Navicat 的「改了没保存」一致。
    data: Vec<Vec<String>>,
    /// 上次「应用」✓ 时的快照；「撤销」× / 刷新把 data 退回这里。
    baseline: Vec<Vec<String>>,
    /// 新增、尚未「应用」的行：显示时永远排在排序结果之后（Navicat 同款——
    /// 新行贴在网格尾部、行头带 *，「应用」后才并进 data 参与排序）。
    inserted: Vec<Vec<String>>,
    /// 数据页签的筛选 / 排序面板（Navicat 网格工具条漏斗）；设计 / 查询页签不用。
    filter: filter::FilterState,
}

impl OpenTab {
    /// 工作集相对基线有变化（⇒ 页签前缀 * / 导航条 ✓× 可用）。
    /// 设计 / 查询页签两者恒空 → 恒 false。
    fn dirty(&self) -> bool {
        self.data != self.baseline || !self.inserted.is_empty()
    }
}

/// 设计表页签里的一列。mock 档取 Flutter 版 DesignColumn 里字段页用得到的
/// 最小集；字段名与含义一一对应。
#[derive(Clone, Debug, PartialEq)]
struct DesignCol {
    name: String,
    ty: String,
    len: String,
    dec: String,
    not_null: bool,
    pk: bool,
    auto_inc: bool,
    comment: String,
    default: String,
    collation: String,
    /// PG「虚拟类型」：空 / ALWAYS / BY DEFAULT（MySQL 无此概念）
    identity: String,
}

impl Default for DesignCol {
    fn default() -> Self {
        Self {
            name: String::new(),
            ty: "varchar".into(),
            len: "255".into(),
            dec: String::new(),
            not_null: false,
            pk: false,
            auto_inc: false,
            comment: String::new(),
            default: String::new(),
            collation: String::new(),
            identity: String::new(),
        }
    }
}

/// 索引里的一列。参考工具「索引 → 字段」浮层一行七格：
/// 名称 / 排序规则模式 / 排序规则 / 运算符类别模式 / 运算符类别 / 排序顺序 /
/// Nulls 排序，这里一一对应；空串 = 用数据库默认（不写进 DDL）。
#[derive(Clone, Debug, PartialEq, Default)]
struct IndexField {
    name: String,
    /// 排序规则模式（pg_catalog），只有文本列才有
    collate_schema: String,
    /// 排序规则（default / C / POSIX …），只有文本列才有
    collation: String,
    /// 运算符类别模式（pg_catalog）
    opclass_schema: String,
    /// 运算符类别（int8_ops / text_ops …）
    opclass: String,
    /// 排序顺序（ASC / DESC）
    sort: String,
    /// Nulls 排序（NULLS FIRST / NULLS LAST）
    nulls: String,
}

/// 类型的默认运算符类别：参考工具里 int8 行是 int8_ops、varchar 行是 text_ops。
/// 只用来「按类型推默认值」和「跟默认值比，决定要不要写进 DDL」。
fn index_default_opclass(ty: &str) -> &'static str {
    match ty.trim().to_ascii_lowercase().as_str() {
        "int8" | "bigint" => "int8_ops",
        "int4" | "int" | "integer" | "serial" => "int4_ops",
        "int2" | "smallint" | "smallserial" => "int2_ops",
        "bool" | "boolean" => "bool_ops",
        "float8" | "double" | "double precision" => "float8_ops",
        "float4" | "real" => "float4_ops",
        "numeric" | "decimal" => "numeric_ops",
        "text" | "varchar" | "character varying" | "char" | "character" | "bpchar" => "text_ops",
        "date" => "date_ops",
        "time" | "timetz" => "time_ops",
        "timestamp" | "timestamptz" => "timestamp_ops",
        "uuid" => "uuid_ops",
        "json" => "json_ops",
        "jsonb" => "jsonb_ops",
        "bytea" => "bytea_ops",
        _ => "",
    }
}

/// 文本列才有排序规则（参考图里 int8 行的两格排序规则是空的）。
fn index_type_is_text(ty: &str) -> bool {
    matches!(
        ty.trim().to_ascii_lowercase().as_str(),
        "text" | "varchar" | "character varying" | "char" | "character" | "bpchar" | "citext"
    )
}

impl IndexField {
    /// 按列类型造一行：默认运算符类别 / 排序规则 / ASC / NULLS LAST。
    fn make(name: &str, ty: &str) -> Self {
        let texty = index_type_is_text(ty);
        Self {
            name: name.to_string(),
            collate_schema: if texty { "pg_catalog".into() } else { String::new() },
            collation: if texty { "default".into() } else { String::new() },
            opclass_schema: "pg_catalog".into(),
            opclass: index_default_opclass(ty).into(),
            sort: "ASC".into(),
            nulls: "NULLS LAST".into(),
        }
    }

    /// 换列：名称换掉，六项参数跟着新类型重算默认值。
    fn set_column(&mut self, name: &str, ty: &str) {
        let texty = index_type_is_text(ty);
        self.name = name.to_string();
        self.collate_schema = if texty { "pg_catalog".into() } else { String::new() };
        self.collation = if texty { "default".into() } else { String::new() };
        self.opclass_schema = "pg_catalog".into();
        self.opclass = index_default_opclass(ty).into();
        if self.sort.trim().is_empty() {
            self.sort = "ASC".into();
        }
        if self.nulls.trim().is_empty() {
            self.nulls = "NULLS LAST".into();
        }
    }

    /// 主表格「字段」格的摘要：`"tenant_id" int8_ops ASC NULLS LAST`
    fn summary(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() {
            return String::new();
        }
        let mut s = format!("\"{name}\"");
        let op = self.opclass.trim();
        if !op.is_empty() {
            s.push(' ');
            s.push_str(op);
        }
        if self.sort.trim().eq_ignore_ascii_case("DESC") {
            s.push_str(" DESC");
        } else {
            s.push_str(" ASC");
        }
        let nulls = self.nulls.trim().to_ascii_uppercase();
        if !nulls.is_empty() {
            s.push(' ');
            s.push_str(&nulls);
        }
        s
    }
}

/// 一条索引里的一列在 DDL 里的写法（不含表名）。
/// 排序 / Nulls 只在非默认时输出；运算符类别与默认值相同就不写，免得 DDL 里
/// 全是 int8_ops / text_ops 这类噪音。MySQL 只认 ASC/DESC。
fn index_field_sql(is_mysql: bool, f: &IndexField, ty: &str) -> String {
    let name = f.name.trim();
    let mut s = if is_mysql {
        format!("`{name}`")
    } else {
        format!("\"{name}\"")
    };
    if !is_mysql {
        let coll = f.collation.trim();
        if !coll.is_empty() && coll != "default" {
            s.push_str(&format!(" COLLATE \"{coll}\""));
        }
        let op = f.opclass.trim();
        if !op.is_empty() && op != index_default_opclass(ty) {
            s.push(' ');
            s.push_str(op);
        }
    }
    let sort = f.sort.trim().to_ascii_uppercase();
    let nulls = f.nulls.trim().to_ascii_uppercase();
    if sort == "DESC" {
        s.push_str(" DESC");
        // DESC 的默认是 NULLS FIRST，只有显式 LAST 才要写
        if nulls == "NULLS LAST" {
            s.push_str(" NULLS LAST");
        }
    } else if nulls == "NULLS FIRST" {
        s.push_str(" NULLS FIRST");
    }
    s
}

/// 索引方法的显示名（参考工具格子里是 B-Tree / Hash / GiST / GIN / BRIN / SP-GiST）。
fn index_method_label(method: &str) -> String {
    let key = method.trim().to_ascii_lowercase().replace(['-', ' ', '_'], "");
    match key.as_str() {
        "" | "btree" => "B-Tree".into(),
        "hash" => "Hash".into(),
        "gist" => "GiST".into(),
        "gin" => "GIN".into(),
        "brin" => "BRIN".into(),
        "spgist" => "SP-GiST".into(),
        _ => method.trim().to_string(),
    }
}

/// 索引方法 → DDL 关键字；空串返回 None（不写 USING，交给数据库默认）。
fn index_method_sql(method: &str) -> Option<String> {
    let key = method.trim().to_ascii_lowercase().replace(['-', ' ', '_'], "");
    match key.as_str() {
        "" => None,
        "btree" => Some("BTREE".into()),
        "hash" => Some("HASH".into()),
        "gist" => Some("GIST".into()),
        "gin" => Some("GIN".into()),
        "brin" => Some("BRIN".into()),
        "spgist" => Some("SPGIST".into()),
        // 手填的未知方法原样大写，别悄悄吞掉
        _ => Some(method.trim().to_ascii_uppercase()),
    }
}

/// 设计表页签里的一个索引：名称 / 字段列表 / 方法 / 唯一 / 并发 / 注释，
/// 外加参考工具底部那七行索引属性（PG 的存储参数，MySQL 不生成）。
#[derive(Clone, Debug, PartialEq)]
struct DesignIndex {
    name: String,
    /// 组成索引的列，顺序即索引列序
    fields: Vec<IndexField>,
    /// 索引方法（B-Tree / Hash / GiST / GIN / BRIN / SP-GiST），空 = 默认
    method: String,
    unique: bool,
    /// 并发建索引（PG 的 CREATE INDEX CONCURRENTLY）
    concurrent: bool,
    comment: String,
    // ── 索引属性（参考工具「索引」页底部那七行）──
    tablespace: String,
    fillfactor: String,
    buffering: String,
    fastupdate: String,
    pending_list_limit: String,
    pages_per_range: String,
    /// 约束：参考工具里这一格带「…」选择器；mock 档只记录，不生成 DDL
    constraint: String,
}

impl Default for DesignIndex {
    fn default() -> Self {
        Self {
            name: String::new(),
            fields: Vec::new(),
            method: String::new(),
            unique: false,
            concurrent: false,
            comment: String::new(),
            tablespace: String::new(),
            fillfactor: String::new(),
            buffering: String::new(),
            fastupdate: String::new(),
            pending_list_limit: String::new(),
            pages_per_range: String::new(),
            constraint: String::new(),
        }
    }
}

/// [`design_index_sql`] 的带模式版：`ON "public"."cameras"`（无模式退化为裸表名）。
fn design_index_sql_at(
    is_mysql: bool,
    schema: &str,
    table: &str,
    idx: &DesignIndex,
    cols: &[DesignCol],
) -> Option<String> {
    let kind = if is_mysql {
        mock::DbKind::MySql
    } else {
        mock::DbKind::PostgreSql
    };
    let qual = sql_preview::qualify(kind, schema, table);
    let name = idx.name.trim();
    let fields: Vec<&IndexField> = idx
        .fields
        .iter()
        .filter(|f| !f.name.trim().is_empty())
        .collect();
    if name.is_empty() || fields.is_empty() {
        return None;
    }
    let q = |n: &str| {
        if is_mysql {
            format!("`{n}`")
        } else {
            format!("\"{n}\"")
        }
    };
    let ty_of = |n: &str| {
        cols.iter()
            .find(|c| c.name == n)
            .map(|c| c.ty.as_str())
            .unwrap_or("")
    };
    let list: Vec<String> = fields
        .iter()
        .map(|f| index_field_sql(is_mysql, f, ty_of(f.name.trim())))
        .collect();
    let unique = if idx.unique { "UNIQUE " } else { "" };
    let concurrently = if !is_mysql && idx.concurrent {
        "CONCURRENTLY "
    } else {
        ""
    };
    let using = index_method_sql(&idx.method)
        .map(|m| format!(" USING {m}"))
        .unwrap_or_default();
    let mut sql = format!(
        "CREATE {unique}INDEX {concurrently}{} ON {qual}{using} ({})",
        q(name),
        list.join(", ")
    );
    if !is_mysql {
        let mut opts: Vec<String> = Vec::new();
        let ff = idx.fillfactor.trim();
        if !ff.is_empty() {
            opts.push(format!("fillfactor = {ff}"));
        }
        let buf = idx.buffering.trim();
        if !buf.is_empty() {
            opts.push(format!("buffering = {}", buf.to_ascii_lowercase()));
        }
        let fu = idx.fastupdate.trim();
        if !fu.is_empty() {
            opts.push(format!("fastupdate = {}", fu.to_ascii_lowercase()));
        }
        let pl = idx.pending_list_limit.trim();
        if !pl.is_empty() {
            opts.push(format!("pendinglistlimit = {pl}"));
        }
        let ppr = idx.pages_per_range.trim();
        if !ppr.is_empty() {
            opts.push(format!("pages_per_range = {ppr}"));
        }
        if !opts.is_empty() {
            sql.push_str(&format!(" WITH ({})", opts.join(", ")));
        }
        let ts = idx.tablespace.trim();
        if !ts.is_empty() {
            sql.push_str(&format!(" TABLESPACE {ts}"));
        }
    }
    sql.push(';');
    let comment = idx.comment.trim();
    if !comment.is_empty() {
        if is_mysql {
            // MySQL 只认挂在 CREATE INDEX 尾部的 COMMENT
            sql.pop();
            sql.push_str(&format!(" COMMENT '{}';", sql_literal(comment)));
        } else {
            sql.push_str(&format!(
                "\nCOMMENT ON INDEX {} IS '{}';",
                q(name),
                sql_literal(comment)
            ));
        }
    }
    Some(sql)
}

/// 设计表页签里的一个外键（迁移自 Flutter `DesignForeignKey` 的常用子集：
/// 名称 / 字段 / 引用表 / 引用字段 / 删除时 / 更新时 / 注释）。
#[derive(Clone, Debug, PartialEq)]
struct DesignForeignKey {
    name: String,
    columns: String,
    /// 被引用的模式（选引用表时自动带 public，可下拉改；PG 才进 DDL）
    ref_schema: String,
    ref_table: String,
    ref_columns: String,
    on_delete: String,
    on_update: String,
    comment: String,
}

impl Default for DesignForeignKey {
    fn default() -> Self {
        Self {
            name: String::new(),
            columns: String::new(),
            ref_schema: String::new(),
            ref_table: String::new(),
            ref_columns: String::new(),
            on_delete: "NO ACTION".into(),
            on_update: "NO ACTION".into(),
            comment: String::new(),
        }
    }
}

/// 唯一键行是否整行空白（种子空行：名称 / 字段都没填）。空白行不参与校验、
/// 不进 DDL、不计入「唯一键数」。
fn design_pair_blank(p: &DesignPair) -> bool {
    p.name.trim().is_empty() && p.value.trim().is_empty()
}

/// SQL 字符串字面量里的单引号双写
pub(crate) fn sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}

/// 从 MOCK_COLUMNS 拼一份假结构：varchar(32) 拆成类型 + 长度，
/// id 列当主键 + 自增，name 列 NOT NULL 带注释。
fn mock_design_cols(_table: &str) -> Vec<DesignCol> {
    mock::MOCK_COLUMNS
        .iter()
        .map(|m| {
            let (ty, len) = match m.ty.split_once('(') {
                Some((base, args)) => (base.to_string(), args.trim_end_matches(')').to_string()),
                None => (m.ty.to_string(), String::new()),
            };
            DesignCol {
                name: m.name.to_string(),
                ty,
                len,
                dec: String::new(),
                not_null: m.name == "id" || m.name == "name",
                pk: m.name == "id",
                auto_inc: m.name == "id",
                comment: if m.name == "name" { "名称".to_string() } else { String::new() },
                ..Default::default()
            }
        })
        .collect()
}

/// 一条外键的表内约束子句（不含行尾分号，放进 CREATE TABLE 的列清单里）。
/// 名称/字段/引用表/引用字段缺一返回 None（校验会先挡住）。
pub(crate) fn design_fk_sql(is_mysql: bool, fk: &DesignForeignKey) -> Option<String> {
    let name = fk.name.trim();
    let columns = fk.columns.trim();
    let ref_table = fk.ref_table.trim();
    let ref_columns = fk.ref_columns.trim();
    if name.is_empty() || columns.is_empty() || ref_table.is_empty() || ref_columns.is_empty() {
        return None;
    }
    let q = |n: &str| {
        if is_mysql {
            format!("`{n}`")
        } else {
            format!("\"{n}\"")
        }
    };
    let col_list = |list: &str| -> Vec<String> {
        list.split(',')
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
            .map(q)
            .collect()
    };
    let cols = col_list(columns);
    let refs = col_list(ref_columns);
    if cols.is_empty() || refs.is_empty() {
        return None;
    }
    // 被引用的模式非空时按 schema.table 限定（MySQL 没有模式层，直接表名）
    let refq = if !is_mysql && !fk.ref_schema.trim().is_empty() {
        format!("{}.{}", q(fk.ref_schema.trim()), q(ref_table))
    } else {
        q(ref_table)
    };
    let mut sql = format!(
        "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
        q(name),
        cols.join(", "),
        refq,
        refs.join(", ")
    );
    let od = fk.on_delete.trim().to_uppercase();
    if !od.is_empty() && od != "NO ACTION" {
        sql.push_str(&format!(" ON DELETE {od}"));
    }
    let ou = fk.on_update.trim().to_uppercase();
    if !ou.is_empty() && ou != "NO ACTION" {
        sql.push_str(&format!(" ON UPDATE {ou}"));
    }
    Some(sql)
}

/// 按方言生成触发器语句（可能一条触发器拆成多条，如 MySQL 多事件）。
fn design_triggers_sql(
    kind: mock::DbKind,
    table: &str,
    triggers: &[DesignTrigger],
) -> Vec<String> {
    let mut out = Vec::new();
    for t in triggers {
        let name = t.name.trim();
        let timing = t.timing.trim().to_uppercase();
        let events = design_trigger_events(t);
        if name.is_empty() || events.is_empty() {
            continue;
        }
        match kind {
            mock::DbKind::MySql => {
                // MySQL 一个触发器只能挂一个事件，多事件按事件后缀拆成多条
                for ev in &events {
                    let trig_name = if events.len() > 1 {
                        format!("{name}_{}", ev.to_lowercase())
                    } else {
                        name.to_string()
                    };
                    out.push(format!(
                        "CREATE TRIGGER `{trig_name}` {timing} {ev} ON `{table}` FOR EACH ROW {};",
                        t.body.trim()
                    ));
                }
            }
            mock::DbKind::PostgreSql => {
                let each = if t.for_each.trim().starts_with('语') {
                    "STATEMENT"
                } else {
                    "ROW"
                };
                // 勾了「更新」且填了「更新字段」：UPDATE 出 UPDATE OF …
                let events = if t.has_event("UPDATE") && !t.update_fields.trim().is_empty() {
                    events
                        .iter()
                        .map(|e| {
                            if e == "UPDATE" {
                                format!("UPDATE OF {}", t.update_fields.trim())
                            } else {
                                e.clone()
                            }
                        })
                        .collect::<Vec<_>>()
                } else {
                    events
                };
                out.push(format!(
                    "CREATE TRIGGER \"{name}\" {timing} {} ON \"{table}\" FOR EACH {each} EXECUTE FUNCTION {}();",
                    events.join(" OR "),
                    t.function.trim()
                ));
            }
            mock::DbKind::SqlServer => {
                // SQL Server 是语句级，没有 BEFORE；AFTER 之外的写 INSTEAD OF
                let timing = if timing == "INSTEAD OF" { "INSTEAD OF" } else { "AFTER" };
                out.push(format!(
                    "CREATE TRIGGER \"{name}\" ON \"{table}\" {timing} {} AS {};",
                    events.join(", "),
                    t.body.trim()
                ));
            }
            // MongoDB / Redis 没有触发器
            _ => {}
        }
    }
    out
}

/// 按方言生成触发器语句的带模式版本（更改脚本用）：触发表的 schema 限定
/// （MySQL 库即模式，不动；PG / SQL Server 的 ON "schema"."table"）。
pub(crate) fn design_triggers_sql_at(
    kind: mock::DbKind,
    schema: &str,
    table: &str,
    triggers: &[DesignTrigger],
) -> Vec<String> {
    let mut out = design_triggers_sql(kind, table, triggers);
    let is_pg = matches!(kind, mock::DbKind::PostgreSql);
    if !matches!(kind, mock::DbKind::MySql) && !schema.is_empty() {
        for s in &mut out {
            if let Some(pos) = s.find(" ON \"") {
                s.insert_str(pos + 5, &format!("{schema}\".\""));
            }
        }
    }
    // 触发器注释：PG 用 COMMENT ON TRIGGER
    if is_pg {
        let qual = if schema.is_empty() {
            format!("\"{table}\"")
        } else {
            format!("\"{schema}\".\"{table}\"")
        };
        for t in triggers.iter().filter(|t| !t.comment.trim().is_empty()) {
            out.push(format!(
                "COMMENT ON TRIGGER {} ON {qual} IS '{}';",
                format!("\"{}\"", t.name.trim()),
                sql_literal(t.comment.trim())
            ));
        }
    }
    out
}

/// 外键行是否整行空白（种子空行：名称 / 字段 / 引用表 / 引用字段都没填）。
/// 空白行不参与校验、不进 DDL、不计入「外键数」—— 参考工具的空行常驻在
/// 表格里等着输入，不算一条外键。
fn design_fk_blank(fk: &DesignForeignKey) -> bool {
    fk.name.trim().is_empty()
        && fk.columns.trim().is_empty()
        && fk.ref_table.trim().is_empty()
        && fk.ref_columns.trim().is_empty()
}

/// 索引行是否整行空白（种子空行：名称 / 字段 / 方法 / 注释 / 属性都没填）。
/// 空白行不参与校验、不进 DDL、不计入「索引数」—— 参考工具的空行常驻在表格里
/// 等着直接输入，不算一条索引。
fn design_index_blank(idx: &DesignIndex) -> bool {
    idx.name.trim().is_empty()
        && idx.fields.iter().all(|f| f.name.trim().is_empty())
        && idx.method.trim().is_empty()
        && !idx.unique
        && !idx.concurrent
        && idx.comment.trim().is_empty()
        && idx.tablespace.trim().is_empty()
        && idx.fillfactor.trim().is_empty()
        && idx.buffering.trim().is_empty()
        && idx.fastupdate.trim().is_empty()
        && idx.pending_list_limit.trim().is_empty()
        && idx.pages_per_range.trim().is_empty()
        && idx.constraint.trim().is_empty()
}

/// 设计器打开时的索引初值：mock 表带一条示例索引；一张都没有时补一行空白种子行，
/// 用户可以直接在行里输入，少点一次「添加索引」。
fn initial_design_indexes(table: &str) -> Vec<DesignIndex> {
    let mut v = mock_design_indexes(table);
    if v.is_empty() {
        v.push(DesignIndex::default());
    }
    v
}

/// 排除约束行是否整行空白（种子空行）。
fn design_exclude_blank(ex: &DesignExclude) -> bool {
    ex.name.trim().is_empty()
        && ex.elements.trim().is_empty()
        && ex.predicate.trim().is_empty()
        && ex.comment.trim().is_empty()
}

/// 规则行是否整行空白（种子空行）。
fn design_rule_blank(r: &DesignRule) -> bool {
    r.name.trim().is_empty()
        && r.event.trim().is_empty()
        && r.qualification.trim().is_empty()
        && r.action.trim().is_empty()
        && r.statement.trim().is_empty()
}

/// 触发器行是否整行空白（种子空行）。
fn design_trigger_blank(t: &DesignTrigger) -> bool {
    t.name.trim().is_empty()
        && t.timing.trim().is_empty()
        && t.events.trim().is_empty()
        && t.for_each.trim().is_empty()
        && t.function.trim().is_empty()
        && t.body.trim().is_empty()
}

/// 唯一键 / 检查子页的「名称 + 内容」模型：唯一键的内容是字段列表，
/// 检查的内容是布尔表达式；注释与「无继承」按各自子页的参考图补齐。
#[derive(Clone, Debug, PartialEq, Default)]
struct DesignPair {
    name: String,
    value: String,
    /// 注释（PG 生成 COMMENT ON CONSTRAINT）
    comment: String,
    /// 检查子页「无继承」：PG 生成 NO INHERIT（继承层级里不向下传递）
    no_inherit: bool,
}

/// 设计器浮层弹层要写回哪个字段（参考工具点格子右边的 … 弹出面板）。
#[derive(Clone, Copy, PartialEq)]
enum PickTarget {
    /// 索引的「字段」：勾选列表面板（勾上的列即索引字段）
    IndexCols,
    /// 外键本地字段
    FkCols,
    /// 外键「被引用的字段」
    FkRefCols,
    /// 唯一键字段
    UniqueCols,
    /// 排除约束的「元素」
    ExcludeElems,
    // ── 外键子页的普通下拉 / 文本域（参考图每列的交互）──
    /// 被引用的模式：普通下拉（连接里的模式清单）
    FkSchema,
    /// 被引用的表：普通下拉（按已输入文本过滤）
    FkRefTable,
    /// 删除时 / 更新时：普通下拉（五种引用动作）
    FkOnDelete,
    FkOnUpdate,
    /// 注释：多行文本域
    FkComment,
    /// 触发器「更新字段」：勾选列浮层（确定写回 update_fields）
    TrigFields,
    /// 选项「继承自」：勾选父表浮层（确定写回 inherit）
    InheritTables,
}

impl PickTarget {
    fn title(self) -> &'static str {
        match self {
            PickTarget::FkRefCols => "被引用的字段",
            PickTarget::ExcludeElems => "元素",
            PickTarget::FkComment => "注释",
            _ => "字段",
        }
    }

    /// 普通下拉（无勾选框 / 表头 / 底栏，点一项即写回）。
    fn is_plain(self) -> bool {
        matches!(
            self,
            PickTarget::FkSchema
                | PickTarget::FkRefTable
                | PickTarget::FkOnDelete
                | PickTarget::FkOnUpdate
        )
    }

    /// 弹层锚点用的 (子页, 列号)：外键页 8 列、唯一键页 3 列
    fn grid_anchor(self) -> Option<(usize, usize)> {
        match self {
            PickTarget::FkCols => Some((2, 1)),
            PickTarget::FkSchema => Some((2, 2)),
            PickTarget::FkRefTable => Some((2, 3)),
            PickTarget::FkRefCols => Some((2, 4)),
            PickTarget::FkOnDelete => Some((2, 5)),
            PickTarget::FkOnUpdate => Some((2, 6)),
            PickTarget::FkComment => Some((2, 7)),
            PickTarget::UniqueCols => Some((3, 1)),
            PickTarget::TrigFields => Some((7, 8)),
            _ => None,
        }
    }
}

/// 外键格子右缘的浮层入口（参考图只有当前格显示：列选择/注释是 …，下拉是 ▼）
#[derive(Clone, Copy)]
enum FkAfford {
    /// 列选择浮层（字段 / 被引用的字段）
    More(PickTarget),
    /// 普通下拉（被引用的模式 / 被引用的表 / 删除时 / 更新时）
    Drop(PickTarget),
    /// 注释文本域
    Note,
}

/// 检查子页的「名称 + 表达式」模型键（唯一键有了独立 1:1 页，不再共用）
enum PairTab {
    Check,
}

impl PairTab {
    fn title(self) -> &'static str {
        match self {
            Self::Check => "检查",
        }
    }

    fn value_label(self) -> &'static str {
        match self {
            Self::Check => "表达式",
        }
    }

    fn value_placeholder(self) -> &'static str {
        match self {
            Self::Check => "如 age > 0",
        }
    }
}

fn pair_list(ds: &DesignerState, _kind: PairTab) -> &Vec<DesignPair> {
    &ds.checks
}

fn pair_list_mut(ds: &mut DesignerState, _kind: PairTab) -> &mut Vec<DesignPair> {
    &mut ds.checks
}

fn pair_edits_mut(
    ds: &mut DesignerState,
    _kind: PairTab,
) -> &mut Vec<[Option<Entity<InputState>>; 3]> {
    &mut ds.check_edits
}

/// 逗号分隔的字段串 → 名字列表
fn split_cols(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
        .map(|x| x.to_string())
        .collect()
}

/// 下拉菜单的「选项 → 写回」：`values` 是 (显示, 写回值)，`apply` 收写回值。
/// 索引页那些「格子 + 小箭头」的列（方法 / 表空间 / 排序规则…）都走这套。
fn pick_menu(
    values: Vec<(String, String)>,
    apply: Rc<dyn Fn(String, &mut App)>,
) -> impl Fn(PopupMenu) -> PopupMenu + 'static {
    move |menu| {
        let mut m = menu;
        for (label, value) in &values {
            let apply = apply.clone();
            let value = value.clone();
            m = m.item(
                PopupMenuItem::new(label.clone())
                    .on_click(move |_, _, cx| apply(value.clone(), cx)),
            );
        }
        m
    }
}

/// mock 档的索引：参考图那屏是一条「int 列 + varchar 列」的唯一索引
/// （`ix_organizations_tenant`：tenant_id + name），这里按 mock 列名换成
/// `ix_<表>_org`（org_id + name），形状一致，一打开「索引」页就有东西可看。
fn mock_design_indexes(table: &str) -> Vec<DesignIndex> {
    let cols = mock_design_cols(table);
    let ty_of = |n: &str| cols.iter().find(|c| c.name == n).map(|c| c.ty.clone());
    let (Some(t_org), Some(t_name)) = (ty_of("org_id"), ty_of("name")) else {
        return Vec::new();
    };
    vec![DesignIndex {
        name: format!("ix_{table}_org"),
        fields: vec![
            IndexField::make("org_id", &t_org),
            IndexField::make("name", &t_name),
        ],
        unique: true,
        ..Default::default()
    }]
}

/// 选项子页的三个值（PG：填充因子 / 表空间 / 表注释；MySQL 出行内 COMMENT='…'）
#[derive(Clone, Debug, PartialEq, Default)]
struct DesignOptions {
    fillfactor: String,
    tablespace: String,
    comment: String,
    /// 「不记录」：PG 的 UNLOGGED 表（不写 WAL，崩溃后不恢复）
    unlogged: bool,
    /// 「所有者」：表属主（PG 出 ALTER TABLE OWNER TO，mock 档仅展示）
    owner: String,
    /// 「继承自」：父表名（PG 出 INHERITS (…)）
    inherit: String,
    /// 「集群」：CLUSTER 依据的索引名
    cluster: String,
}

/// 排除约束（仅 PostgreSQL 生成 DDL）：元素串形如 `room WITH =`，
/// 方法是索引访问方法（gist / spgist / gin …），谓词是不带 WHERE 的条件。
#[derive(Clone, Debug, PartialEq, Default)]
struct DesignExclude {
    name: String,
    elements: String,
    /// 索引访问方法的 SQL 名（btree / gist / …），展示名见 EXCLUDE_METHODS
    method: String,
    predicate: String,
    /// 注释（PG 生成 COMMENT ON CONSTRAINT）
    comment: String,
}

/// 排除约束「索引方法」的 (展示名, SQL 名)：DDL 里 USING 用 SQL 名
pub(crate) const EXCLUDE_METHODS: [(&str, &str); 5] = [
    ("B-Tree", "btree"),
    ("GiST", "gist"),
    ("GIN", "gin"),
    ("BRIN", "brin"),
    ("SP-GiST", "spgist"),
];

/// 排除「元素」浮层的行草稿：参考图七列（元素 / 运算符类别模式 / 运算符类别 /
/// 排序 / Nulls 排序 / 运算符模式 / 运算符）。
#[derive(Clone, Debug, Default, PartialEq)]
struct ExcludeRow {
    on: bool,
    name: String,
    opclass_schema: String,
    opclass: String,
    sort: String,
    nulls: String,
    operator_schema: String,
    operator: String,
}

impl ExcludeRow {
    fn empty(name: &str, on: bool) -> Self {
        Self {
            on,
            name: name.to_string(),
            ..Default::default()
        }
    }
}

/// 把 elements 串拆成行草稿：`room [opclass] WITH =` 一段一行；
/// 与表格列匹配的行 on=true（勾选排前），没匹配上的自由片段放最后。
fn parse_exclude_rows(elements: &str, cols: &[DesignCol]) -> Vec<ExcludeRow> {
    let mut rows: Vec<ExcludeRow> = Vec::new();
    for part in elements.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (left, op) = match part.split_once(" WITH ") {
            Some((l, r)) => (l.trim(), r.trim()),
            None => (part, ""),
        };
        let mut it = left.split_whitespace();
        let name = it.next().unwrap_or_default().to_string();
        let rest: Vec<&str> = it.collect();
        let mut row = ExcludeRow {
            on: true,
            operator: op.to_string(),
            ..Default::default()
        };
        row.name = name;
        if rest.len() >= 2 {
            row.opclass_schema = rest[0].to_string();
            row.opclass = rest[1].to_string();
        } else if rest.len() == 1 {
            row.opclass = rest[0].to_string();
        }
        rows.push(row);
    }
    // 表格里的列：勾选的排前面，没勾的排后面（参考图整列都在，等用户勾）
    for c in cols {
        let name = c.name.trim();
        if name.is_empty() {
            continue;
        }
        let pos = rows.iter().position(|r| r.name == name);
        let mut row = ExcludeRow::empty(name, pos.is_some());
        if let Some(i) = pos {
            row = rows.remove(i);
        }
        rows.push(row);
    }
    rows
}

/// 行草稿序列化回 elements 串：`name [opclass] WITH op`（运算符空缺省 =），
/// 只取勾上的行，顺序即草稿顺序。
fn serialize_exclude_rows(rows: &[ExcludeRow]) -> String {
    rows.iter()
        .filter(|r| r.on && !r.name.trim().is_empty())
        .map(|r| {
            let opclass = r.opclass.trim();
            let op = if r.operator.trim().is_empty() {
                "="
            } else {
                r.operator.trim()
            };
            let schema = r.operator_schema.trim();
            if opclass.is_empty() && schema.is_empty() {
                format!("{} WITH {}", r.name.trim(), op)
            } else if schema.is_empty() {
                format!("{} {} WITH {}", r.name.trim(), opclass, op)
            } else {
                format!(
                    "{} {} OPERATOR ({}) WITH {}",
                    r.name.trim(),
                    opclass,
                    schema,
                    op
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// 排除「元素」浮层的草稿：行 + 选中行 + 每行七格的常驻输入
#[derive(Clone)]
struct ExcludeElementsEditor {
    /// 正在编辑的排除约束行
    row: usize,
    rows: Vec<ExcludeRow>,
    /// 选中行（↑ ↓ + − 作用在它上面）
    sel: usize,
    /// 每行七格的输入（[元素, 运算符类别模式, 运算符类别, 排序, Nulls 排序,
    /// 运算符模式, 运算符]），随行增删重建
    inputs: Vec<[Option<Entity<InputState>>; 7]>,
}

/// 规则（仅 PostgreSQL）：CREATE RULE "name" AS ON event [WHERE (…) DO …]。
/// 启用默认勾上（参考图种子行）；PG 的规则没有 ENABLE / DISABLE 语法，仅展示。
#[derive(Clone, Debug, PartialEq)]
struct DesignRule {
    name: String,
    event: String,
    qualification: String,
    action: String,
    statement: String,
    enabled: bool,
    comment: String,
}

impl Default for DesignRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            event: String::new(),
            qualification: String::new(),
            action: String::new(),
            statement: String::new(),
            enabled: true,
            comment: String::new(),
        }
    }
}

/// 规则「代替运行」下拉的候选（参考图只有 ALSO / INSTEAD 两项）
const RULE_ACTIONS: [&str; 2] = ["ALSO", "INSTEAD"];

/// 触发器（三方言）：MySQL 按事件拆条、PG 引用函数、SQL Server 语句级内联主体
#[derive(Clone, Debug, PartialEq, Default)]
struct DesignTrigger {
    name: String,
    timing: String,
    events: String,
    for_each: String,
    function: String,
    body: String,
    /// 「更新字段」：UPDATE 事件勾上时的字段清单（PG 出 UPDATE OF …）
    update_fields: String,
    /// 「启用」：PG / MySQL / SQL Server 的触发器可禁用，默认勾上
    enabled: bool,
    /// 「当」：WHEN 条件（可空）
    when: String,
    /// 「参数」
    args: String,
    /// 注释（PG 生成 COMMENT ON TRIGGER）
    comment: String,
}

impl DesignTrigger {
    /// 事件勾选格（插入 / 更新 / 删除 / 截断）的读写
    fn has_event(&self, ev: &str) -> bool {
        self.events
            .split(',')
            .any(|e| e.trim().eq_ignore_ascii_case(ev))
    }

    fn set_event(&mut self, ev: &str, on: bool) {
        let mut list: Vec<String> = self
            .events
            .split(',')
            .map(|e| e.trim().to_uppercase())
            .filter(|e| !e.is_empty())
            .collect();
        let upper = ev.to_uppercase();
        match list.iter().position(|e| *e == upper) {
            Some(i) if !on => {
                list.remove(i);
            }
            None if on => list.push(upper),
            _ => {}
        }
        self.events = list.join(", ");
    }
}

/// 触发器的事件清单（逗号分隔 → 大写列表）
fn design_trigger_events(t: &DesignTrigger) -> Vec<String> {
    t.events
        .split(',')
        .map(|e| e.trim().to_uppercase())
        .filter(|e| !e.is_empty())
        .collect()
}

/// 检查约束子句：`CONSTRAINT name CHECK (expr)`；PG 且勾了「无继承」追加
/// NO INHERIT（继承层级里不向下传递）。
pub(crate) fn design_check_sql(is_pg: bool, ck: &DesignPair) -> Option<String> {
    let name = ck.name.trim();
    let expr = ck.value.trim();
    if name.is_empty() || expr.is_empty() {
        return None;
    }
    let quoted = if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        name.to_string()
    } else {
        format!("{name}")
    };
    let no_inherit = if is_pg && ck.no_inherit { " NO INHERIT" } else { "" };
    Some(format!("CONSTRAINT {quoted} CHECK ({expr}){no_inherit}"))
}

/// 唯一键约束子句：`CONSTRAINT name UNIQUE (cols)`（表内约束）。
pub(crate) fn design_unique_sql(is_mysql: bool, uk: &DesignPair) -> Option<String> {
    let name = uk.name.trim();
    let cols = uk.value.trim();
    if name.is_empty() || cols.is_empty() {
        return None;
    }
    let q = |n: &str| {
        if is_mysql {
            format!("`{n}`")
        } else {
            format!("\"{n}\"")
        }
    };
    let list: Vec<String> = cols
        .split(',')
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .map(q)
        .collect();
    if list.is_empty() {
        return None;
    }
    Some(format!("CONSTRAINT {} UNIQUE ({})", q(name), list.join(", ")))
}

/// 排除约束子句：`CONSTRAINT name EXCLUDE USING method (elements) WHERE (predicate)`
/// （表内约束；方法为空时省 USING，谓词为空时省 WHERE）。
/// 行存在就出形状（参考工具口径：新加的空行出 `EXCLUDE USING btree`）——
/// 名字空着不带 CONSTRAINT 前缀、元素空着不带括号；完整性仍由
/// design_excludes_validation 把守，预览不挡。
fn design_exclude_sql(ex: &DesignExclude) -> Option<String> {
    let name = ex.name.trim();
    let elements = ex.elements.trim();
    let method = if ex.method.trim().is_empty() {
        "btree"
    } else {
        ex.method.trim()
    };
    let mut sql = String::new();
    if !name.is_empty() {
        sql.push_str(&format!("CONSTRAINT \"{name}\" "));
    }
    sql.push_str(&format!("EXCLUDE USING {method}"));
    if !elements.is_empty() {
        sql.push_str(&format!(" ({elements})"));
    }
    let predicate = ex.predicate.trim();
    if !predicate.is_empty() {
        sql.push_str(&format!(" WHERE ({predicate})"));
    }
    Some(sql)
}

/// 规则子页的 DDL（仅 PostgreSQL）：`CREATE RULE "name" AS ON event
/// [WHERE (cond)] DO [ALSO | INSTEAD] stmt | DO NOTHING`。
fn design_rule_sql(rule: &DesignRule) -> Option<String> {
    let name = rule.name.trim();
    let event = rule.event.trim().to_uppercase();
    if name.is_empty() || event.is_empty() {
        return None;
    }
    let quoted = format!("\"{name}\"");
    let action = rule.action.trim().to_uppercase();
    let stmt = rule.statement.trim();
    let tail = if action == "NOTHING" || stmt.is_empty() {
        "DO NOTHING".to_string()
    } else if action.is_empty() {
        format!("DO {stmt}")
    } else {
        format!("DO {action} {stmt}")
    };
    let qualification = rule.qualification.trim();
    let where_clause = if qualification.is_empty() {
        String::new()
    } else {
        format!(" WHERE ({qualification})")
    };
    Some(format!(
        "CREATE RULE {quoted} AS ON {event}{where_clause} {tail};"
    ))
}

/// 视图 / 实体化视图的 DDL 预览：正文为空时给出提示注释，否则 `CREATE <kind> <name> AS <body>;`
pub fn design_view_sql(kind_label: &str, name: &str, body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        format!(
            "-- 在「定义」页写入 SELECT 语句后，这里显示生成的 {kind_label} 定义\n-- CREATE {kind_label} {name} AS …\n"
        )
    } else {
        format!("CREATE {kind_label} {name} AS\n{body};\n")
    }
}

/// 完整列类型：类型自带括号（varchar(32)）就原样，否则把长度拼上去
fn design_full_type(c: &DesignCol) -> String {
    let ty = c.ty.trim();
    if ty.is_empty() {
        return String::new();
    }
    let len = c.len.trim();
    if len.is_empty() || ty.contains('(') {
        return ty.to_string();
    }
    let dec = c.dec.trim();
    if dec.is_empty() {
        format!("{ty}({len})")
    } else {
        format!("{ty}({len},{dec})")
    }
}

/// 长度 / 小数点的显示兜底：模型里没填时按类型给参考值（int8 → 64、bool → 1、
/// float8 → 53 …），只在显示层，不进模型、不影响 DDL。
fn design_len_display(ty: &str, len: &str) -> String {
    if !len.trim().is_empty() {
        return len.to_string();
    }
    let known = match ty.trim() {
        "int8" | "bigint" => "64",
        "int4" | "integer" | "int" => "32",
        "int2" | "smallint" => "16",
        "bool" | "boolean" => "1",
        "float8" | "double precision" => "53",
        "float4" | "real" => "24",
        _ => "",
    };
    known.to_string()
}

/// 索引「字段」浮层（参考工具点「字段」格弹出的小表格）的工作状态：
/// 草稿与选中行，确定才写回模型。
#[derive(Clone)]
struct IndexFieldsEditor {
    /// 正在编辑的索引行
    row: usize,
    fields: Vec<IndexField>,
    /// 选中行（↑ ↓ + − 作用在它上面）
    sel: usize,
}

/// 触发器「更新字段」勾选浮层的草稿：确定才写回 update_fields
#[derive(Clone)]
struct TriggerFieldsEditor {
    /// 正在编辑的触发器行
    row: usize,
    items: Vec<(String, bool)>,
    /// 选中行（↑ ↓ 作用在它上面）
    sel: usize,
}

/// 设计器工作状态，随设计页签存亡（放 [OpenTab] 里，关页签即弃）。
/// baseline_* 是打开 / 保存时的快照：「已修改 / 无变更」拿工作集跟它比，
/// SQL 预览页的「之前」与「更改」脚本也从这批基线出发。
#[derive(Clone)]
pub(crate) struct DesignerState {
    pub(crate) popup: Option<DesignPopup>,
    /// 各子页表头拖动后的列宽：key = (子页, 列序号)
    pub(crate) widths: HashMap<(usize, usize), f32>,
    /// 正在拖动表头分割线：(子页, 列序号, 按下时的 x, 按下时的列宽)
    pub(crate) resize: Option<(usize, usize, f32, f32)>,
    pub(crate) cols: Vec<DesignCol>,
    pub(crate) baseline: Vec<DesignCol>,
    /// 锚点行（属性面板显示它的属性）
    pub(crate) sel: usize,
    /// 当前列（表头淡蓝块 + 当前格跟它走）
    pub(crate) cur: usize,
    /// 子页签 0..=10
    pub(crate) sub: usize,
    /// 字段子页的常驻编辑器，每行 [名称, 长度, 小数点, 注释, 默认, 虚拟类型, 类型]。
    pub(crate) edits: Vec<[Option<Entity<InputState>>; 7]>,
    /// 索引子页的模型与常驻编辑器（每行 [名称, 注释, 填充因子, 待处理列表限制,
    /// 每范围页数, 约束]）。
    pub(crate) indexes: Vec<DesignIndex>,
    pub(crate) index_edits: Vec<[Option<Entity<InputState>>; 6]>,
    pub(crate) index_sel: usize,
    pub(crate) index_cur: usize,
    /// 索引「字段」浮层的草稿（None = 收起）
    pub(crate) idx_editor: Option<IndexFieldsEditor>,
    pub(crate) baseline_indexes: Vec<DesignIndex>,
    /// 外键子页的模型与常驻编辑器（每行 [名称, 引用表, 注释]）。
    pub(crate) fks: Vec<DesignForeignKey>,
    pub(crate) fk_edits: Vec<[Option<Entity<InputState>>; 5]>,
    pub(crate) fk_sel: usize,
    pub(crate) fk_cur: usize,
    pub(crate) baseline_fks: Vec<DesignForeignKey>,
    /// 唯一键子页：常驻一行空白种子行（参考工具口径：空行等着输入，不算数据）。
    pub(crate) uniques: Vec<DesignPair>,
    pub(crate) unique_edits: Vec<[Option<Entity<InputState>>; 3]>,
    pub(crate) unique_sel: usize,
    pub(crate) unique_cur: usize,
    pub(crate) baseline_uniques: Vec<DesignPair>,
    /// 检查子页：常驻一行空白种子行（与外键 / 唯一键同口径）
    pub(crate) checks: Vec<DesignPair>,
    pub(crate) check_edits: Vec<[Option<Entity<InputState>>; 3]>,
    pub(crate) check_sel: usize,
    pub(crate) check_cur: usize,
    pub(crate) baseline_checks: Vec<DesignPair>,
    /// 选项 / 注释子页：填充因子、表空间、表注释
    pub(crate) options: DesignOptions,
    pub(crate) opt_edits: [Option<Entity<InputState>>; 5],
    /// 排除子页（仅 PostgreSQL）：名称 / 元素 / 方法 / 谓词 / 注释
    pub(crate) excludes: Vec<DesignExclude>,
    pub(crate) exclude_edits: Vec<[Option<Entity<InputState>>; 3]>,
    /// 排除子页当前行 / 当前列（0 名称 … 3 注释）
    pub(crate) exclude_sel: usize,
    pub(crate) exclude_cur: usize,
    /// 规则子页（仅 PostgreSQL）：名称 / 事件 / 条件 / 动作 / 语句
    pub(crate) rules: Vec<DesignRule>,
    pub(crate) rule_edits: Vec<[Option<Entity<InputState>>; 5]>,
    /// 规则子页当前行 / 当前列（0 名称 … 4 注释）
    pub(crate) rule_sel: usize,
    pub(crate) rule_cur: usize,
    /// 触发器子页（三方言）：名称 / 时机 / 事件 / 级 / 函数 / 主体
    pub(crate) triggers: Vec<DesignTrigger>,
    pub(crate) trigger_edits: Vec<[Option<Entity<InputState>>; 3]>,
    /// 触发器子页当前行 / 当前列（0 名称 … 9 注释）
    pub(crate) trig_sel: usize,
    pub(crate) trig_cur: usize,
    /// 「更新字段」勾选浮层草稿（None = 收起）
    pub(crate) trig_fields_editor: Option<TriggerFieldsEditor>,
    /// 排除「元素」浮层的草稿（None = 收起）
    pub(crate) exclude_editor: Option<ExcludeElementsEditor>,
    /// 保存基线：选项 / 排除 / 规则 / 触发器（baseline_checks 在检查子页字段旁，
    /// 与 baseline、baseline_indexes、baseline_fks、baseline_uniques 同口径）。
    pub(crate) baseline_options: DesignOptions,
    pub(crate) baseline_excludes: Vec<DesignExclude>,
    pub(crate) baseline_rules: Vec<DesignRule>,
    pub(crate) baseline_triggers: Vec<DesignTrigger>,
    /// SQL 预览页的子页签：0 = 更改（增量脚本），1 = DDL（之前 / 之后对比）
    pub(crate) sql_sub: usize,
    /// 视图 / 实体化视图的「定义」正文（最后一页 SQL 预览由它生成）
    pub(crate) view_body: String,
    pub(crate) view_editor: Option<Entity<EditorState>>,
    /// 保存后的提示；一动模型就撤（与 Flutter 版 _note 同款）
    pub(crate) note: Option<&'static str>,
    /// None = 编辑已有表（名字随目录项走）；Some(name) = 新建表，
    /// name 空 = 未命名（页面上没有表名框，点「保存」时弹独立窗口补）。
    pub(crate) new_name: Option<String>,
}

/// 「…」浮层子编辑器：勾选顺序就是写回字符串里的顺序
#[derive(Clone)]
pub(crate) struct DesignPopup {
    pub(crate) target: PickTarget,
    /// 目标行在各自子页模型里的下标
    pub(crate) row: usize,
    pub(crate) items: Vec<(String, bool)>,
    /// 高亮行（↑↓ − 作用在它上面）
    pub(crate) sel: usize,
    /// 检查约束的「表达式」面板：多行输入框（参考工具是一个空文本域 + 确定/取消）
    pub(crate) expr: Option<Entity<TextareaState>>,
}

/// 最小校验：列名 / 索引 / 外键 / 唯一键 / 检查的必填与查重。返回 None 表示完整。
fn design_validation(
    cols: &[DesignCol],
    indexes: &[DesignIndex],
    fks: &[DesignForeignKey],
    uniques: &[DesignPair],
    checks: &[DesignPair],
) -> Option<String> {
    for (i, c) in cols.iter().enumerate() {
        if c.name.trim().is_empty() {
            return Some(format!("第 {} 行的列名不能为空", i + 1));
        }
    }
    for (i, c) in cols.iter().enumerate() {
        if let Some(j) = cols.iter().position(|o| o.name == c.name)
            && j != i
        {
            return Some(format!("列名「{}」重复", c.name));
        }
    }
    // 索引子页：名称/字段不能为空，名称不能重复；索引属性里的数字要合法。
    // 空白种子行（一行都没填）直接跳过：它只是等着输入的占位行。
    for (i, idx) in indexes.iter().enumerate() {
        if design_index_blank(idx) {
            continue;
        }
        if idx.name.trim().is_empty() {
            return Some(format!("第 {} 个索引的名称不能为空", i + 1));
        }
        if idx.fields.iter().all(|f| f.name.trim().is_empty()) {
            return Some(format!("索引「{}」的字段不能为空", idx.name.trim()));
        }
        for (label, value) in [
            ("填充因子", &idx.fillfactor),
            ("待处理列表限制", &idx.pending_list_limit),
            ("每范围页数", &idx.pages_per_range),
        ] {
            let v = value.trim();
            let valid = match label {
                "填充因子" => v.parse::<u32>().is_ok_and(|n| (0..100).contains(&n)),
                _ => v.parse::<u32>().is_ok_and(|n| n > 0),
            };
            if !v.is_empty() && !valid {
                let unit = if label == "填充因子" { "0..100 的整数" } else { "正整数" };
                return Some(format!("索引「{}」的{label}必须是 {unit}", idx.name.trim()));
            }
        }
    }
    for (i, idx) in indexes.iter().enumerate() {
        if design_index_blank(idx) {
            continue;
        }
        if let Some(j) = indexes.iter().position(|o| o.name.trim() == idx.name.trim())
            && j != i
        {
            return Some(format!("索引名「{}」重复", idx.name.trim()));
        }
    }
    // 外键子页：四个必填项 + 名称不能重复；整行空白的种子行跳过
    // （参考工具的空行常驻在表格里，不算待保存的数据）
    for (i, fk) in fks.iter().enumerate() {
        if design_fk_blank(fk) {
            continue;
        }
        if fk.name.trim().is_empty() {
            return Some(format!("第 {} 个外键的名称不能为空", i + 1));
        }
        if fk.columns.trim().is_empty() {
            return Some(format!("外键「{}」的字段不能为空", fk.name.trim()));
        }
        if fk.ref_table.trim().is_empty() {
            return Some(format!("外键「{}」的引用表不能为空", fk.name.trim()));
        }
        if fk.ref_columns.trim().is_empty() {
            return Some(format!("外键「{}」的引用字段不能为空", fk.name.trim()));
        }
    }
    for (i, fk) in fks.iter().enumerate() {
        if design_fk_blank(fk) {
            continue;
        }
        if let Some(j) = fks.iter().position(|o| o.name.trim() == fk.name.trim())
            && j != i
        {
            return Some(format!("外键名「{}」重复", fk.name.trim()));
        }
    }
    // 唯一键 / 检查：名称与内容都不能为空，名称不能重复（空白种子行跳过）
    for (label, list) in [("唯一键", uniques), ("检查", checks)] {
        for (i, p) in list.iter().enumerate() {
            if design_pair_blank(p) {
                continue;
            }
            if p.name.trim().is_empty() {
                return Some(format!("第 {} 个{label}的名称不能为空", i + 1));
            }
            if p.value.trim().is_empty() {
                return Some(format!("{label}「{}」的内容不能为空", p.name.trim()));
            }
        }
        for (i, p) in list.iter().enumerate() {
            if design_pair_blank(p) {
                continue;
            }
            if let Some(j) = list.iter().position(|o| o.name.trim() == p.name.trim())
                && j != i
            {
                return Some(format!("{label}名「{}」重复", p.name.trim()));
            }
        }
    }
    None
}

/// 选项子页校验：填充因子留空或 1..=100 的整数
fn design_options_validation(options: &DesignOptions) -> Option<String> {
    let ff = options.fillfactor.trim();
    if ff.is_empty() {
        return None;
    }
    match ff.parse::<u32>() {
        Ok(n) if (1..=100).contains(&n) => None,
        _ => Some("填充因子必须是 1–100 的整数".to_string()),
    }
}

/// 排除约束门控：仅 PostgreSQL（其它方言填了内容就挡保存）
fn design_excludes_gate(excludes: &[DesignExclude], kind: mock::DbKind) -> Option<String> {
    if excludes.iter().any(|e| !design_exclude_blank(e))
        && !matches!(kind, mock::DbKind::PostgreSql)
    {
        return Some("排除约束仅 PostgreSQL 支持".to_string());
    }
    None
}

/// 排除约束校验：元素必填、名称不能重复
fn design_excludes_validation(excludes: &[DesignExclude]) -> Option<String> {
    for (i, ex) in excludes.iter().enumerate() {
        if design_exclude_blank(ex) {
            continue;
        }
        if ex.elements.trim().is_empty() {
            return Some(format!("排除约束「{}」的元素不能为空", ex.name.trim()));
        }
        if ex.name.trim().is_empty() {
            return Some("排除约束的名称不能为空".to_string());
        }
    }
    for (i, ex) in excludes.iter().enumerate() {
        if design_exclude_blank(ex) {
            continue;
        }
        if let Some(j) = excludes.iter().position(|o| o.name.trim() == ex.name.trim())
            && j != i
        {
            return Some(format!("排除约束名「{}」重复", ex.name.trim()));
        }
    }
    None
}

/// 规则门控：仅 PostgreSQL
fn design_rules_gate(rules: &[DesignRule], kind: mock::DbKind) -> Option<String> {
    if rules.iter().any(|r| !design_rule_blank(r)) && !matches!(kind, mock::DbKind::PostgreSql) {
        return Some("规则仅 PostgreSQL 支持".to_string());
    }
    None
}

/// 规则校验：语句必填（DO NOTHING 除外）、名称不能重复
fn design_rules_validation(rules: &[DesignRule]) -> Option<String> {
    for rule in rules {
        if design_rule_blank(rule) {
            continue;
        }
        if rule.action.trim().to_uppercase() != "NOTHING"
            && rule.statement.trim().is_empty()
        {
            return Some(format!("规则「{}」的语句不能为空", rule.name.trim()));
        }
    }
    for (i, rule) in rules.iter().enumerate() {
        if design_rule_blank(rule) {
            continue;
        }
        if let Some(j) = rules.iter().position(|o| o.name.trim() == rule.name.trim())
            && j != i
        {
            return Some(format!("规则名「{}」重复", rule.name.trim()));
        }
    }
    None
}

/// 触发器门控：MongoDB / Redis 没有触发器；MySQL 不认语句级、SQL Server 不认 BEFORE
fn design_triggers_gate(
    triggers: &[DesignTrigger],
    kind: mock::DbKind,
) -> Option<String> {
    // 空白种子行不算「有触发器」
    let real: Vec<&DesignTrigger> = triggers
        .iter()
        .filter(|t| !design_trigger_blank(t))
        .collect();
    if real.is_empty() {
        return None;
    }
    match kind {
        mock::DbKind::MongoDb | mock::DbKind::Redis => {
            return Some("触发器仅 MySQL / PostgreSQL / SQL Server 支持".to_string());
        }
        mock::DbKind::MySql => {
            for t in &real {
                if t.for_each.trim().starts_with('语') {
                    return Some("MySQL 触发器只能按行触发（FOR EACH ROW）".to_string());
                }
            }
        }
        mock::DbKind::SqlServer => {
            for t in &real {
                if t.timing.trim().to_uppercase() == "BEFORE" {
                    return Some("SQL Server 触发器不支持 BEFORE（用 INSTEAD OF）".to_string());
                }
            }
        }
        _ => {}
    }
    None
}

/// 触发器校验：PG 要函数、MySQL / SQL Server 要主体
fn design_triggers_validation(
    triggers: &[DesignTrigger],
    kind: mock::DbKind,
) -> Option<String> {
    for t in triggers {
        if t.name.trim().is_empty() {
            continue;
        }
        match kind {
            mock::DbKind::PostgreSql => {
                if t.function.trim().is_empty() {
                    return Some(format!("触发器「{}」的函数不能为空", t.name.trim()));
                }
            }
            mock::DbKind::MySql | mock::DbKind::SqlServer => {
                if t.body.trim().is_empty() {
                    return Some(format!("触发器「{}」的主体不能为空", t.name.trim()));
                }
            }
            _ => {}
        }
    }
    None
}

/// 设计器「已修改」判定：任一子页的工作集与基线不一致就算改过
pub(crate) fn design_dirty(ds: &DesignerState) -> bool {
    ds.cols != ds.baseline
        || ds.indexes != ds.baseline_indexes
        || ds.fks != ds.baseline_fks
        || ds.uniques != ds.baseline_uniques
        || ds.checks != ds.baseline_checks
        || ds.options != ds.baseline_options
        || ds.excludes != ds.baseline_excludes
        || ds.rules != ds.baseline_rules
        || ds.triggers != ds.baseline_triggers
}

const DESIGN_PAGE_X: f32 = 257.;
const DESIGN_PAGE_Y: f32 = 135.;
/// 外键子页的模式候选（参考图下拉列出连接里的全部模式；PG 给系统模式 + public，
/// MySQL 没有模式层，给当前库 + 系统库）
fn fk_schema_candidates(kind: mock::DbKind, db: &str) -> Vec<String> {
    match kind {
        mock::DbKind::PostgreSql => ["information_schema", "pg_catalog", "pg_toast", "public"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        mock::DbKind::MySql => {
            let mut v = vec![db.to_string()];
            v.extend(
                ["information_schema", "mysql", "performance_schema", "sys"]
                    .iter()
                    .map(|s| s.to_string()),
            );
            v
        }
        _ => mock::schemas(kind).iter().map(|s| s.to_string()).collect(),
    }
}

/// 设计器浮层（勾选面板 / 普通下拉）锚到目标格正下方：left = 行首槽 + 前 n 列宽，
/// top = 工具条 + 子页签 + 白缝 + 表头 + (行号+1)*行高 + 分隔缝。返回 (left, top, 格宽)。
fn popup_anchor_for(ds: &DesignerState, sub: usize, col: usize, row: usize) -> (f32, f32, f32) {
    let cw = |ix: usize| {
        ds.widths
            .get(&(sub, ix))
            .copied()
            .unwrap_or(match sub {
                2 => D_FK_WIDTHS[ix],
                _ => D_UNIQUE_WIDTHS[ix],
            })
    };
    let left = D_GUTTER_W + (0..col).map(cw).sum::<f32>();
    let top = D_TOOLBAR_H
        + D_TABBAR_H
        + 6.
        + D_HEADER_H
        + (row + 1) as f32 * D_ROW_EXTENT
        + 3.;
    (left, top, cw(col))
}

/// 单个列的行内定义片段（不带前导缩进）：CREATE TABLE 的列行与
/// 「更改」脚本的 ADD COLUMN 共用同一份拼法。
pub(crate) fn design_col_sql(is_mysql: bool, c: &DesignCol) -> String {
    let q = |n: &str| if is_mysql { format!("`{n}`") } else { format!("\"{n}\"") };
    let mut seg = format!("{} {}", q(c.name.trim()), design_full_type(c));
    if c.not_null {
        seg.push_str(" NOT NULL");
    }
    if is_mysql && c.auto_inc {
        seg.push_str(" AUTO_INCREMENT");
    }
    if !is_mysql && !c.identity.is_empty() {
        seg.push_str(&format!(" GENERATED {} AS IDENTITY", c.identity));
    }
    if is_mysql && !c.default.is_empty() {
        seg.push_str(&format!(" DEFAULT {}", c.default));
    }
    if is_mysql && !c.comment.is_empty() {
        seg.push_str(&format!(" COMMENT '{}'", c.comment));
    }
    seg
}

/// 按方言生成 CREATE TABLE 预览（MySQL 反引号 + AUTO_INCREMENT + 行内 COMMENT，
/// 其余双引号 + 尾注 COMMENT ON；PG 虚拟类型出 GENERATED ... AS IDENTITY）。
/// 不带模式限定（历史口径，测试与旧调用点用）；界面上的预览走
/// [`design_create_sql_at`]。
#[allow(clippy::too_many_arguments)]
fn design_create_sql(
    kind: mock::DbKind,
    table: &str,
    cols: &[DesignCol],
    indexes: &[DesignIndex],
    fks: &[DesignForeignKey],
    uniques: &[DesignPair],
    checks: &[DesignPair],
    options: &DesignOptions,
    excludes: &[DesignExclude],
    rules: &[DesignRule],
) -> String {
    design_create_sql_at(kind, "", table, "", cols, indexes, fks, uniques, checks, options, excludes, rules)
}

/// [`design_create_sql`] 的带模式版：PG / SQL Server 出 `"public"."cameras"`
/// 形状的限定名，并在 CREATE TABLE 之后按参考文本补
/// `ALTER TABLE … OWNER TO "postgres";`（owner 空着就不出）。
#[allow(clippy::too_many_arguments)]
fn design_create_sql_at(
    kind: mock::DbKind,
    schema: &str,
    table: &str,
    owner: &str,
    cols: &[DesignCol],
    indexes: &[DesignIndex],
    fks: &[DesignForeignKey],
    uniques: &[DesignPair],
    checks: &[DesignPair],
    options: &DesignOptions,
    excludes: &[DesignExclude],
    rules: &[DesignRule],
) -> String {
    let is_mysql = matches!(kind, mock::DbKind::MySql);
    let is_pg = matches!(kind, mock::DbKind::PostgreSql);
    let is_sqlserver = matches!(kind, mock::DbKind::SqlServer);
    let q = |n: &str| if is_mysql { format!("`{n}`") } else { format!("\"{n}\"") };
    let qual = sql_preview::qualify(kind, schema, table);
    // 「不记录」：PG 的 UNLOGGED 表（创建语句头部）
    let unlogged = if is_pg && options.unlogged { "UNLOGGED " } else { "" };
    let mut lines = Vec::new();
    for c in cols {
        lines.push(format!("    {}", design_col_sql(is_mysql, c)));
    }
    let pks: Vec<String> = cols.iter().filter(|c| c.pk).map(|c| q(&c.name)).collect();
    if !pks.is_empty() {
        lines.push(format!("    PRIMARY KEY ({})", pks.join(", ")));
    }
    // 外键子页：表内 CONSTRAINT 子句，跟在 PRIMARY KEY 之后
    for fk in fks {
        if let Some(clause) = design_fk_sql(is_mysql, fk) {
            lines.push(format!("    {clause}"));
        }
    }
    // 唯一键 / 检查：同为表内约束，排在主键与外键之后
    for uk in uniques {
        if let Some(clause) = design_unique_sql(is_mysql, uk) {
            lines.push(format!("    {clause}"));
        }
    }
    for ck in checks {
        if let Some(clause) = design_check_sql(is_pg, ck) {
            lines.push(format!("    {clause}"));
        }
    }
    // 排除约束：仅 PostgreSQL 生成（其它方言由校验挡住保存）
    if is_pg {
        for ex in excludes {
            if let Some(clause) = design_exclude_sql(ex) {
                lines.push(format!("    {clause}"));
            }
        }
    }
    // 表级选项：MySQL 用 COMMENT=；PG 用 WITH (fillfactor) / TABLESPACE；
    // SQL Server 认 WITH (FILLFACTOR)（注释另走扩展属性，这里不生成）。
    let mut tail = String::new();
    if is_mysql {
        if !options.comment.trim().is_empty() {
            tail.push_str(&format!(" COMMENT='{}'", sql_literal(options.comment.trim())));
        }
    } else if is_pg {
        let ff = options.fillfactor.trim();
        if !ff.is_empty() {
            tail.push_str(&format!(" WITH (fillfactor = {ff})"));
        }
        let ts = options.tablespace.trim();
        if !ts.is_empty() {
            tail.push_str(&format!(" TABLESPACE {ts}"));
        }
    } else if is_sqlserver {
        let ff = options.fillfactor.trim();
        if !ff.is_empty() {
            tail.push_str(&format!(" WITH (FILLFACTOR = {ff})"));
        }
    }
    let mut sql = format!("CREATE TABLE {qual} (\n{}\n){};", lines.join(",\n"), tail);
    // 参考文本（cameras 屏）的形状：CREATE TABLE 之后紧跟 ALTER TABLE … OWNER
    if is_pg && !owner.is_empty() {
        sql.push_str(&format!("\nALTER TABLE {qual}\n  OWNER TO \"{owner}\";"));
    }
    if is_pg && !options.comment.trim().is_empty() {
        sql.push_str(&format!(
            "\nCOMMENT ON TABLE {qual} IS '{}';",
            sql_literal(options.comment.trim())
        ));
    }
    if !is_mysql && is_pg {
        for c in cols.iter().filter(|c| !c.comment.is_empty()) {
            sql.push_str(&format!("\nCOMMENT ON COLUMN {qual}.{} IS '{}';", q(&c.name), c.comment));
        }
    }
    // 索引子页：CREATE TABLE 之后按顺序补 CREATE [UNIQUE] INDEX
    for idx in indexes {
        if let Some(stmt) = design_index_sql_at(is_mysql, schema, table, idx, cols) {
            sql.push('\n');
            sql.push_str(&stmt);
        }
    }
    // 外键注释：PG 用 COMMENT ON CONSTRAINT（MySQL / SQL Server 没有对应语法）
    if is_pg {
        for fk in fks.iter().filter(|f| !f.comment.trim().is_empty()) {
            sql.push_str(&format!(
                "\nCOMMENT ON CONSTRAINT {} ON {qual} IS '{}';",
                q(fk.name.trim()),
                sql_literal(fk.comment.trim())
            ));
        }
    }
    // 唯一键注释：同上
    if is_pg {
        for uk in uniques.iter().filter(|u| !u.comment.trim().is_empty()) {
            sql.push_str(&format!(
                "\nCOMMENT ON CONSTRAINT {} ON {qual} IS '{}';",
                q(uk.name.trim()),
                sql_literal(uk.comment.trim())
            ));
        }
    }
    // 检查注释：同上
    if is_pg {
        for ck in checks.iter().filter(|c| !c.comment.trim().is_empty()) {
            sql.push_str(&format!(
                "\nCOMMENT ON CONSTRAINT {} ON {qual} IS '{}';",
                q(ck.name.trim()),
                sql_literal(ck.comment.trim())
            ));
        }
    }
    // 「集群」：CLUSTER "table" USING "index"（PG；mock 档直接可执行）
    let cluster_idx = options.cluster.trim();
    if is_pg && !cluster_idx.is_empty() {
        sql.push_str(&format!(
            "\nCLUSTER {} USING {};",
            q(table),
            q(cluster_idx)
        ));
    }
    // 排除约束注释：同上
    if is_pg {
        for ex in excludes.iter().filter(|e| !e.comment.trim().is_empty()) {
            sql.push_str(&format!(
                "\nCOMMENT ON CONSTRAINT {} ON {qual} IS '{}';",
                q(ex.name.trim()),
                sql_literal(ex.comment.trim())
            ));
        }
    }
    // 规则注释：PG 用 COMMENT ON RULE
    if is_pg {
        for rule in rules.iter().filter(|r| !r.comment.trim().is_empty()) {
            sql.push_str(&format!(
                "\nCOMMENT ON RULE {} ON {qual} IS '{}';",
                q(rule.name.trim()),
                sql_literal(rule.comment.trim())
            ));
        }
    }
    // 规则子页：PG 专属，作为独立语句排在最后
    if is_pg {
        for rule in rules {
            if let Some(stmt) = design_rule_sql(rule) {
                sql.push('\n');
                sql.push_str(&stmt);
            }
        }
    }
    sql
}

/// 返回指定数据库类型的常用列类型候选列表（设计器「类型」下拉用）
fn design_type_candidates(kind: mock::DbKind) -> &'static [&'static str] {
    match kind {
        mock::DbKind::MySql => &[
            "int", "bigint", "smallint", "tinyint", "mediumint",
            "decimal", "float", "double",
            "varchar", "char", "text", "tinytext", "mediumtext", "longtext",
            "blob", "tinyblob", "mediumblob", "longblob",
            "date", "time", "datetime", "timestamp", "year",
            "bool", "boolean",
            "json", "binary", "varbinary",
        ],
        mock::DbKind::PostgreSql => &[
            "integer", "bigint", "smallint", "serial", "bigserial",
            "numeric", "decimal", "real", "double precision",
            "varchar", "character varying", "char", "character", "text",
            "bytea",
            "date", "time", "timestamp", "timestamptz", "interval",
            "boolean", "bool",
            "json", "jsonb", "uuid", "inet", "cidr", "macaddr",
            "int4range", "int8range", "numrange", "tsrange", "tstzrange", "daterange",
        ],
        mock::DbKind::SqlServer => &[
            "int", "bigint", "smallint", "tinyint",
            "decimal", "numeric", "float", "real", "money", "smallmoney",
            "varchar", "char", "text", "nvarchar", "nchar", "ntext",
            "binary", "varbinary", "image",
            "date", "time", "datetime", "datetime2", "smalldatetime", "datetimeoffset",
            "bit", "bool",
            "xml", "uniqueidentifier", "sql_variant",
        ],
        mock::DbKind::MongoDb => &["string", "int", "long", "double", "decimal", "bool", "date", "object", "array", "binary", "objectId"],
        mock::DbKind::Redis => &["string", "list", "set", "zset", "hash", "stream"],
    }
}

/// 按顶层分号把 SQL 脚本拆成语句（查询页逐条出结果，同 Flutter 版
/// `splitSqlStatements`）。词法感知：单引号 / 双引号 / 反引号 / [方括号]
/// （双写转义）、`--` 行注释、`/* */` 块注释（PG 嵌套）、`$$` / `$tag$`
/// 美元引号——这些结构内部的分号不作为语句分隔符。
/// 末尾无结束符的剩余文本也作为最后一条语句返回。
/// 已知局限：DELIMITER / GO 指令行未做（mock 档跑不了存储过程）。
fn split_sql_statements(script: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut block_depth = 0usize;
    // 引号结构未闭合时的闭合符（`[` 记 `]`）
    let mut quote_close: Option<char> = None;
    // 美元引号未闭合时的完整定界符（含首尾 $）
    let mut dollar_tag: Option<String> = None;
    let mut i = 0usize;
    while i < script.len() {
        if block_depth > 0 {
            if script[i..].starts_with("/*") {
                block_depth += 1;
                buf.push_str("/*");
                i += 2;
            } else if script[i..].starts_with("*/") {
                block_depth -= 1;
                buf.push_str("*/");
                i += 2;
            } else {
                let ch = script[i..].chars().next().unwrap();
                buf.push(ch);
                i += ch.len_utf8();
            }
            continue;
        }
        if let Some(tag) = dollar_tag.clone() {
            if script[i..].starts_with(&tag) {
                buf.push_str(&tag);
                i += tag.len();
                dollar_tag = None;
            } else {
                let ch = script[i..].chars().next().unwrap();
                buf.push(ch);
                i += ch.len_utf8();
            }
            continue;
        }
        if let Some(close) = quote_close {
            let ch = script[i..].chars().next().unwrap();
            buf.push(ch);
            i += ch.len_utf8();
            if ch == close {
                if script[i..].starts_with(close) {
                    buf.push(close);
                    i += 1;
                } else {
                    quote_close = None;
                }
            }
            continue;
        }
        if script[i..].starts_with(';') {
            let s = buf.trim();
            if !s.is_empty() {
                out.push(s.to_string());
            }
            buf.clear();
            i += 1;
            continue;
        }
        if script[i..].starts_with("--") {
            // 行注释整段原样保留（含在 buf 里），直到换行——注释里的分号不拆
            let rest = &script[i..];
            let end = rest.find('\n').map(|p| i + p).unwrap_or(script.len());
            buf.push_str(&script[i..end]);
            i = end;
            continue;
        }
        if script[i..].starts_with("/*") {
            block_depth = 1;
            buf.push_str("/*");
            i += 2;
            continue;
        }
        if script[i..].starts_with('$')
            && let Some(tag) = dollar_quote_tag(&script[i..])
        {
            dollar_tag = Some(tag.clone());
            buf.push_str(&tag);
            i += tag.len();
            continue;
        }
        let ch = script[i..].chars().next().unwrap();
        if matches!(ch, '\'' | '"' | '`' | '[') {
            quote_close = Some(if ch == '[' { ']' } else { ch });
        }
        buf.push(ch);
        i += ch.len_utf8();
    }
    let s = buf.trim();
    if !s.is_empty() {
        out.push(s.to_string());
    }
    out
}

/// `$$` / `$tag$` 的定界符（标签为标识符、不能数字开头，`$1` 占位符不误判）。
/// 不带收尾 `$` 的返回 None。
fn dollar_quote_tag(s: &str) -> Option<String> {
    let mut tag = String::from("$");
    for c in s.chars().skip(1) {
        if c == '$' {
            tag.push('$');
            return Some(tag);
        }
        if c.is_ascii_alphabetic() || c == '_' {
            tag.push(c);
        } else if c.is_ascii_digit() && tag.len() > 1 {
            tag.push(c);
        } else {
            return None;
        }
    }
    None
}

/// mock 档判断语句是否会「出结果网格」：SELECT / WITH / SHOW 开头。
/// 其余语句 mock 不执行写操作，结果页签留白、消息页说明。
fn query_is_select(stmt: &str) -> bool {
    let head: String = stmt
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    matches!(head.as_str(), "SELECT" | "WITH" | "SHOW")
}

/// 一条 SELECT 语句的假结果：行数与内容按语句文本散列（同表数据页口径——
/// 同一条语句恒定、不同语句有差异），列形共用 MOCK_COLUMNS。
fn mock_query_rows(stmt: &str) -> Vec<Vec<String>> {
    let rows = mock::mock_row_count(stmt);
    (0..rows)
        .map(|r| {
            (0..mock::MOCK_COLUMNS.len())
                .map(|c| mock::mock_cell(stmt, r, c))
                .collect()
        })
        .collect()
}

/// 网格排序的单格比较：数值列先按数值比（字符串比较会把 "999" 排到
/// "1002" 后面），解析不出再退回字符串比。
fn compare_cells(a: &str, b: &str, numeric: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if numeric
        && let (Ok(x), Ok(y)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>())
    {
        return x.partial_cmp(&y).unwrap_or(Ordering::Equal);
    }
    a.cmp(b)
}

/// 数据页签打开时把 mock 取数物化成真实二维表（[OpenTab] 的 data/baseline），
/// 让网格编辑 / 增删改的是页签里的副本；确定性的 mock_cell 保证「刷新 /
/// 撤销」重取回来的值与基线一致。
fn materialize_rows(obj: &str) -> Vec<Vec<String>> {
    let rows = mock::mock_row_count(obj);
    (0..rows)
        .map(|r| {
            (0..mock::MOCK_COLUMNS.len())
                .map(|c| mock::mock_cell(obj, r, c).to_string())
                .collect()
        })
        .collect()
}

/// 排序后的显示序位 → 原行下标映射；无排序即恒等。稳定排序：同值行保持
/// 原相对次序（Rust sort_by 稳定）。列下标越界（防御）也按恒等处理。
fn sorted_row_order(data: &[Vec<String>], sort: Option<(usize, bool)>) -> Vec<usize> {
    let Some((col, asc)) = sort else {
        return (0..data.len()).collect();
    };
    let numeric = mock::MOCK_COLUMNS.get(col).is_some_and(|c| c.numeric);
    let cell = |r: usize| {
        data.get(r)
            .and_then(|row| row.get(col))
            .map(|s| s.as_str())
            .unwrap_or("")
    };
    let mut order: Vec<usize> = (0..data.len()).collect();
    order.sort_by(|&a, &b| {
        let ord = compare_cells(cell(a), cell(b), numeric);
        if asc { ord } else { ord.reverse() }
    });
    order
}

/// 数据网格的实际显示序（data 行下标）。筛选面板「应用」过的排序优先
/// （多键），否则回退表头单键排序；应用过的 WHERE 把不过滤条件的行剔除。
/// 新增行不参与（调用方恒把它们排在显示序之后）。
fn visible_data_order(t: &OpenTab) -> Vec<usize> {
    let applied = t.filter.applied.as_ref();
    let mut base = match applied {
        Some(a) if !a.sorts.is_empty() => {
            let picks: Vec<usize> = (0..t.data.len()).collect();
            filter::multi_sort_order(&t.data, &picks, &a.sorts)
        }
        _ => sorted_row_order(&t.data, t.sort),
    };
    if let Some(a) = applied {
        base.retain(|&r| {
            t.data
                .get(r)
                .is_some_and(|row| filter::row_matches(row, &a.model))
        });
    }
    base
}

/// 按路径在模型里定位节点：顶层下标 → 组内下标（仅支持两层）。
fn filter_node_at<'a>(m: &'a filter::FilterModel, path: &[usize]) -> Option<&'a filter::FNode> {
    match path {
        [top] => m.items.get(*top),
        [top, child] => match m.items.get(*top)? {
            filter::FNode::Group { children, .. } => children.get(*child),
            _ => None,
        },
        _ => None,
    }
}

fn filter_node_at_mut<'a>(
    m: &'a mut filter::FilterModel,
    path: &[usize],
) -> Option<&'a mut filter::FNode> {
    match path {
        [top] => m.items.get_mut(*top),
        [top, child] => match m.items.get_mut(*top)? {
            filter::FNode::Group { children, .. } => children.get_mut(*child),
            _ => None,
        },
        _ => None,
    }
}

/// 一条语句的 mock 执行结果。结果面板每条语句一个页签（同 pgAdmin /
/// Flutter 版）：SELECT 出网格，写操作留白、消息页说明。
#[derive(Clone)]
struct QueryOutcome {
    /// 语句原文（消息页回显用）
    sql: String,
    /// SELECT 的假结果行；None = 写操作（mock 不执行）
    result: Option<Vec<Vec<String>>>,
    elapsed_ms: u128,
    /// 网格里当前格（行 × 列）
    cell: (usize, usize),
}

/// 查询页签工作状态，随页签存亡（放 [OpenTab] 里，关页签即弃）。
#[derive(Clone)]
struct QueryState {
    /// SQL 编辑器（代码模式：行号 + 等宽，mock 档无语法高亮）
    editor: Entity<EditorState>,
    /// 消息页的只读日志（可拖选复制，界面必须说实话）；每次运行整份重写
    log: Entity<TextareaState>,
    /// 运行上下文：连接名 / 库下标 / 模式下标（无模式引擎恒 0 且不显示下拉）
    conn: String,
    db: usize,
    schema: usize,
    /// 页签序号：标题「查询-N」，全局递增不复用
    seq: usize,
    /// 编辑过未保存 → 标题前「*」（Navicat 习惯）
    dirty: bool,
    outcomes: Vec<QueryOutcome>,
    /// 结果面板活动页签：0 = 消息，i = 结果 i
    pane: usize,
    /// 最近一次运行的总耗时（状态文案用）
    last_ms: u128,
    /// 运行前的提示（如「请输入 SQL 语句」），出结果后撤
    prompt: Option<&'static str>,
    /// 结果面板高度（可拖动调整，默认 Q_PANEL_H）
    panel_h: f32,
}

/// 数据页当前格的就地编辑器（双击进入）。tab 用 conn/db/schema/cat/obj
/// 五元组定位所属页签（与 open_object 的去重键一致）：提交时页签可能已
/// 不是激活页（中途关页 / 切页会让下标漂移），按下标写会写错页签。
struct CellEdit {
    tab: (String, &'static str, usize, usize, usize),
    /// 显示序位 × 列（与 OpenTab.cell 同口径，排序态下依然成立）
    at: (usize, usize),
    input: Entity<InputState>,
}

struct DaroApp {
    dark: bool,
    /// 树上当前选中的节点。对象面板、右侧信息卡、状态栏都由它推导。
    sel: Act,
    /// 分组是运行时可增的，所以按名字记折叠态（索引会随新建组漂移）
    groups: Vec<String>,
    collapsed_groups: HashSet<String>,
    /// 「我的连接」根节点可以整棵收起来
    collapsed_root: bool,
    /// 星标与标签色按连接名记：删连接会让索引整体前移
    starred: HashSet<String>,
    /// 展开态同样按名字记。Navicat 里「连着」和「展开着」是两件事：
    /// 选中另一条连接不会把已打开的连接收起来。
    expanded: HashSet<String>,
    /// 打开过的库：Navicat 用绿色圆柱标出，没打开过的是灰色
    visited_dbs: HashSet<String>,
    /// 上一次点中的行与时刻，用来自己判双击（见 DOUBLE_CLICK_INTERVAL）
    last_click: Option<(Act, Instant)>,
    conn_colors: HashMap<String, u8>,
    /// 右键命中的节点。树只挂一个右键菜单实体，靠它决定菜单内容，
    /// 否则逐行各挂一份时右击另一行会让两个菜单同时留在屏幕上。
    ctx: Option<Act>,
    list_view: bool,
    show_info: bool,
    conns: Vec<mock::Connection>,
    /// 打开的数据表页签，见 [OpenTab]；`active_tab` 0 号是「对象」页
    opened: Vec<OpenTab>,
    active_tab: usize,
    /// 主工作区的多窗格分栏（页签组 + 分栏树）；`active_tab` 永远等于
    /// 聚焦窗格的当前页，见 [dock::Dock::focused_active]。
    dock: dock::Dock,
    /// 正在拖页签时的落点提示（目标窗格 id + 五区之一）。
    drag_hover: Option<(u64, dock::DropZone)>,
    /// 正在内联改名的节点（连接 / 分组）。改名编辑器就渲染在该行的标签位上。
    renaming: Option<Act>,
    rename_input: Option<Entity<InputState>>,
    /// 树底部的搜索框（InputState 要 window，在 new 拿到 window 时建）。
    search_input: Option<Entity<InputState>>,
    /// 搜索词的实时镜像（InputEvent::Change 时更新），空串 = 不过滤。
    query: String,
    /// 数据页底部页码框的就地编辑；None = 页码是纯显示。
    page_editing: Option<Entity<InputState>>,
    /// 数据页当前格的就地编辑器（双击进入）；None = 没在编辑。
    cell_editing: Option<CellEdit>,
    /// 上一次点中的格与时刻，用来自己判双击（见 DOUBLE_CLICK_INTERVAL）
    last_cell_click: Option<((usize, usize), Instant)>,
    /// 设计页签的序号源；0 号留给数据页签
    next_did: u64,
    /// 查询页签的 id 与「查询-N」序号源（都全局递增不复用）
    next_qid: u64,
    next_qseq: usize,
}

impl DaroApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = load_store();
        let dark = store.as_ref().is_some_and(|s| s.dark);
        Theme::change(
            if dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            None,
            cx,
        );
        Self::apply_palette(dark, cx);

        let (groups, collapsed_groups, collapsed_root, starred, expanded, visited_dbs, conn_colors, list_view, show_info, conns) =
            match store {
                Some(s) => {
                    // Navicat 习惯：重启后连接一律是关闭态（要手动打开），
                    // 但展开键保留——打开连接时树会展开回上次的位置
                    let conns = s
                        .connections
                        .into_iter()
                        .map(|mut c| {
                            c.connected = false;
                            c
                        })
                        .collect::<Vec<_>>();
                    // 连接上挂的分组若不在分组表里（旧版本数据），现场补齐
                    let mut groups = s.groups;
                    for g in conns.iter().map(|c| c.group.clone()).filter(|g| !g.is_empty()) {
                        if !groups.contains(&g) {
                            groups.push(g);
                        }
                    }
                    (
                        groups,
                        s.collapsed_groups.into_iter().collect(),
                        s.collapsed_root,
                        s.starred.into_iter().collect(),
                        s.expanded.into_iter().collect(),
                        s.visited_dbs.into_iter().collect(),
                        s.conn_colors,
                        s.list_view,
                        s.show_info,
                        conns,
                    )
                }
                None => {
                    let conns = connections();
                    // 默认选中第一条连接的初始库第一张表，并把这条路径展开出来：
                    // Navicat 启动时就是「上次会话的选中项可见」的样子。
                    let mut expanded = HashSet::new();
                    let mut visited_dbs = HashSet::new();
                    if let Some(conn) = conns.first() {
                        expanded.insert(conn.name.clone());
                        let db = mock::databases(conn.kind)[0];
                        let db_key = node_key(&conn.name, db);
                        expanded.insert(db_key.clone());
                        visited_dbs.insert(db_key.clone());
                        if mock::has_schemas(conn.kind) {
                            expanded.insert(node_key(&db_key, mock::schemas(conn.kind)[0]));
                        }
                        expanded.insert(node_key(&db_key, CATEGORIES[0].0));
                    }
                    (
                        GROUPS.iter().map(|g| g.to_string()).collect(),
                        HashSet::new(),
                        false,
                        HashSet::new(),
                        expanded,
                        visited_dbs,
                        HashMap::new(),
                        true,
                        true,
                        conns,
                    )
                }
            };
        // 树底部的搜索框（Navicat 的「搜索」框）。输入实时过滤树，
        // Change 里把文本镜像进 self.query 驱动重渲染。
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索"));
        cx.subscribe(&search_input, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                let value = this
                    .search_input
                    .as_ref()
                    .map(|e| e.read(cx).value().to_string())
                    .unwrap_or_default();
                if this.query != value {
                    this.query = value;
                    cx.notify();
                }
            }
        })
        .detach();
        Self {
            dark,
            sel: Act::Obj(0, 0, NO_SCHEMA, 0, 0),
            groups,
            collapsed_groups,
            collapsed_root,
            starred,
            expanded,
            visited_dbs,
            last_click: None,
            conn_colors,
            ctx: None,
            list_view,
            show_info,
            conns,
            opened: Vec::new(),
            active_tab: 0,
            dock: dock::Dock::new(),
            drag_hover: None,
            renaming: None,
            rename_input: None,
            search_input: Some(search_input),
            query: String::new(),
            page_editing: None,
            cell_editing: None,
            last_cell_click: None,
            next_did: 1,
            next_qid: 1,
            next_qseq: 1,
        }
    }

    /// 任何会重启还原的状态变更后落盘。文件很小（几 KB），同步写足够；
    /// 真觉得慢再换 dirty 标记 + 延迟写。
    fn save(&self) {
        let store = Store {
            version: 1,
            dark: self.dark,
            list_view: self.list_view,
            show_info: self.show_info,
            collapsed_root: self.collapsed_root,
            groups: self.groups.clone(),
            collapsed_groups: self.collapsed_groups.iter().cloned().collect(),
            starred: self.starred.iter().cloned().collect(),
            conn_colors: self.conn_colors.clone(),
            expanded: self.expanded.iter().cloned().collect(),
            visited_dbs: self.visited_dbs.iter().cloned().collect(),
            connections: self.conns.clone(),
        };
        let path = store_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(&store) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    eprintln!("[daro] 连接状态写盘失败：{e}");
                }
            }
            Err(e) => eprintln!("[daro] 连接状态序列化失败：{e}"),
        }
    }

    /// 把 daro 的语义色板盖到组件库主题上。组件库的 accent 是 hover 底色、
    /// primary 才是强调色，与 Palette 里的 accent 不是同一个概念。
    ///
    /// 走 [Theme::update] 而不是直接改 [Theme::global_mut]：后者不会把新色值
    /// 推给 base 层（滚动条、拖拽手柄）也不会刷新窗口。
    fn apply_palette(dark: bool, cx: &mut Context<Self>) {
        // SQL 高亮的明暗是固定中调色，靠这个全局标志切换（编辑器下一帧取色）
        sql_highlight::set_dark(dark);
        let p = Palette::of(dark);
        Theme::update(cx, |theme| {
            let colors = &mut theme.colors;
            colors.background = to_hsla(p.background);
            colors.foreground = to_hsla(p.foreground);
            colors.border = to_hsla(p.border);
            colors.muted = to_hsla(p.chrome);
            colors.muted_foreground = to_hsla(p.muted_foreground);
            colors.secondary = to_hsla(p.chrome);
            colors.secondary_foreground = to_hsla(p.foreground);
            colors.accent = to_hsla(p.hover);
            colors.accent_foreground = to_hsla(p.foreground);
            colors.primary = to_hsla(p.accent);
            colors.primary_foreground = to_hsla(0xffffff);
            colors.ring = to_hsla(p.accent);
            colors.selection = to_hsla(p.selection);
            colors.popover = to_hsla(p.background);
            colors.popover_foreground = to_hsla(p.foreground);
            colors.title_bar = to_hsla(p.chrome);
            colors.title_bar_border = to_hsla(p.border);
            colors.sidebar = to_hsla(p.chrome);
        });
    }

    fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        if std::env::var("DARO_DEBUG").is_ok() {
            eprintln!("[dbg] toggle_theme -> {}", !self.dark);
        }
        self.dark = !self.dark;
        Theme::change(
            if self.dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            None,
            cx,
        );
        Self::apply_palette(self.dark, cx);
        self.save();
        cx.notify();
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn active_conn(&self) -> &mock::Connection {
        self.sel
            .conn()
            .and_then(|ix| self.conns.get(ix))
            .unwrap_or(&EMPTY_CONN)
    }

    /// 选中节点落在哪条连接上；分组节点没有连接归属。
    fn sel_conn_ix(&self) -> Option<usize> {
        self.sel.conn().filter(|ix| *ix < self.conns.len())
    }

    /// 选中节点的库名。没选中库层以下时返回 None（Navicat 此时对象区是空的）。
    fn sel_db_name(&self) -> Option<&'static str> {
        let (c, d) = (self.sel_conn_ix()?, self.sel.db()?);
        mock::databases(self.conns[c].kind).get(d).copied()
    }

    fn sel_schema_name(&self) -> Option<&'static str> {
        let (c, s) = (self.sel_conn_ix()?, self.sel.schema()?);
        mock::schemas(self.conns[c].kind).get(s).copied()
    }

    /// 对象面板该显示哪一分类；只有选中库（默认表分类）、分类或对象时才有内容。
    fn pane_cat(&self) -> Option<usize> {
        match self.sel {
            Act::Db(_, _) => Some(0),
            Act::Schema(_, _, _) => Some(0),
            Act::Cat(_, _, _, c) | Act::Obj(_, _, _, c, _) | Act::Col(_, _, _, c, _, _) => Some(c),
            _ => None,
        }
    }

    fn pane_obj(&self) -> Option<usize> {
        self.sel.obj()
    }

    /// 对象面板当前挂在哪条路径上：(连接, 库, 模式, 分类)。
    /// 面板里点中的对象要回填成同样的路径，树和面板才认得是同一个节点。
    fn pane_path(&self) -> (usize, usize, usize, usize) {
        let Some(c) = self.sel_conn_ix() else {
            return (0, 0, NO_SCHEMA, 0);
        };
        let (_, d) = self.anchor_db().unwrap_or((c, 0));
        let has_schema = mock::has_schemas(self.conns[c].kind);
        let s = match self.sel.schema() {
            Some(s) => s,
            None if has_schema => 0,
            None => NO_SCHEMA,
        };
        (c, d, s, self.pane_cat().unwrap_or(0))
    }

    /// 面板里当前分类的对象清单；没选中库层以下就是空（Navicat 就是这样）
    fn pane_objects(&self) -> &'static [&'static str] {
        let (Some(conn), Some(db), Some(cat)) = (
            self.sel_conn_ix().map(|ix| &self.conns[ix]),
            self.sel_db_name(),
            self.pane_cat(),
        ) else {
            return &[];
        };
        mock::objects(conn, db, cat)
    }

    pub(crate) fn push_connection(&mut self, conn: mock::Connection) {
        self.conns.push(conn);
        self.sel = Act::Conn(self.conns.len() - 1);
        self.save();
    }

    /// 编辑连接保存：改名时把星标 / 标签色一并迁到新名字上（名称即标识）
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_connection_edit(
        &mut self,
        ix: usize,
        name: String,
        kind: DbKind,
        host: String,
        port: u16,
        database: String,
        user: String,
        password: String,
    ) {
        let Some(conn) = self.conns.get_mut(ix) else {
            return;
        };
        let old_name = std::mem::replace(&mut conn.name, name);
        let old_db = std::mem::replace(&mut conn.database, database);
        conn.kind = kind;
        conn.host = host;
        conn.port = port;
        conn.user = user;
        conn.password = password;
        conn.connected = false;
        // 编辑完连接是断开的，旧展开态直接丢掉，重连时按 Navicat 重新展开
        self.expanded.remove(&old_name);
        self.expanded.remove(&node_key(&old_name, &old_db));
        if old_name != conn.name {
            let new_name = conn.name.clone();
            if self.starred.remove(&old_name) {
                self.starred.insert(new_name.clone());
            }
            if let Some(color) = self.conn_colors.remove(&old_name) {
                self.conn_colors.insert(new_name, color);
            }
        }
        self.save();
    }

    /// F2（或之后挂上的入口）就地改名：选中节点是连接 / 分组时，
    /// 在该行的标签位上挂一个编辑器，全文选中，Enter 提交 / Esc 取消。
    fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::env::var("DARO_DEBUG").is_ok() {
            eprintln!("[dbg] start_rename sel={:?} renaming={:?}", self.sel, self.renaming.is_some());
        }
        if self.renaming.is_some() {
            return;
        }
        // 焦点在树搜索框里时 F2 是普通按键，不该把选中节点拉进改名
        if self
            .search_input
            .as_ref()
            .is_some_and(|e| e.read(cx).focus_handle(cx).is_focused(window))
        {
            return;
        }
        let name = match self.sel {
            Act::Conn(ix) => self.conns.get(ix).map(|c| c.name.clone()),
            Act::Group(gix) => self.groups.get(gix).cloned(),
            _ => None,
        };
        let Some(name) = name else {
            return;
        };
        let state = cx.new(|cx| InputState::new(window, cx).placeholder(String::new()));
        state.update(cx, |s, cx| {
            s.set_value(name, window, cx);
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        // 点别处（失焦）也按提交算，Windows 树内联改名的惯例
        cx.subscribe(&state, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit_rename(cx),
            _ => {}
        })
        .detach();
        self.renaming = Some(self.sel);
        self.rename_input = Some(state);
        cx.notify();
    }

    /// 该行是否正在改名；是则把编辑器实体交给 tree_row 渲染在标签位上。
    fn rename_editor(&self, act: Act) -> Option<Entity<InputState>> {
        match self.renaming {
            Some(a) if a == act => self.rename_input.clone(),
            _ => None,
        }
    }

    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        if std::env::var("DARO_DEBUG").is_ok() {
            eprintln!("[dbg] commit_rename had={:?}", self.renaming.is_some());
        }
        let Some(act) = self.renaming.take() else {
            return;
        };
        let input = self.rename_input.take();
        let new = input
            .as_ref()
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default();
        match act {
            Act::Conn(ix) => self.rename_connection(ix, new),
            Act::Group(gix) => self.rename_group(gix, new),
            _ => {}
        }
        cx.notify();
    }

    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if std::env::var("DARO_DEBUG").is_ok() {
            eprintln!("[dbg] cancel_rename had={:?}", self.renaming.is_some());
        }
        if self.renaming.take().is_some() {
            self.rename_input = None;
            cx.notify();
        }
    }

    /// 连接改名 = 名称即标识：星标 / 标签色 / 展开键 / 已访问库 / 打开的页签
    /// 全部跟着迁到新名字上（与编辑连接对话框里的改名同口径）。
    fn rename_connection(&mut self, ix: usize, new: String) {
        let Some(conn) = self.conns.get(ix) else {
            return;
        };
        let old = conn.name.clone();
        if new.is_empty() || new == old || self.conns.iter().any(|c| c.name == new) {
            return;
        }
        self.conns[ix].name = new.clone();
        let remap = |set: &mut HashSet<String>| {
            let prefix = format!("{old}\u{1}");
            *set = set
                .iter()
                .map(|k| {
                    if k == &old {
                        new.clone()
                    } else if let Some(rest) = k.strip_prefix(&prefix) {
                        format!("{new}\u{1}{rest}")
                    } else {
                        k.clone()
                    }
                })
                .collect();
        };
        remap(&mut self.expanded);
        remap(&mut self.visited_dbs);
        if self.starred.remove(&old) {
            self.starred.insert(new.clone());
        }
        if let Some(color) = self.conn_colors.remove(&old) {
            self.conn_colors.insert(new.clone(), color);
        }
        for t in &mut self.opened {
            if t.conn_name == old {
                t.conn_name = new.clone();
            }
        }
        self.save();
    }

    /// 分组改名：组表、组内连接的归属、折叠态一起迁。不碰连接名，
    /// 所以不动展开键，也不触发标签迁移。
    fn rename_group(&mut self, gix: usize, new: String) {
        let Some(old) = self.groups.get(gix).cloned() else {
            return;
        };
        if new.is_empty() || new == old || self.groups.contains(&new) {
            return;
        }
        self.groups[gix] = new.clone();
        for c in &mut self.conns {
            if c.group == old {
                c.group = new.clone();
            }
        }
        if self.collapsed_groups.remove(&old) {
            self.collapsed_groups.insert(new);
        }
        self.save();
    }

    fn toggle_key(&mut self, key: String) {
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        self.save();
    }

    /// 断开 / 删连接时把整棵子树的展开态一起清掉。键都带连接名前缀，按前缀扫。
    fn drop_tree_state(&mut self, conn_name: &str) {
        let prefix = format!("{conn_name}\u{1}");
        self.expanded.retain(|k| !k.starts_with(&prefix));
        self.expanded.remove(conn_name);
        self.visited_dbs.retain(|k| !k.starts_with(&prefix));
    }

    fn close_connection(&mut self, ix: usize) {
        if let Some(conn) = self.conns.get_mut(ix) {
            conn.connected = false;
            let name = conn.name.clone();
            // Navicat 断开时子树整棵收掉，重连后要重新展开
            self.drop_tree_state(&name);
            self.save();
        }
    }

    fn open_connection(&mut self, ix: usize) {
        if let Some(conn) = self.conns.get_mut(ix) {
            conn.connected = true;
            let name = conn.name.clone();
            // 实测 Navicat：连接成功后只展开到数据库那一层，库不跟着展开
            self.expanded.insert(name);
            self.save();
        }
    }

    /// 三角与双击都走这里：未连接时先连接（连接即展开），已连接时折叠/展开。
    /// 实测：折叠不会断开连接，「会话」计数与状态栏的连接指示都还在。
    fn toggle_connection(&mut self, ix: usize) {
        let Some(conn) = self.conns.get(ix) else {
            return;
        };
        if !conn.connected {
            self.open_connection(ix);
            return;
        }
        let name = conn.name.clone();
        self.toggle_key(name);
    }

    /// 库节点：展开即「打开」这个库（Navicat 里图标会从灰转绿并一直保持）
    fn toggle_db(&mut self, c: usize, d: usize) {
        let Some(conn) = self.conns.get(c) else {
            return;
        };
        if !conn.connected {
            self.open_connection(c);
            return;
        }
        let Some(db) = mock::databases(conn.kind).get(d) else {
            return;
        };
        let key = node_key(&conn.name, db);
        let opening = !self.expanded.contains(&key);
        self.toggle_key(key.clone());
        if opening {
            self.visited_dbs.insert(key);
        }
    }

    /// 右键「打开数据库」：只展开并把库标记为已打开（Navicat 里图标转绿）
    fn open_db(&mut self, c: usize, d: usize) {
        let Some(conn) = self.conns.get(c) else {
            return;
        };
        if !conn.connected {
            self.open_connection(c);
        }
        let conn = &self.conns[c];
        let Some(db) = mock::databases(conn.kind).get(d).copied() else {
            return;
        };
        let key = node_key(&conn.name, db);
        self.visited_dbs.insert(key.clone());
        self.expanded.insert(key);
    }

    /// 右键「关闭数据库」：卸载（图标回灰）并收起子树；连接本身不断开。
    fn close_db(&mut self, c: usize, d: usize) {
        let Some(conn) = self.conns.get(c) else {
            return;
        };
        let Some(db) = mock::databases(conn.kind).get(d).copied() else {
            return;
        };
        let key = node_key(&conn.name, db);
        self.visited_dbs.remove(&key);
        self.expanded.remove(&key);
        self.save();
    }

    /// 右键「打开模式」：展开模式节点，顺带把所属库标成已打开
    fn open_schema(&mut self, c: usize, d: usize, s: usize) {
        let Some(conn) = self.conns.get(c) else {
            return;
        };
        if !conn.connected {
            self.open_connection(c);
        }
        let conn = &self.conns[c];
        let (Some(db), Some(schema)) = (
            mock::databases(conn.kind).get(d).copied(),
            mock::schemas(conn.kind).get(s).copied(),
        ) else {
            return;
        };
        let db_key = node_key(&conn.name, db);
        let schema_key = node_key(&db_key, schema);
        self.visited_dbs.insert(db_key.clone());
        self.expanded.extend([db_key, schema_key]);
    }

    fn toggle_schema(&mut self, c: usize, d: usize, s: usize) {
        let Some(conn) = self.conns.get(c) else {
            return;
        };
        let (Some(db), Some(schema)) = (
            mock::databases(conn.kind).get(d).copied(),
            mock::schemas(conn.kind).get(s).copied(),
        ) else {
            return;
        };
        self.toggle_key(node_key(&node_key(&conn.name, db), schema));
    }

    fn toggle_cat(&mut self, c: usize, d: usize, s: usize, cat: usize) {
        let Some(conn) = self.conns.get(c) else {
            return;
        };
        let Some(key) = self.cat_key_of(conn, d, s, cat) else {
            return;
        };
        self.toggle_key(key);
    }

    /// 对象（表 / 视图…）节点下挂字段列表，展开键 = 分类键 ⊕ 对象名。
    fn toggle_obj(&mut self, c: usize, d: usize, s: usize, cat: usize, obj: usize) {
        let Some(conn) = self.conns.get(c) else {
            return;
        };
        let Some(key) = self.cat_key_of(conn, d, s, cat) else {
            return;
        };
        let Some(db) = mock::databases(conn.kind).get(d).copied() else {
            return;
        };
        let Some(obj_name) = mock::objects(conn, db, cat).get(obj).copied() else {
            return;
        };
        self.toggle_key(node_key(&key, obj_name));
    }

    /// 分类节点的展开键；库或模式索引越界时返回 None（渲染时该分支本就不存在）
    fn cat_key_of(
        &self,
        conn: &mock::Connection,
        d: usize,
        s: usize,
        cat: usize,
    ) -> Option<String> {
        let db = mock::databases(conn.kind).get(d).copied()?;
        let base = node_key(&conn.name, db);
        let scoped = if mock::has_schemas(conn.kind) {
            node_key(&base, mock::schemas(conn.kind).get(s).copied()?)
        } else {
            base
        };
        Some(node_key(&scoped, CATEGORIES[cat].0))
    }

    fn is_expanded(&self, ix: usize) -> bool {
        self.conns
            .get(ix)
            .is_some_and(|c| c.connected && self.expanded.contains(&c.name))
    }

    /// 自己判双击：同一行且间隔在系统双击时间内。命中后清空，三连击不会连锁触发。
    fn is_double_click(&mut self, act: Act) -> bool {
        let now = Instant::now();
        let double = self
            .last_click
            .is_some_and(|(prev, at)| prev == act && now.duration_since(at) < DOUBLE_CLICK_INTERVAL);
        eprintln!("[dbg] is_double_click: act={:?}, double={}, last_click={:?}", act, double, self.last_click.map(|(_, at)| now.duration_since(at)));
        self.last_click = if double { None } else { Some((act, now)) };
        double
    }

    /// 双击树节点 = Navicat 的「打开表」：表 / 视图开数据网格页签，
    /// 已打开过的直接切过去（不重复开）。
    fn open_object(&mut self, act: Act) {
        eprintln!("[dbg] open_object called: act={:?}", act);
        let Act::Obj(c, d, s, k, o) = act else { 
            eprintln!("[dbg] open_object: not an Obj action");
            return; 
        };
        let Some(conn) = self.conns.get(c) else { 
            eprintln!("[dbg] open_object: connection {} not found", c);
            return; 
        };
        let Some(db) = mock::databases(conn.kind).get(d).copied() else { 
            eprintln!("[dbg] open_object: database {} not found", d);
            return; 
        };
        let obj_name = mock::objects(conn, db, k).get(o).copied().unwrap_or_default();
        eprintln!("[dbg] open_object: opening table '{}' in conn '{}' db '{}'", obj_name, conn.name, db);
        if !self.opened.iter().any(|t| {
            t.conn_name == conn.name
                && t.db == db
                && t.schema == s
                && t.cat == k
                && t.obj == o
        }) {
            self.opened.push(OpenTab {
                conn_name: conn.name.clone(),
                db,
                schema: s,
                cat: k,
                obj: o,
                cell: (0, 0),
                design: false,
                did: 0,
                dstate: None,
                query: false,
                qid: 0,
                qstate: None,
                sort: None,
                data: materialize_rows(obj_name),
                baseline: materialize_rows(obj_name),
                inserted: Vec::new(),
            filter: filter::FilterState::default(),
            });
        }
        // 已开过的页签直接切过去；新页签落在当前聚焦窗格
        let ix = self
            .opened
            .iter()
            .position(|t| {
                t.conn_name == conn.name
                    && t.db == db
                    && t.schema == s
                    && t.cat == k
                    && t.obj == o
            })
            .map(|p| p + 1)
            .unwrap_or(self.opened.len());
        if self.dock.pane_of(ix).is_some() {
            self.dock.focus_tab(ix);
        } else {
            self.dock.add_tab(ix);
        }
        self.active_tab = ix;
        self.sel = act;
    }

    /// 打开「设计表」页签（动作条铅笔 / 对象右键菜单共用入口）。
    /// 同一对象的设计页签只开一份，重复触发只激活它。
    fn open_designer(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        let Act::Obj(c, d, s, k, o) = act else { return };
        let Some(conn) = self.conns.get(c) else { return };
        let Some(db) = mock::databases(conn.kind).get(d).copied() else { return };
        let conn_name = conn.name.clone();
        if let Some(ix) = self.opened.iter().position(|t| {
            t.design
                && t.conn_name == conn_name
                && t.db == db
                && t.schema == s
                && t.cat == k
                && t.obj == o
        }) {
            self.active_tab = ix + 1;
            self.sel = act;
            cx.notify();
            return;
        }
        let obj = mock::objects(conn, db, k).get(o).copied().unwrap_or_default();
        let did = self.next_did;
        self.next_did += 1;
        self.opened.push(OpenTab {
            conn_name,
            db,
            schema: s,
            cat: k,
            obj: o,
            cell: (0, 0),
            design: true,
            did,
            dstate: Some(DesignerState {
                popup: None,
                widths: Default::default(),
                resize: None,
                cols: mock_design_cols(obj),
                baseline: mock_design_cols(obj),
                sel: 0,
                cur: 0,
                sub: 0,
                edits: Vec::new(),
                indexes: initial_design_indexes(obj),
                index_edits: Vec::new(),
                index_sel: 0,
                index_cur: 1,
                idx_editor: None,
                baseline_indexes: initial_design_indexes(obj),
                fks: vec![DesignForeignKey::default()],
                fk_edits: Vec::new(),
                fk_sel: 0,
                fk_cur: 0,
                baseline_fks: vec![DesignForeignKey::default()],
                uniques: vec![DesignPair::default()],
                unique_edits: Vec::new(),
                unique_sel: 0,
                unique_cur: 1,
                baseline_uniques: vec![DesignPair::default()],
                checks: vec![DesignPair::default()],
                check_edits: Vec::new(),
                check_sel: 0,
                check_cur: 1,
                baseline_checks: vec![DesignPair::default()],
                exclude_sel: 0,
                exclude_cur: 2,
                exclude_editor: None,
                options: Self::seed_options(conn.kind),
                opt_edits: Default::default(),
                // 排除空行种子只给 PostgreSQL：其它方言门控会把空行当真数据，
                // 保存按钮从此灰掉
                excludes: if matches!(conn.kind, mock::DbKind::PostgreSql) {
                    vec![DesignExclude::default()]
                } else {
                    Vec::new()
                },
                exclude_edits: Vec::new(),
                // 规则空行种子只给 PostgreSQL：其它方言的门控会把空行当真数据
                rules: Self::seed_rules(conn.kind),
                rule_sel: 0,
                rule_cur: 1,
                rule_edits: Vec::new(),
                triggers: Self::seed_triggers(conn.kind),
                trigger_edits: Vec::new(),
                trig_fields_editor: None,
                trig_sel: 0,
                trig_cur: 0,
                baseline_options: Self::seed_options(conn.kind),
                baseline_excludes: if matches!(conn.kind, mock::DbKind::PostgreSql) {
                    vec![DesignExclude::default()]
                } else {
                    Vec::new()
                },
                baseline_rules: Self::seed_rules(conn.kind),
                baseline_triggers: Self::seed_triggers(conn.kind),
                sql_sub: 0,
                view_body: String::new(),
                view_editor: None,
                note: None,
                new_name: None,
            }),
            query: false,
            qid: 0,
            qstate: None,
            sort: None,
            data: Vec::new(),
            baseline: Vec::new(),
            inserted: Vec::new(),
            filter: filter::FilterState::default(),
        });
        // 已开过的页签直接切过去；新页签落在当前聚焦窗格
        let ix = self
            .opened
            .iter()
            .position(|t| {
                t.design
                    && t.conn_name == conn.name
                    && t.db == db
                    && t.schema == s
                    && t.cat == k
                    && t.obj == o
            })
            .map(|p| p + 1)
            .unwrap_or(self.opened.len());
        if self.dock.pane_of(ix).is_some() {
            self.dock.focus_tab(ix);
        } else {
            self.dock.add_tab(ix);
        }
        self.active_tab = ix;
        self.sel = act;
        self.spawn_design_editors(did, window, cx);
        self.spawn_index_editors(did, window, cx);
        self.spawn_fk_editors(did, window, cx);
        self.spawn_unique_editors(did, window, cx);
        self.spawn_check_editors(did, window, cx);
        self.spawn_option_editors(did, window, cx);
        self.spawn_exclude_editors(did, window, cx);
        self.spawn_rule_editors(did, window, cx);
        self.spawn_trigger_editors(did, window, cx);
        // 视图 / 实体化视图：正文编辑器（代码编辑器 + SQL 高亮/补全）
        if k == 1 || k == 2 {
            self.ensure_view_editor(did, window, cx);
        }
        cx.notify();
    }

    /// 「新建表」：开一份空模型的设计页签（单行空字段，与参考工具一致）。
    /// 表名不在页面上填（工具栏纯图标），点「保存」时弹「表名」输入框补
    /// （[DaroApp::finish_new_design_save] 承接）；保存成功前标题就显示
    /// 「新建表」。obj 记成 usize::MAX 哨兵：目录里没有这一项，改名 / 去重
    /// 等按 obj 配对的逻辑天然不会误命中。
    fn open_new_designer(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        let Act::Obj(c, d, s, 0, _) = act else { return };
        let Some(conn) = self.conns.get(c) else { return };
        let Some(db) = mock::databases(conn.kind).get(d).copied() else { return };
        let did = self.next_did;
        self.next_did += 1;
        let empty = vec![DesignCol::default()];
        self.opened.push(OpenTab {
            conn_name: conn.name.clone(),
            db,
            schema: s,
            cat: 0,
            obj: usize::MAX,
            cell: (0, 0),
            design: true,
            did,
            dstate: Some(DesignerState {
                popup: None,
                widths: Default::default(),
                resize: None,
                cols: empty.clone(),
                baseline: empty,
                sel: 0,
                cur: 0,
                sub: 0,
                edits: Vec::new(),
                indexes: vec![DesignIndex::default()],
                index_edits: Vec::new(),
                index_sel: 0,
                index_cur: 1,
                idx_editor: None,
                baseline_indexes: vec![DesignIndex::default()],
                fks: vec![DesignForeignKey::default()],
                fk_edits: Vec::new(),
                fk_sel: 0,
                fk_cur: 0,
                baseline_fks: vec![DesignForeignKey::default()],
                uniques: vec![DesignPair::default()],
                unique_edits: Vec::new(),
                unique_sel: 0,
                unique_cur: 1,
                baseline_uniques: vec![DesignPair::default()],
                checks: vec![DesignPair::default()],
                check_edits: Vec::new(),
                check_sel: 0,
                check_cur: 1,
                baseline_checks: vec![DesignPair::default()],
                exclude_sel: 0,
                exclude_cur: 2,
                exclude_editor: None,
                options: Self::seed_options(conn.kind),
                opt_edits: Default::default(),
                // 排除空行种子只给 PostgreSQL：其它方言门控会把空行当真数据，
                // 保存按钮从此灰掉
                excludes: if matches!(conn.kind, mock::DbKind::PostgreSql) {
                    vec![DesignExclude::default()]
                } else {
                    Vec::new()
                },
                exclude_edits: Vec::new(),
                // 规则空行种子只给 PostgreSQL：其它方言的门控会把空行当真数据
                rules: Self::seed_rules(conn.kind),
                rule_sel: 0,
                rule_cur: 1,
                rule_edits: Vec::new(),
                triggers: Self::seed_triggers(conn.kind),
                trigger_edits: Vec::new(),
                trig_fields_editor: None,
                trig_sel: 0,
                trig_cur: 0,
                baseline_options: Self::seed_options(conn.kind),
                baseline_excludes: if matches!(conn.kind, mock::DbKind::PostgreSql) {
                    vec![DesignExclude::default()]
                } else {
                    Vec::new()
                },
                baseline_rules: Self::seed_rules(conn.kind),
                baseline_triggers: Self::seed_triggers(conn.kind),
                sql_sub: 0,
                view_body: String::new(),
                view_editor: None,
                note: None,
                new_name: Some(String::new()),
            }),
            query: false,
            qid: 0,
            qstate: None,
            sort: None,
            data: Vec::new(),
            baseline: Vec::new(),
            inserted: Vec::new(),
            filter: filter::FilterState::default(),
        });
        let ix = self.opened.len();
        self.dock.add_tab(ix);
        self.active_tab = ix;
        self.spawn_design_editors(did, window, cx);
        self.spawn_index_editors(did, window, cx);
        self.spawn_fk_editors(did, window, cx);
        self.spawn_unique_editors(did, window, cx);
        self.spawn_check_editors(did, window, cx);
        self.spawn_option_editors(did, window, cx);
        self.spawn_exclude_editors(did, window, cx);
        self.spawn_rule_editors(did, window, cx);
        self.spawn_trigger_editors(did, window, cx);
        cx.notify();
    }

    /// 新建表点「保存」后的承接（「表名」输入框独立窗口的回调）：
    /// 记下名字、基线对齐当前模型、给一句如实说明（mock 档不落库）。
    fn finish_new_design_save(&mut self, did: u64, name: String) {
        let Some(tab) = self.opened.iter_mut().find(|t| t.did == did) else {
            return;
        };
        let Some(ds) = tab.dstate.as_mut() else { return };
        if ds.new_name.as_ref().is_some_and(|n| !n.is_empty()) {
            return;
        }
        ds.new_name = Some(name);
        ds.baseline = ds.cols.clone();
        ds.note = Some("已创建（演示数据，不落库）");
    }

    /// 打开「新建查询」页签（Ribbon / 文件菜单 / 连接右键菜单共用入口）。
    /// 查询页签不去重：每触发一次就是一张新页签（Navicat 同款）。
    /// 运行上下文：优先树选中的连接，否则第一条连接；库取其第一个。
    fn open_query_tab(&mut self, conn_ix: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let ix = conn_ix
            .or_else(|| self.sel.conn())
            .filter(|i| *i < self.conns.len())
            .unwrap_or(0);
        let Some(conn) = self.conns.get(ix) else { return };
        let conn_name = conn.name.clone();
        let qid = self.next_qid;
        let seq = self.next_qseq;
        self.next_qid += 1;
        self.next_qseq += 1;
        let db0 = mock::databases(conn.kind)[0];
        let comp_tables: Vec<String> = mock::objects(conn, db0, 0)
            .iter()
            .map(|s| s.to_string())
            .collect();
        let comp_columns: Vec<String> = mock::MOCK_COLUMNS
            .iter()
            .map(|c| c.name.to_string())
            .collect();
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx).placeholder("-- 在此输入 SQL 语句");
            // SQL 语法高亮（迁移自 Flutter sql_highlight.dart）。
            // 工厂按语言名造 highlighter；set_highlighter 让编辑器立即重建缓存。
            state.set_highlighter_factory(
                std::rc::Rc::new(|_lang: &str| {
                    Some(Box::new(sql_highlight::SqlHighlighter::new())
                        as Box<dyn gpui_kit::component::input::InputHighlighter>)
                }),
                cx,
            );
            state.set_highlighter("sql", cx);
            // SQL 补全（迁移自 Flutter sql_completions.dart）：关键字 + 函数 + 表 + 列
            state.lsp_mut().completion_provider =
                Some(sql_completion::provider_for(&comp_tables, &comp_columns));
            state
        });
        // 编辑过未保存 → 页签标题前「*」（Navicat 习惯）。写回按 qid 定位页签。
        cx.subscribe(&editor, move |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change)
                && let Some(t) = this.opened.iter_mut().find(|t| t.qid == qid)
                && let Some(q) = t.qstate.as_mut()
                && !q.dirty
            {
                q.dirty = true;
                cx.notify();
            }
        })
        .detach();
        let log = cx.new(|cx| TextareaState::new(window, cx));
        self.opened.push(OpenTab {
            conn_name,
            db: mock::databases(self.conns[ix].kind)[0],
            schema: 0,
            cat: 4,
            obj: 0,
            cell: (0, 0),
            design: false,
            did: 0,
            dstate: None,
            query: true,
            qid,
            sort: None,
            data: Vec::new(),
            baseline: Vec::new(),
            inserted: Vec::new(),
            filter: filter::FilterState::default(),
            qstate: Some(QueryState {
                editor,
                log,
                conn: self.conns[ix].name.clone(),
                db: 0,
                schema: 0,
                seq,
                dirty: false,
                outcomes: Vec::new(),
                pane: 0,
                last_ms: 0,
                prompt: None,
                panel_h: Q_PANEL_H,
            }),
        });
        let ix = self.opened.len();
        self.dock.add_tab(ix);
        self.active_tab = ix;
        cx.notify();
    }

    /// 运行（查询页签）：优先选中文本（selected_only 时强制选中），无选中跑整篇。
    /// 按顶层分号逐条「执行」：SELECT 出 mock 结果网格，写操作不出网格；
    /// 全程同步瞬时完成，「停止」因此在 mock 档恒灰。
    fn query_run(&mut self, qid: u64, selected_only: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.opened.iter_mut().find(|t| t.qid == qid) else {
            return;
        };
        let Some(q) = t.qstate.as_mut() else { return };
        let text = if selected_only {
            q.editor.read(cx).selected_text().to_string()
        } else {
            q.editor.read(cx).value().to_string()
        };
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            q.prompt = Some("请输入 SQL 语句");
            q.outcomes.clear();
            q.pane = 0;
            cx.notify();
            return;
        }
        q.prompt = None;
        let started = Instant::now();
        let mut outcomes: Vec<QueryOutcome> = Vec::new();
        for stmt in split_sql_statements(&trimmed) {
            let sw = Instant::now();
            let result = query_is_select(&stmt).then(|| mock_query_rows(&stmt));
            outcomes.push(QueryOutcome {
                elapsed_ms: sw.elapsed().as_millis(),
                sql: stmt,
                result,
                cell: (0, 0),
            });
        }
        q.last_ms = started.elapsed().as_millis();
        // 执行完自动切到最新的「有网格」结果页签；全是写操作就停在消息页
        q.pane = match outcomes.iter().rposition(|o| o.result.is_some()) {
            Some(i) => i + 1,
            None => 0,
        };
        // 消息日志整份重写（可拖选复制的只读文本）
        let mut log = String::new();
        for (i, o) in outcomes.iter().enumerate() {
            log.push_str(&format!("{}\n", o.sql.replace(['\n', '\r'], " ")));
            let summary = match &o.result {
                Some(rows) => format!("{} 行", rows.len()),
                None => "演示数据：写操作未执行".to_string(),
            };
            log.push_str(&format!("语句 {}: {} · {} ms\n\n", i + 1, summary, o.elapsed_ms));
        }
        q.log.update(cx, |s, cx| {
            s.set_value(log, window, cx);
        });
        q.outcomes = outcomes;
        cx.notify();
    }

    /// 结果面板头的状态文案（同 Flutter 版口径：结果 + 耗时）。
    fn query_status_text(q: &QueryState) -> String {
        let n = q.outcomes.len();
        if n == 0 {
            return q.prompt.unwrap_or("").to_string();
        }
        if n == 1 {
            return match &q.outcomes[0].result {
                Some(rows) => format!("执行成功 · {} 行 · {} ms", rows.len(), q.last_ms),
                None => format!("演示数据 · 写操作未执行 · {} ms", q.last_ms),
            };
        }
        format!("执行成功 · {n} 条语句 · {} ms", q.last_ms)
    }

    /// 点结果网格的格子：整行铺选中色、当前格铺 accent（同表数据页）。
    fn query_pick_cell(&mut self, qid: u64, oi: usize, r: usize, c: usize, cx: &mut Context<Self>) {
        if let Some(t) = self.opened.iter_mut().find(|t| t.qid == qid)
            && let Some(q) = t.qstate.as_mut()
            && let Some(o) = q.outcomes.get_mut(oi)
        {
            o.cell = (r, c);
            cx.notify();
        }
    }

    /// 规则空行种子只给 PostgreSQL：其它方言的门控会把空行当真数据、保存键从此灰掉。
    /// 选项子页的种子：所有者 postgres（PG 连接；其它方言不填）
    fn seed_options(kind: mock::DbKind) -> DesignOptions {
        match kind {
            mock::DbKind::PostgreSql => DesignOptions {
                owner: "postgres".into(),
                ..Default::default()
            },
            _ => DesignOptions::default(),
        }
    }

    fn seed_rules(kind: mock::DbKind) -> Vec<DesignRule> {
        if matches!(kind, mock::DbKind::PostgreSql) {
            vec![DesignRule::default()]
        } else {
            Vec::new()
        }
    }

    /// 触发器子页的种子行：仅可触发方言播种（PG / MySQL / SQL Server）；
    /// 给每个 ROW / 触发 BEFORE / 启用勾上，与参考图种子行一致
    fn seed_triggers(kind: mock::DbKind) -> Vec<DesignTrigger> {
        match kind {
            mock::DbKind::MongoDb | mock::DbKind::Redis => Vec::new(),
            _ => vec![DesignTrigger {
                for_each: "ROW".into(),
                timing: "BEFORE".into(),
                enabled: true,
                ..Default::default()
            }],
        }
    }

    /// 为设计页签补齐常驻编辑器（打开页签 / 添加字段时调）。
    /// 编辑器写回按 did 定位页签：关掉前面的页签会让下标整体漂移。
    fn spawn_design_editors(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
            return;
        };
        let specs: Vec<(usize, usize, String, &'static str)> = tab
            .dstate
            .as_ref()
            .expect("设计页签必有状态")
            .cols
            .iter()
            .enumerate()
            .flat_map(|(r, c)| {
                [
                    (0usize, c.name.clone(), "列名"),
                    (1, c.len.clone(), "长度"),
                    (2, c.dec.clone(), "小数点"),
                    (3, c.comment.clone(), "注释"),
                    (4, c.default.clone(), "默认"),
                    (5, c.identity.clone(), "无"),
                    (6, c.ty.clone(), "类型"),
                ]
                .map(move |(f, v, ph)| (r, f, v, ph))
            })
            .collect();
        for (r, f, val, ph) in specs {
            let ds = self
                .opened
                .iter_mut()
                .find(|t| t.did == did)
                .and_then(|t| t.dstate.as_mut())
                .expect("设计页签必有状态");
            while ds.edits.len() < ds.cols.len() {
                ds.edits.push(Default::default());
            }
            if ds.edits[r][f].is_some() {
                continue;
            }
            let state =
                cx.new(|cx| InputState::new(window, cx).placeholder(ph.to_string()));
            state.update(cx, |s, cx| s.set_value(val, window, cx));
            cx.subscribe(&state, move |this, input, ev: &InputEvent, cx| {
                let InputEvent::Change = ev else { return };
                let Some(tab) = this.opened.iter_mut().find(|t| t.did == did) else {
                    return;
                };
                let Some(ds) = tab.dstate.as_mut() else { return };
                let text = input.read(cx).value().to_string();
                if let Some(col) = ds.cols.get_mut(r) {
                    match f {
                        0 => col.name = text,
                        1 => col.len = text,
                        2 => col.dec = text,
                        3 => col.comment = text,
                        4 => col.default = text,
                        5 => col.identity = text,
                        _ => col.ty = text,
                    }
                    ds.note = None;
                    cx.notify();
                }
            })
            .detach();
            let ds = self
                .opened
                .iter_mut()
                .find(|t| t.did == did)
                .and_then(|t| t.dstate.as_mut())
                .expect("设计页签必有状态");
            ds.edits[r][f] = Some(state);
        }
    }

    /// 索引子页的常驻编辑器（每行 [名称, 注释, 填充因子, 待处理列表限制,
    /// 每范围页数, 约束]），口径同字段网格：打开页签 / 增删索引时建齐，
    /// 键入即写回模型。索引方法 / 唯一键 / 并发 / 索引属性里的下拉都是 on_click。
    fn spawn_index_editors(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
            return;
        };
        let specs: Vec<(usize, usize, String, &'static str)> = tab
            .dstate
            .as_ref()
            .expect("设计页签必有状态")
            .indexes
            .iter()
            .enumerate()
            .flat_map(|(r, idx)| {
                [
                    (0usize, idx.name.clone(), "索引名"),
                    (1, idx.comment.clone(), ""),
                    (2, idx.fillfactor.clone(), ""),
                    (3, idx.pending_list_limit.clone(), ""),
                    (4, idx.pages_per_range.clone(), ""),
                    (5, idx.constraint.clone(), ""),
                ]
                .map(move |(f, v, ph)| (r, f, v, ph))
            })
            .collect();
        for (r, f, val, ph) in specs {
            {
                let ds = self
                    .opened
                    .iter_mut()
                    .find(|t| t.did == did)
                    .and_then(|t| t.dstate.as_mut())
                    .expect("设计页签必有状态");
                while ds.index_edits.len() < ds.indexes.len() {
                    ds.index_edits.push(Default::default());
                }
                if r < ds.index_edits.len() && ds.index_edits[r][f].is_some() {
                    continue;
                }
            }
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(ph.to_string()));
            state.update(cx, |s, cx| s.set_value(val, window, cx));
            cx.subscribe(&state, move |this, input, ev: &InputEvent, cx| {
                let InputEvent::Change = ev else { return };
                let Some(tab) = this.opened.iter_mut().find(|t| t.did == did) else {
                    return;
                };
                let Some(ds) = tab.dstate.as_mut() else { return };
                let text = input.read(cx).value().to_string();
                if let Some(idx) = ds.indexes.get_mut(r) {
                    match f {
                        0 => idx.name = text,
                        1 => idx.comment = text,
                        2 => idx.fillfactor = text,
                        3 => idx.pending_list_limit = text,
                        4 => idx.pages_per_range = text,
                        _ => idx.constraint = text,
                    }
                    ds.note = None;
                    cx.notify();
                }
            })
            .detach();
            let ds = self
                .opened
                .iter_mut()
                .find(|t| t.did == did)
                .and_then(|t| t.dstate.as_mut())
                .expect("设计页签必有状态");
            ds.index_edits[r][f] = Some(state);
        }
    }

    fn design_add_index(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            // 已经有空白行就直接选中它（少点一次、也避免堆两行空行）
            if let Some(blank) = ds.indexes.iter().position(design_index_blank) {
                ds.index_sel = blank;
            } else {
                ds.indexes.push(DesignIndex::default());
                ds.index_sel = ds.indexes.len() - 1;
            }
            ds.index_cur = 0;
            ds.index_edits.clear();
            ds.note = None;
        }
        cx.notify();
        self.spawn_index_editors(did, window, cx);
        self.spawn_fk_editors(did, window, cx);
        self.spawn_unique_editors(did, window, cx);
        self.spawn_check_editors(did, window, cx);
        self.spawn_option_editors(did, window, cx);
        self.spawn_exclude_editors(did, window, cx);
        self.spawn_rule_editors(did, window, cx);
        self.spawn_trigger_editors(did, window, cx);
    }

    fn design_remove_index(
        &mut self,
        did: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && row < ds.indexes.len()
        {
            ds.indexes.remove(row);
            // 编辑器按行对齐：只摘掉同一行，其余行的输入框不能跟着清空
            if row < ds.index_edits.len() {
                ds.index_edits.remove(row);
            }
            // 空行常驻（参考工具口径）：删光后补一行空白种子行
            if ds.indexes.is_empty() {
                ds.indexes.push(DesignIndex::default());
                ds.index_edits.clear();
            }
            ds.index_sel = ds.index_sel.min(ds.indexes.len().saturating_sub(1));
            ds.note = None;
        }
        // 补出来的空白种子行也要有输入框
        self.spawn_index_editors(did, window, cx);
        cx.notify();
    }

    /// 索引子页的当前行，越界时夹回最后一行（删索引后常见）。
    fn design_index_sel(ds: &DesignerState) -> usize {
        ds.index_sel.min(ds.indexes.len().saturating_sub(1))
    }

    /// 点索引表格的某一格：换当前行 / 当前列（表头淡蓝块与当前格反白跟它走）。
    fn design_pick_index_cell(&mut self, did: u64, row: usize, col: usize, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.index_sel = row;
            ds.index_cur = col;
            cx.notify();
        }
    }

    /// 外键子页的当前行，越界时夹回最后一行（删外键后常见）。
    fn design_fk_sel(ds: &DesignerState) -> usize {
        ds.fk_sel.min(ds.fks.len().saturating_sub(1))
    }

    /// 点外键表格的某一格：换当前行 / 当前列（行首槽与当前格跟它走）。
    fn design_pick_fk_cell(&mut self, did: u64, row: usize, col: usize, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.fk_sel = row;
            ds.fk_cur = col;
            cx.notify();
        }
    }

    /// 索引方法下拉：写回显示名（B-Tree / Hash / GiST / GIN / BRIN / SP-GiST）。
    fn design_set_index_method(&mut self, did: u64, row: usize, method: String, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.index_sel = row;
            if let Some(idx) = ds.indexes.get_mut(row) {
                idx.method = method;
            }
            ds.note = None;
            cx.notify();
        }
    }

    /// 唯一键 / 并发两列的勾选。
    fn design_toggle_index_flag(&mut self, did: u64, row: usize, unique: bool, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.index_sel = row;
            ds.index_cur = if unique { 3 } else { 4 };
            if let Some(idx) = ds.indexes.get_mut(row) {
                if unique {
                    idx.unique = !idx.unique;
                } else {
                    idx.concurrent = !idx.concurrent;
                }
            }
            ds.note = None;
            cx.notify();
        }
    }

    /// 索引属性里的下拉：0 表空间 / 1 正在缓冲 / 2 快速更新 / 3 约束。
    fn design_set_index_prop(
        &mut self,
        did: u64,
        row: usize,
        prop: u8,
        value: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.index_sel = row;
            if let Some(idx) = ds.indexes.get_mut(row) {
                match prop {
                    0 => idx.tablespace = value,
                    1 => idx.buffering = value,
                    2 => idx.fastupdate = value,
                    _ => idx.constraint = value,
                }
            }
            ds.note = None;
            cx.notify();
        }
    }

    /// 索引子页：一行一个索引（名称 / 字段 / 方法 / 唯一 / 删除），底部「添加索引」。
    /// 打开「…」列选择面板：候选 = 表里的列 + 已经写在字段串里但不在列里的项
    fn design_open_picker(
        &mut self,
        did: u64,
        target: PickTarget,
        row: usize,
        cx: &mut Context<Self>,
    ) {
        // 普通下拉（参考图：被引用的模式 / 被引用的表 / 删除时 / 更新时）：
        // 无勾选框与底栏，点一项即写回
        if target.is_plain() {
            let payload = {
                let Some(t) = self.opened.iter().find(|t| t.did == did) else {
                    return;
                };
                let Some(d) = t.dstate.as_ref() else { return };
                let kind = self
                    .conns
                    .iter()
                    .find(|c| c.name == t.conn_name)
                    .map(|c| c.kind);
                let (current, list): (String, Vec<String>) = match target {
                    PickTarget::FkSchema => (
                        d.fks.get(row).map(|f| f.ref_schema.clone()).unwrap_or_default(),
                        fk_schema_candidates(
                            kind.unwrap_or(mock::DbKind::MySql),
                            &t.db.clone(),
                        ),
                    ),
                    PickTarget::FkRefTable => {
                        let cur = d.fks.get(row).map(|f| f.ref_table.clone()).unwrap_or_default();
                        // 按已输入文本过滤（参考图是输入 + 下拉联动）
                        let needle = cur.trim().to_ascii_lowercase();
                        let tables: Vec<String> = self.fk_ref_tables(did);
                        let list = if needle.is_empty() {
                            tables
                        } else {
                            tables
                                .into_iter()
                                .filter(|n| n.to_ascii_lowercase().contains(&needle))
                                .collect()
                        };
                        (cur, list)
                    }
                    PickTarget::FkOnDelete => (
                        d.fks.get(row).map(|f| f.on_delete.clone()).unwrap_or_default(),
                        FK_ACTIONS.iter().map(|s| s.to_string()).collect(),
                    ),
                    _ => (
                        d.fks.get(row).map(|f| f.on_update.clone()).unwrap_or_default(),
                        FK_ACTIONS.iter().map(|s| s.to_string()).collect(),
                    ),
                };
                let sel = list.iter().position(|n| *n == current).unwrap_or(0);
                let items = list.into_iter().map(|n| (n, false)).collect();
                (items, sel)
            };
            if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
                && let Some(d) = t.dstate.as_mut()
            {
                d.popup = Some(DesignPopup {
                    target,
                    row,
                    items: payload.0,
                    sel: payload.1,
                    expr: None,
                });
                d.note = None;
            }
            cx.notify();
            return;
        }

        let Some(t) = self.opened.iter().find(|t| t.did == did) else {
            return;
        };
        let Some(d) = t.dstate.as_ref() else { return };
        let current = match target {
            PickTarget::IndexCols => d.indexes.get(row).map(|i| {
                i.fields
                    .iter()
                    .map(|f| f.name.trim().to_string())
                    .filter(|n| !n.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ")
            }),
            PickTarget::FkCols => d.fks.get(row).map(|f| f.columns.clone()),
            PickTarget::FkRefCols => d.fks.get(row).map(|f| f.ref_columns.clone()),
            PickTarget::UniqueCols => d.uniques.get(row).map(|u| u.value.clone()),
            PickTarget::ExcludeElems => d.excludes.get(row).map(|e| e.elements.clone()),
            _ => None,
        }
        .unwrap_or_default();
        let chosen = split_cols(&current);
        // 候选列：本表列（FkCols）或被引用表的列（FkRefCols，参考图列出引用表的 id/name/value）
        let mut items: Vec<(String, bool)> = match target {
            PickTarget::FkRefCols => d
                .fks
                .get(row)
                .map(|f| mock_design_cols(f.ref_table.trim()))
                .unwrap_or_default()
                .iter()
                .map(|c| c.name.trim().to_string())
                .filter(|n| !n.is_empty())
                .map(|n| {
                    let on = chosen.iter().any(|x| *x == n);
                    (n, on)
                })
                .collect(),
            _ => d
                .cols
                .iter()
                .map(|c| c.name.trim().to_string())
                .filter(|n| !n.is_empty())
                .map(|n| {
                    let on = chosen.iter().any(|x| *x == n);
                    (n, on)
                })
                .collect(),
        };
        // 已写在字段串里、但当前列里没有的（自由文本 / 引用表的列）也保留，别悄悄吞掉
        for name in &chosen {
            if !items.iter().any(|(n, _)| n == name) {
                items.push((name.clone(), true));
            }
        }
        // 索引的「字段」面板按表里的列顺序列出全部候选（参考图就是散着勾的）；
        // 其余子页仍把勾选的排前面 —— 勾选顺序就是写回顺序。
        if target != PickTarget::IndexCols {
            items.sort_by_key(|(_, on)| !*on);
        }
        let sel = items.iter().position(|(_, on)| *on).unwrap_or(0);
        if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = t.dstate.as_mut()
        {
            d.popup = Some(DesignPopup {
                target,
                row,
                items,
                sel,
                expr: None,
            });
            d.note = None;
        }
        cx.notify();
    }

    /// 唯一键注释写回 + 同步常驻输入格（注释小窗预留，当前为独立小窗形态预留接口）
    fn finish_unique_comment(
        &mut self,
        did: u64,
        row: usize,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(uk) = ds.uniques.get_mut(row)
        {
            uk.comment = value.clone();
            ds.note = None;
        }
        self.design_sync_unique_editor(did, row, 2, &value, window, cx);
        cx.notify();
    }

    /// 唯一键版编辑器同步（浮层写回不经过输入事件）
    fn design_sync_unique_editor(
        &mut self,
        did: u64,
        row: usize,
        f: usize,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .and_then(|d| d.unique_edits.get(row))
            .and_then(|e| e.get(f))
            .cloned()
            .flatten()
        else {
            return;
        };
        let value = value.to_string();
        handle.update(cx, |s, cx| s.set_value(value, window, cx));
    }

    /// 打开外键「注释」小窗：独立顶层窗口（参考图里它能越出主窗边界），
    /// 锚在注释格正下方。确定写回模型并同步主窗的常驻输入格。
    fn design_open_fk_comment(
        &mut self,
        did: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(cur) = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .and_then(|d| d.fks.get(row))
            .map(|f| f.comment.clone())
        else {
            return;
        };
        let dark = self.dark;
        let widths = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .map(|d| d.widths.clone());
        let me = cx.entity().clone();
        let main = window.window_handle();
        // 屏幕锚点 = 主窗原点 + 主窗内设计器页偏移 + 页内格坐标（注释格左下角）
        let cw = |ix: usize| {
            widths
                .as_ref()
                .and_then(|w| w.get(&(2usize, ix)))
                .copied()
                .unwrap_or(D_FK_WIDTHS[ix])
        };
        let anchor_x = window.bounds().origin.x
            + px(DESIGN_PAGE_X + D_GUTTER_W + (0..7).map(cw).sum::<f32>() + 6.);
        let anchor_y = window.bounds().origin.y
            + px(DESIGN_PAGE_Y + D_TOOLBAR_H + D_TABBAR_H + 6. + D_HEADER_H
                + (row as f32 + 1.) * D_ROW_EXTENT
                + 3.);
        // 无边框小窗（参考图形态：白底细边、没有系统标题栏按钮）
        let mut options = TitleBar::window_options();
        options.titlebar = None;
        options.window_min_size = Some(size(px(342.), px(330.)));
        options.window_bounds = Some(WindowBounds::Windowed(Bounds {
            origin: point(anchor_x, anchor_y),
            size: size(px(342.), px(330.)),
        }));
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| FkCommentPopup::new(me, main, did, row, cur, dark, window, cx))
        });
        if opened.is_err() {
            eprintln!("failed to open fk comment window");
        }
    }

    /// 注释小窗「确定」：写回 fks[row].comment 并同步主窗的常驻输入格
    fn finish_fk_comment(
        &mut self,
        did: u64,
        row: usize,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(fk) = ds.fks.get_mut(row)
        {
            fk.comment = value.clone();
            ds.note = None;
        }
        self.design_sync_fk_editor(did, row, 4, &value, window, cx);
        cx.notify();
    }

    /// 外键「被引用的表」下拉的候选：当前库下的表清单
    fn fk_ref_tables(&self, did: u64) -> Vec<String> {
        let Some(t) = self.opened.iter().find(|t| t.did == did) else {
            return Vec::new();
        };
        let Some(conn) = self.conns.iter().find(|c| c.name == t.conn_name) else {
            return Vec::new();
        };
        mock::objects(conn, t.db, 0)
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// 普通下拉点一项：即写即关（模式 / 表 / 删除时 / 更新时）。
    /// 选引用表时把空着的被引用的模式自动带成 public（参考图同款联动）。
    fn design_popup_pick_plain(
        &mut self,
        did: u64,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (target, row, value) = {
            let Some(t) = self.opened.iter().find(|t| t.did == did) else {
                return;
            };
            let Some(d) = t.dstate.as_ref() else { return };
            let Some(p) = d.popup.as_ref() else { return };
            let Some(value) = p.items.get(ix).map(|(n, _)| n.clone()) else {
                return;
            };
            (p.target, p.row, value)
        };
        // 选完引用表自动带模式：MySQL 带所在库，PG 带 public（参考图同款联动）
        let auto_schema = {
            let Some(t) = self.opened.iter().find(|t| t.did == did) else {
                return;
            };
            let is_mysql = self
                .conns
                .iter()
                .find(|c| c.name == t.conn_name)
                .is_some_and(|c| matches!(c.kind, mock::DbKind::MySql));
            if is_mysql {
                t.db.to_string()
            } else {
                "public".to_string()
            }
        };
        let mut synced: Option<(usize, String)> = None;
        if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = t.dstate.as_mut()
        {
            if let Some(fk) = d.fks.get_mut(row) {
                match target {
                    PickTarget::FkSchema => fk.ref_schema = value.clone(),
                    PickTarget::FkRefTable => {
                        fk.ref_table = value.clone();
                        if fk.ref_schema.trim().is_empty() {
                            fk.ref_schema = auto_schema;
                        }
                        synced = Some((2, value.clone()));
                    }
                    PickTarget::FkOnDelete => fk.on_delete = value.clone(),
                    _ => fk.on_update = value.clone(),
                }
            }
            d.popup = None;
            d.note = None;
        }
        if let Some((f, v)) = synced {
            self.design_sync_fk_editor(did, row, f, &v, window, cx);
        }
        cx.notify();
    }

    /// 浮层写回后把常驻输入框的显示值对齐模型（弹层改的值不走输入事件）
    fn design_sync_fk_editor(
        &mut self,
        did: u64,
        row: usize,
        f: usize,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .and_then(|d| d.fk_edits.get(row))
            .and_then(|e| e.get(f))
            .cloned()
            .flatten()
        else {
            return;
        };
        let value = value.to_string();
        handle.update(cx, |s, cx| s.set_value(value, window, cx));
    }

    /// 打开「检查」子页表达式面板（多行文本域 + 确定/取消）
    /// 打开检查「表达式」小窗：独立顶层窗口（与外键注释小窗同款，可越出主窗边界），
    /// 锚在表达式格正下方。确定写回 checks[row].value 并同步主窗的常驻输入格。
    fn design_open_check(
        &mut self,
        did: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(cur) = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .and_then(|d| d.checks.get(row))
            .map(|c| c.value.clone())
        else {
            return;
        };
        let dark = self.dark;
        let widths = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .map(|d| d.widths.clone());
        let me = cx.entity().clone();
        let main = window.window_handle();
        // 屏幕锚点 = 主窗原点 + 主窗内设计器页偏移 + 页内格坐标（表达式格左下角）
        let cw = |ix: usize| {
            widths
                .as_ref()
                .and_then(|w| w.get(&(4usize, ix)))
                .copied()
                .unwrap_or(D_CHECK_WIDTHS[ix])
        };
        let anchor_x = window.bounds().origin.x
            + px(DESIGN_PAGE_X + D_GUTTER_W + cw(0) + 6.);
        let anchor_y = window.bounds().origin.y
            + px(DESIGN_PAGE_Y + D_TOOLBAR_H + D_TABBAR_H + 6. + D_HEADER_H
                + (row as f32 + 1.) * D_ROW_EXTENT
                + 3.);
        // 无边框小窗（参考图形态）
        let mut options = TitleBar::window_options();
        options.titlebar = None;
        options.window_min_size = Some(size(px(342.), px(330.)));
        options.window_bounds = Some(WindowBounds::Windowed(Bounds {
            origin: point(anchor_x, anchor_y),
            size: size(px(342.), px(330.)),
        }));
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| CheckExprPopup::new(me, main, did, row, cur, dark, window, cx))
        });
        if opened.is_err() {
            eprintln!("failed to open check expr window");
        }
    }

    /// 检查「表达式」小窗「确定」：写回 checks[row].value 并同步主窗的常驻输入格
    fn finish_check_expr(
        &mut self,
        did: u64,
        row: usize,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(ck) = ds.checks.get_mut(row)
        {
            ck.value = value.clone();
            ds.note = None;
        }
        self.design_sync_check_editor(did, row, 1, &value, window, cx);
        cx.notify();
    }

    /// 检查版编辑器同步（浮层写回不经过输入事件）
    fn design_sync_check_editor(
        &mut self,
        did: u64,
        row: usize,
        f: usize,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .and_then(|d| d.check_edits.get(row))
            .and_then(|e| e.get(f))
            .cloned()
            .flatten()
        else {
            return;
        };
        let value = value.to_string();
        handle.update(cx, |s, cx| s.set_value(value, window, cx));
    }

    /// 选项 / 注释子页的常驻编辑器（[填充因子, 表空间, 表注释]）
    fn spawn_option_editors(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        let vals: [String; 5] = {
            let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
                return;
            };
            let ds = tab.dstate.as_ref().expect("设计页签必有状态");
            [
                ds.options.owner.clone(),
                ds.options.tablespace.clone(),
                ds.options.inherit.clone(),
                ds.options.fillfactor.clone(),
                ds.options.cluster.clone(),
            ]
        };
        for f in 0..5usize {
            let exists = {
                let ds = self
                    .opened
                    .iter()
                    .find(|t| t.did == did)
                    .and_then(|t| t.dstate.as_ref())
                    .expect("设计页签必有状态");
                ds.opt_edits[f].is_some()
            };
            if exists {
                continue;
            }
            let val = vals[f].clone();
            let state = cx.new(|cx| InputState::new(window, cx));
            state.update(cx, |s, cx| s.set_value(val, window, cx));
            cx.subscribe(&state, move |this, input, ev: &InputEvent, cx| {
                let InputEvent::Change = ev else { return };
                let Some(tab) = this.opened.iter_mut().find(|t| t.did == did) else {
                    return;
                };
                let Some(ds) = tab.dstate.as_mut() else { return };
                let text = input.read(cx).value().to_string();
                match f {
                    0 => ds.options.owner = text,
                    1 => ds.options.tablespace = text,
                    2 => ds.options.inherit = text,
                    3 => ds.options.fillfactor = text,
                    _ => ds.options.cluster = text,
                }
                ds.note = None;
                cx.notify();
            })
            .detach();
            let ds = self
                .opened
                .iter_mut()
                .find(|t| t.did == did)
                .and_then(|t| t.dstate.as_mut())
                .expect("设计页签必有状态");
            ds.opt_edits[f] = Some(state);
        }
    }

    /// 检查子页的常驻编辑器（每行 [名称, 表达式, 注释]；表达式也可走独立小窗）
    fn spawn_check_editors(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
            return;
        };
        let specs: Vec<(usize, usize, String, &'static str)> = tab
            .dstate
            .as_ref()
            .expect("设计页签必有状态")
            .checks
            .iter()
            .enumerate()
            .flat_map(|(r, ck)| {
                [
                    (0usize, ck.name.clone(), ""),
                    (1, ck.value.clone(), ""),
                    (2, ck.comment.clone(), ""),
                ]
                .map(move |(f, v, ph)| (r, f, v, ph))
            })
            .collect();
        for (r, f, val, ph) in specs {
            {
                let ds = self
                    .opened
                    .iter_mut()
                    .find(|t| t.did == did)
                    .and_then(|t| t.dstate.as_mut())
                    .expect("设计页签必有状态");
                while ds.check_edits.len() < ds.checks.len() {
                    ds.check_edits.push(Default::default());
                }
                if r < ds.check_edits.len() && ds.check_edits[r][f].is_some() {
                    continue;
                }
            }
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(ph.to_string()));
            state.update(cx, |s, cx| s.set_value(val, window, cx));
            cx.subscribe(&state, move |this, input, ev: &InputEvent, cx| {
                let InputEvent::Change = ev else { return };
                let Some(tab) = this.opened.iter_mut().find(|t| t.did == did) else {
                    return;
                };
                let Some(ds) = tab.dstate.as_mut() else { return };
                let text = input.read(cx).value().to_string();
                if let Some(ck) = ds.checks.get_mut(r) {
                    match f {
                        0 => ck.name = text,
                        1 => ck.value = text,
                        _ => ck.comment = text,
                    }
                    ds.note = None;
                    cx.notify();
                }
            })
            .detach();
            let ds = self
                .opened
                .iter_mut()
                .find(|t| t.did == did)
                .and_then(|t| t.dstate.as_mut())
                .expect("设计页签必有状态");
            ds.check_edits[r][f] = Some(state);
        }
    }

    /// 检查子页的当前行，越界时夹回最后一行。
    fn design_check_sel(ds: &DesignerState) -> usize {
        ds.check_sel.min(ds.checks.len().saturating_sub(1))
    }

    /// 点检查表格的某一格：换当前行 / 当前列（无继承格点勾选框即切换）。
    fn design_pick_check_cell(&mut self, did: u64, row: usize, col: usize, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.check_sel = row;
            ds.check_cur = col;
            cx.notify();
        }
    }

    /// 切换检查的「无继承」
    fn design_toggle_check_no_inherit(
        &mut self,
        did: u64,
        row: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(ck) = ds.checks.get_mut(row)
        {
            ck.no_inherit = !ck.no_inherit;
            ds.note = None;
            cx.notify();
        }
    }

    fn design_add_check(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.checks.push(DesignPair::default());
            ds.check_edits.clear();
            ds.check_sel = ds.checks.len() - 1;
            ds.note = None;
        }
        cx.notify();
        self.spawn_check_editors(did, window, cx);
        self.spawn_option_editors(did, window, cx);
        self.spawn_exclude_editors(did, window, cx);
        self.spawn_rule_editors(did, window, cx);
        self.spawn_trigger_editors(did, window, cx);
    }

    fn design_remove_check(
        &mut self,
        did: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && row < ds.checks.len()
        {
            ds.checks.remove(row);
            ds.check_edits.clear();
            // 空行常驻（参考工具口径）：删光后补一行空白种子行
            if ds.checks.is_empty() {
                ds.checks.push(DesignPair::default());
            }
            ds.check_sel = ds.check_sel.min(ds.checks.len() - 1);
            ds.note = None;
        }
        cx.notify();
        self.spawn_check_editors(did, window, cx);
    }

    /// 表头分割线按下：记下起始位置与该列当前宽
    fn design_resize_begin(
        &mut self,
        did: u64,
        sub: usize,
        col: usize,
        start_x: f32,
        cur_w: f32,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = t.dstate.as_mut()
        {
            d.resize = Some((sub, col, start_x, cur_w));
            cx.notify();
        }
    }

    /// 表头分割线拖动：按位移改列宽（40..=480）
    fn design_resize_move(&mut self, did: u64, x: f32, cx: &mut Context<Self>) {
        if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = t.dstate.as_mut()
            && let Some((sub, col, start_x, start_w)) = d.resize
        {
            let w = (start_w + (x - start_x)).clamp(40., 480.);
            d.widths.insert((sub, col), w);
            cx.notify();
        }
    }

    fn design_resize_end(&mut self, did: u64, cx: &mut Context<Self>) {
        if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = t.dstate.as_mut()
            && d.resize.take().is_some()
        {
            cx.notify();
        }
    }

    fn design_popup_toggle(&mut self, did: u64, ix: usize, cx: &mut Context<Self>) {
        if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = t.dstate.as_mut()
            && let Some(p) = d.popup.as_mut()
            && let Some((_, on)) = p.items.get_mut(ix)
        {
            *on = !*on;
            p.sel = ix;
            cx.notify();
        }
    }

    /// ↑ / ↓：在候选列表里挪一行（`delta` = -1 / +1）
    fn design_popup_move(&mut self, did: u64, delta: isize, cx: &mut Context<Self>) {
        if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = t.dstate.as_mut()
            && let Some(p) = d.popup.as_mut()
            && !p.items.is_empty()
        {
            let from = p.sel.min(p.items.len() - 1);
            let to = (from as isize + delta).clamp(0, p.items.len() as isize - 1) as usize;
            if from != to {
                p.items.swap(from, to);
                p.sel = to;
            }
            cx.notify();
        }
    }

    /// −：把选中的列从索引 / 唯一键里摘掉（取消勾选）
    fn design_popup_remove(&mut self, did: u64, cx: &mut Context<Self>) {
        if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = t.dstate.as_mut()
            && let Some(p) = d.popup.as_mut()
            && let Some((_, on)) = p.items.get_mut(p.sel)
        {
            *on = false;
            cx.notify();
        }
    }

    fn design_popup_cancel(&mut self, did: u64, cx: &mut Context<Self>) {
        if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = t.dstate.as_mut()
        {
            d.popup = None;
            cx.notify();
        }
    }

    /// 确定：把勾选的列按列表顺序拼成 `a, b, c` 写回模型
    fn design_popup_commit(
        &mut self,
        did: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(t) = self.opened.iter_mut().find(|t| t.did == did) else {
            return;
        };
        let Some(d) = t.dstate.as_mut() else { return };
        let Some(p) = d.popup.take() else { return };
        // 文本域面板：外键「注释」写回 fks[row].comment，检查约束写回 checks[row].value
        if let Some(state) = p.expr.clone() {
            let text = state.read(cx).value().to_string();
            let fk_comment = p.target == PickTarget::FkComment;
            if fk_comment {
                if let Some(f) = d.fks.get_mut(p.row) {
                    f.comment = text.clone();
                }
            } else if let Some(c) = d.checks.get_mut(p.row) {
                c.value = text.clone();
            }
            d.note = None;
            cx.notify();
            drop(d);
            if fk_comment {
                self.design_sync_fk_editor(did, p.row, 4, &text, window, cx);
            }
            return;
        }
        let joined = p
            .items
            .iter()
            .filter(|(_, on)| *on)
            .map(|(n, _)| n.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let mut synced: Option<(usize, String)> = None;
        match p.target {
            PickTarget::IndexCols => {
                // 勾上的列按列表顺序组成索引字段；已有字段的六项参数保留，
                // 新勾的按列类型推默认值（int8 → int8_ops、varchar → text_ops …）。
                let names: Vec<String> = p
                    .items
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(n, _)| n.clone())
                    .collect();
                let old: Vec<IndexField> = d
                    .indexes
                    .get(p.row)
                    .map(|i| i.fields.clone())
                    .unwrap_or_default();
                let mut fields: Vec<IndexField> = Vec::new();
                for name in &names {
                    if let Some(keep) = old.iter().find(|f| f.name == *name) {
                        fields.push(keep.clone());
                    } else {
                        let ty = d
                            .cols
                            .iter()
                            .find(|c| c.name == *name)
                            .map(|c| c.ty.clone())
                            .unwrap_or_default();
                        fields.push(IndexField::make(name, &ty));
                    }
                }
                if let Some(idx) = d.indexes.get_mut(p.row) {
                    idx.fields = fields;
                }
            }
            PickTarget::FkCols => {
                if let Some(f) = d.fks.get_mut(p.row) {
                    f.columns = joined.clone();
                    synced = Some((1, joined.clone()));
                }
            }
            PickTarget::FkRefCols => {
                if let Some(f) = d.fks.get_mut(p.row) {
                    f.ref_columns = joined.clone();
                    synced = Some((3, joined.clone()));
                }
            }
            PickTarget::UniqueCols => {
                if let Some(u) = d.uniques.get_mut(p.row) {
                    u.value = joined.clone();
                    synced = Some((1, joined.clone()));
                }
            }
            PickTarget::TrigFields => {
                if let Some(t) = d.triggers.get_mut(p.row) {
                    t.update_fields = joined.clone();
                }
            }
            PickTarget::InheritTables => {
                d.options.inherit = joined;
            }
            PickTarget::ExcludeElems => {
                if let Some(e) = d.excludes.get_mut(p.row) {
                    e.elements = joined;
                }
            }
            _ => {}
        }
        d.note = None;
        cx.notify();
        drop(d);
        if let Some((f, v)) = synced {
            self.design_sync_fk_editor(did, p.row, f, &v, window, cx);
        }
    }

    /// 浮层子编辑器（「…」面板）：白底 + 1px 边框，表头 / 勾选列表 / 上移下移删除 + 确定取消
    fn design_popup_panel(
        &self,
        did: u64,
        ds: &DesignerState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(popup) = ds.popup.as_ref() else {
            return div().into_any_element();
        };
        let p = self.pal();
        let (fg, muted, border, chrome) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.border),
            to_hsla(p.chrome),
        );
        let hover = to_hsla(p.hover);
        let accent = to_hsla(p.accent);
        if let Some(state) = popup.expr.clone() {
            let footer = h_flex()
                .h(px(40.))
                .flex_none()
                .items_center()
                .justify_end()
                .px(px(8.))
                .gap(px(6.))
                .border_t_1()
                .border_color(border)
                .child(
                    crate::ui::primary_button("dpopup-ok", "确定", self.dark, cx).on_click(
                        cx.listener(move |this, _, window, cx| {
                        this.design_popup_commit(did, window, cx)
                    }),
                    ),
                )
                .child(
                    crate::ui::secondary_button("dpopup-cancel", "取消").on_click(
                        cx.listener(move |this, _, _, cx| this.design_popup_cancel(did, cx)),
                    ),
                );
            let (ex_left, ex_top, _ew) = match popup.target.grid_anchor() {
                Some((sub, col)) => popup_anchor_for(ds, sub, col, popup.row),
                None => (96., 58., 420.),
            };
            return div()
                .id("dpopup")
                .test_support()
                .absolute()
                .left(px(ex_left))
                .top(px(ex_top))
                .w(px(420.))
                .flex()
                .flex_col()
                .bg(to_hsla(p.background))
                .border_1()
                .border_color(border)
                .shadow_lg()
                .child(
                    div()
                        .p(px(8.))
                        .child(
                            div()
                                .h(px(if popup.target == PickTarget::FkComment {
                                    300.
                                } else {
                                    140.
                                }))
                                .w_full()
                                .rounded(px(2.))
                                .border_1()
                                .border_color(border)
                                .p(px(4.))
                                .child(
                                    Textarea::new(&state)
                                        .appearance(false)
                                        .bordered(false)
                                        .h_full()
                                        .text_size(px(12.)),
                                ),
                        ),
                )
                .child(footer)
                .into_any_element();
        }
        // 索引的「字段」面板：左起空白格（勾选框，表头无字）+ 名称 + 参考工具那六列
        // 列参数。勾上的列就是索引的字段，顺序按列表顺序（↑↓ 可调）。
        let is_index = popup.target == PickTarget::IndexCols;
        // 网格样式的勾选面板（外键 / 唯一键）：表头勾选列整格浅蓝 + 「名称」、
        // 底栏没有 −（参考图交互）
        let is_fk_check = matches!(
            popup.target,
            PickTarget::FkCols | PickTarget::FkRefCols | PickTarget::UniqueCols
        );
        // 普通下拉（参考图点 ▼ 出的无边饰列表）：锚在格子正下方，点一项即写回
        if popup.target.is_plain() {
            let (left, top, w) = match popup.target.grid_anchor() {
                Some((sub, col)) => popup_anchor_for(ds, sub, col, popup.row),
                None => (96., 58., 200.),
            };
            let rows = popup.items.iter().enumerate().map(|(ix, (name, _))| {
                let selected = popup.sel == ix;
                h_flex()
                    .id(rid(&format!("dpopup/{ix}")))
                    .w_full()
                    .h(px(22.))
                    .flex_none()
                    .items_center()
                    .px(px(6.))
                    .when(selected, |s| s.bg(to_hsla(p.selection)))
                    .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.design_popup_pick_plain(did, ix, window, cx)
                    }))
                    .test_support()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(fg)
                            .child(name.clone()),
                    )
                    .into_any_element()
            });
            return div()
                .id("dpopup")
                .test_support()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(w.max(120.)))
                .flex()
                .flex_col()
                .bg(to_hsla(p.background))
                .border_1()
                .border_color(border)
                .shadow_lg()
                .child(
                    v_flex()
                        .id("dpopup-list")
                        .max_h(px(160.))
                        .overflow_y_scrollbar()
                        .children(rows),
                )
                .into_any_element();
        }
        let idx_fields: Vec<IndexField> = ds
            .indexes
            .get(popup.row)
            .map(|i| i.fields.clone())
            .unwrap_or_default();
        let idx_cols: [(&str, f32); 8] = [
            ("", 22.),
            ("名称", 104.),
            ("排序规则模式", 100.),
            ("排序规则", 70.),
            ("运算符类别模式", 92.),
            ("运算符类别", 74.),
            ("排序顺序", 70.),
            ("Nulls 排序", 160.),
        ];
        let head = if is_index {
            h_flex()
                .h(px(26.))
                .flex_none()
                .bg(to_hsla(p.background))
                .children(idx_cols.iter().map(|(t, w)| {
                    div()
                        .w(px(*w))
                        .h_full()
                        .flex_none()
                        .flex()
                        .items_center()
                        .px(px(5.))
                        .border_r_1()
                        .border_b_1()
                        .border_color(border)
                        .child(div().text_size(px(11.)).text_color(muted).child(*t))
                }))
                .into_any_element()
        } else if is_fk_check {
            // 外键勾选面板的表头是网格样式（参考图）：勾选列整格浅蓝 + 「名称」
            // 白底黑字，DCDCDC 格线 + 底边线
            h_flex()
                .h(px(28.))
                .flex_none()
                .bg(to_hsla(p.background))
                .border_b_1()
                .border_color(border)
                .child(
                    div()
                        .w(px(6.))
                        .h_full()
                        .flex_none()
                        .border_r_1()
                        .border_color(border),
                )
                .child(
                    div()
                        .w(px(36.))
                        .h_full()
                        .flex_none()
                        .border_r_1()
                        .border_color(border)
                        .bg(to_hsla(blend(p.accent, p.background, 0.74))),
                )
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .flex()
                        .items_center()
                        .px(px(6.))
                        .child(div().text_size(px(12.)).text_color(fg).child("名称")),
                )
                .into_any_element()
        } else {
            h_flex()
                .h(px(26.))
                .flex_none()
                .items_center()
                .px(px(6.))
                .gap(px(6.))
                .bg(chrome)
                .border_b_1()
                .border_color(border)
                .child(
                    div()
                        .w(px(170.))
                        .text_size(px(12.))
                        .text_color(muted)
                        .child(popup.target.title()),
                )
                .into_any_element()
        };

        let body = if is_fk_check {
            // 参考图的行结构：小行首槽（选中行浅蓝 + 强调条）/ 勾选列 / 名称列，
            // 行间 F0F0F0 分隔线；选中行不铺整行浅蓝
            v_flex()
                .id("dpopup-list")
                .flex_1()
                .min_h_0()
                .max_h(px(301.))
                .overflow_y_scrollbar()
                .children(popup.items.iter().enumerate().map(|(ix, (name, on))| {
                    let selected = popup.sel == ix;
                    h_flex()
                        .id(rid(&format!("dpopup/{ix}")))
                        .h(px(23.))
                        .flex_none()
                        .border_b_1()
                        .border_color(to_hsla(p.border_soft))
                        .cursor_pointer()
                        .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.design_popup_toggle(did, ix, cx)
                        }))
                        .test_support()
                        .child(
                            div()
                                .w(px(6.))
                                .h_full()
                                .flex_none()
                                .relative()
                                .border_r_1()
                                .border_color(to_hsla(p.border_soft))
                                .when(selected, |s| {
                                    s.bg(to_hsla(p.selection)).child(
                                        div()
                                            .absolute()
                                            .right_0()
                                            .top_0()
                                            .bottom_0()
                                            .w(px(3.))
                                            .bg(to_hsla(D_FK_BAR)),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .w(px(36.))
                                .h_full()
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .border_r_1()
                                .border_color(to_hsla(p.border_soft))
                                .child(
                                    div()
                                        .size(px(14.))
                                        .rounded(px(2.))
                                        .border_1()
                                        .border_color(if *on {
                                            to_hsla(p.accent)
                                        } else {
                                            to_hsla(p.subtle_foreground)
                                        })
                                        .when(*on, |s| s.bg(to_hsla(p.accent)))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(*on, |s| {
                                            s.child(
                                                Icon::new(IconName::Check)
                                                    .with_size(px(10.))
                                                    .text_color(to_hsla(0xffffff)),
                                            )
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .h_full()
                                .flex()
                                .items_center()
                                .px(px(6.))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(fg)
                                        .child(name.clone()),
                                ),
                        )
                        .into_any_element()
                }))
                .into_any_element()
        } else {
        v_flex()
            .id("dpopup-list")
            .flex_1()
            .min_h_0()
            .max_h(px(if is_index { 260. } else { 300. }))
            .overflow_y_scrollbar()
            .children(popup.items.iter().enumerate().map(|(ix, (name, on))| {
                let selected = popup.sel == ix;
                // 勾上的列：把它的六项参数显示出来（没勾就留空）
                let field = idx_fields
                    .iter()
                    .find(|f| f.name == *name)
                    .filter(|_| *on);
                let meta = [
                    field.map(|f| f.collate_schema.clone()).unwrap_or_default(),
                    field.map(|f| f.collation.clone()).unwrap_or_default(),
                    field.map(|f| f.opclass_schema.clone()).unwrap_or_default(),
                    field.map(|f| f.opclass.clone()).unwrap_or_default(),
                    field.map(|f| f.sort.clone()).unwrap_or_default(),
                    field.map(|f| f.nulls.clone()).unwrap_or_default(),
                ];
                let check = div()
                    .size(px(13.))
                    .flex_none()
                    .rounded(px(2.))
                    .border_1()
                    .border_color(if *on { accent } else { muted })
                    .bg(if *on { accent } else { to_hsla(p.background) })
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(*on, |s| {
                        s.child(
                            Icon::new(IconName::Check)
                                .with_size(px(10.))
                                .text_color(to_hsla(0xffffff)),
                        )
                    });
                if is_index {
                    h_flex()
                        .id(rid(&format!("dpopup/{ix}")))
                        .h(px(22.))
                        .flex_none()
                        .relative()
                        .when(selected, |s| s.bg(to_hsla(p.selection)))
                        .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.design_popup_toggle(did, ix, cx)
                        }))
                        .test_support()
                        .when(selected, |s| {
                            s.child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(2.))
                                    .bg(accent),
                            )
                        })
                        .child(
                            div()
                                .w(px(idx_cols[0].1))
                                .h_full()
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .border_r_1()
                                .border_b_1()
                                .border_color(to_hsla(p.border_soft))
                                .child(check),
                        )
                        .child(
                            div()
                                .w(px(idx_cols[1].1))
                                .h_full()
                                .flex_none()
                                .flex()
                                .items_center()
                                .px(px(5.))
                                .border_r_1()
                                .border_b_1()
                                .border_color(to_hsla(p.border_soft))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(fg)
                                        .child(name.clone()),
                                ),
                        )
                        .children(meta.iter().enumerate().map(|(k, v)| {
                            div()
                                .w(px(idx_cols[k + 2].1))
                                .h_full()
                                .flex_none()
                                .flex()
                                .items_center()
                                .px(px(5.))
                                .border_r_1()
                                .border_b_1()
                                .border_color(to_hsla(p.border_soft))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(muted)
                                        .child(v.clone()),
                                )
                        }))
                        .into_any_element()
                } else {
                    h_flex()
                        .id(rid(&format!("dpopup/{ix}")))
                        .h(px(22.))
                        .flex_none()
                        .items_center()
                        .px(px(6.))
                        .gap(px(6.))
                        .when(selected, |s| s.bg(to_hsla(p.selection)))
                        .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.design_popup_toggle(did, ix, cx)
                        }))
                        .test_support()
                        .child(check)
                        .child(
                            div()
                                .w(px(150.))
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(fg)
                                .child(name.clone()),
                        )
                        .into_any_element()
                }
            }))
        .into_any_element()
        };

        let icon_btn = |id: &'static str, icon: IconName, color: Hsla| {
            h_flex()
                .id(rid(id))
                .size(px(22.))
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(2.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(Icon::new(icon).with_size(px(13.)).text_color(color))
        };

        let footer = h_flex()
            .h(px(40.))
            .flex_none()
            .items_center()
            .px(px(8.))
            .gap(px(2.))
            .border_t_1()
            .border_color(border)
            .child(
                icon_btn("dpopup-up", IconName::ArrowUp, muted)
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| this.design_popup_move(did, -1, cx))),
            )
            .child(
                icon_btn("dpopup-down", IconName::ArrowDown, muted)
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| this.design_popup_move(did, 1, cx))),
            )
            .when(!is_fk_check, |s| {
                s.child(
                    icon_btn("dpopup-del", IconName::Minus, to_hsla(0xd9534f))
                        .test_support()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.design_popup_remove(did, cx)
                        })),
                )
            })
            .child(div().flex_1())
            .child(
                crate::ui::primary_button("dpopup-ok", "确定", self.dark, cx).on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.design_popup_commit(did, window, cx)
                    }),
                ),
            )
            .child(div().w(px(6.)))
            .child(
                crate::ui::secondary_button("dpopup-cancel", "取消").on_click(
                    cx.listener(move |this, _, _, cx| this.design_popup_cancel(did, cx)),
                ),
            );

        let (panel_left, panel_top, panel_w) = if is_index {
            let name_w = ds
                .widths
                .get(&(1usize, 0))
                .copied()
                .unwrap_or(D_INDEX_WIDTHS[0]);
            (
                D_GUTTER_W + name_w + 6.,
                D_TOOLBAR_H
                    + D_TABBAR_H
                    + D_HEADER_H
                    + (popup.row + 1) as f32 * D_ROW_EXTENT
                    + 3.,
                700.,
            )
        } else if let Some((sub, col)) = popup.target.grid_anchor() {
            let (l, t, _w) = popup_anchor_for(ds, sub, col, popup.row);
            (l, t, 420.)
        } else {
            (96., 58., 420.)
        };
        div()
            .id("dpopup")
            .test_support()
            .absolute()
            .left(px(panel_left))
            .top(px(panel_top))
            .w(px(panel_w))
            .flex()
            .flex_col()
            .bg(to_hsla(p.background))
            .border_1()
            .border_color(border)
            .shadow_lg()
            .child(head)
            .child(body)
            .child(footer)
            .into_any_element()
    }

    /// 子页底部的属性区（参考工具每个子页底下都有一组「标签 + 控件」）
    fn design_prop_panel(
        &self,
        rows: Vec<(&'static str, AnyElement)>,
        dark: bool,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (muted, border, bg) = (
            to_hsla(p.muted_foreground),
            to_hsla(p.border),
            to_hsla(p.background),
        );
        v_flex()
            .flex_none()
            .py(px(6.))
            .border_t_1()
            .border_color(border)
            .bg(bg)
            .children(rows.into_iter().map(|(label, field)| {
                h_flex()
                    .h(px(D_PROP_ROW_H))
                    .flex_none()
                    .items_center()
                    .pl(px(D_PROP_PAD_L))
                    .child(
                        div()
                            .w(px(D_PROP_LABEL_W))
                            .text_size(px(12.))
                            .text_color(muted)
                            .child(label),
                    )
                    .child(field)
            }))
            .into_any_element()
    }

    /// 属性区里「空着的输入框 / 下拉框」：参考工具未接元数据时就是这个样子
    fn design_prop_field(&self, w: f32, text: &str, dark: bool) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, border) = (to_hsla(p.foreground), to_hsla(p.border));
        h_flex()
            .h(px(20.))
            .w(px(w))
            .flex_none()
            .px(px(5.))
            .items_center()
            .gap(px(4.))
            .bg(to_hsla(p.background))
            .border_1()
            .border_color(border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(fg)
                    .child(text.to_string()),
            )
            .child(Icon::new(IconName::ChevronDown).with_size(px(9.)).text_color(to_hsla(p.subtle_foreground)))
            .into_any_element()
    }

    /// 下拉单元格：整个格子就是一个下拉按钮（左对齐的值 + 右缘小箭头），
    /// 点格子里任何地方都弹菜单。参考工具这些列（索引方法 / 表空间 / 排序规则…）
    /// 都是「格子 + 小箭头」的形态。
    fn drop_cell(
        &self,
        id: String,
        text: String,
        w: f32,
        h: f32,
        dark: bool,
        enabled: bool,
        caret: bool,
        menu: impl Fn(PopupMenu) -> PopupMenu + 'static,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, muted, subtle) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
        );
        Button::new(rid(&id))
            .ghost()
            .with_size(gpui_kit::component::Size::XSmall)
            .w(px(w))
            .h(px(h))
            .px(px(3.))
            .when(!enabled, |b| b.disabled(true))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(if enabled { fg } else { muted })
                            .child(text),
                    )
                    .when(caret, |s| {
                        s.child(
                            Icon::new(IconName::ChevronDown)
                                .with_size(px(9.))
                                .text_color(if enabled { subtle } else { muted }),
                        )
                    }),
            )
            .dropdown_menu(move |m, _, _| menu(m))
            .into_any_element()
    }

    /// 索引子页（照参考工具 1:1）：索引表格（名称 / 字段 / 索引方法 / 唯一键 /
    /// 并发 / 注释）+ 底部的索引属性（表空间 … 约束）。点「字段」格弹七列浮层。
    fn indexes_tab(
        &self,
        did: u64,
        ds: &DesignerState,
        is_mysql: bool,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, muted, subtle, border, soft, bg, accent, selection) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.background),
            to_hsla(p.accent),
            to_hsla(p.selection),
        );
        let paper = to_hsla(0xffffff);
        // 表头「当前列」底色：和数据页同一口径（亮色往白里调，暗色往底色调）
        let head_cur = to_hsla(blend(p.accent, p.background, 0.74));
        let sel = Self::design_index_sel(ds);
        let cur = ds.index_cur.min(5);
        let me = cx.entity().clone();
        // 列宽：默认宽 + 用户拖过表头分割线后的宽（按子页 1 + 列序号存）
        let iw = |ix: usize| ds.widths.get(&(1usize, ix)).copied().unwrap_or(D_INDEX_WIDTHS[ix]);
        let total_w: f32 = D_GUTTER_W + (0..6).map(iw).sum::<f32>();
        let headers = ["名称", "字段", "索引方法", "唯一键", "并发", "注释"];

        let head = h_flex()
            .h(px(D_HEADER_H))
            .flex_none()
            .bg(bg)
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some(did) = this.did_of_active_design() {
                    this.design_resize_move(did, ev.position.x.as_f32(), cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(did) = this.did_of_active_design() {
                        this.design_resize_end(did, cx);
                    }
                }),
            )
            .child(
                // 行首空白槽：表头这一格空着，点行里的槽 = 选中整行
                div()
                    .w(px(D_GUTTER_W))
                    .h(px(D_HEADER_H))
                    .flex_none()
                    .border_r_1()
                    .border_b_1()
                    .border_color(border),
            )
            .children(headers.iter().enumerate().map(|(i, h)| {
                let cur_w = iw(i);
                div()
                    .w(px(cur_w))
                    .h(px(D_HEADER_H))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(border)
                    .when(i == cur, |s| {
                        s.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .bg(head_cur)
                                .border_b_2()
                                .border_color(accent),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(7.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(div().text_size(px(12.)).text_color(muted).child(*h)),
                    )
                    .child(
                        div()
                            .id(rid(&format!("idx-resize/{i}")))
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(5.))
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    this.design_resize_begin(
                                        did,
                                        1,
                                        i,
                                        ev.position.x.as_f32(),
                                        cur_w,
                                        cx,
                                    );
                                }),
                            ),
                    )
            }));

        // 名称 / 注释：常驻输入框（点格选中，编辑器直接可敲）
        let input_cell = |cx: &mut Context<Self>,
                          r: usize,
                          anchor: bool,
                          col: usize,
                          handle: Option<Entity<InputState>>,
                          f: usize,
                          placeholder: &'static str,
                          value: &str|
         -> AnyElement {
            let current = anchor && col == cur;
            let editor = match handle {
                Some(e) => div()
                    .absolute()
                    .left(px(4.))
                    .right(px(2.))
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&e)
                                .id(rid(&format!("idx-input/{r}/{f}")))
                                .h(px(18.))
                                .appearance(false)
                                .bordered(false)
                                .text_size(px(12.)),
                        ),
                    )
                    .into_any_element(),
                None => div()
                    .absolute()
                    .left(px(6.))
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(if current { paper } else { subtle })
                            .child(if value.is_empty() {
                                placeholder.to_string()
                            } else {
                                value.to_string()
                            }),
                    )
                    .into_any_element(),
            };
            div()
                .id(rid(&format!("idx-cell/{r}/{col}")))
                .w(px(iw(col)))
                .h(px(D_ROW_EXTENT))
                .flex_none()
                .relative()
                .when(anchor, |s| s.bg(selection))
                .when(current, |s| s.bg(accent))
                .border_r_1()
                .border_b_1()
                .border_color(soft)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.design_pick_index_cell(did, r, col, cx)
                }))
                .child(editor)
                .test_support()
                .into_any_element()
        };
        // 唯一键 / 并发：勾选格
        let flag_cell = |cx: &mut Context<Self>,
                         r: usize,
                         anchor: bool,
                         col: usize,
                         on: bool,
                         unique: bool|
         -> AnyElement {
            div()
                .id(rid(&format!("idx-cell/{r}/{col}")))
                .w(px(iw(col)))
                .h(px(D_ROW_EXTENT))
                .flex_none()
                .relative()
                .when(anchor, |s| s.bg(selection))
                .when(anchor && col == cur, |s| s.bg(accent))
                .border_r_1()
                .border_b_1()
                .border_color(soft)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.design_toggle_index_flag(did, r, unique, cx)
                }))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            // 勾选框恒 13：方框是命中目标，不随行高缩（同字段网格）
                            div()
                                .size(px(13.))
                                .rounded(px(2.))
                                .border_1()
                                .border_color(if on { accent } else { subtle })
                                .bg(bg)
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(on, |s| {
                                    s.child(
                                        Icon::new(IconName::Check)
                                            .with_size(px(10.))
                                            .text_color(accent),
                                    )
                                }),
                        ),
                )
                .test_support()
                .into_any_element()
        };

        let mut rows: Vec<AnyElement> = Vec::new();
        for (r, idx) in ds.indexes.iter().enumerate() {
            let anchor = r == sel;
            let edits = ds.index_edits.get(r).cloned().unwrap_or_default();
            // 字段：摘要文本，点一下弹「字段」浮层
            let summary: Vec<String> = idx
                .fields
                .iter()
                .filter(|f| !f.name.trim().is_empty())
                .map(|f| f.summary())
                .collect();
            let fields_cell = div()
                .id(rid(&format!("idx-cell/{r}/1")))
                .w(px(iw(1)))
                .h(px(D_ROW_EXTENT))
                .flex_none()
                .relative()
                .when(anchor, |s| s.bg(selection))
                .when(anchor && cur == 1, |s| s.bg(accent))
                .border_r_1()
                .border_b_1()
                .border_color(soft)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.design_pick_index_cell(did, r, 1, cx);
                    this.design_open_picker(did, PickTarget::IndexCols, r, cx);
                }))
                .child(
                    div()
                        .absolute()
                        .left(px(6.))
                        .right(px(4.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .overflow_hidden()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(if summary.is_empty() {
                                    subtle
                                } else if anchor && cur == 1 {
                                    paper
                                } else {
                                    fg
                                })
                                .child(if summary.is_empty() {
                                    "（无字段）".to_string()
                                } else {
                                    summary.join(", ")
                                }),
                        )
                        // 参考工具那一格右缘的「…」：点它/点格都开字段选择浮层
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(if anchor && cur == 1 {
                                    paper
                                } else {
                                    muted
                                })
                                .child("…"),
                        ),
                )
                .test_support()
                .into_any_element();
            // 索引方法：下拉（参考工具显示 B-Tree / Hash / GiST / GIN / BRIN / SP-GiST）
            let me_m = me.clone();
            let method_cell = self.drop_cell(
                format!("idx-method/{r}"),
                index_method_label(&idx.method),
                iw(2),
                D_ROW_EXTENT,
                dark,
                true,
                true,
                pick_menu(
                    ["B-Tree", "Hash", "GiST", "GIN", "BRIN", "SP-GiST"]
                        .iter()
                        .map(|m| (m.to_string(), m.to_string()))
                        .collect(),
                    Rc::new(move |value: String, app: &mut App| {
                        let me = me_m.clone();
                        me.update(app, move |this, cx| {
                            this.design_set_index_method(did, r, value.clone(), cx)
                        });
                    }),
                ),
            );
            rows.push(
                h_flex()
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .child(
                        // 行首空白槽：点它选中整行（当前行槽里有一条强调条）
                        div()
                            .id(rid(&format!("idx-gutter/{r}")))
                            .w(px(D_GUTTER_W))
                            .h(px(D_ROW_EXTENT))
                            .flex_none()
                            .relative()
                            .border_r_1()
                            .border_b_1()
                            .border_color(border)
                            .when(anchor, |s| {
                                s.bg(selection).child(
                                    div()
                                        .absolute()
                                        .right_0()
                                        .top_0()
                                        .bottom_0()
                                        .w(px(D_ACCENT_BAR_W))
                                        .bg(accent),
                                )
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(ds) = this
                                    .opened
                                    .iter_mut()
                                    .find(|t| t.did == did)
                                    .and_then(|t| t.dstate.as_mut())
                                {
                                    ds.index_sel = r;
                                    cx.notify();
                                }
                            }))
                            .test_support(),
                    )
                    .child(input_cell(cx, r, anchor, 0, edits[0].clone(), 0, "索引名", &idx.name))
                    .child(fields_cell)
                    .child(method_cell)
                    .child(flag_cell(cx, r, anchor, 3, idx.unique, true))
                    .child(flag_cell(cx, r, anchor, 4, idx.concurrent, false))
                    .child(input_cell(cx, r, anchor, 5, edits[1].clone(), 1, "", &idx.comment))
                    .into_any_element(),
            );
        }

        let grid = v_flex().child(head).children(rows);
        v_flex()
            .size_full()
            .bg(bg)
            .child(
                div()
                    .id("idx-grid-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    // 滚动条包一层：外层留 id / test-support，内层管滚动
                    .child(
                        div()
                            .id("idx-grid-inner")
                            .size_full()
                            .overflow_scrollbar()
                            
                            .child(div().w(px(total_w)).child(grid)),
                    ),
            )
            .child(
                div()
                    .h(px(D_PROP_SPLITTER_H))
                    .flex_none()
                    .bg(to_hsla(p.chrome)),
            )
            .child(self.index_props(did, ds, sel, dark, cx))
            .into_any_element()
    }

    /// 索引子页底部的索引属性（参考工具那七行：表空间 / 填充因子 / 正在缓冲 /
    /// 快速更新 / 待处理列表限制 / 每范围页数 / 约束）。都是**选中索引**的属性。
    fn index_props(
        &self,
        did: u64,
        ds: &DesignerState,
        sel: usize,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (muted, subtle, border, soft, bg) = (
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.background),
        );
        let me = cx.entity().clone();
        let idx = ds.indexes.get(sel);
        let edits = ds.index_edits.get(sel).cloned().unwrap_or_default();
        // 缓冲类参数只对 GiST / GIN 有意义：参考工具在 B-Tree 索引下把这两格灰掉
        let method_key = idx
            .map(|i| i.method.trim().to_ascii_lowercase().replace(['-', ' ', '_'], ""))
            .unwrap_or_default();
        let bufferable = matches!(method_key.as_str(), "gist" | "gin");

        // 空 / 禁用时的占位框：参考工具未接元数据时就是这个样子
        let flat = |text: &str| -> AnyElement {
            div()
                .w(px(D_PROP_FIELD_W))
                .h(px(20.))
                .flex_none()
                .px(px(4.))
                .flex()
                .items_center()
                .rounded(px(2.))
                .border_1()
                .border_color(soft)
                .bg(bg)
                .child(div().text_size(px(12.)).text_color(subtle).child(text.to_string()))
                .into_any_element()
        };
        let prop_input = |handle: Option<Entity<InputState>>,
                          f: usize,
                          text: &str,
                          enabled: bool|
         -> AnyElement {
            let Some(handle) = handle.filter(|_| enabled) else {
                return flat(text);
            };
            div()
                .w(px(D_PROP_FIELD_W))
                .h(px(20.))
                .flex_none()
                .px(px(3.))
                .flex()
                .items_center()
                .rounded(px(2.))
                .border_1()
                .border_color(soft)
                .bg(bg)
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&handle)
                            .id(rid(&format!("idx-prop-input/{sel}/{f}")))
                            .h(px(18.))
                            .appearance(false)
                            .bordered(false)
                            .text_size(px(12.)),
                    ),
                )
                .into_any_element()
        };
        let combo = |id: String,
                     text: String,
                     enabled: bool,
                     values: Vec<(String, String)>,
                     prop: u8|
         -> AnyElement {
            let me = me.clone();
            // 参考工具里下拉也是「带边框的格子」：和输入框同一个壳
            div()
                .w(px(D_PROP_FIELD_W))
                .h(px(20.))
                .flex_none()
                .rounded(px(2.))
                .border_1()
                .border_color(soft)
                .bg(bg)
                .child(self.drop_cell(
                    id,
                    text,
                    D_PROP_FIELD_W - 2.,
                    18.,
                    dark,
                    enabled && idx.is_some(),
                    true,
                    pick_menu(
                        values,
                        Rc::new(move |value: String, app: &mut App| {
                            let me = me.clone();
                            me.update(app, move |this, cx| {
                                this.design_set_index_prop(did, sel, prop, value.clone(), cx)
                            });
                        }),
                    ),
                ))
                .into_any_element()
        };

        let space = match idx {
            Some(i) => combo(
                format!("idx-prop-ts/{sel}"),
                i.tablespace.clone(),
                true,
                vec![
                    ("（默认）".to_string(), String::new()),
                    ("pg_default".to_string(), "pg_default".to_string()),
                    ("pg_global".to_string(), "pg_global".to_string()),
                ],
                0,
            ),
            None => flat(""),
        };
        let buffer_values = || {
            vec![
                ("（默认）".to_string(), String::new()),
                ("ON".to_string(), "ON".to_string()),
                ("OFF".to_string(), "OFF".to_string()),
                ("AUTO".to_string(), "AUTO".to_string()),
            ]
        };
        let fill = match idx {
            Some(i) => prop_input(edits[2].clone(), 2, &i.fillfactor, true),
            None => flat(""),
        };
        let buffering = match idx {
            Some(i) => combo(
                format!("idx-prop-buf/{sel}"),
                i.buffering.clone(),
                bufferable,
                buffer_values(),
                1,
            ),
            None => flat(""),
        };
        let fastupdate = match idx {
            Some(i) => combo(
                format!("idx-prop-fu/{sel}"),
                i.fastupdate.clone(),
                bufferable,
                buffer_values(),
                2,
            ),
            None => flat(""),
        };
        let pending = match idx {
            Some(i) => prop_input(edits[3].clone(), 3, &i.pending_list_limit, true),
            None => flat(""),
        };
        let pages = match idx {
            Some(i) => prop_input(edits[4].clone(), 4, &i.pages_per_range, true),
            None => flat(""),
        };
        // 约束：输入框 + 「…」选择器（候选 = 主键 / 唯一键名；mock 档只记录）
        let mut constraint_names: Vec<(String, String)> =
            vec![("（无）".to_string(), String::new())];
        if ds.cols.iter().any(|c| c.pk) {
            constraint_names.push(("PRIMARY".to_string(), "PRIMARY".to_string()));
        }
        for u in &ds.uniques {
            if !u.name.trim().is_empty() {
                constraint_names.push((u.name.clone(), u.name.clone()));
            }
        }
        let me_c = me.clone();
        let constraint_picker = Button::new(rid(&format!("idx-prop-ck/{sel}")))
            .label("...")
            .h(px(20.))
            .w(px(22.))
            .px(px(0.))
            .disabled(idx.is_none())
            .dropdown_menu(move |m, _, _| {
                let me = me_c.clone();
                pick_menu(
                    constraint_names.clone(),
                    Rc::new(move |value: String, app: &mut App| {
                        let me = me.clone();
                        me.update(app, move |this, cx| {
                            this.design_set_index_prop(did, sel, 3, value.clone(), cx)
                        });
                    }),
                )(m)
            });
        let constraint = match idx {
            Some(i) => h_flex()
                .gap(px(8.))
                .items_center()
                .child(prop_input(edits[5].clone(), 5, &i.constraint, true))
                .child(constraint_picker)
                .into_any_element(),
            None => h_flex()
                .gap(px(8.))
                .items_center()
                .child(flat(""))
                .child(constraint_picker)
                .into_any_element(),
        };
        v_flex()
            .flex_none()
            .py(px(6.))
            .border_t_1()
            .border_color(border)
            .bg(bg)
            .id(rid("idx-props"))
            .test_support()
            .children(
                [
                    ("表空间：", space),
                    ("填充因子 (%)：", fill),
                    ("正在缓冲：", buffering),
                    ("快速更新：", fastupdate),
                    ("待处理列表限制：", pending),
                    ("每范围页数：", pages),
                    ("约束：", constraint),
                ]
                .into_iter()
                .map(|(label, control)| {
                    h_flex()
                        .h(px(D_PROP_ROW_H))
                        .flex_none()
                        .items_center()
                        .pl(px(D_PROP_PAD_L))
                        .child(
                            div()
                                .w(px(D_PROP_LABEL_W))
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(muted)
                                .child(label),
                        )
                        .child(control)
                        .into_any_element()
                }),
            )
            .into_any_element()
    }

    /// 外键子页的常驻编辑器（每行 [名称, 引用表, 注释]；字段 / 被引用的字段 /
    /// 删除时 / 更新时走浮层与下拉，不在这里）。
    fn spawn_fk_editors(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
            return;
        };
        let specs: Vec<(usize, usize, String, &'static str)> = tab
            .dstate
            .as_ref()
            .expect("设计页签必有状态")
            .fks
            .iter()
            .enumerate()
            .flat_map(|(r, fk)| {
                [
                    (0usize, fk.name.clone(), ""),
                    (1, fk.columns.clone(), ""),
                    (2, fk.ref_table.clone(), ""),
                    (3, fk.ref_columns.clone(), ""),
                    (4, fk.comment.clone(), ""),
                ]
                .map(move |(f, v, ph)| (r, f, v, ph))
            })
            .collect();
        for (r, f, val, ph) in specs {
            {
                let ds = self
                    .opened
                    .iter_mut()
                    .find(|t| t.did == did)
                    .and_then(|t| t.dstate.as_mut())
                    .expect("设计页签必有状态");
                while ds.fk_edits.len() < ds.fks.len() {
                    ds.fk_edits.push(Default::default());
                }
                if r < ds.fk_edits.len() && ds.fk_edits[r][f].is_some() {
                    continue;
                }
            }
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(ph.to_string()));
            state.update(cx, |s, cx| s.set_value(val, window, cx));
            cx.subscribe(&state, move |this, input, ev: &InputEvent, cx| {
                let InputEvent::Change = ev else { return };
                let text = input.read(cx).value().to_string();
                // 引用表输入联动下拉候选（参考图：输入 + 下拉过滤是同一格）——
                // 先在不可变借用里把候选算好，再进 opened 的可变借用写回
                let popup_refresh = (f == 2)
                    .then(|| {
                        this.opened
                            .iter()
                            .find(|t| t.did == did)
                            .and_then(|t| t.dstate.as_ref())
                            .and_then(|d| d.popup.as_ref())
                            .filter(|p| p.target == PickTarget::FkRefTable && p.row == r)
                            .map(|_| {
                                let needle = text.trim().to_ascii_lowercase();
                                let tables = this.fk_ref_tables(did);
                                if needle.is_empty() {
                                    tables
                                } else {
                                    tables
                                        .into_iter()
                                        .filter(|n| n.to_ascii_lowercase().contains(&needle))
                                        .collect()
                                }
                            })
                    })
                    .flatten();
                let Some(tab) = this.opened.iter_mut().find(|t| t.did == did) else {
                    return;
                };
                let Some(ds) = tab.dstate.as_mut() else { return };
                if let Some(fk) = ds.fks.get_mut(r) {
                    match f {
                        0 => fk.name = text.clone(),
                        1 => fk.columns = text.clone(),
                        2 => fk.ref_table = text.clone(),
                        3 => fk.ref_columns = text.clone(),
                        _ => fk.comment = text.clone(),
                    }
                    ds.note = None;
                    if let Some(list) = popup_refresh
                        && let Some(p) = ds.popup.as_mut()
                    {
                        p.items = list.into_iter().map(|n| (n, false)).collect();
                        p.sel = 0;
                    }
                    cx.notify();
                }
            })
            .detach();
            let ds = self
                .opened
                .iter_mut()
                .find(|t| t.did == did)
                .and_then(|t| t.dstate.as_mut())
                .expect("设计页签必有状态");
            ds.fk_edits[r][f] = Some(state);
        }
    }

    /// 唯一键子页的常驻编辑器（每行 [名称, 字段, 注释]；字段也可走浮层勾选）
    fn spawn_unique_editors(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
            return;
        };
        let specs: Vec<(usize, usize, String, &'static str)> = tab
            .dstate
            .as_ref()
            .expect("设计页签必有状态")
            .uniques
            .iter()
            .enumerate()
            .flat_map(|(r, uk)| {
                [
                    (0usize, uk.name.clone(), ""),
                    (1, uk.value.clone(), ""),
                    (2, uk.comment.clone(), ""),
                ]
                .map(move |(f, v, ph)| (r, f, v, ph))
            })
            .collect();
        for (r, f, val, ph) in specs {
            {
                let ds = self
                    .opened
                    .iter_mut()
                    .find(|t| t.did == did)
                    .and_then(|t| t.dstate.as_mut())
                    .expect("设计页签必有状态");
                while ds.unique_edits.len() < ds.uniques.len() {
                    ds.unique_edits.push(Default::default());
                }
                if r < ds.unique_edits.len() && ds.unique_edits[r][f].is_some() {
                    continue;
                }
            }
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(ph.to_string()));
            state.update(cx, |s, cx| s.set_value(val, window, cx));
            cx.subscribe(&state, move |this, input, ev: &InputEvent, cx| {
                let InputEvent::Change = ev else { return };
                let Some(tab) = this.opened.iter_mut().find(|t| t.did == did) else {
                    return;
                };
                let Some(ds) = tab.dstate.as_mut() else { return };
                let text = input.read(cx).value().to_string();
                if let Some(uk) = ds.uniques.get_mut(r) {
                    match f {
                        0 => uk.name = text,
                        1 => uk.value = text,
                        _ => uk.comment = text,
                    }
                    ds.note = None;
                    cx.notify();
                }
            })
            .detach();
            let ds = self
                .opened
                .iter_mut()
                .find(|t| t.did == did)
                .and_then(|t| t.dstate.as_mut())
                .expect("设计页签必有状态");
            ds.unique_edits[r][f] = Some(state);
        }
    }

    /// 唯一键子页的当前行，越界时夹回最后一行（删唯一键后常见）。
    fn design_unique_sel(ds: &DesignerState) -> usize {
        ds.unique_sel.min(ds.uniques.len().saturating_sub(1))
    }

    /// 点唯一键表格的某一格：换当前行 / 当前列。
    fn design_pick_unique_cell(
        &mut self,
        did: u64,
        row: usize,
        col: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.unique_sel = row;
            ds.unique_cur = col;
            cx.notify();
        }
    }

    fn design_add_unique(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.uniques.push(DesignPair::default());
            ds.unique_edits.clear();
            ds.unique_sel = ds.uniques.len() - 1;
            ds.note = None;
        }
        cx.notify();
        self.spawn_unique_editors(did, window, cx);
        self.spawn_check_editors(did, window, cx);
        self.spawn_option_editors(did, window, cx);
        self.spawn_exclude_editors(did, window, cx);
        self.spawn_rule_editors(did, window, cx);
        self.spawn_trigger_editors(did, window, cx);
    }

    fn design_remove_unique(
        &mut self,
        did: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && row < ds.uniques.len()
        {
            ds.uniques.remove(row);
            ds.unique_edits.clear();
            // 空行常驻（参考工具口径）：删光后补一行空白种子行
            if ds.uniques.is_empty() {
                ds.uniques.push(DesignPair::default());
            }
            ds.unique_sel = ds.unique_sel.min(ds.uniques.len() - 1);
            ds.note = None;
        }
        cx.notify();
        self.spawn_unique_editors(did, window, cx);
    }

    fn design_add_fk(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.fks.push(DesignForeignKey::default());
            ds.fk_edits.clear();
            ds.fk_sel = ds.fks.len() - 1;
            ds.note = None;
        }
        cx.notify();
        self.spawn_fk_editors(did, window, cx);
        self.spawn_unique_editors(did, window, cx);
        self.spawn_check_editors(did, window, cx);
        self.spawn_option_editors(did, window, cx);
        self.spawn_exclude_editors(did, window, cx);
        self.spawn_rule_editors(did, window, cx);
        self.spawn_trigger_editors(did, window, cx);
    }

    fn design_remove_fk(
        &mut self,
        did: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && row < ds.fks.len()
        {
            ds.fks.remove(row);
            ds.fk_edits.clear();
            // 空行常驻（参考工具口径）：删光后补一行空白种子行
            if ds.fks.is_empty() {
                ds.fks.push(DesignForeignKey::default());
            }
            ds.fk_sel = ds.fk_sel.min(ds.fks.len() - 1);
            ds.note = None;
        }
        cx.notify();
        self.spawn_fk_editors(did, window, cx);
    }

    /// 外键子页（照参考工具 1:1）：行首槽 + 八列表格（名称 / 字段 / 被引用的模式 /
    /// 被引用的表 / 被引用的字段 / 删除时 / 更新时 / 注释）+ 底部「符合全部 / 可延迟 /
    /// 延迟」属性区。锚点行只有行首槽铺浅蓝（参考图整行不铺色），当前格深蓝反白；
    /// 「字段 / 被引用的字段」点格弹列选择浮层，「删除时 / 更新时」整格下拉。
    fn fks_tab(
        &self,
        did: u64,
        ds: &DesignerState,
        _is_mysql: bool,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, muted, subtle, border, soft, bg, accent, selection) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.background),
            to_hsla(p.accent),
            to_hsla(p.selection),
        );
        let paper = to_hsla(0xffffff);
        let hover = to_hsla(p.hover);
        // 表头「当前列」底色：和数据页同一口径（亮色往白里调，暗色往底色调）
        let head_cur = to_hsla(blend(p.accent, p.background, 0.74));
        let sel = Self::design_fk_sel(ds);
        let cur = ds.fk_cur.min(7);
        let me = cx.entity().clone();
        // 列宽：默认宽 + 用户拖过表头分割线后的宽（按子页 2 + 列序号存）
        let cw = |ix: usize| ds.widths.get(&(2usize, ix)).copied().unwrap_or(D_FK_WIDTHS[ix]);
        let total_w = D_GUTTER_W + (0..8).map(cw).sum::<f32>();
        let headers = [
            "名称",
            "字段",
            "被引用的模式",
            "被引用的表",
            "被引用的字段",
            "删除时",
            "更新时",
            "注释",
        ];

        // 行首槽：锚点行铺浅蓝 + 右缘 2px 强调条（参考取样 #4DA0E1），表头槽不铺
        let gutter = move |h: f32, anchor: bool| {
            div()
                .w(px(D_GUTTER_W))
                .h(px(h))
                .flex_none()
                .relative()
                .border_r_1()
                .border_b_1()
                .border_color(border)
                .when(anchor, |s| {
                    s.bg(selection).child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(D_ACCENT_BAR_W))
                            .bg(to_hsla(D_FK_BAR)),
                    )
                })
        };

        // 表头：白底黑字，当前列铺淡蓝（上沿内缩 5）+ 底部 2px 强调条
        let head = h_flex()
            .h(px(D_HEADER_H))
            .flex_none()
            .bg(bg)
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some(did) = this.did_of_active_design() {
                    this.design_resize_move(did, ev.position.x.as_f32(), cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(did) = this.did_of_active_design() {
                        this.design_resize_end(did, cx);
                    }
                }),
            )
            .child(gutter(D_HEADER_H, false))
            .children(headers.iter().enumerate().map(|(i, h)| {
                let cur_w = cw(i);
                div()
                    .w(px(cur_w))
                    .h(px(D_HEADER_H))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(border)
                    .when(i == cur, |s| {
                        // 当前格所在列：表头整格浅蓝 + 底部 2px 强调条（参考图同款）
                        s.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .bg(head_cur),
                        )
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(D_ACCENT_BAR_W))
                                .bg(to_hsla(D_FK_BAR)),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(6.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(div().text_size(px(12.)).text_color(fg).child(*h)),
                    )
                    .child(
                        div()
                            .id(rid(&format!("fk-resize/{i}")))
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(5.))
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    this.design_resize_begin(
                                        did,
                                        2,
                                        i,
                                        ev.position.x.as_f32(),
                                        cur_w,
                                        cx,
                                    );
                                }),
                            ),
                    )
            }))
            // 网格右侧的空白：表头条一直铺到窗口右缘（参考图注释列贴边）
            .child(
                div()
                    .flex_1()
                    .h(px(D_HEADER_H))
                    .border_b_1()
                    .border_color(border),
            );

        let mut rows: Vec<AnyElement> = Vec::new();
        for (r, fk) in ds.fks.iter().enumerate() {
            let anchor = r == sel;
            let edits = ds.fk_edits.get(r).cloned().unwrap_or_default();
            let blank = design_fk_blank(fk);

            // 浮层入口按钮（参考图：只有当前格显示 —— 列选择/注释是 …，下拉是 ▼）
            let afford_btn = |col: usize, a: FkAfford| -> AnyElement {
                let me_a = me.clone();
                let glyph: AnyElement = match a {
                    FkAfford::Drop(_) => Icon::new(IconName::ChevronDown)
                        .with_size(px(9.))
                        .text_color(muted)
                        .into_any_element(),
                    _ => div().text_size(px(10.)).text_color(fg).child("…").into_any_element(),
                };
                h_flex()
                    .id(rid(&format!("fk-afford/{r}/{col}")))
                    .absolute()
                    .right(px(3.))
                    .top(px(2.))
                    .w(px(20.))
                    .h(px(16.))
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(2.))
                    .border_1()
                    .border_color(soft)
                    .bg(bg)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(move |_, window, cx| {
                        me_a.update(cx, move |this, cx| match a {
                            FkAfford::More(t) | FkAfford::Drop(t) => {
                                this.design_open_picker(did, t, r, cx)
                            }
                            FkAfford::Note => this.design_open_fk_comment(did, r, window, cx),
                        })
                    })
                    .test_support()
                    .child(glyph)
                    .into_any_element()
            };

            // 文本格（名称 / 字段 / 被引用的表 / 被引用的字段 / 注释）：常驻输入框，
            // 当前格右缘带浮层入口（… 或 ▼）
            let input_cell = |col: usize,
                              f: usize,
                              handle: Option<Entity<InputState>>,
                              value: &str,
                              afford: Option<FkAfford>| -> AnyElement {
                let current = anchor && col == cur;
                let editor = match handle {
                    Some(e) => div()
                        .absolute()
                        .left(px(4.))
                        .right(px(if current && afford.is_some() { 26. } else { 2. }))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&e)
                                    .id(rid(&format!("fk-input/{r}/{f}")))
                                    .h(px(18.))
                                    .appearance(false)
                                    .bordered(false)
                                    .text_size(px(12.)),
                            ),
                        )
                        .into_any_element(),
                    None => div()
                        .absolute()
                        .left(px(6.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(if current { paper } else { subtle })
                                .child(value.to_string()),
                        )
                        .into_any_element(),
                };
                let mut cell = div()
                    .id(rid(&format!("fk-cell/{r}/{col}")))
                    .w(px(cw(col)))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .relative()
                    .when(current, |s| s.bg(accent))
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_pick_fk_cell(did, r, col, cx)
                    }))
                    .child(editor);
                if current
                    && let Some(a) = afford
                {
                    cell = cell.child(afford_btn(col, a));
                }
                cell.test_support().into_any_element()
            };
            // 展示格（被引用的模式 / 删除时 / 更新时）：当前格带 ▼ 下拉入口；
            // 空白种子行的动作列不显示默认值（参考图那行是全空的）
            let text_cell = |col: usize, value: &str, afford: FkAfford| -> AnyElement {
                let current = anchor && col == cur;
                let shown = if blank && col >= 5 { "" } else { value };
                let mut cell = div()
                    .id(rid(&format!("fk-cell/{r}/{col}")))
                    .w(px(cw(col)))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .relative()
                    .when(current, |s| s.bg(accent))
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_pick_fk_cell(did, r, col, cx)
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(px(6.))
                            .right(px(if current { 26. } else { 4. }))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .overflow_hidden()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(if current {
                                        paper
                                    } else if shown.is_empty() {
                                        subtle
                                    } else {
                                        muted
                                    })
                                    .child(shown.to_string()),
                            ),
                    );
                if current {
                    cell = cell.child(afford_btn(col, afford));
                }
                cell.test_support().into_any_element()
            };

            rows.push(
                h_flex()
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .child({
                        let g = gutter(D_ROW_EXTENT, anchor);
                        div()
                            .id(rid(&format!("fk-gutter/{r}")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.design_pick_fk_cell(did, r, 0, cx)
                            }))
                            .child(g)
                            .test_support()
                    })
                    .child(input_cell(0, 0, edits[0].clone(), &fk.name, None))
                    .child(input_cell(
                        1,
                        1,
                        edits[1].clone(),
                        &fk.columns,
                        Some(FkAfford::More(PickTarget::FkCols)),
                    ))
                    .child(text_cell(2, &fk.ref_schema, FkAfford::Drop(PickTarget::FkSchema)))
                    .child(input_cell(
                        3,
                        2,
                        edits[2].clone(),
                        &fk.ref_table,
                        Some(FkAfford::Drop(PickTarget::FkRefTable)),
                    ))
                    .child(input_cell(
                        4,
                        3,
                        edits[3].clone(),
                        &fk.ref_columns,
                        Some(FkAfford::More(PickTarget::FkRefCols)),
                    ))
                    .child(text_cell(5, &fk.on_delete, FkAfford::Drop(PickTarget::FkOnDelete)))
                    .child(text_cell(6, &fk.on_update, FkAfford::Drop(PickTarget::FkOnUpdate)))
                    .child(input_cell(
                        7,
                        4,
                        edits[4].clone(),
                        &fk.comment,
                        Some(FkAfford::Note),
                    ))
                    // 网格右侧的空白：行底一直铺到窗口右缘
                    .child(
                        div()
                            .flex_1()
                            .h(px(D_ROW_EXTENT))
                            .border_b_1()
                            .border_color(soft),
                    )
                    .into_any_element(),
            );
        }

        // 参考图：子页签条与表头之间有一条 ~6 逻辑像素的白缝（属于网格的顶边距）
        let grid = v_flex()
            .child(div().h(px(6.)).flex_none().bg(bg))
            .child(head)
            .children(rows);
        v_flex()
            .size_full()
            .bg(bg)
            .child(
                div()
                    .id("fk-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    // 滚动条包一层：外层留 id / test-support，内层管滚动
                    .child(
                        div()
                            .size_full()
                            .overflow_scrollbar()
                            
                            .child(div().w(px(total_w)).child(grid)),
                    ),
            )
            .child(
                div()
                    .h(px(D_PROP_SPLITTER_H))
                    .flex_none()
                    .bg(to_hsla(p.chrome)),
            )
            .child(self.fk_props(dark))
            .into_any_element()
    }

    /// 唯一键子页（照参考工具 1:1）：三列表格（名称 / 字段 / 注释）+ 底部四行属性
    /// （表空间 / 填充因子 / 可延迟 / 延迟）。当前格是白底 + 2px 强调边
    /// （参考图字段格聚焦态），字段格当前态带 … 浮层入口。
    fn uniques_tab(
        &self,
        did: u64,
        ds: &DesignerState,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, subtle, border, soft, bg, accent, selection) = (
            to_hsla(p.foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.background),
            to_hsla(p.accent),
            to_hsla(p.selection),
        );
        let hover = to_hsla(p.hover);
        let head_cur = to_hsla(blend(p.accent, p.background, 0.74));
        let sel = Self::design_unique_sel(ds);
        let cur = ds.unique_cur.min(2);
        let me = cx.entity().clone();
        // 列宽：默认宽 + 用户拖过表头分割线后的宽（按子页 3 + 列序号存）
        let uw = |ix: usize| ds.widths.get(&(3usize, ix)).copied().unwrap_or(D_UNIQUE_WIDTHS[ix]);
        let total_w = D_GUTTER_W + (0..3).map(uw).sum::<f32>();
        let headers = ["名称", "字段", "注释"];

        // 行首槽：锚点行铺浅蓝 + 右缘 2px 强调条（参考取样 #4DA0E1），表头槽不铺
        let gutter = move |h: f32, anchor: bool| {
            div()
                .w(px(D_GUTTER_W))
                .h(px(h))
                .flex_none()
                .relative()
                .border_r_1()
                .border_b_1()
                .border_color(border)
                .when(anchor, |s| {
                    s.bg(selection).child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(D_ACCENT_BAR_W))
                            .bg(to_hsla(D_FK_BAR)),
                    )
                })
        };

        // 表头：白底黑字，当前列整格浅蓝 + 底部 2px 强调条
        let head = h_flex()
            .h(px(D_HEADER_H))
            .flex_none()
            .bg(bg)
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some(did) = this.did_of_active_design() {
                    this.design_resize_move(did, ev.position.x.as_f32(), cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(did) = this.did_of_active_design() {
                        this.design_resize_end(did, cx);
                    }
                }),
            )
            .child(gutter(D_HEADER_H, false))
            .children(headers.iter().enumerate().map(|(i, h)| {
                let cur_w = uw(i);
                div()
                    .w(px(cur_w))
                    .h(px(D_HEADER_H))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(border)
                    .when(i == cur, |s| {
                        s.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .bg(head_cur),
                        )
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(D_ACCENT_BAR_W))
                                .bg(to_hsla(D_FK_BAR)),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(6.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(div().text_size(px(12.)).text_color(fg).child(*h)),
                    )
                    .child(
                        div()
                            .id(rid(&format!("un-resize/{i}")))
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(5.))
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    this.design_resize_begin(
                                        did,
                                        3,
                                        i,
                                        ev.position.x.as_f32(),
                                        cur_w,
                                        cx,
                                    );
                                }),
                            ),
                    )
            }))
            .child(
                div()
                    .flex_1()
                    .h(px(D_HEADER_H))
                    .border_b_1()
                    .border_color(border),
            );

        let mut rows: Vec<AnyElement> = Vec::new();
        for (r, uk) in ds.uniques.iter().enumerate() {
            let anchor = r == sel;
            let edits = ds.unique_edits.get(r).cloned().unwrap_or_default();

            // 浮层入口按钮（…，仅当前格显示）
            let afford_btn = |col: usize| -> AnyElement {
                let me_a = me.clone();
                h_flex()
                    .id(rid(&format!("un-afford/{r}/{col}")))
                    .absolute()
                    .right(px(3.))
                    .top(px(2.))
                    .w(px(20.))
                    .h(px(16.))
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(2.))
                    .border_1()
                    .border_color(soft)
                    .bg(bg)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(move |_, _, cx| {
                        me_a.update(cx, move |this, cx| {
                            this.design_open_picker(did, PickTarget::UniqueCols, r, cx)
                        })
                    })
                    .test_support()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(fg)
                            .child("…")
                            .into_any_element(),
                    )
                    .into_any_element()
            };
            // 文本格：常驻输入框；当前格是白底 + 2px 强调边（参考图聚焦态），文字深色
            let input_cell = |col: usize,
                              f: usize,
                              handle: Option<Entity<InputState>>,
                              value: &str,
                              afford: bool|
             -> AnyElement {
                let current = anchor && col == cur;
                let editor = match handle {
                    Some(e) => div()
                        .absolute()
                        .left(px(4.))
                        .right(px(if current && afford { 26. } else { 2. }))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&e)
                                    .id(rid(&format!("un-input/{r}/{f}")))
                                    .h(px(18.))
                                    .appearance(false)
                                    .bordered(false)
                                    .text_size(px(12.)),
                            ),
                        )
                        .into_any_element(),
                    None => div()
                        .absolute()
                        .left(px(6.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(if current { fg } else { subtle })
                                .child(value.to_string()),
                        )
                        .into_any_element(),
                };
                let mut cell = div()
                    .id(rid(&format!("un-cell/{r}/{col}")))
                    .w(px(uw(col)))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .when(current, |s| s.border_2().border_color(accent))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_pick_unique_cell(did, r, col, cx)
                    }))
                    .child(editor);
                if current && afford {
                    cell = cell.child(afford_btn(col));
                }
                cell.test_support().into_any_element()
            };

            rows.push(
                h_flex()
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .child({
                        let g = gutter(D_ROW_EXTENT, anchor);
                        div()
                            .id(rid(&format!("un-gutter/{r}")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.design_pick_unique_cell(did, r, 0, cx)
                            }))
                            .child(g)
                            .test_support()
                    })
                    .child(input_cell(0, 0, edits[0].clone(), &uk.name, false))
                    .child(input_cell(1, 1, edits[1].clone(), &uk.value, true))
                    .child(input_cell(2, 2, edits[2].clone(), &uk.comment, false))
                    .child(
                        div()
                            .flex_1()
                            .h(px(D_ROW_EXTENT))
                            .border_b_1()
                            .border_color(soft),
                    )
                    .into_any_element(),
            );
        }

        // 参考图：子页签条与表头之间有一条 ~6 逻辑像素的白缝（属于网格的顶边距）
        let grid = v_flex()
            .child(div().h(px(6.)).flex_none().bg(bg))
            .child(head)
            .children(rows);
        v_flex()
            .size_full()
            .bg(bg)
            .child(
                div()
                    .id("un-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    // 滚动条包一层：外层留 id / test-support，内层管滚动
                    .child(
                        div()
                            .size_full()
                            .overflow_scrollbar()
                            .child(div().w(px(total_w)).child(grid)),
                    ),
            )
            .child(
                div()
                    .h(px(D_PROP_SPLITTER_H))
                    .flex_none()
                    .bg(to_hsla(p.chrome)),
            )
            .child(self.unique_props(dark))
            .into_any_element()
    }

    /// 唯一键子页底部四行属性（参考图：表空间 / 填充因子 / 可延迟 / 延迟）。
    /// 可延迟 / 延迟显示默认值（延迟在 NOT DEFERRABLE 下是灰的），均为显示占位，
    /// 接真库后随 pg_constraint 回填。行距 / 标签列 / 控件宽与外键属性区同一套实测密度。
    fn unique_props(&self, dark: bool) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, muted, subtle, soft, bg) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border_soft),
            to_hsla(p.background),
        );
        // 参考取样 #FAFAFA：空下拉的底比内容底灰一档（暗色用铬件底）
        let combo = |value: &str, color: Hsla, caret: bool| -> AnyElement {
            div()
                .w(px(D_FK_PROP_FIELD_W))
                .h(px(20.))
                .flex_none()
                .px(px(5.))
                .flex()
                .items_center()
                .rounded(px(2.))
                .border_1()
                .border_color(soft)
                .bg(if dark { to_hsla(p.chrome) } else { to_hsla(0xfafafa) })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(color)
                        .child(value.to_string()),
                )
                .when(caret, |s| {
                    s.child(
                        Icon::new(IconName::ChevronDown)
                            .with_size(px(9.))
                            .text_color(subtle),
                    )
                })
                .into_any_element()
        };
        let row = |label: &str, field: AnyElement| {
            h_flex()
                .h(px(D_FK_PROP_ROW_H))
                .flex_none()
                .items_center()
                .child(
                    div()
                        .w(px(D_FK_PROP_LABEL_W))
                        .flex_none()
                        .text_size(px(12.))
                        .text_color(muted)
                        .child(label.to_string()),
                )
                .child(field)
        };
        v_flex()
            .h(px(D_FK_PROPS_H))
            .flex_none()
            .bg(bg)
            .pt(px(8.))
            .pl(px(D_PROP_PAD_L))
            .child(row("表空间：", combo("", subtle, true)))
            .child(row("填充因子 (%)：", combo("", subtle, false)))
            .child(row("可延迟：", combo("NOT DEFERRABLE", fg, true)))
            .child(row("延迟：", combo("INITIALLY IMMEDIATE", subtle, true)))
            .into_any_element()
    }

    /// 检查子页（照参考工具 1:1）：四列表格（名称 / 表达式 / 无继承 / 注释）。
    /// 当前格是白底 + 2px 强调边（参考图表达式格聚焦态），表达式格当前态带 …
    /// 浮层入口（独立小窗），无继承格点勾选框即切换。
    fn checks_tab(
        &self,
        did: u64,
        ds: &DesignerState,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, subtle, border, soft, bg, accent, selection) = (
            to_hsla(p.foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.background),
            to_hsla(p.accent),
            to_hsla(p.selection),
        );
        let hover = to_hsla(p.hover);
        let head_cur = to_hsla(blend(p.accent, p.background, 0.74));
        let sel = Self::design_check_sel(ds);
        let cur = ds.check_cur.min(3);
        let me = cx.entity().clone();
        let cw = |ix: usize| ds.widths.get(&(4usize, ix)).copied().unwrap_or(D_CHECK_WIDTHS[ix]);
        let total_w = D_GUTTER_W + (0..4).map(cw).sum::<f32>();
        let headers = ["名称", "表达式", "无继承", "注释"];

        // 行首槽：锚点行铺浅蓝 + 右缘 2px 强调条（参考取样 #4DA0E1），表头槽不铺
        let gutter = move |h: f32, anchor: bool| {
            div()
                .w(px(D_GUTTER_W))
                .h(px(h))
                .flex_none()
                .relative()
                .border_r_1()
                .border_b_1()
                .border_color(border)
                .when(anchor, |s| {
                    s.bg(selection).child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(D_ACCENT_BAR_W))
                            .bg(to_hsla(D_FK_BAR)),
                    )
                })
        };

        let head = h_flex()
            .h(px(D_HEADER_H))
            .flex_none()
            .bg(bg)
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some(did) = this.did_of_active_design() {
                    this.design_resize_move(did, ev.position.x.as_f32(), cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(did) = this.did_of_active_design() {
                        this.design_resize_end(did, cx);
                    }
                }),
            )
            .child(gutter(D_HEADER_H, false))
            .children(headers.iter().enumerate().map(|(i, h)| {
                let cur_w = cw(i);
                div()
                    .w(px(cur_w))
                    .h(px(D_HEADER_H))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(border)
                    .when(i == cur, |s| {
                        s.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .bg(head_cur),
                        )
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(D_ACCENT_BAR_W))
                                .bg(to_hsla(D_FK_BAR)),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(6.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(div().text_size(px(12.)).text_color(fg).child(*h)),
                    )
                    .child(
                        div()
                            .id(rid(&format!("dck-resize/{i}")))
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(5.))
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    this.design_resize_begin(
                                        did,
                                        4,
                                        i,
                                        ev.position.x.as_f32(),
                                        cur_w,
                                        cx,
                                    );
                                }),
                            ),
                    )
            }))
            .child(
                div()
                    .flex_1()
                    .h(px(D_HEADER_H))
                    .border_b_1()
                    .border_color(border),
            );

        let mut rows: Vec<AnyElement> = Vec::new();
        for (r, ck) in ds.checks.iter().enumerate() {
            let anchor = r == sel;
            let edits = ds.check_edits.get(r).cloned().unwrap_or_default();

            // 浮层入口按钮（…，仅当前格显示）
            let afford_btn = |col: usize| -> AnyElement {
                let me_a = me.clone();
                h_flex()
                    .id(rid(&format!("dck-afford/{r}/{col}")))
                    .absolute()
                    .right(px(3.))
                    .top(px(2.))
                    .w(px(20.))
                    .h(px(16.))
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(2.))
                    .border_1()
                    .border_color(soft)
                    .bg(bg)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(move |_, window, cx| {
                        me_a.update(cx, move |this, cx| {
                            this.design_open_check(did, r, window, cx)
                        })
                    })
                    .test_support()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(fg)
                            .child("…")
                            .into_any_element(),
                    )
                    .into_any_element()
            };
            // 文本格：常驻输入框；当前格是白底 + 2px 强调边（参考图聚焦态）
            let input_cell = |col: usize,
                              f: usize,
                              handle: Option<Entity<InputState>>,
                              value: &str,
                              afford: bool|
             -> AnyElement {
                let current = anchor && col == cur;
                let editor = match handle {
                    Some(e) => div()
                        .absolute()
                        .left(px(4.))
                        .right(px(if current && afford { 26. } else { 2. }))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&e)
                                    .id(rid(&format!("dck-input/{r}/{f}")))
                                    .h(px(18.))
                                    .appearance(false)
                                    .bordered(false)
                                    .text_size(px(12.)),
                            ),
                        )
                        .into_any_element(),
                    None => div()
                        .absolute()
                        .left(px(6.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(if current { fg } else { subtle })
                                .child(value.to_string()),
                        )
                        .into_any_element(),
                };
                let mut cell = div()
                    .id(rid(&format!("dck-cell/{r}/{col}")))
                    .w(px(cw(col)))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .when(current, |s| s.border_2().border_color(accent))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_pick_check_cell(did, r, col, cx)
                    }))
                    .child(editor);
                if current && afford {
                    cell = cell.child(afford_btn(col));
                }
                cell.test_support().into_any_element()
            };
            // 无继承：勾选格（点击即切换，勾选框恒 13）
            let flag_cell = div()
                .id(rid(&format!("dck-cell/{r}/2")))
                .w(px(cw(2)))
                .h(px(D_ROW_EXTENT))
                .flex_none()
                .relative()
                .when(anchor && cur == 2, |s| s.border_2().border_color(accent))
                .border_r_1()
                .border_b_1()
                .border_color(soft)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.design_pick_check_cell(did, r, 2, cx);
                    this.design_toggle_check_no_inherit(did, r, cx);
                }))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(13.))
                                .rounded(px(2.))
                                .border_1()
                                .border_color(if ck.no_inherit { accent } else { subtle })
                                .bg(bg)
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(ck.no_inherit, |s| {
                                    s.child(
                                        Icon::new(IconName::Check)
                                            .with_size(px(10.))
                                            .text_color(accent),
                                    )
                                }),
                        ),
                )
                .test_support()
                .into_any_element();

            rows.push(
                h_flex()
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .child({
                        let g = gutter(D_ROW_EXTENT, anchor);
                        div()
                            .id(rid(&format!("dck-gutter/{r}")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.design_pick_check_cell(did, r, 0, cx)
                            }))
                            .child(g)
                            .test_support()
                    })
                    .child(input_cell(0, 0, edits[0].clone(), &ck.name, false))
                    .child(input_cell(1, 1, edits[1].clone(), &ck.value, true))
                    .child(flag_cell)
                    .child(input_cell(3, 2, edits[2].clone(), &ck.comment, false))
                    .child(
                        div()
                            .flex_1()
                            .h(px(D_ROW_EXTENT))
                            .border_b_1()
                            .border_color(soft),
                    )
                    .into_any_element(),
            );
        }

        // 参考图：子页签条与表头之间有一条 ~6 逻辑像素的白缝（属于网格的顶边距）
        let grid = v_flex()
            .child(div().h(px(6.)).flex_none().bg(bg))
            .child(head)
            .children(rows);
        v_flex()
            .size_full()
            .bg(bg)
            .child(
                div()
                    .id("dck-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    // 滚动条包一层：外层留 id / test-support，内层管滚动
                    .child(
                        div()
                            .size_full()
                            .overflow_scrollbar()
                            .child(div().w(px(total_w)).child(grid)),
                    ),
            )
            .into_any_element()
    }

    fn design_add_exclude(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            if let Some(blank) = ds.excludes.iter().position(design_exclude_blank) {
                ds.exclude_sel = blank;
            } else {
                ds.excludes.push(DesignExclude::default());
                ds.exclude_sel = ds.excludes.len() - 1;
            }
            ds.exclude_edits.clear();
            ds.note = None;
        }
        cx.notify();
        self.spawn_exclude_editors(did, window, cx);
        self.spawn_option_editors(did, window, cx);
        self.spawn_rule_editors(did, window, cx);
        self.spawn_trigger_editors(did, window, cx);
    }

    fn design_remove_exclude(
        &mut self,
        did: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && row < ds.excludes.len()
        {
            ds.excludes.remove(row);
            ds.exclude_edits.clear();
            // 空行常驻（参考工具口径）：删光后补一行空白种子行
            if ds.excludes.is_empty() {
                ds.excludes.push(DesignExclude::default());
            }
            ds.exclude_sel = ds.exclude_sel.min(ds.excludes.len() - 1);
            ds.note = None;
        }
        cx.notify();
        self.spawn_exclude_editors(did, window, cx);
    }

    /// 排除子页（照参考工具 1:1）：四列表格（名称 / 索引方法 / 元素 / 注释）+
    /// 底部六行属性（表空间 / 填充因子 / 正在缓冲 / 位置 / 可延迟 / 延迟）。
    /// 当前格是白底 + 2px 强调边，元素格当前态带 … 浮层入口（七列草稿编辑）。
    fn excludes_tab(
        &self,
        did: u64,
        ds: &DesignerState,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, muted, subtle, border, soft, bg, accent, selection) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.background),
            to_hsla(p.accent),
            to_hsla(p.selection),
        );
        let hover = to_hsla(p.hover);
        let head_cur = to_hsla(blend(p.accent, p.background, 0.74));
        let sel = Self::design_exclude_sel(ds);
        let cur = ds.exclude_cur.min(3);
        let me = cx.entity().clone();
        let cw = |ix: usize| ds.widths.get(&(5usize, ix)).copied().unwrap_or(D_EXCLUDE_WIDTHS[ix]);
        let total_w = D_GUTTER_W + (0..4).map(cw).sum::<f32>();
        let headers = ["名称", "索引方法", "元素", "注释"];

        // 行首槽：锚点行铺浅蓝 + 右缘 2px 强调条（参考取样 #4DA0E1），表头槽不铺
        let gutter = move |h: f32, anchor: bool| {
            div()
                .w(px(D_GUTTER_W))
                .h(px(h))
                .flex_none()
                .relative()
                .border_r_1()
                .border_b_1()
                .border_color(border)
                .when(anchor, |s| {
                    s.bg(selection).child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(D_ACCENT_BAR_W))
                            .bg(to_hsla(D_FK_BAR)),
                    )
                })
        };

        let head = h_flex()
            .h(px(D_HEADER_H))
            .flex_none()
            .bg(bg)
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some(did) = this.did_of_active_design() {
                    this.design_resize_move(did, ev.position.x.as_f32(), cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(did) = this.did_of_active_design() {
                        this.design_resize_end(did, cx);
                    }
                }),
            )
            .child(gutter(D_HEADER_H, false))
            .children(headers.iter().enumerate().map(|(i, h)| {
                let cur_w = cw(i);
                div()
                    .w(px(cur_w))
                    .h(px(D_HEADER_H))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(border)
                    .when(i == cur, |s| {
                        s.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .bg(head_cur),
                        )
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(D_ACCENT_BAR_W))
                                .bg(to_hsla(D_FK_BAR)),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(6.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(div().text_size(px(12.)).text_color(fg).child(*h)),
                    )
                    .child(
                        div()
                            .id(rid(&format!("dex-resize/{i}")))
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(5.))
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    this.design_resize_begin(
                                        did,
                                        5,
                                        i,
                                        ev.position.x.as_f32(),
                                        cur_w,
                                        cx,
                                    );
                                }),
                            ),
                    )
            }))
            .child(
                div()
                    .flex_1()
                    .h(px(D_HEADER_H))
                    .border_b_1()
                    .border_color(border),
            );

        let mut rows: Vec<AnyElement> = Vec::new();
        for (r, ex) in ds.excludes.iter().enumerate() {
            let anchor = r == sel;
            let edits = ds.exclude_edits.get(r).cloned().unwrap_or_default();

            // 浮层入口按钮（…，仅当前格显示）
            let afford_btn = |col: usize| -> AnyElement {
                let me_a = me.clone();
                h_flex()
                    .id(rid(&format!("dex-afford/{r}/{col}")))
                    .absolute()
                    .right(px(3.))
                    .top(px(2.))
                    .w(px(20.))
                    .h(px(16.))
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(2.))
                    .border_1()
                    .border_color(soft)
                    .bg(bg)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(move |_, window, cx| {
                        me_a.update(cx, move |this, cx| {
                            this.design_open_exclude_elements(did, r, window, cx)
                        })
                    })
                    .test_support()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(fg)
                            .child("…")
                            .into_any_element(),
                    )
                    .into_any_element()
            };
            // 文本格：常驻输入框；当前格是白底 + 2px 强调边（参考图聚焦态）
            let input_cell = |col: usize,
                              f: usize,
                              handle: Option<Entity<InputState>>,
                              value: &str,
                              afford: bool|
             -> AnyElement {
                let current = anchor && col == cur;
                let editor = match handle {
                    Some(e) => div()
                        .absolute()
                        .left(px(4.))
                        .right(px(if current && afford { 26. } else { 2. }))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&e)
                                    .id(rid(&format!("dex-input/{r}/{f}")))
                                    .h(px(18.))
                                    .appearance(false)
                                    .bordered(false)
                                    .text_size(px(12.)),
                            ),
                        )
                        .into_any_element(),
                    None => div()
                        .absolute()
                        .left(px(6.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(if current { fg } else { subtle })
                                .child(value.to_string()),
                        )
                        .into_any_element(),
                };
                let mut cell = div()
                    .id(rid(&format!("dex-cell/{r}/{col}")))
                    .w(px(cw(col)))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .when(current, |s| s.border_2().border_color(accent))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_pick_exclude_cell(did, r, col, cx)
                    }))
                    .child(editor);
                if current && afford {
                    cell = cell.child(afford_btn(col));
                }
                cell.test_support().into_any_element()
            };
            // 索引方法：整格下拉（展示名 B-Tree / GiST / …，DDL 存 SQL 名）
            let me_m = me.clone();
            let method_label = EXCLUDE_METHODS
                .iter()
                .find(|(_, sql)| *sql == ex.method)
                .map(|(label, _)| label.to_string())
                .unwrap_or_else(|| ex.method.clone());
            let method_cell = self.drop_cell(
                format!("dex-method/{r}"),
                method_label,
                cw(1),
                D_ROW_EXTENT,
                dark,
                true,
                true,
                pick_menu(
                    EXCLUDE_METHODS
                        .iter()
                        .map(|(l, s)| (l.to_string(), s.to_string()))
                        .collect(),
                    Rc::new(move |value: String, app: &mut App| {
                        let me = me_m.clone();
                        me.update(app, move |this, cx| {
                            this.design_set_exclude_method(did, r, value.clone(), cx)
                        });
                    }),
                ),
            );

            rows.push(
                h_flex()
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .child({
                        let g = gutter(D_ROW_EXTENT, anchor);
                        div()
                            .id(rid(&format!("dex-gutter/{r}")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.design_pick_exclude_cell(did, r, 0, cx)
                            }))
                            .child(g)
                            .test_support()
                    })
                    .child(input_cell(0, 0, edits[0].clone(), &ex.name, false))
                    .child(method_cell)
                    .child(input_cell(2, 1, edits[1].clone(), &ex.elements, true))
                    .child(input_cell(3, 2, edits[2].clone(), &ex.comment, false))
                    .child(
                        div()
                            .flex_1()
                            .h(px(D_ROW_EXTENT))
                            .border_b_1()
                            .border_color(soft),
                    )
                    .into_any_element(),
            );
        }

        // 参考图：子页签条与表头之间有一条 ~6 逻辑像素的白缝（属于网格的顶边距）
        let grid = v_flex()
            .child(div().h(px(6.)).flex_none().bg(bg))
            .child(head)
            .children(rows);
        v_flex()
            .size_full()
            .bg(bg)
            .child(
                div()
                    .id("dex-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    // 滚动条包一层：外层留 id / test-support，内层管滚动
                    .child(
                        div()
                            .size_full()
                            .overflow_scrollbar()
                            .child(div().w(px(total_w)).child(grid)),
                    ),
            )
            .child(
                div()
                    .h(px(D_PROP_SPLITTER_H))
                    .flex_none()
                    .bg(to_hsla(p.chrome)),
            )
            .child(self.exclude_props(did, ds, dark, cx))
            .into_any_element()
    }

    /// 排除「元素」浮层（参考工具那块七列可编辑表格）：锚在元素格正下方，
    /// 草稿在 [DesignerState::exclude_editor]，确定才写回 elements。
    fn exclude_elements_panel(
        &self,
        did: u64,
        ds: &DesignerState,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(ed) = ds.exclude_editor.as_ref() else {
            return div().into_any_element();
        };
        let p = Palette::of(dark);
        let (fg, muted, subtle, border, soft, bg, accent, hover) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.background),
            to_hsla(p.accent),
            to_hsla(p.hover),
        );
        let head_cur = to_hsla(blend(p.accent, p.background, 0.74));
        let (left, top, _w) = popup_anchor_for(ds, 5, 2, ed.row);
        let cols: [(&str, f32); 7] = [
            ("元素", 220.),
            ("运算符类别模式", 125.),
            ("运算符类别", 95.),
            ("排序", 75.),
            ("Nulls 排序", 95.),
            ("运算符模式", 75.),
            ("运算符", 75.),
        ];
        let head = h_flex()
            .h(px(28.))
            .flex_none()
            .bg(bg)
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .w(px(6.))
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(border),
            )
            .child(
                div()
                    .w(px(36.))
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(border)
                    .bg(head_cur),
            )
            .children(cols.iter().map(|(h, w)| {
                div()
                    .w(px(*w))
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(6.))
                    .border_r_1()
                    .border_color(border)
                    .child(div().text_size(px(12.)).text_color(fg).child(*h))
            }))
            .child(div().w(px(16.)).flex_none());

        let rows = ed.rows.iter().enumerate().map(|(ix, row)| {
            let selected = ed.sel == ix;
            let cells = cols
                .iter()
                .enumerate()
                .map(|(f, (_, w))| {
                    let input = ed.inputs.get(ix).and_then(|s| s.get(f)).cloned().flatten();
                    let editor = match input {
                        Some(e) => div()
                            .size_full()
                            .flex()
                            .items_center()
                            .child(
                                Input::new(&e)
                                    .id(rid(&format!("excl-input/{ix}/{f}")))
                                    .h(px(20.))
                                    .appearance(false)
                                    .bordered(false)
                                    .text_size(px(12.)),
                            )
                            .into_any_element(),
                        None => div().into_any_element(),
                    };
                    div()
                        .w(px(*w))
                        .h_full()
                        .flex_none()
                        .border_r_1()
                        .border_color(soft)
                        .child(editor)
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            h_flex()
                .id(rid(&format!("excl/{ix}")))
                .h(px(27.))
                .flex_none()
                .border_b_1()
                .border_color(soft)
                .when(selected, move |s| s.bg(to_hsla(p.selection)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(t) = this.opened.iter_mut().find(|t| t.did == did)
                        && let Some(d) = t.dstate.as_mut()
                        && let Some(ed) = d.exclude_editor.as_mut()
                    {
                        ed.sel = ix;
                        cx.notify();
                    }
                }))
                .test_support()
                .child(
                    div()
                        .w(px(6.))
                        .h_full()
                        .flex_none()
                        .relative()
                        .border_r_1()
                        .border_color(soft)
                        .when(selected, |s| {
                            s.bg(to_hsla(p.selection)).child(
                                div()
                                    .absolute()
                                    .right_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(3.))
                                    .bg(to_hsla(D_FK_BAR)),
                            )
                        }),
                )
                .child(
                    div()
                        .w(px(36.))
                        .h_full()
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .border_r_1()
                        .border_color(soft)
                        .when(selected, |s| s.bg(accent))
                        .child(
                            div()
                                .size(px(13.))
                                .rounded(px(2.))
                                .border_1()
                                .border_color(if row.on { to_hsla(0xffffff) } else { subtle })
                                .bg(if row.on { accent } else { bg })
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(row.on, |s| {
                                    s.child(
                                        Icon::new(IconName::Check)
                                            .with_size(px(10.))
                                            .text_color(to_hsla(0xffffff)),
                                    )
                                }),
                        ),
                )
                .children(cells)
                .into_any_element()
        });

        let icon_btn = |id: &'static str, label: &'static str, color: Hsla| {
            h_flex()
                .id(rid(id))
                .w(px(24.))
                .h(px(22.))
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(2.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(div().text_size(px(13.)).text_color(color).child(label))
        };
        let footer = h_flex()
            .h(px(40.))
            .flex_none()
            .items_center()
            .px(px(8.))
            .gap(px(6.))
            .border_t_1()
            .border_color(border)
            .child(
                icon_btn("excl-up", "↑", muted)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.exclude_editor_move(did, -1, cx)
                    })),
            )
            .child(
                icon_btn("excl-down", "↓", muted)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.exclude_editor_move(did, 1, cx)
                    })),
            )
            .child(div().w(px(6.)))
            .child(
                icon_btn("excl-add", "+", fg)
                    .test_support()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.exclude_editor_add(did, window, cx)
                    })),
            )
            .child(
                icon_btn("excl-del", "−", to_hsla(0xd9534f))
                    .test_support()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.exclude_editor_remove(did, window, cx)
                    })),
            )
            .child(div().flex_1())
            .child(
                ui::primary_button("excl-ok", "确定", dark, cx).on_click(cx.listener(
                    move |this, _, window, cx| this.exclude_editor_commit(did, window, cx),
                )),
            )
            .child(div().w(px(6.)))
            .child(
                ui::secondary_button("excl-cancel", "取消").on_click(cx.listener(
                    move |this, _, window, cx| {
                        if let Some(t) = this
                            .opened
                            .iter_mut()
                            .find(|t| t.did == did)
                            .and_then(|t| t.dstate.as_mut())
                        {
                            t.exclude_editor = None;
                        }
                        cx.notify();
                    },
                )),
            );

        div()
            .id("excl-panel")
            .test_support()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(850.))
            .flex()
            .flex_col()
            .bg(bg)
            .border_1()
            .border_color(border)
            .shadow_lg()
            .child(head)
            .child(
                v_flex()
                    .id("excl-list")
                    .max_h(px(340.))
                    .overflow_y_scrollbar()
                    .children(rows),
            )
            .child(footer)
            .into_any_element()
    }

    /// 排除子页底部六行属性（参考图：表空间 / 填充因子 / 正在缓冲 / 位置 /
    /// 可延迟 / 延迟）。正在缓冲在 B-Tree 下灰掉；「位置」是谓词（PG WHERE …），
    /// 走独立小窗；可延迟 / 延迟为显示占位。行距 / 标签列 / 控件宽与外键属性区同款。
    fn exclude_props(
        &self,
        did: u64,
        ds: &DesignerState,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, muted, subtle, soft, bg) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border_soft),
            to_hsla(p.background),
        );
        let hover = to_hsla(p.hover);
        let sel = Self::design_exclude_sel(ds);
        let method = ds
            .excludes
            .get(sel)
            .map(|e| e.method.trim().to_ascii_lowercase())
            .unwrap_or_default();
        // 正在缓冲只对 GiST 有意义（参考图 B-Tree 下灰掉）
        let buffering_on = method == "gist";
        let combo = |value: &str, color: Hsla, caret: bool| -> AnyElement {
            div()
                .w(px(D_FK_PROP_FIELD_W))
                .h(px(20.))
                .flex_none()
                .px(px(5.))
                .flex()
                .items_center()
                .rounded(px(2.))
                .border_1()
                .border_color(soft)
                .bg(if dark { to_hsla(p.chrome) } else { to_hsla(0xfafafa) })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(color)
                        .child(value.to_string()),
                )
                .when(caret, |s| {
                    s.child(
                        Icon::new(IconName::ChevronDown)
                            .with_size(px(9.))
                            .text_color(subtle),
                    )
                })
                .into_any_element()
        };
        // 「位置」：谓词（PG WHERE …），… 打开独立小窗
        let predicate = ds
            .excludes
            .get(sel)
            .map(|e| e.predicate.clone())
            .unwrap_or_default();
        let me = cx.entity().clone();
        let pos_field = h_flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .child(combo(&predicate, subtle, false))
            .child(
                h_flex()
                    .id(rid(&format!("dex-pred/{sel}")))
                    .w(px(20.))
                    .h(px(16.))
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(2.))
                    .border_1()
                    .border_color(soft)
                    .bg(bg)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(move |_, window, cx| {
                        me.update(cx, move |this, cx| {
                            this.design_open_exclude_predicate(did, sel, window, cx)
                        })
                    })
                    .test_support()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(fg)
                            .child("…")
                            .into_any_element(),
                    ),
            )
            .into_any_element();
        let row = |label: &str, field: AnyElement| {
            h_flex()
                .h(px(D_FK_PROP_ROW_H))
                .flex_none()
                .items_center()
                .child(
                    div()
                        .w(px(D_FK_PROP_LABEL_W))
                        .flex_none()
                        .text_size(px(12.))
                        .text_color(muted)
                        .child(label.to_string()),
                )
                .child(field)
        };
        v_flex()
            .h(px(D_FK_PROPS_H))
            .flex_none()
            .bg(bg)
            .pt(px(8.))
            .pl(px(D_PROP_PAD_L))
            .child(row("表空间：", combo("", subtle, true)))
            .child(row("填充因子 (%)：", combo("", subtle, false)))
            .child(row("正在缓冲：", combo("", subtle, buffering_on)))
            .child(row("位置：", pos_field))
            .child(row("可延迟：", combo("NOT DEFERRABLE", fg, true)))
            .child(row("延迟：", combo("INITIALLY IMMEDIATE", subtle, true)))
            .into_any_element()
    }

    /// 打开排除「位置」（谓词）小窗：独立顶层窗口，锚在「位置」字段下方
    fn design_open_exclude_predicate(
        &mut self,
        did: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(cur) = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .and_then(|d| d.excludes.get(row))
            .map(|e| e.predicate.clone())
        else {
            return;
        };
        let dark = self.dark;
        let me = cx.entity().clone();
        let main = window.window_handle();
        let anchor_x = window.bounds().origin.x + px(DESIGN_PAGE_X + D_FK_PROP_LABEL_W + D_PROP_PAD_L + 6.);
        let anchor_y = window.bounds().origin.y
            + px(DESIGN_PAGE_Y + D_TOOLBAR_H + D_TABBAR_H + 6. + D_HEADER_H
                + 3. * D_FK_PROP_ROW_H
                + 8.
                + 10.
                + 3.);
        let mut options = TitleBar::window_options();
        options.titlebar = None;
        options.window_min_size = Some(size(px(342.), px(330.)));
        options.window_bounds = Some(WindowBounds::Windowed(Bounds {
            origin: point(anchor_x, anchor_y),
            size: size(px(342.), px(330.)),
        }));
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| ExcludePredicatePopup::new(me, main, did, row, cur, dark, window, cx))
        });
        if opened.is_err() {
            eprintln!("failed to open exclude predicate window");
        }
    }

    /// 排除「位置」（谓词）小窗「确定」：写回 excludes[row].predicate
    fn finish_exclude_predicate(
        &mut self,
        did: u64,
        row: usize,
        value: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(ex) = ds.excludes.get_mut(row)
        {
            ex.predicate = value;
            ds.note = None;
            cx.notify();
        }
    }

    /// 排除「位置」（谓词）小窗「确定」：写回 excludes[row].predicate
    /// 排除「元素」浮层（参考工具那块七列可编辑表格）：锚在元素格正下方，
    /// 控件列自 230 起、下拉 342×20，属性行下留白（整块高 198，见常量出处）。
    /// 可延迟 / 延迟是显示占位：模型里还没这两项，接真库后随 pg_constraint 回填。
    fn fk_props(&self, dark: bool) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, muted, subtle, soft, bg) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border_soft),
            to_hsla(p.background),
        );
        // 参考取样 #FAFAFA：空下拉的底比内容底灰一档（暗色用铬件底）
        let combo = || {
            div()
                .w(px(D_FK_PROP_FIELD_W))
                .h(px(20.))
                .flex_none()
                .px(px(5.))
                .flex()
                .items_center()
                .rounded(px(2.))
                .border_1()
                .border_color(soft)
                .bg(if dark { to_hsla(p.chrome) } else { to_hsla(0xfafafa) })
                .child(div().flex_1())
                .child(
                    Icon::new(IconName::ChevronDown)
                        .with_size(px(9.))
                        .text_color(subtle),
                )
        };
        v_flex()
            .h(px(D_FK_PROPS_H))
            .flex_none()
            .bg(bg)
            .pt(px(8.))
            .pl(px(D_PROP_PAD_L))
            .child(
                h_flex()
                    .h(px(D_FK_PROP_ROW_H))
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .child(Checkbox::new("fk-all-cols").checked(false))
                    .child(div().text_size(px(12.)).text_color(fg).child("符合全部")),
            )
            .child(
                h_flex()
                    .h(px(D_FK_PROP_ROW_H))
                    .flex_none()
                    .items_center()
                    .child(
                        div()
                            .w(px(D_FK_PROP_LABEL_W))
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("可延迟："),
                    )
                    .child(combo()),
            )
            .child(
                h_flex()
                    .h(px(D_FK_PROP_ROW_H))
                    .flex_none()
                    .items_center()
                    .child(
                        div()
                            .w(px(D_FK_PROP_LABEL_W))
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("延迟："),
                    )
                    .child(combo()),
            )
            .into_any_element()
    }

    /// 唯一键 / 检查子页的常驻编辑器（每行 [名称, 内容]）。
    /// 「选项」子页：填充因子 + 表空间（方言差异写在提示里）。
    fn options_tab(
        &self,
        did: u64,
        ds: &DesignerState,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, muted, subtle, soft, bg, accent) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border_soft),
            to_hsla(p.background),
            to_hsla(p.accent),
        );
        let hover = to_hsla(p.hover);
        // 展示值：有常驻编辑器读编辑器，否则读模型
        let val = |f: usize| -> String {
            match ds.opt_edits[f].clone() {
                Some(state) => state.read(cx).value().to_string(),
                None => String::new(),
            }
        };
        let (owner, tablespace, inherit, fillfactor, cluster) =
            (val(0), val(1), val(2), val(3), val(4));
        let unlogged = ds.options.unlogged;
        let me = cx.entity().clone();

        // 表单行：标签 + 字段
        let row = |label: &'static str, field: AnyElement| {
            h_flex()
                .h(px(D_FK_PROP_ROW_H))
                .flex_none()
                .items_center()
                .child(
                    div()
                        .w(px(D_FK_PROP_LABEL_W))
                        .flex_none()
                        .text_size(px(12.))
                        .text_color(muted)
                        .child(label),
                )
                .child(field)
        };
        // 文本框（白底细边可编辑）
        let text_field = |f: usize, value: &str| -> AnyElement {
            let handle = ds.opt_edits[f].clone();
            let editor = match handle {
                Some(state) => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .child(
                        Input::new(&state)
                            .id(rid(&format!("dopt-input/{f}")))
                            .h(px(20.))
                            .appearance(false)
                            .bordered(false)
                            .text_size(px(12.)),
                    )
                    .into_any_element(),
                None => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(subtle)
                            .child(value.to_string()),
                    )
                    .into_any_element(),
            };
            div()
                .w(px(D_FK_PROP_FIELD_W))
                .h(px(20.))
                .flex_none()
                .px(px(5.))
                .flex()
                .items_center()
                .rounded(px(2.))
                .border_1()
                .border_color(soft)
                .bg(bg)
                .child(editor)
                .into_any_element()
        };
        // 下拉框（展示值 + ▼）——drop_cell 是 Button 内建下拉
        let combo_field = |f: usize,
                           value: &str,
                           values: Vec<&'static str>,
                           tag: &'static str|
         -> AnyElement {
            let me_c = me.clone();
            self.drop_cell(
                format!("dopt-combo/{tag}"),
                value.to_string(),
                D_FK_PROP_FIELD_W,
                20.,
                dark,
                true,
                true,
                pick_menu(
                    values.iter().map(|v| (v.to_string(), v.to_string())).collect(),
                    Rc::new(move |value: String, app: &mut App| {
                        let me = me_c.clone();
                        me.update(app, move |this, cx| {
                            this.design_set_option(did, f, value.clone(), cx)
                        });
                    }),
                ),
            )
        };
        // 集群：文本框 + ▼（索引清单，选中写回 cluster）；参考图里它是聚焦态
        let cluster_field = {
            let me_c = me.clone();
            let table = self
                .opened
                .iter()
                .find(|t| t.did == did)
                .and_then(|t| {
                    let conn = self.conns.iter().find(|c| c.name == t.conn_name)?;
                    mock::objects(conn, t.db, t.cat)
                        .get(t.obj)
                        .copied()
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();
            let mut indexes: Vec<String> = vec![format!("pk_{table}")];
            indexes.extend(mock_design_indexes(&table).iter().map(|i| i.name.clone()));
            self.drop_cell(
                "dopt-cluster".to_string(),
                cluster,
                D_FK_PROP_FIELD_W,
                20.,
                dark,
                true,
                true,
                pick_menu(
                    indexes.iter().map(|v| (v.to_string(), v.to_string())).collect(),
                    Rc::new(move |value: String, app: &mut App| {
                        let me = me_c.clone();
                        me.update(app, move |this, cx| {
                            this.design_set_cluster(did, value.clone(), cx)
                        });
                    }),
                ),
            )
        };
        // 继承自：文本框 + …（父表勾选浮层）
        let inherit_field = {
            let me_i = me.clone();
            h_flex()
                .flex_none()
                .items_center()
                .child(text_field(2, &inherit))
                .child(div().w(px(6.)))
                .child(
                    h_flex()
                        .id(rid("dopt-inherit"))
                        .w(px(20.))
                        .h(px(16.))
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded(px(2.))
                        .border_1()
                        .border_color(soft)
                        .bg(bg)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(move |_, window, cx| {
                            me_i.update(cx, move |this, cx| {
                                this.design_open_inherit_picker(did, window, cx)
                            })
                        })
                        .test_support()
                        .child(div().text_size(px(10.)).text_color(fg).child("…")),
                )
                .into_any_element()
        };
        // 不记录：勾选格
        let unlogged_cb = h_flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .id(rid("dopt-unlogged"))
                    .size(px(13.))
                    .rounded(px(2.))
                    .border_1()
                    .border_color(if unlogged { accent } else { muted })
                    .bg(bg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .when(unlogged, |s| {
                        s.child(
                            Icon::new(IconName::Check)
                                .with_size(px(10.))
                                .text_color(accent),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(t) = this
                            .opened
                            .iter_mut()
                            .find(|t| t.did == did)
                            .and_then(|t| t.dstate.as_mut())
                        {
                            t.options.unlogged = !t.options.unlogged;
                            t.note = None;
                            cx.notify();
                        }
                    }))
                    .test_support(),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(fg)
                    .child("不记录"),
            );

        v_flex()
            .size_full()
            .bg(bg)
            .pt(px(10.))
            .pl(px(D_PROP_PAD_L))
            .child(unlogged_cb)
            .child(row("所有者：", combo_field(0, &owner, vec!["postgres"], "owner")))
            .child(row("表空间：", combo_field(1, &tablespace, vec![], "ts")))
            .child(row("继承自：", inherit_field))
            .child(row("填充因子 (%)：", text_field(3, &fillfactor)))
            .child(row("集群：", cluster_field))
            .into_any_element()
    }

    /// 打开「继承自」父表勾选浮层：勾选写回 options.inherit（确定才写回）
    fn design_open_inherit_picker(
        &mut self,
        did: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current) = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .map(|d| d.options.inherit.clone())
        else {
            return;
        };
        let chosen = split_cols(&current);
        let tables = self.ds_tables(did);
        let mut items: Vec<(String, bool)> = tables
            .iter()
            .map(|name| {
                let on = chosen.iter().any(|x| x == name);
                (name.clone(), on)
            })
            .collect();
        for name in &chosen {
            if !items.iter().any(|(n, _)| n == name) {
                items.push((name.clone(), true));
            }
        }
        let sel = items.iter().position(|(_, on)| *on).unwrap_or(0);
        if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = t.dstate.as_mut()
        {
            d.popup = Some(DesignPopup {
                target: PickTarget::InheritTables,
                row: 0,
                items,
                sel,
                expr: None,
            });
            d.note = None;
        }
        let _ = window;
        cx.notify();
    }

    /// 选项子页的文本写回（0 所有者 / 1 表空间）
    fn design_set_option(&mut self, did: u64, f: usize, value: String, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            match f {
                0 => ds.options.owner = value,
                1 => ds.options.tablespace = value,
                _ => {}
            }
            ds.note = None;
            cx.notify();
        }
    }

    /// 选项子页「集群」写回
    fn design_set_cluster(&mut self, did: u64, value: String, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.options.cluster = value;
            ds.note = None;
            cx.notify();
        }
    }

    /// 设计器页签当前库的表清单（继承自 / 集群下拉候选）
    fn ds_tables(&self, did: u64) -> Vec<String> {
        let Some(t) = self.opened.iter().find(|t| t.did == did) else {
            return Vec::new();
        };
        let Some(conn) = self.conns.iter().find(|c| c.name == t.conn_name) else {
            return Vec::new();
        };
        mock::objects(conn, t.db, 0)
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// 「注释」子页：表注释（MySQL 行内 COMMENT=，PG 尾注 COMMENT ON TABLE）。
    fn comment_tab(&self, ds: &DesignerState, dark: bool, _cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::of(dark);
        let (muted, bg) = (to_hsla(p.muted_foreground), to_hsla(p.background));
        v_flex()
            .size_full()
            .bg(bg)
            .p(px(16.))
            .gap(px(10.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .items_center()
                    .child(div().w(px(72.)).text_xs().text_color(muted).child("表注释"))
                    .child(div().w(px(520.)).child(match ds.opt_edits[0].clone() {
                        Some(state) => Input::new(&state)
                            .h(px(24.))
                            .appearance(true)
                            .bordered(true)
                            .into_any_element(),
                        None => div().into_any_element(),
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("MySQL：CREATE TABLE 尾部的 COMMENT='…'；PostgreSQL：COMMENT ON TABLE … IS '…'；SQL Server 走扩展属性，本版不生成。"),
            )
            .into_any_element()
    }

    /// 排除子页的常驻编辑器（每行 [名称, 元素, 方法, 谓词]）。
    /// 排除子页的常驻编辑器（每行 [名称, 元素, 注释]；索引方法走下拉、
    /// 谓词走「位置」小窗、元素也可走浮层勾选）
    fn spawn_exclude_editors(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
            return;
        };
        let specs: Vec<(usize, usize, String, &'static str)> = tab
            .dstate
            .as_ref()
            .expect("设计页签必有状态")
            .excludes
            .iter()
            .enumerate()
            .flat_map(|(r, ex)| {
                [
                    (0usize, ex.name.clone(), ""),
                    (1, ex.elements.clone(), ""),
                    (2, ex.comment.clone(), ""),
                ]
                .map(move |(f, v, ph)| (r, f, v, ph))
            })
            .collect();
        for (r, f, val, ph) in specs {
            {
                let ds = self
                    .opened
                    .iter_mut()
                    .find(|t| t.did == did)
                    .and_then(|t| t.dstate.as_mut())
                    .expect("设计页签必有状态");
                while ds.exclude_edits.len() < ds.excludes.len() {
                    ds.exclude_edits.push(Default::default());
                }
                if r < ds.exclude_edits.len() && ds.exclude_edits[r][f].is_some() {
                    continue;
                }
            }
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(ph.to_string()));
            state.update(cx, |s, cx| s.set_value(val, window, cx));
            cx.subscribe(&state, move |this, input, ev: &InputEvent, cx| {
                let InputEvent::Change = ev else { return };
                let Some(tab) = this.opened.iter_mut().find(|t| t.did == did) else {
                    return;
                };
                let Some(ds) = tab.dstate.as_mut() else { return };
                let text = input.read(cx).value().to_string();
                if let Some(ex) = ds.excludes.get_mut(r) {
                    match f {
                        0 => ex.name = text,
                        1 => ex.elements = text,
                        _ => ex.comment = text,
                    }
                    ds.note = None;
                    cx.notify();
                }
            })
            .detach();
            let ds = self
                .opened
                .iter_mut()
                .find(|t| t.did == did)
                .and_then(|t| t.dstate.as_mut())
                .expect("设计页签必有状态");
            ds.exclude_edits[r][f] = Some(state);
        }
    }

    /// 设计器页签当前表的列清单（排除元素解析用）
fn ds_cols_of(app: &DaroApp, did: u64) -> Vec<DesignCol> {
    app.opened
        .iter()
        .find(|t| t.did == did)
        .and_then(|t| t.dstate.as_ref())
        .map(|d| d.cols.clone())
        .unwrap_or_default()
}

/// 排除子页的当前行，越界时夹回最后一行。
    fn design_exclude_sel(ds: &DesignerState) -> usize {
        ds.exclude_sel
            .min(ds.excludes.len().saturating_sub(1))
    }

    /// 点排除表格的某一格：换当前行 / 当前列。
    fn design_pick_exclude_cell(
        &mut self,
        did: u64,
        row: usize,
        col: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.exclude_sel = row;
            ds.exclude_cur = col;
            cx.notify();
        }
    }

    /// 排除「索引方法」下拉写回：存 SQL 名（btree / gist / …）
    fn design_set_exclude_method(
        &mut self,
        did: u64,
        row: usize,
        value: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(ex) = ds.excludes.get_mut(row)
        {
            ex.method = value;
            ds.note = None;
            cx.notify();
        }
    }

    /// 打开排除「元素」浮层：把 elements 串拆成行草稿（确定才写回）
    fn design_open_exclude_elements(
        &mut self,
        did: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(elements) = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .and_then(|d| d.excludes.get(row))
            .map(|e| e.elements.clone())
        else {
            return;
        };
        let rows = parse_exclude_rows(&elements, &Self::ds_cols_of(self, did));
        if let Some(tab) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = tab.dstate.as_mut()
        {
            d.exclude_editor = Some(ExcludeElementsEditor {
                row,
                rows,
                sel: 0,
                inputs: Vec::new(),
            });
        }
        self.exclude_editor_spawn_inputs(did, window, cx);
        cx.notify();
    }

    /// 给元素浮层的草稿行建常驻输入（增删行后整体重建）
    fn exclude_editor_spawn_inputs(
        &mut self,
        did: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
            return;
        };
        let Some(ed) = tab.dstate.as_ref().and_then(|d| d.exclude_editor.clone()) else {
            return;
        };
        let mut inputs: Vec<[Option<Entity<InputState>>; 7]> = Vec::new();
        for (ix, row) in ed.rows.iter().enumerate() {
            let vals = [
                row.name.clone(),
                row.opclass_schema.clone(),
                row.opclass.clone(),
                row.sort.clone(),
                row.nulls.clone(),
                row.operator_schema.clone(),
                row.operator.clone(),
            ];
            let mut slots: [Option<Entity<InputState>>; 7] = Default::default();
            for (f, v) in vals.into_iter().enumerate() {
                let state = cx.new(|cx| InputState::new(window, cx));
                state.update(cx, |s, cx| s.set_value(v.clone(), window, cx));
                let ix2 = ix;
                let ff2 = f;
                cx.subscribe(&state, move |this, input, ev: &InputEvent, cx| {
                    let InputEvent::Change = ev else { return };
                    let text = input.read(cx).value().to_string();
                    this.exclude_editor_set(did, ix2, ff2, text, cx);
                })
                .detach();
                slots[f] = Some(state);
            }
            inputs.push(slots);
        }
        if let Some(tab) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = tab.dstate.as_mut()
            && let Some(ed) = d.exclude_editor.as_mut()
        {
            ed.inputs = inputs;
        }
        cx.notify();
    }

    /// 元素浮层草稿的格子写回
    fn exclude_editor_set(&mut self, did: u64, ix: usize, f: usize, value: String, cx: &mut Context<Self>) {
        if let Some(tab) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = tab.dstate.as_mut()
            && let Some(ed) = d.exclude_editor.as_mut()
            && let Some(row) = ed.rows.get_mut(ix)
        {
            match f {
                0 => row.name = value,
                1 => row.opclass_schema = value,
                2 => row.opclass = value,
                3 => row.sort = value,
                4 => row.nulls = value,
                5 => row.operator_schema = value,
                _ => row.operator = value,
            }
            cx.notify();
        }
    }

    /// 勾选 / 取消勾选一行元素
    fn exclude_editor_toggle(&mut self, did: u64, ix: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = tab.dstate.as_mut()
            && let Some(ed) = d.exclude_editor.as_mut()
            && let Some(row) = ed.rows.get_mut(ix)
        {
            row.on = !row.on;
            ed.sel = ix;
            cx.notify();
        }
    }

    /// ↑ ↓ 挪动选中行（顺序即写回顺序）
    fn exclude_editor_move(&mut self, did: u64, delta: isize, cx: &mut Context<Self>) {
        if let Some(tab) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = tab.dstate.as_mut()
            && let Some(ed) = d.exclude_editor.as_mut()
        {
            let to = ed.sel as isize + delta;
            if to >= 0 && to < ed.rows.len() as isize {
                ed.rows.swap(ed.sel, to as usize);
                ed.inputs.swap(ed.sel, to as usize);
                ed.sel = to as usize;
                cx.notify();
            }
        }
    }

    /// + 加一行空白元素（表达式排除用）
    fn exclude_editor_add(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = tab.dstate.as_mut()
            && let Some(ed) = d.exclude_editor.as_mut()
        {
            ed.rows.push(ExcludeRow {
                on: true,
                ..Default::default()
            });
            ed.sel = ed.rows.len() - 1;
        }
        self.exclude_editor_spawn_inputs(did, window, cx);
    }

    /// − 删掉选中行
    fn exclude_editor_remove(
        &mut self,
        did: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let need_respawn = {
            let Some(tab) = self.opened.iter_mut().find(|t| t.did == did) else {
                return;
            };
            let Some(d) = tab.dstate.as_mut() else { return };
            let Some(ed) = d.exclude_editor.as_mut() else { return };
            if ed.rows.len() > 1 {
                ed.rows.remove(ed.sel);
                ed.sel = ed.sel.min(ed.rows.len() - 1);
                true
            } else {
                false
            }
        };
        if need_respawn {
            self.exclude_editor_respawn_values(did, window, cx);
        }
        cx.notify();
    }

    /// 行增删后按草稿值重灌所有格子输入
    fn exclude_editor_respawn_values(
        &mut self,
        did: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let vals: Vec<[String; 7]> = {
            let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
                return;
            };
            let Some(ed) = tab.dstate.as_ref().and_then(|d| d.exclude_editor.clone()) else {
                return;
            };
            ed.rows
                .iter()
                .map(|r| {
                    [
                        r.name.clone(),
                        r.opclass_schema.clone(),
                        r.opclass.clone(),
                        r.sort.clone(),
                        r.nulls.clone(),
                        r.operator_schema.clone(),
                        r.operator.clone(),
                    ]
                })
                .collect()
        };
        if let Some(tab) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = tab.dstate.as_mut()
            && let Some(ed) = d.exclude_editor.as_mut()
        {
            for (ix, row) in ed.rows.iter_mut().enumerate() {
                let vals2 = vals.get(ix).cloned().unwrap_or_default();
                let fs: [fn(&mut ExcludeRow, String); 7] = [
                    |r, v| r.name = v,
                    |r, v| r.opclass_schema = v,
                    |r, v| r.opclass = v,
                    |r, v| r.sort = v,
                    |r, v| r.nulls = v,
                    |r, v| r.operator_schema = v,
                    |r, v| r.operator = v,
                ];
                for (f, set) in fs.iter().enumerate() {
                    set(row, vals2[f].clone());
                    if let Some(Some(slot)) = ed.inputs.get_mut(ix).map(|s| &mut s[f]) {
                        let v = vals2[f].clone();
                        slot.update(cx, |s, cx| s.set_value(v, window, cx));
                    }
                }
            }
        }
        cx.notify();
    }

    /// 元素浮层「确定」：草稿序列化回 excludes[row].elements + 同步常驻输入格
    fn exclude_editor_commit(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        let (row, value) = {
            let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
                return;
            };
            let Some(d) = tab.dstate.as_ref() else { return };
            let Some(ed) = d.exclude_editor.as_ref() else { return };
            (ed.row, serialize_exclude_rows(&ed.rows))
        };
        if let Some(tab) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = tab.dstate.as_mut()
            && let Some(ex) = d.excludes.get_mut(row)
        {
            ex.elements = value.clone();
            d.exclude_editor = None;
            d.note = None;
        }
        self.design_sync_exclude_editor(did, row, 1, &value, window, cx);
        cx.notify();
    }

    /// 排除版编辑器同步（浮层写回不经过输入事件）
    fn design_sync_exclude_editor(
        &mut self,
        did: u64,
        row: usize,
        f: usize,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .and_then(|d| d.exclude_edits.get(row))
            .and_then(|e| e.get(f))
            .cloned()
            .flatten()
        else {
            return;
        };
        let value = value.to_string();
        handle.update(cx, |s, cx| s.set_value(value, window, cx));
    }

    /// 规则子页的常驻编辑器（每行 [名称, 事件, 条件, 动作, 语句]）。
    /// 规则子页的常驻编辑器（每行 [名称, 事件, 注释]；代替运行走下拉、
    /// 位置 / 定义走独立小窗）
    fn spawn_rule_editors(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        let specs: Vec<(usize, usize, String, &'static str)> = {
            let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
                return;
            };
            let ds = tab.dstate.as_ref().expect("设计页签必有状态");
            ds.rules
                .iter()
                .enumerate()
                .flat_map(|(r, rule)| {
                    [
                        (0usize, rule.name.clone(), ""),
                        (1, rule.event.clone(), "INSERT"),
                        (2, rule.comment.clone(), ""),
                    ]
                    .map(move |(f, v, ph)| (r, f, v, ph))
                })
                .collect()
        };
        for (r, f, val, ph) in specs {
            {
                let ds = self
                    .opened
                    .iter_mut()
                    .find(|t| t.did == did)
                    .and_then(|t| t.dstate.as_mut())
                    .expect("设计页签必有状态");
                while ds.rule_edits.len() < ds.rules.len() {
                    ds.rule_edits.push(Default::default());
                }
                if r < ds.rule_edits.len() && ds.rule_edits[r][f].is_some() {
                    continue;
                }
            }
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(ph.to_string()));
            state.update(cx, |s, cx| s.set_value(val, window, cx));
            cx.subscribe(&state, move |this, input, ev: &InputEvent, cx| {
                let InputEvent::Change = ev else { return };
                let Some(tab) = this.opened.iter_mut().find(|t| t.did == did) else {
                    return;
                };
                let Some(ds) = tab.dstate.as_mut() else { return };
                let text = input.read(cx).value().to_string();
                if let Some(rule) = ds.rules.get_mut(r) {
                    match f {
                        0 => rule.name = text,
                        1 => rule.event = text,
                        _ => rule.comment = text,
                    }
                    ds.note = None;
                    cx.notify();
                }
            })
            .detach();
            let ds = self
                .opened
                .iter_mut()
                .find(|t| t.did == did)
                .and_then(|t| t.dstate.as_mut())
                .expect("设计页签必有状态");
            ds.rule_edits[r][f] = Some(state);
        }
    }

    /// 规则子页的当前行，越界时夹回最后一行。
    fn design_rule_sel(ds: &DesignerState) -> usize {
        ds.rule_sel.min(ds.rules.len().saturating_sub(1))
    }

    /// 点规则表格的某一格：换当前行 / 当前列。
    fn design_pick_rule_cell(&mut self, did: u64, row: usize, col: usize, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.rule_sel = row;
            ds.rule_cur = col;
            cx.notify();
        }
    }

    /// 规则「代替运行」下拉写回（ALSO / INSTEAD）
    fn design_set_rule_action(
        &mut self,
        did: u64,
        row: usize,
        value: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(rule) = ds.rules.get_mut(row)
        {
            rule.action = value;
            ds.note = None;
            cx.notify();
        }
    }

    /// 切换规则的「启用」
    fn design_toggle_rule_enabled(&mut self, did: u64, row: usize, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(rule) = ds.rules.get_mut(row)
        {
            rule.enabled = !rule.enabled;
            ds.note = None;
            cx.notify();
        }
    }

    /// 触发器子页的当前行，越界时夹回最后一行。
    fn design_trig_sel(ds: &DesignerState) -> usize {
        ds.trig_sel.min(ds.triggers.len().saturating_sub(1))
    }

    /// 点触发器表格的某一格：换当前行 / 当前列。
    fn design_pick_trig_cell(&mut self, did: u64, row: usize, col: usize, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.trig_sel = row;
            ds.trig_cur = col;
            cx.notify();
        }
    }

    /// 给每个下拉写回（ROW / STATEMENT）
    fn design_set_trig_for_each(
        &mut self,
        did: u64,
        row: usize,
        value: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(t) = ds.triggers.get_mut(row)
        {
            t.for_each = value;
            ds.note = None;
            cx.notify();
        }
    }

    /// 触发下拉写回（BEFORE / AFTER / INSTEAD OF）
    fn design_set_trig_timing(
        &mut self,
        did: u64,
        row: usize,
        value: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(t) = ds.triggers.get_mut(row)
        {
            t.timing = value;
            ds.note = None;
            cx.notify();
        }
    }

    /// 事件勾选格切换（插入 / 更新 / 删除 / 截断）
    fn design_toggle_trig_event(
        &mut self,
        did: u64,
        row: usize,
        ev: &'static str,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(t) = ds.triggers.get_mut(row)
        {
            t.set_event(ev, !t.has_event(ev));
            ds.note = None;
            cx.notify();
        }
    }

    /// 打开触发器「更新字段」浮层：elements 草稿取自 update_fields（确定才写回）
    fn design_open_trig_fields(&mut self, did: u64, row: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
            return;
        };
        let Some(d) = tab.dstate.as_ref() else { return };
        let current = d
            .triggers
            .get(row)
            .map(|t| t.update_fields.clone())
            .unwrap_or_default();
        let chosen = split_cols(&current);
        let mut items: Vec<(String, bool)> = d
            .cols
            .iter()
            .map(|c| c.name.trim().to_string())
            .filter(|n| !n.is_empty())
            .map(|n| {
                let on = chosen.iter().any(|x| *x == n);
                (n, on)
            })
            .collect();
        for name in &chosen {
            if !items.iter().any(|(n, _)| n == name) {
                items.push((name.clone(), true));
            }
        }
        items.sort_by_key(|(_, on)| !*on);
        let sel = items.iter().position(|(_, on)| *on).unwrap_or(0);
        if let Some(t) = self.opened.iter_mut().find(|t| t.did == did)
            && let Some(d) = t.dstate.as_mut()
        {
            d.popup = Some(DesignPopup {
                target: PickTarget::TrigFields,
                row,
                items,
                sel,
                expr: None,
            });
            d.note = None;
        }
        cx.notify();
    }

    /// 切换触发器的「启用」
    fn design_toggle_trig_enabled(&mut self, did: u64, row: usize, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(t) = ds.triggers.get_mut(row)
        {
            t.enabled = !t.enabled;
            ds.note = None;
            cx.notify();
        }
    }

    /// 规则「位置」（条件）/「定义」（语句）小窗「确定」：按 field 写回
    fn finish_rule_text(
        &mut self,
        did: u64,
        row: usize,
        field: u8,
        value: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && let Some(rule) = ds.rules.get_mut(row)
        {
            match field {
                0 => rule.qualification = value,
                _ => rule.statement = value,
            }
            ds.note = None;
            cx.notify();
        }
    }

    /// 打开规则「位置 / 定义」小窗：独立顶层窗口，锚在属性区字段下方
    fn design_open_rule_text(
        &mut self,
        did: u64,
        row: usize,
        field: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(cur) = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .and_then(|d| d.rules.get(row))
            .map(|r| {
                if field == 0 {
                    r.qualification.clone()
                } else {
                    r.statement.clone()
                }
            })
        else {
            return;
        };
        let dark = self.dark;
        let me = cx.entity().clone();
        let main = window.window_handle();
        // 锚点：属性区「位置」行（第一行）或「定义」行（第二行）的字段右端
        let anchor_x = window.bounds().origin.x
            + px(DESIGN_PAGE_X + D_RL_PROP_LABEL_W + D_RL_PROP_FIELD_W + 30.);
        let anchor_y = window.bounds().origin.y
            + px(DESIGN_PAGE_Y + D_TOOLBAR_H + D_TABBAR_H + 6. + D_HEADER_H
                + (row as f32 + 1.) * D_ROW_EXTENT
                + 3.
                + 8.
                + field as f32 * D_RL_PROP_ROW_H
                + 10.);
        let mut options = TitleBar::window_options();
        options.titlebar = None;
        options.window_min_size = Some(size(px(342.), px(330.)));
        options.window_bounds = Some(WindowBounds::Windowed(Bounds {
            origin: point(anchor_x, anchor_y),
            size: size(px(342.), px(330.)),
        }));
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| RuleTextPopup::new(me, main, did, row, field, cur, dark, window, cx))
        });
        if opened.is_err() {
            eprintln!("failed to open rule text window");
        }
    }

    fn design_add_rule(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            if !ds.rules.iter().any(design_rule_blank) {
                ds.rules.push(DesignRule::default());
            }
            ds.rule_edits.clear();
            ds.note = None;
        }
        cx.notify();
        self.spawn_rule_editors(did, window, cx);
        self.spawn_trigger_editors(did, window, cx);
    }

    fn design_remove_rule(
        &mut self,
        did: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && row < ds.rules.len()
        {
            ds.rules.remove(row);
            if ds.rules.is_empty() {
                ds.rules.push(DesignRule::default());
            }
            ds.rule_edits.clear();
            ds.note = None;
        }
        self.spawn_rule_editors(did, window, cx);
        cx.notify();
    }

    /// 「规则」子页：一行 名称 / 事件 / 条件 / 动作 / 语句 / ×，仅 PostgreSQL 生成。
    /// 规则子页（照参考工具 1:1）：五列表格（名称 / 事件 / 代替运行 / 启用 /
    /// 注释）。当前格是白底 + 2px 强调边，代替运行当前态带 ▼ 下拉（ALSO /
    /// INSTEAD），启用格点勾选框即切换；底部「位置（条件）/ 定义（语句）」两行
    /// 属性，… 打开独立小窗。
    fn rules_tab(
        &self,
        did: u64,
        ds: &DesignerState,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, muted, subtle, border, soft, bg, accent, selection) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.background),
            to_hsla(p.accent),
            to_hsla(p.selection),
        );
        let hover = to_hsla(p.hover);
        let head_cur = to_hsla(blend(p.accent, p.background, 0.74));
        let sel = Self::design_rule_sel(ds);
        let cur = ds.rule_cur.min(4);
        let me = cx.entity().clone();
        let cw = |ix: usize| ds.widths.get(&(6usize, ix)).copied().unwrap_or(D_RULE_WIDTHS[ix]);
        let total_w = D_GUTTER_W + (0..5).map(cw).sum::<f32>();
        let headers = ["名称", "事件", "代替运行", "启用", "注释"];

        // 行首槽：锚点行铺浅蓝 + 右缘 2px 强调条（参考取样 #4DA0E1），表头槽不铺
        let gutter = move |h: f32, anchor: bool| {
            div()
                .w(px(D_GUTTER_W))
                .h(px(h))
                .flex_none()
                .relative()
                .border_r_1()
                .border_b_1()
                .border_color(border)
                .when(anchor, |s| {
                    s.bg(selection).child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(D_ACCENT_BAR_W))
                            .bg(to_hsla(D_FK_BAR)),
                    )
                })
        };

        let head = h_flex()
            .h(px(D_HEADER_H))
            .flex_none()
            .bg(bg)
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some(did) = this.did_of_active_design() {
                    this.design_resize_move(did, ev.position.x.as_f32(), cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(did) = this.did_of_active_design() {
                        this.design_resize_end(did, cx);
                    }
                }),
            )
            .child(gutter(D_HEADER_H, false))
            .children(headers.iter().enumerate().map(|(i, h)| {
                let cur_w = cw(i);
                div()
                    .w(px(cur_w))
                    .h(px(D_HEADER_H))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(border)
                    .when(i == cur, |s| {
                        s.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .bg(head_cur),
                        )
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(D_ACCENT_BAR_W))
                                .bg(to_hsla(D_FK_BAR)),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(6.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(div().text_size(px(12.)).text_color(fg).child(*h)),
                    )
                    .child(
                        div()
                            .id(rid(&format!("drule-resize/{i}")))
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(5.))
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    this.design_resize_begin(
                                        did,
                                        6,
                                        i,
                                        ev.position.x.as_f32(),
                                        cur_w,
                                        cx,
                                    );
                                }),
                            ),
                    )
            }))
            .child(
                div()
                    .flex_1()
                    .h(px(D_HEADER_H))
                    .border_b_1()
                    .border_color(border),
            );

        let mut rows: Vec<AnyElement> = Vec::new();
        for (r, rule) in ds.rules.iter().enumerate() {
            let anchor = r == sel;
            let edits = ds.rule_edits.get(r).cloned().unwrap_or_default();

            // 代替运行：整格下拉（ALSO / INSTEAD），当前态带 ▼
            let me_a = me.clone();
            let action_cell = {
                let current = anchor && cur == 2;
                let cell = div()
                    .id(rid(&format!("drule-cell/{r}/2")))
                    .w(px(cw(2)))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .when(current, |s| s.border_2().border_color(accent))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_pick_rule_cell(did, r, 2, cx)
                    }));
                let inner = self.drop_cell(
                    format!("drule-action/{r}"),
                    if rule.action.trim().is_empty() {
                        "ALSO".to_string()
                    } else {
                        rule.action.clone()
                    },
                    cw(2) - 4.,
                    D_ROW_EXTENT - 4.,
                    dark,
                    true,
                    current,
                    pick_menu(
                        RULE_ACTIONS
                            .iter()
                            .map(|a| (a.to_string(), a.to_string()))
                            .collect(),
                        Rc::new(move |value: String, app: &mut App| {
                            let me = me_a.clone();
                            me.update(app, move |this, cx| {
                                this.design_set_rule_action(did, r, value.clone(), cx)
                            });
                        }),
                    ),
                )
                .into_any_element();
                cell.child(
                    div()
                        .absolute()
                        .left(px(2.))
                        .top(px(2.))
                        .right(px(2.))
                        .bottom(px(2.))
                        .child(inner),
                )
                .into_any_element()
            };
            // 启用：勾选格（点击即切换）
            let flag_cell = div()
                .id(rid(&format!("drule-cell/{r}/3")))
                .w(px(cw(3)))
                .h(px(D_ROW_EXTENT))
                .flex_none()
                .relative()
                .when(anchor && cur == 3, |s| s.border_2().border_color(accent))
                .border_r_1()
                .border_b_1()
                .border_color(soft)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.design_pick_rule_cell(did, r, 3, cx);
                    this.design_toggle_rule_enabled(did, r, cx);
                }))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(13.))
                                .rounded(px(2.))
                                .border_1()
                                .border_color(if rule.enabled { accent } else { subtle })
                                .bg(if rule.enabled { accent } else { bg })
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(rule.enabled, |s| {
                                    s.child(
                                        Icon::new(IconName::Check)
                                            .with_size(px(10.))
                                            .text_color(to_hsla(0xffffff)),
                                    )
                                }),
                        ),
                )
                .test_support()
                .into_any_element();
            // 文本格：常驻输入框；当前格白底 + 2px 强调边
            let input_cell = |col: usize,
                              f: usize,
                              handle: Option<Entity<InputState>>,
                              value: &str|
             -> AnyElement {
                let current = anchor && col == cur;
                let editor = match handle {
                    Some(e) => div()
                        .absolute()
                        .left(px(4.))
                        .right(px(2.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&e)
                                    .id(rid(&format!("drule-input/{r}/{f}")))
                                    .h(px(18.))
                                    .appearance(false)
                                    .bordered(false)
                                    .text_size(px(12.)),
                            ),
                        )
                        .into_any_element(),
                    None => div()
                        .absolute()
                        .left(px(6.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(if current { fg } else { subtle })
                                .child(value.to_string()),
                        )
                        .into_any_element(),
                };
                div()
                    .id(rid(&format!("drule-cell/{r}/{col}")))
                    .w(px(cw(col)))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .when(current, |s| s.border_2().border_color(accent))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_pick_rule_cell(did, r, col, cx)
                    }))
                    .child(editor)
                    .test_support()
                    .into_any_element()
            };

            rows.push(
                h_flex()
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .child({
                        let g = gutter(D_ROW_EXTENT, anchor);
                        div()
                            .id(rid(&format!("drule-gutter/{r}")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.design_pick_rule_cell(did, r, 0, cx)
                            }))
                            .child(g)
                            .test_support()
                    })
                    .child(input_cell(0, 0, edits[0].clone(), &rule.name))
                    .child(input_cell(1, 1, edits[1].clone(), &rule.event))
                    .child(action_cell)
                    .child(flag_cell)
                    .child(input_cell(4, 2, edits[2].clone(), &rule.comment))
                    .child(
                        div()
                            .flex_1()
                            .h(px(D_ROW_EXTENT))
                            .border_b_1()
                            .border_color(soft),
                    )
                    .into_any_element(),
            );
        }

        // 参考图：子页签条与表头之间有一条 ~6 逻辑像素的白缝（属于网格的顶边距）
        let grid = v_flex()
            .child(div().h(px(6.)).flex_none().bg(bg))
            .child(head)
            .children(rows);
        v_flex()
            .size_full()
            .bg(bg)
            .child(
                div()
                    .id("drule-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    // 滚动条包一层：外层留 id / test-support，内层管滚动
                    .child(
                        div()
                            .size_full()
                            .overflow_scrollbar()
                            .child(div().w(px(total_w)).child(grid)),
                    ),
            )
            .child(
                div()
                    .h(px(D_PROP_SPLITTER_H))
                    .flex_none()
                    .bg(to_hsla(p.chrome)),
            )
            .child(self.rule_props(did, ds, dark, cx))
            .into_any_element()
    }

    /// 规则子页底部两行属性（参考图：位置 = 条件、定义 = 语句），… 打开独立小窗。
    /// 行距 38 / 标签列 203 / 字段 357×20（字段自 x=214 起，比外键属性区更靠左）。
    fn rule_props(
        &self,
        did: u64,
        ds: &DesignerState,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, muted, subtle, soft, bg) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border_soft),
            to_hsla(p.background),
        );
        let hover = to_hsla(p.hover);
        let sel = Self::design_rule_sel(ds);
        let me = cx.entity().clone();
        let (qual, stmt) = ds
            .rules
            .get(sel)
            .map(|r| (r.qualification.clone(), r.statement.clone()))
            .unwrap_or_default();
        let field = |value: &str, tag: &'static str, field: u8| -> AnyElement {
            let me_f = me.clone();
            h_flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .w(px(D_RL_PROP_FIELD_W))
                        .h(px(20.))
                        .flex_none()
                        .px(px(5.))
                        .flex()
                        .items_center()
                        .rounded(px(2.))
                        .border_1()
                        .border_color(soft)
                        .bg(if dark {
                            to_hsla(p.chrome)
                        } else {
                            to_hsla(0xfafafa)
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(subtle)
                                .child(value.to_string()),
                        ),
                )
                .child(
                    h_flex()
                        .id(rid(&format!("drule-text/{tag}/{sel}")))
                        .w(px(20.))
                        .h(px(16.))
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded(px(2.))
                        .border_1()
                        .border_color(soft)
                        .bg(bg)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(move |_, window, cx| {
                            me_f.update(cx, move |this, cx| {
                                this.design_open_rule_text(did, sel, field, window, cx)
                            })
                        })
                        .test_support()
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(fg)
                                .child("…")
                                .into_any_element(),
                        ),
                )
                .into_any_element()
        };
        v_flex()
            .h(px(110.))
            .flex_none()
            .bg(bg)
            .pt(px(8.))
            .pl(px(D_PROP_PAD_L))
            .child(
                h_flex()
                    .h(px(D_RL_PROP_ROW_H))
                    .flex_none()
                    .items_center()
                    .child(
                        div()
                            .w(px(D_RL_PROP_LABEL_W))
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("位置："),
                    )
                    .child(field(&qual, "qual", 0)),
            )
            .child(
                h_flex()
                    .h(px(D_RL_PROP_ROW_H))
                    .flex_none()
                    .items_center()
                    .child(
                        div()
                            .w(px(D_RL_PROP_LABEL_W))
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("定义："),
                    )
                    .child(field(&stmt, "stmt", 1)),
            )
            .into_any_element()
    }

    /// 为视图 / 实体化视图补「定义」编辑器（打开页签 / 切子页时调）。    /// 视图 / 实体化视图的「定义」正文编辑器（代码编辑器 + SQL 高亮/补全）。
    /// 只在视图类设计页签里创建；正文写回 `ds.view_body`，SQL 预览据此生成。
    fn ensure_view_editor(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(has) = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .map(|ds| ds.view_editor.is_some())
        else {
            return;
        };
        if has {
            return;
        }
        let initial = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .map(|ds| ds.view_body.clone())
            .unwrap_or_default();
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx).placeholder("SELECT …");
            state.set_highlighter_factory(
                std::rc::Rc::new(|_lang: &str| {
                    Some(Box::new(sql_highlight::SqlHighlighter::new())
                        as Box<dyn gpui_kit::component::input::InputHighlighter>)
                }),
                cx,
            );
            state.set_highlighter("sql", cx);
            state.lsp_mut().completion_provider = Some(sql_completion::provider_for(&[], &[]));
            if !initial.is_empty() {
                state.set_value(initial, window, cx);
            }
            state
        });
        cx.subscribe(&editor, move |this, input, ev: &InputEvent, cx| {
            let InputEvent::Change = ev else { return };
            let text = input.read(cx).value().to_string();
            if let Some(ds) = this
                .opened
                .iter_mut()
                .find(|t| t.did == did)
                .and_then(|t| t.dstate.as_mut())
            {
                ds.view_body = text;
                ds.note = None;
                cx.notify();
            }
        })
        .detach();
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            ds.view_editor = Some(editor);
        }
    }

    /// 触发器子页的常驻编辑器（每行 [名称, 时机, 事件, 级, 函数, 主体]）。
    fn spawn_trigger_editors(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        let specs: Vec<(usize, usize, String, &'static str)> = {
            let Some(tab) = self.opened.iter().find(|t| t.did == did) else {
                return;
            };
            let ds = tab.dstate.as_ref().expect("设计页签必有状态");
            ds.triggers
                .iter()
                .enumerate()
                .flat_map(|(r, t)| {
                    [
                        (0usize, t.name.clone(), ""),
                        (2, t.body.clone(), ""),
                    ]
                    .map(move |(f, v, ph)| (r, f, v, ph))
                })
                .collect()
        };
        for (r, f, val, ph) in specs {
            {
                let ds = self
                    .opened
                    .iter_mut()
                    .find(|t| t.did == did)
                    .and_then(|t| t.dstate.as_mut())
                    .expect("设计页签必有状态");
                while ds.trigger_edits.len() < ds.triggers.len() {
                    ds.trigger_edits.push(Default::default());
                }
                if r < ds.trigger_edits.len() && ds.trigger_edits[r][f].is_some() {
                    continue;
                }
            }
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(ph.to_string()));
            state.update(cx, |s, cx| s.set_value(val, window, cx));
            cx.subscribe(&state, move |this, input, ev: &InputEvent, cx| {
                let InputEvent::Change = ev else { return };
                let Some(ds) = this
                    .opened
                    .iter_mut()
                    .find(|t| t.did == did)
                    .and_then(|t| t.dstate.as_mut())
                else {
                    return;
                };
                let text = input.read(cx).value().to_string();
                if let Some(t) = ds.triggers.get_mut(r) {
                    match f {
                        0 => t.name = text,
                        _ => t.body = text,
                    }
                    ds.note = None;
                    cx.notify();
                }
            })
            .detach();
            let ds = self
                .opened
                .iter_mut()
                .find(|t| t.did == did)
                .and_then(|t| t.dstate.as_mut())
                .expect("设计页签必有状态");
            ds.trigger_edits[r][f] = Some(state);
        }
    }

    fn design_add_trigger(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
        {
            if !ds.triggers.iter().any(design_trigger_blank) {
                ds.triggers.push(DesignTrigger::default());
            }
            ds.trigger_edits.clear();
            ds.note = None;
        }
        cx.notify();
        self.spawn_trigger_editors(did, window, cx);
    }

    fn design_remove_trigger(
        &mut self,
        did: u64,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ds) = self
            .opened
            .iter_mut()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_mut())
            && row < ds.triggers.len()
        {
            ds.triggers.remove(row);
            if ds.triggers.is_empty() {
                ds.triggers.push(DesignTrigger::default());
            }
            ds.trigger_edits.clear();
            ds.note = None;
        }
        self.spawn_trigger_editors(did, window, cx);
        cx.notify();
    }

    /// 「触发器」子页：一行 名称 / 时机 / 事件 / 级 / 函数 / 主体 / ×。
    /// 触发器子页（照参考工具 1:1）：十列表格（名称 / 给每个 / 触发 / 插入 /
    /// 更新 / 删除 / 截断 / 更新字段 / 启用 / 注释）。给每个与触发是下拉，
    /// 插入～截断是事件勾选格，更新字段当前态带 … 浮层（勾选列，确定写回
    /// update_fields），启用默认勾上；底部「常规」属性区（当 / 触发函数 / 参数）。
    /// 整体超出窗格宽度，横向滚动（与参考工具同款裁切）。
    fn triggers_tab(
        &self,
        did: u64,
        ds: &DesignerState,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, subtle, border, soft, bg, accent, selection) = (
            to_hsla(p.foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.background),
            to_hsla(p.accent),
            to_hsla(p.selection),
        );
        let hover = to_hsla(p.hover);
        let head_cur = to_hsla(blend(p.accent, p.background, 0.74));
        let sel = Self::design_trig_sel(ds);
        let cur = ds.trig_cur.min(9);
        let me = cx.entity().clone();
        let cw = |ix: usize| ds.widths.get(&(7usize, ix)).copied().unwrap_or(D_TRIGGER_WIDTHS[ix]);
        let total_w = D_GUTTER_W + (0..10).map(cw).sum::<f32>();
        let headers = [
            "名称", "给每个", "触发", "插入", "更新", "删除", "截断", "更新字段", "启用", "注释",
        ];
        let events = [
            ("插入", "INSERT"),
            ("更新", "UPDATE"),
            ("删除", "DELETE"),
            ("截断", "TRUNCATE"),
        ];

        // 行首槽：锚点行铺浅蓝 + 右缘 2px 强调条（参考取样 #4DA0E1），表头槽不铺
        let gutter = move |h: f32, anchor: bool| {
            div()
                .w(px(D_GUTTER_W))
                .h(px(h))
                .flex_none()
                .relative()
                .border_r_1()
                .border_b_1()
                .border_color(border)
                .when(anchor, |s| {
                    s.bg(selection).child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(D_ACCENT_BAR_W))
                            .bg(to_hsla(D_FK_BAR)),
                    )
                })
        };

        let head = h_flex()
            .h(px(D_HEADER_H))
            .flex_none()
            .bg(bg)
            .child(gutter(D_HEADER_H, false))
            .children(headers.iter().enumerate().map(|(i, h)| {
                div()
                    .w(px(cw(i)))
                    .h(px(D_HEADER_H))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(border)
                    .when(i == cur, |s| {
                        s.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .bg(head_cur),
                        )
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(D_ACCENT_BAR_W))
                                .bg(to_hsla(D_FK_BAR)),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(6.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(div().text_size(px(12.)).text_color(fg).child(*h)),
                    )
            }))
            .child(
                div()
                    .flex_1()
                    .h(px(D_HEADER_H))
                    .border_b_1()
                    .border_color(border),
            );

        let mut rows: Vec<AnyElement> = Vec::new();
        for (r, t) in ds.triggers.iter().enumerate() {
            let anchor = r == sel;
            let edits = ds.trigger_edits.get(r).cloned().unwrap_or_default();

            // 给每个 / 触发：整格下拉（当前态带 ▼）
            let me_d = me.clone();
            let drop_cell = |col: usize,
                             value: &str,
                             values: Vec<&'static str>,
                             tag: usize|
             -> AnyElement {
                let current = anchor && col == cur;
                let me_c = me_d.clone();
                let cell = div()
                    .id(rid(&format!("dtrg-cell/{r}/{col}")))
                    .w(px(cw(col)))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .when(current, |s| s.border_2().border_color(accent))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_pick_trig_cell(did, r, col, cx)
                    }));
                let inner = self.drop_cell(
                    format!("dtrg-drop/{r}/{tag}"),
                    value.to_string(),
                    cw(col) - 4.,
                    D_ROW_EXTENT - 4.,
                    dark,
                    true,
                    current,
                    pick_menu(
                        values.iter().map(|v| (v.to_string(), v.to_string())).collect(),
                        Rc::new(move |value: String, app: &mut App| {
                            let me = me_c.clone();
                            me.update(app, move |this, cx| {
                                if tag == 0 {
                                    this.design_set_trig_for_each(did, r, value.clone(), cx)
                                } else {
                                    this.design_set_trig_timing(did, r, value.clone(), cx)
                                }
                            });
                        }),
                    ),
                )
                .into_any_element();
                cell.child(
                    div()
                        .absolute()
                        .left(px(2.))
                        .top(px(2.))
                        .right(px(2.))
                        .bottom(px(2.))
                        .child(inner),
                )
                .into_any_element()
            };
            // 事件勾选格（插入 / 更新 / 删除 / 截断）
            let flag_cell = |col: usize, ev: &'static str| -> AnyElement {
                let current = anchor && col == cur;
                let on = t.has_event(ev);
                div()
                    .id(rid(&format!("dtrg-cell/{r}/{col}")))
                    .w(px(cw(col)))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .relative()
                    .when(current, |s| s.border_2().border_color(accent))
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_pick_trig_cell(did, r, col, cx);
                        this.design_toggle_trig_event(did, r, ev, cx);
                    }))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(13.))
                                    .rounded(px(2.))
                                    .border_1()
                                    .border_color(if on { accent } else { subtle })
                                    .bg(if on { accent } else { bg })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(on, |s| {
                                        s.child(
                                            Icon::new(IconName::Check)
                                                .with_size(px(10.))
                                                .text_color(to_hsla(0xffffff)),
                                        )
                                    }),
                            ),
                    )
                    .test_support()
                    .into_any_element()
            };
            // 更新字段：文本格 + 当前态 … 浮层入口（勾选列，确定写回）
            let uf_cell = {
                let current = anchor && cur == 7;
                let editor = div()
                    .absolute()
                    .left(px(4.))
                    .right(px(2.))
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(if current { fg } else { subtle })
                            .truncate()
                            .child(t.update_fields.clone()),
                    )
                    .into_any_element();
                let mut cell = div()
                    .id(rid(&format!("dtrg-cell/{r}/7")))
                    .w(px(cw(7)))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .relative()
                    .when(current, |s| s.border_2().border_color(accent))
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_pick_trig_cell(did, r, 7, cx)
                    }))
                    .child(editor);
                if current {
                    let me_a = me.clone();
                    cell = cell.child(
                        h_flex()
                            .id(rid(&format!("dtrg-afford/{r}/7")))
                            .absolute()
                            .right(px(3.))
                            .top(px(2.))
                            .w(px(20.))
                            .h(px(16.))
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .rounded(px(2.))
                            .border_1()
                            .border_color(soft)
                            .bg(bg)
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(move |_, _, cx| {
                                me_a.update(cx, move |this, cx| {
                                    this.design_open_trig_fields(did, r, cx)
                                })
                            })
                            .test_support()
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(fg)
                                    .child("…")
                                    .into_any_element(),
                            ),
                    );
                }
                cell.test_support().into_any_element()
            };
            // 文本格：常驻输入框；当前格白底 + 2px 强调边
            let input_cell = |col: usize,
                              f: usize,
                              handle: Option<Entity<InputState>>,
                              value: &str|
             -> AnyElement {
                let current = anchor && col == cur;
                let editor = match handle {
                    Some(e) => div()
                        .absolute()
                        .left(px(4.))
                        .right(px(2.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&e)
                                    .id(rid(&format!("dtrg-input/{r}/{f}")))
                                    .h(px(18.))
                                    .appearance(false)
                                    .bordered(false)
                                    .text_size(px(12.)),
                            ),
                        )
                        .into_any_element(),
                    None => div()
                        .absolute()
                        .left(px(6.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(if current { fg } else { subtle })
                                .child(value.to_string()),
                        )
                        .into_any_element(),
                };
                div()
                    .id(rid(&format!("dtrg-cell/{r}/{col}")))
                    .w(px(cw(col)))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .when(current, |s| s.border_2().border_color(accent))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_pick_trig_cell(did, r, col, cx)
                    }))
                    .child(editor)
                    .test_support()
                    .into_any_element()
            };
            // 启用：勾选格
            let enable_cell = div()
                .id(rid(&format!("dtrg-cell/{r}/8")))
                .w(px(cw(8)))
                .h(px(D_ROW_EXTENT))
                .flex_none()
                .relative()
                .when(anchor && cur == 8, |s| s.border_2().border_color(accent))
                .border_r_1()
                .border_b_1()
                .border_color(soft)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.design_pick_trig_cell(did, r, 8, cx);
                    this.design_toggle_trig_enabled(did, r, cx);
                }))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(13.))
                                .rounded(px(2.))
                                .border_1()
                                .border_color(if t.enabled { accent } else { subtle })
                                .bg(if t.enabled { accent } else { bg })
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(t.enabled, |s| {
                                    s.child(
                                        Icon::new(IconName::Check)
                                            .with_size(px(10.))
                                            .text_color(to_hsla(0xffffff)),
                                    )
                                }),
                        ),
                )
                .test_support()
                .into_any_element();

            rows.push(
                h_flex()
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .child({
                        let g = gutter(D_ROW_EXTENT, anchor);
                        div()
                            .id(rid(&format!("dtrg-gutter/{r}")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.design_pick_trig_cell(did, r, 0, cx)
                            }))
                            .child(g)
                            .test_support()
                    })
                    .child(input_cell(0, 0, edits[0].clone(), &t.name))
                    .child(drop_cell(1, &t.for_each, vec!["ROW", "STATEMENT"], 0))
                    .child(drop_cell(
                        2,
                        &t.timing,
                        vec!["BEFORE", "AFTER", "INSTEAD OF"],
                        1,
                    ))
                    .child(flag_cell(3, "INSERT"))
                    .child(flag_cell(4, "UPDATE"))
                    .child(flag_cell(5, "DELETE"))
                    .child(flag_cell(6, "TRUNCATE"))
                    .child(uf_cell)
                    .child(enable_cell)
                    .child(input_cell(9, 2, edits[2].clone(), &t.comment))
                    .child(
                        div()
                            .flex_1()
                            .h(px(D_ROW_EXTENT))
                            .border_b_1()
                            .border_color(soft),
                    )
                    .into_any_element(),
            );
        }

        // 参考图：子页签条与表头之间有一条 ~6 逻辑像素的白缝（属于网格的顶边距）
        let grid = v_flex()
            .child(div().h(px(6.)).flex_none().bg(bg))
            .child(head)
            .children(rows);
        v_flex()
            .size_full()
            .bg(bg)
            .child(
                div()
                    .id("dtrg-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    // 滚动条包一层：外层留 id / test-support，内层管滚动
                    .child(
                        div()
                            .size_full()
                            .overflow_scrollbar()
                            .child(div().w(px(total_w)).child(grid)),
                    ),
            )
            .child(
                div()
                    .h(px(D_PROP_SPLITTER_H))
                    .flex_none()
                    .bg(to_hsla(p.chrome)),
            )
            .child(self.trigger_props(did, ds, dark, cx))
            .into_any_element()
    }

    /// 触发器子页底部「常规」属性区（参考图：当 / 触发函数（两个下拉） / 参数），
    /// 上方「常规 | 约束」子页签为展示占位（约束页未实现）。行距 30，
    /// 标签列 203 / 字段 357（与规则属性区同款密度）。
    fn trigger_props(
        &self,
        did: u64,
        ds: &DesignerState,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::of(dark);
        let (fg, muted, subtle, soft, bg, accent, border) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.border_soft),
            to_hsla(p.background),
            to_hsla(p.accent),
            to_hsla(p.border),
        );
        let hover = to_hsla(p.hover);
        let sel = Self::design_trig_sel(ds);
        let (when, function, args) = ds
            .triggers
            .get(sel)
            .map(|t| (t.when.clone(), t.function.clone(), t.args.clone()))
            .unwrap_or_default();
        let me = cx.entity().clone();
        let combo = |value: &str| -> AnyElement {
            div()
                .w(px((D_RL_PROP_FIELD_W - 14.) / 2.))
                .h(px(20.))
                .flex_none()
                .px(px(5.))
                .flex()
                .items_center()
                .rounded(px(2.))
                .border_1()
                .border_color(soft)
                .bg(if dark { to_hsla(p.chrome) } else { to_hsla(0xfafafa) })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(subtle)
                        .child(value.to_string()),
                )
                .child(
                    Icon::new(IconName::ChevronDown)
                        .with_size(px(9.))
                        .text_color(subtle),
                )
                .into_any_element()
        };
        let text_field = |value: &str| -> AnyElement {
            let me_f = me.clone();
            h_flex()
                .flex_none()
                .items_center()
                .child(
                    div()
                        .w(px(D_RL_PROP_FIELD_W))
                        .h(px(20.))
                        .flex_none()
                        .px(px(5.))
                        .flex()
                        .items_center()
                        .rounded(px(2.))
                        .border_1()
                        .border_color(soft)
                        .bg(if dark { to_hsla(p.chrome) } else { to_hsla(0xfafafa) })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(subtle)
                                .child(value.to_string()),
                        ),
                )
                .child(
                    h_flex()
                        .id(rid(&format!("dtrg-when/{sel}")))
                        .w(px(20.))
                        .h(px(16.))
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded(px(2.))
                        .border_1()
                        .border_color(soft)
                        .bg(bg)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(move |_, window, cx| {
                            me_f.update(cx, move |this, cx| {
                                this.design_open_rule_text(did, sel, 0, window, cx)
                            })
                        })
                        .test_support()
                        .child(div().text_size(px(10.)).text_color(fg).child("…")),
                )
                .into_any_element()
        };
        v_flex()
            .h(px(150.))
            .flex_none()
            .bg(bg)
            .border_t_1()
            .border_color(border)
            .child(
                h_flex()
                    .h(px(26.))
                    .flex_none()
                    .items_center()
                    .px(px(10.))
                    .gap(px(18.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(fg)
                            .pb(px(2.))
                            .border_b_2()
                            .border_color(accent)
                            .child("常规"),
                    )
                    .child(div().text_size(px(12.)).text_color(muted).child("约束")),
            )
            .child(
                v_flex()
                    .flex_none()
                    .pt(px(4.))
                    .pl(px(D_PROP_PAD_L))
                    .child(
                        h_flex()
                            .h(px(D_RL_PROP_ROW_H))
                            .flex_none()
                            .items_center()
                            .child(
                                div()
                                    .w(px(D_RL_PROP_LABEL_W))
                                    .flex_none()
                                    .text_size(px(12.))
                                    .text_color(muted)
                                    .child("当："),
                            )
                            .child(text_field(&when)),
                    )
                    .child(
                        h_flex()
                            .h(px(D_RL_PROP_ROW_H))
                            .flex_none()
                            .items_center()
                            .child(
                                div()
                                    .w(px(D_RL_PROP_LABEL_W))
                                    .flex_none()
                                    .text_size(px(12.))
                                    .text_color(muted)
                                    .child("触发函数："),
                            )
                            .child(combo(&function))
                            .child(combo("")),
                    )
                    .child(
                        h_flex()
                            .h(px(D_RL_PROP_ROW_H))
                            .flex_none()
                            .items_center()
                            .child(
                                div()
                                    .w(px(D_RL_PROP_LABEL_W))
                                    .flex_none()
                                    .text_size(px(12.))
                                    .text_color(muted)
                                    .child("参数："),
                            )
                            .child(text_field(&args)),
                    ),
            )
            .into_any_element()
    }

    /// 当前活动页签若是设计页，返回其 did。
    fn did_of_active_design(&self) -> Option<u64> {
        self.active_table().filter(|t| t.design).map(|t| t.did)
    }

    /// 设计页签的锚点行，越界时夹回最后一行（删行后常见）。
    fn design_sel(ds: &DesignerState) -> usize {
        ds.sel.min(ds.cols.len().saturating_sub(1))
    }

    fn design_add_col(&mut self, did: u64, window: &mut Window, cx: &mut Context<Self>) {
        {
            let Some(tab) = self.opened.iter_mut().find(|t| t.did == did) else {
                return;
            };
            let Some(ds) = tab.dstate.as_mut() else { return };
            ds.cols.push(DesignCol::default());
            ds.sel = ds.cols.len() - 1;
            ds.note = None;
        }
        cx.notify();
        self.spawn_design_editors(did, window, cx);
        self.spawn_index_editors(did, window, cx);
        self.spawn_fk_editors(did, window, cx);
        self.spawn_unique_editors(did, window, cx);
        self.spawn_check_editors(did, window, cx);
        self.spawn_option_editors(did, window, cx);
        self.spawn_exclude_editors(did, window, cx);
        self.spawn_rule_editors(did, window, cx);
        self.spawn_trigger_editors(did, window, cx);
    }

    fn design_del_col(&mut self, did: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.opened.iter_mut().find(|t| t.did == did) else {
            return;
        };
        let Some(ds) = tab.dstate.as_mut() else { return };
        let sel = Self::design_sel(ds);
        if ds.cols.len() <= 1 {
            return;
        }
        ds.cols.remove(sel);
        ds.edits.remove(sel);
        ds.sel = sel.min(ds.cols.len() - 1);
        ds.note = None;
        cx.notify();
    }

    fn design_toggle_pk(&mut self, did: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.opened.iter_mut().find(|t| t.did == did) else {
            return;
        };
        let Some(ds) = tab.dstate.as_mut() else { return };
        let sel = Self::design_sel(ds);
        if let Some(c) = ds.cols.get_mut(sel) {
            c.pk = !c.pk;
            ds.note = None;
        }
        cx.notify();
    }

    fn design_set_not_null(&mut self, did: u64, row: usize, on: bool, cx: &mut Context<Self>) {
        let Some(tab) = self.opened.iter_mut().find(|t| t.did == did) else {
            return;
        };
        let Some(ds) = tab.dstate.as_mut() else { return };
        if let Some(c) = ds.cols.get_mut(row) {
            c.not_null = on;
            ds.note = None;
        }
        cx.notify();
    }

    fn design_save(&mut self, did: u64, cx: &mut Context<Self>) {
        let unnamed_new = self
            .opened
            .iter()
            .find(|t| t.did == did)
            .and_then(|t| t.dstate.as_ref())
            .and_then(|d| d.new_name.clone())
            .is_some_and(|n| n.is_empty());
        if unnamed_new {
            // 新建表还没有名字：先弹「表名」输入框（独立窗口，与 Navicat 的
            // 确认框一致），取消 = 放弃这次保存，主窗口状态不动
            let me = cx.entity();
            open_text_prompt(
                me,
                self.dark,
                "表名",
                "表名",
                String::new(),
                move |app, value| app.finish_new_design_save(did, value),
                cx,
            );
            return;
        }
        let Some(tab) = self.opened.iter_mut().find(|t| t.did == did) else {
            return;
        };
        let Some(ds) = tab.dstate.as_mut() else { return };
        ds.baseline = ds.cols.clone();
        ds.baseline_indexes = ds.indexes.clone();
        ds.baseline_fks = ds.fks.clone();
        ds.baseline_uniques = ds.uniques.clone();
        ds.baseline_checks = ds.checks.clone();
        ds.baseline_options = ds.options.clone();
        ds.baseline_excludes = ds.excludes.clone();
        ds.baseline_rules = ds.rules.clone();
        ds.baseline_triggers = ds.triggers.clone();
        ds.note = Some("已保存（演示数据，不落库）");
        cx.notify();
    }

    /// 当前激活的数据页签；连接已删的页签在这里就地失效（回落对象页）。
    fn active_table(&self) -> Option<OpenTab> {
        let t = self.opened.get(self.active_tab.checked_sub(1)?)?.clone();
        self.conns
            .iter()
            .any(|c| c.name == t.conn_name)
            .then_some(t)
    }

    /// 打开「导出表数据」对话框（SQL/CSV/JSON… + 分块写盘）。
    /// 数据页签在时用它当前工作集；否则回落到树/对象面板里选中的表。
    fn open_table_export(&mut self, cx: &mut Context<Self>) {
        let columns: Vec<String> = mock::MOCK_COLUMNS.iter().map(|c| c.name.to_string()).collect();
        let (table, rows) = if let Some(tab) = self.active_table() {
            let table = self
                .conns
                .iter()
                .find(|c| c.name == tab.conn_name)
                .and_then(|c| mock::objects(c, tab.db, tab.cat).get(tab.obj).map(|s| s.to_string()))
                .unwrap_or_else(|| "export".to_string());
            (table, tab.data)
        } else if let Act::Obj(c, d, _s, k, o) = self.sel {
            let Some(conn) = self.conns.get(c) else { return };
            let Some(db) = mock::databases(conn.kind).get(d).copied() else {
                return;
            };
            let Some(name) = mock::objects(conn, db, k).get(o).copied() else {
                return;
            };
            let rows = materialize_rows(name);
            (name.to_string(), rows)
        } else {
            return;
        };
        table_export_dialog::open_table_export(
            cx.entity().clone(),
            self.dark,
            table,
            columns,
            rows,
            cx,
        );
    }

    /// 打开「导出结果」对话框：数据页签的当前工作集 + mock 列名。
    fn open_export_result(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_table() else {
            return;
        };
        let label = self
            .conns
            .iter()
            .find(|c| c.name == tab.conn_name)
            .and_then(|c| {
                mock::objects(c, tab.db, tab.cat)
                    .get(tab.obj)
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "result".to_string());
        let columns: Vec<String> = mock::MOCK_COLUMNS
            .iter()
            .map(|c| c.name.to_string())
            .collect();
        export_result_dialog::open_export_result(
            cx.entity().clone(),
            self.dark,
            label,
            columns,
            tab.data.clone(),
            cx,
        );
    }

    fn close_tab(&mut self, opened_ix: usize) {
        if opened_ix >= self.opened.len() {
            return;
        }
        self.opened.remove(opened_ix);
        // 窗格侧的页签下标（1 基）整体前移；拖空的非 home 窗格会在这里塌掉
        self.dock.remove_tab(opened_ix + 1);
        self.active_tab = self.dock.focused_active();
    }

    /// 页签标题，照 Navicat 的「对象 @库[.模式]（连接）」拼。
    fn tab_title(&self, t: &OpenTab) -> String {
        self.tab_title_named(t, None)
    }

    /// 同上，但对象名可覆盖——新建表设计页签保存命名后没有目录项可查，
    /// 名字直接来自 [DaroApp::finish_new_design_save] 记下的输入。
    fn tab_title_named(&self, t: &OpenTab, name: Option<&str>) -> String {
        let Some(conn) = self.conns.iter().find(|c| c.name == t.conn_name) else {
            return String::new();
        };
        let obj: &str = match name {
            Some(n) => n,
            None => mock::objects(conn, t.db, t.cat)
                .get(t.obj)
                .copied()
                .unwrap_or_default(),
        };
        let schema = if mock::has_schemas(conn.kind) {
            format!(
                ".{}",
                mock::schemas(conn.kind).get(t.schema).copied().unwrap_or("public")
            )
        } else {
            String::new()
        };
        format!("{obj} @{}{schema} ({})", t.db, conn.name)
    }

    /// 数据页签状态栏的取数 SQL 与「第 x 条记录（共 n 条）」段。
    /// 查询页签没有取数 SQL，状态栏退回「就绪」段。
    fn table_page_status(&self, t: &OpenTab) -> (String, String) {
        if t.query {
            return (String::new(), String::new());
        }
        let Some(conn) = self.conns.iter().find(|c| c.name == t.conn_name) else {
            return (String::new(), String::new());
        };
        let obj = mock::objects(conn, t.db, t.cat)
            .get(t.obj)
            .copied()
            .unwrap_or_default();
        let schema = mock::schemas(conn.kind)
            .get(t.schema)
            .copied()
            .unwrap_or_default();
        let orders: Vec<(usize, bool)> = match t.filter.applied.as_ref() {
            Some(a) if !a.sorts.is_empty() => a.sorts.clone(),
            _ => t.sort.into_iter().collect(),
        };
        let sql = mock::mock_select_sql(
            conn.kind,
            t.db,
            schema,
            obj,
            t.filter
                .applied
                .as_ref()
                .and_then(|a| a.where_sql.as_deref()),
            &orders,
        );
        let rows = visible_data_order(t).len() + t.inserted.len();
        let page = t.cell.0 / PAGE_SIZE + 1;
        if rows == 0 {
            return (sql, "共 0 条记录".to_string());
        }
        let (rec, total) = (t.cell.0.min(rows - 1) + 1, rows);
        (sql, format!("第 {rec} 条记录（共 {total} 条）于第 {page} 页"))
    }

    /// 页码框就绪：把当前页码预填进编辑器并全选，回车 / 失焦提交（见 commit_page_jump）。
    fn start_page_edit(&mut self, page: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.page_editing.is_some() {
            return;
        }
        let state = cx.new(|cx| InputState::new(window, cx).placeholder(String::new()));
        state.update(cx, |s, cx| {
            s.set_value(format!("{}", page + 1), window, cx);
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        cx.subscribe(&state, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit_page_jump(cx),
            _ => {}
        })
        .detach();
        self.page_editing = Some(state);
        cx.notify();
    }

    /// 页码框回车 / 失焦：把输入页码夹进 1..=总页数 后跳到该页首行；
    /// 不是数字就当没输入过。
    fn commit_page_jump(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.page_editing.take() else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        let Ok(n) = text.parse::<usize>() else {
            cx.notify();
            return;
        };
        if self.active_tab > 0
            && let Some(tab) = self.opened.get_mut(self.active_tab - 1)
        {
            let total = tab.data.len() + tab.inserted.len();
            let pages = total.div_ceil(PAGE_SIZE).max(1);
            tab.cell.0 = (n.clamp(1, pages) - 1) * PAGE_SIZE;
        }
        cx.notify();
    }

    /// 格子上的双击判定（同 [DaroApp::is_double_click] 的口径，键是格位）。
    fn is_cell_double_click(&mut self, at: (usize, usize)) -> bool {
        let now = Instant::now();
        let double = self.last_cell_click.is_some_and(|(prev, at0)| {
            prev == at && now.duration_since(at0) < DOUBLE_CLICK_INTERVAL
        });
        if std::env::var("DARO_DEBUG").is_ok() {
            eprintln!("[dbg] cell double={double} at={at:?}");
        }
        self.last_cell_click = if double { None } else { Some((at, now)) };
        double
    }

    /// 双击格子进就地编辑：先提交还开着的上一格，用当前值预填编辑器并
    /// 全选（Navicat 同款），回车 / 失焦提交（commit_cell_edit）、Esc 取消。
    fn start_cell_edit(
        &mut self,
        disp: usize,
        cix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab == 0 {
            return;
        }
        if self
            .cell_editing
            .as_ref()
            .is_some_and(|e| e.at == (disp, cix))
        {
            return;
        }
        self.commit_cell_edit(cx);
        let Some(t) = self.opened.get(self.active_tab - 1) else {
            return;
        };
        if t.design || t.query {
            return;
        }
        let tab = (
            t.conn_name.clone(),
            t.db,
            t.schema,
            t.cat,
            t.obj,
        );
        let order = visible_data_order(t);
        let value = if disp < order.len() {
            t.data
                .get(order[disp])
                .and_then(|row| row.get(cix))
                .cloned()
                .unwrap_or_default()
        } else {
            t.inserted
                .get(disp - order.len())
                .and_then(|row| row.get(cix))
                .cloned()
                .unwrap_or_default()
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |s, cx| {
            s.set_value(value, window, cx);
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        cx.subscribe(&input, |this, _, ev: &InputEvent, cx| {
            if std::env::var("DARO_DEBUG").is_ok() {
                let kind = match ev {
                    InputEvent::Change => "change",
                    InputEvent::PressEnter { .. } => "enter",
                    InputEvent::Focus => "focus",
                    InputEvent::Blur => "blur",
                };
                eprintln!("[dbg] cell edit event {kind}");
            }
            match ev {
                InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit_cell_edit(cx),
                _ => {}
            }
        })
        .detach();
        self.cell_editing = Some(CellEdit {
            tab,
            at: (disp, cix),
            input,
        });
        cx.notify();
    }

    /// 回车 / 失焦：把编辑值写回所属页签的数据。值没变不动 dirty；
    /// 排序态下 disp 经 order 换算回原行。新增行（disp 落在 data 之外）
    /// 直接写 inserted。
    fn commit_cell_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.cell_editing.take() else {
            return;
        };
        let value = edit.input.read(cx).value().to_string();
        let tab_ix = self.opened.iter().position(|t| {
            !t.design
                && !t.query
                && t.conn_name == edit.tab.0
                && t.db == edit.tab.1
                && t.schema == edit.tab.2
                && t.cat == edit.tab.3
                && t.obj == edit.tab.4
        });
        if let Some(t) = tab_ix.and_then(|ix| self.opened.get_mut(ix)) {
            let order = visible_data_order(t);
            let row = if edit.at.0 < order.len() {
                t.data.get_mut(order[edit.at.0])
            } else {
                t.inserted.get_mut(edit.at.0 - order.len())
            };
            if let Some(row) = row
                && let Some(cell) = row.get_mut(edit.at.1)
                && *cell != value
            {
                *cell = value;
            }
        }
        cx.notify();
    }

    /// Esc：丢弃编辑，格子退回原值（不写 dirty）。
    fn cancel_cell_edit(&mut self, cx: &mut Context<Self>) {
        if self.cell_editing.take().is_some() {
            cx.notify();
        }
    }

    /// 导航条 +：网格末尾追加一行空行并选中它。新行贴在尾部（排序结果
    /// 之后）、行头带 * 标记；「应用」✓ 后并进 data 参与排序。
    fn grid_insert_row(&mut self, cx: &mut Context<Self>) {
        if self.active_tab == 0 {
            return;
        }
        self.commit_cell_edit(cx);
        if let Some(t) = self.opened.get_mut(self.active_tab - 1) {
            // 新行贴在「可见行」之后：筛选态下 = 过滤后的数据行数 + 已有新增
            let vis = visible_data_order(t).len();
            t.inserted.push(vec![String::new(); mock::MOCK_COLUMNS.len()]);
            t.cell = (vis + t.inserted.len() - 1, 0);
        }
        cx.notify();
    }

    /// 导航条 −：删除当前格所在行。新增未应用的行直接移除；已有行从
    /// 工作集移除，「应用」前 × 都能反悔（mock 档不做 Navicat 的删除标记态）。
    fn grid_delete_row(&mut self, cx: &mut Context<Self>) {
        if self.active_tab == 0 {
            return;
        }
        self.commit_cell_edit(cx);
        if let Some(t) = self.opened.get_mut(self.active_tab - 1) {
            let order = visible_data_order(t);
            let disp = t.cell.0;
            if disp < order.len() {
                t.data.remove(order[disp]);
            } else if disp < order.len() + t.inserted.len() {
                t.inserted.remove(disp - order.len());
            }
            let total = visible_data_order(t).len() + t.inserted.len();
            t.cell.0 = t.cell.0.min(total.saturating_sub(1));
        }
        cx.notify();
    }

    /// 导航条 ✓（应用）：把工作集固定成新基线（mock 档没有真库可写，
    /// 这一步等于「保存成功」，✓ / × 随即变灰，页签 * 摘掉）。
    fn grid_apply(&mut self, cx: &mut Context<Self>) {
        if self.active_tab == 0 {
            return;
        }
        self.commit_cell_edit(cx);
        if let Some(t) = self.opened.get_mut(self.active_tab - 1) {
            if !t.inserted.is_empty() {
                t.data.extend(t.inserted.drain(..));
            }
            t.baseline = t.data.clone();
        }
        cx.notify();
    }

    /// 导航条 × / 刷新：丢弃工作集，退回上次「应用」的基线。mock 档里
    /// 「刷新」= 重新取数，取回来的正是基线，两个按钮共用这条路径。
    fn grid_revert(&mut self, cx: &mut Context<Self>) {
        if self.active_tab == 0 {
            return;
        }
        self.cancel_cell_edit(cx);
        if let Some(t) = self.opened.get_mut(self.active_tab - 1) {
            t.data = t.baseline.clone();
            t.inserted.clear();
            // 筛选面板的应用态刻意保留：Navicat 的「刷新」= 重发带 WHERE 的
            // 取数，面板条件不清
            let total = visible_data_order(t).len();
            t.cell.0 = t.cell.0.min(total.saturating_sub(1));
        }
        cx.notify();
    }

    /// F5 刷新当前视图。数据页签 = 重新取数（mock 档取回基线，走 × 的
    /// 同一条路径）；树 / 对象面板的目录在 mock 档是静态的、设计 / 查询
    /// 页签没有「数据」可重取，重渲染即可 = 无操作。
    fn refresh_view(&mut self, cx: &mut Context<Self>) {
        if self.active_tab == 0 {
            return;
        }
        if self
            .opened
            .get(self.active_tab - 1)
            .is_some_and(|t| !t.design && !t.query)
        {
            self.grid_revert(cx);
        }
    }

    /// Delete 键：删除当前选中项。数据页 = 删除选中行；树 / 对象面板 / 设计器
    /// 在 mock 档不实际删除（连接与表结构是静态数据），仅消费按键避免冒泡到
    /// 系统产生蜂鸣。
    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_data_tab_mut() {
            let row = tab.cell.0;
            if row < tab.data.len() {
                // 删除选中行：从 data 里移除
                tab.data.remove(row);
                // 调整选中位置
                tab.cell.0 = row.min(tab.data.len().saturating_sub(1));
                cx.notify();
            }
        }
        // 其它上下文（树、设计器）暂不实现，仅消费按键
    }

    /// Ctrl+S：保存当前页签。设计页 = 走保存流程（弹表名框或落盘）；数据页
    /// = 应用变更（同 ✓ 按钮）；查询页不保存（无持久化对象）。
    fn save_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab == 0 {
            return;
        }
        let Some(tab) = self.opened.get(self.active_tab - 1) else {
            return;
        };
        if tab.design {
            // 设计页：触发保存（复用工具栏保存按钮的逻辑）
            let did = tab.did;
            drop(tab); // 释放借用
            self.design_save(did, cx);
        } else if !tab.query {
            // 数据页：应用变更
            drop(tab); // 释放借用
            self.grid_apply(cx);
        }
        // 查询页不响应 Ctrl+S
        let _ = window; // 消费 window 参数避免警告
    }

    // ── 筛选面板（数据网格工具条漏斗） ────────────────────────────────
    // Navicat Lite 实测：漏斗图标 + 4 态徽标（无徽标 / 已编辑铅笔 / 应用成功绿勾
    // / 应用失败红 ! / 带排序的排序线）；面板分「创建工具」与「文本」两模式，
    // 准则行 = ☑ 启用 + 列名下拉 + 算子下拉 + 值点击弹窗 + × 删除；组行 =
    // 「(」+ 缩进子行 + 「)」，× 整组删；连接词 "and"/"or" 在行间可点切换。

    fn active_data_tab(&self) -> Option<&OpenTab> {
        self.opened
            .get(self.active_tab.checked_sub(1)?)
            .filter(|t| !t.design && !t.query)
    }

    fn active_data_tab_mut(&mut self) -> Option<&mut OpenTab> {
        let ix = self.active_tab.checked_sub(1)?;
        self.opened
            .get_mut(ix)
            .filter(|t| !t.design && !t.query)
    }

    /// 漏斗按钮：开关态 + 4 态徽标。
    fn filter_funnel_btn(&self, t: &OpenTab, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let hover = to_hsla(p.hover);
        let open = t.filter.open;
        let fg = if open {
            to_hsla(p.accent)
        } else {
            to_hsla(p.muted_foreground)
        };
        // 徽标优先级：error > edited > applied-with-sorts > applied-ok
        let badge: Option<(IconName, u32)> = if t.filter.error.is_some() {
            Some((IconName::CircleAlert, 0xd1242f))
        } else if t.filter.edited {
            Some((IconName::Pencil, p.muted_foreground))
        } else if let Some(a) = t.filter.applied.as_ref() {
            if !a.sorts.is_empty() {
                Some((IconName::ArrowUpDown, p.muted_foreground))
            } else {
                Some((IconName::Check, 0x2ea44f))
            }
        } else {
            None
        };
        div()
            .id(rid("tp/filter"))
            .size(px(24.))
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(2.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(|this, _, _, cx| {
                this.filter_toggle(cx);
            }))
            .child(Icon::new(IconName::Funnel).with_size(px(14.)).text_color(fg))
            .when_some(badge, |this, (icon, color)| {
                this.child(
                    div()
                        .absolute()
                        .bottom(px(-1.))
                        .right(px(-1.))
                        .bg(to_hsla(p.background))
                        .rounded_full()
                        .child(
                            Icon::new(icon)
                                .with_size(px(9.))
                                .text_color(to_hsla(color)),
                        ),
                )
            })
    }

    /// 数据页工具条上的「筛选 & 排序」：图标 + 文字，带同样的状态徽标。
    fn filter_bar_btn(&self, t: &OpenTab, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let hover = to_hsla(p.hover);
        let open = t.filter.open;
        let fg = to_hsla(p.foreground);
        let badge: Option<(IconName, u32)> = if t.filter.error.is_some() {
            Some((IconName::CircleAlert, 0xd1242f))
        } else if t.filter.edited {
            Some((IconName::Pencil, p.muted_foreground))
        } else if let Some(a) = t.filter.applied.as_ref() {
            if !a.sorts.is_empty() {
                Some((IconName::ArrowUpDown, p.muted_foreground))
            } else {
                Some((IconName::Check, 0x2ea44f))
            }
        } else {
            None
        };
        h_flex()
            .id(rid("tp/filter"))
            .relative()
            .h(px(22.))
            .flex_none()
            .px(px(5.))
            .gap(px(4.))
            .items_center()
            .rounded(px(2.))
            .cursor_pointer()
            .when(open, move |s| s.bg(to_hsla(p.selection)))
            .when(!open, move |s| s.hover(move |s| s.bg(hover)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.filter_toggle(cx);
            }))
            .child(
                div()
                    .relative()
                    .flex_none()
                    .w(px(15.))
                    .h(px(15.))
                    .child(art::icon(art::Glyph::FilterSort, 15., to_hsla(p.muted_foreground)))
                    .when_some(badge, |this, (icon, color)| {
                        this.child(
                            div()
                                .absolute()
                                .bottom(px(-2.))
                                .right(px(-3.))
                                .rounded_full()
                                .child(Icon::new(icon).with_size(px(9.)).text_color(to_hsla(color))),
                        )
                    }),
            )
            .child(div().text_size(px(12.)).text_color(fg).child("筛选 & 排序"))
    }

    fn filter_toggle(&mut self, cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            t.filter.open = !t.filter.open;
            cx.notify();
        }
    }

    fn filter_add_crit(&mut self, cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            let crit = filter::FCrit::new(0);
            if !t.filter.model.items.is_empty() {
                t.filter.model.conns.push(true);
            }
            t.filter.model.items.push(filter::FNode::Crit(crit));
            t.filter.edited = true;
            cx.notify();
        }
    }

    fn filter_add_group(&mut self, cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            let g = filter::FNode::Group {
                conns: vec![],
                children: vec![filter::FNode::Crit(filter::FCrit::new(0))],
            };
            if !t.filter.model.items.is_empty() {
                t.filter.model.conns.push(true);
            }
            t.filter.model.items.push(g);
            t.filter.sel = Some(vec![t.filter.model.items.len() - 1]);
            t.filter.edited = true;
            cx.notify();
        }
    }

    fn filter_del_node(&mut self, path: &[usize], cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            if path.len() == 1 {
                let ix = path[0];
                if ix < t.filter.model.items.len() {
                    t.filter.model.items.remove(ix);
                    if ix < t.filter.model.conns.len() {
                        t.filter.model.conns.remove(ix);
                    } else if !t.filter.model.conns.is_empty() && ix == t.filter.model.items.len()
                    {
                        t.filter.model.conns.pop();
                    }
                }
            } else if path.len() == 2 {
                let top = path[0];
                let child_ix = path[1];
                if let Some(filter::FNode::Group { children, conns }) =
                    t.filter.model.items.get_mut(top)
                {
                    if child_ix < children.len() {
                        children.remove(child_ix);
                        if child_ix < conns.len() {
                            conns.remove(child_ix);
                        } else if !conns.is_empty() && child_ix == children.len() {
                            conns.pop();
                        }
                    }
                }
            }
            t.filter.edited = true;
            t.filter.sel = None;
            cx.notify();
        }
    }

    fn filter_toggle_enable(&mut self, path: &[usize], cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            if let Some(filter::FNode::Crit(c)) = filter_node_at_mut(&mut t.filter.model, path) {
                c.enabled = !c.enabled;
                t.filter.edited = true;
                cx.notify();
            }
        }
    }

    fn filter_toggle_conn(&mut self, top_ix: usize, cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            if top_ix < t.filter.model.conns.len() {
                t.filter.model.conns[top_ix] = !t.filter.model.conns[top_ix];
                t.filter.edited = true;
                cx.notify();
            }
        }
    }

    fn filter_set_col(&mut self, path: &[usize], col: usize, cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            if let Some(filter::FNode::Crit(c)) = filter_node_at_mut(&mut t.filter.model, path) {
                c.col = col;
                c.op = filter::default_op(col);
                c.v1.clear();
                c.v2.clear();
                t.filter.edited = true;
                cx.notify();
            }
        }
    }

    fn filter_set_op(&mut self, path: &[usize], op: filter::FOp, cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            if let Some(filter::FNode::Crit(c)) = filter_node_at_mut(&mut t.filter.model, path) {
                c.op = op;
                let (need1, need2) = op.arity();
                if !need1 {
                    c.v1.clear();
                }
                if !need2 {
                    c.v2.clear();
                }
                t.filter.edited = true;
                cx.notify();
            }
        }
    }

    fn filter_open_value(
        &mut self,
        path: Vec<usize>,
        which: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(t) = self.active_data_tab() else { return };
        let Some(filter::FNode::Crit(c)) = filter_node_at(&t.filter.model, &path) else {
            return;
        };
        let col = c.col;
        let col_name = mock::MOCK_COLUMNS
            .get(col)
            .map(|m| m.name)
            .unwrap_or("?");
        let title: &'static str = Box::leak(format!("输入筛选值 - {col_name}").into_boxed_str());
        let v1 = c.v1.clone();
        let v2 = if c.op.arity().1 { Some(c.v2.clone()) } else { None };
        let distinct = filter::distinct_values(&t.data, col);
        let parent = cx.entity().clone();
        let dark = self.dark;
        filter_value_dialog::open_filter_value(
            parent,
            dark,
            title,
            v1,
            v2,
            distinct,
            move |app, v1, v2| {
                app.filter_set_value(&path, which, v1, v2);
            },
            cx,
        );
        let _ = window;
    }

    fn filter_set_value(
        &mut self,
        path: &[usize],
        _which: u8,
        v1: String,
        v2: Option<String>,
    ) {
        if let Some(t) = self.active_data_tab_mut() {
            if let Some(filter::FNode::Crit(c)) = filter_node_at_mut(&mut t.filter.model, path) {
                c.v1 = v1;
                if let Some(v2) = v2 {
                    c.v2 = v2;
                }
                t.filter.edited = true;
            }
        }
    }

    fn filter_apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let conn_name = self.active_data_tab().map(|t| t.conn_name.clone());
        let Some(conn_name) = conn_name else { return };
        let conn_kind = self
            .conns
            .iter()
            .find(|c| c.name == conn_name)
            .map(|c| c.kind)
            .unwrap_or(mock::DbKind::MySql);
        let Some(t) = self.active_data_tab_mut() else { return };
        if t.filter.text_mode {
            let text = t
                .filter
                .text_editor
                .as_ref()
                .map(|e| e.read(cx).value().to_string())
                .unwrap_or_default();
            match filter::parse_where(conn_kind, &text) {
                Ok(model) => {
                    let where_sql = filter::where_sql(conn_kind, &model);
                    let order_sql = filter::order_by_sql(conn_kind, &t.filter.sorts);
                    t.filter.applied = Some(filter::Applied {
                        model,
                        where_sql,
                        order_sql,
                        sorts: t.filter.sorts.clone(),
                        has_custom: t.filter.model.has_custom(),
                    });
                    t.filter.edited = false;
                    t.filter.error = None;
                    t.filter.model = t.filter.applied.as_ref().unwrap().model.clone();
                }
                Err(e) => {
                    t.filter.error = Some(e.clone());
                    prompt_dialog::open_confirm(
                        cx.entity(),
                        self.dark,
                        "应用筛选",
                        e,
                        |_| {},
                        cx,
                    );
                }
            }
        } else {
            let where_sql = filter::where_sql(conn_kind, &t.filter.model);
            let order_sql = filter::order_by_sql(conn_kind, &t.filter.sorts);
            t.filter.applied = Some(filter::Applied {
                model: t.filter.model.clone(),
                where_sql,
                order_sql,
                sorts: t.filter.sorts.clone(),
                has_custom: t.filter.model.has_custom(),
            });
            t.filter.edited = false;
            t.filter.error = None;
        }
        let _ = window;
        cx.notify();
    }

    fn filter_clear(&mut self, cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            t.filter.applied = None;
            t.filter.edited = false;
            t.filter.error = None;
            cx.notify();
        }
    }

    fn filter_add_sort(&mut self, cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            t.filter.sorts.push((0, true));
            t.filter.edited = true;
            cx.notify();
        }
    }

    fn filter_del_sort(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            if ix < t.filter.sorts.len() {
                t.filter.sorts.remove(ix);
                t.filter.edited = true;
                cx.notify();
            }
        }
    }

    fn filter_set_sort_col(&mut self, ix: usize, col: usize, cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            if let Some(s) = t.filter.sorts.get_mut(ix) {
                s.0 = col;
                t.filter.edited = true;
                cx.notify();
            }
        }
    }

    fn filter_toggle_sort_dir(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(t) = self.active_data_tab_mut() {
            if let Some(s) = t.filter.sorts.get_mut(ix) {
                s.1 = !s.1;
                t.filter.edited = true;
            }
        }
        cx.notify();
    }

    fn filter_switch_mode(&mut self, text_mode: bool, cx: &mut Context<Self>) {
        let conn_name = self.active_data_tab().map(|t| t.conn_name.clone());
        let Some(conn_name) = conn_name else { return };
        let conn_kind = self
            .conns
            .iter()
            .find(|c| c.name == conn_name)
            .map(|c| c.kind)
            .unwrap_or(mock::DbKind::MySql);
        if let Some(t) = self.active_data_tab_mut() {
            if t.filter.text_mode == text_mode {
                return;
            }
            if text_mode {
                // 切到文本模式：把当前模型序列化成 WHERE 正文
                t.filter.text_sql = filter::where_sql(conn_kind, &t.filter.model)
                    .unwrap_or_default();
            } else {
                // 切回创建工具：解析文本；失败保留文本模式 + 报错
                let text = t
                    .filter
                    .text_editor
                    .as_ref()
                    .map(|e| e.read(cx).value().to_string())
                    .unwrap_or_else(|| t.filter.text_sql.clone());
                match filter::parse_where(conn_kind, &text) {
                    Ok(m) => {
                        t.filter.model = m;
                        t.filter.text_mode = false;
                        t.filter.edited = true;
                    }
                    Err(e) => {
                        t.filter.error = Some(e);
                    }
                }
            }
            cx.notify();
        }
    }

    fn filter_pane(&self, t: &OpenTab, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let (bg, border, soft, fg, muted, subtle, accent, selection) = (
            to_hsla(p.background),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            p.accent,
            to_hsla(p.selection),
        );
        let hover = to_hsla(p.hover);
        let band = to_hsla(blend(accent, p.background, 0.92));
        let _ = bg;
        let _ = selection;
        let _ = soft;
        let _ = border;

        // ── 头部：筛选 + 模式单选 + 关闭 ──
        let header = h_flex()
            .h(px(26.))
            .flex_none()
            .px(px(8.))
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(to_hsla(p.border_soft))
            .child(
                div()
                    .text_size(px(12.))
                    .font_medium()
                    .text_color(fg)
                    .child("筛选"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id(rid("fp/mode-tool"))
                    .px(px(6.))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .rounded(px(2.))
                    .when(!t.filter.text_mode, |s| s.bg(to_hsla(p.hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.filter_switch_mode(false, cx);
                    }))
                    .child(
                        div()
                            .w(px(10.))
                            .h(px(10.))
                            .rounded_full()
                            .border_1()
                            .border_color(if !t.filter.text_mode {
                                to_hsla(accent)
                            } else {
                                muted
                            })
                            .when(!t.filter.text_mode, |s| {
                                s.bg(to_hsla(accent))
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(fg)
                            .child("创建工具"),
                    ),
            )
            .child(
                div()
                    .id(rid("fp/mode-text"))
                    .px(px(6.))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .rounded(px(2.))
                    .when(t.filter.text_mode, |s| s.bg(to_hsla(p.hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.filter_switch_mode(true, cx);
                    }))
                    .child(
                        div()
                            .w(px(10.))
                            .h(px(10.))
                            .rounded_full()
                            .border_1()
                            .border_color(if t.filter.text_mode {
                                to_hsla(accent)
                            } else {
                                muted
                            })
                            .when(t.filter.text_mode, |s| s.bg(to_hsla(accent))),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(fg)
                            .child("文本"),
                    ),
            )
            .child(
                div()
                    .id(rid("fp/close"))
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(2.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.filter_toggle(cx);
                    }))
                    .child(
                        Icon::new(IconName::X)
                            .with_size(px(12.))
                            .text_color(muted),
                    ),
            );

        // ── 准则带 ──
        let criteria_band = self.filter_criteria_band(t, band, fg, muted, subtle, accent, hover, cx);

        // ── 排序方式 ──
        let sort_section = self.filter_sort_section(t, fg, muted, accent, hover, cx);

        // ── 底部：应用 + 状态 ──
        let footer = h_flex()
            .h(px(28.))
            .flex_none()
            .px(px(8.))
            .items_center()
            .gap(px(8.))
            .border_t_1()
            .border_color(to_hsla(p.border_soft))
            .child(
                div()
                    .id(rid("fp/apply"))
                    .px(px(10.))
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(2.))
                    .bg(to_hsla(accent))
                    .text_color(to_hsla(0xffffff))
                    .text_size(px(11.5))
                    .cursor_pointer()
                    .hover(move |s| s.bg(to_hsla(blend(accent, 0, 0.85))))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.filter_apply(window, cx);
                    }))
                    .child("应用筛选"),
            )
            .child(if let Some(e) = t.filter.error.as_ref() {
                div()
                    .text_size(px(11.))
                    .text_color(to_hsla(0xd1242f))
                    .child(e.clone())
            } else if t.filter.edited {
                div()
                    .text_size(px(11.))
                    .text_color(muted)
                    .child("已编辑准则")
            } else if t.filter.applied.as_ref().is_some_and(|a| a.has_custom) {
                div()
                    .text_size(px(11.))
                    .text_color(muted)
                    .child("自定义准则不参与本地行匹配")
            } else {
                div()
            })
            .child(div().flex_1())
            .when(t.filter.applied.is_some(), |this| {
                this.child(
                    div()
                        .id(rid("fp/clear"))
                        .px(px(8.))
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(2.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.filter_clear(cx);
                        }))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(muted)
                                .child("清除"),
                        ),
                )
            });

        v_flex()
            .flex_none()
            .h(px(198.))
            .bg(to_hsla(p.background))
            .border_b_1()
            .border_color(to_hsla(p.border_soft))
            .child(header)
            .child(criteria_band)
            .child(sort_section)
            .child(footer)
            .into_any_element()
    }

    fn filter_criteria_band(
        &self,
        t: &OpenTab,
        band: gpui::Hsla,
        fg: gpui::Hsla,
        muted: gpui::Hsla,
        subtle: gpui::Hsla,
        accent: u32,
        hover: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = band;
        let _ = subtle;
        let items = &t.filter.model.items;
        let conns = &t.filter.model.conns;
        if items.is_empty() {
            return v_flex()
                .flex_1()
                .min_h_0()
                .bg(band)
                .px(px(8.))
                .py(px(6.))
                .child(
                    div()
                        .id(rid("fp/add-crit-empty"))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .hover(move |s| s.text_color(to_hsla(accent)))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.filter_add_crit(cx);
                        }))
                        .child(
                            Icon::new(IconName::Plus)
                                .with_size(px(12.))
                                .text_color(to_hsla(accent)),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(muted)
                                .child("点击 \"+\" 以添加筛选准则"),
                        ),
                )
                .into_any_element();
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        for (ix, node) in items.iter().enumerate() {
            rows.push(self.filter_criterion_row(
                t,
                &[ix],
                node,
                fg,
                muted,
                accent,
                hover,
                cx,
            ));
            if ix < conns.len() {
                let and = conns[ix];
                let conn_ix = ix;
                rows.push(
                    div()
                        .id(rid(&format!("fp/conn/{conn_ix}")))
                        .pl(px(28.))
                        .py(px(2.))
                        .cursor_pointer()
                        .hover(move |s| s.text_color(to_hsla(accent)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.filter_toggle_conn(conn_ix, cx);
                        }))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(muted)
                                .child(if and { "and" } else { "or" }),
                        )
                        .into_any_element(),
                );
            }
        }
        // 工具条：+ 准则 / ( 组
        let tools = h_flex()
            .px(px(6.))
            .py(px(4.))
            .gap(px(4.))
            .child(
                div()
                    .id(rid("fp/add-crit"))
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(2.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.filter_add_crit(cx);
                    }))
                    .child(
                        Icon::new(IconName::Plus)
                            .with_size(px(12.))
                            .text_color(muted),
                    ),
            )
            .child(
                div()
                    .id(rid("fp/add-group"))
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(2.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.filter_add_group(cx);
                    }))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(muted)
                            .child("( )"),
                    ),
            );
        v_flex()
            .flex_1()
            .min_h_0()
            .bg(band)
            .overflow_y_scrollbar()
            .children(rows)
            .child(tools)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn filter_criterion_row(
        &self,
        t: &OpenTab,
        path: &[usize],
        node: &filter::FNode,
        fg: gpui::Hsla,
        muted: gpui::Hsla,
        accent: u32,
        hover: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let me = cx.entity().clone();
        match node {
            filter::FNode::Crit(c) => {
                let col_name = mock::MOCK_COLUMNS
                    .get(c.col)
                    .map(|m| m.name)
                    .unwrap_or("?");
                let op_label = c.op.label();
                let value_text = c.value_text(1);
                let value2_text = if c.op.arity().1 {
                    Some(c.value_text(2))
                } else {
                    None
                };
                let enabled = c.enabled;
                let _path_owned = path.to_vec();
                let path_del = path.to_vec();
                let path_col = path.to_vec();
                let path_op = path.to_vec();
                let path_val1 = path.to_vec();
                let path_val2 = path.to_vec();
                let path_en = path.to_vec();
                let me_en = me.clone();
                let me_col = me.clone();
                let me_op = me.clone();
                h_flex()
                    .h(px(22.))
                    .flex_none()
                    .px(px(6.))
                    .items_center()
                    .gap(px(6.))
                    .child(
                        Checkbox::new(("fp-en", path[0]))
                            .checked(enabled)
                            .on_click(move |checked, _, app_cx| {
                                let checked = *checked;
                                let p = path_en.clone();
                                me_en.update(app_cx, |this, cx| {
                                    if let Some(t) = this.active_data_tab_mut() {
                                        if let Some(filter::FNode::Crit(c)) =
                                            filter_node_at_mut(&mut t.filter.model, &p)
                                        {
                                            c.enabled = checked;
                                            t.filter.edited = true;
                                        }
                                    }
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        Button::new(rid(&format!("fp/col/{}", path_col[0])))
                            .ghost()
                            .with_size(gpui_kit::component::Size::XSmall)
                            .min_w(px(60.))
                            .px(px(4.))
                            .h(px(18.))
                            .rounded(px(2.))
                            .text_color(to_hsla(accent))
                            .label(col_name)
                            .dropdown_menu(move |menu, _window, _cx| {
                                let mut m = menu;
                                for (ix, col) in mock::MOCK_COLUMNS.iter().enumerate() {
                                    let p = path_col.clone();
                                    let me = me_col.clone();
                                    m = m.item(
                                        PopupMenuItem::new(col.name)
                                            .on_click(move |_, _, app_cx| {
                                                me.update(app_cx, |this, cx| {
                                                    this.filter_set_col(&p, ix, cx);
                                                });
                                            }),
                                    );
                                }
                                m
                            }),
                    )
                    .child(
                        Button::new(rid(&format!("fp/op/{}", path_op[0])))
                            .ghost()
                            .with_size(gpui_kit::component::Size::XSmall)
                            .min_w(px(100.))
                            .px(px(4.))
                            .h(px(18.))
                            .rounded(px(2.))
                            .text_color(to_hsla(accent))
                            .label(op_label)
                            .dropdown_menu(move |menu, _window, _cx| {
                                let mut m = menu;
                                for op in filter::ALL_OPS {
                                    let p = path_op.clone();
                                    let me = me_op.clone();
                                    m = m.item(
                                        PopupMenuItem::new(op.label())
                                            .on_click(move |_, _, app_cx| {
                                                me.update(app_cx, |this, cx| {
                                                    this.filter_set_op(&p, op, cx);
                                                });
                                            }),
                                    );
                                }
                                m
                            }),
                    )
                    .child(
                        div()
                            .id(rid(&format!("fp/v1/{}", path_val1[0])))
                            .min_w(px(80.))
                            .px(px(4.))
                            .h(px(18.))
                            .flex()
                            .items_center()
                            .rounded(px(2.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.filter_open_value(path_val1.clone(), 1, window, cx);
                            }))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(to_hsla(accent))
                                    .child(value_text),
                            ),
                    )
                    .when_some(value2_text, |this, v2| {
                        let path_v2 = path_val2.clone();
                        this.child(
                            div()
                                .text_size(px(11.))
                                .text_color(muted)
                                .child("到"),
                        )
                        .child(
                            div()
                                .id(rid(&format!("fp/v2/{}", path_v2[0])))
                                .min_w(px(80.))
                                .px(px(4.))
                                .h(px(18.))
                                .flex()
                                .items_center()
                                .rounded(px(2.))
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.filter_open_value(path_v2.clone(), 2, window, cx);
                                }))
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(to_hsla(accent))
                                        .child(v2),
                                ),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .id(rid(&format!("fp/del/{}", path_del[0])))
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(2.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.filter_del_node(&path_del, cx);
                            }))
                            .child(
                                Icon::new(IconName::X)
                                    .with_size(px(11.))
                                    .text_color(muted),
                            ),
                    )
                    .into_any_element()
            }
            filter::FNode::Group { children, conns } => {
                let path_del = path.to_vec();
                let mut rows: Vec<AnyElement> = Vec::new();
                rows.push(
                    h_flex()
                        .h(px(22.))
                        .flex_none()
                        .px(px(6.))
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(fg)
                                .child("("),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .id(rid(&format!("fp/del-g/{}/{}", path[0], path.get(1).copied().unwrap_or(0))))
                                .size(px(18.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(2.))
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.filter_del_node(&path_del, cx);
                                }))
                                .child(
                                    Icon::new(IconName::X)
                                        .with_size(px(11.))
                                        .text_color(muted),
                                ),
                        )
                        .into_any_element(),
                );
                for (ix, child) in children.iter().enumerate() {
                    let mut child_path = path.to_vec();
                    child_path.push(ix);
                    rows.push(self.filter_criterion_row(
                        t,
                        &child_path,
                        child,
                        fg,
                        muted,
                        accent,
                        hover,
                        cx,
                    ));
                    if ix < conns.len() {
                        let and = conns[ix];
                        let top = path[0];
                        let gix = ix;
                        rows.push(
                            div()
                                .id(rid(&format!("fp/gconn/{top}/{gix}")))
                                .pl(px(28.))
                                .py(px(2.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(muted)
                                        .child(if and { "and" } else { "or" }),
                                )
                                .into_any_element(),
                        );
                    }
                }
                rows.push(
                    h_flex()
                        .h(px(22.))
                        .flex_none()
                        .px(px(6.))
                        .items_center()
                        .child(
                            div()
                                .pl(px(6.))
                                .text_size(px(12.))
                                .text_color(fg)
                                .child(")"),
                        )
                        .into_any_element(),
                );
                v_flex()
                    .pl(px(12.))
                    .children(rows)
                    .into_any_element()
            }
        }
    }

    fn filter_sort_section(
        &self,
        t: &OpenTab,
        fg: gpui::Hsla,
        muted: gpui::Hsla,
        accent: u32,
        hover: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let me = cx.entity().clone();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (ix, (col, asc)) in t.filter.sorts.iter().enumerate() {
            let col_name = mock::MOCK_COLUMNS
                .get(*col)
                .map(|m| m.name)
                .unwrap_or("?");
            let dir_text = if *asc { "升序" } else { "降序" };
            let ix_del = ix;
            let ix_col = ix;
            let ix_dir = ix;
            let me_sort = me.clone();
            rows.push(
                h_flex()
                    .h(px(22.))
                    .flex_none()
                    .px(px(6.))
                    .items_center()
                    .gap(px(6.))
                    .child(
                        Button::new(rid(&format!("fp/sort-col/{ix_col}")))
                            .ghost()
                            .with_size(gpui_kit::component::Size::XSmall)
                            .min_w(px(60.))
                            .px(px(4.))
                            .h(px(18.))
                            .rounded(px(2.))
                            .text_color(to_hsla(accent))
                            .label(col_name)
                            .dropdown_menu(move |menu, _window, _cx| {
                                let mut m = menu;
                                for (cix, col) in mock::MOCK_COLUMNS.iter().enumerate() {
                                    let ix = ix_col;
                                    let me = me_sort.clone();
                                    m = m.item(
                                        PopupMenuItem::new(col.name).on_click(move |_, _, app_cx| {
                                            me.update(app_cx, |this, cx| {
                                                this.filter_set_sort_col(ix, cix, cx);
                                            });
                                        }),
                                    );
                                }
                                m
                            }),
                    )
                    .child(
                        div()
                            .id(rid(&format!("fp/sort-dir/{ix_dir}")))
                            .px(px(4.))
                            .h(px(18.))
                            .flex()
                            .items_center()
                            .rounded(px(2.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.filter_toggle_sort_dir(ix_dir, cx);
                            }))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(to_hsla(accent))
                                    .child(dir_text),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id(rid(&format!("fp/sort-del/{ix_del}")))
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(2.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.filter_del_sort(ix_del, cx);
                            }))
                            .child(
                                Icon::new(IconName::X)
                                    .with_size(px(11.))
                                    .text_color(muted),
                            ),
                    )
                    .into_any_element(),
            );
        }
        v_flex()
            .flex_none()
            .border_t_1()
            .border_color(to_hsla(self.pal().border_soft))
            .child(
                h_flex()
                    .h(px(24.))
                    .px(px(8.))
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_medium()
                            .text_color(fg)
                            .child("排序方式"),
                    )
                    .child(
                        div()
                            .id(rid("fp/sort-add"))
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(2.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.filter_add_sort(cx);
                            }))
                            .child(
                                Icon::new(IconName::Plus)
                                    .with_size(px(12.))
                                    .text_color(muted),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .children(rows)
                    .when(t.filter.sorts.is_empty(), |this| {
                        this.child(
                            div()
                                .px(px(8.))
                                .py(px(4.))
                                .text_size(px(11.))
                                .text_color(muted)
                                .child("点击 \"+\" 以添加排序准则"),
                        )
                    }),
            )
            .into_any_element()
    }

    /// 取当前数据页签的选中行（支持多选：当前行 + 连续选区）。
    /// mock 档先做单行，真数据接入后扩成 HashSet<usize>。
    fn selected_data_rows(&self) -> Option<(DbKind, String, String, String, Vec<Vec<String>>)> {
        let t = self.active_data_tab()?;
        let conn = self.conns.iter().find(|c| c.name == t.conn_name)?;
        let order = visible_data_order(t);
        let (row, _col) = t.cell;
        let data_row = order.get(row)?;
        let values = t.data.get(*data_row)?.clone();
        let db = t.db.to_string();
        let schema = mock::schemas(conn.kind).get(t.schema).unwrap_or(&"").to_string();
        let table = mock::objects(conn, t.db, t.cat).get(t.obj).unwrap_or(&"").to_string();
        Some((conn.kind, db, schema, table, vec![values]))
    }

    fn copy_as_insert(&self, cx: &mut Context<Self>) {
        let Some((kind, db, schema, table, rows)) = self.selected_data_rows() else { return };
        let cols: Vec<&str> = mock::MOCK_COLUMNS.iter().map(|c| c.name).collect();
        let sql = copy_as_insert(kind, &db, &schema, &table, &cols, &rows);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(sql));
    }

    fn copy_as_update(&self, cx: &mut Context<Self>) {
        let Some((kind, db, schema, table, rows)) = self.selected_data_rows() else { return };
        let cols: Vec<&str> = mock::MOCK_COLUMNS.iter().map(|c| c.name).collect();
        let sql = copy_as_update(kind, &db, &schema, &table, &cols, &rows);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(sql));
    }

    fn copy_as_select(&self, cx: &mut Context<Self>) {
        let Some((kind, db, schema, table, rows)) = self.selected_data_rows() else { return };
        let cols: Vec<&str> = mock::MOCK_COLUMNS.iter().map(|c| c.name).collect();
        let sql = copy_as_select(kind, &db, &schema, &table, &cols, &rows);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(sql));
    }

    fn copy_as_html(&self, cx: &mut Context<Self>) {
        let Some((_kind, _db, _schema, _table, rows)) = self.selected_data_rows() else { return };
        let cols: Vec<&str> = mock::MOCK_COLUMNS.iter().map(|c| c.name).collect();
        let html = copy_as_html(&cols, &rows);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(html));
    }

    fn remove_connection_named(&mut self, name: &str) {
        match self.conns.iter().position(|c| c.name == name) {
            Some(ix) => self.remove_connection(ix),
            None => self.remap_selection(|c| c),
        }
    }

    /// 连接增删后按 `map` 校正选中索引（各层里记的都是位次）。
    /// `last` 用于把删空后的越界索引夹回来。
    fn remap_selection(&mut self, map: impl Fn(usize) -> usize) {
        let last = self.conns.len().saturating_sub(1);
        let m = |c: usize| map(c).min(last);
        self.sel = match self.sel {
            Act::Conn(c) => Act::Conn(m(c)),
            Act::Db(c, d) => Act::Db(m(c), d),
            Act::Schema(c, d, s) => Act::Schema(m(c), d, s),
            Act::Cat(c, d, s, k) => Act::Cat(m(c), d, s, k),
            Act::Obj(c, d, s, k, o) => Act::Obj(m(c), d, s, k, o),
            Act::Col(c, d, s, k, o, col) => Act::Col(m(c), d, s, k, o, col),
            a @ (Act::Group(_) | Act::Root) => a,
        };
    }

    fn remove_connection(&mut self, ix: usize) {
        if ix >= self.conns.len() {
            return;
        }
        let removed = self.conns.remove(ix);
        self.starred.remove(&removed.name);
        self.conn_colors.remove(&removed.name);
        // 该连接下的页签一并关掉（从后往前，窗格与页签下标同步前移）
        let gone: Vec<usize> = self
            .opened
            .iter()
            .enumerate()
            .filter(|(_, t)| t.conn_name == removed.name)
            .map(|(i, _)| i)
            .collect();
        for ix in gone.into_iter().rev() {
            self.close_tab(ix);
        }
        self.active_tab = self.dock.focused_active();
        self.drop_tree_state(&removed.name);
        if self.conns.is_empty() {
            self.sel = Act::Conn(0);
        } else {
            self.remap_selection(|c| if c > ix { c - 1 } else { c });
        }
        self.save();
    }

    fn duplicate_connection(&mut self, ix: usize, name: String) {
        let Some(conn) = self.conns.get(ix).cloned() else {
            return;
        };
        let copy = mock::Connection::new(
            name,
            conn.kind,
            conn.host,
            conn.port,
            conn.database,
            conn.user,
            conn.password,
            conn.group,
            false,
        );
        let at = ix + 1;
        self.conns.insert(at, copy);
        self.remap_selection(|c| if c >= at { c + 1 } else { c });
        self.save();
    }

    fn duplicate_connection_named(&mut self, old: &str, name: String) {
        // 按名字定位：提示框是独立窗口，开着的时候主窗口仍可增删连接，索引会失效
        if let Some(ix) = self.conns.iter().position(|c| c.name == old) {
            self.duplicate_connection(ix, name);
        }
    }

    fn toggle_star(&mut self, name: &str) {
        if !self.starred.remove(name) {
            self.starred.insert(name.to_string());
        }
        self.save();
    }

    fn set_conn_color(&mut self, name: &str, color: Option<u8>) {
        match color {
            Some(ix) => {
                self.conn_colors.insert(name.to_string(), ix);
            }
            None => {
                self.conn_colors.remove(name);
            }
        }
        self.save();
    }

    /// 新建组后把连接挪进去：Navicat 的「管理组 > 新建组」就是一次搬移 + 建组
    /// 现有连接名（导入向导判重用）。
    fn conn_names(&self) -> Vec<String> {
        self.conns.iter().map(|c| c.name.clone()).collect()
    }

    /// 现有分组名（导入向导标「·新建」用）。
    fn group_names(&self) -> Vec<String> {
        self.groups.clone()
    }

    /// 导出连接用的快照（迁移自 Flutter `NavicatExportDialog` 取 `app.connections`）。
    fn export_snapshot(&self) -> Vec<navicat_ncx::ExportConnection> {
        self.conns
            .iter()
            .map(|c| navicat_ncx::ExportConnection {
                name: c.name.clone(),
                kind: c.kind,
                host: c.host.clone(),
                port: c.port.to_string(),
                username: c.user.clone(),
                password: c.password.clone(),
                database: c.database.clone(),
                auth_method: String::new(),
                group: c.group.clone(),
            })
            .collect()
    }

    /// 导入一批连接（迁移自 Flutter `AppState` 的 `_uniqueNamed` 口径）：
    /// - 重名追加 " (2)"、" (3)"… 直到不重名；
    /// - 文件里出现、本地没有的分组顺带登记（即「分组不存在则重建」）；
    /// - 导入的连接一律是未打开态，和 Navicat 一致。
    fn import_connections(&mut self, items: Vec<ImportItem>) -> ImportOutcome {
        let mut outcome = ImportOutcome {
            imported: 0,
            renamed: 0,
            new_groups: Vec::new(),
        };
        for item in items {
            let group = item.group.trim().to_string();
            if !group.is_empty() && !self.groups.contains(&group) {
                self.groups.push(group.clone());
                outcome.new_groups.push(group.clone());
            }
            let mut name = item.name.trim().to_string();
            if name.is_empty() {
                name = "未命名连接".to_string();
            }
            if self.conns.iter().any(|c| c.name == name) {
                let base = name.clone();
                let mut n = 2;
                while self.conns.iter().any(|c| c.name == format!("{base} ({n})")) {
                    n += 1;
                }
                name = format!("{base} ({n})");
                outcome.renamed += 1;
            }
            self.conns.push(Connection::new(
                name,
                item.kind,
                item.host,
                item.port,
                item.database,
                item.user,
                item.password,
                group,
                false,
            ));
            outcome.imported += 1;
        }
        self.save();
        outcome
    }

    fn add_group_with(&mut self, name: String, conn_name: &str) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        if !self.groups.contains(&name) {
            self.groups.push(name.clone());
        }
        if let Some(conn) = self.conns.iter_mut().find(|c| c.name == conn_name) {
            conn.group = name.clone();
        }
        self.collapsed_groups.remove(&name);
        self.save();
    }

    /// 组节点菜单的「新建组」：允许空组存在，重名自动顺延序号，落成后就地改名。
    fn add_group(&mut self, base: &str) -> usize {
        let mut name = base.to_string();
        let mut n = 2;
        while self.groups.contains(&name) {
            name = format!("{base} {n}");
            n += 1;
        }
        self.groups.push(name.clone());
        self.save();
        self.groups.iter().position(|g| *g == name).unwrap()
    }

    /// 删除组：组内连接回落未分组，连接本体不动。
    fn delete_group(&mut self, gix: usize) {
        let Some(name) = self.groups.get(gix).cloned() else {
            return;
        };
        self.groups.remove(gix);
        for c in &mut self.conns {
            if c.group == name {
                c.group = String::new();
            }
        }
        self.collapsed_groups.remove(&name);
        if matches!(self.sel, Act::Group(_)) {
            self.sel = Act::Root;
        }
        self.save();
    }

    fn move_connection_to_group(&mut self, conn_name: &str, group: &str) {
        if let Some(conn) = self.conns.iter_mut().find(|c| c.name == conn_name) {
            conn.group = group.to_string();
        }
        if !group.is_empty() && !self.groups.iter().any(|g| g == group) {
            self.groups.push(group.to_string());
        }
        self.save();
    }

    fn pane_label(&self) -> &'static str {
        CATEGORIES[self.pane_cat().unwrap_or(0)].0
    }

    /// 状态栏左侧的「N 个 X」：选中节点的直接子节点数量与类别名。
    /// 对象叶子节点没有子项，返回 None（调用方回落到「已选择 1 个项目」）。
    fn sel_child_count(&self) -> (Option<usize>, &'static str) {
        match self.sel {
            Act::Root => (Some(self.conns.len()), "连接"),
            Act::Group(gix) => (
                Some(
                    self.conns
                        .iter()
                        .filter(|c| c.group == self.groups[gix])
                        .count(),
                ),
                "连接",
            ),
            Act::Conn(c) => (Some(mock::databases(self.conns[c].kind).len()), "数据库"),
            Act::Db(c, _) => {
                let kind = self.conns[c].kind;
                // MySQL 系没有模式层，库节点下面直接就是分类
                if mock::has_schemas(kind) {
                    (Some(mock::schemas(kind).len()), "模式")
                } else {
                    (Some(CATEGORIES.len()), "类别")
                }
            }
            Act::Schema(_, _, _) => (Some(CATEGORIES.len()), "类别"),
            Act::Cat(c, d, s, k) => {
                let conn = &self.conns[c];
                let db = mock::databases(conn.kind)[d];
                let _ = s;
                (Some(mock::objects(conn, db, k).len()), CATEGORIES[k].0)
            }
            Act::Obj(..) | Act::Col(..) => (None, ""),
        }
    }

    /// 状态栏中部的活动路径：连接 → 库 →（模式）→ 分类。
    fn breadcrumb_items(&self, fg: Hsla) -> AnyElement {
        let Some(c) = self.sel_conn_ix() else {
            return h_flex().child(div().text_xs().text_color(fg).child("我的连接")).into_any_element();
        };
        let conn = &self.conns[c];
        let mut items = vec![breadcrumb(art::Glyph::Conn(conn.kind), 0x3d7fd0, &conn.name, fg)];
        if let Some(db) = self.sel_db_name() {
            items.push(breadcrumb(art::Glyph::Db, 0x1e9e63, db, fg));
        }
        if let Some(s) = self.sel_schema_name() {
            items.push(breadcrumb(art::Glyph::Schema, 0x6b7785, s, fg));
        }
        if let Some(cat) = self.pane_cat() {
            items.push(breadcrumb(CATEGORIES[cat].1, 0x1e9e63, CATEGORIES[cat].0, fg));
        }
        h_flex().gap(px(14.)).items_center().children(items).into_any_element()
    }

    fn pane_icon(&self) -> art::Glyph {
        CATEGORIES[self.pane_cat().unwrap_or(0)].1
    }

    /// 当前选中路径落在哪个库；没选到库层就用连接的初始库。
    fn anchor_db(&self) -> Option<(usize, usize)> {
        let c = self.sel_conn_ix()?;
        let d = match self.sel.db() {
            Some(d) => d,
            None => mock::databases(self.conns[c].kind)
                .iter()
                .position(|db| *db == self.conns[c].database)
                .unwrap_or(0),
        };
        Some((c, d))
    }

    /// Ribbon 上的分类按钮：Navicat 里它切的是「当前库」的分类，
    /// 选中还停在连接节点时会自动落到初始库并把路径展开出来。
    fn select_category(&mut self, cat: usize) {
        let Some((c, d)) = self.anchor_db() else {
            return;
        };
        let has_schema = mock::has_schemas(self.conns[c].kind);
        let s = self.sel.schema().unwrap_or(0);
        let conn_name = self.conns[c].name.clone();
        let db_name = mock::databases(self.conns[c].kind)[d];
        let db_key = node_key(&conn_name, db_name);
        self.expanded.insert(conn_name);
        self.expanded.insert(db_key.clone());
        self.visited_dbs.insert(db_key.clone());
        let scoped = if has_schema {
            let key = node_key(&db_key, mock::schemas(self.conns[c].kind)[s]);
            self.expanded.insert(key.clone());
            key
        } else {
            db_key
        };
        self.expanded.insert(node_key(&scoped, CATEGORIES[cat].0));
        self.sel = Act::Cat(c, d, if has_schema { s } else { NO_SCHEMA }, cat);
    }
}

impl Render for DaroApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 拖拽结束（松开 / Esc 取消）后落点提示要立刻收掉：拖动会话不在时
        // 任何残留的高亮都不该再画。
        if !cx.has_active_drag() && self.drag_hover.take().is_some() {
            // 只清状态，不额外 notify（本帧本来就在重绘）
        }
        let (bg, fg) = {
            let t = cx.theme();
            (t.background, t.foreground)
        };
        v_flex()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .child(self.title_bar())
            .child(self.menu_bar(cx))
            .child(self.ribbon(cx))
            .child(div().flex_1().min_h_0().child(self.body(cx)))
            .child(self.status_bar(cx))
    }
}

impl DaroApp {
    fn title_bar(&self) -> impl IntoElement {
        let p = self.pal();
        TitleBar::new().h(px(TITLE_H)).child(
            h_flex()
                .h_full()
                .items_center()
                .gap(px(6.))
                .pl(px(4.))
                .child(art::art(art::Glyph::Logo, 15.))
                .child(
                    div()
                        .text_xs()
                        .text_color(to_hsla(p.foreground))
                        .child("daro-rs"),
                ),
        )
    }

    fn menu_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (bg, fg) = (to_hsla(p.chrome), to_hsla(p.foreground));
        let me = cx.entity().clone();
        let dark = self.dark;
        let (list_view, show_info) = (self.list_view, self.show_info);

        h_flex()
            .h(px(MENU_H))
            .pl(px(2.))
            .items_center()
            .bg(bg)
            .children(MENUS.iter().enumerate().map(move |(ix, (name, items))| {
                let items = *items;
                let me = me.clone();
                Button::new(*name)
                    .ghost()
                    .with_size(gpui_kit::component::Size::XSmall)
                    .px(px(16.))
                    .h(px(MENU_H))
                    .rounded(px(2.))
                    .text_color(fg)
                    .label(*name)
                    .when(ix == 3, |this| this.ml(px(2.)))
                    .dropdown_menu(move |menu, window, _| {
                        let mut menu = menu;
                        for (iix, item) in items.iter().enumerate() {
                            let wired: Option<PopupMenuItem> = match (ix, iix) {
                                // 文件 > 新建连接…
                                (0, 0) => {
                                    let me = me.clone();
                                    Some(PopupMenuItem::new(*item).on_click(move |_, _, cx| {
                                        open_new_connection(me.clone(), dark, cx);
                                    }))
                                }
                                // 文件 > 新建查询
                                (0, 1) => {
                                    let me = me.clone();
                                    Some(PopupMenuItem::new(*item).on_click(move |_, window, cx| {
                                        me.update(cx, |a, cx| a.open_query_tab(None, window, cx));
                                    }))
                                }
                                // 文件 > 导出连接…
                                (0, 3) => {
                                    let me = me.clone();
                                    Some(PopupMenuItem::new(*item).on_click(move |_, _, cx| {
                                        navicat_export_dialog::open_navicat_export(
                                            me.clone(),
                                            dark,
                                            cx,
                                        );
                                    }))
                                }
                                // 文件 > 导入连接…
                                (0, 2) => {
                                    let me = me.clone();
                                    Some(PopupMenuItem::new(*item).on_click(move |_, _, cx| {
                                        navicat_import_dialog::open_navicat_import(
                                            me.clone(),
                                            dark,
                                            cx,
                                        );
                                    }))
                                }
                                // 文件 > 退出
                                (0, 4) => Some(
                                    PopupMenuItem::new(*item).on_click(|_, _, cx| cx.quit()),
                                ),
                                // 查看 > 刷新（mock 数据下就是整页重渲染）
                                (2, 0) => {
                                    let me = me.clone();
                                    Some(PopupMenuItem::new(*item).on_click(move |_, _, cx| {
                                        me.update(cx, |_, cx| cx.notify());
                                    }))
                                }
                                // 查看 > 大型图标 / 列表：对象面板双视图
                                (2, 1) => {
                                    let me = me.clone();
                                    Some(
                                        PopupMenuItem::new(*item)
                                            .checked(!list_view)
                                            .on_click(move |_, _, cx| {
                                                me.update(cx, |this, cx| {
                                                    this.list_view = false;
                                                    this.save();
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                }
                                (2, 2) => {
                                    let me = me.clone();
                                    Some(
                                        PopupMenuItem::new(*item)
                                            .checked(list_view)
                                            .on_click(move |_, _, cx| {
                                                me.update(cx, |this, cx| {
                                                    this.list_view = true;
                                                    this.save();
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                }
                                // 查看 > 详细信息：Lite 里是企业版专属（橙色 Ent 角标），置灰占位
                                (2, 3) => Some(todo_item("详细信息", true)),
                                // 查看 > 显示右侧面板
                                (2, 4) => {
                                    let me = me.clone();
                                    Some(
                                        PopupMenuItem::new(*item)
                                            .checked(show_info)
                                            .on_click(move |_, _, cx| {
                                                me.update(cx, |this, cx| {
                                                    this.show_info = !this.show_info;
                                                    this.save();
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                }
                                // 查看 > 切换主题
                                (2, 5) => {
                                    let me = me.clone();
                                    Some(PopupMenuItem::new(*item).on_click(move |_, _, cx| {
                                        me.update(cx, |this, cx| this.toggle_theme(cx));
                                    }))
                                }
                                // 窗口 > 最小化 / 全屏
                                (5, 0) => Some(
                                    PopupMenuItem::new(*item)
                                        .on_click(|_, window, _| window.minimize_window()),
                                ),
                                (5, 1) => Some(
                                    PopupMenuItem::new(*item)
                                        .checked(window.is_fullscreen())
                                        .on_click(|_, window, _| window.toggle_fullscreen()),
                                ),
                                _ => None,
                            };
                            menu = match wired {
                                Some(w) => menu.item(w),
                                // 未实现的动作保持死标签（不留点了没反应的入口）
                                None => menu.label(*item),
                            };
                        }
                        menu
                    })
            }))
    }

    fn ribbon(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (bg, border, fg, muted, hover, active_bg, active_border) = (
            to_hsla(p.chrome),
            to_hsla(p.border),
            to_hsla(p.foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.hover),
            to_hsla(p.selection),
            to_hsla(0x9ccbf2),
        );

        let mut tiles: Vec<AnyElement> = Vec::new();
        for (ix, (icon, label, tint, cat, caret, ent)) in RIBBON.iter().enumerate() {
            if ix == 2 {
                tiles.push(
                    div()
                        .w(px(1.))
                        .h(px(44.))
                        .mx(px(16.))
                        .bg(border)
                        .into_any_element(),
                );
            }
            let active = *cat == Some(self.pane_cat().unwrap_or(0));
            let cat = *cat;
            let new_conn = ix == 0;
            let new_query = ix == 1;
            tiles.push(
                v_flex()
                    .id(*label)
                    .w(px(if *caret { 72. } else { 66. }))
                    .h(px(57.))
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .when(active, move |s| s.bg(active_bg).border_1().border_color(active_border))
                    .when(!active, move |s| s.hover(move |s| s.bg(hover)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if new_conn {
                            open_new_connection(cx.entity().clone(), this.dark, cx);
                        } else if new_query {
                            this.open_query_tab(None, window, cx);
                        } else if let Some(cat) = cat {
                            this.select_category(cat);
                            cx.notify();
                        }
                    }))
                    .test_support()
                    .child(
                        div()
                            .relative()
                            .w(px(29.))
                            .h(px(28.))
                            .child(art::icon(*icon, 28., to_hsla(*tint)))
                            .when(*ent, |s| {
                                s.child(
                                    div()
                                        .absolute()
                                        .bottom_0()
                                        .right(px(-2.))
                                        .child(ent_badge_small()),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(1.))
                            .child(div().text_xs().text_color(fg).child(*label))
                            .when(*caret, |s| {
                                s.child(
                                    Icon::new(IconName::ChevronDown)
                                        .with_size(px(9.))
                                        .text_color(muted),
                                )
                            }),
                    )
                    .into_any_element(),
            );
        }

        let avatar_bg = if self.dark { 0x39424d } else { 0xd8dee6 };
        let avatar_fg = if self.dark { 0x9fb0c0 } else { 0x8797a8 };

        h_flex()
            .h(px(RIBBON_H))
            .items_center()
            .bg(bg)
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .h_full()
                    .items_center()
                    .children(tiles),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .items_center()
                    .gap(px(14.))
                    .pr(px(12.))
                    .child(
                        div()
                            .id("theme-toggle")
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_theme(cx)))
                            .child(
                                Icon::new(if self.dark {
                                    IconName::Sun
                                } else {
                                    IconName::Moon
                                })
                                .with_size(px(15.))
                                .text_color(to_hsla(p.accent)),
                            ),
                    )
                    .child(
                        div()
                            .px(px(4.))
                            .py(px(1.))
                            .rounded(px(3.))
                            .bg(to_hsla(0xef8b2c))
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_bold()
                                    .text_color(to_hsla(0xffffff))
                                    .child("Ent"),
                            ),
                    )
                    .child(
                        div()
                            .size(px(26.))
                            .rounded_full()
                            .bg(to_hsla(avatar_bg))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(IconName::User)
                                    .with_size(px(17.))
                                    .text_color(to_hsla(avatar_fg)),
                            ),
                    ),
            )
    }

    /// 三栏：左树 250、右信息卡 315 固定，中栏吃掉剩下的宽度。
    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let border = to_hsla(p.border_soft);
        h_flex()
            .size_full()
            .items_stretch()
            .child(
                div()
                    .w(px(LEFT_W))
                    .flex_none()
                    .h_full()
                    .border_r_1()
                    .border_color(border)
                    .child(self.tree(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(160.))
                    .h_full()
                    .overflow_hidden()
                    .child(self.workspace(cx)),
            )
            .when(self.show_info, |s| {
                s.child(
                    div()
                        .w(px(RIGHT_W))
                        .flex_none()
                        .h_full()
                        .border_l_1()
                        .border_color(border)
                        .child(self.info()),
                )
            })
            .into_any_element()
    }

    fn tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let bg = to_hsla(p.background);
        let ink = TreeInk {
            fg: to_hsla(p.foreground),
            muted: to_hsla(p.muted_foreground),
            hover: to_hsla(p.hover),
            focus_bg: to_hsla(p.selection_focus),
            hl: if self.dark {
                to_hsla(0x7a6414)
            } else {
                to_hsla(0xffd83d)
            },
            selected: self.sel,
        };

        // 搜索词非空 = 过滤态。Navicat 实测的语义：
        //   命中的节点整棵按普通态渲染（内容不筛，展开键照旧）；
        //   未命中但子树里有命中 = 只当路径，展开到命中处；
        //   分类行（表 / 视图…）是结构行：父级显示时全留，没命中的折叠；
        //   分组在搜索态不出现，命中的连接平铺列出；
        //   一个命中都没有 = 整棵树只剩居中的「找不到项目」。
        let q = if self.query.is_empty() {
            None
        } else {
            Some(self.query.to_lowercase())
        };

        let mut rows: Vec<AnyElement> = Vec::new();
        let mut no_match = false;
        match q.as_deref() {
            // 根名命中：整棵树照常（Navicat 对命中节点不做内容过滤）
            Some(q) if contains_ci("我的连接", q) => rows = self.normal_rows(&ink, Some(q), cx),
            Some(q) => {
                if self.any_match(q) {
                    // 搜索态的根行不带三角（实测即使子路径已展开也没有）
                    rows.push(
                        tree_row(
                            "root",
                            5.,
                            None,
                            art::Glyph::RibbonConn,
                            ink.muted,
                            "我的连接",
                            Act::Root,
                            None,
                            Some(q),
                            &ink,
                            cx,
                        )
                        .into_any_element(),
                    );
                    // 分组不出现在搜索结果里，命中的连接平铺
                    for ix in 0..self.conns.len() {
                        rows.extend(self.filtered_conn(ix, 17.5, q, &ink, cx));
                    }
                } else {
                    no_match = true;
                }
            }
            None => rows = self.normal_rows(&ink, None, cx),
        }

        let me = cx.entity().clone();
        let list = if no_match {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .child(div().text_xs().text_color(ink.muted).child("找不到项目"))
                .into_any_element()
        } else {
            div()
                .id("tree-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .context_menu(node_context_menu(me))
                .child(v_flex().py(px(4.)).children(rows))
                .into_any_element()
        };

        v_flex()
            .size_full()
            .bg(bg)
            .child(list)
            .child(self.search_bar(&ink, cx))
    }

    /// 无搜索词时的整棵树（也是「命中节点整棵渲染」的内容来源）。
    fn normal_rows(
        &self,
        ink: &TreeInk,
        q: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut rows: Vec<AnyElement> = vec![tree_row(
            "root",
            5.,
            Some(if self.collapsed_root {
                IconName::ChevronRight
            } else {
                IconName::ChevronDown
            }),
            art::Glyph::RibbonConn,
            ink.muted,
            "我的连接",
            Act::Root,
            None,
            q,
            ink,
            cx,
        )
        .into_any_element()];

        if !self.collapsed_root {
            for (gix, group) in self.groups.iter().enumerate() {
                let collapsed = self.collapsed_groups.contains(group);
                rows.push(
                    tree_row(
                        ("group", gix),
                        17.5,
                        Some(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        }),
                        art::Glyph::Folder,
                        ink.muted,
                        group,
                        Act::Group(gix),
                        self.rename_editor(Act::Group(gix)),
                        q,
                        ink,
                        cx,
                    )
                    .into_any_element(),
                );
                if collapsed {
                    continue;
                }
                for (ix, conn) in self.conns.iter().enumerate() {
                    if conn.group != *group {
                        continue;
                    }
                    rows.extend(self.conn_rows(ix, 30., q, ink, cx));
                }
            }

            for (ix, conn) in self.conns.iter().enumerate() {
                if !conn.group.is_empty() {
                    continue;
                }
                rows.extend(self.conn_rows(ix, 17.5, q, ink, cx));
            }
        }
        rows
    }

    /// 树底部的搜索条：Navicat 的搜索框，输入即过滤。
    /// 🔍 空态灰 / 有词蓝；右端的 ✕ 用 Input 自带 cleanable（空串时隐藏，同实测）。
    fn search_bar(&self, ink: &TreeInk, cx: &mut Context<Self>) -> impl IntoElement {
        let border = to_hsla(self.pal().border_soft);
        let has_text = !self.query.is_empty();
        h_flex()
            .h(px(30.))
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .border_t_1()
            .border_color(border)
            .child(
                h_flex()
                    .id("search-icon")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(input) = this.search_input.clone() {
                            input.update(cx, |s, cx| s.focus(window, cx));
                        }
                    }))
                    .child(
                        Icon::new(IconName::Search)
                            .with_size(px(13.))
                            .text_color(if has_text {
                                to_hsla(0x2f8fd8)
                            } else {
                                ink.muted
                            }),
                    ),
            )
            .when_some(self.search_input.clone(), |s, input| {
                s.child(
                    div().flex_1().min_w_0().child(
                        Input::new(&input)
                            .h(px(24.))
                            .appearance(false)
                            .bordered(false)
                            .cleanable(true),
                    ),
                )
            })
            .child(Icon::new(IconName::List).with_size(px(13.)).text_color(ink.muted))
            .child(Icon::new(IconName::Star).with_size(px(13.)).text_color(ink.muted))
            .child(Icon::new(IconName::Crosshair).with_size(px(13.)).text_color(ink.muted))
            .child(
                Icon::new(IconName::ChevronDown)
                    .with_size(px(13.))
                    .text_color(ink.muted),
            )
    }

    /// 搜索态的连接行（语义见 tree()）。命中 = 整棵普通态；
    /// 未命中但库里有命中 = 路径展开；未连接的连接只可能命中它自己。
    fn filtered_conn(
        &self,
        ix: usize,
        base: f32,
        q: &str,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let conn = &self.conns[ix];
        if contains_ci(&conn.name, q) {
            let expanded = self.is_expanded(ix);
            let caret_icon = if conn.connected {
                Some(caret(expanded))
            } else {
                None
            };
            let mut rows = vec![self.conn_row_elem(ix, base, caret_icon, Some(q), ink, cx)];
            if expanded {
                rows.extend(self.db_levels(ix, base + 12.5, Some(q), ink, cx));
            }
            return rows;
        }
        // 未连接的连接库列表还没加载
        if !conn.connected {
            return Vec::new();
        }
        let mut parts: Vec<Vec<AnyElement>> = Vec::new();
        for dix in 0..mock::databases(conn.kind).len() {
            let sub = self.filtered_db(ix, dix, base + 12.5, q, ink, cx);
            if !sub.is_empty() {
                parts.push(sub);
            }
        }
        if parts.is_empty() {
            return Vec::new();
        }
        let mut rows =
            vec![self.conn_row_elem(ix, base, Some(IconName::ChevronDown), Some(q), ink, cx)];
        for part in parts {
            rows.extend(part);
        }
        rows
    }

    /// 搜索态的库行。返回空 = 这条库整个隐藏。
    fn filtered_db(
        &self,
        ix: usize,
        dix: usize,
        db_base: f32,
        q: &str,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let conn = &self.conns[ix];
        let db = mock::databases(conn.kind)[dix];
        let db_key = node_key(&conn.name, db);
        if contains_ci(db, q) {
            let open = self.expanded.contains(&db_key);
            let mut rows =
                vec![self.db_row_elem(ix, dix, db_base, Some(caret(open)), Some(q), ink, cx)];
            if open {
                rows.extend(self.db_children(ix, dix, &db_key, db_base, Some(q), ink, cx));
            }
            return rows;
        }
        let has_schema = mock::has_schemas(conn.kind);
        let mut parts: Vec<Vec<AnyElement>> = Vec::new();
        let mut any = false;
        if has_schema {
            for six in 0..mock::schemas(conn.kind).len() {
                let sub =
                    self.filtered_schema(ix, dix, six, &db_key, db_base + 12.5, q, ink, cx);
                any |= !sub.is_empty();
                parts.push(sub);
            }
        } else {
            for cix in 0..CATEGORIES.len() {
                let (shown, rows) = self.filtered_cats(
                    ix, dix, None, cix, &db_key, db_base + 12.5, q, ink, cx,
                );
                any |= shown;
                parts.push(rows);
            }
        }
        if !any {
            return Vec::new();
        }
        let mut rows = vec![self.db_row_elem(
            ix,
            dix,
            db_base,
            Some(IconName::ChevronDown),
            Some(q),
            ink,
            cx,
        )];
        for part in parts {
            rows.extend(part);
        }
        rows
    }

    /// 搜索态的模式行。返回空 = 整个隐藏；显示时全部分类都留着（结构行）。
    #[allow(clippy::too_many_arguments)]
    fn filtered_schema(
        &self,
        ix: usize,
        dix: usize,
        six: usize,
        db_key: &str,
        sch_base: f32,
        q: &str,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let conn = &self.conns[ix];
        let schema = mock::schemas(conn.kind)[six];
        let scoped = node_key(db_key, schema);
        if contains_ci(schema, q) {
            let open = self.expanded.contains(&scoped);
            let mut rows = vec![self.schema_row_elem(
                ix,
                dix,
                six,
                sch_base,
                Some(caret(open)),
                schema,
                Some(q),
                ink,
                cx,
            )];
            if open {
                rows.extend(self.cat_levels(
                    ix,
                    dix,
                    Some(six),
                    &scoped,
                    sch_base + 12.5,
                    Some(q),
                    ink,
                    cx,
                ));
            }
            return rows;
        }
        let mut parts: Vec<Vec<AnyElement>> = Vec::new();
        let mut any = false;
        for cix in 0..CATEGORIES.len() {
            let (shown, rows) =
                self.filtered_cats(ix, dix, Some(six), cix, &scoped, sch_base + 12.5, q, ink, cx);
            any |= shown;
            parts.push(rows);
        }
        if !any {
            return Vec::new();
        }
        let mut rows = vec![self.schema_row_elem(
            ix,
            dix,
            six,
            sch_base,
            Some(IconName::ChevronDown),
            schema,
            Some(q),
            ink,
            cx,
        )];
        for part in parts {
            rows.extend(part);
        }
        rows
    }

    /// 搜索态的分类行。返回 (下面是否有命中的对象, 各行)。
    /// 结构行：父级显示时一定显示，没命中对象就折叠（▶）。
    #[allow(clippy::too_many_arguments)]
    fn filtered_cats(
        &self,
        ix: usize,
        dix: usize,
        six: Option<usize>,
        cix: usize,
        parent_key: &str,
        cat_base: f32,
        q: &str,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> (bool, Vec<AnyElement>) {
        let conn = &self.conns[ix];
        let db = mock::databases(conn.kind)[dix];
        let (label, icon) = CATEGORIES[cix];
        let six_val = six.unwrap_or(NO_SCHEMA);
        let cat_key = node_key(parent_key, label);
        let objs = mock::objects(conn, db, cix);
        let indent = cat_base + 12.5;
        // 分类名自己命中：整棵按普通态渲染
        if contains_ci(label, q) {
            let open = self.expanded.contains(&cat_key);
            let mut rows = vec![self.cat_row_elem(
                ix,
                dix,
                six_val,
                cix,
                cat_base,
                Some(caret(open)),
                label,
                Some(q),
                ink,
                cx,
            )];
            if open {
                for (oix, obj) in objs.iter().enumerate() {
                    let obj_key = node_key(&cat_key, obj);
                    let obj_open = self.expanded.contains(&obj_key);
                    rows.push(self.obj_row_elem(
                        ix, dix, six_val, cix, oix, indent, icon, obj, obj_open, Some(q), ink, cx,
                    ));
                    if obj_open {
                        for (colix, col) in mock::MOCK_COLUMNS.iter().enumerate() {
                            rows.push(self.col_row_elem(
                                ix, dix, six_val, cix, oix, colix, indent + 12.5, col.name,
                                Some(q), ink, cx,
                            ));
                        }
                    }
                }
            }
            return (true, rows);
        }
        // 路径模式：只留「名字命中」或「字段命中」的对象
        let mut parts: Vec<Vec<AnyElement>> = Vec::new();
        for (oix, obj) in objs.iter().enumerate() {
            if contains_ci(obj, q) {
                let obj_key = node_key(&cat_key, obj);
                let open = self.expanded.contains(&obj_key);
                let mut r = vec![self.obj_row_elem(
                    ix, dix, six_val, cix, oix, indent, icon, obj, open, Some(q), ink, cx,
                )];
                if open {
                    for (colix, col) in mock::MOCK_COLUMNS.iter().enumerate() {
                        r.push(self.col_row_elem(
                            ix, dix, six_val, cix, oix, colix, indent + 12.5, col.name, Some(q),
                            ink, cx,
                        ));
                    }
                }
                parts.push(r);
                continue;
            }
            let hits: Vec<usize> = mock::MOCK_COLUMNS
                .iter()
                .enumerate()
                .filter(|(_, col)| contains_ci(col.name, q))
                .map(|(colix, _)| colix)
                .collect();
            if !hits.is_empty() {
                let mut r = vec![self.obj_row_elem(
                    ix, dix, six_val, cix, oix, indent, icon, obj, false, Some(q), ink, cx,
                )];
                for colix in hits {
                    r.push(self.col_row_elem(
                        ix, dix, six_val, cix, oix, colix, indent + 12.5,
                        mock::MOCK_COLUMNS[colix].name, Some(q), ink, cx,
                    ));
                }
                parts.push(r);
            }
        }
        if parts.is_empty() {
            return (false, Vec::new());
        }
        let mut rows = vec![self.cat_row_elem(
            ix,
            dix,
            six_val,
            cix,
            cat_base,
            Some(IconName::ChevronDown),
            label,
            Some(q),
            ink,
            cx,
        )];
        for part in parts {
            rows.extend(part);
        }
        (true, rows)
    }

    /// 一条连接连同它下面所有层的行（普通态）。
    /// Navicat 实测的固定层级：连接 → 数据库 →（模式）→ 分类 → 对象。
    /// `base` 是连接行自己的缩进：组内连接比分组深一级（30），
    /// 未分组连接与分组同级（17.5）——Navicat 里分组只是根下的一个文件夹节点，
    /// 以下各层全部相对 `base` 递进 12.5。
    fn conn_rows(
        &self,
        ix: usize,
        base: f32,
        q: Option<&str>,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let conn = &self.conns[ix];
        let expanded = self.is_expanded(ix);
        // Navicat：未连接的连接没有三角（子树还没加载，没东西可展开）
        let caret_icon = if conn.connected {
            Some(caret(expanded))
        } else {
            None
        };
        let mut rows = vec![self.conn_row_elem(ix, base, caret_icon, q, ink, cx)];
        if expanded {
            rows.extend(self.db_levels(ix, base + 12.5, q, ink, cx));
        }
        rows
    }

    fn conn_row_elem(
        &self,
        ix: usize,
        base: f32,
        caret_icon: Option<IconName>,
        q: Option<&str>,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let conn = &self.conns[ix];
        let name = conn.name.clone();
        let selected = self.sel == Act::Conn(ix);
        let tint = self
            .conn_colors
            .get(&name)
            .map(|c| to_hsla(CONN_COLORS[*c as usize].1))
            .unwrap_or(if selected { ink.fg } else { ink.muted });
        let starred = self.starred.contains(&name);
        tree_row(
            ("conn", ix),
            base,
            caret_icon,
            art::Glyph::Conn(conn.kind),
            tint,
            &name,
            Act::Conn(ix),
            self.rename_editor(Act::Conn(ix)),
            q,
            ink,
            cx,
        )
        .when(starred, |s| {
            s.child(
                Icon::new(IconName::Star)
                    .with_size(px(13.))
                    .text_color(to_hsla(0xe8b23f)),
            )
        })
        .into_any_element()
    }

    /// 某条连接下所有库的行（普通态）。`db_base` 是库行缩进。
    fn db_levels(
        &self,
        ix: usize,
        db_base: f32,
        q: Option<&str>,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let conn = &self.conns[ix];
        let name = conn.name.clone();
        let mut rows = Vec::new();
        for (dix, db) in mock::databases(conn.kind).iter().enumerate() {
            let db_key = node_key(&name, db);
            let db_open = self.expanded.contains(&db_key);
            rows.push(self.db_row_elem(ix, dix, db_base, Some(caret(db_open)), q, ink, cx));
            if !db_open {
                continue;
            }
            rows.extend(self.db_children(ix, dix, &db_key, db_base, q, ink, cx));
        }
        rows
    }

    /// 一条库下的（模式 →）分类 → 对象 → 字段各行（普通态）。
    /// `db_base` 是库行缩进：模式行在其下 12.5，分类行再进 12.5。
    #[allow(clippy::too_many_arguments)]
    fn db_children(
        &self,
        ix: usize,
        dix: usize,
        db_key: &str,
        db_base: f32,
        q: Option<&str>,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let conn = &self.conns[ix];
        let has_schema = mock::has_schemas(conn.kind);
        let mut rows = Vec::new();
        if has_schema {
            for (six, schema) in mock::schemas(conn.kind).iter().enumerate() {
                let scoped = node_key(db_key, schema);
                let schema_open = self.expanded.contains(&scoped);
                rows.push(self.schema_row_elem(
                    ix,
                    dix,
                    six,
                    db_base + 12.5,
                    Some(caret(schema_open)),
                    schema,
                    q,
                    ink,
                    cx,
                ));
                if !schema_open {
                    continue;
                }
                rows.extend(self.cat_levels(
                    ix,
                    dix,
                    Some(six),
                    &scoped,
                    db_base + 25.,
                    q,
                    ink,
                    cx,
                ));
            }
        } else {
            // 没有模式层的引擎用 NO_SCHEMA 占位，只走一轮，缩进不变
            rows.extend(self.cat_levels(ix, dix, None, db_key, db_base + 12.5, q, ink, cx));
        }
        rows
    }

    /// 分类 → 对象 → 字段的行（普通态）。`cat_base` 是分类行缩进。
    #[allow(clippy::too_many_arguments)]
    fn cat_levels(
        &self,
        ix: usize,
        dix: usize,
        six: Option<usize>,
        parent_key: &str,
        cat_base: f32,
        q: Option<&str>,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let conn = &self.conns[ix];
        let db = mock::databases(conn.kind)[dix];
        let six_val = six.unwrap_or(NO_SCHEMA);
        let mut rows = Vec::new();
        for (cix, (label, icon)) in CATEGORIES.iter().enumerate() {
            let cat_key = node_key(parent_key, label);
            let cat_open = self.expanded.contains(&cat_key);
            let objs = mock::objects(conn, db, cix);
            rows.push(self.cat_row_elem(
                ix,
                dix,
                six_val,
                cix,
                cat_base,
                Some(caret(cat_open)),
                label,
                q,
                ink,
                cx,
            ));
            if !cat_open {
                continue;
            }
            let indent = cat_base + 12.5;
            for (oix, obj) in objs.iter().enumerate() {
                // 表 / 视图自己也能展开（下面是字段行），键挂在分类键之下
                let obj_key = node_key(&cat_key, obj);
                let obj_open = self.expanded.contains(&obj_key);
                rows.push(self.obj_row_elem(
                    ix, dix, six_val, cix, oix, indent, *icon, obj, obj_open, q, ink, cx,
                ));
                if !obj_open {
                    continue;
                }
                for (colix, col) in mock::MOCK_COLUMNS.iter().enumerate() {
                    rows.push(self.col_row_elem(
                        ix, dix, six_val, cix, oix, colix, indent + 12.5, col.name, q, ink, cx,
                    ));
                }
            }
        }
        rows
    }

    #[allow(clippy::too_many_arguments)]
    fn db_row_elem(
        &self,
        ix: usize,
        dix: usize,
        db_base: f32,
        caret_icon: Option<IconName>,
        q: Option<&str>,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let conn = &self.conns[ix];
        let db = mock::databases(conn.kind)[dix];
        let opened = self.visited_dbs.contains(&node_key(&conn.name, db));
        tree_row(
            rid(&format!("db/{ix}/{dix}")),
            db_base,
            caret_icon,
            art::Glyph::Db,
            if opened {
                to_hsla(0x3fae5a)
            } else {
                ink.muted
            },
            db,
            Act::Db(ix, dix),
            None,
            q,
            ink,
            cx,
        )
        .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn schema_row_elem(
        &self,
        ix: usize,
        dix: usize,
        six: usize,
        sch_base: f32,
        caret_icon: Option<IconName>,
        schema: &str,
        q: Option<&str>,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        tree_row(
            rid(&format!("sch/{ix}/{dix}/{six}")),
            sch_base,
            caret_icon,
            art::Glyph::Schema,
            ink.muted,
            schema,
            Act::Schema(ix, dix, six),
            None,
            q,
            ink,
            cx,
        )
        .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn cat_row_elem(
        &self,
        ix: usize,
        dix: usize,
        six_val: usize,
        cix: usize,
        cat_base: f32,
        caret_icon: Option<IconName>,
        label: &str,
        q: Option<&str>,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (_, icon) = CATEGORIES[cix];
        tree_row(
            rid(&format!("cat/{ix}/{dix}/{six_val}/{cix}")),
            cat_base,
            caret_icon,
            icon,
            ink.muted,
            label,
            Act::Cat(ix, dix, six_val, cix),
            None,
            q,
            ink,
            cx,
        )
        .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn obj_row_elem(
        &self,
        ix: usize,
        dix: usize,
        six_val: usize,
        cix: usize,
        oix: usize,
        indent: f32,
        icon: art::Glyph,
        obj: &str,
        obj_open: bool,
        q: Option<&str>,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        tree_row(
            rid(&format!("obj/{ix}/{dix}/{six_val}/{cix}/{oix}")),
            indent,
            Some(caret(obj_open)),
            icon,
            ink.muted,
            obj,
            Act::Obj(ix, dix, six_val, cix, oix),
            None,
            q,
            ink,
            cx,
        )
        .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn col_row_elem(
        &self,
        ix: usize,
        dix: usize,
        six_val: usize,
        cix: usize,
        oix: usize,
        colix: usize,
        indent: f32,
        col: &str,
        q: Option<&str>,
        ink: &TreeInk,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        tree_row(
            rid(&format!("col/{ix}/{dix}/{six_val}/{cix}/{oix}/{colix}")),
            indent,
            None,
            art::Glyph::Column,
            ink.muted,
            col,
            Act::Col(ix, dix, six_val, cix, oix, colix),
            None,
            q,
            ink,
            cx,
        )
        .into_any_element()
    }

    /// 树里是否还有任何名字命中（决定根行与「找不到项目」空态）。
    fn any_match(&self, q: &str) -> bool {
        for conn in &self.conns {
            if contains_ci(&conn.name, q) {
                return true;
            }
            if !conn.connected {
                continue;
            }
            for db in mock::databases(conn.kind) {
                if contains_ci(db, q) || CATEGORIES.iter().any(|(label, _)| contains_ci(label, q)) {
                    return true;
                }
                for (cix, _) in CATEGORIES.iter().enumerate() {
                    for obj in mock::objects(conn, db, cix) {
                        if contains_ci(obj, q)
                            || mock::MOCK_COLUMNS.iter().any(|col| contains_ci(col.name, q))
                        {
                            return true;
                        }
                    }
                }
            }
            if mock::has_schemas(conn.kind)
                && mock::schemas(conn.kind).iter().any(|s| contains_ci(s, q))
            {
                return true;
            }
        }
        false
    }

    /// 旧单栏渲染路径：分栏改成 [dock::DaroApp::workspace] 之后保留一份参考实现，
    /// 不再接入渲染树（避免和分栏版重复挂页签点击 / 滚动 id）。
    #[allow(dead_code)]
    fn center(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let bg = to_hsla(p.background);

        let tab = self.active_table();
        let has_tab = tab.is_some();
        // 数据页自带「工具条 / 网格滚动区 / 记录导航条」三段，高度要确定，
        // 外层再套竖向滚动会让 flex_1 拿到无界高度、导航条被顶出窗口
        let self_scrolls = tab.as_ref().is_some_and(|t| !t.query && !t.design);
        let content = if let Some(t) = tab {
            if t.query {
                self.query_page(&t, cx).into_any_element()
            } else if t.design {
                self.designer_page(&t, cx).into_any_element()
            } else {
                self.table_page(&t, cx).into_any_element()
            }
        } else if self.list_view {
            self.object_list(cx).into_any_element()
        } else {
            div()
                .p(px(8.))
                .child(self.object_grid(cx))
                .into_any_element()
        };

        let scroll_id = if has_tab {
            "table-grid-scroll"
        } else if self.list_view {
            "object-list-scroll"
        } else {
            "object-grid-scroll"
        };
        // overflow_y_scrollbar 会把 div 包成 Scrollable，条件书写不了，
        // 只能分两条路各自成型再擦成同一个类型
        let panel: AnyElement = if self_scrolls {
            div().flex_1().min_h_0().child(content).into_any_element()
        } else {
            div()
                .id(scroll_id)
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(content)
                .into_any_element()
        };

        v_flex()
            .size_full()
            .bg(bg)
            .child(self.tab_bar(cx))
            .when(!has_tab, |s| s.child(self.action_bar(cx)))
            .child(panel)
    }

    /// 页签条上的文字：查询 =「查询-N」、设计 =「名字 @库 (连接) (设计)」、
    /// 数据 =「名字 @库 (连接)」，工作集有改动都带前缀「*」。
    /// 新建表未命名时设计页签显示「新建表」，保存命名后换真名。
    fn tab_label(&self, t: &OpenTab) -> String {
        if t.query {
            // Navicat 查询页签：「查询-N」，编辑过未保存前缀「*」
            let star = if t.qstate.as_ref().is_some_and(|q| q.dirty) { "*" } else { "" };
            let seq = t.qstate.as_ref().map(|q| q.seq).unwrap_or(0);
            return format!("{star}查询-{seq}");
        }
        let star = if t.design {
            let dirty = t.dstate.as_ref().is_some_and(design_dirty);
            if dirty { "*" } else { "" }
        } else if t.dirty() {
            // 数据页签工作集有变化（改格 / 增删行未应用）→ 前缀「*」
            "*"
        } else {
            ""
        };
        if t.design {
            match t.dstate.as_ref().and_then(|d| d.new_name.as_deref()) {
                Some("") => return format!("{star}新建表"),
                Some(name) => {
                    return format!("{star}{} (设计)", self.tab_title_named(t, Some(name)));
                }
                None => {}
            }
        }
        format!("{star}{}", self.tab_title(t))
    }

    /// 中部的页签条：0 号「对象」页常驻，双击树节点追加数据页签。
    /// 活动页签铺 accent 淡底（Navicat 实测 #d7cdff，从这里按 accent 派生）。
    /// 旧单栏页签条（分栏版在 [dock::DaroApp::pane_tab_bar]）。
    #[allow(dead_code)]
    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (bg, border, fg, muted, accent) = (
            to_hsla(p.background),
            to_hsla(p.border),
            to_hsla(p.foreground),
            to_hsla(p.subtle_foreground),
            p.accent,
        );
        // Navicat 取样：页签条 #f0f0f0、活动页签纯白并压住下边线、非活动页签同条底
        let strip = to_hsla(p.chrome);
        let active_bg = to_hsla(p.background);
        let tab_line = to_hsla(p.border);

        let object_tab = h_flex()
            .id("tab-objects")
            .h_full()
            .px(px(12.))
            .items_center()
            .cursor_pointer()
            .when(self.active_tab == 0, |s| s.bg(active_bg))
            .when(self.active_tab > 0, move |s| s.hover(move |s| s.bg(to_hsla(p.hover))))
            .on_click(cx.listener(|this, _, _, cx| {
                this.active_tab = 0;
                cx.notify();
            }))
            .child(div().text_xs().text_color(fg).child("对象"));

        let tabs = self.opened.iter().enumerate().map(|(ix, t)| {
            let glyph = if t.query {
                art::Glyph::Query
            } else if t.design {
                art::Glyph::DesignTable
            } else if t.cat == 0 {
                art::Glyph::Table
            } else if t.cat == 2 {
                art::Glyph::MatView
            } else {
                art::Glyph::View
            };
            let active = self.active_tab == ix + 1;
            let title = self.tab_label(t);
            h_flex()
                .id(rid(&format!("tab/{ix}")))
                .h_full()
                .px(px(10.))
                .gap(px(5.))
                .items_center()
                .cursor_pointer()
                .when(active, move |s| {
                    s.bg(active_bg)
                        .border_l_1()
                        .border_r_1()
                        .border_color(tab_line)
                })
                .when(!active, move |s| s.hover(move |s| s.bg(to_hsla(p.hover))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    // × 的点击会冒泡到页签本体，刚关完一页时下标可能越界，夹回来
                    this.active_tab = (ix + 1).min(this.opened.len());
                    cx.notify();
                }))
                .child(art::icon(glyph, 15., to_hsla(accent)))
                .child(div().text_xs().text_color(fg).child(title))
                .child(
                    h_flex()
                        .id(rid(&format!("tab-close/{ix}")))
                        .size(px(14.))
                        .items_center()
                        .justify_center()
                        .rounded(px(2.))
                        .cursor_pointer()
                        .when(!active, |s| s.opacity(0.))  // 非活动页签默认透明
                        .hover(move |s| {
                            s.opacity(1.).bg(to_hsla(p.hover))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.close_tab(ix);
                            this.active_tab = this.active_tab.min(this.opened.len());
                            cx.notify();
                        }))
                        .child(
                            Icon::new(IconName::X)
                                .with_size(px(11.))
                                .text_color(muted),
                        ),
                )
        });

        h_flex()
            .h(px(26.))
            .flex_none()
            .bg(bg)
            .border_b_1()
            .border_color(border)
            .child(object_tab)
            .children(tabs)
    }

    /// 「打开表」的数据页：工具条 + 双行表头的网格 + 底部记录导航条，
    /// 布局与密度照 Navicat Lite 实测（行高 17.5、表头 40、列宽数值）。
    fn table_page(&self, t: &OpenTab, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let (bg, border, fg, muted, subtle, accent, selection) = (
            to_hsla(p.background),
            to_hsla(p.border),
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            p.accent,
            to_hsla(p.selection),
        );
        let hover = to_hsla(p.hover);
        // 数据页配色照 Navicat 取样：表头白底、当前列两行各铺一档浅蓝、
        // 选中行 #cde4f6 一档、当前格深蓝反白、竖向分隔线比外框浅一档。
        // 表头 / 页码框的「纸底」：亮色主题就是纯白，暗色跟内容底同色
        let paper = to_hsla(p.background);
        // 明色主题往白里调（≈#afd4f1），暗色主题往底色调（深蓝格），否则白字压浅蓝看不见
        let head_cur = to_hsla(blend(accent, p.background, 0.74));
        let head_cur_ty = to_hsla(blend(accent, p.background, 0.56));
        // 当前格：暗色下把蓝压深一档，保证白字有对比
        let cur_cell = to_hsla(if self.dark {
            blend(accent, 0x000000, 0.22)
        } else {
            accent
        });
        let line = to_hsla(blend(p.border, 0xffffff, 0.22));
        let row_line = to_hsla(blend(p.border, 0xffffff, 0.58));

        // 工作集（可编辑）行数 = 筛选后的已有数据 + 新增未应用；排序只作用于
        // data（应用面板多键优先，否则表头单键），新增行永远贴在尾部（Navicat 同款）
        // 视图页（cat != 0）：表头只有列名一行（视图没有声明类型），
        // 工具条也没有「导入」（参考工具实测，见 DETAILS.md「数据页 1:1」）
        let is_view = t.cat != 0;
        let order = visible_data_order(t);
        let rows = order.len() + t.inserted.len();
        let dirty = t.dirty();
        let cols = &mock::MOCK_COLUMNS;
        // 列宽按「表头名 / 类型 / 该列最长的值」自适应，再夹到 [56, 260]：
        // 参考工具里 parent_id 这类列不会被截成 parent…，长 id 也放得下。
        let col_w: Vec<f32> = cols
            .iter()
            .enumerate()
            .map(|(cix, col)| {
                let mut w: f32 = est_text_w(col.name, 12.).max(est_text_w(col.ty, 10.5) + 12.);
                for disp in 0..rows {
                    let v = if disp < order.len() {
                        t.data.get(order[disp]).and_then(|r| r.get(cix))
                    } else {
                        t.inserted
                            .get(disp - order.len())
                            .and_then(|r| r.get(cix))
                    };
                    if let Some(v) = v {
                        w = w.max(est_text_w(v, 12.));
                    }
                }
                (w + 20.).clamp(56., 320.)
            })
            .collect();
        let total_w = T_GUTTER_W + col_w.iter().sum::<f32>();
        let pages = rows.div_ceil(PAGE_SIZE).max(1);
        let page = t.cell.0 / PAGE_SIZE;
        let page_start = page * PAGE_SIZE;
        let page_end = (page_start + PAGE_SIZE).min(rows);

        // ── 工具条：图标 + 文字（Navicat 数据页；带 Ent 角标的项按用户要求不放）──
        let tbtn = |id: &'static str, glyph: art::Glyph, label: &'static str, enabled: bool| {
            h_flex()
                .id(rid(&format!("tp/{id}")))
                .h(px(22.))
                .flex_none()
                .px(px(5.))
                .gap(px(4.))
                .items_center()
                .rounded(px(2.))
                .when(enabled, |s| s.cursor_pointer().hover(move |s| s.bg(hover)))
                .opacity(if enabled { 1. } else { 0.85 })
                .child(art::icon(glyph, 15., muted))
                .child(div().text_size(px(12.)).text_color(fg).child(label))
        };
        let tsep = || div().w(px(1.)).h(px(15.)).mx(px(6.)).flex_none().bg(border);

        let toolbar = h_flex()
            .h(px(T_TOOLBAR_H))
            .flex_none()
            .items_center()
            .px(px(6.))
            .gap(px(2.))
            .bg(to_hsla(p.chrome))
            .border_b_1()
            .border_color(border)
            .child(tbtn("txn", art::Glyph::Txn, "开始事务", false))
            .child(tsep())
            .child(tbtn("edit", art::Glyph::CellEdit, "单元格编辑器", false))
            .child(tsep())
            .child(self.filter_bar_btn(t, cx))
            .child(tbtn("cols", art::Glyph::Columns, "列", false))
            .child(tsep())
            .child(tbtn("hist", art::Glyph::History, "历史", false))
            .child(tsep())
            .when(!is_view, |s| {
                s.child(tbtn("import", art::Glyph::ImportWizard, "导入", false))
            })
            .child(
                tbtn("export", art::Glyph::ExportWizard, "导出", true).on_click(cx.listener(
                    |this, _, _, cx| this.open_export_result(cx),
                )),
            )
            .child(div().flex_1());

        // ── 表头：列名行 + 类型行；当前列两行各铺一档浅蓝，底部 2px 强调条 ──
        let header = h_flex()
            .flex_none()
            .child(
                div()
                    .w(px(T_GUTTER_W))
                    .h_full()
                    .flex_none()
                    .bg(paper)
                    .border_b_1()
                    .border_color(line),
            )
            .children(cols.iter().enumerate().map(|(cix, col)| {
                let width = col_w[cix];
                let ty_glyph = match col.numeric {
                    true => "#",
                    false if col.ty == "bool" => "{}",
                    false => "AB",
                };
                let (sorted, asc) = match t.sort {
                    Some((sc, a)) if sc == cix => (true, a),
                    _ => (false, true),
                };
                let cur = t.cell.1 == cix;
                v_flex()
                    .id(rid(&format!("th/{cix}")))
                    .w(px(width))
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(line)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(tab) =
                            this.opened.get_mut(this.active_tab.checked_sub(1).unwrap_or(0))
                        {
                            tab.sort = Some(match tab.sort {
                                Some((sc, asc)) if sc == cix => (cix, !asc),
                                _ => (cix, true),
                            });
                            if let Some(a) = tab.filter.applied.as_mut() {
                                a.sorts.clear();
                                a.order_sql = None;
                            }
                            cx.notify();
                        }
                    }))
                    .child(
                        h_flex()
                            .h(px(T_HEAD_NAME_H))
                            .w_full()
                            .flex_none()
                            .px(px(7.))
                            .gap(px(3.))
                            .items_center()
                            .bg(if cur { head_cur } else { paper })
                            .border_b_1()
                            .border_color(row_line)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .font_medium()
                                    .text_color(fg)
                                    .child(col.name),
                            )
                            .when(sorted, |s| {
                                s.child(
                                    div().flex_none().child(
                                        Icon::new(if asc {
                                            IconName::ArrowUp
                                        } else {
                                            IconName::ArrowDown
                                        })
                                        .with_size(px(11.))
                                        .text_color(to_hsla(accent)),
                                    ),
                                )
                            }),
                    )
                    .when(!is_view, |s| {
                        s.child(
                        h_flex()
                            .h(px(T_HEAD_TYPE_H))
                            .w_full()
                            .flex_none()
                            .px(px(7.))
                            .gap(px(3.))
                            .items_center()
                            .bg(if cur { head_cur_ty } else { paper })
                            .border_b_1()
                            .border_color(line)
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(9.5))
                                    .text_color(if cur { fg } else { subtle })
                                    .child(ty_glyph),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(10.5))
                                    .text_color(if cur { fg } else { subtle })
                                    .child(col.ty),
                            ),
                        )
                    })
                    .when(cur, |s| {
                        s.child(div().w_full().h(px(2.)).flex_none().bg(to_hsla(accent)))
                    })
                    .test_support()
            }));

        let grid = v_flex()
            .w(px(total_w))
            .flex_none()
            .child(header.h(px(if is_view {
                T_HEAD_NAME_H + 2.
            } else {
                T_HEAD_NAME_H + T_HEAD_TYPE_H + 2.
            })))
            .children((page_start..page_end).map(|disp| {
                let is_new = disp >= order.len();
                let selected = t.cell.0 == disp;
                h_flex()
                    .flex_none()
                    .h(px(T_ROW_H))
                    .when(selected, |s| s.bg(selection))
                    .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
                    .border_b_1()
                    .border_color(row_line)
                    // 行首槽：当前行铺表头同款浅蓝 + 左端 3px 强调条（Navicat 没有行号）
                    .child(
                        div()
                            .w(px(T_GUTTER_W))
                            .h_full()
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(selected, move |s| s.bg(head_cur))
                            .child(if selected {
                                div().w(px(3.)).h_full().flex_none().bg(to_hsla(accent)).into_any_element()
                            } else if is_new {
                                // 新增未应用的行：行首一个小「*」，应用后消失
                                div()
                                    .text_size(px(9.))
                                    .text_color(to_hsla(accent))
                                    .child("*")
                                    .into_any_element()
                            } else {
                                div().into_any_element()
                            }),
                    )
                    .children(cols.iter().enumerate().map(|(cix, col)| {
                        let width = col_w[cix];
                        let current = t.cell == (disp, cix);
                        let editing = self
                            .cell_editing
                            .as_ref()
                            .is_some_and(|e| e.at == (disp, cix));
                        let value = if disp < order.len() {
                            t.data
                                .get(order[disp])
                                .and_then(|r| r.get(cix))
                                .cloned()
                                .unwrap_or_default()
                        } else {
                            t.inserted
                                .get(disp - order.len())
                                .and_then(|r| r.get(cix))
                                .cloned()
                                .unwrap_or_default()
                        };
                        div()
                            .id(rid(&format!("cell/{disp}/{cix}")))
                            .w(px(width))
                            .h_full()
                            .flex_none()
                            .px(px(7.))
                            .flex()
                            .items_center()
                            .border_r_1()
                            .border_color(line)
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                                // 点到别的格：先提交还开着的编辑（div 点击不夺焦，
                                // 不显式提交的话旧编辑器会一直占着焦点）
                                if this
                                    .cell_editing
                                    .as_ref()
                                    .is_some_and(|e| e.at != (disp, cix))
                                {
                                    this.commit_cell_edit(cx);
                                }
                                if this.active_tab > 0
                                    && let Some(tab) = this.opened.get_mut(this.active_tab - 1)
                                {
                                    // cell.0 存显示序位，排序态下依然成立
                                    tab.cell = (disp, cix);
                                }
                                // 双击 = 就地编辑。Windows 平台真双击的
                                // click_count 恒 1（同 is_double_click 的缘由），
                                // 所以叠加自己的时间窗判定；测试台发出的双击
                                // click_count=2，两条路任一命中即可。
                                let multi = match ev {
                                    ClickEvent::Mouse(m) => m.down.click_count > 1,
                                    _ => false,
                                };
                                if multi || this.is_cell_double_click((disp, cix)) {
                                    this.start_cell_edit(disp, cix, window, cx);
                                }
                                cx.notify();
                            }))
                            .test_support()
                            // 编辑态：白底 + accent 边框包住整格，输入文字与
                            // 单元格文字同字号同留白（Input 内边距为 0）
                            .when(editing, |s| {
                                let input = self
                                    .cell_editing
                                    .as_ref()
                                    .map(|e| e.input.clone())
                                    .unwrap();
                                s.bg(bg)
                                    .border_1()
                                    .border_color(to_hsla(accent))
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            Input::new(&input)
                                                .h(px(T_ROW_H - 4.))
                                                .appearance(false)
                                                .bordered(false)
                                                .text_size(px(12.)),
                                        ),
                                    )
                            })
                            .when(!editing, |s| {
                                // 当前格铺深蓝反白（Navicat 取样 #0073ca）
                                s.when(current, move |s| s.bg(cur_cell))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .max_w_full()
                                            .truncate()
                                            .text_size(px(12.))
                                            .text_color(if current {
                                                to_hsla(0xffffff)
                                            } else {
                                                fg
                                            })
                                            .child(value),
                                    )
                            })
                    }))
            }));

        // 页码框：纯显示时可点进入就地编辑（回车 / 失焦提交，见 start_page_edit）
        let page_box = if let Some(input) = self.page_editing.as_ref() {
            div()
                .id("nav/pagebox")
                .w(px(32.))
                .h(px(18.))
                .mx(px(2.))
                .flex()
                .items_center()
                .overflow_hidden()
                .border_1()
                .border_color(to_hsla(accent))
                .bg(paper)
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(input)
                            .h(px(16.))
                            .appearance(false)
                            .bordered(false)
                            .text_size(px(11.5)),
                    ),
                )
        } else {
            div()
                .id("nav/pagebox")
                .w(px(32.))
                .h(px(18.))
                .mx(px(2.))
                .flex()
                .items_center()
                .justify_center()
                .border_1()
                .border_color(border)
                .bg(paper)
                .text_size(px(11.5))
                .text_color(fg)
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(format!("{}", page + 1))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.start_page_edit(page, window, cx);
                }))
        };

        let nav = {
            let ghost = |id: &'static str, icon: IconName| {
                tool_icon(rid(&format!("nav/{id}")), icon, subtle, hover, false)
            };
            // 记录编辑组（Navicat 导航条左组）：+ 新增 / − 删除恒可用；
            // ✓ 应用 / × 撤销只在工作集有变化时可用；刷新 = 重新取数（= 撤销）
            let edit_btn = |id: &'static str, icon: IconName, enabled: bool| {
                div()
                    .id(rid(&format!("nav/{id}")))
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(2.))
                    .text_color(if enabled { muted } else { subtle })
                    .when(enabled, |s| s.cursor_pointer().hover(move |s| s.bg(hover)))
                    .child(Icon::new(icon).with_size(px(13.)).text_color(if enabled {
                        muted
                    } else {
                        subtle
                    }))
            };
            let nav_btn = |id: &'static str,
                           icon: IconName,
                           enabled: bool,
                           target: usize,
                           cx: &mut Context<DaroApp>| {
                div()
                    .id(rid(&format!("nav/{id}")))
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(2.))
                    .text_color(if enabled { muted } else { subtle })
                    .when(enabled, |s| s.cursor_pointer().hover(move |s| s.bg(hover)))
                    .when(enabled, |s| {
                        s.on_click(cx.listener(move |this, _, _, cx| {
                            if this.active_tab > 0
                                && let Some(tab) = this.opened.get_mut(this.active_tab - 1)
                            {
                                tab.cell = (target, tab.cell.1);
                                cx.notify();
                            }
                        }))
                    })
                    .child(Icon::new(icon).with_size(px(13.)).text_color(if enabled {
                        muted
                    } else {
                        subtle
                    }))
            };
            h_flex()
                .h(px(T_BAR_H))
                .flex_none()
                .items_center()
                .px(px(6.))
                .gap(px(2.))
                .bg(to_hsla(p.chrome))
                .border_t_1()
                .border_color(border)
                .child(edit_btn("add", IconName::Plus, true).on_click(cx.listener(
                    |this, _, _, cx| this.grid_insert_row(cx),
                )))
                .child(edit_btn("del", IconName::Minus, rows > 0).on_click(cx.listener(
                    |this, _, _, cx| this.grid_delete_row(cx),
                )))
                .child(edit_btn("ok", IconName::Check, dirty).on_click(cx.listener(
                    |this, _, _, cx| this.grid_apply(cx),
                )))
                .child(edit_btn("cancel", IconName::X, dirty).on_click(cx.listener(
                    |this, _, _, cx| this.grid_revert(cx),
                )))
                .child(edit_btn("refresh", IconName::RefreshCw, true).on_click(cx.listener(
                    |this, _, _, cx| this.grid_revert(cx),
                )))
                .child(ghost("stop", IconName::SquareStop))
                .child(div().flex_1())
                .child(nav_btn("first", IconName::ChevronFirst, page > 0, 0, cx))
                .child(nav_btn(
                    "prev",
                    IconName::ChevronLeft,
                    page > 0,
                    page.saturating_sub(1) * PAGE_SIZE,
                    cx,
                ))
                .child(page_box)
                .child(nav_btn(
                    "next",
                    IconName::ChevronRight,
                    page + 1 < pages,
                    ((page + 1) * PAGE_SIZE).min(rows.saturating_sub(1)),
                    cx,
                ))
                .child(nav_btn(
                    "last",
                    IconName::ChevronLast,
                    page + 1 < pages,
                    rows.saturating_sub(1),
                    cx,
                ))
                .child(ghost("opts", IconName::Settings))
                .child(div().w(px(1.)).h(px(14.)).mx(px(4.)).bg(border))
                .child(ghost("find", IconName::ZoomIn))
                .child(div().w(px(1.)).h(px(14.)).mx(px(4.)).bg(border))
                .child(ghost("grid", IconName::Table2))
        };

        v_flex()
            .size_full()
            .bg(bg)
            .child(toolbar)
            .when(t.filter.open, |s| s.child(self.filter_pane(t, cx)))
            .child(
                div()
                    .id("table-grid-inner")
                    .flex_1()
                    .min_h_0()
                    .overflow_scrollbar()
                    
                    .context_menu(grid_context_menu(cx.entity().clone(), self.dark))
                    .child(grid),
            )
            .child(nav)
            .into_any_element()
    }

    /// 「设计表」页签：工具栏 + 子标签条 + 字段网格 + 底部属性面板 + 状态条。
    /// 密度与配色照 Flutter 版 table_designer_page.dart 的实测档
    /// （行 21 / 表头 30 / 行首槽 12 / 列宽 [200,120,60,60,60,60,300]，
    /// 表头白底、网格 1px 线、当前格铺 accent 反白、锚点行淡蓝 + 2px 强调条）。
    fn designer_page(&self, tab: &OpenTab, cx: &mut Context<Self>) -> AnyElement {
        // 根据对象类型分发到不同的设计器：表(0)走完整实现，视图/实体化视图/函数走简化版
        if tab.cat != 0 {
            return self.view_or_function_designer(tab, cx);
        }
        
        let p = self.pal();
        let (bg, border, soft, fg, muted, subtle, accent, selection) = (
            to_hsla(p.background),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.accent),
            to_hsla(p.selection),
        );
        let hover = to_hsla(p.hover);
        let paper = to_hsla(0xffffff);
        // 表头「当前列」底色：和数据页同一口径（亮色往白里调，暗色往底色调）
        let head_cur = to_hsla(blend(p.accent, p.background, 0.74));

        let ds = tab.dstate.as_ref().expect("设计页签必有状态");
        let did = tab.did;
        let conn = self
            .conns
            .iter()
            .find(|c| c.name == tab.conn_name)
            .cloned()
            .unwrap_or_else(|| EMPTY_CONN.clone());
        // SQL 预览里的表名：新建表用保存时输入的名字（未命名时如实留空），
        // 编辑已有表用目录项名
        let table = ds
            .new_name
            .as_ref()
            .map(|n| n.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| {
                mock::objects(&conn, tab.db, tab.cat)
                    .get(tab.obj)
                    .copied()
                    .unwrap_or_default()
            });
        let is_mysql = matches!(conn.kind, mock::DbKind::MySql);
        let is_pg = matches!(conn.kind, mock::DbKind::PostgreSql);
        let dirty = design_dirty(ds);
        let invalid = design_validation(&ds.cols, &ds.indexes, &ds.fks, &ds.uniques, &ds.checks)
            .or_else(|| design_options_validation(&ds.options))
            .or_else(|| design_excludes_gate(&ds.excludes, conn.kind))
            .or_else(|| design_excludes_validation(&ds.excludes))
            .or_else(|| design_rules_gate(&ds.rules, conn.kind))
            .or_else(|| design_rules_validation(&ds.rules))
            .or_else(|| design_triggers_gate(&ds.triggers, conn.kind))
            .or_else(|| design_triggers_validation(&ds.triggers, conn.kind));
        let can_save = dirty && invalid.is_none();
        let sel = Self::design_sel(ds);
        // 主键序号 = 该列在 pk 集合里的次序（1 起），与 PRIMARY KEY 列序一致
        let pk_ord =
            |r: usize| ds.cols.iter().take(r + 1).filter(|c| c.pk).count();

        // 工具栏按钮：图标 + 文字（参考工具是「保存 | 添加字段 删除字段 | 主键 ▾」）
        let tbtn = |id: &'static str,
                    icon: AnyElement,
                    label: &'static str,
                    caret: bool,
                    enabled: bool|
         -> Stateful<Div> {
            h_flex()
                .id(rid(id))
                .h(px(22.))
                .flex_none()
                .px(px(5.))
                .gap(px(4.))
                .items_center()
                .rounded(px(2.))
                .when(enabled, |s| s.cursor_pointer().hover(move |s| s.bg(hover)))
                .opacity(if enabled { 1. } else { 0.85 })
                .child(icon)
                .child(div().text_size(px(12.)).text_color(fg).child(label))
                .when(caret, move |s| {
                    s.child(
                        Icon::new(IconName::ChevronDown)
                            .with_size(px(9.))
                            .text_color(subtle),
                    )
                })
        };
        let gicon = |glyph: art::Glyph| -> AnyElement {
            art::icon(glyph, 15., to_hsla(p.muted_foreground))
        };
        let licon = |icon: IconName, tint: u32| -> AnyElement {
            Icon::new(icon)
                .with_size(px(14.))
                .text_color(to_hsla(tint))
                .into_any_element()
        };
        let tsep = || div().w(px(1.)).h(px(15.)).mx(px(6.)).flex_none().bg(border);

        // 工具栏右侧：校验错误（没有就留白）。底部不再放描述性信息，
        // 「已修改」由页签标题前的 * 表示（见 tab_label）。
        let toolbar_tail = || {
            h_flex()
                .flex_1()
                .min_w_0()
                .justify_end()
                .pr(px(10.))
                .when_some(invalid.as_ref(), |s, err| {
                    s.child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(to_hsla(0xDC2626))
                            .child(err.clone()),
                    )
                })
        };

        // 工具栏按子页分流：参考工具「索引」页是「保存 | 添加索引 删除索引」，
        // 「外键」页是「保存 | 添加外键 删除外键」，字段页是
        // 「保存 | 添加字段 删除字段 | 主键 ▾」。AI 项按用户要求不放。
        let toolbar = if ds.sub == 2 {
            h_flex()
                .h(px(D_TOOLBAR_H))
                .flex_none()
                .items_center()
                .px(px(4.))
                .bg(to_hsla(p.chrome))
                .border_b_1()
                .border_color(soft)
                .child(
                    tbtn("dsave", gicon(art::Glyph::SaveFile), "保存", false, can_save)
                        .when(can_save, |s| {
                            s.on_click(cx.listener(move |this, _, _, cx| this.design_save(did, cx)))
                        })
                        .test_support(),
                )
                .child(tsep())
                .child(
                    tbtn(
                        "dfk-add",
                        gicon(art::Glyph::NewTable),
                        "添加外键",
                        false,
                        true,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.design_add_fk(did, window, cx)
                    }))
                    .test_support(),
                )
                .child(
                    tbtn(
                        "dfk-del",
                        gicon(art::Glyph::DropTable),
                        "删除外键",
                        false,
                        !ds.fks.is_empty(),
                    )
                    .when(!ds.fks.is_empty(), |s| {
                        s.on_click(cx.listener(move |this, _, window, cx| {
                            let row = this
                                .opened
                                .iter()
                                .find(|t| t.did == did)
                                .and_then(|t| t.dstate.as_ref())
                                .map(Self::design_fk_sel)
                                .unwrap_or(0);
                            this.design_remove_fk(did, row, window, cx)
                        }))
                    })
                    .test_support(),
                )
                .child(toolbar_tail())
        } else if ds.sub == 3 {
            h_flex()
                .h(px(D_TOOLBAR_H))
                .flex_none()
                .items_center()
                .px(px(4.))
                .bg(to_hsla(p.chrome))
                .border_b_1()
                .border_color(soft)
                .child(
                    tbtn("dsave", gicon(art::Glyph::SaveFile), "保存", false, can_save)
                        .when(can_save, |s| {
                            s.on_click(cx.listener(move |this, _, _, cx| this.design_save(did, cx)))
                        })
                        .test_support(),
                )
                .child(tsep())
                .child(
                    tbtn(
                        "dun-add",
                        gicon(art::Glyph::NewTable),
                        "添加唯一键",
                        false,
                        true,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.design_add_unique(did, window, cx)
                    }))
                    .test_support(),
                )
                .child(
                    tbtn(
                        "dun-del",
                        gicon(art::Glyph::DropTable),
                        "删除唯一键",
                        false,
                        !ds.uniques.is_empty(),
                    )
                    .when(!ds.uniques.is_empty(), |s| {
                        s.on_click(cx.listener(move |this, _, window, cx| {
                            let row = this
                                .opened
                                .iter()
                                .find(|t| t.did == did)
                                .and_then(|t| t.dstate.as_ref())
                                .map(Self::design_unique_sel)
                                .unwrap_or(0);
                            this.design_remove_unique(did, row, window, cx)
                        }))
                    })
                    .test_support(),
                )
                .child(div().flex_1())
        } else if ds.sub == 4 {
            h_flex()
                .h(px(D_TOOLBAR_H))
                .flex_none()
                .items_center()
                .px(px(4.))
                .bg(to_hsla(p.chrome))
                .border_b_1()
                .border_color(soft)
                .child(
                    tbtn("dsave", gicon(art::Glyph::SaveFile), "保存", false, can_save)
                        .when(can_save, |s| {
                            s.on_click(cx.listener(move |this, _, _, cx| this.design_save(did, cx)))
                        })
                        .test_support(),
                )
                .child(tsep())
                .child(
                    tbtn("dck-add", gicon(art::Glyph::NewTable), "添加检查", false, true)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.design_add_check(did, window, cx)
                        }))
                        .test_support(),
                )
                .child(
                    tbtn(
                        "dck-del",
                        gicon(art::Glyph::DropTable),
                        "删除检查",
                        false,
                        !ds.checks.is_empty(),
                    )
                    .when(!ds.checks.is_empty(), |s| {
                        s.on_click(cx.listener(move |this, _, window, cx| {
                            let row = this
                                .opened
                                .iter()
                                .find(|t| t.did == did)
                                .and_then(|t| t.dstate.as_ref())
                                .map(Self::design_check_sel)
                                .unwrap_or(0);
                            this.design_remove_check(did, row, window, cx)
                        }))
                    })
                    .test_support(),
                )
                .child(div().flex_1())
        } else if ds.sub == 5 {
            h_flex()
                .h(px(D_TOOLBAR_H))
                .flex_none()
                .items_center()
                .px(px(4.))
                .bg(to_hsla(p.chrome))
                .border_b_1()
                .border_color(soft)
                .child(
                    tbtn("dsave", gicon(art::Glyph::SaveFile), "保存", false, can_save)
                        .when(can_save, |s| {
                            s.on_click(cx.listener(move |this, _, _, cx| this.design_save(did, cx)))
                        })
                        .test_support(),
                )
                .child(tsep())
                .child(
                    tbtn("dex-add", gicon(art::Glyph::NewTable), "添加排除", false, true)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.design_add_exclude(did, window, cx)
                        }))
                        .test_support(),
                )
                .child(
                    tbtn(
                        "dex-del",
                        gicon(art::Glyph::DropTable),
                        "删除排除",
                        false,
                        !ds.excludes.is_empty(),
                    )
                    .when(!ds.excludes.is_empty(), |s| {
                        s.on_click(cx.listener(move |this, _, window, cx| {
                            let row = this
                                .opened
                                .iter()
                                .find(|t| t.did == did)
                                .and_then(|t| t.dstate.as_ref())
                                .map(Self::design_exclude_sel)
                                .unwrap_or(0);
                            this.design_remove_exclude(did, row, window, cx)
                        }))
                    })
                    .test_support(),
                )
                .child(div().flex_1())
        } else if ds.sub == 6 {
            h_flex()
                .h(px(D_TOOLBAR_H))
                .flex_none()
                .items_center()
                .px(px(4.))
                .bg(to_hsla(p.chrome))
                .border_b_1()
                .border_color(soft)
                .child(
                    tbtn("dsave", gicon(art::Glyph::SaveFile), "保存", false, can_save)
                        .when(can_save, |s| {
                            s.on_click(cx.listener(move |this, _, _, cx| this.design_save(did, cx)))
                        })
                        .test_support(),
                )
                .child(tsep())
                .child(
                    tbtn("drule-add", gicon(art::Glyph::NewTable), "添加规则", false, true)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.design_add_rule(did, window, cx)
                        }))
                        .test_support(),
                )
                .child(
                    tbtn(
                        "drule-del",
                        gicon(art::Glyph::DropTable),
                        "删除规则",
                        false,
                        !ds.rules.is_empty(),
                    )
                    .when(!ds.rules.is_empty(), |s| {
                        s.on_click(cx.listener(move |this, _, window, cx| {
                            let row = this
                                .opened
                                .iter()
                                .find(|t| t.did == did)
                                .and_then(|t| t.dstate.as_ref())
                                .map(Self::design_rule_sel)
                                .unwrap_or(0);
                            this.design_remove_rule(did, row, window, cx)
                        }))
                    })
                    .test_support(),
                )
                .child(div().flex_1())
        } else if ds.sub == 8 {
            h_flex()
                .h(px(D_TOOLBAR_H))
                .flex_none()
                .items_center()
                .px(px(4.))
                .bg(to_hsla(p.chrome))
                .border_b_1()
                .border_color(soft)
                .child(
                    tbtn("dsave", gicon(art::Glyph::SaveFile), "保存", false, can_save)
                        .when(can_save, |s| {
                            s.on_click(cx.listener(move |this, _, _, cx| this.design_save(did, cx)))
                        })
                        .test_support(),
                )
                .child(div().flex_1())
        } else if ds.sub == 1 {
            h_flex()
                .h(px(D_TOOLBAR_H))
                .flex_none()
                .items_center()
                .px(px(4.))
                .bg(to_hsla(p.chrome))
                .border_b_1()
                .border_color(soft)
                .child(
                    tbtn("dsave", gicon(art::Glyph::SaveFile), "保存", false, can_save)
                        .when(can_save, |s| {
                            s.on_click(cx.listener(move |this, _, _, cx| this.design_save(did, cx)))
                        })
                        .test_support(),
                )
                .child(tsep())
                .child(
                    tbtn(
                        "didx-add",
                        licon(IconName::Plus, D_ICON_ADD),
                        "添加索引",
                        false,
                        true,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.design_add_index(did, window, cx)
                    }))
                    .test_support(),
                )
                .child(
                    tbtn(
                        "didx-del",
                        licon(IconName::Minus, D_ICON_DANGER),
                        "删除索引",
                        false,
                        !ds.indexes.is_empty(),
                    )
                    .when(!ds.indexes.is_empty(), |s| {
                        s.on_click(cx.listener(move |this, _, window, cx| {
                            let row = this
                                .opened
                                .iter()
                                .find(|t| t.did == did)
                                .and_then(|t| t.dstate.as_ref())
                                .map(Self::design_index_sel)
                                .unwrap_or(0);
                            this.design_remove_index(did, row, window, cx)
                        }))
                    })
                    .test_support(),
                )
                .child(toolbar_tail())
        } else {
            h_flex()
                .h(px(D_TOOLBAR_H))
                .flex_none()
                .items_center()
                .px(px(4.))
                .bg(to_hsla(p.chrome))
                .border_b_1()
                .border_color(soft)
                .child(
                    tbtn("dsave", gicon(art::Glyph::SaveFile), "保存", false, can_save)
                        .when(can_save, |s| {
                            s.on_click(cx.listener(move |this, _, _, cx| this.design_save(did, cx)))
                        })
                        .test_support(),
                )
                .child(tsep())
                .child(
                    tbtn("dadd", gicon(art::Glyph::NewTable), "添加字段", false, true)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.design_add_col(did, window, cx)
                        }))
                        .test_support(),
                )
                .child(
                    tbtn(
                        "ddel",
                        gicon(art::Glyph::DropTable),
                        "删除字段",
                        false,
                        ds.cols.len() > 1,
                    )
                    .when(ds.cols.len() > 1, |s| {
                        s.on_click(
                            cx.listener(move |this, _, _, cx| this.design_del_col(did, cx)),
                        )
                    })
                    .test_support(),
                )
                .child(
                    tbtn(
                        "dkey",
                        gicon(art::Glyph::KeyGold),
                        "主键",
                        true,
                        !ds.cols.is_empty(),
                    )
                    .when(!ds.cols.is_empty(), |s| {
                        s.on_click(
                            cx.listener(move |this, _, _, cx| this.design_toggle_pk(did, cx)),
                        )
                    })
                    .test_support(),
                )
                .child(toolbar_tail())
        };

        // 子标签条：活动页签铺内容底，非活动可 hover
        let tabbar = h_flex()
            .h(px(D_TABBAR_H))
            .flex_none()
            .items_stretch()
            .bg(to_hsla(p.chrome))
            .border_b_1()
            .border_color(border)
            .children(D_SUBTABS.iter().enumerate().map(|(i, label)| {
                let active = ds.sub == i;
                h_flex()
                    .id(rid(&format!("dsub/{i}")))
                    .px(px(11.5))
                    .items_center()
                    .when(active, |s| s.bg(bg))
                    .when(!active, |s| {
                        s.cursor_pointer().hover(move |s| s.bg(hover))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(t) = this.opened.iter_mut().find(|t| t.did == did)
                            && let Some(d) = t.dstate.as_mut()
                        {
                            d.sub = i;
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(if active { fg } else { muted })
                            .child(*label),
                    )
                    .test_support()
            }));

        // ── 字段网格 ──────────────────────────────────────────
        // 列宽：默认 D_FIELD_WIDTHS，拖过表头分割线后按 (子页 0, 列) 取用户宽
        let fw = |ix: usize| -> f32 {
            ds.widths
                .get(&(0usize, ix))
                .copied()
                .unwrap_or(D_FIELD_WIDTHS[ix])
        };
        let total_w = D_GUTTER_W + (0..7).map(fw).sum::<f32>();
        let gutter = move |h: f32, current: bool| {
            div()
                .w(px(D_GUTTER_W))
                .h(px(h))
                .flex_none()
                .relative()
                .border_r_1()
                .border_b_1()
                .border_color(border)
                .when(current, |s| {
                    s.bg(selection).child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(D_ACCENT_BAR_W))
                            .bg(accent),
                    )
                })
        };
        let headers =
            ["名称", "类型", "长度", "小数点", "不是 null", "键", "注释"];
        let header_row = h_flex()
            .h(px(D_HEADER_H))
            .flex_none()
            .child(gutter(D_HEADER_H, false))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some(did) = this.did_of_active_design() {
                    this.design_resize_move(did, ev.position.x.as_f32(), cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(did) = this.did_of_active_design() {
                        this.design_resize_end(did, cx);
                    }
                }),
            )
            .children(headers.iter().enumerate().map(|(i, h)| {
                let cur_w = fw(i);
                div()
                    .w(px(cur_w))
                    .h(px(D_HEADER_H))
                    .flex_none()
                    .relative()
                    .border_r_1()
                    .border_b_1()
                    .border_color(border)
                    .when(i == ds.cur, |s| {
                        s.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .bg(head_cur)
                                .border_b_2()
                                .border_color(accent),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(5.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(div().text_size(px(12.)).text_color(fg).child(*h)),
                    )
                    .child(
                        div()
                            .id(rid(&format!("field-resize/{i}")))
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(5.))
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    this.design_resize_begin(
                                        did,
                                        0,
                                        i,
                                        ev.position.x.as_f32(),
                                        cur_w,
                                        cx,
                                    );
                                }),
                            ),
                    )
            }));

        // 文本格 / 勾选格 / 键格的通用包装：定宽 + 1px 网格线 + 点击选中
        let anchor_row = sel;
        let cell_box = move |r: usize, col: usize| {
            div()
                .id(rid(&format!("dcell/{r}/{col}")))
                .w(px(fw(col)))
                .h(px(D_ROW_EXTENT))
                .flex_none()
                .relative()
                .when(r == anchor_row, |s| s.bg(selection))
                .border_r_1()
                .border_b_1()
                .border_color(soft)
        };
        let cell_pick = |this: &mut Self,
                         did: u64,
                         r: usize,
                         col: usize,
                         _: &ClickEvent,
                         _: &mut Window,
                         cx: &mut Context<Self>| {
            if let Some(t) = this.opened.iter_mut().find(|t| t.did == did)
                && let Some(d) = t.dstate.as_mut()
            {
                d.sel = r;
                d.cur = col;
                cx.notify();
            }
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        for (r, c) in ds.cols.iter().enumerate() {
            let anchor = r == sel;
            let edits = ds.edits.get(r);
            let text_cell = |col: usize,
                             f: usize,
                             val: &str,
                             placeholder: &'static str|
             -> AnyElement {
                let current = anchor && col == ds.cur;
                let editor = edits.and_then(|e| e[f].clone());
                cell_box(r, col)
                    .when(current, |s| s.bg(accent))
                    .on_click(cx.listener(move |this, ev, window, cx| {
                        cell_pick(this, did, r, col, ev, window, cx)
                    }))
                    .child(match editor {
                        Some(e) => div()
                            .absolute()
                            .left(px(5.))
                            .right(px(2.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&e)
                                        .id(rid(&format!("dinput/{r}/{f}")))
                                        .h(px(18.))
                                        .appearance(false)
                                        .bordered(false)
                                        .text_size(px(12.)),
                                ),
                            )
                            .into_any_element(),
                        None => {
                            let empty = val.is_empty();
                            div()
                                .absolute()
                                .left(px(5.))
                                .top_0()
                                .bottom_0()
                                .flex()
                                .items_center()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(if current {
                                            paper
                                        } else if empty {
                                            subtle
                                        } else {
                                            fg
                                        })
                                        .child(if empty {
                                            placeholder.to_string()
                                        } else {
                                            val.to_string()
                                        }),
                                )
                                .into_any_element()
                        }
                    })
                    .test_support()
                    .into_any_element()
            };
            let check_cell = {
                let checked = c.not_null;
                div()
                    .id(rid(&format!("dcell/{r}/4")))
                    .w(px(D_FIELD_WIDTHS[4]))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_set_not_null(did, r, !checked, cx)
                    }))
                    .child(
                        // 勾选框恒 13：方框是命中目标，不随行高缩（同 Flutter 版）
                        div()
                            .size(px(13.))
                            .rounded(px(2.))
                            .border_1()
                            .border_color(if checked { accent } else { subtle })
                            .bg(bg)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(checked, |s| {
                                s.child(
                                    Icon::new(IconName::Check)
                                        .with_size(px(10.))
                                        .text_color(accent),
                                )
                            }),
                    )
                    .test_support()
                    .into_any_element()
            };
            let key_cell = {
                let pk_on = c.pk;
                let ord = pk_ord(r);
                div()
                    .id(rid(&format!("dcell/{r}/5")))
                    .w(px(D_FIELD_WIDTHS[5]))
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_r_1()
                    .border_b_1()
                    .border_color(soft)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // 点键格 = 选中该行 + 切换主键（零延迟，同 Flutter 版）
                        if let Some(t) = this.opened.iter_mut().find(|t| t.did == did)
                            && let Some(d) = t.dstate.as_mut()
                        {
                            d.sel = r;
                        }
                        this.design_toggle_pk(did, cx);
                    }))
                    .when(pk_on, |s| {
                        s.child(
                            h_flex()
                                .gap(px(2.))
                                .items_center()
                                .child(
                                    Icon::new(IconName::Key)
                                        .with_size(px(14.))
                                        .text_color(to_hsla(D_ICON_KEY)),
                                )
                                .child(
                                    div().text_size(px(12.)).text_color(fg).child(
                                        format!("{ord}"),
                                    ),
                                ),
                        )
                    })
                    .test_support()
                    .into_any_element()
            };
            rows.push(
                h_flex()
                    .h(px(D_ROW_EXTENT))
                    .flex_none()
                    .child(
                        div()
                            .id(rid(&format!("dgutter/{r}")))
                            .when(anchor, |s| {
                                // 行首槽的点击命中交给外层包装：这里直接铺样式
                                s.bg(selection).child(
                                    div().absolute().right_0().top_0().bottom_0()
                                        .w(px(D_ACCENT_BAR_W)).bg(accent),
                                )
                            })
                            .w(px(D_GUTTER_W))
                            .h(px(D_ROW_EXTENT))
                            .flex_none()
                            .relative()
                            .border_r_1()
                            .border_b_1()
                            .border_color(border)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(t) =
                                    this.opened.iter_mut().find(|t| t.did == did)
                                    && let Some(d) = t.dstate.as_mut()
                                {
                                    d.sel = r;
                                    cx.notify();
                                }
                            }))
                            .test_support(),
                    )
                    .child(text_cell(0, 0, &c.name, "列名"))
                    .child(text_cell(1, 6, &c.ty, "类型"))
                    .child(text_cell(2, 1, &design_len_display(&c.ty, &c.len), ""))
                    .child(text_cell(3, 2, if c.dec.is_empty() { "0" } else { &c.dec }, ""))
                    .child(check_cell)
                    .child(key_cell)
                    .child(text_cell(6, 3, &c.comment, ""))
                    .into_any_element(),
            );
        }

        let grid = v_flex()
            .child(header_row)
            .children(rows)
            .context_menu(designer_context_menu(cx.entity().clone(), did));

        // 属性面板：标签列 190（面板左起 11）+ 控件列自 201 起，
        // 与网格第一列(200)对齐 —— 同 Flutter 版 _propField 的口径
        let flat_disabled = |w: f32, v: &str| -> AnyElement {
            div()
                .w(px(w))
                .h(px(20.))
                .flex_none()
                .px(px(5.))
                .flex()
                .items_center()
                .rounded(px(2.))
                .border_1()
                .border_color(soft)
                .bg(bg)
                .overflow_hidden()
                .child(div().text_size(px(12.)).text_color(subtle).child(v.to_string()))
                .into_any_element()
        };
        let prop_row = |label: &str, w: f32, control: AnyElement| -> AnyElement {
            h_flex()
                .h(px(D_PROP_ROW_H))
                .flex_none()
                .items_center()
                .child(
                    div()
                        .w(px(D_PROP_LABEL_W))
                        .flex_none()
                        .overflow_hidden()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(fg)
                                .child(format!("{label}:")),
                        ),
                )
                .child(div().w(px(w)).flex_none().flex().child(control))
                .into_any_element()
        };
        let props: AnyElement = if ds.cols.is_empty() {
            div().into_any_element()
        } else {
            let c = &ds.cols[sel];
            let editor_of = |f: usize| {
                ds.edits.get(sel).and_then(|e| e[f].clone()).map(|e| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .child(
                            Input::new(&e)
                                .id(rid(&format!("dprop-input/{f}")))
                                .h(px(20.))
                                .appearance(false)
                                .bordered(false)
                                .text_size(px(12.)),
                        )
                        .into_any_element()
                })
            };
            let mut prows: Vec<AnyElement> = Vec::new();
            // 默认
            prows.push(prop_row(
                "默认",
                D_PROP_FIELD_W,
                editor_of(4).unwrap_or_else(|| flat_disabled(D_PROP_FIELD_W, &c.default)),
            ));
            // 排序规则：两个下拉并排 200 + 204（右槽 PG 无对应语法，占位）
            prows.push(prop_row(
                "排序规则",
                404.,
                h_flex()
                    .child(flat_disabled(200., &c.collation))
                    .child(flat_disabled(204., ""))
                    .into_any_element(),
            ));
            // 维度（数组维数，仅 PG 参与 DDL；mock 档只读展示）
            prows.push(prop_row(
                "维度",
                D_PROP_FIELD_W,
                flat_disabled(D_PROP_FIELD_W, ""),
            ));
            if is_mysql {
                let on = c.auto_inc;
                prows.push(prop_row(
                    "自增",
                    D_PROP_FIELD_W,
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(
                            div()
                                .id(rid(&format!("dprop/auto/{sel}")))
                                .size(px(13.))
                                .rounded(px(2.))
                                .border_1()
                                .border_color(if on { accent } else { subtle })
                                .bg(bg)
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .when(on, |s| {
                                    s.child(
                                        Icon::new(IconName::Check)
                                            .with_size(px(10.))
                                            .text_color(accent),
                                    )
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(t) =
                                        this.opened.iter_mut().find(|t| t.did == did)
                                        && let Some(d) = t.dstate.as_mut()
                                        && let Some(col) = d.cols.get_mut(sel)
                                    {
                                        col.auto_inc = !on;
                                        d.note = None;
                                        cx.notify();
                                    }
                                }))
                                .test_support(),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(muted)
                                .child("AUTO_INCREMENT"),
                        )
                        .into_any_element(),
                ));
            } else {
                prows.push(prop_row(
                    if is_pg { "虚拟类型" } else { "标识" },
                    D_PROP_FIELD_W,
                    editor_of(5).unwrap_or_else(|| flat_disabled(D_PROP_FIELD_W, "")),
                ));
                for label in ["递增", "开始值"] {
                    prows.push(prop_row(
                        label,
                        D_PROP_FIELD_W,
                        flat_disabled(D_PROP_FIELD_W, ""),
                    ));
                }
                if is_pg {
                    for label in ["最小", "最大", "缓存"] {
                        prows.push(prop_row(
                            label,
                            D_PROP_FIELD_W,
                            flat_disabled(D_PROP_FIELD_W, ""),
                        ));
                    }
                    prows.push(prop_row(
                        "循环",
                        D_PROP_FIELD_W,
                        div()
                            .size(px(13.))
                            .rounded(px(2.))
                            .border_1()
                            .border_color(subtle)
                            .bg(bg)
                            .into_any_element(),
                    ));
                }
            }
            let hint = if is_mysql {
                Some("MySQL / MariaDB 自增：生成 AUTO_INCREMENT。")
            } else if is_pg {
                Some("PostgreSQL 虚拟类型：填 ALWAYS / BY DEFAULT 启用 IDENTITY。")
            } else {
                Some("SQL Server 标识列：生成 IDENTITY(开始值, 递增)。")
            };
            div()
                .id("dprops")
                .test_support()
                .child(
                    v_flex()
                        .flex_none()
                        .max_h(px(232.))
                        .overflow_y_scrollbar()
                        .pl(px(D_PROP_PAD_L))
                        .pr(px(D_PROP_PAD_L))
                        .pt(px(10.))
                        .pb(px(8.))
                        .bg(bg)
                        .children(prows)
                        .when_some(hint, |s, h| {
                            s.child(
                                div()
                                    .pt(px(4.))
                                    .text_size(px(12.))
                                    .text_color(subtle)
                                    .child(h),
                            )
                        }),
                )
                .into_any_element()
        };

        let fields_tab = v_flex()
            .size_full()
            .child(
                div()
                    .id("dgrid-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_scrollbar()
                    
                    .child(div().w(px(total_w)).child(grid)),
            )
            .child(div().h(px(D_PROP_SPLITTER_H)).flex_none().bg(to_hsla(p.chrome)))
            .child(props)
            .into_any_element();

        // ── SQL 预览页（参考工具 cameras 屏口径）────────────────
        // 「更改」子页 = 增量脚本（ALTER … ADD/DROP + CREATE/DROP INDEX），
        //   末尾固定带触发器 / 规则的生成段，右上角复制按钮；
        // 「DDL」子页 = 「之前：/之后：」并排行号对比，改动行整行铺绿底。
        let dark = self.dark;
        let schema_name = mock::schemas(conn.kind).get(tab.schema).copied().unwrap_or("");
        let owner = if matches!(conn.kind, mock::DbKind::PostgreSql) {
            conn.user.as_str()
        } else {
            ""
        };
        let mut preview_sql = design_create_sql_at(
            conn.kind,
            schema_name,
            table,
            owner,
            &ds.cols,
            &ds.indexes,
            &ds.fks,
            &ds.uniques,
            &ds.checks,
            &ds.options,
            &ds.excludes,
            &ds.rules,
        );
        for stmt in design_triggers_sql_at(conn.kind, schema_name, table, &ds.triggers) {
            preview_sql.push('\n');
            preview_sql.push_str(&stmt);
        }
        // 「之前」一栏：打开设计器（或上次保存）时的基线快照，同一条流水线生成
        let mut baseline_sql = design_create_sql_at(
            conn.kind,
            schema_name,
            table,
            owner,
            &ds.baseline,
            &ds.baseline_indexes,
            &ds.baseline_fks,
            &ds.baseline_uniques,
            &ds.baseline_checks,
            &ds.baseline_options,
            &ds.baseline_excludes,
            &ds.baseline_rules,
        );
        for stmt in design_triggers_sql_at(conn.kind, schema_name, table, &ds.baseline_triggers) {
            baseline_sql.push('\n');
            baseline_sql.push_str(&stmt);
        }
        let change_sql = sql_preview::design_change_sql(conn.kind, schema_name, table, ds);

        // 「更改 | DDL」子页签：活动项蓝字 + 蓝下划线（贴着文字），点哪页换哪页
        let sql_subtab = |i: usize, label: &'static str, active: bool| -> AnyElement {
            h_flex()
                .id(rid(&format!("dsqlsub/{i}")))
                .test_support()
                .px(px(3.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(t) = this.opened.iter_mut().find(|t| t.did == did)
                        && let Some(d) = t.dstate.as_mut()
                    {
                        d.sql_sub = i;
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .text_size(px(12.))
                        .pb(px(2.))
                        .text_color(if active { accent } else { fg })
                        .when(active, |s| s.border_b_2().border_color(accent))
                        .child(label),
                )
                .into_any_element()
        };
        let sql_strip = h_flex()
            .h(px(24.))
            .flex_none()
            .items_center()
            .px(px(10.))
            .gap(px(18.))
            .child(sql_subtab(0, "更改", ds.sql_sub == 0))
            .child(sql_subtab(1, "DDL", ds.sql_sub == 1));
        let sql_body: AnyElement = if ds.sql_sub == 1 {
            // 「DDL」子页：之前 / 之后 两栏并排 diff，改动行整行绿底
            let before_lines: Vec<&str> = baseline_sql.lines().collect();
            let after_lines: Vec<&str> = preview_sql.lines().collect();
            let (chg_b, chg_a) = sql_preview::diff_lines(&baseline_sql, &preview_sql);
            let gutter_w = 28.;
            let w_before = sql_preview::code_lines_w(&before_lines, gutter_w);
            let w_after = sql_preview::code_lines_w(&after_lines, gutter_w);
            // id 挂外层（滚动条包一层内层 div，否则 overflow_scrollbar 会拆掉
            // test_support 的观察包装，元素就从测试注册表里消失）
            let pane = |id: &'static str, lines: &[&str], chg: &[bool], w: f32| {
                div()
                    .id(rid(id))
                    .test_support()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div().size_full().overflow_scrollbar().child(
                            v_flex().w(px(w)).children(lines.iter().enumerate().map(|(i, l)| {
                                sql_preview::diff_row(i + 1, l, chg[i], w, gutter_w, dark, fg)
                            })),
                        ),
                    )
            };
            v_flex()
                .size_full()
                .child(
                    h_flex()
                        .flex_none()
                        .px(px(10.))
                        .pt(px(6.))
                        .pb(px(3.))
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.))
                                .text_color(fg)
                                .child("之前:"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.))
                                .text_color(fg)
                                .child("之后:"),
                        ),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .px(px(10.))
                        .pb(px(6.))
                        .gap(px(10.))
                        .child(pane("dsql-before", &before_lines, &chg_b, w_before))
                        .child(pane("dsql-after", &after_lines, &chg_a, w_after)),
                )
                .into_any_element()
        } else {
            // 「更改」子页：增量脚本 + 右上角复制按钮；无改动就留白
            let lines: Vec<&str> = change_sql.lines().collect();
            let w_change = sql_preview::code_lines_w(&lines, 0.);
            let copy_btn = h_flex()
                .id("dsql-copy")
                .test_support()
                .absolute()
                .top(px(8.))
                .right(px(14.))
                .p(px(4.))
                .cursor_pointer()
                .hover(|s| s.bg(hover))
                .on_click({
                    let sql = change_sql.clone();
                    cx.listener(move |_, _, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(sql.clone()));
                    })
                })
                .child(
                    Icon::new(IconName::Copy)
                        .with_size(px(15.))
                        .text_color(to_hsla(0x4da0e1)),
                );
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("dsql-scroll")
                        .test_support()
                        .size_full()
                        .child(
                    div()
                        .size_full()
                        .overflow_scrollbar()
                        .child(v_flex().w(px(w_change)).pt(px(6.)).children(
                            lines.iter().map(|l| sql_preview::plain_row(l, w_change, dark, fg)),
                        )),
                ),
                )
                .when(!change_sql.is_empty(), |s| s.child(copy_btn))
                .into_any_element()
        };
        let sql_tab = v_flex()
            .size_full()
            .child(sql_strip)
            .child(sql_body)
            .into_any_element();

        let content: AnyElement = match ds.sub {
            0 => fields_tab,
            1 => self.indexes_tab(tab.did, ds, is_mysql, self.dark, cx),
            2 => self.fks_tab(tab.did, ds, is_mysql, self.dark, cx),
            3 => self.uniques_tab(tab.did, ds, self.dark, cx),
            4 => self.checks_tab(tab.did, ds, self.dark, cx),
            5 => self.excludes_tab(tab.did, ds, self.dark, cx),
            6 => self.rules_tab(tab.did, ds, self.dark, cx),
            7 => self.triggers_tab(tab.did, ds, self.dark, cx),
            8 => self.options_tab(tab.did, ds, self.dark, cx),
            9 => self.comment_tab(ds, self.dark, cx),
            10 => sql_tab,
            i => self.design_placeholder_tab(i, table, &conn, fg, subtle, bg, border),
        };

        // 底部不再放状态条：计数在窗口最底栏（status_bar）、「已修改」是页签标题前的 *、
        // 校验错挂在工具栏右端（见上面的 toolbar_tail）。
        let page = v_flex()
            .size_full()
            .bg(bg)
            .child(toolbar)
            .child(tabbar)
            .child(div().flex_1().min_h_0().child(content));
        // 浮层子编辑器（索引「字段」七列表格 / 外键等的「…」面板）挂在外层相对容器上
        let popup = self.design_popup_panel(tab.did, ds, cx);
        div()
            .relative()
            .size_full()
            .child(page)
            .when(ds.popup.is_some(), |s| s.child(popup))
            .into_any_element()
    }

    /// 设计器占位子页：索引 / 外键 / 唯一键 / 检查 / 排除 / 规则 / 触发器 / 选项 / 注释
    /// 每个子页显示即将实现的功能说明，保持界面结构完整
    fn design_placeholder_tab(
        &self,
        tab_index: usize,
        table: &str,
        conn: &mock::Connection,
        fg: Hsla,
        subtle: Hsla,
        bg: Hsla,
        _border: Hsla,
    ) -> AnyElement {
        let description = match tab_index {
            1 => format!("索引设计器（{}）：管理表的索引定义，包括主键索引、唯一索引、普通索引等。", table),
            2 => format!("外键设计器（{}）：定义表之间的外键关系，设置引用约束与级联规则。", table),
            3 => format!("唯一键设计器（{}）：管理唯一约束，确保列或列组合的值唯一。", table),
            4 => format!("检查约束设计器（{}）：定义 CHECK 约束，限制列值必须满足的条件。", table),
            5 => {
                if matches!(conn.kind, mock::DbKind::PostgreSql) {
                    format!("排除约束设计器（{}）：PostgreSQL 特有的排除约束，用于防止重叠范围等场景。", table)
                } else {
                    format!("排除约束（{}）：此约束类型仅 PostgreSQL 支持。", table)
                }
            }
            6 => format!("规则设计器（{}）：管理数据库规则（查询重写规则）。", table),
            7 => format!("触发器设计器（{}）：定义 BEFORE/AFTER/INSTEAD OF 触发器及其执行逻辑。", table),
            8 => format!("选项配置（{}）：设置表的存储引擎、字符集、排序规则等选项。", table),
            9 => format!("注释编辑器（{}）：为表及其列添加或修改注释说明。", table),
            _ => format!("子页 {} 即将实现。", tab_index),
        };

        v_flex()
            .size_full()
            .bg(bg)
            .p(px(24.))
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(fg)
                    .mb(px(12.))
                    .child(description),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(subtle)
                    .child("此功能将在后续版本中实现"),
            )
            .into_any_element()
    }

    /// 视图 / 实体化视图 / 函数设计器：简化版设计器，结构与表设计器类似但子页签不同
    fn view_or_function_designer(&self, tab: &OpenTab, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let (bg, border, soft, fg, muted, subtle, accent) = (
            to_hsla(p.background),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.accent),
        );
        let hover = to_hsla(p.hover);

        let ds = tab.dstate.as_ref().expect("设计页签必有状态");
        let did = tab.did;
        let conn = self
            .conns
            .iter()
            .find(|c| c.name == tab.conn_name)
            .cloned()
            .unwrap_or_else(|| EMPTY_CONN.clone());
        
        let obj_name = mock::objects(&conn, tab.db, tab.cat)
            .get(tab.obj)
            .copied()
            .unwrap_or_default();
        
        // 根据对象类型确定设计器标题和子页签
        let (designer_kind, subtabs): (&str, &[&str]) = match tab.cat {
            1 => ("视图", &["定义", "SQL 预览"]),
            2 => ("实体化视图", &["定义", "刷新选项", "SQL 预览"]),
            3 => ("函数", &["参数", "返回值", "函数体", "SQL 预览"]),
            _ => ("对象", &["SQL 预览"]),
        };

        // 视图类页签的「定义」正文也算修改；表设计器 view_body 恒空，不受影响
        let dirty = ds.cols != ds.baseline || !ds.view_body.trim().is_empty();
        let can_save = dirty;

        // 工具栏按钮
        let tbtn = |id: &'static str, icon: IconName, tint: u32, enabled: bool| -> Stateful<Div> {
            div()
                .id(rid(id))
                .w(px(D_ICON_BTN_W))
                .h(px(23.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(2.))
                .when(enabled, |s| s.cursor_pointer().hover(move |s| s.bg(hover)))
                .opacity(if enabled { 1. } else { 0.4 })
                .child(Icon::new(icon).with_size(px(14.)).text_color(to_hsla(tint)))
        };
        let tsep = || {
            div()
                .w(px(D_TOOLSEP_W))
                .h(px(23.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(div().w(px(1.)).h(px(23.)).bg(border))
        };

        let toolbar = h_flex()
            .h(px(D_TOOLBAR_H))
            .flex_none()
            .items_center()
            .px(px(4.))
            .bg(to_hsla(p.chrome))
            .border_b_1()
            .border_color(soft)
            .child(
                tbtn("dsave", IconName::Save, p.muted_foreground, can_save).when(can_save, {
                    |s| s.on_click(cx.listener(move |this, _, _, cx| this.design_save(did, cx)))
                }).test_support(),
            )
            .child(tsep())
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child(format!("{}设计器（mock 档，功能占位）", designer_kind)),
            );

        // 子标签条
        let tabbar = h_flex()
            .h(px(D_TABBAR_H))
            .flex_none()
            .items_end()
            .px(px(4.))
            .bg(to_hsla(p.chrome))
            .border_b_1()
            .border_color(soft)
            .children(subtabs.iter().enumerate().map(|(i, label)| {
                let active = ds.sub == i;
                div()
                    .id(rid(&format!("dsub/{}", i)))
                    .px(px(12.))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(if active { accent } else { soft })
                    .bg(if active { bg } else { to_hsla(p.chrome) })
                    .cursor_pointer()
                    .hover(|s| s.bg(to_hsla(0xf0f0f0)))
                    .on_click(cx.listener({
                        let did = did;
                        move |this, _, _, cx| {
                            if let Some(t) = this.opened.iter_mut().find(|t| t.did == did)
                                && let Some(d) = t.dstate.as_mut()
                            {
                                d.sub = i;
                                cx.notify();
                            }
                        }
                    }))
                    .child(div().text_size(px(12.)).text_color(if active { fg } else { muted }).child(*label))
            }));

        // 内容区：根据子页签显示不同内容
        let content: AnyElement = match ds.sub {
            0 if tab.cat == 1 || tab.cat == 2 => {
                // 「定义」页：代码编辑器（SQL 高亮 + 补全），正文写回 ds.view_body
                match ds.view_editor.clone() {
                    Some(editor) => div()
                        .size_full()
                        .bg(bg)
                        .p(px(6.))
                        .child(Editor::new(&editor).h_full())
                        .into_any_element(),
                    None => v_flex()
                        .size_full()
                        .bg(bg)
                        .p(px(24.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(subtle)
                                .child("编辑器尚未就绪"),
                        )
                        .into_any_element(),
                }
            }
            _ if ds.sub == subtabs.len() - 1 => {
                // SQL 预览页：由「定义」正文生成
                let sql = design_view_sql(designer_kind, obj_name, &ds.view_body);
                div()
                    .id(rid("dsql"))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .bg(to_hsla(0xffffff))
                    .p(px(12.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_family("Consolas")
                            .text_color(fg)
                            .child(sql),
                    )
                    .into_any_element()
            }
            _ => {
                // 其他子页签：占位内容
                v_flex()
                    .size_full()
                    .bg(bg)
                    .p(px(24.))
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(fg)
                            .mb(px(12.))
                            .child(format!("{} - {}（即将实现）", designer_kind, subtabs.get(ds.sub).unwrap_or(&""))),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(subtle)
                            .child("此功能将在后续版本中实现"),
                    )
                    .into_any_element()
            }
        };

        // 底部不放状态条（计数在窗口最底栏，「已修改」看页签标题前的 *）
        v_flex()
            .size_full()
            .bg(bg)
            .child(toolbar)
            .child(tabbar)
            .child(div().flex_1().min_h_0().child(content))
            .into_any_element()
    }

    /// 「新建查询」页：运行上下文栏（连接 / 库 / 模式下拉 + 运行 / 停止 / 解释）
    /// + SQL 编辑器（行号 + 等宽）+ 结果面板（状态头 + 消息 / 结果页签）。
    /// 布局与量照 Flutter 版 query_page.dart 复刻 Navicat 查询窗口的实测。
    fn query_page(&self, tab: &OpenTab, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let (bg, border, soft, fg, muted) = (
            to_hsla(p.background),
            to_hsla(p.border),
            to_hsla(p.border_soft),
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
        );
        let hover = to_hsla(p.hover);
        let chrome = to_hsla(p.chrome);
        let q = tab.qstate.as_ref().expect("查询页签必有状态");
        let qid = tab.qid;
        let conn = self
            .conns
            .iter()
            .find(|c| c.name == q.conn)
            .cloned()
            .unwrap_or_else(|| EMPTY_CONN.clone());
        let show_schema = mock::has_schemas(conn.kind);
        let db = mock::databases(conn.kind).get(q.db).copied().unwrap_or("");
        let schema = mock::schemas(conn.kind)
            .get(q.schema)
            .copied()
            .unwrap_or("public");
        let has_selection = !q.editor.read(cx).selected_range().is_empty();
        let me = cx.entity();

        // ── 运行上下文栏 ─────────────────────────────────────
        // 三个白底定宽下拉（图标 + 名称左对齐、箭头钉在右端）+「▷ 运行 ▾ /
        // □ 停止 / 解释」左对齐（不推到右端），量照 Navicat Lite 实测。
        let conn_tint = match conn.kind {
            DbKind::MySql => 0x1f86c8,
            DbKind::PostgreSql => 0x39709b,
            DbKind::SqlServer => 0xc0392b,
            DbKind::MongoDb => 0x4db33d,
            DbKind::Redis => 0xd0342c,
        };
        let conn_btn = Button::new("qconn")
            .icon(Icon::new(IconName::Database).text_color(to_hsla(conn_tint)))
            .label(q.conn.clone())
            .dropdown_caret(true)
            .w(px(Q_CONN_W))
            .h(px(Q_TRIGGER_H))
            .text_size(px(12.))
            .rounded(px(2.))
            .border_1()
            .border_color(border)
            .dropdown_menu({
                let me = me.clone();
                let qid = qid;
                move |menu, _, cx| {
                    let conns = me.read(cx).conns.clone();
                    let mut m = menu;
                    for c in conns {
                        let me = me.clone();
                        let name = c.name.clone();
                        m = m.item(PopupMenuItem::new(c.name).on_click(move |_, _, cx| {
                            me.update(cx, |a, cx| a.query_set_conn(qid, &name, cx));
                        }));
                    }
                    m
                }
            });
        let db_btn = Button::new("qdb")
            .icon(Icon::new(IconName::Database).text_color(to_hsla(0x2fa84f)))
            .label(db.to_string())
            .dropdown_caret(true)
            .w(px(Q_DB_W))
            .h(px(Q_TRIGGER_H))
            .text_size(px(12.))
            .rounded(px(2.))
            .border_1()
            .border_color(border)
            .dropdown_menu({
                let me = me.clone();
                let qid = qid;
                move |menu, _, cx| {
                    let app = me.read(cx);
                    let conn = app.conns.iter().find(|c| c.name == app.active_query_conn(qid));
                    let dbs: Vec<&str> = conn
                        .map(|c| mock::databases(c.kind).to_vec())
                        .unwrap_or_default();
                    let mut m = menu;
                    for d in dbs {
                        let me = me.clone();
                        let d = d.to_string();
                        m = m.item(PopupMenuItem::new(d.clone()).on_click(move |_, _, cx| {
                            me.update(cx, |a, cx| a.query_set_db(qid, &d, cx));
                        }));
                    }
                    m
                }
            });
        let run_btn = DropdownButton::new("qrun")
            .button(
                Button::new("qrun-main")
                    .icon(Icon::new(IconName::Play).text_color(to_hsla(Q_RUN_BLUE)))
                    .label(if has_selection { "运行已选择的" } else { "运行" })
                    .text_size(px(12.))
                    .h(px(Q_TRIGGER_H))
                    .on_click({
                        let me = me.clone();
                        move |_, window, cx| {
                            // 有选区时主按钮就是「运行已选择的」（同 Navicat），
                            // 别让标签与行为各说各话
                            me.update(cx, |a, cx| a.query_run(qid, has_selection, window, cx));
                        }
                    }),
            )
            .dropdown_menu({
                let me = me.clone();
                move |menu, _, _cx| {
                    menu.item(PopupMenuItem::new("运行").on_click({
                        let me = me.clone();
                        move |_, window, cx| {
                            me.update(cx, |a, cx| a.query_run(qid, false, window, cx));
                        }
                    }))
                    .item(
                        PopupMenuItem::new("运行已选择的")
                            .disabled(!has_selection)
                            .on_click({
                                let me = me.clone();
                                move |_, window, cx| {
                                    me.update(cx, |a, cx| a.query_run(qid, true, window, cx));
                                }
                            }),
                    )
                }
            });
        let stop_btn = Button::new("qstop")
            .icon(Icon::new(IconName::SquareStop).text_color(to_hsla(Q_STOP_IDLE)))
            .label("停止")
            .text_size(px(12.))
            .h(px(Q_TRIGGER_H))
            .disabled(true)
            .when(true, |b| b.opacity(0.6));
        let explain_btn = Button::new("qexplain")
            .icon(Icon::new(IconName::Network).text_color(to_hsla(Q_EXPLAIN_BLUE)))
            .label("解释")
            .text_size(px(12.))
            .h(px(Q_TRIGGER_H))
            .disabled(true)
            .when(true, |b| b.opacity(0.6));

        // ── 工具条：保存 / 美化 SQL / 代码段（带 Ent 角标的「查询创建工具」
        //    「询问 AI」按用户要求不放）──
        let qbtn = |id: &'static str, glyph: art::Glyph, label: &'static str, enabled: bool| {
            h_flex()
                .id(rid(&format!("qp/{id}")))
                .h(px(22.))
                .flex_none()
                .px(px(5.))
                .gap(px(4.))
                .items_center()
                .rounded(px(2.))
                .when(enabled, |s| s.cursor_pointer().hover(move |s| s.bg(hover)))
                .opacity(if enabled { 1. } else { 0.85 })
                .child(art::icon(glyph, 15., muted))
                .child(div().text_size(px(12.)).text_color(fg).child(label))
        };
        let qsep = || div().w(px(1.)).h(px(15.)).mx(px(6.)).flex_none().bg(border);
        let qbar = h_flex()
            .h(px(T_TOOLBAR_H))
            .flex_none()
            .items_center()
            .px(px(6.))
            .gap(px(2.))
            .bg(chrome)
            .border_b_1()
            .border_color(soft)
            .child(
                qbtn("save", art::Glyph::SaveFile, "保存", true).on_click(cx.listener(
                    move |this, _, _, cx| {
                        // mock 档没有磁盘文件：保存 = 清掉「未保存」标记（页签的 *）
                        if let Some(t) = this.opened.iter_mut().find(|t| t.qid == qid)
                            && let Some(q) = t.qstate.as_mut()
                        {
                            q.dirty = false;
                        }
                        cx.notify();
                    },
                )),
            )
            .child(qsep())
            .child(qbtn("pretty", art::Glyph::Beautify, "美化 SQL", false))
            .child(qbtn("snippet", art::Glyph::Snippet, "代码段", false))
            .child(div().flex_1());

        let conn_bar = h_flex()
            .h(px(Q_CONNBAR_H))
            .flex_none()
            .items_center()
            .px(px(6.))
            .gap(px(4.))
            .bg(to_hsla(p.chrome))
            .border_b_1()
            .border_color(soft)
            .child(conn_btn)
            .child(db_btn)
            .when(show_schema, |s| {
                s.child(
                    Button::new("qschema")
                        .icon(Icon::new(IconName::Boxes).text_color(to_hsla(0x2e9e63)))
                        .label(schema.to_string())
                        .dropdown_caret(true)
                        .w(px(Q_SCHEMA_W))
                        .h(px(Q_TRIGGER_H))
                        .text_size(px(12.))
                        .rounded(px(2.))
                        .border_1()
                        .border_color(border)
                        .dropdown_menu({
                            let me = me.clone();
                            move |menu, _, cx| {
                                let app = me.read(cx);
                                let conn = app
                                    .conns
                                    .iter()
                                    .find(|c| c.name == app.active_query_conn(qid));
                                let schemas: Vec<&str> = conn
                                    .map(|c| mock::schemas(c.kind).to_vec())
                                    .unwrap_or_default();
                                let mut m = menu;
                                for sc in schemas {
                                    let me = me.clone();
                                    let sc = sc.to_string();
                                    m = m.item(PopupMenuItem::new(sc.clone()).on_click(
                                        move |_, _, cx| {
                                            me.update(cx, |a, cx| a.query_set_schema(qid, &sc, cx));
                                        },
                                    ));
                                }
                                m
                            }
                        }),
                )
            })
            .child(div().w(px(6.)))
            // 包一层观察容器只为了让测试能点「运行」本体（Button 自身不注册观察）
            .child(div().id("qrun-wrap").flex_none().test_support().child(run_btn))
            .child(div().w(px(2.)))
            .child(stop_btn)
            .child(div().w(px(4.)))
            .child(explain_btn)
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap(px(2.))
                    .opacity(0.45)
                    .child(Icon::new(IconName::Columns2).with_size(px(15.)).text_color(muted))
                    .child(Icon::new(IconName::Rows2).with_size(px(15.)).text_color(muted)),
            );

        // ── 结果面板 ─────────────────────────────────────────
        let n_out = q.outcomes.len();
        let status = Self::query_status_text(q);
        let header = h_flex()
            .id("qhead")
            .h(px(Q_HEADER_H))
            .flex_none()
            .items_center()
            .pl(px(12.))
            .pr(px(8.))
            .bg(chrome)
            .border_t_1()
            .border_color(border)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child(status),
            )
            .test_support();

        // 结果页签条：消息 + 结果 1..N（只在出过结果后出现）
        let tabbar = if n_out > 0 {
            h_flex()
                .h(px(Q_TABBAR_H))
                .flex_none()
                .items_stretch()
                .bg(chrome)
                .border_b_1()
                .border_color(border)
                .children((0..=n_out).map(|i| {
                    let active = q.pane == i;
                    h_flex()
                        .id(rid(&format!("qtab/{i}")))
                        .px(px(11.5))
                        .items_center()
                        .when(active, |s| s.bg(bg))
                        .when(!active, |s| s.cursor_pointer().hover(move |s| s.bg(hover)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(t) = this.opened.iter_mut().find(|t| t.qid == qid)
                                && let Some(q) = t.qstate.as_mut()
                            {
                                q.pane = i;
                                cx.notify();
                            }
                        }))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(if active { fg } else { muted })
                                .child(if i == 0 {
                                    "消息".to_string()
                                } else {
                                    format!("结果 {i}")
                                }),
                        )
                        .test_support()
                }))
                .into_any_element()
        } else {
            div().into_any_element()
        };

        // 内容区：消息日志（只读、可拖选复制）或结果网格
        let body: AnyElement = if n_out == 0 {
            div()
                .size_full()
                .flex()
                .items_start()
                .child(
                    div()
                        .p(px(12.))
                        .text_size(px(12.5))
                        .text_color(muted)
                        .child(q.prompt.unwrap_or("").to_string()),
                )
                .into_any_element()
        } else if q.pane == 0 {
            div()
                .id("qmsg")
                .size_full()
                .overflow_hidden()
                .p(px(12.))
                .child(
                    Textarea::new(&q.log)
                        .appearance(false)
                        .bordered(false)
                        .readonly(true)
                        .text_size(px(12.5))
                        .text_color(fg),
                )
                .test_support()
                .into_any_element()
        } else {
            self.query_grid(q, qid, cx).into_any_element()
        };

        v_flex()
            .size_full()
            .bg(bg)
            .child(qbar)
            .child(conn_bar)
            .child(
                div()
                    .id("qedit")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .bg(bg)
                    .test_support()
                    .child(
                        Editor::new(&q.editor)
                            .appearance(false)
                            .bordered(false)
                            .h_full(),
                    ),
            )
            .when(n_out > 0, |s| {
                s.child({
                // 可拖动的分隔条：按下后跟踪鼠标 Y 位移，实时更新面板高度
                let qid_for_split = qid;
                div()
                    .id("qsplit")
                    .h(px(D_PROP_SPLITTER_H))
                    .flex_none()
                    .bg(to_hsla(p.chrome))
                    .cursor_row_resize()
                    .hover(|s| s.bg(to_hsla(0xcccccc)))
                    .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                        // 点击分隔条不做任何事，拖动才调整高度
                    }))
                    .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, window, cx| {
                        // 鼠标移动时不直接改高度，需要配合鼠标按下状态
                    }))
                    .test_support()
                })
                .child(
                    v_flex()
                        .h(px(q.panel_h))
                        .flex_none()
                        .border_t_1()
                        .border_color(border)
                        .child(header)
                        .child(tabbar)
                        .child(div().flex_1().min_h_0().overflow_hidden().child(body)),
                )
            })
            .into_any_element()
    }

    /// 结果网格：单行列头（仅列名）+ 行号列 + 数据行，密度同表数据页
    /// （行高 17.5、横向留白 6、数值列右对齐）。列形共用 MOCK_COLUMNS。
    fn query_grid(&self, q: &QueryState, qid: u64, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let (soft, fg) = (to_hsla(p.border_soft), to_hsla(p.foreground));
        let hover = to_hsla(p.hover);
        let selection = to_hsla(p.selection);
        let accent = p.accent;
        let cols = &mock::MOCK_COLUMNS;
        let oi = q.pane.saturating_sub(1);
        let Some(out) = q.outcomes.get(oi) else {
            return div().into_any_element();
        };
        let rows = out.result.as_deref().unwrap_or(&[]);
        let cell = out.cell;
        let total_w = Q_ROWNO_W + cols.iter().map(|c| c.width).sum::<f32>();

        let header = h_flex()
            .h(px(Q_GRID_HEADER_H))
            .flex_none()
            .child(
                div()
                    .w(px(Q_ROWNO_W))
                    .h_full()
                    .flex_none()
                    .bg(to_hsla(p.chrome))
                    .border_b_1()
                    .border_r_1()
                    .border_color(soft),
            )
            .children(cols.iter().map(|col| {
                div()
                    .w(px(col.width))
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(6.))
                    .bg(to_hsla(p.chrome))
                    .border_b_1()
                    .border_r_1()
                    .border_color(soft)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.5))
                            .font_medium()
                            .text_color(fg)
                            .child(col.name),
                    )
            }));

        let grid = v_flex()
            .w(px(total_w))
            .flex_none()
            .child(header)
            .children(rows.iter().enumerate().map(|(rix, row)| {
                let selected = cell.0 == rix;
                h_flex()
                    .flex_none()
                    .h(px(Q_ROW_H))
                    .when(selected, |s| s.bg(selection))
                    .when(!selected, |s| s.hover(move |s| s.bg(hover)))
                    .child(
                        div()
                            .w(px(Q_ROWNO_W))
                            .h_full()
                            .flex_none()
                            .when(selected, |s| {
                                s.bg(to_hsla(if p.background > 0x808080 {
                                    blend(accent, 0xffffff, 0.62)
                                } else {
                                    accent
                                }))
                            })
                            .border_b_1()
                            .border_r_1()
                            .border_color(soft),
                    )
                    .children(row.iter().enumerate().zip(cols.iter()).map(|((cix, value), col)| {
                        let current = cell == (rix, cix);
                        div()
                            .id(rid(&format!("qcell/{oi}/{rix}/{cix}")))
                            .w(px(col.width))
                            .h_full()
                            .flex_none()
                            .px(px(6.))
                            .flex()
                            .items_center()
                            .when(col.numeric, |s| s.justify_end())
                            .border_b_1()
                            .border_r_1()
                            .border_color(soft)
                            .when(cix == 0, |s| s.border_l_1().border_color(soft))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.query_pick_cell(qid, oi, rix, cix, cx);
                            }))
                            .test_support()
                            .child(
                                div()
                                    .min_w_0()
                                    .max_w_full()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .when(current, |s| s.text_color(to_hsla(0xffffff)))
                                    .when(!current, |s| s.text_color(fg))
                                    .child(value.clone()),
                            )
                            .when(current, |s| s.bg(to_hsla(accent)))
                    }))
            }));

        div()
            .id(rid(&format!("qgrid-scroll/{oi}")))
            .test_support()
            .size_full()
            .context_menu(grid_context_menu(cx.entity().clone(), self.dark))
            .child(
                div()
                    .size_full()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                        .id(rid(&format!("qgrid-x/{oi}")))
                        .w(px(total_w))
                        .overflow_x_scrollbar()
                        .child(grid),
                    ),
            )
            .into_any_element()
    }

    /// 查询页签切换运行上下文：连接（库 / 模式回到第一个）。
    fn query_set_conn(&mut self, qid: u64, name: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.opened.iter_mut().find(|t| t.qid == qid)
            && let Some(q) = t.qstate.as_mut()
        {
            q.conn = name.to_string();
            q.db = 0;
            q.schema = 0;
            cx.notify();
        }
    }

    fn query_set_db(&mut self, qid: u64, db: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.opened.iter_mut().find(|t| t.qid == qid)
            && let Some(q) = t.qstate.as_mut()
            && let Some(conn) = self.conns.iter().find(|c| c.name == q.conn)
            && let Some(i) = mock::databases(conn.kind).iter().position(|d| *d == db)
        {
            q.db = i;
            cx.notify();
        }
    }

    fn query_set_schema(&mut self, qid: u64, schema: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.opened.iter_mut().find(|t| t.qid == qid)
            && let Some(q) = t.qstate.as_mut()
            && let Some(conn) = self.conns.iter().find(|c| c.name == q.conn)
            && let Some(i) = mock::schemas(conn.kind).iter().position(|s| *s == schema)
        {
            q.schema = i;
            cx.notify();
        }
    }

    /// 查询页签当前的运行上下文连接名（下拉菜单构建期读）。
    fn active_query_conn(&self, qid: u64) -> String {
        self.opened
            .iter()
            .find(|t| t.qid == qid)
            .and_then(|t| t.qstate.as_ref())
            .map(|q| q.conn.clone())
            .unwrap_or_default()
    }

    fn action_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (chrome, border, fg, muted, hover) = (
            to_hsla(p.chrome),
            to_hsla(p.border),
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.hover),
        );
        // 设计表只对树里选中的表节点开放（cat 0 = 表）
        // 表 / 视图 / 实体化视图 / 函数都能进设计器（k 为对象类别）
        let design_enabled = matches!(self.sel, Act::Obj(_, _, _, 0..=3, _));

        h_flex()
            .h(px(30.))
            .px(px(4.))
            .gap(px(1.))
            .items_center()
            .bg(chrome)
            .border_b_1()
            .border_color(border)
            .child(action_item(
                "act-open",
                art::Glyph::OpenTable,
                0x2f8fd8,
                "打开表",
                false,
                false,
                fg,
                hover,
                muted,
            ))
            .child(
                action_item(
                    "act-design",
                    art::Glyph::DesignTable,
                    0x3f8fbf,
                    "设计表",
                    false,
                    design_enabled,
                    fg,
                    hover,
                    muted,
                )
                .when(design_enabled, |s| {
                    s.on_click(cx.listener(|this, _, window, cx| {
                        let act = this.sel;
                        this.open_designer(act, window, cx);
                    }))
                })
                .test_support(),
            )
            .child(
                action_item(
                    "act-new",
                    art::Glyph::NewTable,
                    0x2fa84f,
                    "新建表",
                    true,
                    true,
                    fg,
                    hover,
                    muted,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    // 动作条只挂在对象页上，上下文取树里的选中节点：
                    // 对象 / 分类 / 模式直接给 (连接, 库, 模式)，库节点用
                    // NO_SCHEMA 兜底，根 / 分组 / 连接选中时不响应
                    let (c, d, s) = match this.sel {
                        Act::Obj(c, d, s, ..) | Act::Col(c, d, s, ..) => (c, d, s),
                        Act::Cat(c, d, s, _) | Act::Schema(c, d, s) => (c, d, s),
                        Act::Db(c, d) => (c, d, NO_SCHEMA),
                        _ => return,
                    };
                    this.open_new_designer(Act::Obj(c, d, s, 0, 0), window, cx);
                }))
                .test_support(),
            )
            .child(action_item(
                "act-ai",
                art::Glyph::AiTable,
                0xe08a3c,
                "通过 AI 新建表",
                true,
                false,
                fg,
                hover,
                muted,
            ))
            .child(action_item(
                "act-delete",
                art::Glyph::DropTable,
                0xd2504a,
                "删除表",
                false,
                false,
                fg,
                hover,
                muted,
            ))
            .child(div().w(px(1.)).h(px(14.)).mx(px(2.)).bg(border))
            .child(action_item(
                "act-import",
                art::Glyph::ImportWizard,
                0x2fa84f,
                "导入向导",
                true,
                false,
                fg,
                hover,
                muted,
            ))
            .child(action_item(
                "act-export",
                art::Glyph::ExportWizard,
                0xe08a3c,
                "导出向导",
                true,
                false,
                fg,
                hover,
                muted,
            ))
            .child(div().flex_1())
            .child(
                Icon::new(IconName::Search)
                    .with_size(px(15.))
                    .mr(px(6.))
                    .text_color(muted),
            )
    }

    fn object_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, hover, sel, accent) = (
            to_hsla(p.foreground),
            to_hsla(p.hover),
            to_hsla(p.selection),
            to_hsla(p.accent),
        );
        let icon = self.pane_icon();
        let is_table = self.pane_cat().unwrap_or(0) == 0;
        let path = self.pane_path();

        v_flex()
            .py(px(1.))
            .context_menu(object_context_menu(cx.entity().clone(), self.sel))
            .children(
                self.pane_objects()
                    .iter()
                    .enumerate()
                    .map(|(ix, name)| {
                        let selected = self.sel.obj() == Some(ix) && self.sel.cat() == Some(path.3);
                        let act = Act::Obj(path.0, path.1, path.2, path.3, ix);
                        h_flex()
                            .id(("obj", ix))
                            .h(px(LIST_ROW_H))
                            .pl(px(2.))
                            .pr(px(6.))
                            .gap(px(4.))
                            .items_center()
                            .cursor_pointer()
                            .when(selected, move |s| s.bg(sel))
                            .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                // 对象面板双击 = 打开表
                                if this.is_double_click(act) {
                                    this.open_object(act);
                                }
                                this.sel = act;
                                cx.notify();
                            }))
                            .child(art::icon(
                                if is_table { art::Glyph::Table } else { icon },
                                16.,
                                accent,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_xs()
                                    .text_color(fg)
                                    .child(*name),
                            )
                    }),
            )
    }

    fn object_grid(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, hover, sel, accent) = (
            to_hsla(p.foreground),
            to_hsla(p.hover),
            to_hsla(p.selection),
            to_hsla(p.accent),
        );
        let icon = self.pane_icon();
        let is_table = self.pane_cat().unwrap_or(0) == 0;
        let path = self.pane_path();

        h_flex()
            .gap(px(4.))
            .items_start()
            .flex_wrap()
            .context_menu(object_context_menu(cx.entity().clone(), self.sel))
            .children(self.pane_objects().iter().enumerate().map(|(ix, name)| {
                let selected = self.sel.obj() == Some(ix) && self.sel.cat() == Some(path.3);
                let act = Act::Obj(path.0, path.1, path.2, path.3, ix);
                v_flex()
                    .id(("tile", ix))
                    .w(px(96.))
                    .h(px(68.))
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .when(selected, move |s| s.bg(sel))
                    .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // 对象面板双击 = 打开表
                        if this.is_double_click(act) {
                            this.open_object(act);
                        }
                        this.sel = act;
                        cx.notify();
                    }))
                    .child(art::icon(
                        if is_table { art::Glyph::Table } else { icon },
                        22.,
                        accent,
                    ))
                    .child(
                        div()
                            .px(px(4.))
                            .truncate()
                            .text_xs()
                            .text_color(fg)
                            .child(*name),
                    )
            }))
    }

    fn info(&self) -> AnyElement {
        let p = self.pal();
        let (bg, fg, muted, subtle, accent) = (
            to_hsla(p.background),
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.subtle_foreground),
            to_hsla(p.accent),
        );
        let conn = self.active_conn();
        // 右侧信息卡按选中的节点类型换内容：Navicat 里选中连接、库、模式、
        // 对象各自是一张不同的卡，字段名与顺序照实测截图。
        let card = match self.sel {
            Act::Obj(_, _, _, _, o) | Act::Col(_, _, _, _, o, _) => self
                .pane_objects()
                .get(o)
                .copied()
                .map(Card::Obj)
                .unwrap_or(Card::Conn),
            Act::Db(..) => self.sel_db_name().map_or(Card::Conn, Card::Db),
            Act::Schema(..) => self.sel_schema_name().map_or(Card::Conn, Card::Schema),
            // 根节点与分组没有详情卡：Navicat 里右侧是空的，不放一张假的连接卡
            Act::Root | Act::Group(_) => Card::None,
            _ => Card::Conn,
        };
        let (title, kind, tile_icon) = match &card {
            Card::None => ("", "", None),
            Card::Obj(name) => (*name, self.pane_label(), Some(art::Glyph::HeroTable)),
            Card::Db(name) => (*name, "数据库", Some(art::Glyph::HeroDb)),
            Card::Schema(name) => (*name, "模式", Some(art::Glyph::HeroSchema)),
            Card::Conn => (
                conn.name.as_str(),
                if conn.kind == DbKind::PostgreSql {
                    "PostgreSQL"
                } else {
                    conn.kind.label()
                },
                Some(art::Glyph::HeroDb),
            ),
        };

        let mut props: Vec<AnyElement> = Vec::new();
        match &card {
            Card::None => {}
            Card::Obj(_) => {
                // PG / SQL Server 在库下面还有一层模式；MySQL 的库即模式，只有两行
                let schema = match conn.kind {
                    DbKind::PostgreSql => Some("public"),
                    DbKind::SqlServer => Some("dbo"),
                    _ => None,
                };
                props.push(
                    v_flex()
                        .child(source_row(art::Glyph::Conn(conn.kind), &conn.name, fg, subtle))
                        .child(source_row(
                            art::Glyph::Db,
                            self.sel_db_name().unwrap_or(&conn.database),
                            fg,
                            subtle,
                        ))
                        .when_some(schema, |s, name| {
                            s.child(source_row(art::Glyph::Schema, name, fg, subtle))
                        })
                        .into_any_element(),
                );
                for (label, value) in [
                    ("OID", "43830".to_string()),
                    ("所有者", "postgres".to_string()),
                    ("行", "-1".to_string()),
                    ("Table Type", "常规".to_string()),
                    ("分区属于", "--".to_string()),
                    ("表空间", "--".to_string()),
                    ("Inherits From", "--".to_string()),
                    ("Has OIDs", "否".to_string()),
                    ("填充因子", "--".to_string()),
                    ("ACL", "--".to_string()),
                    ("注释", "--".to_string()),
                ] {
                    props.push(prop_block(label, value, fg, subtle));
                }
            }
            Card::Db(db) => {
                props.push(
                    source_row(art::Glyph::Conn(conn.kind), &conn.name, fg, subtle).into_any_element(),
                );
                for (label, value) in [
                    ("OID", "43604".to_string()),
                    ("所有者", conn.user.clone()),
                    ("表空间", "pg_default".to_string()),
                    ("编码", "UTF8".to_string()),
                    ("排序规则排序", "en_US.utf8".to_string()),
                    ("连接限制", "无".to_string()),
                    ("注释", "--".to_string()),
                ] {
                    props.push(prop_block(label, value.to_string(), fg, subtle));
                }
                let _ = db;
            }
            Card::Schema(_) => {
                props.push(
                    v_flex()
                        .child(source_row(art::Glyph::Conn(conn.kind), &conn.name, fg, subtle))
                        .child(source_row(
                            art::Glyph::Db,
                            self.sel_db_name().unwrap_or(&conn.database),
                            fg,
                            subtle,
                        ))
                        .into_any_element(),
                );
                for (label, value) in [
                    ("OID", "2200".to_string()),
                    ("所有者", conn.user.clone()),
                    ("注释", "--".to_string()),
                ] {
                    props.push(prop_block(label, value.to_string(), fg, subtle));
                }
            }
            Card::Conn => {
                for (label, value) in [
                    ("活动配置文件", "主要配置文件".to_string()),
                    ("服务器版本", "180003".to_string()),
                    (
                        "会话",
                        if conn.connected { "2" } else { "0" }.to_string(),
                    ),
                    ("主机", conn.host.clone()),
                    ("端口", conn.port.to_string()),
                    ("初始数据库", conn.database.clone()),
                    (
                        "用户名",
                        if conn.user.is_empty() {
                            "--".to_string()
                        } else {
                            conn.user.clone()
                        },
                    ),
                    (
                        "设置位置",
                        format!("C:\\Users\\{}\\Documents\\daro-rs\\{}", "Sprin", conn.kind.label()),
                    ),
                    ("备注", "--".to_string()),
                ] {
                    props.push(prop_block(label, value, fg, subtle));
                }
            }
        }

        v_flex()
            .size_full()
            .bg(bg)
            .child(
                h_flex()
                    .h(px(26.))
                    .px(px(8.))
                    .gap(px(8.))
                    .items_center()
                    .child(div().flex_1())
                    .child(Icon::new(IconName::Info).with_size(px(15.)).text_color(accent))
                    .child(
                        Icon::new(IconName::SquareCode)
                            .with_size(px(15.))
                            .text_color(muted),
                    )
                    .child(
                        Icon::new(IconName::Columns2)
                            .with_size(px(15.))
                            .text_color(muted),
                    )
                    .child(Icon::new(IconName::Rows2).with_size(px(15.)).text_color(muted))
                    .child(div().flex_1())
                    .child(
                        Icon::new(IconName::Maximize2)
                            .with_size(px(14.))
                            .text_color(muted),
                    ),
            )
            .child(
                div()
                    .id("info-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .px(px(16.))
                            .py(px(12.))
                            .gap(px(9.))
                            .when(!matches!(card, Card::None), |s| {
                                s.child(
                                h_flex()
                                    .gap(px(14.))
                                    .items_start()
                                    .child(match tile_icon {
                                        Some(glyph) => art::art(glyph, 62.),
                                        None => art::art(art::Glyph::HeroDb, 62.),
                                    })
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .child(
                                                div()
                                                    .text_size(px(15.))
                                                    .font_semibold()
                                                    .text_color(fg)
                                                    .child(title.to_string()),
                                            )
                                            .child(
                                                div()
                                                    .mt(px(2.))
                                                    .text_size(px(13.))
                                                    .text_color(muted)
                                                    .child(kind),
                                            )
                                            .child(
                                                h_flex()
                                                    .id("share-link")
                                                    .mt(px(4.))
                                                    .gap(px(3.))
                                                    .items_center()
                                                    .cursor_pointer()
                                                    .hover(move |s| s.underline())
                                                    .child(
                                                        Icon::new(IconName::Share2)
                                                            .with_size(px(13.))
                                                            .text_color(accent),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .text_color(accent)
                                                            .child("共享"),
                                                    ),
                                            ),
                                    ),
                            )
                            })
                            .children(props)
                            .when(matches!(card, Card::Obj(_)), |s| {
                                s.child(
                                    h_flex()
                                        .id("reset-limit")
                                        .mt(px(4.))
                                        .cursor_pointer()
                                        .hover(move |s| s.underline())
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(accent)
                                                .child("重置限制记录设置"),
                                        ),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (bg, border, fg, muted) = (
            to_hsla(p.status_bar),
            to_hsla(p.border),
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
        );
        let (count, unit) = self.sel_child_count();
        // Navicat 在有搜索过滤时追加「（N 个项目已隐藏）」，过滤功能落地前恒为 0
        let status = match count {
            Some(n) => format!("{n} 个 {unit}"),
            None if self.pane_obj().is_some() => "已选择 1 个项目".to_string(),
            None => "就绪".to_string(),
        };
        // 设计器页签：窗口最底栏最左两格是「字段数」+ 当前子页对应的计数
        // （索引 → 索引数，外键 → 外键数，唯一键 → 唯一键数 …）。字段页 / 选项 /
        // 注释 / SQL 预览这些子页没有对应集合，就只留「字段数」一格。
        let designer = self.active_table().filter(|t| t.design).and_then(|t| {
            t.dstate.as_ref().map(|d| {
                let extra = match d.sub {
                    1 => Some(("索引数", d.indexes.iter().filter(|i| !design_index_blank(i)).count())),
                    // 种子空行不算（参考图 0 条外键时显示 外键数: 0）
                    2 => Some(("外键数", d.fks.iter().filter(|f| !design_fk_blank(f)).count())),
                    // 种子空行不算（参考图 0 条唯一键时显示 唯一键数: 0）
                    3 => Some((
                        "唯一键数",
                        d.uniques.iter().filter(|u| !design_pair_blank(u)).count(),
                    )),
                    // 种子空行不算
                    4 => Some((
                        "检查数",
                        d.checks.iter().filter(|c| !design_pair_blank(c)).count(),
                    )),
                    // 种子空行不算
                    5 => Some((
                        "排除数",
                        d.excludes
                            .iter()
                            .filter(|e| {
                                !(e.name.trim().is_empty() && e.elements.trim().is_empty())
                            })
                            .count(),
                    )),
                    6 => Some(("规则数", d.rules.iter().filter(|r| !design_rule_blank(r)).count())),
                    7 => Some(("触发器数", d.triggers.iter().filter(|t| !design_trigger_blank(t)).count())),
                    _ => None,
                };
                (d.cols.len(), extra)
            })
        });
        // 数据页签激活时状态栏换成取数 SQL 与记录计数（Navicat 同款两段）
        let (sql, recinfo) = self
            .active_table()
            .map(|t| self.table_page_status(&t))
            .unwrap_or_default();
        let has_sql = designer.is_none() && !sql.is_empty();

        h_flex()
            .h(px(22.))
            .items_center()
            .bg(bg)
            .border_t_1()
            .border_color(border)
            .child(
                div()
                    .w(px(LEFT_W))
                    .px(px(12.))
                    .child(match designer {
                        Some((ncols, _)) => div()
                            .truncate()
                            .text_xs()
                            .text_color(fg)
                            .child(format!("字段数: {ncols}")),
                        None => div()
                            .truncate()
                            .text_xs()
                            .text_color(fg)
                            .child(if has_sql { sql } else { status }),
                    }),
            )
            .child(div().w(px(1.)).h_full().bg(border))
            .when_some(designer.and_then(|(_, e)| e), |s, (label, n)| {
                s.child(
                    h_flex()
                        .flex_1()
                        .px(px(12.))
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(fg)
                                .child(format!("{label}: {n}")),
                        ),
                )
            })
            .when_some(
                designer.filter(|(_, e)| e.is_none()),
                |s, _| {
                    // 没有对应集合的子页：后面留空（面包屑只给非设计页签用）
                    s.child(div().flex_1())
                },
            )
            .when(designer.is_none() && !has_sql, |s| {
                s.child(
                    h_flex()
                        .flex_1()
                        .px(px(12.))
                        .gap(px(14.))
                        .items_center()
                        .child(self.breadcrumb_items(fg)),
                )
            })
            .when(designer.is_none() && has_sql, |s| {
                s.child(
                    h_flex()
                        .flex_1()
                        .items_center()
                        .justify_end()
                        .pr(px(10.))
                        .child(div().text_xs().text_color(muted).child(recinfo)),
                )
            })
            .child(
                h_flex()
                    .items_center()
                    .gap(px(2.))
                    .pr(px(8.))
                    .child(status_toggle(
                        "view-grid",
                        IconName::Grid3x3,
                        !self.list_view,
                        p,
                        cx,
                        ViewAct::Grid,
                    ))
                    .child(status_toggle(
                        "view-list",
                        IconName::List,
                        self.list_view,
                        p,
                        cx,
                        ViewAct::List,
                    ))
                    // Navicat Lite 的第三个视图「详细信息」是企业版专属：图标可点但点了弹升级窗，
                    // 这里按 H 节模型/BI 同款口径做成置灰占位 + Ent 角标
                    .child(
                        div()
                            .relative()
                            .mr(px(3.))
                            .child(
                                h_flex()
                                    .size(px(18.))
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(2.))
                                    .border_1()
                                    .border_color(to_hsla(p.status_bar))
                                    .child(
                                        Icon::new(IconName::Table)
                                            .with_size(px(13.))
                                            .text_color(to_hsla(p.subtle_foreground)),
                                    ),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .bottom(px(-2.))
                                    .right(px(-3.))
                                    .child(ent_badge_small()),
                            ),
                    )
                    .child(status_toggle(
                        "view-info",
                        IconName::PanelRight,
                        self.show_info,
                        p,
                        cx,
                        ViewAct::Info,
                    ))
                    .child(div().text_xs().text_color(muted).pl(px(8.)).child("v0.1")),
            )
    }
}

/// 右侧信息卡的四种内容，对应树上四类可选节点。
enum Card {
    /// 根节点 / 分组：Navicat 右侧不出卡
    None,
    Conn,
    Db(&'static str),
    Schema(&'static str),
    Obj(&'static str),
}

/// 树里一行的共用配色，外加当前选中节点。每行都要这六档色，
/// 逐个当参数传签名先爆。
struct TreeInk {
    fg: Hsla,
    muted: Hsla,
    hover: Hsla,
    focus_bg: Hsla,
    /// 搜索命中段的高亮底色（Navicat 是黄底，明暗主题各一档）
    hl: Hsla,
    selected: Act,
}

fn caret(open: bool) -> IconName {
    if open {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    }
}

#[allow(clippy::too_many_arguments)]
fn tree_row(
    id: impl Into<ElementId>,
    indent: f32,
    caret_icon: Option<IconName>,
    icon: impl Into<art::Glyph>,
    icon_color: Hsla,
    label: &str,
    action: Act,
    editor: Option<Entity<InputState>>,
    q: Option<&str>,
    ink: &TreeInk,
    cx: &mut Context<DaroApp>,
) -> Stateful<Div> {
    let icon = icon.into();
    let (fg, hover, muted, focus_bg) = (ink.fg, ink.hover, ink.muted, ink.focus_bg);
    let selected = ink.selected == action;
    let row_id: ElementId = id.into();
    // 三角要能单独点（只管折叠），所以给它一个派生 id。ElementId 的 Display
    // 对每种变体都不同，加个 ^ 后缀就不会和整行的 id 撞车。
    let caret_id = ElementId::Name(format!("{row_id}^").into());
    let caret_action = match action {
        a @ (Act::Root | Act::Group(_) | Act::Conn(_) | Act::Db(_, _) | Act::Schema(_, _, _)
        | Act::Cat(_, _, _, _) | Act::Obj(_, _, _, _, _)) => Some(a),
        Act::Col(_, _, _, _, _, _) => None,
    };
    // 搜索态 / 未连接的连接行不渲染三角，也就不留下看不见的折叠点击区
    let caret_action = if caret_icon.is_some() { caret_action } else { None };
    h_flex()
        .id(row_id)
        .h(px(ROW_H))
        .pl(px(indent))
        .pr(px(8.))
        .gap(px(4.))
        .items_center()
        .cursor_pointer()
        .when(selected, |s| s.bg(focus_bg))
        .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            // Navicat：单击选中，双击展开/收起（连接则是连接/折叠），
            // 三角只管折叠，不顺手改选中
            match action {
                Act::Root => {
                    this.collapsed_root = !this.collapsed_root;
                    this.sel = Act::Root;
                    this.save();
                }
                Act::Group(gix) => {
                    let group = this.groups[gix].clone();
                    if !this.collapsed_groups.remove(&group) {
                        this.collapsed_groups.insert(group);
                    }
                    this.sel = Act::Group(gix);
                    this.save();
                }
                Act::Conn(ix) => {
                    if this.is_double_click(action) {
                        this.toggle_connection(ix);
                    }
                    this.sel = Act::Conn(ix);
                }
                Act::Db(c, d) => {
                    if this.is_double_click(action) {
                        this.toggle_db(c, d);
                    }
                    this.sel = Act::Db(c, d);
                }
                Act::Schema(c, d, s) => {
                    if this.is_double_click(action) {
                        this.toggle_schema(c, d, s);
                    }
                    this.sel = Act::Schema(c, d, s);
                }
                Act::Cat(c, d, s, k) => {
                    if this.is_double_click(action) {
                        this.toggle_cat(c, d, s, k);
                    }
                    this.sel = Act::Cat(c, d, s, k);
                }
                Act::Obj(..) => {
                    if this.is_double_click(action) {
                        this.open_object(action);
                    }
                    this.sel = action;
                }
                // 字段行只能选中：Navicat 里双击字段不做任何事
                Act::Col(..) => {
                    this.sel = action;
                }
            }
            cx.notify();
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, _, _, cx| {
                // 右击先选中脚下这一行，菜单内容再按节点类型分叉；
                // 字段行没有菜单（实测没做到这一层，先不开空弹层）
                this.sel = action;
                if !matches!(action, Act::Col(..)) {
                    this.ctx = Some(action);
                }
                cx.notify();
            }),
        )
        .child({
            let cell = div()
                .w(px(11.))
                .flex()
                .items_center()
                .justify_center()
                .when_some(caret_icon, |s, c| {
                    s.child(Icon::new(c).with_size(px(10.)).text_color(muted))
                });
            match caret_action {
                Some(a) => cell
                    .id(caret_id)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match a {
                            Act::Root => {
                                this.collapsed_root = !this.collapsed_root;
                                this.save();
                            }
                            Act::Group(gix) => {
                                let group = this.groups[gix].clone();
                                if !this.collapsed_groups.remove(&group) {
                                    this.collapsed_groups.insert(group);
                                }
                                this.save();
                            }
                            Act::Conn(ix) => this.toggle_connection(ix),
                            Act::Db(c, d) => this.toggle_db(c, d),
                            Act::Schema(c, d, s) => this.toggle_schema(c, d, s),
                            Act::Cat(c, d, s, k) => this.toggle_cat(c, d, s, k),
                            Act::Obj(c, d, s, k, o) => this.toggle_obj(c, d, s, k, o),
                            _ => {}
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .into_any_element(),
                None => cell.into_any_element(),
            }
        })
        .child(art::icon(icon, 16., icon_color))
        .child(match editor {
            // 就地改名：编辑器占住标签位，行内其余元素（三角 / 图标）照旧
            Some(input) => div()
                .flex_1()
                .min_w_0()
                .child(
                    Input::new(&input)
                        .h(px(ROW_H))
                        .appearance(true)
                        .bordered(true)
                        .cleanable(false),
                )
                .into_any_element(),
            None => match q.and_then(|needle| find_ci(label, needle)) {
                // 搜索命中：命中段铺黄底（Navicat 浏览器式高亮）
                Some(range) => div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(fg)
                    .child(highlighted_label(label, range, ink.hl))
                    .into_any_element(),
                None => div()
                    .flex_1()
                    .truncate()
                    .text_xs()
                    .text_color(fg)
                    .child(label.to_string())
                    .into_any_element(),
            },
        })
}

/// 大小写不敏感的子串查找，返回原串里的字节区间（给命中高亮用）。
/// 按 to_lowercase 后查找；ASCII / CJK 是等长映射，字节下标可直接搬回原串。
fn find_ci(hay: &str, needle: &str) -> Option<std::ops::Range<usize>> {
    let needle = needle.to_lowercase();
    if needle.is_empty() {
        return None;
    }
    let lower = hay.to_lowercase();
    let start = lower.find(&needle)?;
    Some(start..start + needle.len())
}

fn contains_ci(hay: &str, needle: &str) -> bool {
    find_ci(hay, needle).is_some()
}

/// 命中段铺黄底的标签文本（Navicat 搜索的浏览器式高亮）。
fn highlighted_label(label: &str, range: std::ops::Range<usize>, hl: Hsla) -> StyledText {
    StyledText::new(label.to_string()).with_highlights([(
        range,
        HighlightStyle {
            background_color: Some(hl),
            ..Default::default()
        },
    )])
}

/// 树节点右键菜单的总分发。条目、顺序、分隔线都按 Navicat Lite 实测还原；
/// 未实现的动作一律置灰（不留「点了没反应」的假入口），
/// 企业版专属项带橙色「Ent」角标。
fn node_context_menu(
    me: Entity<DaroApp>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, cx| {
        let (act, dark) = (me.read(cx).ctx, me.read(cx).dark);
        match act {
            Some(Act::Conn(ix)) => conn_menu(me.clone(), ix, dark, menu, window, cx),
            Some(Act::Group(g)) => group_menu(me.clone(), g, dark, menu, window, cx),
            Some(Act::Db(c, d)) => db_menu(me.clone(), c, d, menu, window, cx),
            Some(Act::Schema(c, d, s)) => schema_menu(me.clone(), c, d, s, menu, window, cx),
            Some(Act::Cat(c, d, s, k)) => cat_menu(me.clone(), c, d, s, k, menu, window, cx),
            Some(Act::Obj(c, d, s, k, o)) => {
                obj_menu(me.clone(), c, d, s, k, o, menu, window, cx)
            }
            _ => menu,
        }
    }
}

/// 数据网格右键菜单：复制为 INSERT / UPDATE / SELECT / HTML。
/// Navicat Lite 实测口径：四项都在「复制为」子菜单下，这里直接平铺。
fn grid_context_menu(
    me: Entity<DaroApp>,
    _dark: bool,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu: PopupMenu, _window: &mut Window, _cx: &mut Context<PopupMenu>| {
        let me_insert = me.clone();
        let me_update = me.clone();
        let me_select = me.clone();
        let me_html = me.clone();
        let me_export = me.clone();
        let me_table_export = me.clone();
        menu.separator()
            .item(
                PopupMenuItem::new("复制为 INSERT")
                    .on_click(move |_, window, cx| {
                        let me = me_insert.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| this.copy_as_insert(cx));
                        });
                    }),
            )
            .item(
                PopupMenuItem::new("复制为 UPDATE")
                    .on_click(move |_, window, cx| {
                        let me = me_update.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| this.copy_as_update(cx));
                        });
                    }),
            )
            .item(
                PopupMenuItem::new("复制为 SELECT")
                    .on_click(move |_, window, cx| {
                        let me = me_select.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| this.copy_as_select(cx));
                        });
                    }),
            )
            .item(
                PopupMenuItem::new("复制为 HTML")
                    .on_click(move |_, window, cx| {
                        let me = me_html.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| this.copy_as_html(cx));
                        });
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::new("导出表数据…")
                    .on_click(move |_, window, cx| {
                        let me = me_table_export.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| this.open_table_export(cx));
                        });
                    }),
            )
            .item(
                PopupMenuItem::new("导出结果…")
                    .on_click(move |_, window, cx| {
                        let me = me_export.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| this.open_export_result(cx));
                        });
                    }),
            )
    }
}

/// 对象面板右键菜单：打开 / 设计表 / 复制名字。
/// Navicat Lite 实测口径：表节点有「打开」「设计」「复制名字」；
/// 视图 / 函数类似但「设计」改成「编辑」；非表对象没有「设计」项。
fn object_context_menu(
    me: Entity<DaroApp>,
    act: Act,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu: PopupMenu, window, cx| {
        let me_open = me.clone();
        let me_design = me.clone();
        let me_copy = me.clone();
        let me_export_table = me.clone();
        
        let mut menu = menu
            .item(
                PopupMenuItem::new("打开").on_click(move |_, window, cx| {
                    let me = me_open.clone();
                    window.defer(cx, move |_window, cx| {
                        me.update(cx, |this, cx| {
                            if let Act::Obj(c, d, s, k, o) = act {
                                this.open_object(Act::Obj(c, d, s, k, o));
                            }
                            cx.notify();
                        });
                    });
                }),
            );
        
        // 表 / 视图 / 函数才有「设计」
        if matches!(act, Act::Obj(_, _, _, 0..=3, _)) {
            menu = menu.item(
                PopupMenuItem::new("设计").on_click(move |_, window, cx| {
                    let me = me_design.clone();
                    window.defer(cx, move |window, cx| {
                        me.update(cx, |this, cx| {
                            if let Act::Obj(c, d, s, k, o) = act {
                                this.open_new_designer(Act::Obj(c, d, s, k, o), window, cx);
                            }
                        });
                    });
                }),
            );
        }
        
        menu = menu
            .separator()
            .item(
                PopupMenuItem::new("导出表数据…").on_click(move |_, window, cx| {
                    let me = me_export_table.clone();
                    window.defer(cx, move |_window, cx| {
                        me.update(cx, |this, cx| this.open_table_export(cx));
                    });
                }),
            )
            .item(
                PopupMenuItem::new("复制名字").on_click(move |_, window, cx| {
                    let me = me_copy.clone();
                    window.defer(cx, move |_window, cx| {
                        if let Act::Obj(_, _, _, _, o) = act {
                            let name = me.read(cx).pane_objects().get(o).copied().unwrap_or("");
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(name.to_string()));
                        }
                    });
                }),
            );
        
        menu
    }
}

/// 设计器网格右键菜单：插入行 / 删除行 / 复制行。
/// Navicat Lite 实测口径：字段页右键有「插入行」「删除行」「复制行」三项。
fn designer_context_menu(
    me: Entity<DaroApp>,
    did: u64,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu: PopupMenu, window, cx| {
        let me_insert = me.clone();
        let me_delete = me.clone();
        let me_copy = me.clone();
        let me_export_table = me.clone();
        
        menu.item(
            PopupMenuItem::new("插入行").on_click(move |_, window, cx| {
                let me = me_insert.clone();
                window.defer(cx, move |_window, cx| {
                    me.update(cx, |this, cx| {
                        if let Some(tab) = this.opened.iter_mut().find(|t| t.did == did) {
                            if let Some(ds) = tab.dstate.as_mut() {
                                // 在当前选中行后插入一空行
                                let insert_at = (ds.sel + 1).min(ds.cols.len());
                                ds.cols.insert(insert_at, DesignCol::default());
                                ds.edits.insert(insert_at, [const { None }; 7]);
                                ds.sel = insert_at;
                                cx.notify();
                            }
                        }
                    });
                });
            }),
        )
        .item(
            PopupMenuItem::new("删除行").on_click(move |_, window, cx| {
                let me = me_delete.clone();
                window.defer(cx, move |_window, cx| {
                    me.update(cx, |this, cx| {
                        if let Some(tab) = this.opened.iter_mut().find(|t| t.did == did) {
                            if let Some(ds) = tab.dstate.as_mut() {
                                if !ds.cols.is_empty() {
                                    let del_at = ds.sel.min(ds.cols.len() - 1);
                                    ds.cols.remove(del_at);
                                    ds.edits.remove(del_at);
                                    ds.sel = del_at.min(ds.cols.len().saturating_sub(1));
                                    cx.notify();
                                }
                            }
                        }
                    });
                });
            }),
        )
        .item(
            PopupMenuItem::new("复制行").on_click(move |_, window, cx| {
                let me = me_copy.clone();
                window.defer(cx, move |_window, cx| {
                    me.update(cx, |this, cx| {
                        if let Some(tab) = this.opened.iter_mut().find(|t| t.did == did) {
                            if let Some(ds) = tab.dstate.as_mut() {
                                if !ds.cols.is_empty() {
                                    let copy_at = ds.sel.min(ds.cols.len() - 1);
                                    let copied = ds.cols[copy_at].clone();
                                    ds.cols.insert(copy_at + 1, copied);
                                    let edits_copy = ds.edits.get(copy_at).cloned().unwrap_or([const { None }; 7]);
                                    ds.edits.insert(copy_at + 1, edits_copy);
                                    ds.sel = copy_at + 1;
                                    cx.notify();
                                }
                            }
                        }
                    });
                });
            }),
        )
    }
}

/// Navicat Lite 给企业版专属菜单项加的橙色角标。
fn ent_badge() -> impl IntoElement {
    div()
        .flex_none()
        .w(px(19.))
        .h(px(12.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.))
        .bg(to_hsla(0xf08b32))
        .child(div().text_size(px(9.)).text_color(to_hsla(0xffffff)).child("Ent"))
}

/// 状态栏 18px 视图按钮上的小号 Ent 角标（Navicat 里约压住图标下沿一半）。
fn ent_badge_small() -> impl IntoElement {
    div()
        .flex_none()
        .w(px(16.))
        .h(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.))
        .bg(to_hsla(0xf08b32))
        .child(div().text_size(px(7.5)).text_color(to_hsla(0xffffff)).child("Ent"))
}

/// 带可选「Ent」角标的菜单项。角标要排在文字前面，普通 Item 做不到，只能用 ElementItem。
fn mi(label: impl Into<SharedString>, ent: bool) -> PopupMenuItem {
    let label: SharedString = label.into();
    PopupMenuItem::element(move |_, _| {
        let label = label.clone();
        h_flex()
            .items_center()
            .gap(px(5.))
            .when(ent, |s| s.child(ent_badge()))
            .child(div().child(label))
    })
}

/// 置灰的占位项：驱动层 / 对应页面落地前一律不可点。
fn todo_item(label: impl Into<SharedString>, ent: bool) -> PopupMenuItem {
    mi(label, ent).disabled(true)
}

/// 库节点右键菜单（Navicat Lite 实测，PostgreSQL；关闭态的置灰差异同样实测）。
fn db_menu(
    me: Entity<DaroApp>,
    c: usize,
    d: usize,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (opened, is_pg) = {
        let app = me.read(cx);
        (
            app.visited_dbs.contains(&node_key(
                &app.conns[c].name,
                mock::databases(app.conns[c].kind)[d],
            )),
            matches!(app.conns[c].kind, mock::DbKind::PostgreSql),
        )
    };
    let mut menu = menu;
    let target = me.clone();
    // 第一项随库状态换字：已打开是「关闭数据库」，没打开是「打开数据库」
    let db_item = if opened { "关闭数据库" } else { "打开数据库" };
    menu = menu.item(PopupMenuItem::new(db_item).on_click(move |_, _, cx| {
        target.update(cx, |app, cx| {
            if opened {
                app.close_db(c, d);
            } else {
                app.open_db(c, d);
            }
            app.sel = Act::Db(c, d);
            cx.notify();
        });
    }));
    menu = menu.separator();
    menu = menu
        .item(todo_item("编辑数据库...", false))
        .item(todo_item("新建数据库...", false))
        .item(todo_item("删除数据库", false));
    menu = menu.separator().item(todo_item("管理扩展...", false));
    menu = menu.separator();
    // 实测：库没打开时「新建模式」是灰的
    menu = menu.item(mi("新建模式...", false).disabled(!opened));
    let target = me.clone();
    menu = menu.item(mi("新建查询", false).on_click(move |_, window, cx| {
        target.update(cx, |a, cx| a.open_query_tab(Some(c), window, cx));
    }));
    menu = menu.separator();
    menu = menu
        .item(todo_item("命令列界面...", false))
        .item(todo_item("运行 SQL 文件...", false));
    menu = if is_pg {
        // PG 实测：允许（库已允许连接所以是灰的）/ 不允许 / 分析数据库 /
        // 真空数据库▸（六个变体）/ 重建数据库索引。都是真实写库动作，
        // 驱动层落地前统一置灰。
        menu.submenu("维护", window, cx, |sub, window, cx| {
            sub.item(todo_item("允许", false))
                .item(todo_item("不允许", false))
                .separator()
                .item(todo_item("分析数据库", false))
                .submenu("真空数据库", window, cx, |v, _, _| {
                    v.item(todo_item("真空", false))
                        .item(todo_item("真空分析", false))
                        .item(todo_item("完全真空", false))
                        .item(todo_item("完全真空分析", false))
                        .item(todo_item("真空冻结", false))
                        .item(todo_item("真空冻结分析", false))
                })
                .item(todo_item("重建数据库索引", false))
        })
    } else {
        // 非 PG 的维护子菜单没有实测样本（打开那些连接要真连库），先按备份/还原占位。
        menu.submenu("维护", window, cx, |sub, _, _| {
            sub.item(todo_item("备份数据库...", false))
                .item(todo_item("还原数据库...", false))
        })
    };
    menu = menu.separator().item(todo_item("共享...", false));
    menu.separator().item(todo_item("刷新", false))
}

/// 模式节点右键菜单（Navicat Lite 实测，PostgreSQL public）。
fn schema_menu(
    me: Entity<DaroApp>,
    c: usize,
    d: usize,
    s: usize,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu;
    let target = me.clone();
    menu = menu.item(
        mi("打开模式", false).on_click(move |_, _, cx| {
            target.update(cx, |app, cx| {
                app.open_schema(c, d, s);
                app.sel = Act::Schema(c, d, s);
                cx.notify();
            });
        }),
    );
    menu = menu.separator();
    menu = menu
        .item(todo_item("编辑模式...", false))
        .item(todo_item("新建模式...", false))
        // 实测：public 的「删除模式」是灰的（系统模式不可删）
        .item(mi("删除模式", false).disabled(true));
    menu = menu.separator();
    let target = me.clone();
    menu = menu.item(mi("新建查询", false).on_click(move |_, window, cx| {
        target.update(cx, |a, cx| a.open_query_tab(Some(c), window, cx));
    }));
    menu = menu.separator();
    // 实测：模式层的「运行 SQL 文件...」本身就是灰的
    menu = menu
        .item(todo_item("命令列界面...", false))
        .item(todo_item("运行 SQL 文件...", false));
    menu = menu.submenu("转储 SQL 文件", window, cx, |sub, _, _| {
        sub.item(todo_item("结构和数据...", false))
            .item(todo_item("仅结构...", false))
    });
    menu = menu
        .item(todo_item("数据字典...", true))
        .item(todo_item("逆向模式到模型...", true));
    menu = menu.separator().item(todo_item("共享...", false));
    menu.separator().item(todo_item("刷新", false))
}

/// 分类节点（表 / 视图 / …）右键菜单，五个分类逐个按 Navicat Lite 实测还原
/// （函数 / 查询等分类在样本库里是空的，但分类节点本身右键得到）。
fn cat_menu(
    me: Entity<DaroApp>,
    c: usize,
    d: usize,
    s: usize,
    k: usize,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let kind = CATEGORIES[k].0;
    let mut menu = menu;
    match k {
        0 => {
            let me_new = me.clone();
            menu = menu.submenu(format!("新建{kind}"), window, cx, move |sub, _, _| {
                // 子项实测（对象级同款）：常规 / 外部 / 分区；只有常规接了设计器
                let me2 = me_new.clone();
                sub.item(mi("常规", false).on_click(move |_, window, cx| {
                    me2.update(cx, |app, cx| {
                        app.open_new_designer(Act::Obj(c, d, s, 0, 0), window, cx)
                    });
                }))
                .item(todo_item("外部", false))
                .item(todo_item("分区", false))
            });
            menu = menu.item(todo_item("通过 AI 新建表", true));
            menu = menu.separator();
            menu = menu
                .item(todo_item("导入向导...", false))
                .item(todo_item("导出向导...", false));
            menu = menu.separator();
            menu = menu
                .item(todo_item("数据字典...", true))
                .item(todo_item("数据生成...", true))
                .item(todo_item("数据脱敏...", true));
            menu = menu.separator();
            menu = menu
                .item(todo_item("运行 SQL 文件...", false))
                .item(todo_item("在模式中查找", true));
        }
        1 => {
            menu = menu.item(todo_item(format!("新建{kind}"), false));
            menu = menu.separator();
            menu = menu
                .item(todo_item("导出向导", false))
                .item(todo_item("数据字典...", true));
        }
        2 => {
            menu = menu.item(todo_item(format!("新建{kind}"), false));
            menu = menu.separator();
            menu = menu.item(todo_item("导出向导", false));
        }
        3 => {
            menu = menu
                .item(todo_item("新建函数", false))
                .item(todo_item("数据字典...", true));
        }
        _ => {
            let target = me.clone();
            menu = menu.item(mi("新建查询", false).on_click(move |_, window, cx| {
                target.update(cx, |a, cx| a.open_query_tab(Some(c), window, cx));
            }));
        }
    }
    menu = menu.separator();
    // 分类级「新建组」管的是对象分组，本体没实现；粘贴永远灰（无可粘贴对象）
    menu = menu
        .item(todo_item("新建组", false))
        .item(mi("粘贴", false).disabled(true));
    if k == 4 {
        menu = menu
            .separator()
            .item(todo_item("打开所在的文件夹...", false))
            .item(todo_item("打开外部查询...", false));
    }
    menu.separator().item(todo_item("刷新", false))
}

/// 对象级「管理组」子菜单：Navicat 是 新建组 / 移动到▸ / 从组中排除（不在组内时灰）。
/// 对象分组本体没实现，三个入口都置灰。
fn obj_group_submenu(sub: PopupMenu) -> PopupMenu {
    sub.item(todo_item("新建组", false))
        .item(todo_item("移动到", false))
        .item(todo_item("从组中排除", false))
}

/// 对象（表 / 视图 / …）节点右键菜单。表 / 视图逐项按 Navicat Lite 实测还原；
/// 实体化视图 / 函数 / 查询的对象菜单没有样本可量（样本库这几类是空的），
/// 按视图的骨架推断填充。
fn obj_menu(
    me: Entity<DaroApp>,
    c: usize,
    d: usize,
    s: usize,
    k: usize,
    o: usize,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let kind = CATEGORIES[k].0;
    let mut menu = menu;
    let act = Act::Obj(c, d, s, k, o);
    if k == 0 {
        let me_open = me.clone();
        menu = menu.item(mi("打开表", false).on_click(move |_, _, cx| {
            me_open.update(cx, |app, cx| {
                app.open_object(act);
                cx.notify();
            });
        }));
        menu = menu.item(todo_item("通过配置文件打开表", true));
        let me_des = me.clone();
        menu = menu.item(mi("设计表", false).on_click(move |_, window, cx| {
            me_des.update(cx, |app, cx| app.open_designer(act, window, cx));
        }));
        let me_new = me.clone();
        menu = menu.submenu("新建表", window, cx, move |sub, _, _| {
            // 子项实测：常规 / 外部 / 分区；只有常规接了设计器
            let me2 = me_new.clone();
            sub.item(mi("常规", false).on_click(move |_, window, cx| {
                me2.update(cx, |app, cx| {
                    app.open_new_designer(Act::Obj(c, d, s, 0, 0), window, cx)
                });
            }))
            .item(todo_item("外部", false))
            .item(todo_item("分区", false))
        });
        menu = menu.item(todo_item("通过 AI 新建表", true));
        menu = menu
            .item(todo_item("删除表", false))
            .item(todo_item("清空表", false))
            .item(todo_item("截断表", false));
        menu = menu.submenu("复制表", window, cx, |sub, _, _| {
            sub.item(todo_item("结构和数据", false))
                .item(todo_item("仅结构", false))
        });
        menu = menu.item(todo_item("设置权限", false));
        menu = menu.separator();
        menu = menu
            .item(todo_item("导入向导...", false))
            .item(todo_item("导出向导...", false))
            .item(todo_item("数据字典...", true))
            .item(todo_item("数据生成...", true))
            .item(todo_item("数据脱敏...", true));
        menu = menu.separator();
        menu = menu.submenu("转储 SQL 文件", window, cx, |sub, _, _| {
            sub.item(todo_item("结构和数据...", false))
                .item(todo_item("仅结构...", false))
        });
        menu = menu.submenu("维护", window, cx, |sub, window, cx| {
            sub.item(todo_item("分析表...", false))
                .submenu("真空表", window, cx, |v, _, _| {
                    v.item(todo_item("真空...", false))
                        .item(todo_item("真空分析...", false))
                        .item(todo_item("完全真空...", false))
                        .item(todo_item("完全真空分析...", false))
                        .item(todo_item("真空冻结...", false))
                        .item(todo_item("真空冻结分析...", false))
                })
                .item(todo_item("重建表索引...", false))
        });
        menu = menu
            .item(todo_item("逆向表到模型...", true))
            .item(todo_item("创建 BI 工作区...", true));
    } else {
        let me_open = me.clone();
        let open_item = if k == 1 {
            mi("打开视图", false).on_click(move |_, _, cx| {
                me_open.update(cx, |app, cx| {
                    app.open_object(act);
                    cx.notify();
                });
            })
        } else {
            // 实体化视图 / 函数 / 查询：打开行为没有实测样本，先置灰
            todo_item(format!("打开{kind}"), false)
        };
        menu = menu.item(open_item);
        menu = menu.item(todo_item(
            if k == 1 {
                "设计视图".to_string()
            } else {
                format!("设计{kind}")
            },
            false,
        ));
        menu = menu
            .item(todo_item(format!("新建{kind}"), false))
            .item(todo_item(format!("删除{kind}"), false))
            .item(todo_item("设置权限", false));
        menu = menu.separator();
        menu = menu
            .item(todo_item("导出向导", false))
            .item(todo_item("数据字典...", true))
            .item(todo_item(format!("逆向{kind}到模型..."), true))
            .item(todo_item("创建 BI 工作区...", true));
    }
    menu = menu.separator();
    menu = menu.submenu("管理组", window, cx, |sub, _, _| obj_group_submenu(sub));
    menu = menu
        .item(todo_item("复制", false))
        .item(todo_item("共享...", false));
    menu = menu.separator();
    if k == 0 {
        menu = menu
            .item(todo_item("重命名", false))
            .item(todo_item("创建打开表快捷方式...", false));
    } else if k == 1 {
        menu = menu
            .item(todo_item("重命名", false))
            .item(todo_item("创建打开视图快捷方式...", false));
    } else {
        menu = menu.item(todo_item("重命名", false));
    }
    menu.separator().item(todo_item("刷新", false))
}

/// 连接节点右键菜单，条目与分组顺序照 Navicat Lite 实测结果。
/// 还没做的功能（新建库 / 命令列 / 运行 SQL 文件 / 共享）先置灰，
/// 不留「点了没反应」的假入口。
fn conn_menu(
    me: Entity<DaroApp>,
    ix: usize,
    dark: bool,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    {
        let (name, connected, group) = {
            let app = me.read(cx);
            match app.conns.get(ix) {
                Some(conn) => (conn.name.clone(), conn.connected, conn.group.clone()),
                None => return menu,
            }
        };
        conn_menu_body(&me, ix, dark, menu, window, cx, &name, connected, &group)
    }
}

fn conn_menu_body(
    me: &Entity<DaroApp>,
    ix: usize,
    dark: bool,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    name: &str,
    connected: bool,
    group: &str,
) -> PopupMenu {
    let name = name.to_string();
    let group = group.to_string();
    let starred = me.read(cx).starred.contains(&name);
    let (name_copy, name_star, name_color, name_group) = (
        name.clone(),
        name.clone(),
        name.clone(),
        name.clone(),
    );
    let mut menu = menu;

        let target = me.clone();
        // Navicat 的第一项随连接状态换字：断开时是「打开连接」，不是灰掉的「关闭连接」
        let conn_item = if connected { "关闭连接" } else { "打开连接" };
        menu = menu.item(PopupMenuItem::new(conn_item).on_click(move |_, _, cx| {
            target.update(cx, |app, cx| {
                if connected {
                    app.close_connection(ix);
                } else {
                    app.open_connection(ix);
                }
                cx.notify();
            });
        }));
        menu = menu.submenu("切换连接配置文件", window, cx, |sub, _, _| {
            // 实测：打勾的当前配置文件之后是分隔线 + 新建入口
            sub.item(PopupMenuItem::new("主要配置文件").checked(true))
                .separator()
                .item(PopupMenuItem::new("新建连接配置文件...").disabled(true))
        });

        menu = menu.separator();

        let target = me.clone();
        menu = menu.item(PopupMenuItem::new("编辑连接...").on_click(move |_, _, cx| {
            open_connection_dialog(target.clone(), dark, ConnDialogMode::Edit(ix), cx);
        }));
        let target = me.clone();
        menu = menu.item(PopupMenuItem::new("新建连接...").on_click(move |_, _, cx| {
            open_new_connection(target.clone(), dark, cx);
        }));
        let target = me.clone();
        menu = menu.item(PopupMenuItem::new("删除连接").on_click(move |_, _, cx| {
            let target = target.clone();
            let value = name.clone();
            // 删除要弹窗二次确认（Navicat 也是独立窗口），且按名字定位：
            // 确认框开着时主窗口仍可操作，索引可能已经失效。
            open_confirm(
                target,
                dark,
                "删除连接",
                format!("确实要删除连接「{value}」吗？"),
                move |app| app.remove_connection_named(&value),
                cx,
            );
        }));
        let target = me.clone();
        menu = menu.item(PopupMenuItem::new("复制连接...").on_click(move |_, _, cx| {
            let target = target.clone();
            let src = name_copy.clone();
            let default_name = format!("{src} 副本");
            open_text_prompt(
                target,
                dark,
                "复制连接",
                "连接名",
                default_name,
                move |app, new_name| app.duplicate_connection_named(&src, new_name),
                cx,
            );
        }));

        menu = menu.separator();

        menu = menu
            .item(PopupMenuItem::new("新建数据库...").disabled(true))
            .item(PopupMenuItem::new("新建查询").on_click({
                let target = me.clone();
                move |_, window, cx| {
                    target.update(cx, |a, cx| a.open_query_tab(Some(ix), window, cx));
                }
            }))
            .item(PopupMenuItem::new("命令列界面...").disabled(true))
            .item(PopupMenuItem::new("运行 SQL 文件...").disabled(true));

        menu = menu.separator();

        let target = me.clone();
        // 实测 Navicat：已星标时菜单项是「移除星标」，不是「删除星标」
        let star_label = if starred { "移除星标" } else { "添加星标" };
        menu = menu.item(PopupMenuItem::new(star_label).on_click(move |_, _, cx| {
            target.update(cx, |app, cx| {
                app.toggle_star(&name_star);
                cx.notify();
            });
        }));

        let color_name = name_color;
        let target = me.clone();
        menu = menu.submenu_with_icon(None, "颜色", window, cx, move |sub, _, _| {
            let mut sub = sub;
            for (cix, (label, color)) in CONN_COLORS.iter().enumerate() {
                let target = target.clone();
                let color_name = color_name.clone();
                sub = sub.item(
                    PopupMenuItem::new(*label)
                        .icon(Icon::new(IconName::Circle).text_color(to_hsla(*color)))
                        .on_click(move |_, _, cx| {
                            let color_name = color_name.clone();
                            target.update(cx, |app, cx| {
                                app.set_conn_color(&color_name, Some(cix as u8));
                                cx.notify();
                            });
                        }),
                );
            }
            let target = target.clone();
            let color_name = color_name.clone();
            sub.item(PopupMenuItem::new("自定义...").disabled(true))
                .separator()
                .item(PopupMenuItem::new("无").on_click(move |_, _, cx| {
                    let color_name = color_name.clone();
                    target.update(cx, |app, cx| {
                        app.set_conn_color(&color_name, None);
                        cx.notify();
                    });
                }))
        });

        // 实测：颜色与管理组之间没有分隔线（Navicat 菜单里 颜色/管理组/共享/刷新 连续排列）
        let conn_name = name_group;
        let current_group = group.clone();
        let all_groups: Vec<String> = me.read(cx).groups.clone();
        let target = me.clone();
        menu = menu.submenu_with_icon(None, "管理组", window, cx, move |sub, window, cx| {
            let mut sub = sub;

            let target_new = target.clone();
            let conn_name_new = conn_name.clone();
            sub = sub.item(PopupMenuItem::new("新建组...").on_click(move |_, _, cx| {
                let target = target_new.clone();
                let conn_name = conn_name_new.clone();
                open_text_prompt(
                    target,
                    dark,
                    "新建组",
                    "组名",
                    String::new(),
                    move |app, group| app.add_group_with(group, &conn_name),
                    cx,
                );
            }));

            let target_move = target.clone();
            let conn_name_move = conn_name.clone();
            let skip = current_group.clone();
            let group_list = all_groups.clone();
            sub = sub.submenu("移动到", window, cx, move |m, _, _| {
                let mut m = m;
                for group in group_list.iter().filter(|g| **g != skip) {
                    let target = target_move.clone();
                    let conn_name = conn_name_move.clone();
                    let group = group.clone();
                    m = m.item(PopupMenuItem::new(group.clone()).on_click(
                        move |_, _, cx| {
                            let group = group.clone();
                            target.update(cx, |app, cx| {
                                app.move_connection_to_group(&conn_name, &group);
                                cx.notify();
                            });
                        },
                    ));
                }
                m
            });

            let target = target.clone();
            let conn_name = conn_name.clone();
            sub.item(
                PopupMenuItem::new("从组中排除")
                    .disabled(current_group.is_empty())
                    .on_click(move |_, _, cx| {
                        let conn_name = conn_name.clone();
                        target.update(cx, |app, cx| {
                            app.move_connection_to_group(&conn_name, "");
                            cx.notify();
                        });
                    }),
            )
        });

        menu.item(PopupMenuItem::new("共享...").disabled(true))
            // Navicat 里 共享 与 刷新 之间没有分隔线
            // 刷新要重查库的目录，驱动层落地前没有可重载的东西；
            // 给一个点了没反应的enabled项等于骗人，所以先灰掉。
            .item(PopupMenuItem::new("刷新").disabled(true))
}

/// 分组节点右键菜单（Navicat Lite 实测：新建连接... / 新建组 / 删除组 / 重命名 / 刷新）。
/// 新建组落成空组并就地进入改名；删除组要二次确认，组内连接回落未分组。
fn group_menu(
    me: Entity<DaroApp>,
    gix: usize,
    dark: bool,
    menu: PopupMenu,
    _window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(group) = me.read(cx).groups.get(gix).cloned() else {
        return menu;
    };
    let mut menu = menu;
    let target = me.clone();
    menu = menu.item(PopupMenuItem::new("新建连接...").on_click(move |_, _, cx| {
        open_new_connection(target.clone(), dark, cx);
    }));
    menu = menu.separator();
    let target = me.clone();
    menu = menu.item(PopupMenuItem::new("新建组").on_click(move |_, window, cx| {
        target.update(cx, |app, cx| {
            let g = app.add_group("新建组");
            app.sel = Act::Group(g);
            app.start_rename(window, cx);
            cx.notify();
        });
    }));
    let target = me.clone();
    menu = menu.item(PopupMenuItem::new("删除组").on_click(move |_, _, cx| {
        let target = target.clone();
        let value = group.clone();
        open_confirm(
            target,
            dark,
            "删除组",
            format!("确实要删除组「{value}」吗？组内连接将移出分组。"),
            move |app| app.delete_group(gix),
            cx,
        );
    }));
    let target = me.clone();
    menu = menu.item(PopupMenuItem::new("重命名").on_click(move |_, window, cx| {
        target.update(cx, |app, cx| {
            app.sel = Act::Group(gix);
            app.start_rename(window, cx);
            cx.notify();
        });
    }));
    // 树是本地即时渲染，没有可重载的东西；先灰掉，不留点了没反应的假入口
    menu.separator().item(PopupMenuItem::new("刷新").disabled(true))
}

/// 数据页工具条 / 记录导航条的纯图标按钮。`enabled: false` 时是置灰占位
/// （驱动层动作落地前不装「点了没反应」的假入口）。
fn tool_icon(id: ElementId, icon: IconName, color: Hsla, hover: Hsla, enabled: bool) -> AnyElement {
    h_flex()
        .id(id)
        .size(px(24.))
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .when(enabled, |s| s.cursor_pointer().hover(move |s| s.bg(hover)))
        .child(Icon::new(icon).with_size(px(14.)).text_color(color))
        .into_any_element()
}

fn action_item(
    id: &'static str,
    glyph: impl Into<art::Glyph>,
    tint: u32,
    label: &'static str,
    caret: bool,
    enabled: bool,
    fg: Hsla,
    hover: Hsla,
    muted: Hsla,
) -> Stateful<Div> {
    let glyph = glyph.into();
    h_flex()
        .id(id)
        .h(px(22.))
        .px(px(4.))
        .gap(px(4.))
        .items_center()
        .rounded(px(3.))
        .when(enabled, |s| s.cursor_pointer().hover(move |s| s.bg(hover)))
        .opacity(if enabled { 1. } else { 0.45 })
        .child(art::icon(glyph, 15., to_hsla(tint)))
        .child(div().text_xs().text_color(fg).child(label))
        .when(caret, move |s| {
            s.child(
                Icon::new(IconName::ChevronDown)
                    .with_size(px(9.))
                    .text_color(muted),
            )
        })
}

fn prop_block(label: &'static str, value: String, fg: Hsla, muted: Hsla) -> AnyElement {
    v_flex()
        .gap(px(1.))
        .child(div().text_size(px(13.)).text_color(muted).child(label))
        .child(div().text_xs().text_color(fg).child(value))
        .into_any_element()
}

fn source_row(glyph: impl Into<art::Glyph>, label: &str, fg: Hsla, muted: Hsla) -> impl IntoElement {
    h_flex()
        .gap(px(6.))
        .items_center()
        .child(art::icon(glyph.into(), 16., muted))
        .child(div().text_size(px(13.)).text_color(fg).child(label.to_string()))
}

fn breadcrumb(
    glyph: impl Into<art::Glyph>,
    tint: u32,
    label: &str,
    fg: Hsla,
) -> impl IntoElement {
    h_flex()
        .gap(px(5.))
        .items_center()
        .child(art::icon(glyph.into(), 14., to_hsla(tint)))
        .child(div().text_xs().text_color(fg).child(label.to_string()))
}

fn status_toggle(
    id: &'static str,
    icon: IconName,
    active: bool,
    p: Palette,
    cx: &mut Context<DaroApp>,
    act: ViewAct,
) -> impl IntoElement {
    let (muted, hover, accent, active_bg) = (
        to_hsla(p.subtle_foreground),
        to_hsla(p.hover),
        to_hsla(p.accent),
        to_hsla(p.selection),
    );
    h_flex()
        .id(id)
        .size(px(18.))
        .items_center()
        .justify_center()
        .rounded(px(2.))
        .cursor_pointer()
        .border_1()
        .border_color(if active {
            to_hsla(0x9ccbf2)
        } else {
            to_hsla(p.status_bar)
        })
        .when(active, move |s| s.bg(active_bg))
        .when(!active, move |s| s.hover(move |s| s.bg(hover)))
        .on_click(cx.listener(move |this, _, _, cx| {
            match act {
                ViewAct::Grid => this.list_view = false,
                ViewAct::List => this.list_view = true,
                ViewAct::Info => this.show_info = !this.show_info,
            }
            this.save();
            cx.notify();
        }))
        .child(
            Icon::new(icon)
                .with_size(px(13.))
                .text_color(if active { accent } else { muted }),
        )
}

// ── 设计器 headless 测试 ─────────────────────────────────────────────
// gpui-kit 测试台：真建窗口、真派发点击 / 键入；断言打在 DaroApp 的模型上，
// 元素快照只用来驱动交互，不碰像素。main.rs 是 bin，集成测试引不进来，
// 只能住 cfg(test)。用例共享进程级 env 与 Theme 全局，须 --test-threads=1。

/// 外键「注释」小窗：独立顶层窗口（参考图里它能越出主窗边界），白底 1px 边框
/// + 多行文本域 + 确定/取消。顶部细条可拖动窗口。
struct FkCommentPopup {
    parent: Entity<DaroApp>,
    main: AnyWindowHandle,
    did: u64,
    row: usize,
    text: Entity<TextareaState>,
    dark: bool,
    submitted: bool,
}

impl FkCommentPopup {
    fn new(
        parent: Entity<DaroApp>,
        main: AnyWindowHandle,
        did: u64,
        row: usize,
        initial: String,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = cx.new(|cx| TextareaState::new(window, cx).default_value(initial));
        Self {
            parent,
            main,
            did,
            row,
            text,
            dark,
            submitted: false,
        }
    }

    /// 确定：写回主窗模型 + 同步常驻输入格，然后关窗（只认第一次）
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitted {
            return;
        }
        self.submitted = true;
        let value = self.text.read(cx).value().to_string();
        let (parent, main, did, row) = (self.parent.clone(), self.main, self.did, self.row);
        let _ = main.update(cx, |_, window, cx| {
            parent.update(cx, |app, cx| app.finish_fk_comment(did, row, value, window, cx));
        });
        window.remove_window();
    }
}

/// 检查「表达式」小窗：独立顶层窗口（与外键注释小窗同款，可越出主窗边界），
/// 白底 1px 边框 + 多行文本域 + 确定/取消。
struct CheckExprPopup {
    parent: Entity<DaroApp>,
    main: AnyWindowHandle,
    did: u64,
    row: usize,
    text: Entity<TextareaState>,
    dark: bool,
    submitted: bool,
}

impl CheckExprPopup {
    fn new(
        parent: Entity<DaroApp>,
        main: AnyWindowHandle,
        did: u64,
        row: usize,
        initial: String,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = cx.new(|cx| TextareaState::new(window, cx).default_value(initial));
        Self {
            parent,
            main,
            did,
            row,
            text,
            dark,
            submitted: false,
        }
    }

    /// 确定：写回主窗模型 + 同步常驻输入格，然后关窗（只认第一次）
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitted {
            return;
        }
        self.submitted = true;
        let value = self.text.read(cx).value().to_string();
        let (parent, main, did, row) = (self.parent.clone(), self.main, self.did, self.row);
        let _ = main.update(cx, |_, window, cx| {
            parent.update(cx, |app, cx| app.finish_check_expr(did, row, value, window, cx));
        });
        window.remove_window();
    }
}

/// 排除「位置」（谓词）小窗：独立顶层窗口（与外键注释 / 检查表达式同款），
/// 白底 1px 边框 + 多行文本域 + 确定/取消。
struct ExcludePredicatePopup {
    parent: Entity<DaroApp>,
    main: AnyWindowHandle,
    did: u64,
    row: usize,
    text: Entity<TextareaState>,
    dark: bool,
    submitted: bool,
}

impl ExcludePredicatePopup {
    fn new(
        parent: Entity<DaroApp>,
        main: AnyWindowHandle,
        did: u64,
        row: usize,
        initial: String,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = cx.new(|cx| TextareaState::new(window, cx).default_value(initial));
        Self {
            parent,
            main,
            did,
            row,
            text,
            dark,
            submitted: false,
        }
    }

    /// 确定：写回主窗模型，然后关窗（只认第一次）
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitted {
            return;
        }
        self.submitted = true;
        let value = self.text.read(cx).value().to_string();
        let (parent, main, did, row) = (self.parent.clone(), self.main, self.did, self.row);
        let _ = main.update(cx, |_, window, cx| {
            parent.update(cx, |app, cx| app.finish_exclude_predicate(did, row, value, cx));
        });
        window.remove_window();
    }
}

/// 规则「位置（条件）/ 定义（语句）」小窗：独立顶层窗口（field 0 = 条件、
/// 1 = 语句），白底 1px 边框 + 多行文本域 + 确定/取消。
struct RuleTextPopup {
    parent: Entity<DaroApp>,
    main: AnyWindowHandle,
    did: u64,
    row: usize,
    field: u8,
    text: Entity<TextareaState>,
    dark: bool,
    submitted: bool,
}

impl RuleTextPopup {
    #[allow(clippy::too_many_arguments)]
    fn new(
        parent: Entity<DaroApp>,
        main: AnyWindowHandle,
        did: u64,
        row: usize,
        field: u8,
        initial: String,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = cx.new(|cx| TextareaState::new(window, cx).default_value(initial));
        Self {
            parent,
            main,
            did,
            row,
            field,
            text,
            dark,
            submitted: false,
        }
    }

    /// 确定：按 field 写回主窗模型，然后关窗（只认第一次）
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitted {
            return;
        }
        self.submitted = true;
        let value = self.text.read(cx).value().to_string();
        let (parent, main, did, row, field) =
            (self.parent.clone(), self.main, self.did, self.row, self.field);
        let _ = main.update(cx, |_, window, cx| {
            parent.update(cx, |app, cx| {
                app.finish_rule_text(did, row, field, value, cx)
            });
        });
        window.remove_window();
    }
}

impl Render for RuleTextPopup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::of(self.dark);
        let (border, bg) = (to_hsla(p.border), to_hsla(p.background));
        let state = self.text.clone();
        v_flex()
            .size_full()
            .bg(bg)
            .border_1()
            .border_color(border)
            .child(div().h(px(4.)).flex_none())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(4.))
                    .child(
                        Textarea::new(&state)
                            .appearance(false)
                            .bordered(false)
                            .h_full()
                            .text_size(px(12.)),
                    ),
            )
            .child(
                h_flex()
                    .h(px(40.))
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .px(px(8.))
                    .gap(px(6.))
                    .border_t_1()
                    .border_color(border)
                    .child(
                        ui::primary_button("rule-text-ok", "确定", self.dark, cx).on_click(
                            cx.listener(move |this, _, window, cx| this.commit(window, cx)),
                        ),
                    )
                    .child(
                        ui::secondary_button("rule-text-cancel", "取消").on_click(cx.listener(
                            move |this, _, window, _| {
                                this.submitted = true;
                                window.remove_window();
                            },
                        )),
                    ),
            )
            .into_any_element()
    }
}

impl Render for ExcludePredicatePopup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::of(self.dark);
        let (border, bg) = (to_hsla(p.border), to_hsla(p.background));
        let state = self.text.clone();
        v_flex()
            .size_full()
            .bg(bg)
            .border_1()
            .border_color(border)
            .child(div().h(px(4.)).flex_none())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(4.))
                    .child(
                        Textarea::new(&state)
                            .appearance(false)
                            .bordered(false)
                            .h_full()
                            .text_size(px(12.)),
                    ),
            )
            .child(
                h_flex()
                    .h(px(40.))
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .px(px(8.))
                    .gap(px(6.))
                    .border_t_1()
                    .border_color(border)
                    .child(
                        ui::primary_button("dex-pred-ok", "确定", self.dark, cx).on_click(
                            cx.listener(move |this, _, window, cx| this.commit(window, cx)),
                        ),
                    )
                    .child(
                        ui::secondary_button("dex-pred-cancel", "取消").on_click(cx.listener(
                            move |this, _, window, _| {
                                this.submitted = true;
                                window.remove_window();
                            },
                        )),
                    ),
            )
            .into_any_element()
    }
}

impl Render for CheckExprPopup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::of(self.dark);
        let (border, bg) = (to_hsla(p.border), to_hsla(p.background));
        let state = self.text.clone();
        v_flex()
            .size_full()
            .bg(bg)
            .border_1()
            .border_color(border)
            .child(div().h(px(4.)).flex_none())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(4.))
                    .child(
                        Textarea::new(&state)
                            .appearance(false)
                            .bordered(false)
                            .h_full()
                            .text_size(px(12.)),
                    ),
            )
            .child(
                h_flex()
                    .h(px(40.))
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .px(px(8.))
                    .gap(px(6.))
                    .border_t_1()
                    .border_color(border)
                    .child(
                        ui::primary_button("ck-expr-ok", "确定", self.dark, cx).on_click(
                            cx.listener(move |this, _, window, cx| this.commit(window, cx)),
                        ),
                    )
                    .child(
                        ui::secondary_button("ck-expr-cancel", "取消").on_click(cx.listener(
                            move |this, _, window, _| {
                                this.submitted = true;
                                window.remove_window();
                            },
                        )),
                    ),
            )
            .into_any_element()
    }
}

impl Render for FkCommentPopup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::of(self.dark);
        let (fg, border, bg) = (
            to_hsla(p.foreground),
            to_hsla(p.border),
            to_hsla(p.background),
        );
        let state = self.text.clone();
        v_flex()
            .size_full()
            .bg(bg)
            .border_1()
            .border_color(border)
            .child(div().h(px(4.)).flex_none())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(4.))
                    .child(
                        Textarea::new(&state)
                            .appearance(false)
                            .bordered(false)
                            .h_full()
                            .text_size(px(12.)),
                    ),
            )
            .child(
                h_flex()
                    .h(px(40.))
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .px(px(8.))
                    .gap(px(6.))
                    .border_t_1()
                    .border_color(border)
                    .child(
                        ui::primary_button("fk-comment-ok", "确定", self.dark, cx).on_click(
                            cx.listener(move |this, _, window, cx| this.commit(window, cx)),
                        ),
                    )
                    .child(
                        ui::secondary_button("fk-comment-cancel", "取消").on_click(cx.listener(
                            move |this, _, window, _| {
                                this.submitted = true;
                                window.remove_window();
                            },
                        )),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod designer_tests {
    use super::*;
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt;

    // 显式引回标准 test 宏：`use super::*` 会把 gpui 的同名 test 宏带进作用域，
    // 不然 #[gpui_kit::test] 生成的内部 #[test] 会再次命中 gpui 宏 → 无限递归
    use core::prelude::v1::test;

    /// 每个用例独立数据目录：隔离 connections.json，也隔离用例间状态泄漏。
    fn isolate_store(tag: &str) {
        let dir =
            std::env::temp_dir().join(format!("daro_rs_test_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: 用例由 --test-threads=1 串行执行，且写 env 先于任何 DaroApp 构造
        unsafe { std::env::set_var("DARO_DATA_DIR", &dir) };
    }

    fn open_app(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DaroApp>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(1440.), px(900.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| DaroApp::new(window, cx)),
            )
            .expect("open test window")
        })
    }

    #[test]
    fn design_table_options_and_comment_per_dialect() {
        let cols = vec![
            DesignCol { name: "id".into(), ty: "int".into(), pk: true, ..Default::default() },
        ];
        let opts = DesignOptions {
            comment: "用户 表".into(),
            fillfactor: "70".into(),
            tablespace: "fast".into(),
            ..Default::default()
        };
        let mysql = design_create_sql(
            mock::DbKind::MySql,
            "users",
            &cols,
            &[],
            &[],
            &[],
            &[],
            &opts,
            &[],
            &[],
        );
        assert!(mysql.contains(r#") COMMENT='用户 表';"#), "{mysql}");
        assert!(!mysql.contains("fillfactor"), "{mysql}");

        let pg = design_create_sql(
            mock::DbKind::PostgreSql,
            "users",
            &cols,
            &[],
            &[],
            &[],
            &[],
            &opts,
            &[],
            &[],
        );
        assert!(pg.contains(r#") WITH (fillfactor = 70) TABLESPACE fast;"#), "{pg}");
        assert!(
            pg.contains(r#"COMMENT ON TABLE "users" IS '用户 表';"#),
            "{pg}"
        );

        let ss = design_create_sql(
            mock::DbKind::SqlServer,
            "users",
            &cols,
            &[],
            &[],
            &[],
            &[],
            &opts,
            &[],
            &[],
        );
        assert!(ss.contains(r#") WITH (FILLFACTOR = 70);"#), "{ss}");
        assert!(!ss.contains("COMMENT ON"), "{ss}");

        // 单引号双写
        let quoted = DesignOptions {
            comment: "it's".into(),
            ..Default::default()
        };
        let sql = design_create_sql(
            mock::DbKind::MySql,
            "users",
            &cols,
            &[],
            &[],
            &[],
            &[],
            &quoted,
            &[],
            &[],
        );
        assert!(sql.contains(r#"COMMENT='it''s'"#), "{sql}");

        // 填充因子校验
        assert!(design_options_validation(&DesignOptions {
            fillfactor: "abc".into(),
            ..Default::default()
        })
        .is_some());
        assert!(design_options_validation(&DesignOptions {
            fillfactor: "120".into(),
            ..Default::default()
        })
        .is_some());
        assert!(design_options_validation(&DesignOptions {
            fillfactor: "70".into(),
            ..Default::default()
        })
        .is_none());
    }

    #[test]
    fn design_view_sql_uses_body() {
        let sql = design_view_sql("视图", "v_users", "SELECT id, name FROM users");
        assert_eq!(sql, "CREATE 视图 v_users AS\nSELECT id, name FROM users;\n");
        let empty = design_view_sql("视图", "v_users", "   ");
        assert!(empty.contains("CREATE 视图 v_users AS …"));
    }

    #[test]
    fn design_triggers_per_dialect() {
        let t = DesignTrigger {
            name: "trg".into(),
            timing: "BEFORE".into(),
            events: "INSERT, UPDATE".into(),
            for_each: "行".into(),
            function: "audit_fn".into(),
            body: "SET NEW.x = 1".into(),
            ..Default::default()
        };

        // MySQL：一个触发器只能挂一个事件 → 按事件拆成两条，名字带后缀
        let my = design_triggers_sql(mock::DbKind::MySql, "t", &[t.clone()]);
        assert_eq!(my.len(), 2, "{my:?}");
        assert!(
            my[0].contains("CREATE TRIGGER `trg_insert` BEFORE INSERT ON `t` FOR EACH ROW SET NEW.x = 1;"),
            "{}",
            my[0]
        );
        assert!(my[1].contains("`trg_update`"), "{}", my[1]);

        // PG：单条，事件用 OR 连，引用函数
        let pg = design_triggers_sql(mock::DbKind::PostgreSql, "t", &[t.clone()]);
        assert_eq!(pg.len(), 1, "{pg:?}");
        assert!(
            pg[0].contains(
                r#"CREATE TRIGGER "trg" BEFORE INSERT OR UPDATE ON "t" FOR EACH ROW EXECUTE FUNCTION audit_fn();"#
            ),
            "{}",
            pg[0]
        );

        // SQL Server：语句级、事件逗号连、内联主体
        let ss = design_triggers_sql(mock::DbKind::SqlServer, "t", &[t.clone()]);
        assert_eq!(ss.len(), 1, "{ss:?}");
        assert!(
            ss[0].contains(r#"CREATE TRIGGER "trg" ON "t" AFTER INSERT, UPDATE AS SET NEW.x = 1;"#),
            "{}",
            ss[0]
        );

        // 方言门控
        let stmt_level = DesignTrigger { for_each: "语句".into(), ..t.clone() };
        assert!(design_triggers_gate(&[stmt_level], mock::DbKind::MySql).is_some());
        let before_ss = DesignTrigger { timing: "BEFORE".into(), ..t.clone() };
        assert!(design_triggers_gate(&[before_ss], mock::DbKind::SqlServer).is_some());

        // 校验：PG 要函数、MySQL 要主体
        let pg_missing = DesignTrigger { function: String::new(), ..t.clone() };
        assert!(design_triggers_validation(&[pg_missing], mock::DbKind::PostgreSql).is_some());
        let my_missing = DesignTrigger { body: String::new(), ..t.clone() };
        assert!(design_triggers_validation(&[my_missing], mock::DbKind::MySql).is_some());
        assert!(design_triggers_validation(&[t], mock::DbKind::PostgreSql).is_none());
    }

    #[test]
    fn design_rules_generate_pg_only() {
        let cols = vec![
            DesignCol { name: "id".into(), ty: "int".into(), pk: true, ..Default::default() },
        ];
        let rule = DesignRule {
            name: "r1".into(),
            event: "UPDATE".into(),
            qualification: "old.id > 0".into(),
            action: "INSTEAD".into(),
            statement: "INSERT INTO log VALUES (1)".into(),
            ..Default::default()
        };
        let pg = design_create_sql(
            mock::DbKind::PostgreSql,
            "t",
            &cols,
            &[],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[rule.clone()],
        );
        assert!(
            pg.contains(
                r#"CREATE RULE "r1" AS ON UPDATE WHERE (old.id > 0) DO INSTEAD INSERT INTO log VALUES (1);"#
            ),
            "{pg}"
        );
        let my = design_create_sql(
            mock::DbKind::MySql,
            "t",
            &cols,
            &[],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[rule],
        );
        assert!(!my.contains("CREATE RULE"), "{my}");

        let nothing = DesignRule {
            name: "r2".into(),
            event: "DELETE".into(),
            action: "NOTHING".into(),
            ..Default::default()
        };
        let pg2 = design_create_sql(
            mock::DbKind::PostgreSql,
            "t",
            &cols,
            &[],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[nothing],
        );
        assert!(pg2.contains("DO NOTHING;"), "{pg2}");

        assert!(
            design_rules_gate(
                &[DesignRule { name: "r".into(), ..Default::default() }],
                mock::DbKind::MySql
            )
            .is_some()
        );
        assert!(
            design_rules_gate(&[DesignRule::default()], mock::DbKind::MySql).is_none(),
            "空白种子行不算规则"
        );
        assert!(design_rules_gate(&[], mock::DbKind::MySql).is_none());
        let missing = vec![DesignRule {
            name: "x".into(),
            event: "INSERT".into(),
            action: "ALSO".into(),
            ..Default::default()
        }];
        assert_eq!(
            design_rules_validation(&missing).as_deref(),
            Some("规则「x」的语句不能为空")
        );
        let dup = vec![
            DesignRule { name: "x".into(), event: "INSERT".into(), action: "NOTHING".into(), ..Default::default() },
            DesignRule { name: "x".into(), event: "DELETE".into(), action: "NOTHING".into(), ..Default::default() },
        ];
        assert_eq!(
            design_rules_validation(&dup).as_deref(),
            Some("规则名「x」重复")
        );
    }

    #[test]
    fn design_excludes_pg_only_and_validate() {
        let cols = vec![
            DesignCol { name: "id".into(), ty: "int".into(), pk: true, ..Default::default() },
        ];
        let ex = DesignExclude {
            name: "ex_room".into(),
            elements: "room WITH =".into(),
            method: "gist".into(),
            predicate: "state = 'active'".into(),
            ..Default::default()
        };
        let pg = design_create_sql(
            mock::DbKind::PostgreSql,
            "booking",
            &cols,
            &[],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[ex.clone()],
            &[],
        );
        assert!(
            pg.contains(r#"CONSTRAINT "ex_room" EXCLUDE USING gist (room WITH =) WHERE (state = 'active')"#),
            "{pg}"
        );
        let my = design_create_sql(
            mock::DbKind::MySql,
            "booking",
            &cols,
            &[],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[ex],
            &[],
        );
        assert!(!my.contains("EXCLUDE"), "{my}");

        // 方言门控 + 必填/去重校验
        assert!(
            design_excludes_gate(
                &[DesignExclude { name: "e".into(), ..Default::default() }],
                mock::DbKind::MySql
            )
            .is_some()
        );
        assert!(
            design_excludes_gate(&[DesignExclude::default()], mock::DbKind::MySql).is_none(),
            "空白种子行不算排除约束"
        );
        assert!(design_excludes_gate(&[], mock::DbKind::MySql).is_none());
        assert!(design_excludes_gate(&[DesignExclude::default()], mock::DbKind::PostgreSql).is_none());
        let empty = vec![DesignExclude { name: "e".into(), ..Default::default() }];
        assert_eq!(
            design_excludes_validation(&empty).as_deref(),
            Some("排除约束「e」的元素不能为空")
        );
        let dup = vec![
            DesignExclude { name: "e".into(), elements: "a WITH =".into(), ..Default::default() },
            DesignExclude { name: "e".into(), elements: "b WITH =".into(), ..Default::default() },
        ];
        assert_eq!(
            design_excludes_validation(&dup).as_deref(),
            Some("排除约束名「e」重复")
        );
    }

    #[test]
    fn design_unique_and_check_generate_constraints() {
        let cols = vec![DesignCol { name: "id".into(), ty: "int".into(), pk: true, ..Default::default() }];
        let uk = DesignPair { name: "uk_name".into(), value: "name, org_id".into(), ..Default::default() };
        let ck = DesignPair { name: "ck_age".into(), value: "age > 0".into(), ..Default::default() };
        let mysql = design_create_sql(
            mock::DbKind::MySql,
            "users",
            &cols,
            &[],
            &[],
            &[uk.clone()],
            &[ck.clone()],
            &DesignOptions::default(),
            &[],
            &[],
        );
        assert!(mysql.contains("CONSTRAINT `uk_name` UNIQUE (`name`, `org_id`)"));
        assert!(mysql.contains("CONSTRAINT ck_age CHECK (age > 0)"));
        let pg = design_create_sql(
            mock::DbKind::PostgreSql,
            "users",
            &cols,
            &[],
            &[],
            &[uk],
            &[ck],
            &DesignOptions::default(),
            &[],
            &[],
        );
        assert!(pg.contains("CONSTRAINT \"uk_name\" UNIQUE (\"name\", \"org_id\")"));

        let empty = vec![DesignPair { name: "u".into(), value: String::new(), ..Default::default() }];
        assert_eq!(
            design_validation(&cols, &[], &[], &empty, &[]).as_deref(),
            Some("唯一键「u」的内容不能为空")
        );
        let dup = vec![
            DesignPair { name: "c".into(), value: "a > 0".into(), ..Default::default() },
            DesignPair { name: "c".into(), value: "b > 0".into(), ..Default::default() },
        ];
        assert_eq!(
            design_validation(&cols, &[], &[], &[], &dup).as_deref(),
            Some("检查名「c」重复")
        );
    }

    #[test]
    fn design_fks_generate_constraint_and_validate() {
        let cols = vec![
            DesignCol { name: "id".into(), ty: "int".into(), pk: true, ..Default::default() },
            DesignCol { name: "org_id".into(), ty: "int".into(), ..Default::default() },
        ];
        let fk = DesignForeignKey {
            name: "fk_org".into(),
            columns: "org_id".into(),
            ref_table: "org".into(),
            ref_columns: "id".into(),
            on_delete: "CASCADE".into(),
            on_update: "NO ACTION".into(),
            ..Default::default()
        };
        let mysql = design_create_sql(
            mock::DbKind::MySql,
            "users",
            &cols,
            &[],
            &[fk.clone()],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[],
        );
        assert!(mysql.contains(
            "CONSTRAINT `fk_org` FOREIGN KEY (`org_id`) REFERENCES `org` (`id`) ON DELETE CASCADE"
        ));
        // NO ACTION 不落进 DDL
        assert!(!mysql.contains("ON UPDATE"));
        let pg = design_create_sql(
            mock::DbKind::PostgreSql,
            "users",
            &cols,
            &[],
            &[fk],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[],
        );
        assert!(pg.contains(
            "CONSTRAINT \"fk_org\" FOREIGN KEY (\"org_id\") REFERENCES \"org\" (\"id\") ON DELETE CASCADE"
        ));

        // 校验：四个必填项 + 名称重复
        let missing_cols = vec![DesignForeignKey { name: "fk".into(), ..Default::default() }];
        assert_eq!(
            design_validation(&cols, &[], &missing_cols, &[], &[]).as_deref(),
            Some("外键「fk」的字段不能为空")
        );
        let missing_ref = vec![DesignForeignKey {
            name: "fk".into(),
            columns: "org_id".into(),
            ..Default::default()
        }];
        assert_eq!(
            design_validation(&cols, &[], &missing_ref, &[], &[]).as_deref(),
            Some("外键「fk」的引用表不能为空")
        );
        let dup = vec![
            DesignForeignKey { name: "fk".into(), columns: "a".into(), ref_table: "t".into(), ref_columns: "id".into(), ..Default::default() },
            DesignForeignKey { name: "fk".into(), columns: "b".into(), ref_table: "t".into(), ref_columns: "id".into(), ..Default::default() },
        ];
        assert_eq!(
            design_validation(&cols, &[], &dup, &[], &[]).as_deref(),
            Some("外键名「fk」重复")
        );
    }

    #[test]
    fn design_indexes_generate_create_index_and_validate() {
        let cols = vec![
            DesignCol { name: "id".into(), ty: "int8".into(), pk: true, ..Default::default() },
            DesignCol { name: "org_id".into(), ty: "int8".into(), ..Default::default() },
        ];
        let idx = DesignIndex {
            name: "idx_org".into(),
            fields: vec![
                IndexField::make("org_id", "int8"),
                IndexField::make("id", "int8"),
            ],
            method: String::new(),
            unique: true,
            ..Default::default()
        };
        let mysql = design_create_sql(
            mock::DbKind::MySql,
            "users",
            &cols,
            &[idx.clone()],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[],
        );
        assert!(mysql.contains("CREATE UNIQUE INDEX `idx_org` ON `users` (`org_id`, `id`);"));
        let pg = design_create_sql(
            mock::DbKind::PostgreSql,
            "users",
            &cols,
            &[idx.clone()],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[],
        );
        assert!(pg.contains("CREATE UNIQUE INDEX \"idx_org\" ON \"users\" (\"org_id\", \"id\");"));

        // 方法非空 → USING <method>
        let with_method = DesignIndex { method: "BTREE".into(), ..idx.clone() };
        let sql = design_create_sql(
            mock::DbKind::MySql,
            "users",
            &cols,
            &[with_method],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[],
        );
        assert!(sql.contains(" USING BTREE ("));

        // 校验：名称/字段为空、名称重复都要拦住
        // 名称空但有字段 → 不是空白行，照样拦住
        let empty_name = vec![DesignIndex {
            name: " ".into(),
            fields: vec![IndexField::make("id", "int8")],
            ..Default::default()
        }];
        assert_eq!(
            design_validation(&cols, &empty_name, &[], &[], &[]).as_deref(),
            Some("第 1 个索引的名称不能为空")
        );
        let empty_cols = vec![DesignIndex { name: "i1".into(), ..Default::default() }];
        assert_eq!(
            design_validation(&cols, &empty_cols, &[], &[], &[]).as_deref(),
            Some("索引「i1」的字段不能为空")
        );
        let dup = vec![
            DesignIndex {
                name: "i1".into(),
                fields: vec![IndexField::make("id", "int8")],
                ..Default::default()
            },
            DesignIndex {
                name: "i1".into(),
                fields: vec![IndexField::make("org_id", "int8")],
                ..Default::default()
            },
        ];
        assert_eq!(design_validation(&cols, &dup, &[], &[], &[]).as_deref(), Some("索引名「i1」重复"));
        assert_eq!(design_validation(&cols, &[idx.clone()], &[], &[], &[]), None);

        // 索引属性：填充因子 / 待处理列表限制 / 每范围页数要能变成 DDL
        let opts = DesignIndex {
            fillfactor: "70".into(),
            tablespace: "pg_default".into(),
            ..idx.clone()
        };
        let pg = design_create_sql(
            mock::DbKind::PostgreSql,
            "users",
            &cols,
            &[opts],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[],
        );
        assert!(
            pg.contains(" WITH (fillfactor = 70) TABLESPACE pg_default;"),
            "{pg}"
        );

        // 并发 / 注释 / DESC 排序 / 运算符类别
        let mut fancy = DesignIndex {
            concurrent: true,
            comment: "it's fast".into(),
            ..idx.clone()
        };
        fancy.fields[0].sort = "DESC".into();
        fancy.fields[0].nulls = "NULLS LAST".into();
        fancy.fields[1].opclass = "text_ops".into();
        let pg = design_create_sql(
            mock::DbKind::PostgreSql,
            "users",
            &cols,
            &[fancy],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[],
        );
        assert!(pg.contains("CREATE UNIQUE INDEX CONCURRENTLY \"idx_org\""), "{pg}");
        assert!(pg.contains("\"org_id\" DESC NULLS LAST"), "{pg}");
        // int8 的默认运算符类别就是 int8_ops，不该写进 DDL；手改的 text_ops 要写
        assert!(!pg.contains("int8_ops"), "{pg}");
        assert!(pg.contains("\"id\" text_ops"), "{pg}");
        assert!(pg.contains("COMMENT ON INDEX \"idx_org\" IS 'it''s fast';"), "{pg}");

        // 填充因子越界要拦住
        let bad_ff = vec![DesignIndex {
            name: "i1".into(),
            fields: vec![IndexField::make("id", "int8")],
            fillfactor: "120".into(),
            ..Default::default()
        }];
        assert_eq!(
            design_validation(&cols, &bad_ff, &[], &[], &[]).as_deref(),
            Some("索引「i1」的填充因子必须是 0..100 的整数")
        );
    }

    #[test]
    fn design_pure_functions() {
        let mut c = DesignCol {
            name: "a".into(),
            ty: "int8".into(),
            len: String::new(),
            ..Default::default()
        };
        assert_eq!(design_full_type(&c), "int8");
        c.len = "4".into();
        assert_eq!(design_full_type(&c), "int8(4)");
        c.dec = "2".into();
        assert_eq!(design_full_type(&c), "int8(4,2)");
        c.ty = String::new();
        assert_eq!(design_full_type(&c), "");

        // mock 档：11 列，id 主键自增非空，name varchar(32) 非空带注释
        let cols = mock_design_cols("t");
        assert_eq!(cols.len(), 11);
        let id = &cols[0];
        assert_eq!(id.name, "id");
        assert!(id.pk && id.auto_inc && id.not_null);
        assert_eq!(id.ty, "int8");
        let name = cols.iter().find(|c| c.name == "name").unwrap();
        assert_eq!((name.ty.as_str(), name.len.as_str()), ("varchar", "32"));
        assert!(name.not_null && !name.pk);
        assert_eq!(name.comment, "名称");

        assert_eq!(
            design_validation(&[DesignCol {
                name: "  ".into(),
                ..Default::default()
            }], &[], &[], &[], &[])
            .as_deref(),
            Some("第 1 行的列名不能为空")
        );
        let dup = vec![
            DesignCol {
                name: "a".into(),
                ..Default::default()
            },
            DesignCol {
                name: "a".into(),
                ..Default::default()
            },
        ];
        assert_eq!(design_validation(&dup, &[], &[], &[], &[]).as_deref(), Some("列名「a」重复"));
        assert_eq!(design_validation(&cols, &[], &[], &[], &[]), None);

        // MySQL：反引号 + AUTO_INCREMENT + 行内 COMMENT + PRIMARY KEY
        let mysql = design_create_sql(
            mock::DbKind::MySql,
            "users",
            &cols,
            &[],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[],
        );
        assert!(mysql.starts_with("CREATE TABLE `users` ("), "{mysql}");
        assert!(
            mysql.contains("    `id` int8 NOT NULL AUTO_INCREMENT"),
            "{mysql}"
        );
        assert!(
            mysql.contains("    `name` varchar(32) NOT NULL COMMENT '名称'"),
            "{mysql}"
        );
        assert!(mysql.contains("    PRIMARY KEY (`id`)"), "{mysql}");
        assert!(!mysql.contains("COMMENT ON"), "{mysql}");

        // PG：双引号 + GENERATED AS IDENTITY + 尾注 COMMENT ON；AUTO_INCREMENT 不出
        let mut pgcols = cols.clone();
        pgcols[0].identity = "ALWAYS".into();
        pgcols[1].auto_inc = true;
        let pg = design_create_sql(
            mock::DbKind::PostgreSql,
            "users",
            &pgcols,
            &[],
            &[],
            &[],
            &[],
            &DesignOptions::default(),
            &[],
            &[],
        );
        assert!(pg.starts_with("CREATE TABLE \"users\" ("), "{pg}");
        assert!(
            pg.contains("    \"id\" int8 NOT NULL GENERATED ALWAYS AS IDENTITY"),
            "{pg}"
        );
        assert!(!pg.contains("AUTO_INCREMENT"), "{pg}");
        assert!(pg.contains("    PRIMARY KEY (\"id\")"), "{pg}");
        assert!(
            pg.ends_with("COMMENT ON COLUMN \"users\".\"name\" IS '名称';"),
            "{pg}"
        );
    }

    #[gpui_kit::test]
    fn designer_opens_and_renders(cx: &mut TestAppContext) {
        isolate_store("open");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("act-design").is_some(), "动作条设计入口");
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);

            for i in 0..11 {
                assert!(window.try_find(format!("dsub/{i}")).is_some(), "dsub/{i}");
            }
            for id in ["dsave", "dadd", "ddel", "dkey", "dprops"] {
                assert!(window.try_find(id).is_some(), "{id}");
            }
            assert!(window.try_find("dcell/0/0").is_some());
            assert!(window.try_find("dcell/10/6").is_some());
            assert!(window.try_find("dcell/11/0").is_none(), "只有 11 行");

            app.update(cx, |a, _| {
                assert_eq!(a.opened.len(), 1);
                let t = &a.opened[0];
                assert!(t.design && t.did != 0);
                let ds = t.dstate.as_ref().unwrap();
                assert_eq!(ds.cols.len(), 11);
                assert_eq!(ds.edits.len(), 11, "常驻编辑器随行建齐");
                assert_eq!(ds.cols, ds.baseline, "初始无变更");
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn designer_entry_via_action_bar_dedups(cx: &mut TestAppContext) {
        isolate_store("entry");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("act-design", cx);
            window.render_frame(cx);
            app.update(cx, |a, _| {
                assert_eq!(a.opened.len(), 1);
                assert_eq!(a.active_tab, 1, "设计页签激活");
                assert!(a.opened[0].design);
            });
            // 重复打开同一张表：去重回同一页签，不新开
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            app.update(cx, |a, _| {
                assert_eq!(a.opened.len(), 1);
                assert_eq!(a.active_tab, 1);
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn designer_click_type_save(cx: &mut TestAppContext) {
        isolate_store("edit");
        let (handle, app) = open_app(cx);
        // 点格 → 选中态（on_click 同步）+ 编辑器聚焦 + 键入
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);

            // 点注释格（行 6 flags，注释为空）→ 锚点与当前列跟进
            window.click("dcell/6/6", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!((ds.sel, ds.cur), (6, 6));
            });

            // 格内常驻编辑器跟着拿焦点；万一没中就直接点编辑器
            if window.find("dinput/6/3").focused() != Some(true) {
                window.click("dinput/6/3", cx);
            }
            assert_eq!(window.find("dinput/6/3").focused(), Some(true), "点格聚焦编辑器");
            window.input("cmt", cx);
            assert_eq!(window.find("dinput/6/3").value(), Some("cmt"));
        })
        .unwrap();

        // 订阅回调走 effect 队列，冲刷发生在 update 边界 —— 模型断言另起一轮
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.cols[6].comment, "cmt");
                assert_ne!(ds.cols, ds.baseline);
                assert!(ds.note.is_none());
            });

            // 保存：基线换工作集，出提示（on_click 同步）
            window.click("dsave", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.cols, ds.baseline);
                assert_eq!(ds.note, Some("已保存（演示数据，不落库）"));
            });
        })
        .unwrap();

        // 全选替换类型列（f=6 写回分支）：int8 → int4
        cx.update_window(handle, |_, window, cx| {
            window.click("dcell/0/1", cx);
            window.click("dinput/0/6", cx);
            assert_eq!(window.find("dinput/0/6").value(), Some("int8"));
            window.press("ctrl-a", cx);
            window.input("int4", cx);
            assert_eq!(window.find("dinput/0/6").value(), Some("int4"));
        })
        .unwrap();
        cx.update_window(handle, |_, _, cx| {
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.cols[0].ty, "int4");
                assert!(ds.note.is_none(), "一动模型提示即撤");
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn designer_add_del_pk_notnull_autoinc(cx: &mut TestAppContext) {
        isolate_store("ops");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);

            window.click("dadd", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.cols.len(), 12);
                assert_eq!(ds.sel, 11, "锚点跳到新行");
                assert_eq!(ds.edits.len(), 12, "新行编辑器补齐");
            });

            // 空列名 → 校验错 → 保存键无 on_click：点了不动基线
            window.render_frame(cx);
            window.click("dsave", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.cols.len(), 12);
                assert_eq!(ds.baseline.len(), 11, "保存键灭：基线不动");
            });

            // 补列名 → 勾主键 / 不是 null / 自增
            window.click("dcell/11/0", cx);
            if window.find("dinput/11/0").focused() != Some(true) {
                window.click("dinput/11/0", cx);
            }
            assert_eq!(window.find("dinput/11/0").focused(), Some(true));
            window.input("newcol", cx);
            assert_eq!(window.find("dinput/11/0").value(), Some("newcol"));
        })
        .unwrap();

        // 键入写回随 effect 冲刷落地；主键 / 非空 / 自增是同步 on_click
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, _| {
                let c = &a.opened[0].dstate.as_ref().unwrap().cols[11];
                assert_eq!(c.name, "newcol");
            });
            window.click("dkey", cx);
            window.click("dcell/11/4", cx);
            window.click("dprop/auto/11", cx);
            app.update(cx, |a, _| {
                let c = &a.opened[0].dstate.as_ref().unwrap().cols[11];
                assert!(c.pk && c.not_null && c.auto_inc);
            });

            // 再点键格取消主键
            window.click("dkey", cx);
            app.update(cx, |a, _| {
                assert!(!a.opened[0].dstate.as_ref().unwrap().cols[11].pk)
            });

            // 删锚点行：模型与编辑器行同步回收，回到基线
            window.click("ddel", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.cols.len(), 11);
                assert_eq!(ds.edits.len(), 11);
                assert_eq!(ds.cols, ds.baseline);
            });

            // 删到只剩一行时禁删
            for _ in 0..10 {
                window.click("ddel", cx);
            }
            app.update(cx, |a, _| {
                assert_eq!(a.opened[0].dstate.as_ref().unwrap().cols.len(), 1)
            });
            window.click("ddel", cx);
            app.update(cx, |a, _| {
                assert_eq!(a.opened[0].dstate.as_ref().unwrap().cols.len(), 1)
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn designer_subtabs_sql_preview(cx: &mut TestAppContext) {
        isolate_store("subtabs");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);

            // 占位子标签：点「索引」只换状态，字段格退场
            window.click("dsub/1", cx);
            window.render_frame(cx);
            assert!(window.try_find("dcell/0/0").is_none(), "字段格退场");
            app.update(cx, |a, _| {
                assert_eq!(a.opened[0].dstate.as_ref().unwrap().sub, 1)
            });

            // SQL 预览页
            window.click("dsub/10", cx);
            window.render_frame(cx);
            assert!(window.try_find("dsql-scroll").is_some());
            app.update(cx, |a, _| {
                assert_eq!(a.opened[0].dstate.as_ref().unwrap().sub, 10)
            });

            // 回字段页
            window.click("dsub/0", cx);
            window.render_frame(cx);
            assert!(window.try_find("dcell/0/0").is_some());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn sql_preview_change_script_and_diff(cx: &mut TestAppContext) {
        isolate_store("sqlpv");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            // PostgreSQL 连接（连接 1 / 库 postgres 全量池 / 模式 public）
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(1, 3, 0, 0, 0), window, cx)
            });
            window.render_frame(cx);

            // 加一条排除空行（模型直改——排除页的添加入口还在并行重做；
            // 参考工具口径：新行方法默认 btree，空行也出形状）
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_mut().unwrap();
                ds.excludes.push(DesignExclude::default());
                assert!(design_dirty(ds), "加排除算已修改");
            });

            // SQL 预览页：默认「更改」——增量脚本 + 复制按钮
            window.click("dsub/10", cx);
            window.render_frame(cx);
            assert!(window.try_find("dsql-copy").is_some(), "更改脚本带复制按钮");
            app.update(cx, |a, _| {
                let tab = &a.opened[0];
                let ds = tab.dstate.as_ref().unwrap();
                let conn = a.conns.iter().find(|c| c.name == tab.conn_name).unwrap();
                let schema = mock::schemas(conn.kind)[tab.schema];
                let table = mock::objects(conn, tab.db, tab.cat)[tab.obj];
                let sql = sql_preview::design_change_sql(conn.kind, schema, table, ds);
                assert!(
                    sql.contains(&format!("ALTER TABLE \"public\".\"{table}\"\n  ADD EXCLUDE USING btree;")),
                    "{sql}"
                );
                // 触发器 / 规则段：参考工具即使没有触发器也照出空模板
                assert!(
                    sql.contains(&format!(
                        "CREATE TRIGGER BEFORE ON \"public\".\"{table}\"\nFOR EACH ROW\nEXECUTE PROCEDURE ;"
                    )),
                    "{sql}"
                );
                assert!(
                    sql.contains(&format!(
                        "CREATE RULE  AS ON INSERT TO \"public\".\"{table}\" DO NOTHING;"
                    )),
                    "{sql}"
                );
            });

            // 切「DDL」子页：之前 / 之后 两栏都渲染
            window.click("dsqlsub/1", cx);
            window.render_frame(cx);
            app.update(cx, |a, _| {
                assert_eq!(a.opened[0].dstate.as_ref().unwrap().sql_sub, 1);
            });
            // 切到 DDL 后，「更改」页的元素退场，两栏都在
            assert!(window.try_find("dsql-before").is_some(), "之前栏");
            assert!(window.try_find("dsql-after").is_some(), "之后栏");
            assert!(window.try_find("dsql-copy").is_none(), "复制按钮退场");
        })
        .unwrap();
    }

    // ── 查询页（排期7）──────────────────────────────────────────────

    #[test]
    fn query_pure_functions() {
        // 顶层分号拆分 + trim；末尾无分号也算一条；空 / 纯空白不算
        assert_eq!(
            split_sql_statements("SELECT 1; SELECT 2;"),
            vec!["SELECT 1", "SELECT 2"]
        );
        assert_eq!(split_sql_statements("SELECT 1"), vec!["SELECT 1"]);
        assert!(split_sql_statements("").is_empty());
        assert!(split_sql_statements("  ;  ;  ").is_empty());

        // 结构内部的分号不拆：单引号（含双写转义）/ 双引号 / 反引号 / 方括号
        assert_eq!(split_sql_statements("SELECT 'a;b'"), vec!["SELECT 'a;b'"]);
        assert_eq!(
            split_sql_statements("SELECT 'it''s;ok'"),
            vec!["SELECT 'it''s;ok'"]
        );
        assert_eq!(split_sql_statements("SELECT \"a;b\""), vec!["SELECT \"a;b\""]);
        assert_eq!(split_sql_statements("SELECT `a;b`"), vec!["SELECT `a;b`"]);
        assert_eq!(split_sql_statements("SELECT [a;b]"), vec!["SELECT [a;b]"]);

        // 行注释 / PG 嵌套块注释 / 美元引号内的分号不拆
        assert_eq!(
            split_sql_statements("SELECT 1 -- x;y\n; SELECT 2"),
            vec!["SELECT 1 -- x;y", "SELECT 2"]
        );
        assert_eq!(
            split_sql_statements("SELECT /* a /* b */ c */ 1; SELECT 2"),
            vec!["SELECT /* a /* b */ c */ 1", "SELECT 2"]
        );
        assert_eq!(
            split_sql_statements("SELECT $$;$$; SELECT 2"),
            vec!["SELECT $$;$$", "SELECT 2"]
        );
        assert_eq!(
            split_sql_statements("SELECT $fn$;$fn$; SELECT 2"),
            vec!["SELECT $fn$;$fn$", "SELECT 2"]
        );

        // $1 占位参数不是美元引号起始；标签必须以 $ 收尾
        assert_eq!(dollar_quote_tag("$1"), None);
        assert_eq!(dollar_quote_tag("$$"), Some("$$".into()));
        assert_eq!(dollar_quote_tag("$fn$"), Some("$fn$".into()));
        assert_eq!(dollar_quote_tag("$fn"), None);

        assert!(query_is_select("SELECT 1"));
        assert!(query_is_select("  with cte as (select 1) select * from cte"));
        assert!(query_is_select("show tables"));
        assert!(!query_is_select("UPDATE t SET x = 1"));
        assert!(!query_is_select("insert into t values (1)"));
        assert!(!query_is_select(""));

        // mock 结果按语句文本散列：同语句恒定、行数 8..=47、列数同 MOCK_COLUMNS
        let rows = mock_query_rows("SELECT 1");
        assert_eq!(rows, mock_query_rows("SELECT 1"));
        let n = mock::mock_row_count("SELECT 1");
        assert_eq!(rows.len(), n);
        assert!((8..=47).contains(&n));
        assert_eq!(rows[0].len(), mock::MOCK_COLUMNS.len());
        assert_eq!(rows[3][4], mock::mock_cell("SELECT 1", 3, 4));
    }

    /// 索引「字段」浮层：勾选列 → 确定写回字段序；取消不动模型；勾掉的列从索引里摘掉。
    #[gpui_kit::test]
    fn index_fields_picker_commits_in_order(cx: &mut TestAppContext) {
        isolate_store("idxfields");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);
            app.update(cx, |a, cx| {
                let did = a.opened[0].did;
                // mock 表自带一条索引（org_id + name），照参考图那屏
                {
                    let ds = a.opened[0].dstate.as_ref().unwrap();
                    assert_eq!(ds.indexes.len(), 1);
                    assert_eq!(ds.indexes[0].fields.len(), 2);
                    assert!(ds.indexes[0].unique);
                }
                a.design_open_picker(did, PickTarget::IndexCols, 0, cx);
                {
                    let ds = a.opened[0].dstate.as_ref().unwrap();
                    let popup = ds.popup.as_ref().expect("浮层已打开");
                    // 候选 = 表里全部列，顺序按表顺序（id / org_id / parent_id / name …）
                    let names: Vec<&str> = popup.items.iter().map(|(n, _)| n.as_str()).collect();
                    assert_eq!(names[0], "id");
                    assert_eq!(names[1], "org_id");
                    assert_eq!(names[3], "name");
                    // 已勾的是索引里的两列
                    assert!(!popup.items[0].1);
                    assert!(popup.items[1].1);
                    assert!(popup.items[3].1);
                }

                // 勾上 id → 确定：字段顺序按列表顺序（id 在最前）
                a.design_popup_toggle(did, 0, cx);
                a.design_popup_commit(did, window, cx);
                {
                    let ds = a.opened[0].dstate.as_ref().unwrap();
                    assert!(ds.popup.is_none(), "确定后浮层关闭");
                    assert_eq!(
                        ds.indexes[0]
                            .fields
                            .iter()
                            .map(|f| f.name.as_str())
                            .collect::<Vec<_>>(),
                        vec!["id", "org_id", "name"]
                    );
                    // 新勾的列按类型推默认运算符类别
                    assert_eq!(ds.indexes[0].fields[0].opclass, "int8_ops");
                    assert_eq!(ds.indexes[0].fields[2].opclass, "text_ops");
                }

                // 取消勾选 org_id + 取消 → 模型不动
                a.design_open_picker(did, PickTarget::IndexCols, 0, cx);
                a.design_popup_toggle(did, 1, cx);
                a.design_popup_cancel(did, cx);
                {
                    let ds = a.opened[0].dstate.as_ref().unwrap();
                    assert!(ds.popup.is_none());
                    assert_eq!(ds.indexes[0].fields.len(), 3, "取消不改模型");
                }
                // 再取消勾选后确定 → org_id 从索引里摘掉
                a.design_open_picker(did, PickTarget::IndexCols, 0, cx);
                a.design_popup_toggle(did, 1, cx);
                a.design_popup_commit(did, window, cx);
                {
                    let ds = a.opened[0].dstate.as_ref().unwrap();
                    assert_eq!(
                        ds.indexes[0]
                            .fields
                            .iter()
                            .map(|f| f.name.as_str())
                            .collect::<Vec<_>>(),
                        vec!["id", "name"]
                    );
                    assert!(design_dirty(ds), "索引改动算已修改（页签前的 *）");
                }
            });
        })
        .unwrap();
    }

    /// 索引子页：工具栏换成「添加索引 / 删除索引」，表格带行首空白槽，
    /// 点「字段」格弹勾选浮层（空白表头格 + 七列）。
    #[gpui_kit::test]
    fn index_page_toolbar_grid_and_popup(cx: &mut TestAppContext) {
        isolate_store("idxpage");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);
            window.click("dsub/1", cx);
            window.render_frame(cx);

            for id in [
                "didx-add",
                "didx-del",
                "idx-gutter/0",
                "idx-cell/0/0",
                "idx-cell/0/1",
                "idx-method/0",
            ] {
                assert!(window.try_find(id).is_some(), "{id}");
            }
            assert!(window.try_find("dadd").is_none(), "字段页的按钮退场");

            // 点行首槽：选中整行
            window.click("idx-gutter/0", cx);
            app.update(cx, |a, _| {
                assert_eq!(a.opened[0].dstate.as_ref().unwrap().index_sel, 0)
            });

            // 点「字段」格 → 勾选浮层；表头首格空着，行里是勾选框
            window.click("idx-cell/0/1", cx);
            window.render_frame(cx);
            assert!(window.try_find("dpopup").is_some(), "字段浮层");
            assert!(window.try_find("idx-grid-scroll").is_some(), "表格滚动容器");
            assert!(window.try_find("dpopup/0").is_some());
            assert!(window.try_find("dpopup-ok").is_some(), "确定按钮");
            window.click("dpopup-cancel", cx);
            window.render_frame(cx);
            assert!(window.try_find("dpopup").is_none(), "取消关掉浮层");

            // 添加 / 删除索引（工具栏）
            window.click("didx-add", cx);
            app.update(cx, |a, _| {
                assert_eq!(a.opened[0].dstate.as_ref().unwrap().indexes.len(), 2)
            });
            window.render_frame(cx);
            window.click("didx-del", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.indexes.len(), 1);
                assert_eq!(ds.index_sel, 0, "删完锚点夹回");
            });
        })
        .unwrap();
    }

    /// 一条索引都没有时表格常驻一行空白种子行：直接输入即可、不用先点「添加索引」；
    /// 删光后补回来；已有空白行时「添加索引」只选中它；空白行不计入索引数 / 不算已修改。
    #[gpui_kit::test]
    fn index_blank_row_seeded_and_reused(cx: &mut TestAppContext) {
        isolate_store("idxblank");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            // 新建表：没有索引 → 索引页应有一行空白种子行
            app.update(cx, |a, cx| {
                a.open_new_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.indexes.len(), 1, "空白种子行");
                assert!(design_index_blank(&ds.indexes[0]));
                assert!(!design_dirty(ds), "空白行不算已修改");
            });

            // 删掉唯一那一行 → 仍然补一行空白
            app.update(cx, |a, cx| {
                let did = a.opened[0].did;
                a.design_remove_index(did, 0, window, cx);
            });
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.indexes.len(), 1, "删光补空白行");
                assert!(design_index_blank(&ds.indexes[0]));
            });

            // 再点「添加索引」：复用那行空白，不堆第二行
            app.update(cx, |a, cx| {
                let did = a.opened[0].did;
                a.design_add_index(did, window, cx);
            });
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.indexes.len(), 1, "已有空白行就选中它");
            });

            // 索引页：空白行不算「索引数」，也不挡保存
            window.click("dsub/1", cx);
            window.render_frame(cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(
                    ds.indexes.iter().filter(|i| !design_index_blank(i)).count(),
                    0
                );
                // 新建表本身还有一行空字段（第 1 行列名不能为空），这里只关心索引：
                // 空白索引行不应该产生任何「索引…」的校验错。
                if let Some(err) =
                    design_validation(&ds.cols, &ds.indexes, &ds.fks, &ds.uniques, &ds.checks)
                {
                    assert!(!err.contains("索引"), "空白索引行不挡保存：{err}");
                }
            });
        })
        .unwrap();
    }

    /// 索引属性 / 方法 / 唯一键 / 并发落到模型并进 DDL（预演里的 CREATE INDEX）。
    #[gpui_kit::test]
    fn index_props_and_flags_feed_ddl(cx: &mut TestAppContext) {
        isolate_store("idxprops");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);
            app.update(cx, |a, cx| {
                let did = a.opened[0].did;
                a.design_set_index_method(did, 0, "GiST".to_string(), cx);
                a.design_toggle_index_flag(did, 0, false, cx); // 并发
                a.design_set_index_prop(did, 0, 1, "ON".to_string(), cx); // 正在缓冲
                a.design_set_index_prop(did, 0, 0, "pg_default".to_string(), cx); // 表空间
                {
                    let ds = a.opened[0].dstate.as_ref().unwrap();
                    let idx = &ds.indexes[0];
                    assert_eq!(idx.method, "GiST");
                    assert!(idx.concurrent);
                    assert_eq!(idx.buffering, "ON");
                    assert_eq!(idx.tablespace, "pg_default");
                }
                let ds = a.opened[0].dstate.as_ref().unwrap();
                let sql = design_index_sql_at(false, "", "t", &ds.indexes[0], &ds.cols).unwrap();
                assert!(sql.contains("CREATE UNIQUE INDEX CONCURRENTLY"), "{sql}");
                assert!(sql.contains("USING GIST"), "{sql}");
                assert!(sql.contains("WITH (buffering = on)"), "{sql}");
                assert!(sql.contains("TABLESPACE pg_default"), "{sql}");
                // 索引改动要算「已修改」
                assert!(design_dirty(ds));
            });
            // 保存后基线跟上，不再算脏
            window.render_frame(cx);
            window.click("dsub/1", cx);
            window.render_frame(cx);
            window.click("dsave", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert!(!design_dirty(ds));
                assert_eq!(ds.note, Some("已保存（演示数据，不落库）"));
            });
        })
        .unwrap();
    }

    /// 外键子页 1:1：工具条分流（添加 / 删除外键）、种子空行、列选择浮层、
    /// 下拉写回与「已修改」联动。
    #[gpui_kit::test]
    fn fk_page_toolbar_grid_picker_and_actions(cx: &mut TestAppContext) {
        isolate_store("fkpage");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);
            window.click("dsub/2", cx);
            window.render_frame(cx);

            // 工具条：保存 | 添加外键 删除外键；索引页 / 字段页的按钮退场
            for id in ["dsave", "dfk-add", "dfk-del"] {
                assert!(window.try_find(id).is_some(), "{id}");
            }
            assert!(window.try_find("didx-add").is_none(), "索引按钮退场");
            assert!(window.try_find("dadd").is_none(), "字段按钮退场");

            // 种子空行：在模型里占一格，但不算「外键数」、不算已修改
            for id in ["fk-cell/0/0", "fk-cell/0/1", "fk-cell/0/2", "fk-scroll"] {
                assert!(window.try_find(id).is_some(), "{id}");
            }
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.fks.len(), 1);
                assert!(design_fk_blank(&ds.fks[0]));
                assert_eq!(ds.fks.iter().filter(|f| !design_fk_blank(f)).count(), 0);
                assert!(!design_dirty(ds), "空白种子行不算改动");
                assert_eq!(
                    design_validation(&ds.cols, &ds.indexes, &ds.fks, &ds.uniques, &ds.checks),
                    None,
                    "空白种子行不挡保存"
                );
            });

            // 当前格「字段」右缘 … → 列选择浮层（FK 勾选面板没有 −）；取消不动模型
            window.click("fk-cell/0/1", cx);
            window.render_frame(cx);
            assert!(window.try_find("fk-afford/0/1").is_some(), "当前格带 … 入口");
            assert!(
                window.try_find("fk-afford/0/4").is_none(),
                "非当前格不带入口"
            );
            window.click("fk-afford/0/1", cx);
            window.render_frame(cx);
            assert!(window.try_find("dpopup").is_some(), "列选择浮层");
            assert!(
                window.try_find("dpopup-del").is_none(),
                "FK 勾选面板没有 −"
            );
            window.click("dpopup-cancel", cx);
            window.render_frame(cx);
            assert!(window.try_find("dpopup").is_none(), "取消关掉浮层");

            // 被引用的表 ▼ → 普通下拉；点一项即写回并自动带出 public 模式
            window.click("fk-cell/0/3", cx);
            window.render_frame(cx);
            window.click("fk-afford/0/3", cx);
            window.render_frame(cx);
            assert!(window.try_find("dpopup").is_some(), "引用表下拉");
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                let names: Vec<&str> = ds
                    .popup
                    .as_ref()
                    .unwrap()
                    .items
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect();
                assert!(names.contains(&"bx_cart"), "下拉列当前库的表: {names:?}");
            });
            window.click("dpopup/0", cx);
            window.render_frame(cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert!(ds.popup.is_none(), "点选即关");
                let fk = &ds.fks[0];
                assert!(!fk.ref_table.is_empty(), "引用表写回");
                // 测试连接是 MySQL：模式自动带出所在库（PG 则带 public）
                assert_eq!(fk.ref_schema, "bx11", "模式自动带出");
            });

            // 删除时 ▼ → 动作下拉（参考图顺序 RESTRICT 最前）
            window.click("fk-cell/0/5", cx);
            window.render_frame(cx);
            window.click("fk-afford/0/5", cx);
            window.render_frame(cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                let p = ds.popup.as_ref().unwrap();
                assert_eq!(p.items[0].0, "RESTRICT", "RESTRICT 排最前");
                assert_eq!(p.items.len(), 5);
            });
            window.click("dpopup/0", cx);
            window.render_frame(cx);
            app.update(cx, |a, _| {
                assert_eq!(
                    a.opened[0].dstate.as_ref().unwrap().fks[0].on_delete,
                    "RESTRICT"
                );
            });

            // 注释是独立小窗（参考图可越出主窗边界）：写回走 finish_fk_comment，
            // 模型与常驻输入格同步
            app.update(cx, |a, cx| {
                let did = a.opened[0].did;
                a.finish_fk_comment(did, 0, "组织外键".to_string(), window, cx);
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.fks[0].comment, "组织外键");
            });
            window.render_frame(cx);

            // 添加外键：两行（种子 + 新行），锚点跟到新行
            window.click("dfk-add", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.fks.len(), 2);
                assert_eq!(ds.fk_sel, 1);
            });
            window.render_frame(cx);
            window.click("dfk-del", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.fks.len(), 1);
                assert_eq!(ds.fk_sel, 0, "锚点夹回");
            });
            // 删光后空行常驻（参考工具口径）
            window.click("dfk-del", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.fks.len(), 1, "空行常驻");
                assert!(design_fk_blank(&ds.fks[0]));
            });

            // 填好一条外键：改动要算「已修改」，DDL 带上模式限定 / ON DELETE / COMMENT
            app.update(cx, |a, _| {
                {
                    let ds = a.opened[0].dstate.as_mut().unwrap();
                    ds.fks[0].on_delete = "CASCADE".into();
                    ds.fks[0].on_update = "SET NULL".into();
                    ds.fks[0].name = "fk_org".into();
                    ds.fks[0].ref_schema = "public".into();
                    ds.fks[0].columns = "org_id".into();
                    ds.fks[0].ref_table = "organizations".into();
                    ds.fks[0].ref_columns = "id".into();
                    ds.fks[0].comment = "组织外键".into();
                }
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert!(design_dirty(ds));
                let sql = design_create_sql(
                    mock::DbKind::PostgreSql,
                    "tickets",
                    &ds.cols,
                    &ds.indexes,
                    &ds.fks,
                    &ds.uniques,
                    &ds.checks,
                    &ds.options,
                    &ds.excludes,
                    &ds.rules,
                );
                assert!(sql.contains("ON DELETE CASCADE"), "{sql}");
                assert!(sql.contains("ON UPDATE SET NULL"), "{sql}");
                assert!(
                    sql.contains("REFERENCES \"public\".\"organizations\""),
                    "{sql}"
                );
                assert!(
                    sql.contains(
                        "COMMENT ON CONSTRAINT \"fk_org\" ON \"tickets\" IS '组织外键';"
                    ),
                    "{sql}"
                );
                // 外键数：填好后的这一行要算进去
                assert_eq!(ds.fks.iter().filter(|f| !design_fk_blank(f)).count(), 1);
            });
        })
        .unwrap();
    }

    /// 唯一键子页 1:1：工具条分流（添加 / 删除唯一键）、种子空行、勾选浮层、
    /// 字段写回与注释进 DDL。
    #[gpui_kit::test]
    fn unique_page_toolbar_grid_and_popup(cx: &mut TestAppContext) {
        isolate_store("unqpage");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);
            window.click("dsub/3", cx);
            window.render_frame(cx);

            // 工具条：保存 | 添加唯一键 删除唯一键；外键 / 索引按钮退场
            for id in ["dsave", "dun-add", "dun-del"] {
                assert!(window.try_find(id).is_some(), "{id}");
            }
            assert!(window.try_find("dfk-add").is_none(), "外键按钮退场");
            assert!(window.try_find("didx-add").is_none(), "索引按钮退场");

            // 种子空行：在模型里占一格，但不算「唯一键数」、不算已修改
            for id in ["un-cell/0/0", "un-cell/0/1", "un-cell/0/2", "un-scroll"] {
                assert!(window.try_find(id).is_some(), "{id}");
            }
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.uniques.len(), 1);
                assert!(design_pair_blank(&ds.uniques[0]));
                assert_eq!(
                    ds.uniques.iter().filter(|u| !design_pair_blank(u)).count(),
                    0
                );
                assert!(!design_dirty(ds), "空白种子行不算改动");
            });

            // 当前格「字段」… → 勾选浮层（网格样式，无 −）；勾一列确定写回
            window.click("un-cell/0/1", cx);
            window.render_frame(cx);
            assert!(window.try_find("un-afford/0/1").is_some(), "当前格带 … 入口");
            window.click("un-afford/0/1", cx);
            window.render_frame(cx);
            assert!(window.try_find("dpopup").is_some(), "列选择浮层");
            assert!(window.try_find("dpopup-del").is_none(), "勾选面板没有 −");
            window.click("dpopup/1", cx);
            window.render_frame(cx);
            window.click("dpopup-ok", cx);
            window.render_frame(cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert!(ds.popup.is_none(), "确定关掉浮层");
                assert_eq!(ds.uniques[0].value, "org_id", "勾选顺序写回");
                assert!(design_dirty(ds));
            });

            // 添加 / 删除唯一键；删光后空行常驻
            window.click("dun-add", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.uniques.len(), 2);
                assert_eq!(ds.unique_sel, 1);
            });
            window.render_frame(cx);
            window.click("dun-del", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.uniques.len(), 1);
                assert_eq!(ds.unique_sel, 0, "锚点夹回");
            });
            window.click("dun-del", cx);
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.uniques.len(), 1, "空行常驻");
                assert!(design_pair_blank(&ds.uniques[0]));
            });

            // 填好一条唯一键：DDL 带 CONSTRAINT + 注释（PG COMMENT ON CONSTRAINT）
            app.update(cx, |a, cx| {
                let did = a.opened[0].did;
                {
                    let ds = a.opened[0].dstate.as_mut().unwrap();
                    ds.uniques[0].name = "uk_cameras_org".into();
                    ds.uniques[0].value = "org_id".into();
                    ds.uniques[0].comment = "组织唯一".into();
                }
                a.finish_unique_comment(did, 0, "组织唯一".to_string(), window, cx);
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert!(design_dirty(ds));
                let sql = design_create_sql(
                    mock::DbKind::PostgreSql,
                    "tickets",
                    &ds.cols,
                    &ds.indexes,
                    &ds.fks,
                    &ds.uniques,
                    &ds.checks,
                    &ds.options,
                    &ds.excludes,
                    &ds.rules,
                );
                assert!(
                    sql.contains("CONSTRAINT \"uk_cameras_org\" UNIQUE (\"org_id\")"),
                    "{sql}"
                );
                assert!(
                    sql.contains(
                        "COMMENT ON CONSTRAINT \"uk_cameras_org\" ON \"tickets\" IS '组织唯一';"
                    ),
                    "{sql}"
                );
                // 唯一键数：填好后的这一行要算进去
                assert_eq!(
                    ds.uniques.iter().filter(|u| !design_pair_blank(u)).count(),
                    1
                );
            });
        })
        .unwrap();
    }

    /// 检查约束的表达式面板：文本域内容写回 checks[row]；取消不动。
    #[gpui_kit::test]
    fn design_check_expr_window_commits_text(cx: &mut TestAppContext) {
        isolate_store("dcheck");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);
            // 表达式小窗（独立顶层窗口）：确定写回 checks[row].value + 同步输入格
            app.update(cx, |a, cx| {
                let did = a.opened[0].did;
                a.design_add_check(did, window, cx);
                a.finish_check_expr(did, 0, "age > 0".to_string(), window, cx);
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.checks[0].value, "age > 0");
            });
            window.render_frame(cx);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn query_opens_from_ribbon_no_dedup(cx: &mut TestAppContext) {
        isolate_store("qopen");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("新建查询", cx);
            window.render_frame(cx);

            assert!(window.try_find("qedit").is_some(), "编辑器区已渲染");
            // 参考工具：没跑过查询时整块结果面板都不出现（查询页只有编辑区）
            assert!(window.try_find("qhead").is_none(), "未执行时没有结果面板");
            assert!(window.try_find("qtab/0").is_none(), "没出结果前没有结果页签");
            app.update(cx, |a, cx| {
                assert_eq!(a.opened.len(), 1);
                assert_eq!(a.active_tab, 1);
                let t = &a.opened[0];
                assert!(t.query);
                let q = t.qstate.as_ref().unwrap();
                assert_eq!((q.seq, q.db, q.schema, q.pane, q.dirty), (1, 0, 0, 0, false));
                assert_eq!(q.conn, "本地 MySQL", "无选中连接时取第一个");
                assert_eq!(q.editor.read(cx).value().to_string(), "");
            });

            // Navicat 习惯：每次触发都开新页签，不去重
            window.click("新建查询", cx);
            window.render_frame(cx);
            app.update(cx, |a, _| {
                assert_eq!(a.opened.len(), 2);
                assert_eq!(a.active_tab, 2);
                assert_eq!(a.opened[1].qstate.as_ref().unwrap().seq, 2);
            });

            // 选中连接 1（PostgreSQL）再开：运行上下文跟着选中走，库取第一个
            app.update(cx, |a, cx| {
                a.sel = Act::Conn(1);
                a.open_query_tab(None, window, cx);
            });
            app.update(cx, |a, _| {
                assert_eq!(a.opened.len(), 3);
                let (t, q) = (&a.opened[2], a.opened[2].qstate.as_ref().unwrap());
                assert_eq!(q.conn, "本地 PostgreSQL");
                assert_eq!(t.db, mock::databases(DbKind::PostgreSql)[0]);
            });

            // 上下文切换：连接变了库 / 模式回 0；库 / 模式按下标落位
            app.update(cx, |a, cx| {
                let qid = a.opened[2].qid;
                a.query_set_conn(qid, "本地 MySQL", cx);
                let q = a.opened[2].qstate.as_ref().unwrap();
                assert_eq!(q.conn, "本地 MySQL");
                assert_eq!((q.db, q.schema), (0, 0));
                a.query_set_db(qid, "sys", cx);
                assert_eq!(a.opened[2].qstate.as_ref().unwrap().db, 5);
                a.query_set_conn(qid, "本地 PostgreSQL", cx);
                assert_eq!(
                    a.opened[2].qstate.as_ref().map(|q| (q.db, q.schema)),
                    Some((0, 0))
                );
                a.query_set_schema(qid, "public", cx);
                assert_eq!(a.opened[2].qstate.as_ref().unwrap().schema, 0);
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn query_run_select_write_empty(cx: &mut TestAppContext) {
        isolate_store("qrun");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, cx| {
                a.open_query_tab(None, window, cx);
            });
        })
        .unwrap();

        // 点编辑器拿焦点，键入一条 SELECT
        cx.update_window(handle, |_, window, cx| {
            window.click("qedit", cx);
            window.input("SELECT 1", cx);
        })
        .unwrap();

        // 订阅回调走 effect 队列，冲刷发生在 update 边界 —— 模型断言另起一轮
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| {
                let q = a.opened[0].qstate.as_ref().unwrap();
                assert_eq!(q.editor.read(cx).value().to_string(), "SELECT 1");
                assert!(q.dirty, "编辑过 → 页签标题前 *");
            });

            // 点「运行」主按钮（观察容器转发真点击）
            window.click_at("qrun-wrap", point(px(10.), px(11.)), cx);
            window.render_frame(cx);
            app.update(cx, |a, cx| {
                let q = a.opened[0].qstate.as_ref().unwrap();
                assert_eq!(q.outcomes.len(), 1);
                let o = &q.outcomes[0];
                assert_eq!(o.sql, "SELECT 1");
                assert_eq!(o.result.as_ref().map(|r| r.len()), Some(mock::mock_row_count("SELECT 1")));
                assert_eq!(q.pane, 1, "执行完切到结果页签");
                let status = DaroApp::query_status_text(q);
                assert!(status.starts_with("执行成功 · "), "{status}");
                assert!(status.contains(" 行 · "), "{status}");
                let log = q.log.read(cx).value().to_string();
                assert!(log.starts_with("SELECT 1\n语句 1: "), "{log}");
                assert!(log.contains(" 行 · "), "{log}");
            });
            assert!(window.try_find("qtab/0").is_some(), "消息页签");
            assert!(window.try_find("qtab/1").is_some(), "结果 1 页签");
            assert!(window.try_find("qgrid-scroll/0").is_some());
            assert!(window.try_find("qcell/0/0/0").is_some());
            assert!(window.try_find("qmsg").is_none(), "网格页不渲染消息容器");

            // 结果页签条点回「消息」：网格退场、日志容器进场
            window.click("qtab/0", cx);
            window.render_frame(cx);
            assert!(window.try_find("qmsg").is_some());
            assert!(window.try_find("qcell/0/0/0").is_none());
            app.update(cx, |a, _| {
                assert_eq!(a.opened[0].qstate.as_ref().unwrap().pane, 0)
            });

            // 写操作：不出网格、停在消息页、状态文案换成演示口径
            app.update(cx, |a, cx| {
                let editor = a.opened[0].qstate.as_ref().unwrap().editor.clone();
                editor.update(cx, |s, cx| s.set_value("UPDATE t SET x = 1", window, cx));
            });
            window.click_at("qrun-wrap", point(px(10.), px(11.)), cx);
            window.render_frame(cx);
            app.update(cx, |a, _| {
                let q = a.opened[0].qstate.as_ref().unwrap();
                assert_eq!(q.outcomes.len(), 1);
                assert!(q.outcomes[0].result.is_none(), "mock 不执行写操作");
                assert_eq!(q.pane, 0, "全是写操作停在消息页");
                let status = DaroApp::query_status_text(q);
                assert!(status.starts_with("演示数据 · 写操作未执行 · "), "{status}");
            });
            assert!(window.try_find("qtab/0").is_some());
            assert!(window.try_find("qtab/1").is_some(), "写操作也有结果页签（内容留白）");
            assert!(window.try_find("qmsg").is_some());
            assert!(window.try_find("qcell/0/0/0").is_none());

            // 空脚本：出提示、清结果、撤页签条
            app.update(cx, |a, cx| {
                let editor = a.opened[0].qstate.as_ref().unwrap().editor.clone();
                editor.update(cx, |s, cx| s.set_value("   ", window, cx));
            });
            window.click_at("qrun-wrap", point(px(10.), px(11.)), cx);
            window.render_frame(cx);
            app.update(cx, |a, _| {
                let q = a.opened[0].qstate.as_ref().unwrap();
                assert!(q.outcomes.is_empty());
                assert_eq!(q.prompt, Some("请输入 SQL 语句"));
                assert_eq!(DaroApp::query_status_text(q), "请输入 SQL 语句");
            });
            assert!(window.try_find("qtab/0").is_none(), "清掉结果页签条");
            assert!(window.try_find("qmsg").is_none());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn query_run_selected_only(cx: &mut TestAppContext) {
        isolate_store("qsel");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, cx| {
                a.open_query_tab(None, window, cx);
            });
            window.click("qedit", cx);
            window.input("SELECT 11;\nSELECT 22;", cx);
        })
        .unwrap();

        cx.update_window(handle, |_, window, cx| {
            // 光标回行首 + shift-end：只选中第一条
            window.press("ctrl-home", cx);
            window.press("shift-end", cx);
            app.update(cx, |a, cx| {
                let q = a.opened[0].qstate.as_ref().unwrap();
                assert_eq!(
                    q.editor.read(cx).selected_text().to_string(),
                    "SELECT 11;",
                    "选区落在第一条语句上"
                );
            });

            // 有选区时主按钮 = 「运行已选择的」：只跑选区
            window.click_at("qrun-wrap", point(px(10.), px(11.)), cx);
            app.update(cx, |a, _| {
                let q = a.opened[0].qstate.as_ref().unwrap();
                assert_eq!(q.outcomes.len(), 1, "只执行了选中的那条");
                assert_eq!(q.outcomes[0].sql, "SELECT 11");
                assert!(q.outcomes[0].result.is_some());
            });
        })
        .unwrap();
    }

    // ── 数据网格列头排序（E节）─────────────────────────────────────

    #[test]
    fn table_sort_pure_functions() {
        use std::cmp::Ordering;
        // 数值列按数值比："999" 要排在 "1002" 前面
        assert_eq!(compare_cells("999", "1002", true), Ordering::Less);
        assert_eq!(compare_cells("1002", "999", true), Ordering::Greater);
        assert_eq!(compare_cells("42", "42.0", true), Ordering::Equal);
        // 数值列里混进非数值：退回字符串比
        assert_eq!(compare_cells("abc", "abd", true), Ordering::Less);
        assert_eq!(compare_cells("b", "a", false), Ordering::Greater);

        // 无排序：恒等映射
        let data: Vec<Vec<String>> = (0..5)
            .map(|r| {
                (0..mock::MOCK_COLUMNS.len())
                    .map(|c| mock::mock_cell("orders", r, c).to_string())
                    .collect()
            })
            .collect();
        assert_eq!(sorted_row_order(&data, None), vec![0, 1, 2, 3, 4]);

        // flags 列（值 = 行号 % 3）升序：0/1/2 三组，组内保持原行序（稳定）
        let rows = mock::mock_row_count("orders");
        let data: Vec<Vec<String>> = (0..rows)
            .map(|r| {
                (0..mock::MOCK_COLUMNS.len())
                    .map(|c| mock::mock_cell("orders", r, c).to_string())
                    .collect()
            })
            .collect();
        let flag = |row: &Vec<String>| row[6].parse::<i64>().unwrap();
        let asc = sorted_row_order(&data, Some((6, true)));
        assert_eq!(asc.len(), rows);
        for w in asc.windows(2) {
            let (a, b) = (flag(&data[w[0]]), flag(&data[w[1]]));
            assert!(a <= b, "升序被破坏: {w:?}");
            if a == b {
                assert!(w[0] < w[1], "同值行乱了原相对次序: {w:?}");
            }
        }
        // 降序：键反序，组内仍按原行序
        let desc = sorted_row_order(&data, Some((6, false)));
        for w in desc.windows(2) {
            let (a, b) = (flag(&data[w[0]]), flag(&data[w[1]]));
            assert!(a >= b, "降序被破坏: {w:?}");
            if a == b {
                assert!(w[0] < w[1], "同值行乱了原相对次序: {w:?}");
            }
        }
        // 同一输入恒定
        assert_eq!(asc, sorted_row_order(&data, Some((6, true))));
        // 列下标越界：防御性恒等
        assert_eq!(sorted_row_order(&data[..4], Some((99, true))), vec![0, 1, 2, 3]);
    }

    #[gpui_kit::test]
    fn table_grid_cell_edit(cx: &mut TestAppContext) {
        isolate_store("gridedit");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, _| {
                a.open_object(Act::Obj(0, 0, NO_SCHEMA, 0, 0));
            });
            window.render_frame(cx);
            // 双击 cell/0/1 进就地编辑（测试台的双击 click_count=2；
            // 真机的双击走 is_cell_double_click 的时间窗，实机验证）
            window.double_click("cell/0/1", cx);
        })
        .unwrap();

        // 编辑器事件订阅的激活走 effect 队列，冲刷发生在 update 边界 ——
        // 建编辑器、键入 / 断言要分轮（同 query_run_select_write_empty 的口径）
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, _| {
                let e = a.cell_editing.as_ref().expect("双击后应有编辑器");
                assert_eq!(e.at, (0, 1));
            });
            app.update(cx, |a, cx| {
                let input = a.cell_editing.as_ref().unwrap().input.clone();
                input.update(cx, |s, cx| s.set_value("hello", window, cx));
            });
            window.press("enter", cx);
        })
        .unwrap();

        cx.update_window(handle, |_, _, cx| {
            app.update(cx, |a, _| {
                let t = &a.opened[0];
                assert_eq!(t.data[0][1], "hello", "回车提交写回工作集");
                assert!(t.dirty(), "改值后 dirty");
                assert!(a.cell_editing.is_none(), "回车后编辑器收起");
            });
        })
        .unwrap();

        // 取消：改值不写回（Esc 全局动作在 main 里注册，测试直调取消）
        cx.update_window(handle, |_, window, cx| {
            window.double_click("cell/0/2", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| {
                let input = a.cell_editing.as_ref().expect("再次双击进编辑").input.clone();
                input.update(cx, |s, cx| s.set_value("nope", window, cx));
                a.cancel_cell_edit(cx);
                let t = &a.opened[0];
                assert_ne!(t.data[0][2], "nope", "取消不写回");
                assert!(t.dirty(), "第一次的提交还在");
                assert!(a.cell_editing.is_none());
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn table_grid_insert_delete_apply_revert(cx: &mut TestAppContext) {
        isolate_store("gridins");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, _| {
                a.open_object(Act::Obj(0, 0, NO_SCHEMA, 0, 0));
            });
            window.render_frame(cx);
        })
        .unwrap();
        let rows = app.update(cx, |a, _| a.opened[0].data.len());

        // + 新增：网格尾追加空行并选中，dirty 点亮
        app.update(cx, |a, cx| a.grid_insert_row(cx));
        app.update(cx, |a, _| {
            let t = &a.opened[0];
            assert_eq!(t.inserted.len(), 1);
            assert!(t.inserted[0].iter().all(|v| v.is_empty()));
            assert_eq!(t.cell, (rows, 0), "选中新行");
            assert!(t.dirty());
        });

        // 新行贴在最后一页，可就地编辑（订阅激活分轮，同上一个用例）
        let cid = format!("cell/{rows}/3");
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.double_click(cid, cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| {
                let input = a.cell_editing.as_ref().expect("新行可编辑").input.clone();
                input.update(cx, |s, cx| s.set_value("9.5".to_string(), window, cx));
            });
            app.update(cx, |a, cx| a.commit_cell_edit(cx));
            app.update(cx, |a, _| {
                assert_eq!(a.opened[0].inserted[0][3], "9.5", "新行单元格可写");
            });
        })
        .unwrap();

        // ✓ 应用：inserted 并进 data、基线刷新、dirty 熄灭
        app.update(cx, |a, cx| a.grid_apply(cx));
        app.update(cx, |a, _| {
            let t = &a.opened[0];
            assert_eq!(t.data.len(), rows + 1);
            assert!(t.inserted.is_empty());
            assert_eq!(t.data[rows][3], "9.5");
            assert!(!t.dirty(), "应用后无变更");
        });

        // − 删除当前行（刚应用的新行）：工作集移除，dirty 重新点亮
        app.update(cx, |a, cx| a.grid_delete_row(cx));
        app.update(cx, |a, _| {
            let t = &a.opened[0];
            assert_eq!(t.data.len(), rows);
            assert!(t.dirty(), "删行未应用 → dirty");
        });

        // × 撤销 / 刷新：退回基线，删的行复原
        app.update(cx, |a, cx| a.grid_revert(cx));
        app.update(cx, |a, _| {
            let t = &a.opened[0];
            assert_eq!(t.data.len(), rows + 1);
            assert_eq!(t.data[rows][3], "9.5");
            assert!(!t.dirty());
        });
    }

    #[gpui_kit::test]
    fn table_header_click_sorts(cx: &mut TestAppContext) {
        isolate_store("thsort");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, _| {
                a.open_object(Act::Obj(0, 0, NO_SCHEMA, 0, 0));
            });
            window.render_frame(cx);

            // 初始无排序，状态栏 SQL 无 ORDER BY
            app.update(cx, |a, _| {
                let t = &a.opened[0];
                assert_eq!(t.sort, None);
                let (sql, _) = a.table_page_status(t);
                assert!(!sql.contains("ORDER BY"), "{sql}");
            });

            // 点列头 0：升序 → 再点：降序；换列 1：重置升序
            window.click("th/0", cx);
            app.update(cx, |a, _| {
                let t = &a.opened[0];
                assert_eq!(t.sort, Some((0, true)));
                let (sql, _) = a.table_page_status(t);
                assert!(sql.contains("ORDER BY `id` ASC"), "{sql}");
            });
            window.click("th/0", cx);
            app.update(cx, |a, _| {
                let t = &a.opened[0];
                assert_eq!(t.sort, Some((0, false)));
                let (sql, _) = a.table_page_status(t);
                assert!(sql.contains("ORDER BY `id` DESC"), "{sql}");
            });
            window.click("th/1", cx);
            app.update(cx, |a, _| {
                let t = &a.opened[0];
                assert_eq!(t.sort, Some((1, true)));
                let (sql, _) = a.table_page_status(t);
                assert!(sql.contains("ORDER BY `org_id` ASC"), "{sql}");
            });

            // 排序态下点格：cell 存显示序位，点选与翻页口径不乱
            window.click("cell/2/3", cx);
            app.update(cx, |a, _| {
                let t = &a.opened[0];
                assert_eq!(t.cell, (2, 3));
                let (_, rec) = a.table_page_status(t);
                assert!(rec.starts_with("第 3 条记录"), "{rec}");
            });

            // 换页签回来排序态还在（排序随页签存亡）
            app.update(cx, |a, _| {
                a.active_tab = 0;
            });
            window.render_frame(cx);
            app.update(cx, |a, _| {
                a.active_tab = 1;
            });
            window.render_frame(cx);
            assert!(window.try_find("th/1").is_some(), "表头回得来");
            app.update(cx, |a, _| {
                assert_eq!(a.opened[0].sort, Some((1, true)));
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn f5_refresh_reverts_grid_via_key_binding(cx: &mut TestAppContext) {
        isolate_store("f5refresh");
        let (handle, app) = open_app(cx);
        // 与 main 同一条装配（绑定 f5 + 全局动作 + defer 进窗口）
        cx.update(|cx| wire_global_keys(app.clone(), handle, cx));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, _| a.open_object(Act::Obj(0, 0, NO_SCHEMA, 0, 0)));
            window.render_frame(cx);
            app.update(cx, |a, _| {
                let t = a.opened.first_mut().unwrap();
                assert!(!t.design && !t.query, "前置：数据页签");
                t.data[0][1] = "dirty".into();
                t.inserted.push(vec![String::new(); t.data[0].len()]);
                assert!(t.dirty());
            });
            window.press("f5", cx);
        })
        .unwrap();
        // 动作经 defer 进窗口，effect 冲刷后再断言
        cx.update_window(handle, |_, _, cx| {
            app.update(cx, |a, _| {
                let t = &a.opened[0];
                assert!(!t.dirty(), "F5 后工作集退回基线");
                assert_eq!(t.data, t.baseline);
                assert!(t.inserted.is_empty());
            });
        })
        .unwrap();

        // 设计页签上 F5 不动工作集（无「数据」可重取）
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| {
                a.open_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            window.render_frame(cx);
            app.update(cx, |a, _| {
                let ix = a.active_tab - 1;
                let t = &mut a.opened[ix];
                assert!(t.design);
                let ds = t.dstate.as_mut().unwrap();
                ds.cols[0].name = "renamed".into();
                assert!(ds.cols != ds.baseline, "设计器工作集已脏");
            });
            window.press("f5", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, _, cx| {
            app.update(cx, |a, _| {
                let t = &a.opened[a.active_tab - 1];
                let ds = t.dstate.as_ref().unwrap();
                assert_eq!(ds.cols[0].name, "renamed", "设计器工作集不被 F5 清");
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn new_table_designer_flow(cx: &mut TestAppContext) {
        isolate_store("newtable");
        let (handle, app) = open_app(cx);
        let mut did = 0;
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            // 动作条入口：树里选中一张表 → 「新建表」开空模型设计页签
            app.update(cx, |a, _| a.sel = Act::Obj(0, 0, NO_SCHEMA, 0, 0));
            window.render_frame(cx);
            window.click("act-new", cx);
            window.render_frame(cx);
            app.update(cx, |a, _| {
                assert_eq!(a.opened.len(), 1);
                assert_eq!(a.active_tab, 1, "新页签激活");
                let t = &a.opened[0];
                assert!(t.design);
                assert_eq!(t.obj, usize::MAX, "新表无目录项");
                let ds = t.dstate.as_ref().unwrap();
                assert_eq!(ds.new_name.as_deref(), Some(""), "未命名新表");
                assert_eq!(ds.cols.len(), 1, "预置一行空字段");
                assert_eq!(ds.cols, ds.baseline, "初始无变更");
                assert_eq!(ds.edits.len(), 1, "常驻编辑器随行建齐");
                assert_eq!(a.tab_label(t), "新建表", "命名前标题就叫新建表");
                did = t.did;
            });
            // 每次触发都开新页签，不去重（Navicat 同款）
            app.update(cx, |a, cx| {
                a.open_new_designer(Act::Obj(0, 0, NO_SCHEMA, 0, 0), window, cx)
            });
            app.update(cx, |a, _| {
                assert_eq!(a.opened.len(), 2);
                assert_eq!(a.active_tab, 2);
            });
            // 关掉第二个，回到第一个做保存链路
            app.update(cx, |a, _| a.close_tab(1));
            app.update(cx, |a, _| {
                assert_eq!(a.active_tab, 1);
            });
            // 填上字段名 → 工作集脏、标题带 *；保存 → 弹「表名」输入框（独立
            // 窗口），主窗口模型不动；模拟输入框确定 → 记名 + 基线对齐
            app.update(cx, |a, _| {
                let t = a.opened.first_mut().unwrap();
                let ds = t.dstate.as_mut().unwrap();
                ds.cols[0].name = "id".into();
                ds.cols[0].pk = true;
            });
            window.render_frame(cx);
            app.update(cx, |a, _| {
                let t = &a.opened[0];
                let ds = t.dstate.as_ref().unwrap();
                assert_ne!(ds.cols, ds.baseline, "改字段后为脏");
                assert_eq!(a.tab_label(t), "*新建表", "脏标进标题");
            });
            app.update(cx, |a, cx| a.design_save(did, cx));
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.new_name.as_deref(), Some(""), "弹窗期间主窗口不动");
                assert_ne!(ds.cols, ds.baseline, "弹窗期间不落基线");
            });
            app.update(cx, |a, _| a.finish_new_design_save(did, "t1".into()));
            app.update(cx, |a, _| {
                let t = &a.opened[0];
                let ds = t.dstate.as_ref().unwrap();
                assert_eq!(ds.new_name.as_deref(), Some("t1"));
                assert_eq!(ds.cols, ds.baseline, "保存后基线对齐");
                assert_eq!(ds.note, Some("已创建（演示数据，不落库）"));
                let label = a.tab_label(t);
                assert!(label.starts_with("t1 @"), "{label}");
                assert!(label.ends_with(" (设计)"), "{label}");
            });
            // 已命名的再保存走既有路径（不弹窗）
            app.update(cx, |a, cx| a.design_save(did, cx));
            app.update(cx, |a, _| {
                let ds = a.opened[0].dstate.as_ref().unwrap();
                assert_eq!(ds.note, Some("已保存（演示数据，不落库）"));
            });
        })
        .unwrap();
    }
}

/// 树右键菜单接线背后的模型逻辑：分组增删、库开合。
/// 菜单本体（PopupMenu）要真窗口渲染才能展开，这里只钉住状态迁移。
#[cfg(test)]
mod tree_menu_tests {
    use super::*;
    use gpui_kit::TestAppContext;

    use core::prelude::v1::test;

    fn isolate_store(tag: &str) {
        let dir =
            std::env::temp_dir().join(format!("daro_rs_test_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: 用例由 --test-threads=1 串行执行，且写 env 先于任何 DaroApp 构造
        unsafe { std::env::set_var("DARO_DATA_DIR", &dir) };
    }

    fn open_app(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DaroApp>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(1440.), px(900.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| DaroApp::new(window, cx)),
            )
            .expect("open test window")
        })
    }

    #[gpui_kit::test]
    fn group_add_dedup_and_delete_falls_back(cx: &mut TestAppContext) {
        isolate_store("menugroup");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, _, cx| {
            app.update(cx, |a, _| {
                let base = a.groups.len();
                let g1 = a.add_group("新建组");
                let g2 = a.add_group("新建组");
                assert_eq!((g1, g2), (base, base + 1));
                assert_eq!(&a.groups[g1], "新建组");
                assert_eq!(&a.groups[g2], "新建组 2");

                // 组内连接随删组回落未分组，连接本体不动
                let conn = a.conns[0].name.clone();
                a.move_connection_to_group(&conn, "新建组");
                a.sel = Act::Group(g1);
                a.delete_group(g1);
                assert_eq!(a.groups.len(), base + 1, "只删掉目标组");
                assert!(!a.groups.contains(&"新建组".to_string()));
                assert!(a.groups.contains(&"新建组 2".to_string()));
                assert_eq!(a.conns[0].group, "");
                assert_eq!(a.sel, Act::Root, "选中组被删后回落根节点");
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn close_db_unvisits_and_collapses(cx: &mut TestAppContext) {
        isolate_store("menudb");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, _, cx| {
            app.update(cx, |a, _| {
                let kind = a.conns[0].kind;
                let db = mock::databases(kind)[0];
                a.open_db(0, 0);
                let key = node_key(&a.conns[0].name, db);
                assert!(a.visited_dbs.contains(&key), "打开后图标转绿");
                assert!(a.expanded.contains(&key));
                a.close_db(0, 0);
                assert!(!a.visited_dbs.contains(&key), "关闭后图标回灰");
                assert!(!a.expanded.contains(&key), "关闭后子树收起");
            });
        })
        .unwrap();
    }
}
