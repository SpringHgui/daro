//! 全应用统一的控件尺寸与控件构造。
//!
//! 每个控件按 Zed `crates/ui` 里对应组件的配方复刻（尺寸、圆角、描边、三档
//! 高亮、图标间距全部取源码常量，Zed 默认 `ui_font_size` = 16 折算成 px）：
//! 按钮 `ButtonStyle` + `ButtonSize::Medium`、复选 `Checkbox`、输入框
//! `InputField`、标签 `Label`、键位 `KeyBinding`、页签 `Tab`/`TabBar`。
//! Zed 的组件绑定它自家 gpui 与 theme crate，这里用 gpui-kit 的控件复刻同一套
//! 数值与语义色；需要新尺寸先在这里加常量。
//!
//! 唯一有意偏离的是字号：Zed 控件正文走 `Label` Default = 14px，daro 全应用
//! 统一 13px（树 / 工具栏 / 网格的固定宽度都按 13 排过），所以控件沿用
//! [`UI_TEXT_SIZE`]，其余数值照抄。

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
    Sizable as _, Size as ComponentSize, TitleBar, h_flex, v_flex,
};
use gpui_kit::base::{Disableable as _, InteractiveElementExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::palette::Palette;
use crate::to_hsla;
use crate::zed_icons::{self, ZedIcon};

/// 工具栏、连接树和页签等主界面文字的字号。
pub const UI_TEXT_SIZE: f32 = 13.;

/// 标准按钮高度（Zed `ButtonSize::Medium` = `rems_from_px(28)`）
pub const CONTROL_H: f32 = 28.;
/// 标准按钮水平留白（Zed Medium / Large 档 `px(DynamicSpacing::Base08)` = 8px）
pub const CONTROL_PAD_X: f32 = 8.;
/// Zed 的按钮按内容收宽，没有最小宽度；留常量给仍想对齐宽度的调用方。
pub const BUTTON_MIN_W: f32 = 0.;
/// 紧凑按钮高度（Zed `ButtonSize::Compact` = 18px）
pub const COMPACT_H: f32 = 18.;
/// 紧凑控件水平留白（Zed Default / Compact 档 `px(Base04)` = 4px）
pub const COMPACT_PAD_X: f32 = 4.;
/// 面板 / 对话框标题栏高度（gpui-component `title_bar::TITLE_BAR_HEIGHT` = 34px，
/// `TitleBar` 不设高度时也是这一档；右侧关闭键取同值的正方形热区）
pub const TITLE_H: f32 = 34.;
/// 主界面工具条行高（树顶搜索条 / 对象动作条 / 主区页签条）：
/// 三者并排或被跨分栏的同一条底边线切开，必须共用同一档。
pub const TOOLBAR_H: f32 = 30.;
/// 对话框底栏高度（Zed `ModalFooter` = `p(Base08)` 8 + 按钮 28）
pub const FOOTER_H: f32 = 44.;
/// 对话框底栏左右留白（Zed footer `p(Base08)`）
pub const FOOTER_PAD: f32 = 8.;
/// 对话框内列表行高（Zed `ListItem` Dense：行高由 20 高内容撑出）
pub const DIALOG_ROW_H: f32 = 22.;
/// 表单左侧标签列宽
pub const LABEL_W: f32 = 64.;
/// 表单输入框总高（Zed `InputField` = `min_h_8` = 32px）
pub const FIELD_H: f32 = 32.;
/// 表单输入框水平留白（Zed `InputField` = `px_2`）
pub const FIELD_PAD_X: f32 = 8.;
/// 面板描边（Zed 全部 `border_1` = 1px）
pub const HAIRLINE: f32 = 1.;
/// 圆形单选 / 方形复选的边长（Zed `Checkbox` 内框 `size_4` = 16px）
pub const CONTROL_DOT: f32 = 16.;
/// 复选的点击热区（Zed `Checkbox::container_size` = 20px）
pub const CONTROL_HIT: f32 = 20.;
/// 复选框圆角（Zed `rounded_xs` = 2px）
pub const CONTROL_RADIUS: f32 = 2.;
/// 复选勾图标（Zed `IconSize::Small` = 14px）
pub const CONTROL_ICON: f32 = 14.;
/// 控件与标签的间距（Zed `gap(Base06)` = 6px）
pub const CONTROL_GAP: f32 = 6.;
/// 圆角（Zed `rounded_sm` = 4px，按钮 / 列表行 / Contained Section 同档）
pub const RADIUS: f32 = 4.;
/// 输入框圆角（Zed `rounded_md` = 6px）
pub const FIELD_RADIUS: f32 = 6.;
/// 卡片 / 浮层圆角（Zed `rounded_lg` = 8px，`elevation_2` / `elevation_3`）
pub const CARD_RADIUS: f32 = 8.;
/// 页签条总高（Zed `TabBar` = `DynamicSpacing::Base32` = 32px）
pub const TAB_STRIP_H: f32 = 32.;
/// 页签高（Zed 页签内容高 = Base32 - 1 条底边线）
pub const TAB_H: f32 = 32.;
/// 页签左右留白（Zed `Tab` 内容 `px(Base04)` = 4px）
pub const TAB_PAD_X: f32 = 4.;
/// 页签内图标 / 文字间距（Zed `gap(Base04)` = 4px）
pub const TAB_GAP: f32 = 4.;
/// 页签图标槽（Zed `TAB_START_SLOT_SIZE` = 12px）
pub const TAB_ICON_W: f32 = 12.;
/// 页签关闭按钮槽（Zed `TAB_END_SLOT_SIZE` = 14px）
pub const TAB_CLOSE_W: f32 = 14.;

// ---- 浮层卡片与列表行（Zed `ContextMenu` / `Popover` / `ListItem`）----
// 弹出菜单、组合框下拉、悬停浮层共用这一档，避免同一个浮层两套画法。

/// 卡片圆角（Zed `elevation_2` / `elevation_3` = `rounded_lg` = 8px）
pub const FLOAT_RADIUS: f32 = 8.;
/// 卡片最小宽（Zed `CONTEXT_MENU_MIN_WIDTH` = 200px）
pub const FLOAT_MIN_W: f32 = 200.;
/// 列表行外侧留白（Zed `ListItem` 的 inset 档 `Base04` = 4px，画在高亮盒外面）
pub const ROW_INSET: f32 = 4.;
/// 列表行内左右留白（Zed `ListItem` `px(Base06)` = 6px）
pub const ROW_PAD_X: f32 = 6.;
/// 列表行高亮圆角（Zed `rounded_sm` = 4px）
pub const ROW_RADIUS: f32 = 4.;
/// 树 / 列表每深一档的缩进（Zed `ListItem::indent_step` = 12px）
pub const INDENT_STEP: f32 = 12.;
/// 分隔线上下留白（Zed `ListSeparator` `my(Base06)` = 6px）
pub const SEPARATOR_GAP: f32 = 6.;

// ---- 状态栏（Zed `workspace::StatusBar`）----

/// 整条左右留白（Zed `p(Base04)` = 4px）
pub const STATUS_PAD_X: f32 = 4.;
/// 左右两群之间的间距（Zed `gap(Base08)` = 8px）
pub const STATUS_GAP: f32 = 8.;
/// 一群内部条目之间的间距（Zed `gap_1` = 4px）
pub const STATUS_INNER_GAP: f32 = 4.;

/// 竖分隔线：Zed `Divider::vertical().color(DividerColor::Border)` —— 1px 宽、
/// 16px 高（`h_4`），左右各留 6px（`mx_1p5`）。
pub fn divider_v(border: Hsla) -> Div {
    div()
        .flex_none()
        .w(px(HAIRLINE))
        .h(px(16.))
        .mx(px(6.))
        .bg(border)
}

/// 进度条：Zed `ProgressBar` = 8 高（`h_2`）+ `rounded_full` 轨道与填充，
/// 轨道是同色 20% 淡底。填充色由调用方给（Zed 用 `status().info`）。
pub fn progress_bar(id: impl Into<ElementId>, value: f32, fill: Hsla, loading: bool) -> Progress {
    Progress::new(id)
        .with_size(ComponentSize::Medium)
        .loading(loading)
        .value(value)
        .color(fill)
        .rounded(px(9999.))
}

/// 横分隔线：Zed `Divider::horizontal()` —— 1px 高（`h_px`）、通宽，
/// 上下各留 6px（`my_1p5`）。
pub fn divider_h(border: Hsla) -> Div {
    div()
        .flex_none()
        .w_full()
        .h(px(HAIRLINE))
        .my(px(6.))
        .bg(border)
}

/// 键帽：Zed `KeyBinding` 里的 `Key` —— `py_0p5` 2、`rounded_xs` 2，单字符键宽度
/// 等于字号并居中，多字符键左右 `px_0p5` 2，文字走 `Color::Muted`。
/// Zed 的键位不带描边，几枚键帽之间 `gap(Base04)` 4（见 [`KEY_GAP`]）。
pub fn key_cap(key: impl Into<SharedString>, muted: Hsla) -> Div {
    let key: SharedString = key.into();
    let single = key.chars().count() == 1;
    div()
        .flex_none()
        .py(px(2.))
        .rounded(px(CONTROL_RADIUS))
        .text_size(px(UI_TEXT_SIZE))
        .line_height(relative(1.))
        .text_color(muted)
        .when(single, |s| s.w(px(UI_TEXT_SIZE)).flex().justify_center())
        .when(!single, |s| s.px(px(2.)))
        .child(key)
}

/// 键帽之间的间距（Zed `KeyBinding` `gap(Base04)` = 4px）
pub const KEY_GAP: f32 = 4.;

/// 浮层投影：照抄 Zed `ElevationIndex::ElevatedSurface.shadow()` —— 一层
/// y=2 / blur=3 / 黑 12% 的主投影，加一层 y=1、不模糊的近身投影（浅色 3%、深色 6%）。
pub fn float_shadow(is_dark: bool) -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(2.), hsla(0., 0., 0., 0.12)).blur_radius(px(3.)),
        BoxShadow::new(
            px(0.),
            px(1.),
            hsla(0., 0., 0., if is_dark { 0.06 } else { 0.03 }),
        ),
    ]
}

