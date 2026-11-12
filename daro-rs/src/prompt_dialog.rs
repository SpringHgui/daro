//! 单行文本输入小窗（新建组 / 复制连接）。参考客户端里这类确认框都是独立窗口，
//! 这里同样开顶层窗口，避免在主窗口里叠遮罩。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;

use crate::DaroApp;
use crate::palette::Palette;
use crate::to_hsla;

use crate::ui::{self, TITLE_H};

type Submit = Box<dyn Fn(&mut DaroApp, String)>;
type Confirm = Box<dyn Fn(&mut DaroApp)>;

pub fn open_text_prompt(
    parent: Entity<DaroApp>,
    dark: bool,
    title: &'static str,
    label: &'static str,
    initial: String,
    on_submit: impl Fn(&mut DaroApp, String) + 'static,
    cx: &mut App,
) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(360.), px(150.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(420.), px(180.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some(title.into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        let on_submit: Submit = Box::new(on_submit);
        cx.new(|cx| TextPrompt::new(parent, dark, title, label, initial, on_submit, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open prompt window");
    }
}

struct TextPrompt {
    parent: Entity<DaroApp>,
    dark: bool,
    title: &'static str,
    label: &'static str,
    input: Entity<InputState>,
    on_submit: Submit,
    /// Enter 与按钮可能同时到达，提交只认第一次
    submitted: bool,
}

impl TextPrompt {
    #[allow(clippy::too_many_arguments)]
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        title: &'static str,
        label: &'static str,
        initial: String,
        on_submit: Submit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        let window_handle = window.window_handle();
        cx.subscribe(&input, move |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                // 订阅回调拿不到 Window，靠窗口句柄关窗（参考客户端的确认框 Enter = 确定）
                if this.take_value(cx).is_some() {
                    let _ = cx.update_window(window_handle, |_, window, _| window.remove_window());
                }
            } else {
                cx.notify();
            }
        })
        .detach();
        cx.defer_in(window, |this, window, cx| {
            this.input.update(cx, |state, cx| {
                state.focus(window, cx);
                // 参考客户端的预填框打开时是全选态：直接打字即替换，不用先退格
                state.select_all(window, cx);
            });
        });
        Self {
            parent,
            dark,
            title,
            label,
            input,
            on_submit,
            submitted: false,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn take_value(&mut self, cx: &mut Context<Self>) -> Option<String> {
        if self.submitted {
            return None;
        }
        let value = self.input.read(cx).value().trim().to_string();
        if value.is_empty() {
            return None;
        }
        self.submitted = true;
        let parent = self.parent.clone();
        let submit = &self.on_submit;
        parent.update(cx, |app, cx| {
            submit(app, value.clone());
            cx.notify();
        });
        Some(value)
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.take_value(cx).is_some() {
            window.remove_window();
        }
    }

    fn title_bar(&self) -> impl IntoElement {
        let p = self.pal();
        TitleBar::new().h(px(TITLE_H)).border_b_0().child(
            h_flex()
                .h_full()
                .items_center()
                .gap(px(6.))
                .pl(px(4.))
                .child(
                    div()
                        .text_xs()
                        .text_color(to_hsla(p.foreground))
                        .child(self.title),
                ),
        )
    }
}


impl Render for TextPrompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let can_submit = !self.input.read(cx).value().trim().is_empty();

        v_flex()
            .id("prompt-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                let m = ev.keystroke.modifiers;
                if ev.keystroke.key == "enter" && !m.control && !m.alt && !m.platform {
                    this.submit(window, cx);
                    cx.stop_propagation();
                } else if ev.keystroke.key == "escape" {
                    window.remove_window();
                    cx.stop_propagation();
                }
            }))
            .child(self.title_bar())
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(16.))
                    .gap(px(10.))
                    .items_center()
                    .child(div().w(px(56.)).text_xs().text_color(fg).child(self.label))
                    .child(
                        div()
                            .flex_1()
                            .child(ui::field_input(&self.input, self.dark, window, cx)),
                    ),
            )
            .child(
                ui::footer_bar(self.dark)
                    .child(div().flex_1())
                    .child(
                        ui::primary_button("ok", "确定", self.dark, cx)
                            .disabled(!can_submit)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    )
                    .child(
                        ui::secondary_button("cancel", "取消")
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}

pub fn open_confirm(
    parent: Entity<DaroApp>,
    dark: bool,
    title: &'static str,
    message: String,
    on_ok: impl Fn(&mut DaroApp) + 'static,
    cx: &mut App,
) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(340.), px(140.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(420.), px(170.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some(title.into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| ConfirmPrompt::new(parent, dark, title, message, Box::new(on_ok), window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open confirm window");
    }
}

struct ConfirmPrompt {
    parent: Entity<DaroApp>,
    dark: bool,
    title: &'static str,
    message: String,
    on_ok: Confirm,
    /// 窗口里没有输入框，必须自己占住焦点，否则 Enter / Esc 到不了 on_key_down
    focus: FocusHandle,
    /// Enter 与按钮可能同时到达，只认第一次
    answered: bool,
}

impl ConfirmPrompt {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        title: &'static str,
        message: String,
        on_ok: Confirm,
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
            title,
            message,
            on_ok,
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
        parent.update(cx, |app, cx| {
            on_ok(app);
            cx.notify();
        });
    }
}

impl Render for ConfirmPrompt {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);

        v_flex()
            .track_focus(&self.focus)
            .id("confirm-root")
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
            .child(TitleBar::new().h(px(TITLE_H)).border_b_0().child(
                h_flex()
                    .h_full()
                    .items_center()
                    .pl(px(4.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(to_hsla(p.foreground))
                            .child(self.title),
                    ),
            ))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(16.))
                    .text_sm()
                    .child(self.message.clone()),
            )
            .child(
                ui::footer_bar(self.dark)
                    .child(div().flex_1())
                    .child(
                        ui::primary_button("ok", "确定", self.dark, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.accept(window, cx))),
                    )
                    .child(
                        ui::secondary_button("cancel", "取消")
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}

pub fn open_notice(dark: bool, title: &'static str, message: String, cx: &mut App) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(340.), px(140.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(420.), px(170.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some(title.into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| Notice::new(dark, title, message, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open notice window");
    }
}

