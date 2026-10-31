//! 导出连接为 Navicat `.ncx`（迁移自 Flutter 版
//! `lib/widgets/navicat_export_dialog.dart`）。
//!
//! 口径照 Flutter 版：
//! - 默认全选「Navicat 有对应类型」的连接（Redis 在 .ncx 里没有属性模板，灰显不勾）；
//! - 「全选 / 取消全选」语义固定，不做状态取反；
//! - 默认导出到桌面 `connections.ncx`，可换路径（原生保存对话框）；
//! - 「导出密码」默认勾上，密文用 Navicat 内置算法加密，Navicat 侧可直接解；
//! - 写出失败把原因留在对话框里，不静默吞掉。

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::navicat_ncx::{build_navicat_ncx, navicat_export_blocker, ExportConnection};
use crate::palette::Palette;
use crate::{to_hsla, DaroApp};

use crate::ui::{self, DIALOG_ROW_H, FOOTER_H, TITLE_H};

pub fn open_navicat_export(parent: Entity<DaroApp>, dark: bool, cx: &mut App) {
    let mut options = TitleBar::window_options();
    options.window_min_size = Some(size(px(560.), px(380.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(780.), px(560.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("导出连接".into());
    }
    let conns = parent.read_with(cx, |app, _| app.export_snapshot());
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| ExportDialog::new(parent, dark, conns, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open navicat export window");
    }
}

/// 默认导出路径：桌面 `connections.ncx`（Flutter 版同款）。
fn default_path() -> String {
    dirs::desktop_dir()
        .map(|p| p.join("connections.ncx").display().to_string())
        .unwrap_or_else(|| "connections.ncx".to_string())
}

/// Navicat 的 SettingsSavePath 根：`<文档>\Navicat`。
fn settings_root() -> String {
    dirs::document_dir()
        .map(|p| p.join("Navicat").display().to_string())
        .unwrap_or_default()
}

struct ExportDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    conns: Vec<ExportConnection>,
    checked: Vec<bool>,
    include_passwords: bool,
    path: Entity<InputState>,
    error: Option<String>,
    done: Option<String>,
}

impl ExportDialog {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        conns: Vec<ExportConnection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = cx
            .new(|cx| InputState::new(window, cx).default_value(default_path()));
        cx.subscribe(&path, |this, _, event: &InputEvent, cx| {
            match event {
                InputEvent::PressEnter { .. } => this.export(cx),
                _ => {
                    // 改路径时清掉上一次的失败原因，避免误导
                    if this.error.is_some() {
                        this.error = None;
                    }
                    cx.notify();
                }
            }
        })
        .detach();
        let checked = conns
            .iter()
            .map(|c| navicat_export_blocker(c.kind).is_none())
            .collect();
        Self {
            parent,
            dark,
            conns,
            checked,
            include_passwords: true,
            path,
            error: None,
            done: None,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn is_exportable(&self, ix: usize) -> bool {
        self.conns
            .get(ix)
            .map(|c| navicat_export_blocker(c.kind).is_none())
            .unwrap_or(false)
    }

    fn selected_count(&self) -> usize {
        self.checked
            .iter()
            .enumerate()
            .filter(|(ix, on)| **on && self.is_exportable(*ix))
            .count()
    }

    fn toggle_all(&mut self, on: bool, cx: &mut Context<Self>) {
        for ix in 0..self.checked.len() {
            self.checked[ix] = on && self.is_exportable(ix);
        }
        cx.notify();
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = rfd::FileDialog::new()
            .set_title("导出连接")
            .set_file_name("connections.ncx")
            .add_filter("Navicat 连接文件", &["ncx"])
            .save_file();
        if let Some(path) = picked {
            let text = path.display().to_string();
            self.path
                .update(cx, |state, cx| state.set_value(text, window, cx));
            self.error = None;
            cx.notify();
        }
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        if self.done.is_some() {
            return;
        }
        let path = self.path.read(cx).value().trim().to_string();
        if path.is_empty() {
            self.error = Some("请先选择导出路径".into());
            cx.notify();
            return;
        }
        let picked: Vec<ExportConnection> = self
            .conns
            .iter()
            .enumerate()
            .filter(|(ix, _)| self.checked.get(*ix).copied().unwrap_or(false) && self.is_exportable(*ix))
            .map(|(_, c)| c.clone())
            .collect();
        if picked.is_empty() {
            self.error = Some("没有勾选任何可导出的连接".into());
            cx.notify();
            return;
        }
        let result = build_navicat_ncx(&picked, self.include_passwords, &settings_root());
        if let Some(dir) = std::path::Path::new(&path).parent() {
            if !dir.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    self.error = Some(format!("创建目录失败：{e}"));
                    cx.notify();
                    return;
                }
            }
        }
        match std::fs::write(&path, result.xml.as_bytes()) {
            Ok(()) => {
                let mut summary = format!(
                    "已导出 {} 条连接到 {}",
                    result.exported.len(),
                    path
                );
                if !result.skipped.is_empty() {
                    let names: Vec<String> =
                        result.skipped.iter().map(|(n, _)| n.clone()).collect();
                    summary.push_str(&format!("；跳过 {} 条（{}）", names.len(), names.join("、")));
                }
                self.done = Some(summary);
                self.error = None;
                let parent = self.parent.clone();
                parent.update(cx, |_, cx| cx.notify());
            }
            Err(e) => {
                self.error = Some(format!("写出文件失败：{e}"));
            }
        }
        cx.notify();
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
                        .child("导出连接"),
                ),
        )
    }

    fn list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, muted, border) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.border),
        );
        let me = cx.entity().clone();
        let band = to_hsla(p.chrome);
        v_flex()
            .flex_1()
            .min_h_0()
            .border_1()
            .border_color(border)
            .child(
                h_flex()
                    .h(px(28.))
                    .px(px(8.))
                    .gap(px(8.))
                    .items_center()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        ui::compact_button("export-all", "全选")
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_all(true, cx))),
                    )
                    .child(
                        ui::compact_button("export-none", "取消全选")
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_all(false, cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("共 {} 条", self.conns.len())),
                    ),
            )
            .child(
                v_flex()
                    .id("export-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(if self.conns.is_empty() {
                        vec![div()
                            .p(px(16.))
                            .text_xs()
                            .text_color(muted)
                            .child("还没有任何连接可导出")
                            .into_any_element()]
                    } else {
                        self.conns
                            .iter()
                            .enumerate()
                            .map(|(ix, conn)| {
                                let me = me.clone();
                                let checked = self.checked.get(ix).copied().unwrap_or(false);
                                let blocked = navicat_export_blocker(conn.kind);
                                h_flex()
                                    .h(px(DIALOG_ROW_H))
                                    .px(px(8.))
                                    .gap(px(8.))
                                    .items_center()
                                    .when(ix % 2 == 1, move |this| this.bg(band))
                                    .child(if blocked.is_some() {
                                        div().w(px(14.)).into_any_element()
                                    } else {
                                        Checkbox::new(("export-row", ix))
                                            .checked(checked)
                                            .on_click(move |next, _, cx| {
                                                let next = *next;
                                                me.update(cx, |this, cx| {
                                                    this.checked[ix] = next;
                                                    cx.notify();
                                                });
                                            })
                                            .into_any_element()
                                    })
                                    .child(
                                        div()
                                            .w(px(210.))
                                            .truncate()
                                            .text_xs()
                                            .text_color(if blocked.is_some() { muted } else { fg })
                                            .child(conn.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .w(px(90.))
                                            .text_xs()
                                            .text_color(muted)
                                            .child(conn.kind.label().to_string()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .truncate()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(format!("{}:{}", conn.host, conn.port)),
                                    )
                                    .child(
                                        div()
                                            .w(px(110.))
                                            .truncate()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(if blocked.is_some() {
                                                "不支持 .ncx".to_string()
                                            } else {
                                                conn.group.clone()
                                            }),
                                    )
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>()
                    }),
            )
    }
}

impl Render for ExportDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, border, muted) = (
            to_hsla(p.foreground),
            to_hsla(p.border),
            to_hsla(p.muted_foreground),
        );
        let selected = self.selected_count();
        let can_export = self.done.is_none() && selected > 0;
        let status = self
            .done
            .clone()
            .or_else(|| self.error.clone())
            .unwrap_or_default();
        let is_error = self.error.is_some() && self.done.is_none();
        let me = cx.entity().clone();

        v_flex()
            .id("export-root")
            .size_full()
            .bg(to_hsla(p.background))
            .text_color(fg)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    window.remove_window();
                    cx.stop_propagation();
                }
            }))
            .child(self.title_bar())
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(14.))
                    .gap(px(8.))
                    .child(div().text_xs().text_color(fg).child("连接："))
                    .child(self.list(cx))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                div()
                                    .w(px(56.))
                                    .text_xs()
                                    .text_color(muted)
                                    .child("导出到："),
                            )
                            .child(div().flex_1().child(ui::text_input(&self.path)))
                            .child(
                                ui::secondary_button("export-browse", "…")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.browse(window, cx)
                                    })),
                            ),
                    )
                    .child(
                        Checkbox::new("export-passwords")
                            .checked(self.include_passwords)
                            .label("导出密码（按 Navicat 内置算法加密，Navicat 可直接解密）")
                            .on_click({
                                let me = me.clone();
                                move |next, _, cx| {
                                    let next = *next;
                                    me.update(cx, |this, cx| {
                                        this.include_passwords = next;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(if is_error { to_hsla(0xc0392b) } else { muted })
                            .child(status),
                    ),
            )
            .child(
                h_flex()
                    .h(px(FOOTER_H))
                    .px(px(12.))
                    .gap(px(8.))
                    .items_center()
                    .border_t_1()
                    .border_color(border)
                    .child(
                        ui::primary_button("export-ok", "确定", self.dark, cx)
                            .disabled(!can_export)
                            .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
                    )
                    .child(
                        ui::secondary_button("export-cancel", if self.done.is_some() { "关闭" } else { "取消" })
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("已勾选 {selected} 条")),
                    ),
            )
    }
}