/// 主按钮（确定 / 下一步 / 导入）：Zed `ButtonStyle::Tinted(TintColor::Accent)`
/// —— status.info.background 淡蓝面、status.info.border 描边、正文色文字，hover 按
/// Zed `darken(bg, 0.05, 0.2)` 压暗一档。尺寸走 Zed `ButtonSize::Medium`。
/// VS Code 2026 两档是实心 button.background，文字色由色板的 `primary_foreground` 给白字。
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
                .foreground(to_hsla(p.primary_foreground))
                .hover(to_hsla(p.primary_face_hover))
                .active(to_hsla(p.primary_face_hover)),
        )
        .label(label)
        .h(px(CONTROL_H))
        .px(px(CONTROL_PAD_X))
        .text_size(px(UI_TEXT_SIZE))
        .rounded(px(RADIUS))
        .bg(to_hsla(p.primary_face))
        .border(px(HAIRLINE))
        .border_color(to_hsla(p.primary_border))
}

/// 可禁用的主按钮：启用时与 [`primary_button`] 一致；禁用时退成次按钮的灰面
/// 加 muted 字。库里的禁用态文字固定是 `muted_foreground` 的 50%，又会被
/// 实例上的实心底盖回去，蓝底上读不出来，所以禁用配色在这里自己给。
/// Zed 的禁用档是 element_disabled + border_disabled + text_disabled。
pub fn primary_button_enabled(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    dark: bool,
    enabled: bool,
    cx: &App,
) -> Button {
    let button = primary_button(id, label, dark, cx);
    if enabled {
        return button;
    }
    let p = Palette::of(dark);
    button
        .disabled(true)
        .bg(to_hsla(p.button_face))
        .border_color(to_hsla(p.border_soft))
        .text_color(to_hsla(p.muted_foreground))
}

