//! Zed 同款图标库。
//!
//! SVG 取自 Zed 仓库 `assets/icons`（单色控件字形）与 `assets/icons/file_icons`
//! （文件/数据库类型字形），按 16×16 线稿绘制，渲染时由 `text_color` 整体着色。
//! 资源路径统一为 `icons/zed/<name>.svg`，由 [`AppAssets`] 与 Lucide 资源合并提供。
//!
//! 数据库品牌图标（PostgreSQL / MySQL / SQLite）与信息卡大图仍走参考客户端位图，
//! 那属于内容标识而不是控件字形。

use std::borrow::Cow;

use gpui_kit::component::{Icon, IconNamed, Sizable as _};
use gpui_kit::*;

macro_rules! zed_icons {
    ($($variant:ident => $file:literal,)*) => {
        /// 常用 Zed 图标。
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum ZedIcon {
            $($variant,)*
        }

        impl ZedIcon {
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
}

const PREFIX: &str = "icons/zed/";

zed_icons! {
    Archive => "archive.svg",
    ArrowDown => "arrow_down.svg",
    ArrowDownRight => "arrow_down_right.svg",
    ArrowLeft => "arrow_left.svg",
    ArrowRight => "arrow_right.svg",
    ArrowRightLeft => "arrow_right_left.svg",
    ArrowUp => "arrow_up.svg",
    ArrowUpRight => "arrow_up_right.svg",
    Blocks => "blocks.svg",
    BoltOutlined => "bolt_outlined.svg",
    Box => "box.svg",
    BoxOpen => "box_open.svg",
    ChartBar => "chart_bar.svg",
    Check => "check.svg",
    ChevronDown => "chevron_down.svg",
    ChevronLeft => "chevron_left.svg",
    ChevronRight => "chevron_right.svg",
    ChevronUp => "chevron_up.svg",
    ChevronUpDown => "chevron_up_down.svg",
    Circle => "circle.svg",
    CircleHelp => "circle_help.svg",
    Clock => "clock.svg",
    Close => "close.svg",
    Code => "code.svg",
    Columns => "columns.svg",
    Copy => "copy.svg",
    CursorIBeam => "cursor_i_beam.svg",
    Dash => "dash.svg",
    Database => "database.svg",
    Download => "download.svg",
    Ellipsis => "ellipsis.svg",
    ExpandVertical => "expand_vertical.svg",
    Eye => "eye.svg",
    EyeOff => "eye_off.svg",
    File => "file.svg",
    FileCode => "file_code.svg",
    FilePlus => "file_plus.svg",
    Filter => "filter.svg",
    FilterFunnel => "filter_funnel.svg",
    FoldVertical => "fold_vertical.svg",
    Folder => "folder.svg",
    FolderOpen => "folder_open.svg",
    GenericClose => "generic_close.svg",
    GenericMaximize => "generic_maximize.svg",
    GenericMinimize => "generic_minimize.svg",
    GenericRestore => "generic_restore.svg",
    Github => "github.svg",
    Hash => "hash.svg",
    HistoryRerun => "history_rerun.svg",
    Image => "image.svg",
    Info => "info.svg",
    Key => "key.svg",
    Layers => "layers.svg",
    Link => "link.svg",
    ListTodo => "list_todo.svg",
    ListTree => "list_tree.svg",
    Lock => "lock.svg",
    MagnifyingGlass => "magnifying_glass.svg",
    Maximize => "maximize.svg",
    Menu => "menu.svg",
    Minimize => "minimize.svg",
    Pencil => "pencil.svg",
    Person => "person.svg",
    Pin => "pin.svg",
    PlayFilled => "play_filled.svg",
    PlayOutlined => "play_outlined.svg",
    Plug => "plug.svg",
    Plus => "plus.svg",
    Power => "power.svg",
    RotateCw => "rotate_cw.svg",
    Save => "save.svg",
    Server => "server.svg",
    Settings => "settings.svg",
    Share => "share.svg",
    SkipBack => "skip_back.svg",
    SkipForward => "skip_forward.svg",
    Sort => "sort.svg",
    Sparkle => "sparkle.svg",
    SquareMinus => "square_minus.svg",
    Star => "star.svg",
    StarFilled => "star_filled.svg",
    Stop => "stop.svg",
    Sun => "sun.svg",
    Moon => "moon.svg",
    Table => "table.svg",
    Terminal => "terminal.svg",
    TextSnippet => "text_snippet.svg",
    TextWrap => "text_wrap.svg",
    Trash => "trash.svg",
    Unpin => "unpin.svg",
    Upload => "upload.svg",
    UserGroup => "user_group.svg",
    Warning => "warning.svg",
    XCircle => "x_circle.svg",
    Wand => "wand.svg",
}

impl IconNamed for ZedIcon {
    fn path(self) -> SharedString {
        self.asset_path()
    }
}

impl RenderOnce for ZedIcon {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let text_style = window.text_style();
        svg()
            .path(self.asset_path())
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
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::AllAssets.list(path)?;
        paths.extend(all_paths().into_iter().filter(|name| name.starts_with(path)));
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
