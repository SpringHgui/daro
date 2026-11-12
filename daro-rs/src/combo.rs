//! 查询工具栏与「另存为」共用的自绘组合框。
//!
//! 触发器 22 高：图标格 / 文本格（打开时可换成筛选输入框）/ 箭头格；展开时
//! 描边换成 focus 色。下拉面板与行：行高 22、上下留白 4、0.5 灰边，当前项
//! 与悬停整行铺 element 底色，筛选命中子串铺半透明强调色。
//! 面板放在页面里还是另开浮层窗口由调用方决定，组件只负责外观与行列结构。

use std::ops::Range;

use crate::zed_icons::ZedIcon;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 触发器宽（查询窗口三个等宽下拉）
pub const TRIGGER_W: f32 = 183.;
pub const TRIGGER_H: f32 = 22.;
/// 下拉行高 / 上下留白
pub const ROW_H: f32 = 22.;
pub const PAD_Y: f32 = 4.;
/// 下拉面板最高（超出滚动）
pub const MENU_MAX_H: f32 = 420.;


const ICON_CELL_W: f32 = 22.;
const ARROW_CELL_W: f32 = 20.;
const TEXT_SIZE: f32 = 12.5;

/// 两处主题各自的配色，由调用方从 Skin / Palette 映射进来。
#[derive(Clone, Copy)]
pub struct Colors {
    /// 触发器底色
    pub trigger_bg: Hsla,
    /// 下拉面板底色
    pub menu_bg: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    /// 收起态描边
    pub edge: Hsla,
    /// 打开态描边（Zed border.focused）
    pub focus: Hsla,
    /// 当前项 / 键盘高亮底色
    pub selected: Hsla,
    /// 悬停底色
    pub hover: Hsla,
    /// 筛选命中的底色
    pub highlight: Hsla,
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
            .text_size(px(TEXT_SIZE))
            .text_color(colors.text)
            .px(px(0.))
            .into_any_element()
    } else {
        div()
            .truncate()
            .text_size(px(TEXT_SIZE))
            .text_color(colors.text)
            .child(label)
            .into_any_element()
    };
    let mut root = h_flex()
        .id(id.clone())
        .h(px(TRIGGER_H))
        .flex_none()
        .items_center()
        .bg(colors.trigger_bg)
        .border(px(0.5))
        .border_color(if open { colors.focus } else { colors.edge })
        .cursor_pointer()
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
    root.child(
        div()
            .w(px(ICON_CELL_W))
            .h_full()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(icon),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .px(px(1.))
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
            .when(open, |s| s.bg(colors.hover))
            .child(
                Icon::new(ZedIcon::ChevronDown)
                    .with_size(px(12.))
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
    h_flex()
        .id(id)
        .h(px(ROW_H))
        .w_full()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .pl(px(4.))
        .pr(px(6.))
        .when(active, |s| s.bg(colors.selected))
        .when(enabled && !active, |s| {
            s.cursor_pointer().hover(move |s| s.bg(colors.hover))
        })
        .when(!enabled, |s| s.opacity(0.45))
        .when_some(icon, |el, icon| el.child(icon))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(TEXT_SIZE))
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
                    .pl(px(12.))
                    .text_size(px(11.5))
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

/// 下拉面板：0.5px 描边、上下留白 4、超出滚动。
pub fn panel(rows: Vec<AnyElement>, fit: PanelFit, colors: Colors) -> AnyElement {
    let base = v_flex()
        .w_full()
        .overflow_y_scrollbar()
        .bg(colors.menu_bg)
        .border(px(0.5))
        .border_color(colors.edge)
        .py(px(PAD_Y));
    let sized = match fit {
        PanelFit::Content { max_h } => base.max_h(px(max_h)),
        PanelFit::Fill => base.size_full(),
    };
    sized.children(rows).into_any_element()
}

/// 命中段铺黄底的标签文本（参考客户端搜索的浏览器式高亮）。
pub fn highlighted_label(label: &str, range: Range<usize>, hl: Hsla) -> StyledText {
    StyledText::new(label.to_string()).with_highlights([(
        range,
        HighlightStyle {
            background_color: Some(hl),
            ..Default::default()
        },
    )])
}