/// 次按钮（取消 / 关闭 / 浏览…）：Zed `ButtonStyle::Filled` —— element_background
/// 底、**无描边**、hover 把同一底色提亮一档、active 再压一档。底与 hover / active
/// 由 `apply_palette` 映射到色板的 button_face / hover / selection_focus。
pub fn secondary_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id)
        .with_variant(ButtonVariant::Default)
        .label(label)
        .h(px(CONTROL_H))
        .px(px(CONTROL_PAD_X))
        .text_size(px(UI_TEXT_SIZE))
        .rounded(px(RADIUS))
        .border_color(transparent_black())
}

/// 链接型操作（GitHub / Gitee / 通过 URI 新建…）：Zed `ButtonStyle::Subtle` ——
/// 无底面、hover 才铺 ghost_element_hover，文字取强调色。
pub fn link_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    dark: bool,
) -> Button {
    let p = Palette::of(dark);
    Button::new(id)
        .ghost()
        .label(label)
        .text_color(to_hsla(p.accent))
        .h(px(CONTROL_H))
        .px(px(CONTROL_PAD_X))
        .text_size(px(UI_TEXT_SIZE))
        .rounded(px(RADIUS))
}

/// 紧凑按钮（列表工具条：全选 / 取消全选 / 分隔符选择）：Zed `ButtonSize::Compact`
/// = 18 高 / `px(Base04)` 4，配 Subtle 档无底面。
pub fn compact_button(id: impl Into<ElementId>, label: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .label(label)
        .h(px(COMPACT_H))
        .px(px(COMPACT_PAD_X))
        .text_size(px(12.))
        .rounded(px(RADIUS))
}

