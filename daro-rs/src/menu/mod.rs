//! 应用自己的弹出菜单组件。
//!
//! 外壳来自组件库 `gpui-component` 的 `menu` 模块，观感按 Zed
//! `ui::ContextMenu` + `ListItem` 的口径重画（见 `popup_menu.rs` 里的几何常量）。
//! 行为（焦点、快捷键、子菜单、点击外部关闭）与上游保持一致。

use gpui_kit::{App, Hsla};

use crate::palette::Palette;
use crate::to_hsla;

mod context_menu;
mod dropdown_menu;
mod menu_item;
mod popup_menu;

pub use context_menu::ContextMenuExt;
pub use dropdown_menu::DropdownMenu;
pub use popup_menu::{PopupMenu, PopupMenuItem};

pub(crate) fn init(cx: &mut App) {
    popup_menu::init(cx);
}

/// 弹出菜单的语义色，对齐 Zed ContextMenu 用到的那几档：卡片底 =
/// `elevated_surface_background`、描边与分隔线 = `border.variant`；高亮三档对应
/// `ListItem` 的 ghost_element_hover / selected / active，即 element 同三档。
#[derive(Clone, Copy)]
pub(crate) struct MenuColors {
    pub bg: Hsla,
    pub border: Hsla,
    pub separator: Hsla,
    pub text: Hsla,
    pub disabled: Hsla,
    pub hover_bg: Hsla,
    pub selected_bg: Hsla,
    pub active_bg: Hsla,
}

pub(crate) fn menu_colors(is_dark: bool) -> MenuColors {
    let p = Palette::of(is_dark);
    MenuColors {
        bg: to_hsla(p.dialog_surface),
        border: to_hsla(p.border_soft),
        separator: to_hsla(p.border_soft),
        text: to_hsla(p.foreground),
        disabled: to_hsla(p.subtle_foreground),
        hover_bg: to_hsla(p.hover),
        selected_bg: to_hsla(p.selection),
        active_bg: to_hsla(p.selection_focus),
    }
}
