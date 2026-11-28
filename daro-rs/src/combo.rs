//! 查询工具栏与「另存为」共用的自绘组合框。
//!
//! 观感按 Zed 的对应件复刻：触发器 = `ui::DropdownMenu` 的 `ButtonLike`
//! （22 高、`px(Base04)` 4、`rounded_sm` 4、1px `border_variant`，打开换
//! `border_focused`，整块 hover 提亮，箭头 `ChevronUpDown` 12 灰）；面板与行 =
//! `Popover`(`elevation_2`) + `ListItem`(inset 4 / 内距 6 / 圆角 4 / Dense) +
//! `ListSeparator` 那一档，几何常量与弹出菜单共用 [`crate::ui`] 里的一组。
//! 面板放在页面里还是另开浮层窗口由调用方决定，组件只负责外观与行列结构。

use std::ops::Range;

use crate::ui::{self, float_shadow, UI_TEXT_SIZE};
use crate::zed_icons::ZedIcon;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 触发器宽（查询窗口三个等宽下拉）
pub const TRIGGER_W: f32 = 183.;
/// 触发器高（Zed `ButtonSize::Default` = `rems_from_px(22)`）
pub const TRIGGER_H: f32 = 22.;
/// 触发器左右留白（Zed Default / Compact 档 `px(Base04)` = 4px）
pub const TRIGGER_PAD_X: f32 = 4.;
/// 下拉行高（Zed `ListItem` Dense：内容高 20 起步）
pub const ROW_H: f32 = 22.;
/// 面板上下留白 = 行的外侧留白（Zed `ContextMenu` 列表内边距 `Base04`）
pub const PAD_Y: f32 = 4.;
/// 下拉面板最高（超出滚动）
pub const MENU_MAX_H: f32 = 420.;

const ARROW_CELL_W: f32 = 16.;
/// 箭头图标（Zed `IconSize::XSmall` = 12px）
const ARROW_ICON: f32 = 12.;

// 面板与行的几何取 `crate::ui` 里 Zed 浮层那一档，和弹出菜单保持一致。
const PANEL_RADIUS: Pixels = px(ui::FLOAT_RADIUS);
const ROW_INSET: Pixels = px(ui::ROW_INSET);
const ROW_RADIUS: Pixels = px(ui::ROW_RADIUS);
const ROW_PAD_X: Pixels = px(ui::ROW_PAD_X);

/// 两处主题各自的配色，由调用方从 Skin / Palette 映射进来。
#[derive(Clone, Copy)]
pub struct Colors {
    /// 触发器底色
    pub trigger_bg: Hsla,
    /// 下拉面板底色
    pub menu_bg: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    /// 收起态描边（Zed border.variant）
    pub edge: Hsla,
    /// 打开态描边（Zed border.focused）
    pub focus: Hsla,
    /// 当前项 / 键盘高亮底色（Zed ghost_element_selected）
    pub selected: Hsla,
    /// 悬停底色（Zed ghost_element_hover）
    pub hover: Hsla,
    /// 按下底色（Zed ghost_element_active）
    pub active: Hsla,
    /// 筛选命中的底色
    pub highlight: Hsla,
    /// 当前明暗档：面板投影按 Zed ElevatedSurface 分明暗给
    pub is_dark: bool,
}

/// 触发器内容。`width` 为 None 时撑满父容器。
pub struct TriggerSpec {
    pub id: String,
    pub width: Option<f32>,
    pub icon: AnyElement,
    pub label: String,
    pub open: bool,
    /// Some 时打开态正文换成无边框输入框（可输入筛选）
    pub input: Option<Entity<InputState>>,
}

/// 触发器：22 高、0.5px 边、图标 / 文本 / 箭头三格。
/// 点正文只在收起时回调（展开时正文交给输入框），箭头负责开 / 关。
pub fn trigger<T: 'static>(
    spec: TriggerSpec,
    colors: Colors,
    cx: &mut Context<T>,
    on_body: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
    on_arrow: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
    on_key: impl Fn(&mut T, &KeyDownEvent, &mut Window, &mut Context<T>) + 'static,
) -> AnyElement {
    let TriggerSpec {
        id,
        width,
        icon,
        label,
        open,
        input,
    } = spec;
    let editable = open && input.is_some();
    let text = if editable {
        Input::new(input.as_ref().expect("editable 必有输入框"))
            .appearance(false)
            .bordered(false)
            .focus_bordered(false)
            .cleanable(false)
            .with_size(gpui_kit::component::Size::XSmall)
            .text_size(px(UI_TEXT_SIZE))
            .text_color(colors.text)
            .px(px(0.))
            .into_any_element()
    } else {
        div()
            .truncate()
            .text_size(px(UI_TEXT_SIZE))
            .text_color(colors.text)
            .child(label)
            .into_any_element()
    };
    let mut root = h_flex()
        .id(id.clone())
        .h(px(TRIGGER_H))
        .flex_none()
        .items_center()
        // Zed `ButtonLike`：内容 `px(Base04)` 4、图标与文字间距 4、`rounded_sm`
        .px(px(TRIGGER_PAD_X))
        .gap(px(TRIGGER_PAD_X))
        .rounded(px(ui::RADIUS))
        .bg(colors.trigger_bg)
        .border(px(1.))
        .border_color(if open { colors.focus } else { colors.edge })
        .cursor_pointer()
        .when(!open, |s| s.hover(move |s| s.bg(colors.hover)))
        .when(open, |s| s.bg(colors.hover))
        .on_click(cx.listener(move |this, _, window, cx| on_body(this, window, cx)))
        .when(editable, |s| {
            s.on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                on_key(this, ev, window, cx)
            }))
        })
        .test_support();
    root = match width {
        Some(w) => root.w(px(w)),
        None => root.w_full(),
    };
    root.child(icon).child(
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .child(text),
    )
    .child(
        div()
            .id(format!("{id}-arrow"))
            .w(px(ARROW_CELL_W))
            .h_full()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(
                Icon::new(ZedIcon::ChevronDown)
                    .with_size(px(ARROW_ICON))
                    .text_color(colors.muted),
            )
            .on_click(cx.listener(move |this, _, window, cx| on_arrow(this, window, cx))),
    )
    .into_any_element()
}