/// 对话框窗口选项。
///
/// Windows / Linux：`WindowKind::Dialog` 是系统级模态——禁用父窗并设 owner，
/// 子窗口开着时主窗口锁定，关掉才能继续操作。
///
/// macOS：Dialog 会变成挂在主窗上的 sheet，而 sheet 天生没有红绿灯按钮。这里改用
/// 普通窗口（多窗口形态，不锁主窗）：可独立拖动 / 最小化，关闭走系统红绿灯。
pub fn dialog_options() -> WindowOptions {
    let mut options = TitleBar::window_options();
    options.kind = WindowKind::Normal;
    // 对话框一律自绘 ui::TITLE_H 高的标题栏，红绿灯按这个高度对齐（issue #25）
    align_traffic_lights(&mut options, TITLE_H);
    #[cfg(not(target_os = "macos"))]
    {
        options.kind = WindowKind::Dialog;
    }
    options
}

/// 自绘标题栏比默认 34 矮时，把 mac 红绿灯按栏高重新对齐（issue #25）。
///
/// `TitleBar::window_options()` 给的 `traffic_light_position = (9, 9)` 是按
/// 34 高的栏配的：y 是按钮的上下留白，按钮高 16，中心落在 9 + 8 = 17。
/// 新建连接 / 选项这类窗口自绘栏只有 24 高，标题文字中心在 12，红绿灯
/// 就整组低 5px。这里按 `(bar_h - 16) / 2` 重算留白，让按钮中心与文字同线。
pub fn align_traffic_lights(options: &mut WindowOptions, bar_h: f32) {
    #[cfg(target_os = "macos")]
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.traffic_light_position = Some(point(px(9.), px((bar_h - 16.) / 2.)));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (options, bar_h);
}

/// 建窗之后再设一次红绿灯位置（issue #25）。
///
/// `WindowOptions.titlebar.traffic_light_position` 只在原生窗创建时被
/// `move_traffic_light` 消费一次，窗口首次上屏 / 成为 key 窗后 AppKit 会按
/// 系统标题栏高度重排标准按钮，把位置顶回默认的 y=9——截图量出来按钮中心
/// 17.5px、24 高标题栏的文字中心 11px，就是这么错开的。渲染期重设走的是
/// `Window::set_traffic_light_position`，每次调用都重新按当前栏高摆放。
pub fn reapply_traffic_lights(window: &gpui_kit::Window, bar_h: f32) {
    #[cfg(target_os = "macos")]
    window.set_traffic_light_position(point(px(9.), px((bar_h - 16.) / 2.)));
    #[cfg(not(target_os = "macos"))]
    let _ = (window, bar_h);
}

