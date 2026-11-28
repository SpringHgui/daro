//! Zed 和 Lucide 界面图标。
//!
//! SVG 取自 Zed 仓库 `assets/icons`（单色控件字形）与 `assets/icons/file_icons`
//! （文件/数据库类型字形），按 16×16 线稿绘制，渲染时由 `text_color` 整体着色。
//! 资源路径统一为 `icons/zed/<name>.svg`，由 [`AppAssets`] 与 Lucide 资源合并提供。
//!
//! 数据库品牌图标与信息卡大图不走这里，见 `db_icons`：品牌用厂商开源 SVG，
//! 信息卡大图直接复用本模块的控件字形。

use std::borrow::Cow;
use std::cell::Cell;

use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, IconNamed, Sizable as _};
use gpui_kit::*;

/// 界面图标主题与明暗配色独立保存。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconTheme {
    Lucide,
    Tabler,
    #[default]
    #[serde(other)]
    Zed,
}

thread_local! {
    // IconNamed 没有 App 参数，图标路径在 UI 线程上解析。
    static ACTIVE_THEME: Cell<IconTheme> = const { Cell::new(IconTheme::Zed) };
}

impl IconTheme {
    pub const ALL: [Self; 3] = [Self::Zed, Self::Lucide, Self::Tabler];

    pub fn label(self) -> &'static str {
        match self {
            // 这套是 daro 自己的默认字形（Zed 上游稿 + 按上游语言补的配对稿），
            // 菜单里就叫 daro，别挂 Zed 的名。
            Self::Zed => "daro（默认）",
            Self::Lucide => "Lucide",
            Self::Tabler => "Tabler",
        }
    }

    pub fn active() -> Self {
        ACTIVE_THEME.get()
    }

    pub fn activate(self) {
        ACTIVE_THEME.set(self);
    }
}

macro_rules! zed_icons {
    ($($variant:ident => $file:literal $(=> $lucide:ident)?,)*) => {
        /// 常用 Zed 图标。
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum ZedIcon {
            $($variant,)*
        }

        impl ZedIcon {
            /// 全部变体（宏按声明序生成）。
            pub const ALL: [Self; {[$(Self::$variant,)*].len()}] = [$(Self::$variant,)*];

            /// 资源文件名（含扩展名）。
            pub fn file(self) -> &'static str {
                match self {
                    $(Self::$variant => $file,)*
                }
            }

            /// AssetSource 路径。
            pub fn asset_path(self) -> SharedString {
                format!("{}{}", PREFIX, self.file()).into()
            }

            pub fn path_for(self, theme: IconTheme) -> SharedString {
                if theme == IconTheme::Lucide {
                    let alternate = match self {
                        $(Self::$variant => zed_icons!(@lucide $($lucide)?),)*
                    };
                    if let Some(icon) = alternate {
                        return icon.path();
                    }
                }
                // Tabler 按同名文件命中；缺文件回退 Zed 线稿。
                // 窗口控件是功能件，任何主题下都保持同一张图。
                if theme == IconTheme::Tabler
                    && !self.is_window_control()
                    && crate::tabler_assets::load_file(self.file()).is_some()
                {
                    return format!("icons/tabler/{}", self.file()).into();
                }
                self.asset_path()
            }

            /// 内嵌的 SVG 字节。
            pub fn bytes(self) -> &'static [u8] {
                match self {
                    $(Self::$variant => include_bytes!(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/assets/zed/",
                        $file
                    )),)*
                }
            }
        }

        /// 按资源路径取内嵌 SVG。
        pub fn load_asset(path: &str) -> Option<&'static [u8]> {
            let file = path.strip_prefix(PREFIX)?;
            Some(match file {
                $($file => include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/assets/zed/",
                    $file
                )),)*
                _ => return None,
            })
        }

        /// 全部内嵌资源路径。
        pub fn all_paths() -> Vec<SharedString> {
            vec![$(format!("{}{}", PREFIX, $file).into(),)*]
        }
    };
    (@lucide $icon:ident) => { Some(IconName::$icon) };
    (@lucide) => { None };
}

const PREFIX: &str = "icons/zed/";

