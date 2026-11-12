//! 全应用统一的控件尺寸与控件构造。
//!
//! 尺寸与配色对齐 Zed（crates/ui）：按钮 Default 22 / Medium 28、
//! 输入框 24、列表行 22、页签条 32、面板 1px 边框、圆角 4。
//! Zed 的组件绑定它自家 gpui 与 theme crate，这里只搬设计数值与语义色，
//! 用 gpui-kit 的控件复刻；需要新尺寸先在这里加常量。

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Sizable as _, Size as ComponentSize, TitleBar, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::palette::Palette;
use crate::to_hsla;
use crate::zed_icons::{self, ZedIcon};

/// 标准按钮高度（Zed ButtonSize::Medium）
pub const CONTROL_H: f32 = 28.;
/// 标准按钮水平留白
pub const CONTROL_PAD_X: f32 = 14.;
/// 底栏按钮的最小宽度
pub const BUTTON_MIN_W: f32 = 64.;
/// 紧凑控件高度（Zed ButtonSize::Default）
pub const COMPACT_H: f32 = 22.;
/// 紧凑控件水平留白
pub const COMPACT_PAD_X: f32 = 8.;
/// 面板 / 对话框标题栏高度
pub const TITLE_H: f32 = 32.;
/// 对话框底栏高度
pub const FOOTER_H: f32 = 44.;
/// 对话框内列表行高（Zed 列表行）
pub const DIALOG_ROW_H: f32 = 22.;
/// 表单左侧标签列宽
pub const LABEL_W: f32 = 64.;
/// 表单输入框总高
pub const FIELD_H: f32 = 24.;
/// 面板描边
pub const HAIRLINE: f32 = 1.;
/// 圆形单选 / 方形复选的边长
pub const CONTROL_DOT: f32 = 14.;
/// 圆角（Zed rounded_md）
pub const RADIUS: f32 = 4.;
/// 页签条总高
pub const TAB_STRIP_H: f32 = 32.;
/// 页签高
pub const TAB_H: f32 = 32.;
/// 页签左右留白
pub const TAB_PAD_X: f32 = 11.;

/// 主按钮（确定 / 下一步 / 导入）：Zed `TintColor::Accent` —— info.background 淡蓝底、
/// info.border 浅蓝边、正文色文字；hover 压暗一档，不是实心品牌蓝配白字。
pub fn primary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    dark: bool,
    cx: &App,
) -> Button {
    let p = Palette::of(dark);
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(to_hsla(p.primary_face))
                .foreground(to_hsla(p.foreground))
                .hover(to_hsla(p.primary_face_hover))
                .active(to_hsla(p.primary_face)),
        )
        .label(label)
        .h(px(CONTROL_H))
        .min_w(px(BUTTON_MIN_W))
        .px(px(CONTROL_PAD_X))
        .text_size(px(13.))
        .rounded(px(RADIUS))
        .bg(to_hsla(p.primary_face))
        .border(px(HAIRLINE))
        .border_color(to_hsla(p.primary_border))
}

/// 次按钮（取消 / 关闭 / 浏览…）：Zed Subtle —— 透明底，hover 铺 element.hover。
pub fn secondary_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id)
        .ghost()
        .label(label)
        .h(px(CONTROL_H))
        .min_w(px(BUTTON_MIN_W))
        .px(px(CONTROL_PAD_X))
        .text_size(px(13.))
        .rounded(px(RADIUS))
}

/// 紧凑按钮（列表工具条：全选 / 取消全选 / 分隔符选择）
pub fn compact_button(id: impl Into<ElementId>, label: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .label(label)
        .h(px(COMPACT_H))
        .px(px(COMPACT_PAD_X))
        .text_size(px(12.))
        .rounded(px(RADIUS))
}

/// 对话框窗口选项：默认模态——子窗口开着时主窗口锁定，关掉才能继续操作。
pub fn dialog_options() -> WindowOptions {
    let mut options = TitleBar::window_options();
    options.kind = WindowKind::Dialog;
    options
}

