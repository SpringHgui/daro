//! `.ncx` 兼容格式 导入向导（迁移自 Flutter 版
//! `lib/widgets/ncx_import_dialog.dart`）。
//!
//! 流程与 Flutter 版一致：选文件 → 解析 + 解密已保存密码 → 勾选条目 → 写入连接树。
//! 差异只在文件选择：Flutter 用 `file_selector`，这里用原生 `rfd`（同一个
//! Windows 文件对话框）。解析/解密/分组/私有序号改名全部复用
//! [crate::ncx] 与 [crate::DaroApp::import_connections]。
//!
//! Flutter 版对 379 条的真实导出文件用懒加载列表；这里先一次性铺满
//! （条目都是「勾选框 + 四段文字」的轻量行），真机上如果出现卡顿再换
//! `VirtualList`。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;

use crate::ncx::{NcxFile, PasswordState};
use crate::palette::Palette;
use crate::{to_hsla, DaroApp, ImportItem};

use crate::ui::{self, DIALOG_ROW_H, TITLE_H};

pub fn open_ncx_import(parent: Entity<DaroApp>, dark: bool, cx: &mut App) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(560.), px(360.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(820.), px(560.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("导入连接".into());
    }
    let existing: Vec<String> = parent.read_with(cx, |app, _| app.conn_names());
    let existing_groups: Vec<String> = parent.read_with(cx, |app, _| app.group_names());
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| ImportDialog::new(parent, dark, existing, existing_groups, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open ncx import window");
    }
}

struct ImportDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    _esc: Subscription,
    path: Entity<InputState>,
    ncx: Option<NcxFile>,
    error: Option<String>,
    checked: Vec<bool>,
    /// 与现有连接（或文件内更早条目）重名的下标：默认不勾，勾了会加 " (2)" 序号
    duplicates: Vec<bool>,
    /// 现有连接名，判重用
    existing: Vec<String>,
    /// 现有分组名，标「·新建」用
    existing_groups: Vec<String>,
    /// 导入完成后的汇总文案
    done: Option<String>,
}