zed_icons! {
    Archive => "archive.svg" => Archive,
    ArrowDown => "arrow_down.svg" => ArrowDown,
    ArrowDownRight => "arrow_down_right.svg" => ArrowDownRight,
    ArrowLeft => "arrow_left.svg" => ArrowLeft,
    ArrowRight => "arrow_right.svg" => ArrowRight,
    ArrowRightLeft => "arrow_right_left.svg" => ArrowRightLeft,
    ArrowUp => "arrow_up.svg" => ArrowUp,
    ArrowUpRight => "arrow_up_right.svg" => ArrowUpRight,
    Blocks => "blocks.svg" => Blocks,
    BoltOutlined => "bolt_outlined.svg" => Zap,
    Box => "box.svg" => Box,
    BoxOpen => "box_open.svg" => PackageOpen,
    ChartBar => "chart_bar.svg" => ChartColumn,
    Check => "check.svg" => Check,
    ChevronDown => "chevron_down.svg" => ChevronDown,
    ChevronLeft => "chevron_left.svg" => ChevronLeft,
    ChevronRight => "chevron_right.svg" => ChevronRight,
    ChevronUp => "chevron_up.svg" => ChevronUp,
    ChevronUpDown => "chevron_up_down.svg" => ChevronsUpDown,
    Circle => "circle.svg" => Circle,
    CircleHelp => "circle_help.svg" => CircleQuestionMark,
    Clock => "clock.svg" => Clock,
    Close => "close.svg" => X,
    Code => "code.svg" => CodeXml,
    Columns => "columns.svg" => Columns3,
    Copy => "copy.svg" => Copy,
    CursorIBeam => "cursor_i_beam.svg" => TextCursor,
    Dash => "dash.svg" => Minus,
    Database => "database.svg" => Database,
    DetailView => "detail_view.svg" => Rows3,
    // 选中态：Zed 的开关靠换一张图（同一几何、内部填充 0.1 ↔ 0.8），不是加底色。
    DetailViewActive => "detail_view_active.svg" => Rows3,
    Download => "download.svg" => Download,
    Ellipsis => "ellipsis.svg" => Ellipsis,
    ExpandVertical => "expand_vertical.svg" => ChevronsUpDown,
    Eye => "eye.svg" => Eye,
    EyeOff => "eye_off.svg" => EyeOff,
    File => "file.svg" => File,
    FileCode => "file_code.svg" => FileCode,
    FilePlus => "file_plus.svg" => FilePlus,
    Filter => "filter.svg" => ListFilter,
    FilterFunnel => "filter_funnel.svg" => Funnel,
    FoldVertical => "fold_vertical.svg" => ChevronsDownUp,
    Folder => "folder.svg" => Folder,
    FolderOpen => "folder_open.svg" => FolderOpen,
    FormView => "form_view.svg" => TextCursorInput,
    FormViewActive => "form_view_active.svg" => TextCursorInput,
    GenericClose => "generic_close.svg",
    GenericMaximize => "generic_maximize.svg",
    GenericMinimize => "generic_minimize.svg",
    GenericRestore => "generic_restore.svg",
    Github => "github.svg",
    GridView => "grid_view.svg" => LayoutGrid,
    GridViewActive => "grid_view_active.svg" => LayoutGrid,
    Hash => "hash.svg" => Hash,
    HistoryRerun => "history_rerun.svg" => ClockArrowUp,
    Image => "image.svg" => Image,
    Info => "info.svg" => Info,
    Key => "key.svg" => KeyRound,
    Layers => "layers.svg" => Layers,
    // Lucide 没有实心档：视图四件套的选中态在这一主题下沿用同一线稿，靠图标颜色分状态。
    LeftPanel => "left_panel.svg" => PanelLeftClose,
    LeftPanelActive => "left_panel_active.svg" => PanelLeftOpen,
    Link => "link.svg" => Link2,
    ListTodo => "list_todo.svg" => ListTodo,
    ListTree => "list_tree.svg" => ListTree,
    ListView => "list_view.svg" => LayoutList,
    ListViewActive => "list_view_active.svg" => LayoutList,
    Lock => "lock.svg" => Lock,
    MagnifyingGlass => "magnifying_glass.svg" => Search,
    Maximize => "maximize.svg" => Maximize2,
    Menu => "menu.svg" => Menu,
    Minimize => "minimize.svg" => Minimize2,
    Pencil => "pencil.svg" => Pencil,
    Person => "person.svg" => UserRound,
    Pin => "pin.svg" => Pin,
    PlayFilled => "play_filled.svg",
    PlayOutlined => "play_outlined.svg" => Play,
    Plug => "plug.svg" => Plug,
    Plus => "plus.svg" => Plus,
    Power => "power.svg" => Power,
    RotateCw => "rotate_cw.svg" => RotateCw,
    RightPanel => "right_panel.svg" => PanelRightClose,
    RightPanelActive => "right_panel_active.svg" => PanelRightOpen,
    Save => "save.svg" => Save,
    Server => "server.svg" => Server,
    Settings => "settings.svg" => Settings,
    Share => "share.svg" => Share2,
    SkipBack => "skip_back.svg" => SkipBack,
    SkipForward => "skip_forward.svg" => SkipForward,
    Sort => "sort.svg" => ArrowDownWideNarrow,
    Sparkle => "sparkle.svg" => Sparkles,
    SquareMinus => "square_minus.svg" => SquareMinus,
    Star => "star.svg" => Star,
    StarFilled => "star_filled.svg",
    Stop => "stop.svg" => SquareStop,
    Sun => "sun.svg" => Sun,
    Moon => "moon.svg" => Moon,
    Table => "table.svg" => Table2,
    Terminal => "terminal.svg" => SquareTerminal,
    TextSnippet => "text_snippet.svg" => FileText,
    TextWrap => "text_wrap.svg" => TextWrap,
    Trash => "trash.svg" => Trash,
    Unpin => "unpin.svg" => PinOff,
    Upload => "upload.svg" => Upload,
    UserGroup => "user_group.svg" => UsersRound,
    Warning => "warning.svg" => TriangleAlert,
    XCircle => "x_circle.svg" => CircleX,
    Wand => "wand.svg" => WandSparkles,
}