/// 对话框内容面：面板底，直接贴窗口边，不画外框。
pub fn dialog_surface(dark: bool) -> Div {
    let p = Palette::of(dark);
    div()
        .flex_1()
        .min_h_0()
        .bg(to_hsla(p.dialog_surface))
}

/// 对话框底栏：与内容面同色，顶部 1px 分割线。
pub fn footer_bar(dark: bool) -> Div {
    let p = Palette::of(dark);
    h_flex()
        .h(px(FOOTER_H))
        .px(px(12.))
        .gap(px(8.))
        .items_center()
        .bg(to_hsla(p.dialog_surface))
        .border_t(px(HAIRLINE))
        .border_color(to_hsla(p.dialog_line))
}

/// 表单标签：与输入列左对齐，冒号由调用方给出。
pub fn form_label(text: impl Into<SharedString>, muted: Hsla) -> impl IntoElement {
    div()
        .w(px(LABEL_W))
        .text_xs()
        .text_color(muted)
        .child(text.into())
}

/// 对话框表单标签：固定列宽、左对齐、深色正文（参考图里的「连接名称:」）。
pub fn dialog_label(text: impl Into<SharedString>, dark: bool, width: f32) -> impl IntoElement {
    let p = Palette::of(dark);
    div()
        .w(px(width))
        .flex_none()
        .text_size(px(12.))
        .text_color(to_hsla(p.foreground))
        .child(text.into())
}

/// 对话框输入框：Zed 输入框（element 底、1px 描边、聚焦换 focused 边色），
/// 内部仍用组件库的 Input —— 文本编辑、选区、快捷键都不另造。
pub fn field_input(
    state: &Entity<InputState>,
    dark: bool,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let p = Palette::of(dark);
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    let edge = to_hsla(if focused { p.field_focus } else { p.field_border });
    let input = Input::new(state)
        .appearance(false)
        .bordered(false)
        .focus_bordered(false)
        .with_size(ComponentSize::XSmall);
    // 输入框比面板深一档（Zed 输入框压 editor 底），同色会糊成一片显得脏
    div()
        .h(px(FIELD_H))
        .w_full()
        .flex()
        .items_center()
        .bg(to_hsla(p.background))
        .border(px(HAIRLINE))
        .border_color(edge)
        .rounded(px(RADIUS))
        .child(Styled::h(input, px(FIELD_H - HAIRLINE * 2.)))
        .into_any_element()
}

/// 单选：14px 圆，选中实心强调色 + 中心点，未选中 1px 描边。
pub fn radio(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    enabled: bool,
    dark: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let p = Palette::of(dark);
    let label = label.into();
    let dot = if checked {
        div()
            .flex_none()
            .size(px(CONTROL_DOT))
            .rounded_full()
            .bg(to_hsla(p.control_fill))
            .flex()
            .items_center()
            .justify_center()
            .child(div().size(px(5.)).rounded_full().bg(to_hsla(p.on_accent)))
    } else {
        div()
            .flex_none()
            .size(px(CONTROL_DOT))
            .rounded_full()
            .bg(to_hsla(p.background))
            .border(px(1.))
            .border_color(to_hsla(p.control_ring))
    };
    h_flex()
        .id(id)
        .h(px(22.))
        .gap(px(6.))
        .items_center()
        .when(enabled, |s| s.cursor_pointer().on_click(on_click))
        .when(!enabled, |s| s.opacity(0.4))
        .child(dot)
        .when(!label.is_empty(), |s| {
            s.child(
                div()
                    .text_size(px(13.))
                    .text_color(to_hsla(p.foreground))
                    .child(label.clone()),
            )
        })
        .into_any_element()
}

