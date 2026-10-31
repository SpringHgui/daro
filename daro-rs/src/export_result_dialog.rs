//! 「导出结果」对话框（迁移自 Flutter 版 `lib/widgets/result_export_dialog.dart`）。
//!
//! 面向已经取回内存的结果集：格式 + 该格式的选项 + 输出文件，单页布局，
//! 不提供进度 / 停止（Flutter 的同一分工：向导面向服务端分页的大表）。
//! 结果集没有目标表，因此不含 SQL 格式（与 Flutter 一致）。

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::export_format::{encode, DelimitedStyle, ExportFormat};
use crate::palette::Palette;
use crate::{to_hsla, DaroApp};

use crate::ui::{self, FOOTER_H, TITLE_H};

/// 把标签清洗成安全文件名（Flutter `_safeLabel` 同款）
pub fn safe_label(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "result".to_string()
    } else {
        trimmed.to_string()
    }
}

/// 默认导出路径：桌面 + 安全标签 + 扩展名
fn default_path(label: &str, format: ExportFormat) -> String {
    let name = format!("{}.{}", safe_label(label), format.ext());
    dirs::desktop_dir()
        .map(|p| p.join(&name).display().to_string())
        .unwrap_or(name)
}

pub fn open_export_result(
    parent: Entity<DaroApp>,
    dark: bool,
    label: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    cx: &mut App,
) {
    let mut options = TitleBar::window_options();
    options.window_min_size = Some(size(px(520.), px(360.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(620.), px(520.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("导出结果".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| ExportResultDialog::new(parent, dark, label, columns, rows, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open result export window");
    }
}

struct ExportResultDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    label: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    format: ExportFormat,
    style: DelimitedStyle,
    with_header: bool,
    bom: bool,
    quote_all: bool,
    pretty_json: bool,
    path: Entity<InputState>,
    /// 路径是否仍是自动建议值：换格式时跟着换扩展名
    auto_path: bool,
    error: Option<String>,
    done: Option<String>,
}

impl ExportResultDialog {
    #[allow(clippy::too_many_arguments)]
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        label: String,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let format = ExportFormat::Csv;
        let path = cx.new(|cx| {
            InputState::new(window, cx).default_value(default_path(&label, format))
        });
        cx.subscribe(&path, |this, _, event: &InputEvent, cx| match event {
            InputEvent::PressEnter { .. } => this.export(cx),
            _ => {
                this.auto_path = false;
                if this.error.is_some() {
                    this.error = None;
                }
                cx.notify();
            }
        })
        .detach();
        Self {
            parent,
            dark,
            label,
            columns,
            rows,
            format,
            style: DelimitedStyle::STANDARD,
            with_header: true,
            bom: true,
            quote_all: false,
            pretty_json: false,
            path,
            auto_path: true,
            error: None,
            done: None,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn set_format(&mut self, format: ExportFormat, window: &mut Window, cx: &mut Context<Self>) {
        self.format = format;
        self.error = None;
        self.done = None;
        if self.auto_path {
            let text = default_path(&self.label, format);
            self.path
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
        cx.notify();
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = rfd::FileDialog::new()
            .set_title("导出结果")
            .set_file_name(format!(
                "{}.{}",
                safe_label(&self.label),
                self.format.ext()
            ))
            .add_filter(self.format.label(), &[self.format.ext()])
            .save_file();
        if let Some(path) = picked {
            let text = path.display().to_string();
            self.path
                .update(cx, |state, cx| state.set_value(text, window, cx));
            self.auto_path = false;
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
        let content = encode(
            self.format,
            &self.label,
            &self.columns,
            &self.rows,
            self.style,
            self.with_header,
            self.quote_all,
            self.bom && self.format != ExportFormat::Txt,
            self.pretty_json,
        );
        if let Some(dir) = std::path::Path::new(&path).parent() {
            if !dir.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    self.error = Some(format!("创建目录失败：{e}"));
                    cx.notify();
                    return;
                }
            }
        }
        match std::fs::write(&path, content.as_bytes()) {
            Ok(()) => {
                self.done = Some(format!(
                    "已导出 {} 行 × {} 列到 {}",
                    self.rows.len(),
                    self.columns.len(),
                    path
                ));
                self.error = None;
                let parent = self.parent.clone();
                parent.update(cx, |_, cx| cx.notify());
            }
            Err(e) => self.error = Some(format!("写出文件失败：{e}")),
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
                        .child("导出结果"),
                ),
        )
    }

    fn format_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let accent = to_hsla(p.selection);
        h_flex()
            .gap(px(6.))
            .items_center()
            .children(ExportFormat::ALL.into_iter().map(|fmt| {
                let selected = self.format == fmt;
                let me = cx.entity().clone();
                ui::compact_button(("fmt", fmt as usize), fmt.label())
                    .when(selected, move |b| b.bg(accent))
                    .on_click(move |_, window, cx| {
                        me.update(cx, |this, cx| this.set_format(fmt, window, cx));
                    })
            }))
    }

    fn options(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let muted = to_hsla(p.muted_foreground);
        let me = cx.entity().clone();
        let mut block = v_flex().gap(px(6.)).child(
            div()
                .text_xs()
                .text_color(muted)
                .child(self.format.hint().to_string()),
        );
        if self.format.delimited() {
            let fixed = self.format == ExportFormat::Txt;
            let mut delim_row = h_flex().gap(px(6.)).items_center();
            if !fixed {
                delim_row = delim_row.child(div().w(px(52.)).text_xs().text_color(muted).child("分隔符"));
                for style in [
                    DelimitedStyle::STANDARD,
                    DelimitedStyle::SEMICOLON,
                    DelimitedStyle::TAB,
                    DelimitedStyle {
                        delimiter: '|',
                        ..DelimitedStyle::STANDARD
                    },
                ] {
                    let me = me.clone();
                    let selected = self.style.delimiter == style.delimiter;
                    delim_row = delim_row.child(
                        ui::compact_button(("delim", style.delimiter as usize), style.delimiter_label())
                            .when(selected, {
                                let accent = to_hsla(p.selection);
                                move |b| b.bg(accent)
                            })
                            .on_click(move |_, _, cx| {
                                me.update(cx, |this, cx| {
                                    this.style = style;
                                    this.done = None;
                                    cx.notify();
                                });
                            }),
                    );
                }
            }
            block = block.child(delim_row).child(
                h_flex()
                    .gap(px(14.))
                    .items_center()
                    .child(
                        Checkbox::new("exp-header")
                            .checked(self.with_header)
                            .label("含列名")
                            .on_click({
                                let me = me.clone();
                                move |next, _, cx| {
                                    let next = *next;
                                    me.update(cx, |this, cx| {
                                        this.with_header = next;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Checkbox::new("exp-bom")
                            .checked(self.bom)
                            .label("UTF-8 BOM")
                            .on_click({
                                let me = me.clone();
                                move |next, _, cx| {
                                    let next = *next;
                                    me.update(cx, |this, cx| {
                                        this.bom = next;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Checkbox::new("exp-quote-all")
                            .checked(self.quote_all)
                            .label("全部加引号")
                            .on_click({
                                let me = me.clone();
                                move |next, _, cx| {
                                    let next = *next;
                                    me.update(cx, |this, cx| {
                                        this.quote_all = next;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            );
        } else if self.format == ExportFormat::Json {
            block = block.child(
                Checkbox::new("exp-pretty")
                    .checked(self.pretty_json)
                    .label("格式化输出")
                    .on_click({
                        let me = me.clone();
                        move |next, _, cx| {
                            let next = *next;
                            me.update(cx, |this, cx| {
                                this.pretty_json = next;
                                cx.notify();
                            });
                        }
                    }),
            );
        }
        block
    }
}

impl Render for ExportResultDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, border, muted, accent) = (
            to_hsla(p.foreground),
            to_hsla(p.border),
            to_hsla(p.muted_foreground),
            to_hsla(p.selection),
        );
        let status = self
            .done
            .clone()
            .or_else(|| self.error.clone())
            .unwrap_or_default();
        let is_error = self.error.is_some() && self.done.is_none();
        let finished = self.done.is_some();

        v_flex()
            .id("export-result-root")
            .size_full()
            .bg(to_hsla(p.background))
            .text_color(fg)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    window.remove_window();
                    cx.stop_propagation();
                } else if ev.keystroke.key == "enter" {
                    this.export(cx);
                }
            }))
            .child(self.title_bar())
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(16.))
                    .gap(px(10.))
                    .child(
                        div().text_xs().text_color(accent).child(format!(
                            "导出 {} 列 × {} 行结果",
                            self.columns.len(),
                            self.rows.len()
                        )),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(ui::form_label("导出格式", muted))
                            .child(self.format_row(cx)),
                    )
                    .child(self.options(cx))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(ui::form_label("导出到", muted))
                            .child(div().flex_1().child(ui::text_input(&self.path)))
                            .child(
                                ui::secondary_button("exp-result-browse", "…")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.browse(window, cx)
                                    })),
                            ),
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
                        ui::primary_button("exp-result-ok", if finished { "关闭" } else { "导出" }, self.dark, cx)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.done.is_some() {
                                    window.remove_window();
                                } else {
                                    this.export(cx);
                                }
                            })),
                    )
                    .child(
                        ui::secondary_button("exp-result-cancel", "取消")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(div().flex_1())
                    .child(
                        div().text_xs().text_color(muted).child(format!(
                            "{} 行 × {} 列",
                            self.rows.len(),
                            self.columns.len()
                        )),
                    ),
            )
    }
}