impl ZedIcon {
    /// 标题栏的最小化 / 最大化 / 还原 / 关闭：窗口功能件，不参与图标主题切换。
    /// Lucide 侧本来就没给配对名，Tabler 侧靠这里挡掉，免得重抓素材时又漂过去。
    pub fn is_window_control(self) -> bool {
        matches!(
            self,
            Self::GenericMinimize | Self::GenericMaximize | Self::GenericRestore | Self::GenericClose
        )
    }

    /// 选中态字形。Zed 的状态栏开关不铺底色，而是换成同一几何、内部填充更实的
    /// 那张图（上游 threads_sidebar_*_open / _closed 就是 0.8 与 0.1 两版）。
    pub fn active_variant(self) -> Option<Self> {
        Some(match self {
            Self::GridView => Self::GridViewActive,
            Self::ListView => Self::ListViewActive,
            Self::DetailView => Self::DetailViewActive,
            Self::FormView => Self::FormViewActive,
            Self::LeftPanel => Self::LeftPanelActive,
            Self::RightPanel => Self::RightPanelActive,
            _ => return None,
        })
    }

    /// 开关按钮按选中状态取图：有配对字形就换图，没有就原样（靠图标颜色分状态）。
    pub fn for_state(self, active: bool) -> Self {
        if active {
            self.active_variant().unwrap_or(self)
        } else {
            self
        }
    }
}

impl IconNamed for ZedIcon {
    fn path(self) -> SharedString {
        self.path_for(IconTheme::active())
    }
}

impl RenderOnce for ZedIcon {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let text_style = window.text_style();
        svg()
            .path(self.path())
            .flex_shrink_0()
            .size(text_style.font_size.to_pixels(window.rem_size()))
            .text_color(text_style.color)
    }
}

impl From<ZedIcon> for AnyElement {
    fn from(icon: ZedIcon) -> Self {
        Icon::new(icon).into_any_element()
    }
}

/// 指定尺寸与颜色的 Zed 图标。
pub fn ic(icon: ZedIcon, size: f32, color: Hsla) -> AnyElement {
    Icon::new(icon)
        .with_size(px(size))
        .text_color(color)
        .into_any_element()
}

/// 应用资源源：Zed 图标优先，其余回落到 GPUI Kit 的 Lucide 资源。
#[derive(Clone, Copy, Debug, Default)]
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = load_asset(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(bytes) = crate::tabler_assets::load(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::AllAssets.list(path)?;
        paths.extend(
            crate::tabler_assets::all_paths()
                .filter(|name| name.starts_with(path))
                .map(SharedString::from),
        );
        paths.extend(all_paths().into_iter().filter(|name| name.starts_with(path)));
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