/// 原生「打开文件」对话框。必须走异步版：rfd 的同步 API 会在点击回调里起自己的
/// 消息循环，期间投给 gpui 窗口的消息会重入并二次可变借用 App，进程直接 abort。
/// 异步版把对话框放到工作线程上显示，选完再回 UI 线程改状态。
pub fn pick_file<T: 'static>(
    window: &Window,
    dialog: rfd::AsyncFileDialog,
    cx: &mut Context<T>,
    on_pick: impl FnOnce(&mut T, &mut Window, &mut Context<T>, std::path::PathBuf) + 'static,
) {
    let dialog = dialog.set_parent(window);
    cx.spawn(async move |this, cx| {
        let Some(file) = dialog.pick_file().await else {
            return;
        };
        let path = file.path().to_path_buf();
        let _ = this.update_in(cx, |this, window, cx| on_pick(this, window, cx, path));
    })
    .detach();
}

/// 原生「保存文件」对话框，见 [`pick_file`]。
pub fn save_file<T: 'static>(
    window: &Window,
    dialog: rfd::AsyncFileDialog,
    cx: &mut Context<T>,
    on_save: impl FnOnce(&mut T, &mut Window, &mut Context<T>, std::path::PathBuf) + 'static,
) {
    let dialog = dialog.set_parent(window);
    cx.spawn(async move |this, cx| {
        let Some(file) = dialog.save_file().await else {
            return;
        };
        let path = file.path().to_path_buf();
        let _ = this.update_in(cx, |this, window, cx| on_save(this, window, cx, path));
    })
    .detach();
}

/// 关窗快捷键：Esc 或 Ctrl-W（macOS 的 ⌘W）。
pub fn is_close_keystroke(key: &Keystroke) -> bool {
    key.key == "escape"
        || (key.key.eq_ignore_ascii_case("w")
            && (key.modifiers.platform || key.modifiers.control)
            && !key.modifiers.alt
            && !key.modifiers.shift)
}

/// 子窗口统一的 Esc / Ctrl-W 关窗。挂在按键拦截器上而不是内容根的 `on_key_down`：
/// 无聚焦元素时按键只派发到窗口 Root，内容根收不到；聚焦输入框时 Esc 还可能被
/// 编辑器的 Escape 动作消费。拦截器跑在动作派发之前，按窗口句柄限定作用域。
///
/// Ctrl-W 必须在这里截住：它是全局的「关闭页签」，在对话框窗口里不拦就会去关
/// 背后主窗的页签，而对话框本身纹丝不动。
pub fn close_on_escape<T: 'static>(window: &Window, cx: &mut Context<T>) -> Subscription {
    let this_window = window.window_handle();
    cx.intercept_keystrokes(move |ev, window, cx| {
        if is_close_keystroke(&ev.keystroke) && window.window_handle() == this_window {
            cx.stop_propagation();
            window.remove_window();
        }
    })
}

/// 对话框内容面：面板底，直接贴窗口边，不画外框。
///
/// `w_full` + `min_w_0`：内容面本身按窗口宽定死，子级里的长文案才没有机会把整页
/// 撑得比窗口还宽（taffy 默认 flex-shrink 为 0，撑宽后右对齐的按钮就落到窗口外）。
pub fn dialog_surface(dark: bool) -> Div {
    let p = Palette::of(dark);
    div()
        .w_full()
        .min_w_0()
        .flex_1()
        .min_h_0()
        .bg(to_hsla(p.dialog_surface))
}

