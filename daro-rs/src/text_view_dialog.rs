//! 只读文本详情窗（摘要页每行「…」的入口）。
//!
//! 概览表一行放不下完整 SQL 与驱动报错原文：SQL 会换行截断、报错消息里还夹着
//! 整条语句（rusqlite 是 `{msg} in {sql} at offset {n}`），所以给一个能滚动、
//! 能整段复制的详情窗。

use gpui_kit::component::*;
use gpui_kit::*;
use gpui;

use crate::palette::Palette;
use crate::ui;
use crate::to_hsla;

/// `sections`：[(小标题, 正文)]，正文保留换行原样显示。
pub fn open_text_view(
    dark: bool,
    title: &'static str,
    sections: Vec<(&'static str, String)>,
    cx: &mut App,
) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(420.), px(280.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(760.), px(560.)), cx));
    if let Some(tb) = options.titlebar.as_mut() {
        tb.title = Some(title.into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            let esc = ui::close_on_escape(window, cx);
            TextViewDialog {
                dark,
                title,
                sections,
                _esc: esc,
            }
        })
    });
    if opened.is_err() {
        eprintln!("[text_view_dialog] 打开详情窗失败");
    }
}

struct TextViewDialog {
    dark: bool,
    title: &'static str,
    sections: Vec<(&'static str, String)>,
    _esc: Subscription,
}

impl Render for TextViewDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::of(self.dark);
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let border = to_hsla(p.border);

        let all = self
            .sections
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let sections = self.sections.clone();

        v_flex()
            .id("textview-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .child(
                TitleBar::new()
                    .h(px(crate::ui::TITLE_H))
                    .border_b_0()
                    .child(
                        h_flex()
                            .h_full()
                            .items_center()
                            .pl(px(4.))
                            .child(div().text_xs().text_color(fg).child(self.title)),
                    ),
            )
            .child(
                v_flex()
                    .id("textview-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(10.))
                    .gap(px(8.))
                    .children(sections.into_iter().map(|(label, text)| {
                        v_flex()
                            .gap(px(4.))
                            .child(div().text_size(px(11.5)).text_color(muted).child(label))
                            .child(
                                div()
                                    .w_full()
                                    .p(px(8.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(border)
                                    .font_family("Consolas")
                                    .text_size(px(12.))
                                    .text_color(fg)
                                    .child(text),
                            )
                    })),
            )
            .child(
                ui::footer_bar(self.dark)
                    .child(ui::secondary_button("tv-copy", "复制").on_click(
                        cx.listener(move |_, _, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(all.clone()));
                        }),
                    ))
                    .child(div().flex_1())
                    .child(ui::secondary_button("tv-close", "关闭").on_click(
                        cx.listener(|_, _, window, _cx| window.remove_window()),
                    )),
            )
    }
}