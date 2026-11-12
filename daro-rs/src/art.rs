//! 图标入口：树 / 功能区 / 工具条统一按 `Glyph` 取图。
//!
//! 控件字形统一用 Zed 同款 SVG（`zed_icons`），跟随主题色与 hover 状态；
//! 数据库品牌图标用厂商开源 SVG（见 `db_icons`），属于内容标识而非控件字形。

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::Sizable as _;
use gpui_kit::*;

use crate::db_icons;
use crate::mock::DbKind;
use crate::zed_icons::{self, ZedIcon};

/// 图标种类。树 / 功能区 / 工具条都走这一套，
/// 单色线稿（lucide）通过 `Lucide` 变体继续可用。
#[derive(Clone, Copy, PartialEq)]
pub enum Glyph {
    // ── 树节点 ──
    /// 分组文件夹
    Folder,
    /// 连接：按数据库品牌上色
    Conn(DbKind),
    /// 库
    Db,
    /// 模式
    Schema,
    /// 表
    Table,
    /// 视图
    View,
    /// 实体化视图
    MatView,
    /// 视图设计器
    DesignView,
    /// 实体化视图设计器
    DesignMatView,
    /// 设计器工具条「预览」
    PreviewEye,
    /// 设计器工具条「视图创建工具」
    ViewBuilder,
    /// 函数
    Function,
    /// 查询
    Query,
    /// 用户 / 角色对象图标（页签用；功能区大图标仍走 `RibbonUser`）
    User,
    /// 字段 / 列
    Column,
    /// 表分组：索引
    IndexGroup,
    /// 表分组：外键
    ForeignKeyGroup,
    /// 表分组：唯一键
    UniqueKeyGroup,
    /// 表分组：检查
    CheckGroup,
    /// 表分组：排除
    ExcludeGroup,
    /// 表分组：规则
    RuleGroup,
    /// 表分组：触发器
    TriggerGroup,
    // ── 功能区 ──
    RibbonConn,
    RibbonNewQuery,
    RibbonTable,
    RibbonView,
    RibbonMatView,
    RibbonFunction,
    RibbonUser,
    RibbonTools,
    RibbonQuery,
    RibbonBackup,
    RibbonAutoRun,
    // ── 对象面板工具条 ──
    OpenTable,
    /// 工具条「设计」动作：只有一支铅笔（参考界面 TB_Design）
    Pencil,
    /// 表设计页签 / 设计器：表格 + 铅笔（参考界面 OV_Table_Design）
    DesignTable,
    NewTable,
    AiTable,
    DropTable,
    ImportWizard,
    ExportWizard,
    // ── 数据页工具条 ──
    /// 开始事务
    Txn,
    /// 单元格编辑器
    CellEdit,
    /// 筛选 & 排序
    FilterSort,
    /// 列
    Columns,
    /// 历史
    History,
    // ── 查询页工具条 ──
    /// 保存
    SaveFile,
    /// 美化 SQL
    Beautify,
    /// 代码段
    Snippet,
    /// 查询创建工具
    QueryBuilder,
    /// 代码段（括号版）
    CodeSnippet,
    /// 询问 AI
    AskAi,
    /// 运行
    PlayOutline,
    /// 停止
    StopOutline,
    /// 解释
    ExplainPlan,
    /// 主键
    KeyGold,
    // ── 右侧信息卡 ──
    HeroSchema,
    HeroTable,
    HeroView,
    HeroMatView,
    HeroFunction,
    HeroQuery,
    HeroFolder,
    HeroEngine(DbKind),
    HeroDb,
    /// 直接指定资源名，给一次性工具条按钮用
    Asset(&'static str),
    /// 单色线稿，颜色由调用方给
    Lucide(IconName),
    /// 直接指定 Zed 图标
    Zed(ZedIcon),
}

impl From<ZedIcon> for Glyph {
    fn from(icon: ZedIcon) -> Self {
        Glyph::Zed(icon)
    }
}

impl From<IconName> for Glyph {
    fn from(name: IconName) -> Self {
        // 常见的几款直接映射到语义变体，其余退回线稿
        match name {
            IconName::Table2 => Glyph::Table,
            IconName::Eye => Glyph::View,
            IconName::Layers => Glyph::MatView,
            IconName::SquareFunction => Glyph::Function,
            IconName::SquareTerminal => Glyph::Query,
            IconName::Database => Glyph::Db,
            IconName::Folder | IconName::FolderOpen | IconName::FolderClosed => Glyph::Folder,
            IconName::Boxes => Glyph::Schema,
            IconName::Funnel | IconName::ListFilter => Glyph::FilterSort,
            IconName::ArrowUpDown => Glyph::Asset("SORT"),
            IconName::Pencil => Glyph::Asset("TB_Design"),
            IconName::RefreshCw => Glyph::Asset("TB_Refresh"),
            IconName::SquareStop => Glyph::StopOutline,
            IconName::User => Glyph::RibbonUser,
            IconName::Star => Glyph::Asset("OBJECT_STAR"),
            IconName::Zap => Glyph::TriggerGroup,
            IconName::CalendarClock => Glyph::Asset("OV_Event"),
            IconName::Settings => Glyph::Asset("TB_Tools"),
            IconName::Share2 => Glyph::Asset("OV_Share"),
            IconName::Download => Glyph::Asset("TB_Save_To_Disk"),
            IconName::Info => Glyph::Asset("OV_Information"),
            IconName::CircleQuestionMark => Glyph::Asset("OV_Question"),
            IconName::Image => Glyph::Asset("OV_Model_Image"),
            IconName::ChartColumn => Glyph::Zed(ZedIcon::ChartBar),
            IconName::KeyRound => Glyph::KeyGold,
            IconName::ListChecks => Glyph::Asset("OV_Validation"),
            IconName::Shapes => Glyph::Asset("OV_Model_Shape"),
            IconName::Copy => Glyph::Asset("BTN_COPY_TO_CLIPBOARD"),
            IconName::Search => Glyph::Asset("BTN_SEARCH"),
            other => Glyph::Lucide(other),
        }
    }
}

/// 单色控件字形改走 Zed 图标；品牌连接、信息卡大图返回 None 继续用位图。
fn zed_for(glyph: Glyph) -> Option<ZedIcon> {
    Some(match glyph {
        // ── 树节点 ──
        Glyph::Folder => ZedIcon::Folder,
        Glyph::Db => ZedIcon::Database,
        // 模式是库下的命名空间容器：用立方体，别再用田字格（和表格 `Table` 撞脸）。
        Glyph::Schema => ZedIcon::Box,
        Glyph::Table => ZedIcon::Table,
        Glyph::View | Glyph::PreviewEye => ZedIcon::Eye,
        Glyph::MatView => ZedIcon::Layers,
        Glyph::DesignView | Glyph::DesignMatView | Glyph::DesignTable | Glyph::Pencil => {
            ZedIcon::Pencil
        }
        Glyph::ViewBuilder | Glyph::QueryBuilder => ZedIcon::Blocks,
        Glyph::Function => ZedIcon::Code,
        Glyph::Query => ZedIcon::FileCode,
        Glyph::User | Glyph::RibbonUser => ZedIcon::Person,
        Glyph::Column => ZedIcon::Hash,
        Glyph::IndexGroup => ZedIcon::ListTree,
        Glyph::ForeignKeyGroup => ZedIcon::Link,
        Glyph::UniqueKeyGroup => ZedIcon::StarFilled,
        Glyph::CheckGroup => ZedIcon::Check,
        Glyph::ExcludeGroup => ZedIcon::SquareMinus,
        Glyph::RuleGroup => ZedIcon::ListTodo,
        Glyph::TriggerGroup => ZedIcon::BoltOutlined,
        // ── 功能区 / 对象工具条 ──
        Glyph::RibbonConn => ZedIcon::Plug,
        Glyph::RibbonNewQuery => ZedIcon::FileCode,
        Glyph::RibbonQuery => ZedIcon::Code,
        Glyph::RibbonTable => ZedIcon::Table,
        Glyph::RibbonView => ZedIcon::Eye,
        Glyph::RibbonMatView => ZedIcon::Layers,
        Glyph::RibbonFunction => ZedIcon::BoltOutlined,
        Glyph::RibbonTools => ZedIcon::Settings,
        Glyph::RibbonBackup => ZedIcon::Archive,
        Glyph::RibbonAutoRun => ZedIcon::Clock,
        Glyph::OpenTable => ZedIcon::BoxOpen,
        Glyph::NewTable => ZedIcon::Plus,
        Glyph::AiTable | Glyph::AskAi => ZedIcon::Sparkle,
        Glyph::DropTable => ZedIcon::Trash,
        Glyph::ImportWizard => ZedIcon::Download,
        Glyph::SaveFile => ZedIcon::Save,
        Glyph::ExportWizard => ZedIcon::Upload,
        Glyph::Txn => ZedIcon::ArrowRightLeft,
        Glyph::CellEdit => ZedIcon::Pencil,
        Glyph::FilterSort => ZedIcon::FilterFunnel,
        Glyph::Columns => ZedIcon::Columns,
        Glyph::History => ZedIcon::HistoryRerun,
        Glyph::Beautify => ZedIcon::Wand,
        Glyph::Snippet | Glyph::CodeSnippet => ZedIcon::TextSnippet,
        Glyph::PlayOutline => ZedIcon::PlayOutlined,
        Glyph::StopOutline => ZedIcon::Stop,
        Glyph::ExplainPlan => ZedIcon::ListTree,
        Glyph::KeyGold => ZedIcon::Key,
        // ── 一次性工具条按钮 ──
        Glyph::Asset(id) => match id {
            "TB_New" => ZedIcon::Plus,
            "TB_Delete" => ZedIcon::Trash,
            "TB_Design" => ZedIcon::Pencil,
            "BTN_SEARCH" => ZedIcon::MagnifyingGlass,
            "BTN_COPY_TO_CLIPBOARD" => ZedIcon::Copy,
            "SORT" => ZedIcon::ChevronUpDown,
            "OV_Information" => ZedIcon::Info,
            "OV_Event" => ZedIcon::Clock,
            "TB_Tools" => ZedIcon::Settings,
            "TB_Save_To_Disk" => ZedIcon::Save,
            "REGISTRATION_TICK" => ZedIcon::Check,
            "OV_Share" => ZedIcon::Share,
            "OV_Model_Image" => ZedIcon::Image,
            "MSG_ERROR" => ZedIcon::XCircle,
            "FILTER_MOVE_UP" => ZedIcon::ArrowUp,
            "FILTER_MOVE_DOWN" => ZedIcon::ArrowDown,
            "TB_User_Privileges_Manager" | "TB_User" => ZedIcon::Person,
            "TB_Table_Profile" => ZedIcon::Table,
            "OBJECT_STAR" => ZedIcon::Star,
            "TB_Starred" => ZedIcon::StarFilled,
            "TB_Unstarred" => ZedIcon::Star,
            "OV_ASCENDING" => ZedIcon::ArrowUp,
            "OV_DESCENDING" => ZedIcon::ArrowDown,
            "TB_Schema_Analysis" => ZedIcon::Blocks,
            "TB_Refresh" => ZedIcon::RotateCw,
            "TB_Filter_Sort" => ZedIcon::FilterFunnel,
            "TB_Data_Masking" => ZedIcon::EyeOff,
            "TB_Data_Generation" => ZedIcon::Sparkle,
            "REVEAL_ACTIVE_TAB" => ZedIcon::Eye,
            "OV_Validation" => ZedIcon::ListTodo,
            "OV_Question" => ZedIcon::CircleHelp,
            "OV_Model_Shape" => ZedIcon::Blocks,
            "FILTER" => ZedIcon::Filter,
            "COLLAPSE_ALL" => ZedIcon::FoldVertical,
            _ => return None,
        },
        Glyph::Lucide(name) => return zed_for_lucide(name),
        Glyph::Zed(icon) => icon,
        // 品牌连接 / 信息卡大图 / 引擎图标交给 `db_icons`。
        Glyph::Conn(_)
        | Glyph::HeroDb
        | Glyph::HeroSchema
        | Glyph::HeroTable
        | Glyph::HeroView
        | Glyph::HeroMatView
        | Glyph::HeroFunction
        | Glyph::HeroQuery
        | Glyph::HeroFolder
        | Glyph::HeroEngine(_) => return None,
    })
}

/// Lucide 名字到 Zed 图标的映射；Zed 没有对应字形的返回 None 继续用 Lucide。
fn zed_for_lucide(name: IconName) -> Option<ZedIcon> {
    Some(match name {
        IconName::ArrowDownLeft => ZedIcon::ArrowDown,
        IconName::ArrowDownWideNarrow => ZedIcon::Sort,
        IconName::ArrowUpDown => ZedIcon::ChevronUpDown,
        IconName::ArrowUpRight => ZedIcon::ArrowUpRight,
        IconName::Blocks | IconName::Boxes | IconName::Layers | IconName::Shapes => {
            ZedIcon::Blocks
        }
        IconName::CalendarClock => ZedIcon::Clock,
        IconName::ChartColumn => ZedIcon::ChartBar,
        IconName::Check => ZedIcon::Check,
        IconName::ChevronDown => ZedIcon::ChevronDown,
        IconName::ChevronFirst => ZedIcon::SkipBack,
        IconName::ChevronLast => ZedIcon::SkipForward,
        IconName::ChevronLeft => ZedIcon::ChevronLeft,
        IconName::ChevronRight => ZedIcon::ChevronRight,
        IconName::ChevronUp => ZedIcon::ChevronUp,
        IconName::ChevronsDownUp => ZedIcon::FoldVertical,
        IconName::Circle => ZedIcon::Circle,
        IconName::CircleQuestionMark => ZedIcon::CircleHelp,
        IconName::Copy => ZedIcon::Copy,
        IconName::Database => ZedIcon::Database,
        IconName::DatabaseBackup => ZedIcon::Archive,
        IconName::Download => ZedIcon::Download,
        IconName::Ellipsis => ZedIcon::Ellipsis,
        IconName::ExternalLink | IconName::Link2 => ZedIcon::Link,
        IconName::Eye => ZedIcon::Eye,
        IconName::FilePlus => ZedIcon::FilePlus,
        IconName::Folder | IconName::FolderClosed => ZedIcon::Folder,
        IconName::FolderOpen => ZedIcon::FolderOpen,
        IconName::Funnel | IconName::ListFilter => ZedIcon::FilterFunnel,
        IconName::Image => ZedIcon::Image,
        IconName::Info => ZedIcon::Info,
        IconName::KeyRound => ZedIcon::Key,
        IconName::ListChecks => ZedIcon::ListTodo,
        IconName::Maximize2 => ZedIcon::Maximize,
        IconName::Minimize2 => ZedIcon::Minimize,
        IconName::MousePointerClick => ZedIcon::CursorIBeam,
        IconName::Pencil => ZedIcon::Pencil,
        IconName::Pin => ZedIcon::Pin,
        IconName::PinOff => ZedIcon::Unpin,
        IconName::Play => ZedIcon::PlayOutlined,
        IconName::Plug => ZedIcon::Plug,
        IconName::Plus => ZedIcon::Plus,
        IconName::RefreshCw => ZedIcon::RotateCw,
        IconName::Search => ZedIcon::MagnifyingGlass,
        IconName::Settings | IconName::SlidersHorizontal => ZedIcon::Settings,
        IconName::Share2 => ZedIcon::Share,
        IconName::SquareFunction => ZedIcon::Code,
        IconName::SquareStop => ZedIcon::Stop,
        IconName::SquareTerminal => ZedIcon::Terminal,
        IconName::Star => ZedIcon::Star,
        IconName::Table | IconName::Table2 => ZedIcon::Table,
        IconName::User => ZedIcon::Person,
        IconName::Users => ZedIcon::UserGroup,
        IconName::WindowClose => ZedIcon::GenericClose,
        IconName::WindowMaximize => ZedIcon::GenericMaximize,
        IconName::WindowMinimize => ZedIcon::GenericMinimize,
        IconName::WindowRestore => ZedIcon::GenericRestore,
        IconName::X => ZedIcon::Close,
        IconName::Zap => ZedIcon::BoltOutlined,
        // Zed 图标库没有太阳 / 月亮字形，保留 Lucide。
        IconName::Sun | IconName::Moon => return None,
        _ => return None,
    })
}

/// 布局类按钮只有资源名、没有 Zed 字形，退回 Lucide 线稿。
fn lucide_for_asset(id: &str) -> Option<IconName> {
    Some(match id {
        "GLYPH_GRIDVIEW" | "GLYPH_GRIDVIEW_DARK" => IconName::LayoutGrid,
        "GLYPH_LISTVIEW" | "GLYPH_LISTVIEW_DARK" => IconName::LayoutList,
        "GLYPH_DETAILVIEW" | "GLYPH_DETAILVIEW_DARK" => IconName::Rows3,
        "GLYPH_FORMVIEW" | "GLYPH_FORMVIEW_DARK" => IconName::TextCursorInput,
        "GLYPH_LEFTPANEL" | "GLYPH_LEFTPANEL_DARK" => IconName::PanelLeft,
        "GLYPH_RIGHTPANEL" | "GLYPH_RIGHTPANEL_DARK" => IconName::PanelRight,
        _ => return None,
    })
}

/// 画一个图标，返回定宽定高的元素。
pub fn art(glyph: Glyph, size: f32) -> AnyElement {
    if let Some(z) = zed_for(glyph) {
        return Icon::new(z).with_size(px(size)).into_any_element();
    }
    match glyph {
        Glyph::Conn(kind) => db_icons::engine(kind, size),
        // 信息卡 64px 大图与树节点同一套字形，不再按像素档准备位图。
        Glyph::HeroDb => db_icons::database_hero(size),
        Glyph::HeroSchema => db_icons::hero_schema(size),
        Glyph::HeroTable => db_icons::hero_table(size),
        Glyph::HeroView => db_icons::hero_view(size),
        Glyph::HeroMatView => db_icons::hero_matview(size),
        Glyph::HeroFunction => db_icons::hero_function(size),
        Glyph::HeroQuery => db_icons::hero_query(size),
        Glyph::HeroFolder => db_icons::hero_folder(size),
        Glyph::HeroEngine(kind) => db_icons::hero_engine(kind, size),
        Glyph::Lucide(name) => Icon::new(name).with_size(px(size)).into_any_element(),
        Glyph::Asset(id) => match lucide_for_asset(id) {
            Some(name) => Icon::new(name).with_size(px(size)).into_any_element(),
            None => div().flex_none().w(px(size)).h(px(size)).into_any_element(),
        },
        // 未登记的 Asset 名没有对应字形，留空位。
        _ => div().flex_none().w(px(size)).h(px(size)).into_any_element(),
    }
}

/// 有主题色的图标：统一走同一套字形，颜色由调用方给。
pub fn icon(glyph: Glyph, size: f32, tint: Hsla) -> AnyElement {
    if let Some(z) = zed_for(glyph) {
        return zed_icons::ic(z, size, tint);
    }
    match glyph {
        Glyph::Lucide(name) => Icon::new(name)
            .with_size(px(size))
            .text_color(tint)
            .into_any_element(),
        Glyph::Asset(id) => match lucide_for_asset(id) {
            Some(name) => Icon::new(name)
                .with_size(px(size))
                .text_color(tint)
                .into_any_element(),
            None => art(glyph, size),
        },
        other => art(other, size),
    }
}