struct Notice {
    dark: bool,
    title: &'static str,
    message: String,
    /// 同确认框：没有输入框，得自己占住焦点才能收 Enter / Esc
    focus: FocusHandle,
}

impl Notice {
    fn new(
        dark: bool,
        title: &'static str,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let deferred = focus.clone();
        cx.defer_in(window, move |_, window, app| {
            deferred.focus(window, app);
        });
        Self {
            dark,
            title,
            message,
            focus,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }
}

impl Render for Notice {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);

        v_flex()
            .track_focus(&self.focus)
            .id("notice-root")
            .test_support()
            // 读屏要念出正文，无头用例也靠它核对每个入口给的文案
            .aria_label(self.message.clone())
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                let m = ev.keystroke.modifiers;
                if (ev.keystroke.key == "enter" && !m.control && !m.alt && !m.platform)
                    || ev.keystroke.key == "escape"
                {
                    window.remove_window();
                    cx.stop_propagation();
                }
            }))
            .child(TitleBar::new().h(px(TITLE_H)).border_b_0().child(
                h_flex()
                    .h_full()
                    .items_center()
                    .pl(px(4.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(to_hsla(p.foreground))
                            .child(self.title),
                    ),
            ))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(16.))
                    .text_sm()
                    .child(self.message.clone()),
            )
            .child(
                ui::footer_bar(self.dark)
                    .child(div().flex_1())
                    .child(
                        ui::primary_button("ok", "确定", self.dark, cx)
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}