/// 对话框标题栏：Zed `ModalHeader` 的留白口径 —— 左右 `px(Base12)` 12、图标与标题
/// `gap(Base08)` 8、不画分隔线；标题走 `Headline small`（12px 半粗）；右侧关闭键取
/// gpui-component `TITLE_BAR_HEIGHT` 同值的正方形热区（34×34）、图标 `.small()` 14。
///
/// 外壳刻意不用 gpui-component 的 `TitleBar`：它会在右端连着画最小化 / 最大化 /
/// 关闭三枚窗口按钮（实测「选项」窗各 34×34），还会自带 12px 左边距叠加成 24；
/// 对话框只要一枚关闭键。拖动带沿用主窗口那套 —— `window_control_area(Drag)` 加
/// Windows 上补发 `WM_NCLBUTTONDOWN`，非 Windows 由移动事件自己交给系统。
pub fn dialog_title(
    title: impl Into<SharedString>,
    icon: Option<AnyElement>,
    dark: bool,
) -> AnyElement {
    let p = Palette::of(dark);
    h_flex()
        .id("title-bar")
        .test_support()
        .role(Role::TitleBar)
        .h(px(TITLE_H))
        .flex_none()
        .w_full()
        .items_center()
        // 不铺底色：标题条就是对话框根节点的最上一格，跟随它的面色，
        // 免得两条色带在标题栏下沿对不齐。
        .when(cfg!(target_os = "macos"), |s| s.pl(px(80.)))
        .child(
            h_flex()
                .id("title-bar-drag")
                .test_support()
                .h_full()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .window_control_area(WindowControlArea::Drag)
                .on_mouse_down(MouseButton::Left, |event, window, cx| {
                    // 双击标题栏 = 最大化 / 还原切换（#28），与主窗口同一条路：
                    // 不能只用 `zoom_window`，Windows 上它只发 SW_MAXIMIZE，
                    // 已最大化时再双击没有反应；gpui 也会把双击的第二下派发进
                    // 元素树，所以这里拦下自己切。
                    if event.click_count >= 2 {
                        crate::window_drag::toggle_zoom(window);
                        cx.stop_propagation();
                        return;
                    }
                    if event.click_count == 1 && crate::window_drag::begin_window_drag(window) {
                        cx.stop_propagation();
                    }
                })
                // 非 Windows：`window_control_area` 在 X11 / Wayland 是空操作，
                // macOS 也设了 `app_owns_titlebar_drag`，得自己把拖动交给系统。
                // 移动循环里 gpui 收不到事件，故按住的每次移动各自调用即可（等价于
                // 一次按住只触发一次）。
                .when(cfg!(not(target_os = "windows")), |s| {
                    s.on_mouse_move(|event: &MouseMoveEvent, window, _| {
                        if event.pressed_button == Some(MouseButton::Left) {
                            window.start_window_move();
                        }
                    })
                })
                .when_some(icon, |s, ic| s.child(ic))
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(to_hsla(p.foreground))
                        .child(title.into()),
                ),
        )
        .child(
            div()
                .id("title-bar-close")
                .test_support()
                .w(px(TITLE_H))
                .h(px(TITLE_H))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_color(to_hsla(p.foreground))
                .hover(|s| s.bg(to_hsla(p.hover)))
                .on_click(|_, window, _| window.remove_window())
                .child(zed_icons::ic(
                    ZedIcon::Close,
                    CONTROL_ICON,
                    to_hsla(p.foreground),
                )),
        )
        .into_any_element()
}

/// 对话框底栏：Zed `ModalFooter` —— 四周 `p(Base08)` 8、顶部 1px `border_variant`
/// 分割线、按钮之间 `gap_1` 4；高度由 28 高按钮 + 上下 8 撑成 44。
///
/// 宽度钉在窗口上（`w_full` + `min_w_0`）：底栏里的说明 / 报错文案是动态的，
/// 不钉住就会连着把整页撑宽。
pub fn footer_bar(dark: bool) -> Div {
    let p = Palette::of(dark);
    h_flex()
        .w_full()
        .min_w_0()
        .h(px(FOOTER_H))
        .px(px(FOOTER_PAD))
        .gap(px(4.))
        .items_center()
        .bg(to_hsla(p.dialog_surface))
        .border_t(px(HAIRLINE))
        .border_color(to_hsla(p.border_soft))
}

/// 空态：Zed `ProjectEmptyState` 的那一格 —— 外层 16 内距居中，内容列宽 192
/// （`w_48`）、行距 4，主说明用正文号 + muted 前景，副文案再降一档。
pub fn empty_state(title: impl Into<SharedString>, body: impl Into<SharedString>, dark: bool) -> Div {
    let p = Palette::of(dark);
    v_flex()
        .size_full()
        .p(px(16.))
        .items_center()
        .justify_center()
        .child(
            v_flex()
                .w(px(192.))
                .flex_none()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(UI_TEXT_SIZE))
                        .text_color(to_hsla(p.muted_foreground))
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(to_hsla(p.subtle_foreground))
                        .child(body.into()),
                ),
        )
}

