//! 全应用统一的控件尺寸与按钮构造。
//!
//! 目的：所有对话框的按钮 / 输入框 / 列表行只用一套尺寸，避免出现
//! 「同一个对话框里按钮 22、输入框 24、另一个对话框又 26」的参差。
//! 需要新尺寸时先在这里加常量，不要在各文件里随手写 `.h(px(...))`。
//!
//! 密度参照 Flutter 版 base-ui 的桌面控件：
//! - 标准控件（按钮 / 单行输入 / 下拉）高 24，水平留白 12；
//! - 紧凑控件（列表工具条、分隔符选择这类行内小按钮）高 22，水平留白 8；
//! - 对话框标题栏 24、底栏 46、列表行 28；
//! - 正文 12px（`.text_xs()`），次要文字同字号换 muted 色。

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;

use crate::palette::Palette;
use crate::to_hsla;

/// 标准控件高度（按钮 / 单行输入）
pub const CONTROL_H: f32 = 24.;
/// 标准控件水平留白
pub const CONTROL_PAD_X: f32 = 12.;
/// 紧凑控件高度（列表工具条里的小按钮）
pub const COMPACT_H: f32 = 22.;
/// 紧凑控件水平留白
pub const COMPACT_PAD_X: f32 = 8.;
/// 对话框标题栏高度
pub const TITLE_H: f32 = 24.;
/// 对话框底栏高度
pub const FOOTER_H: f32 = 46.;
/// 对话框内列表行高
pub const DIALOG_ROW_H: f32 = 28.;
/// 表单左侧标签列宽
pub const LABEL_W: f32 = 64.;

/// 主按钮（强调底 + 强调边），用于「确定 / 导入 / 导出」这类主动作。
pub fn primary_button(id: &'static str, label: &'static str, dark: bool, cx: &App) -> Button {
    let p = Palette::of(dark);
    let variant = ButtonCustomVariant::new(cx)
        .color(to_hsla(p.selection))
        .foreground(to_hsla(p.foreground))
        .hover(to_hsla(0xb8daf6))
        .active(to_hsla(0xa4cff3));
    Button::new(id)
        .label(label)
        .h(px(CONTROL_H))
        .px(px(CONTROL_PAD_X))
        .custom(variant)
}

/// 次按钮（取消 / 关闭 / 浏览…）
pub fn secondary_button(id: &'static str, label: &'static str) -> Button {
    Button::new(id)
        .label(label)
        .h(px(CONTROL_H))
        .px(px(CONTROL_PAD_X))
}

/// 紧凑按钮（列表工具条：全选 / 取消全选 / 分隔符选择）
pub fn compact_button(id: impl Into<ElementId>, label: &'static str) -> Button {
    Button::new(id)
        .label(label)
        .h(px(COMPACT_H))
        .px(px(COMPACT_PAD_X))
}

/// 标准单行输入框
pub fn text_input(state: &Entity<InputState>) -> Input {
    Input::new(state).h(px(CONTROL_H))
}

/// 表单行左侧标签（统一宽度与颜色）
pub fn form_label(text: impl Into<SharedString>, muted: Hsla) -> impl IntoElement {
    div()
        .w(px(LABEL_W))
        .text_xs()
        .text_color(muted)
        .child(text.into())
}
