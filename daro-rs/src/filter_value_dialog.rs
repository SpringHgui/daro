//! 筛选值输入窗（Navicat TFilterValuePopupEditForm 的多窗口对应物）：
//! 值输入 + 建议值勾选列表 + 搜索，确定后回写准则值。
//! Navicat 实测窗口 ≈ 413×331 逻辑像素；介于双值档给两个输入、不出建议列表
//! （该形态未实测到勾选交互，mock 档先按双输入落地）。

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::DaroApp;
use crate::palette::Palette;
use crate::to_hsla;

use crate::ui::{self, FOOTER_H, TITLE_H};
const LIST_H: f32 = 150.;

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
    let mut options = TitleBar::window_options();
    options.window_min_size = Some(size(px(360.), px(280.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(420.), px(360.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some(title.into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        let on_ok: OnOk = Box::new(on_ok);
        cx.new(|cx| {
            FilterValueDialog::new(parent, dark, title, v1, v2, distinct, on_ok, window, cx)
        })
    });
    if opened.is_err() {
        eprintln!("failed to open filter value window");
    }
}

struct FilterValueDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    title: &'static str,
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
        title: &'static str,
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
        Self {
            parent,
            dark,
            title,
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

    fn title_bar(&self) -> impl IntoElement {
        let p = self.pal();
        TitleBar::new().h(px(TITLE_H)).child(
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

    fn value_rows(&self) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        v_flex()
            .gap(px(8.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .items_center()
                    .child(div().w(px(56.)).text_xs().text_color(fg).child("值"))
                    .child(div().flex_1().child(ui::text_input(&self.v1))),
            )
            .when_some(self.v2.clone(), |this, v2| {
                this.child(
                    h_flex()
                        .gap(px(10.))
                        .items_center()
                        .child(div().w(px(56.)).text_xs().text_color(fg).child("到"))
                        .child(div().flex_1().child(ui::text_input(&v2))),
                )
            })
    }

    fn suggest_rows(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = self.pal();
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
            .gap(px(6.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("建议值：{}", self.distinct.len())),
                    )
                    .child(div().w(px(140.)).child(ui::text_input(&self.search))),
            )
            .child(
                v_flex()
                    .id("dv-list")
                    .h(px(LIST_H))
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(to_hsla(p.border))
                    .p(px(6.))
                    .gap(px(2.))
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
                        Checkbox::new(("dv", ix))
                            .checked(self.checked[ix])
                            .label(value)
                            .on_click(move |checked, _, cx| {
                                let checked = *checked;
                                me.update(cx, |this, cx| {
                                    this.checked[ix] = checked;
                                    cx.notify();
                                });
                            })
                    })),
            )
    }
}

impl Render for FilterValueDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, border) = (to_hsla(p.foreground), to_hsla(p.border));

        v_flex()
            .id("filter-value-root")
            .size_full()
            .bg(to_hsla(p.background))
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
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(16.))
                    .gap(px(12.))
                    .child(self.value_rows())
                    .when(self.v2.is_none(), |this| this.child(self.suggest_rows(cx))),
            )
            .child(
                h_flex()
                    .h(px(FOOTER_H))
                    .px(px(12.))
                    .gap(px(8.))
                    .items_center()
                    .border_t_1()
                    .border_color(border)
                    .child(div().flex_1())
                    .child(
                        ui::secondary_button("fv-cancel", "取消")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(ui::primary_button("fv-ok", "确定", self.dark, cx).on_click(
                        cx.listener(|this, _, window, cx| this.submit(window, cx)),
                    )),
            )
    }
}