/// 表单标签：与输入列左对齐，冒号由调用方给出。
pub fn form_label(text: impl Into<SharedString>, muted: Hsla) -> impl IntoElement {
    div()
        .w(px(LABEL_W))
        .text_size(px(UI_TEXT_SIZE))
        .text_color(muted)
        .child(text.into())
}

/// 对话框表单标签：Zed `Label` Default —— 正文色（`Color::Default` → text），
/// 固定列宽、左对齐。
pub fn dialog_label(text: impl Into<SharedString>, dark: bool, width: f32) -> impl IntoElement {
    let p = Palette::of(dark);
    div()
        .w(px(width))
        .flex_none()
        .text_size(px(UI_TEXT_SIZE))
        .text_color(to_hsla(p.foreground))
        .child(text.into())
}

/// 对话框输入框：照 Zed `ui_input::InputField` 的配方——32 高、左右 8、
/// `rounded_md` 6、1px `border_variant` 描边、`editor_background` 底，聚焦换
/// `border_focused`。内部仍用组件库的 Input —— 文本编辑、选区、快捷键都不另造。
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
        .with_size(ComponentSize::XSmall)
        .px(px(0.));
    // 输入框比面板深一档（Zed 用 editor.background 压 surface 一档），同色会糊成一片
    div()
        .h(px(FIELD_H))
        .w_full()
        .flex()
        .items_center()
        .px(px(FIELD_PAD_X))
        .bg(to_hsla(p.background))
        .border(px(HAIRLINE))
        .border_color(edge)
        .rounded(px(FIELD_RADIUS))
        .child(Styled::h(input, px(FIELD_H - HAIRLINE * 2.)))
        .into_any_element()
}

/// Zed `Checkbox` 的方块本体：20 见方的点击容器里放 16 见方外框，`rounded_xs`、
/// 1px `border`、底保持 `ghost_element_background`（透明）——Zed 的勾选态靠勾的
/// 颜色表达，不靠铺底；悬停只把描边提亮到 0.7 alpha。
fn control_box(dark: bool, radius: f32, inner: Option<AnyElement>) -> Div {
    let p = Palette::of(dark);
    div()
        .flex_none()
        .size(px(CONTROL_HIT))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .flex_none()
                .size(px(CONTROL_DOT))
                .rounded(px(radius))
                .flex()
                .items_center()
                .justify_center()
                .bg(transparent_black())
                .border(px(HAIRLINE))
                .border_color(to_hsla(p.border))
                .hover(|s| s.border_color(to_hsla(p.border).alpha(0.7)))
                .children(inner),
        )
}

/// 单选：Zed 没有圆点单选件（单选语义走 `ChoiceCard` 卡片），这里按 `Checkbox`
/// 同一档几何复刻——16 见方圆圈、1px `border` 描边，选中时描边换强调色并在
/// 中心放 `Indicator::Dot` 那档 6px 强调色实心点。
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
    let ring = div()
        .flex_none()
        .size(px(CONTROL_DOT))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(transparent_black())
        .border(px(HAIRLINE))
        .border_color(to_hsla(if checked { p.accent } else { p.border }))
        .when(checked, |s| {
            s.child(
                div()
                    .flex_none()
                    .size(px(6.))
                    .rounded_full()
                    .bg(to_hsla(p.accent)),
            )
        });
    h_flex()
        .id(id)
        .gap(px(CONTROL_GAP))
        .items_center()
        .cursor_pointer()
        .child(
            div()
                .flex_none()
                .size(px(CONTROL_HIT))
                .flex()
                .justify_center()
                .items_center()
                .child(ring),
        )
        .when(enabled, |s| s.on_click(on_click))
        .when(!enabled, |s| s.opacity(0.4).cursor_not_allowed())
        .when(!label.is_empty(), |s| {
            s.child(
                div()
                    .text_size(px(UI_TEXT_SIZE))
                    .text_color(to_hsla(if enabled {
                        p.muted_foreground
                    } else {
                        p.subtle_foreground
                    }))
                    .child(label.clone()),
            )
        })
        .into_any_element()
}