/// 复选：14px 方块，选中实心强调色 + 白勾。
pub fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    enabled: bool,
    dark: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let p = Palette::of(dark);
    let label = label.into();
    let box_ = if checked {
        div()
            .flex_none()
            .size(px(CONTROL_DOT))
            .rounded(px(3.))
            .bg(to_hsla(p.control_fill))
            .flex()
            .items_center()
            .justify_center()
            .child(zed_icons::ic(ZedIcon::Check, 10., to_hsla(p.on_accent)))
    } else {
        div()
            .flex_none()
            .size(px(CONTROL_DOT))
            .rounded(px(3.))
            .bg(to_hsla(p.background))
            .border(px(1.))
            .border_color(to_hsla(p.control_ring))
    };
    h_flex()
        .id(id)
        .h(px(22.))
        .gap(px(6.))
        .items_center()
        .when(enabled, |s| s.cursor_pointer().on_click(on_click))
        .when(!enabled, |s| s.opacity(0.4))
        .child(box_)
        .when(!label.is_empty(), |s| {
            s.child(
                div()
                    .text_size(px(13.))
                    .text_color(to_hsla(p.foreground))
                    .child(label.clone()),
            )
        })
        .into_any_element()
}

/// Zed 页签条：未激活页签压在 tab bar 底上，激活页签与内容面同色并盖住底边线。
pub fn page_tabs(tabs: &[&'static str], active: usize, dark: bool) -> impl IntoElement {
    let p = Palette::of(dark);
    let line = to_hsla(p.dialog_line);
    h_flex()
        .h(px(TAB_STRIP_H))
        .items_end()
        .bg(to_hsla(p.tab_face))
        .border_b(px(HAIRLINE))
        .border_color(line)
        .children(tabs.iter().enumerate().map(|(ix, tab)| {
            let on = ix == active;
            div()
                .h(px(TAB_H))
                .px(px(TAB_PAD_X))
                .flex()
                .items_center()
                .text_size(px(13.))
                .text_color(to_hsla(if on { p.foreground } else { p.muted_foreground }))
                .bg(to_hsla(if on { p.background } else { p.tab_face }))
                .when(on, |s| {
                    s.child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .right(px(0.))
                            .bottom(px(-HAIRLINE))
                            .h(px(HAIRLINE))
                            .bg(to_hsla(p.background)),
                    )
                })
                .child((*tab).to_string())
                .into_any_element()
        }))
}

/// 单选分段（格式 / 类型这类二选一）：选中铺 selection 底，未选中 hover。
pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    dark: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let p = Palette::of(dark);
    let label = label.into();
    div()
        .id(id)
        .h(px(24.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(RADIUS))
        .text_size(px(13.))
        .cursor_pointer()
        .text_color(to_hsla(if selected { p.foreground } else { p.muted_foreground }))
        .when(selected, move |s| s.bg(to_hsla(p.selection)))
        .when(!selected, move |s| s.hover(move |s| s.bg(to_hsla(p.hover))))
        .on_click(on_click)
        .child(label)
}