/// 下拉里的一行：图标 + 文本；active 是当前值 / 键盘高亮，enabled=false 置灰。
/// `hint` 靠右显示快捷键（运行菜单的 Ctrl+R 等）。
pub struct RowSpec {
    pub id: String,
    pub label: String,
    pub icon: Option<AnyElement>,
    pub enabled: bool,
    pub highlight: Option<Range<usize>>,
    pub active: bool,
    pub hint: Option<String>,
}

pub fn row<T: 'static>(
    spec: RowSpec,
    colors: Colors,
    cx: &mut Context<T>,
    on_click: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
) -> AnyElement {
    let RowSpec {
        id,
        label,
        icon,
        enabled,
        highlight,
        active,
        hint,
    } = spec;
    let text_color = if enabled { colors.text } else { colors.muted };
    // Zed `ListItem`：行的 4px 留白画在高亮盒外面（`mx`），盒内左右 `px(Base06)` 6、
    // 高亮盒 `rounded_sm` 4、Dense 档不加上下内距；指针 hover、当前项 selected、
    // 按下 active 三档分开。
    h_flex()
        .id(id)
        .h(px(ROW_H))
        .w_full()
        .flex_none()
        .items_center()
        .mx(ROW_INSET)
        .px(ROW_PAD_X)
        .gap(ROW_PAD_X)
        .rounded(ROW_RADIUS)
        .when(active, |s| s.bg(colors.selected))
        .when(enabled && !active, |s| {
            s.cursor_pointer()
                .hover(move |s| s.bg(colors.hover))
                .active(move |s| s.bg(colors.active))
        })
        .when(!enabled, |s| s.opacity(0.45))
        .when_some(icon, |el, icon| el.child(icon))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(UI_TEXT_SIZE))
                .text_color(text_color)
                .child(match highlight {
                    Some(range) => highlighted_label(&label, range, colors.highlight)
                        .into_any_element(),
                    None => label.into_any_element(),
                }),
        )
        .when_some(hint, |el, hint| {
            el.child(
                div()
                    .flex_none()
                    .pl(px(8.))
                    .text_size(px(12.))
                    .text_color(colors.muted)
                    .child(hint),
            )
        })
        .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
        .test_support()
        .into_any_element()
}

/// 面板怎么定尺寸：查询窗口里按内容高、最多 420；另存为浮窗里撑满窗口。
pub enum PanelFit {
    Content { max_h: f32 },
    Fill,
}

/// 下拉面板：Zed `Popover` 的 `elevation_2` 卡片——8 圆角、1px `border_variant`
/// 描边、ElevatedSurface 双层投影，行区间上下留白 4、超出滚动。
/// 描边画在滚动区外层：滚动组件会把内容自身的边框留在内容底部，候选多到
/// 撑出视口时下边框就跟着被裁掉了，选中列表看上去没有下沿。
pub fn panel(rows: Vec<AnyElement>, fit: PanelFit, colors: Colors) -> AnyElement {
    let scroll = v_flex()
        .w_full()
        .overflow_y_scrollbar()
        .py(ROW_INSET)
        .children(rows);
    let frame = v_flex()
        .w_full()
        .bg(colors.menu_bg)
        .rounded(PANEL_RADIUS)
        .border(px(1.))
        .border_color(colors.edge)
        .shadow(float_shadow(colors.is_dark));
    match fit {
        PanelFit::Content { max_h } => frame
            .max_h(px(max_h))
            .child(scroll.max_h(px(max_h))),
        PanelFit::Fill => frame.size_full().child(scroll.size_full()),
    }
    .into_any_element()
}

/// 命中段铺黄底的标签文本（参考界面搜索的浏览器式高亮）。
pub fn highlighted_label(label: &str, range: Range<usize>, hl: Hsla) -> StyledText {
    StyledText::new(label.to_string()).with_highlights([(
        range,
        HighlightStyle {
            background_color: Some(hl),
            ..Default::default()
        },
    )])
}