/// 复选：Zed `Checkbox` 原样——16 见方 `rounded_xs` 外框、透明底、1px `border`，
/// 选中时在框内放 14px 的强调色对勾（不是铺满强调色 + 白勾）；禁用换
/// `element_disabled` 60% 底 + `border_variant` 描边。
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
    let icon = if checked {
        Some(zed_icons::ic(ZedIcon::Check, CONTROL_ICON, to_hsla(p.accent)).into_any_element())
    } else {
        None
    };
    let box_ = if enabled {
        control_box(dark, CONTROL_RADIUS, icon)
    } else {
        div()
            .flex_none()
            .size(px(CONTROL_HIT))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex_none()
                    .size(px(CONTROL_DOT))
                    .rounded(px(CONTROL_RADIUS))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(to_hsla(p.button_face).opacity(0.6))
                    .border(px(HAIRLINE))
                    .border_color(to_hsla(p.border_soft))
                    .children(icon),
            )
    };
    h_flex()
        .id(id)
        .gap(px(CONTROL_GAP))
        .items_center()
        .when(enabled, |s| s.cursor_pointer().on_click(on_click))
        .when(!enabled, |s| s.cursor_not_allowed().opacity(0.4))
        .child(box_)
        .when(!label.is_empty(), |s| {
            s.child(
                div()
                    .text_size(px(UI_TEXT_SIZE))
                    .text_color(to_hsla(if enabled {
                        p.muted_foreground
                    } else {
                        p.subtle_foreground
                    }))
                    .child(label.clone()),
            )
        })
        .into_any_element()
}

/// Zed 页签条：条底画一条 1px `border` 线，未激活页签压在 `tab.inactive_background`
/// 上并各自带 1px 底边；激活页签改成内容底色 + 1px 底内距，把条底那条线盖住，
/// 于是它和下方内容面连成一片（Zed `Tab` 的 `pb_px()` 手法）。页签之间 1px 竖线
/// 分隔，方角，内容 `px(Base04)` 4、图标间距 4。
pub fn page_tabs(tabs: &[&'static str], active: usize, dark: bool) -> impl IntoElement {
    let p = Palette::of(dark);
    let line = to_hsla(p.border);
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
                .text_size(px(UI_TEXT_SIZE))
                .text_color(to_hsla(if on { p.foreground } else { p.muted_foreground }))
                .bg(to_hsla(if on { p.background } else { p.tab_face }))
                .when(ix > 0, |s| s.border_l(px(HAIRLINE)).border_color(line))
                .when(!on, |s| s.border_b(px(HAIRLINE)).border_color(line))
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

/// 单选分段（格式 / 类型这类二选一）：Zed `ToggleButtonGroup` 的 Transparent 档——
/// 条目各自成块、之间只留一条缝；选中项铺 `ButtonStyle::Tinted(Accent)`
/// （info.background 面 + info.border 描边 + `primary_foreground` 文字——
/// VS Code 2026 两档的 face 是实心品牌蓝，写 accent 会蓝字压蓝底看不见），未选中无底面、文字走
/// muted、hover 才铺 ghost_element_hover。尺寸 `ButtonSize::Medium` = 28 / px 8。
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
        .h(px(CONTROL_H))
        .px(px(CONTROL_PAD_X))
        .flex()
        .items_center()
        .rounded(px(RADIUS))
        .text_size(px(UI_TEXT_SIZE))
        .cursor_pointer()
        .text_color(to_hsla(if selected {
            p.primary_foreground
        } else {
            p.muted_foreground
        }))
        .when(selected, move |s| {
            s.bg(to_hsla(p.primary_face))
                .border(px(HAIRLINE))
                .border_color(to_hsla(p.primary_border))
        })
        .when(!selected, move |s| {
            s.hover(move |s| s.bg(to_hsla(p.hover)))
        })
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
