//! 「历史」对话框（数据页工具条「历史」按钮入口）。
//!
//! 列出自动记录的已执行 SQL：时间、连接 / 库、SQL 预览、成功 / 失败、耗时。
//! 支持按连接筛选；每行可「重新运行」（灌入新查询页签）或「复制」；底部可清空。

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui;

use chrono::{LocalResult, TimeZone, Utc};

use crate::history;
use crate::palette::Palette;
use crate::ui;
use crate::{to_hsla, DaroApp};

/// 打开查询历史对话框。
pub fn open_history_dialog(parent: Entity<DaroApp>, dark: bool, cx: &mut App) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(520.), px(360.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(760.), px(540.)), cx));
    if let Some(tb) = options.titlebar.as_mut() {
        tb.title = Some("查询历史".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            let esc = ui::close_on_escape(window, cx);
            HistoryDialog {
                parent,
                dark,
                filter: None,
                _esc: esc,
            }
        })
    });
    if opened.is_err() {
        eprintln!("[history_dialog] 打开历史窗失败");
    }
}

/// Unix 秒 → 本地时间字符串（UTC 解析，无需 chrono 的 clock feature）。
fn fmt_ts(ts: i64) -> String {
    match Utc.timestamp_opt(ts, 0) {
        LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => ts.to_string(),
    }
}

struct HistoryDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    /// None = 全部连接；Some(n) = 仅该连接名。
    filter: Option<String>,
    _esc: Subscription,
}

impl HistoryDialog {
    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    /// 去重连接清单（按出现顺序），用于筛选下拉。
    fn conn_list(&self, cx: &App) -> Vec<String> {
        let mut out = Vec::new();
        for e in &self.parent.read(cx).history {
            if !out.contains(&e.conn) {
                out.push(e.conn.clone());
            }
        }
        out
    }

    /// 按当前筛选过滤后的记录（最新在前）。
    fn filtered(&self, cx: &App) -> Vec<history::HistoryEntry> {
        let hist = &self.parent.read(cx).history;
        let mut v: Vec<history::HistoryEntry> = hist
            .iter()
            .filter(|e| self.filter.as_ref().map_or(true, |f| &e.conn == f))
            .cloned()
            .collect();
        v.reverse();
        v
    }

    /// 点击筛选标签：在「全部 → 各连接 → 全部」间循环。
    fn cycle_filter(&mut self, cx: &mut Context<Self>) {
        let conns = self.conn_list(cx);
        self.filter = match &self.filter {
            None => conns.first().cloned(),
            Some(c) => match conns.iter().position(|x| x == c) {
                Some(i) if i + 1 < conns.len() => Some(conns[i + 1].clone()),
                _ => None,
            },
        };
        cx.notify();
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
                    .child(
                        div()
                            .text_xs()
                            .text_color(to_hsla(p.foreground))
                            .child("查询历史"),
                    ),
            )
    }
}

impl Render for HistoryDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let border = to_hsla(p.border);
        let ok_color = to_hsla(p.accent);
        let err_color = to_hsla(if self.dark { 0xe06c75 } else { 0xc0392b });

        let filter_label = match &self.filter {
            None => "全部连接".to_string(),
            Some(c) => c.clone(),
        };

        let items = self.filtered(cx);
        let parent = self.parent.clone();

        v_flex()
            .id("hist-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .child(self.title_bar())
            .child(
                h_flex()
                    .h(px(34.))
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .border_b_1()
                    .border_color(border)
                    .child(div().text_xs().text_color(muted).child("筛选："))
                    .child(
                        ui::secondary_button("hist-filter", &filter_label).on_click(
                            cx.listener(|this, _, _, cx| this.cycle_filter(cx)),
                        ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("共 {} 条", items.len())),
                    ),
            )
            .child(
                v_flex()
                    .id("hist-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(8.))
                    .gap(px(4.))
                    .children(items.into_iter().enumerate().map(|(i, e)| {
                        let ts = fmt_ts(e.ts);
                        let conn_db = format!("{}/{}", e.conn, e.db);
                        let preview: String = e
                            .sql
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim()
                            .chars()
                            .take(120)
                            .collect::<String>();
                        let preview = if preview.len() < e.sql.trim().len() {
                            format!("{preview}…")
                        } else {
                            preview
                        };
                        let status = if e.ok { "成功" } else { "失败" };
                        let sql = e.sql.clone();
                        let conn = e.conn.clone();
                        let ix = parent.read(cx).conns.iter().position(|c| c.name == conn);
                        h_flex()
                            .id(format!("hist-row-{i}"))
                            .items_start()
                            .gap(px(8.))
                            .p(px(6.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(border)
                            .child(
                                div()
                                    .w(px(132.))
                                    .flex_none()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(ts),
                            )
                            .child(
                                div()
                                    .w(px(150.))
                                    .flex_none()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(conn_db),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(fg)
                                    .child(preview),
                            )
                            .child(
                                div()
                                    .w(px(40.))
                                    .flex_none()
                                    .text_xs()
                                    .text_color(if e.ok { ok_color } else { err_color })
                                    .child(status),
                            )
                            .child(
                                div()
                                    .w(px(72.))
                                    .flex_none()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{} 行 · {}ms", e.rows, e.ms)),
                            )
                            .child(
                                ui::compact_button(format!("hist-rerun-{i}"), "重新运行").on_click({
                                    let parent = parent.clone();
                                    let sql = sql.clone();
                                    cx.listener(move |_, _, window, cx| {
                                        if let Some(ix) = ix {
                                            parent.update(cx, |a, cx| {
                                                a.open_sql_text_in_new_query(
                                                    &sql, Some(ix), window, cx,
                                                );
                                            });
                                        }
                                        window.remove_window();
                                    })
                                }),
                            )
                            .child(
                                ui::compact_button(format!("hist-copy-{i}"), "复制").on_click({
                                    let sql = sql.clone();
                                    cx.listener(move |_, _, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            sql.clone(),
                                        ));
                                    })
                                }),
                            )
                    })),
            )
            .child(
                ui::footer_bar(self.dark)
                    .child(
                        ui::secondary_button("hist-clear", "清空").on_click(cx.listener(
                            |this, _, _, cx| {
                                let f = this.filter.clone();
                                let parent = this.parent.clone();
                                parent.update(cx, |a, cx| {
                                    a.clear_history(f, cx);
                                });
                                cx.notify();
                            },
                        )),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::secondary_button("hist-close", "关闭").on_click(
                            cx.listener(|_, _, window, _cx| {
                                window.remove_window();
                            }),
                        ),
                    ),
            )
    }
}
