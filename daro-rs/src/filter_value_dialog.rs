//! 筛选值输入窗（参考界面的多窗口对应物）：
//! 值输入 + 建议值勾选列表 + 搜索，确定后回写准则值。
//! 基线样式：无标题行，顶部整行值输入，建议值勾选列表 + 底部搜索行，
//! 浅灰 footer 右对齐「确定」「取消」。介于双值档给两个输入、不出建议列表。

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::DaroApp;
use crate::palette::Palette;
use crate::to_hsla;

use crate::ui;

type OnOk = Box<dyn Fn(&mut DaroApp, String, Option<String>)>;

pub fn open_filter_value(
    parent: Entity<DaroApp>,
    dark: bool,
    title: &'static str,
    v1: String,
    v2: Option<String>,
    distinct: Vec<String>,
    on_ok: impl Fn(&mut DaroApp, String, Option<String>) + 'static,
    cx: &mut App,
) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(420.), px(300.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(620.), px(520.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some(title.into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        let on_ok: OnOk = Box::new(on_ok);
        cx.new(|cx| {
            FilterValueDialog::new(parent, dark, v1, v2, distinct, on_ok, window, cx)
        })
    });
    if opened.is_err() {
        eprintln!("failed to open filter value window");
    }
}

struct FilterValueDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    _esc: Subscription,
    v1: Entity<InputState>,
    v2: Option<Entity<InputState>>,
    distinct: Vec<String>,
    checked: Vec<bool>,
    search: Entity<InputState>,
    on_ok: OnOk,
    /// Enter 与按钮可能同时到达，提交只认第一次
    submitted: bool,
}

impl FilterValueDialog {
    #[allow(clippy::too_many_arguments)]
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        v1_init: String,
        v2_init: Option<String>,
        distinct: Vec<String>,
        on_ok: OnOk,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let v1 = cx.new(|cx| InputState::new(window, cx).default_value(v1_init));
        let v2 = v2_init
            .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索"));
        let window_handle = window.window_handle();
        let inputs: Vec<Entity<InputState>> =
            vec![v1.clone()].into_iter().chain(v2.clone()).collect();
        for input in inputs {
            cx.subscribe(&input, move |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    // 订阅回调拿不到 Window，靠窗口句柄关窗；重复提交已被
                    // take_values 的 submitted 档住
                    if this.take_values(cx).is_some() {
                        let _ = cx.update_window(window_handle, |_, window, _| {
                            window.remove_window()
                        });
                    }
                } else {
                    cx.notify();
                }
            })
            .detach();
        }
        cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::PressEnter { .. }) {
                cx.notify();
            }
        })
        .detach();
        cx.defer_in(window, |this, window, cx| {
            this.v1.update(cx, |state, cx| {
                state.focus(window, cx);
                state.select_all(window, cx);
            });
        });
        let esc = ui::close_on_escape(window, cx);
        Self {
            parent,
            dark,
            _esc: esc,
            v1,
            v2,
            checked: vec![false; distinct.len()],
            distinct,
            search,
            on_ok,
            submitted: false,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    /// 勾选优先于手输（勾了就用勾选集），返回回写值；重复提交返回 None。
    fn take_values(&mut self, cx: &mut Context<Self>) -> Option<(String, Option<String>)> {
        if self.submitted {
            return None;
        }
        self.submitted = true;
        let picked: Vec<String> = self
            .distinct
            .iter()
            .enumerate()
            .filter(|(ix, _)| self.checked[*ix])
            .map(|(_, v)| v.clone())
            .collect();
        let v1 = if picked.is_empty() {
            self.v1.read(cx).value().trim().to_string()
        } else {
            picked.join(", ")
        };
        let v2 = self
            .v2
            .as_ref()
            .map(|s| s.read(cx).value().trim().to_string());
        let parent = self.parent.clone();
        let on_ok = &self.on_ok;
        let v2w = v2.clone();
        let v1w = v1.clone();
        parent.update(cx, |app, cx| {
            on_ok(app, v1w, v2w);
            cx.notify();
        });
        Some((v1, v2))
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.take_values(cx).is_some() {
            window.remove_window();
        }
    }

    fn value_rows(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap(px(8.))
            .child(ui::field_input(&self.v1, self.dark, window, cx))
            .when_some(self.v2.clone(), |this, v2| {
                this.child(ui::field_input(&v2, self.dark, window, cx))
            })
    }

    fn suggest_rows(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let me = cx.entity().clone();
        let q = self.search.read(cx).value().to_lowercase();
        let hits: Vec<usize> = self
            .distinct
            .iter()
            .enumerate()
            .filter(|(_, v)| q.is_empty() || v.to_lowercase().contains(&q))
            .map(|(ix, _)| ix)
            .collect();
        v_flex()
            .flex_1()
            .min_h_0()
            .gap(px(8.))
            .child(div().text_size(px(13.)).text_color(fg).child("建议值:"))
            .child(
                v_flex()
                    .id("dv-list")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(if hits.is_empty() {
                        Some(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("无匹配的值"),
                        )
                    } else {
                        None
                    })
                    .children(hits.into_iter().map(move |ix| {
                        let me = me.clone();
                        let value = self.distinct[ix].clone();
                        h_flex()
                            .h(px(ui::DIALOG_ROW_H))
                            .items_center()
                            .pl(px(8.))
                            .hover(move |this| this.bg(to_hsla(p.hover)))
                            .child(ui::checkbox(
                                ("dv", ix),
                                value,
                                self.checked[ix],
                                true,
                                self.dark,
                                move |_, _, cx| {
                                    me.update(cx, |this, cx| {
                                        this.checked[ix] = !this.checked[ix];
                                        cx.notify();
                                    });
                                },
                            ))
                    })),
            )
            .child(
                h_flex()
                    .h(px(32.))
                    .items_center()
                    .gap(px(6.))
                    .border_t_1()
                    .border_color(to_hsla(p.border))
                    .child(
                        crate::art::icon(crate::art::Glyph::Asset("BTN_SEARCH"), 14., muted),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.search)
                                .appearance(false)
                                .bordered(false)
                                .text_size(px(13.)),
                        ),
                    ),
            )
    }
}

impl Render for FilterValueDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);

        v_flex()
            .id("filter-value-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                let m = ev.keystroke.modifiers;
                if ev.keystroke.key == "enter" && !m.control && !m.alt && !m.platform {
                    this.submit(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(16.))
                    .gap(px(12.))
                    .child(self.value_rows(window, cx))
                    .when(self.v2.is_none(), |this| this.child(self.suggest_rows(cx))),
            )
            .child(
                ui::footer_bar(self.dark)
                    .justify_end()
                    .child(
                        ui::primary_button("fv-ok", "确定", self.dark, cx)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.submit(window, cx)),
                            ),
                    )
                    .child(
                        ui::secondary_button("fv-cancel", "取消")
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}