/// 把对话框的客户区校正成设计尺寸；返回 true 表示已由原生窗口接管。
///
/// gpui-pre 在 Windows 上创建 Dialog 窗口时，`CreateWindowExW` 之后、真正定位之前
/// 就量了一次边框：那一刻 GetClientRect 反而比 GetWindowRect 大，算出的负
/// height_offset 被缓存下来，之后 SetWindowPlacement / resize 都按它把窗口缩小。
/// 设计 402×183 的「另存为」实际只有 389×147.5 逻辑像素，底部按钮行被裁掉。
/// 这里按真实客户区补足物理像素差（与 DPI 无关），必须在窗口尺寸变化后同步调用，
/// 否则会先按旧视口排一帧。测试窗口拿不到原生句柄时返回 false，调用方照常用
/// `window.resize`。
#[cfg(target_os = "windows")]
pub fn fit_dialog_client(window: &Window, design: (Pixels, Pixels)) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
    };

    let Ok(handle) = <Window as HasWindowHandle>::window_handle(window) else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = win32.hwnd.get() as HWND;
    let scale = window.scale_factor();
    let want_w = (design.0.as_f32() * scale).round() as i32;
    let want_h = (design.1.as_f32() * scale).round() as i32;
    unsafe {
        let mut client = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetClientRect(hwnd, &mut client) == 0 || GetWindowRect(hwnd, &mut rect) == 0 {
            return false;
        }
        let dw = want_w - (client.right - client.left);
        let dh = want_h - (client.bottom - client.top);
        if dw == 0 && dh == 0 {
            return true;
        }
        SetWindowPos(
            hwnd,
            core::ptr::null_mut(),
            0,
            0,
            (rect.right - rect.left) + dw,
            (rect.bottom - rect.top) + dh,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    true
}

#[cfg(not(target_os = "windows"))]
pub fn fit_dialog_client(_window: &Window, _design: (Pixels, Pixels)) -> bool {
    false
}

/// 把浮层窗口的客户区左上角对齐到锚窗口客户区 + 偏移（逻辑像素）的屏幕位置。
///
/// gpui-pre 在 Windows 上按「外框 = 给定 bounds 再减一整圈边框偏移」建窗，客户区
/// 实际落在目标左上半圈边框处（200% DPI 下差 6.5×17.5 逻辑像素），浮层会盖住自己
/// 的触发器。建窗后用真实句柄量一次客户区原点再搬回去；测试窗口拿不到句柄时不动。
#[cfg(target_os = "windows")]
pub fn place_client_at_anchor(
    window: &Window,
    anchor: &Window,
    offset: (f32, f32),
) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    };

    let hwnd_of = |w: &Window| -> Option<HWND> {
        let handle = <Window as HasWindowHandle>::window_handle(w).ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(win32) => Some(win32.hwnd.get() as HWND),
            _ => None,
        }
    };
    let (Some(hwnd), Some(anchor_hwnd)) = (hwnd_of(window), hwnd_of(anchor)) else {
        return false;
    };
    let scale = window.scale_factor();
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if ClientToScreen(anchor_hwnd, &mut pt) == 0 {
            return false;
        }
        let target_x = pt.x + (offset.0 * scale).round() as i32;
        let target_y = pt.y + (offset.1 * scale).round() as i32;
        let mut here = POINT { x: 0, y: 0 };
        if ClientToScreen(hwnd, &mut here) == 0 {
            return false;
        }
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return false;
        }
        let x = rect.left + (target_x - here.x);
        let y = rect.top + (target_y - here.y);
        if x == rect.left && y == rect.top {
            return true;
        }
        SetWindowPos(
            hwnd,
            core::ptr::null_mut(),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        ) != 0
    }
}

#[cfg(not(target_os = "windows"))]
pub fn place_client_at_anchor(_window: &Window, _anchor: &Window, _offset: (f32, f32)) -> bool {
    false
}

/// Win11 的 DWM 默认给顶层窗口圆角、还描一圈 1px 窗口边线；浮层列表是独立窗口，
/// 两者都会叠在组合框面板自己的 0.5px 描边上。这里关掉圆角并去掉 DWM 边线，
/// 浮层最终只剩面板那 0.5px 的边。测试窗口拿不到句柄时不动。
#[cfg(target_os = "windows")]
pub fn square_corners(window: &Window) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };

    let Ok(handle) = <Window as HasWindowHandle>::window_handle(window) else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = win32.hwnd.get() as HWND;
    let pref = DWMWCP_DONOTROUND;
    let border = DWMWA_COLOR_NONE;
    unsafe {
        let square = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &pref as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&pref) as u32,
        ) == 0;
        let no_border = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            &border as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&border) as u32,
        ) == 0;
        square && no_border
    }
}

#[cfg(not(target_os = "windows"))]
pub fn square_corners(_window: &Window) -> bool {
    false
}

/// Win11 只给普通顶层窗口圆角，工具窗（`WindowKind::PopUp`）默认是方角。
/// 主题面板要像 Zed 的命令面板那样是圆角卡片：显式要求 DWM 圆角，同时去掉
/// DWM 自带的窗口边线——卡片自己的 1px 描边画在客户区里。
#[cfg(target_os = "windows")]
pub fn round_corners(window: &Window) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };

    let Ok(handle) = <Window as HasWindowHandle>::window_handle(window) else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = win32.hwnd.get() as HWND;
    let pref = DWMWCP_ROUND;
    let border = DWMWA_COLOR_NONE;
    unsafe {
        let round = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &pref as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&pref) as u32,
        ) == 0;
        let no_border = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            &border as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&border) as u32,
        ) == 0;
        round && no_border
    }
}

#[cfg(not(target_os = "windows"))]
pub fn round_corners(_window: &Window) -> bool {
    false
}
