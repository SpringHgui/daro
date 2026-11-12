//! 「粘贴表」对话框：照 Navicat 的交互，粘贴前确认目标与「结构和数据 /
//! 仅结构」。表名由调用方先规划好（撞名自动 `_copy`），这里只列落点。

use gpui_kit::component::*;
use gpui_kit::*;

use crate::palette::Palette;
use crate::table_copy::PasteMode;
use crate::{DaroApp, to_hsla, ui};

use crate::ui::TITLE_H;

type Submit = Box<dyn Fn(&mut DaroApp, PasteMode)>;

/// 对话框要展示的内容。
#[derive(Clone, Debug)]
pub struct PasteRequest {
    /// 源上下文（连接 / 库 / 模式），只用于说明。
    pub source: String,
    /// 目标上下文（连接 / 库 / 模式）。
    pub target: String,
    /// 源表名 → 目标表名。
    pub names: Vec<(String, String)>,
}

pub fn open_table_paste(
    parent: Entity<DaroApp>,
    dark: bool,
    req: PasteRequest,
    on_ok: impl Fn(&mut DaroApp, PasteMode) + 'static,
    cx: &mut App,
) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(420.), px(240.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(520.), px(320.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("粘贴表".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| TablePaste::new(parent, dark, req, Box::new(on_ok), window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open table paste window");
    }
}

struct TablePaste {
    parent: Entity<DaroApp>,
    dark: bool,
    req: PasteRequest,
    on_ok: Submit,
    mode: PasteMode,
    /// 没有输入框，自己占住焦点才能收 Enter / Esc
    focus: FocusHandle,
    answered: bool,
}

impl TablePaste {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        req: PasteRequest,
        on_ok: Submit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let deferred = focus.clone();
        cx.defer_in(window, move |_, window, app| {
            deferred.focus(window, app);
        });
        Self {
            parent,
            dark,
            req,
            on_ok,
            mode: PasteMode::Data,
            focus,
            answered: false,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.remove_window();
        if self.answered {
            return;
        }
        self.answered = true;
        let parent = self.parent.clone();
        let on_ok = &self.on_ok;
        let mode = self.mode;
        parent.update(cx, |app, cx| {
            on_ok(app, mode);
            cx.notify();
        });
    }
}

impl Render for TablePaste {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let dark = self.dark;
        let mode = self.mode;

        let mut list = v_flex().gap(px(3.));
        for (source, target) in &self.req.names {
            list = list.child(
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .child(div().text_xs().text_color(muted).child(source.clone()))
                    .child(div().text_xs().text_color(muted).child("→"))
                    .child(div().text_xs().text_color(fg).child(target.clone())),
            );
        }

        let pick = |this_mode: PasteMode| {
            let checked = mode == this_mode;
            ui::radio(
                ("paste-mode", if this_mode.with_data() { 0usize } else { 1 }),
                this_mode.label(),
                checked,
                true,
                dark,
                cx.listener(move |this, _, _, cx| {
                    this.mode = this_mode;
                    cx.notify();
                }),
            )
        };

        v_flex()
            .track_focus(&self.focus)
            .id("paste-root")
            .test_support()
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                let m = ev.keystroke.modifiers;
                if ev.keystroke.key == "enter" && !m.control && !m.alt && !m.platform {
                    this.accept(window, cx);
                    cx.stop_propagation();
                } else if ev.keystroke.key == "escape" {
                    window.remove_window();
                    cx.stop_propagation();
                }
            }))
            .child(
                TitleBar::new().h(px(TITLE_H)).border_b_0().child(
                    h_flex().h_full().items_center().pl(px(4.)).child(
                        div()
                            .text_xs()
                            .text_color(to_hsla(p.foreground))
                            .child("粘贴表"),
                    ),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(10.))
                    .p(px(16.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("从：{}", self.req.source)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("粘贴到：{}", self.req.target)),
                    )
                    .child(
                        v_flex()
                            .id("paste-plan")
                            .test_support()
                            .flex_1()
                            .min_h_0()
                            .gap(px(3.))
                            .overflow_y_scroll()
                            .child(list),
                    )
                    .child(
                        h_flex()
                            .gap(px(18.))
                            .items_center()
                            .child(pick(PasteMode::Data))
                            .child(pick(PasteMode::Structure)),
                    ),
            )
            .child(
                ui::footer_bar(dark)
                    .child(div().flex_1())
                    .child(
                        ui::primary_button("paste-ok", "粘贴", dark, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.accept(window, cx))),
                    )
                    .child(
                        ui::secondary_button("paste-cancel", "取消")
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}
