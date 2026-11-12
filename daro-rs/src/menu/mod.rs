//! 应用自己的弹出菜单组件。
//!
//! 源自组件库 `gpui-component` 的 `menu` 模块，观感换成 Zed One：
//! panel 底、1px border 外框、element.selected 高亮、紧凑行高。
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

/// 弹出菜单的语义色，跟随 Zed One：panel 底、border 外框、border_soft 分隔线、
/// subtle 置灰文字，高亮沿用 element.selected。
#[derive(Clone, Copy)]
pub(crate) struct MenuColors {
    pub bg: Hsla,
    pub border: Hsla,
    pub separator: Hsla,
    pub text: Hsla,
    pub disabled: Hsla,
    pub hover_bg: Hsla,
    pub hover_text: Hsla,
}

pub(crate) fn menu_colors(is_dark: bool) -> MenuColors {
    let p = Palette::of(is_dark);
    MenuColors {
        bg: to_hsla(p.dialog_surface),
        border: to_hsla(p.border),
        separator: to_hsla(p.border_soft),
        text: to_hsla(p.foreground),
        disabled: to_hsla(p.subtle_foreground),
        hover_bg: to_hsla(p.selection),
        hover_text: to_hsla(p.foreground),
    }
}