impl ImportDialog {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        existing: Vec<String>,
        existing_groups: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = cx.new(|cx| InputState::new(window, cx).placeholder("选择或粘贴 .ncx / .ano 文件路径"));
        cx.subscribe(&path, move |this, _, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let text = this.path.read(cx).value().trim().to_string();
                this.parse_path(text, cx);
            }
        })
        .detach();
        let esc = ui::close_on_escape(window, cx);
        Self {
            parent,
            dark,
            _esc: esc,
            path,
            ncx: None,
            error: None,
            checked: Vec::new(),
            duplicates: Vec::new(),
            existing,
            existing_groups,
            done: None,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn parse_path(&mut self, path: String, cx: &mut Context<Self>) {
        self.done = None;
        if path.trim().is_empty() {
            self.ncx = None;
            self.error = None;
            self.checked.clear();
            self.duplicates.clear();
            cx.notify();
            return;
        }
        match std::fs::read_to_string(&path) {
            Ok(mut text) => {
                if text.starts_with('\u{feff}') {
                    text.remove(0);
                }
                match NcxFile::parse(&text) {
                    Ok(ncx) => {
                        let mut seen: Vec<String> = self.existing.clone();
                        let mut duplicates = Vec::with_capacity(ncx.connections.len());
                        for c in &ncx.connections {
                            let name = c.name().to_string();
                            let dup = seen.iter().any(|n| *n == name);
                            duplicates.push(dup);
                            seen.push(name);
                        }
                        self.checked = duplicates.iter().map(|d| !d).collect();
                        self.duplicates = duplicates;
                        self.ncx = Some(ncx);
                        self.error = None;
                    }
                    Err(e) => {
                        self.ncx = None;
                        self.error = Some(e);
                    }
                }
            }
            Err(e) => {
                self.ncx = None;
                self.error = Some(format!("读取文件失败：{e}"));
            }
        }
        cx.notify();
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // rfd 的同步文件对话框自带消息循环；放在点击回调里会把 gpui 事件循环
        // 卡住到用户关对话框为止——与原生 modal 的表现一致，可接受。
        let picked = rfd::FileDialog::new()
            .set_title("导入连接")
            .add_filter("连接文件", &["ncx", "ano"])
            .pick_file();
        if let Some(path) = picked {
            let text = path.display().to_string();
            self.path
                .update(cx, |state, cx| state.set_value(text.clone(), window, cx));
            self.parse_path(text, cx);
        }
    }

    fn toggle_all(&mut self, checked: bool, cx: &mut Context<Self>) {
        for c in self.checked.iter_mut() {
            *c = checked;
        }
        cx.notify();
    }

    fn selected_count(&self) -> usize {
        match &self.ncx {
            Some(ncx) => ncx
                .connections
                .iter()
                .enumerate()
                .filter(|(ix, conn)| {
                    conn.unsupported_reason().is_none()
                        && self.checked.get(*ix).copied().unwrap_or(false)
                })
                .count(),
            None => 0,
        }
    }

    fn do_import(&mut self, cx: &mut Context<Self>) {
        if self.done.is_some() {
            return;
        }
        let Some(ncx) = self.ncx.clone() else {
            return;
        };
        let mut items: Vec<ImportItem> = Vec::new();
        for (ix, conn) in ncx.connections.iter().enumerate() {
            if !self.checked.get(ix).copied().unwrap_or(false) {
                continue;
            }
            if conn.unsupported_reason().is_some() {
                continue;
            }
            let Some(kind) = conn.db_kind() else {
                continue;
            };
            let port = conn
                .port()
                .parse::<u16>()
                .unwrap_or_else(|_| kind.default_port());
            items.push(ImportItem {
                name: conn.name().to_string(),
                kind,
                host: conn.host().to_string(),
                port,
                database: conn.database().to_string(),
                user: conn.user_name().to_string(),
                password: if conn.password_state == PasswordState::Decrypted {
                    conn.password.clone()
                } else {
                    String::new()
                },
                group: conn.group().to_string(),
            });
        }
        if items.is_empty() {
            self.error = Some("没有可导入的条目（选中项为空或类型不受支持）".into());
            cx.notify();
            return;
        }
        let parent = self.parent.clone();
        let outcome = parent.update(cx, |app, cx| {
            let outcome = app.import_connections(items);
            cx.notify();
            outcome
        });
        let mut summary = format!("已导入 {} 条连接", outcome.imported);
        if outcome.renamed > 0 {
            summary.push_str(&format!("，其中 {} 条重名已自动改名", outcome.renamed));
        }
        if !outcome.new_groups.is_empty() {
            summary.push_str(&format!("，新建分组：{}", outcome.new_groups.join("、")));
        }
        self.done = Some(summary);
        cx.notify();
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
                        .child("导入连接"),
                ),
        )
    }

    fn file_row(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let muted = to_hsla(p.muted_foreground);
        h_flex()
            .gap(px(8.))
            .items_center()
            .child(ui::form_label("文件", muted))
            .child(
                div()
                    .flex_1()
                    .child(ui::field_input(&self.path, self.dark, window, cx)),
            )
            .child(
                ui::secondary_button("import-browse", "浏览…")
                    .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
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
        let total = self.ncx.as_ref().map(|n| n.connections.len()).unwrap_or(0);
        let fmt_label = self
            .ncx
            .as_ref()
            .map(|n| n.format.label())
            .unwrap_or("");
        v_flex()
            .flex_1()
            .min_h_0()
            .border_1()
            .border_color(border)
            .child(
                h_flex()
                    .h(px(32.))
                    .px(px(6.))
                    .gap(px(2.))
                    .items_center()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        ui::compact_button("import-all", "全选")
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_all(true, cx))),
                    )
                    .child(
                        ui::compact_button("import-none", "全不选")
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_all(false, cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(if fmt_label.is_empty() {
                                format!("共 {total} 条")
                            } else {
                                format!("{fmt_label} · 共 {total} 条")
                            }),
                    ),
            )
            .child(
                v_flex()
                    .id("import-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(self.ncx.as_ref().map(|ncx| {
                        ncx.connections
                            .iter()
                            .enumerate()
                            .map(|(ix, conn)| {
                                let me = me.clone();
                                let checked = self.checked.get(ix).copied().unwrap_or(false);
                                let blocker = conn.unsupported_reason();
                                let unsupported = blocker.is_some();
                                let kind_label = conn
                                    .db_kind()
                                    .map(|k| k.label())
                                    .unwrap_or(conn.conn_type());
                                let target = if conn.is_file_based() {
                                    conn.file_path().to_string()
                                } else {
                                    format!("{}:{}", conn.host(), conn.port())
                                };
                                let group = conn.group().to_string();
                                // 本地还没有的分组标「·新建」（Flutter 版同款口径）
                                let new_group = !group.is_empty()
                                    && !self.existing_groups.iter().any(|g| *g == group);
                                let badge = match conn.password_state {
                                    PasswordState::Decrypted => "",
                                    PasswordState::NoPassword => "无密码",
                                    PasswordState::NotSaved => "密码未保存",
                                    PasswordState::Undecryptable => "密码无法解密",
                                };
                                h_flex()
                                    .h(px(DIALOG_ROW_H))
                                    .px(px(8.))
                                    .gap(px(8.))
                                    .items_center()
                                    .hover(move |this| this.bg(to_hsla(p.hover)))
                                    .child(if unsupported {
                                        div().w(px(14.)).into_any_element()
                                    } else {
                                        ui::checkbox(
                                            ("import-row", ix),
                                            "",
                                            checked,
                                            true,
                                            self.dark,
                                            move |_, _, cx| {
                                                me.update(cx, |this, cx| {
                                                    this.checked[ix] = !this.checked[ix];
                                                    cx.notify();
                                                });
                                            },
                                        )
                                    })
                                    .child(
                                        div()
                                            .w(px(210.))
                                            .truncate()
                                            .text_xs()
                                            .text_color(if unsupported { muted } else { fg })
                                            .child(conn.name().to_string()),
                                    )
                                    .child(
                                        div()
                                            .w(px(88.))
                                            .truncate()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(match &blocker {
                                                Some(reason) => reason.clone(),
                                                None => kind_label.to_string(),
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .truncate()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(target),
                                    )
                                    .child(
                                        div()
                                            .w(px(96.))
                                            .truncate()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(if new_group {
                                                format!("{} ·新建", group)
                                            } else {
                                                group
                                            }),
                                    )
                                    .child(
                                        div()
                                            .w(px(96.))
                                            .truncate()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(badge.to_string()),
                                    )
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>()
                    })
                    .into_iter()
                    .flatten()),
            )
    }
}

