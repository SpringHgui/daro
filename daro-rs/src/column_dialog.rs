//! 「列」管理对话框（数据页工具条「列」按钮入口）。
//!
//! 列表按期望显示顺序列出全部列，每行可勾选「显示」并上下重排；隐藏列仍出现在
//! 列表里（灰态文字），方便重新勾选显示。确定时把「顺序 + 可见性」折算成
//! `OpenTab.col_order` 的有序子集写回活动数据页签——网格渲染只遍历 `col_order()`，
//! 因此隐藏列会自动从表头与单元格里消失，无需改动渲染逻辑。

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::palette::Palette;
use crate::ui;
use crate::{to_hsla, DaroApp};

/// 打开列管理对话框。
///
/// `names` 是全部列名（按下标），`order` 是当前可见列的原始下标子集（显示顺序）。
pub fn open_column_manager(
    parent: Entity<DaroApp>,
    dark: bool,
    names: Vec<String>,
    order: Vec<usize>,
    cx: &mut App,
) {
    // 期望显示顺序的全排列：可见子集在前（保持当前顺序），隐藏列按自然序补到后面，
    // 这样列表里始终能看到被隐藏的列，可重新勾选显示。
    let mut perm: Vec<usize> = order.clone();
    let mut seen = vec![false; names.len()];
    for &c in &perm {
        seen[c] = true;
    }
    for c in 0..names.len() {
        if !seen[c] {
            perm.push(c);
        }
    }
    let visible: Vec<bool> = (0..names.len()).map(|c| order.contains(&c)).collect();

    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(360.), px(320.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(420.), px(460.)), cx));
    if let Some(tb) = options.titlebar.as_mut() {
        tb.title = Some("列".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            let esc = ui::close_on_escape(window, cx);
            ColumnDialog {
                parent,
                dark,
                names,
                perm,
                visible,
                sel: 0,
                _esc: esc,
            }
        })
    });
    if opened.is_err() {
        eprintln!("[column_dialog] 打开列管理窗失败");
    }
}

struct ColumnDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    /// 全部列名，按下标
    names: Vec<String>,
    /// 期望显示顺序的全排列（含隐藏列，排在可见子集之后）
    perm: Vec<usize>,
    /// 按原始列下标的可见性
    visible: Vec<bool>,
    /// 列表当前选中行（perm 下标）
    sel: usize,
    _esc: Subscription,
}

impl ColumnDialog {
    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn toggle(&mut self, c: usize, cx: &mut Context<Self>) {
        self.visible[c] = !self.visible[c];
        cx.notify();
    }

    fn move_up(&mut self, i: usize, cx: &mut Context<Self>) {
        if i > 0 {
            self.perm.swap(i, i - 1);
            self.sel = i - 1;
            cx.notify();
        }
    }

    fn move_down(&mut self, i: usize, cx: &mut Context<Self>) {
        if i + 1 < self.perm.len() {
            self.perm.swap(i, i + 1);
            self.sel = i + 1;
            cx.notify();
        }
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let order: Vec<usize> = self
            .perm
            .iter()
            .copied()
            .filter(|&c| self.visible[c])
            .collect();
        let parent = self.parent.clone();
        parent.update(cx, |app, cx| {
            app.set_column_order(order, cx);
        });
        window.remove_window();
    }

    fn title_bar(&self) -> impl IntoElement {
        let p = self.pal();
        TitleBar::new()
            .h(px(crate::ui::TITLE_H))
            .border_b_0()
            .child(
                h_flex()
                    .h_full()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(4.))
                    .child(div().text_xs().text_color(to_hsla(p.foreground)).child("列")),
            )
    }
}

impl Render for ColumnDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let border = to_hsla(p.border);
        let sel_bg = to_hsla(p.selection);

        v_flex()
            .id("col-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .child(self.title_bar())
            .child(
                v_flex()
                    .id("col-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(8.))
                    .gap(px(2.))
                    .children(self.perm.iter().enumerate().map(|(i, &c)| {
                        let name = self.names[c].clone();
                        let vis = self.visible[c];
                        let seld = i == self.sel;
                        h_flex()
                            .id(format!("col-row-{i}"))
                            .h(px(30.))
                            .items_center()
                            .gap(px(6.))
                            .px(px(8.))
                            .when(seld, |s| s.bg(sel_bg))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.sel = i;
                                cx.notify();
                            }))
                            .child(
                                ui::compact_button(format!("col-up-{i}"), "↑").on_click(
                                    cx.listener(move |this, _, _, cx| this.move_up(i, cx)),
                                ),
                            )
                            .child(
                                ui::compact_button(format!("col-down-{i}"), "↓").on_click(
                                    cx.listener(move |this, _, _, cx| this.move_down(i, cx)),
                                ),
                            )
                            .child(
                                div()
                                    .id(format!("col-vis-{i}"))
                                    .w(px(16.))
                                    .h(px(16.))
                                    .flex_none()
                                    .border_1()
                                    .border_color(border)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(vis, |s| s.child(div().text_color(fg).child("✓")))
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.toggle(c, cx)),
                                    ),
                            )
                            .child(div().text_color(if vis { fg } else { muted }).child(name))
                    })),
            )
            .child(
                ui::footer_bar(self.dark)
                    .child(
                        ui::secondary_button("col-cancel", "取消")
                            .on_click(cx.listener(|_, _, window, _cx| {
                                window.remove_window();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::secondary_button("col-ok", "确定")
                            .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
                    ),
            )
    }
}