impl Render for ImportDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, muted) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
        );
        let can_import = self.done.is_none() && self.selected_count() > 0;
        let total = self.ncx.as_ref().map(|n| n.connections.len()).unwrap_or(0);
        let selected = self.selected_count();
        let status = self
            .done
            .clone()
            .or_else(|| self.error.clone())
            .unwrap_or_else(|| {
                if let Some(ncx) = &self.ncx {
                    format!(
                        "{} · 已选 {selected} / 共 {total} 条",
                        ncx.format.label()
                    )
                } else {
                    "选择 .ncx 或 Another Redis Desktop Manager 的 .ano 文件开始导入"
                        .to_string()
                }
            });
        let is_error = self.error.is_some() && self.done.is_none();

        v_flex()
            .id("import-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .child(self.title_bar())
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(14.))
                    .gap(px(10.))
                    .child(self.file_row(window, cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(if is_error { to_hsla(0xc0392b) } else { muted })
                            .child(status),
                    )
                    .child(self.list(cx)),
            )
            .child(
                ui::footer_bar(self.dark)
                    .child(
                        ui::primary_button_enabled("import-ok", "导入", self.dark, can_import, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.do_import(cx))),
                    )
                    .child(
                        ui::secondary_button("import-cancel", if self.done.is_some() { "关闭" } else { "取消" })
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("已选 {selected} / 共 {total} 条")),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_test_support::{isolate_store, open_app};
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt;

    // 显式引回标准 test 宏：`use super::*` 会把 gpui 的同名 test 宏带进作用域
    use core::prelude::v1::test;

    #[gpui_kit::test]
    fn ncx_import_dialog_closes_on_escape(cx: &mut TestAppContext) {
        isolate_store("ncxesc");
        let (_main, app) = open_app(cx);
        let handle = cx
            .update(|cx| {
                open_ncx_import(app, false, cx);
                cx.windows().last().copied()
            })
            .expect("ncx import window opened");
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press("escape", cx);
        })
        .unwrap();
        assert_eq!(cx.windows().len(), 1, "Esc 关导入连接窗");
    }
}
