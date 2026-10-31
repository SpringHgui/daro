//! 「导出表数据」对话框（迁移自 Flutter `lib/data/db_export.dart` 的导出表路径）。
//!
//! 与「导出结果」的区别：这里多 SQL 格式，且写盘走 [table_export] 的分块写入接口
//! （进度 / 取消能力已在模块层实现并有单测；mock 数据量下导出瞬时完成，UI 暂不显示
//! 进度条，待真实驱动接入后再接）。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::export_format::DelimitedStyle;
use crate::palette::Palette;
use crate::table_export::{self, ExportError, TableExportSettings, TableFormat};
use crate::{to_hsla, ui, DaroApp};

use crate::ui::{FOOTER_H, TITLE_H};

fn default_path(table: &str, format: TableFormat) -> String {
    let name = format!("{}.{}", safe_file_stem(table), format.ext());
    dirs::desktop_dir()
        .map(|p| p.join(&name).display().to_string())
        .unwrap_or(name)
}

fn safe_file_stem(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
}

pub fn open_table_export(
    parent: Entity<DaroApp>,
    dark: bool,
    table: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    cx: &mut App,
) {
    let mut options = TitleBar::window_options();
    options.window_min_size = Some(size(px(560.), px(360.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(660.), px(500.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("导出表数据".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| TableExportDialog::new(parent, dark, table, columns, rows, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open table export window");
    }
}

struct TableExportDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    table: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    format: TableFormat,
    style: DelimitedStyle,
    with_header: bool,
    bom: bool,
    pretty_json: bool,
    path: Entity<InputState>,
    auto_path: bool,
    error: Option<String>,
    done: Option<String>,
}

impl TableExportDialog {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        table: String,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let format = TableFormat::Csv;
        let path = cx.new(|cx| InputState::new(window, cx).default_value(default_path(&table, format)));
        cx.subscribe(&path, |this, _, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.export(cx);
            } else {
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
            table,
            columns,
            rows,
            format,
            style: DelimitedStyle::STANDARD,
            with_header: true,
            bom: true,
            pretty_json: true,
            path,
            auto_path: true,
            error: None,
            done: None,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn set_format(&mut self, format: TableFormat, window: &mut Window, cx: &mut Context<Self>) {
        self.format = format;
        self.error = None;
        self.done = None;
        if self.auto_path {
            let text = default_path(&self.table, format);
            self.path
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
        cx.notify();
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = rfd::FileDialog::new()
            .set_title("导出表数据")
            .set_file_name(format!("{}.{}", safe_file_stem(&self.table), self.format.ext()))
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

    fn settings(&self, cx: &Context<Self>) -> TableExportSettings {
        TableExportSettings {
            table: self.table.clone(),
            path: self.path.read(cx).value().trim().to_string(),
            format: self.format,
            csv_style: self.style,
            with_header: self.with_header,
            bom: self.bom,
            pretty_json: self.pretty_json,
            quote_all: false,
            batch_rows: 200,
            is_mysql: true,
        }
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        if self.done.is_some() {
            return;
        }
        let settings = self.settings(cx);
        if settings.path.is_empty() {
            self.error = Some("请先选择导出路径".into());
            cx.notify();
            return;
        }
        if let Some(dir) = std::path::Path::new(&settings.path).parent()
            && !dir.as_os_str().is_empty()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            self.error = Some(format!("创建目录失败：{e}"));
            cx.notify();
            return;
        }
        let cancel = std::sync::atomic::AtomicBool::new(false);
        match table_export::export_table(
            &settings,
            &self.columns,
            &self.rows,
            &mut |_, _| {},
            &cancel,
        ) {
            Ok(outcome) => {
                self.done = Some(format!(
                    "已导出 {} 行 × {} 列（{} 字节）到 {}",
                    outcome.rows,
                    self.columns.len(),
                    outcome.bytes,
                    settings.path
                ));
                self.error = None;
                self.parent.update(cx, |_, cx| cx.notify());
            }
            Err(ExportError::Cancelled) => self.error = Some("已取消".into()),
            Err(ExportError::Io(e)) => self.error = Some(format!("写出文件失败：{e}")),
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
                .child(div().text_xs().text_color(to_hsla(p.foreground)).child("导出表数据")),
        )
    }

    fn format_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = to_hsla(self.pal().selection);
        h_flex()
            .gap(px(6.))
            .items_center()
            .children(TableFormat::ALL.into_iter().map(|fmt| {
                let selected = self.format == fmt;
                let me = cx.entity().clone();
                ui::compact_button(("tfmt", fmt as usize), fmt.label())
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
        let delimited = matches!(self.format, TableFormat::Csv | TableFormat::Txt);
        let mut block = v_flex().gap(px(6.));
        if delimited {
            let mut row = h_flex().gap(px(8.)).items_center();
            if self.format == TableFormat::Csv {
                row = row.child(div().w(px(52.)).text_xs().text_color(muted).child("分隔符"));
                for style in [
                    DelimitedStyle::STANDARD,
                    DelimitedStyle::SEMICOLON,
                    DelimitedStyle::TAB,
                ] {
                    let me = me.clone();
                    let selected = self.style.delimiter == style.delimiter;
                    row = row.child(
                        ui::compact_button(("tdelim", style.delimiter as usize), style.delimiter_label())
                            .when(selected, {
                                let accent = to_hsla(p.selection);
                                move |b| b.bg(accent)
                            })
                            .on_click(move |_, _, cx| {
                                me.update(cx, |this, cx| {
                                    this.style = style;
                                    cx.notify();
                                });
                            }),
                    );
                }
            }
            let me2 = me.clone();
            let me3 = me.clone();
            row = row
                .child(
                    ui::compact_button("theader", "含列标题")
                        .when(self.with_header, {
                            let accent = to_hsla(p.selection);
                            move |b| b.bg(accent)
                        })
                        .on_click(move |_, _, cx| {
                            me2.update(cx, |this, cx| {
                                this.with_header = !this.with_header;
                                this.done = None;
                                cx.notify();
                            });
                        }),
                )
                .child(
                    ui::compact_button("tbom", "UTF-8 BOM")
                        .when(self.bom, {
                            let accent = to_hsla(p.selection);
                            move |b| b.bg(accent)
                        })
                        .on_click(move |_, _, cx| {
                            me3.update(cx, |this, cx| {
                                this.bom = !this.bom;
                                this.done = None;
                                cx.notify();
                            });
                        }),
                );
            block = block.child(row);
        } else if self.format == TableFormat::Json {
            let me4 = me.clone();
            block = block.child(
                h_flex().gap(px(8.)).items_center().child(
                    ui::compact_button("tpretty", "缩进美化")
                        .when(self.pretty_json, {
                            let accent = to_hsla(p.selection);
                            move |b| b.bg(accent)
                        })
                        .on_click(move |_, _, cx| {
                            me4.update(cx, |this, cx| {
                                this.pretty_json = !this.pretty_json;
                                this.done = None;
                                cx.notify();
                            });
                        }),
                ),
            );
        } else if self.format == TableFormat::Sql {
            block = block.child(div().text_xs().text_color(muted).child(
                "按每 200 行一条 INSERT 生成；数字不加引号，NULL 原样，字符串单引号转义。",
            ));
        }
        block
    }
}

impl Render for TableExportDialog {
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
            .id("table-export-root")
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
                    .child(div().text_xs().text_color(accent).child(format!(
                        "导出 {}：{} 列 × {} 行",
                        self.table,
                        self.columns.len(),
                        self.rows.len()
                    )))
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
                                ui::secondary_button("table-export-browse", "…")
                                    .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
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
                        ui::primary_button(
                            "table-export-ok",
                            if finished { "关闭" } else { "导出" },
                            self.dark,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.done.is_some() {
                                window.remove_window();
                            } else {
                                this.export(cx);
                            }
                        })),
                    )
                    .child(
                        ui::secondary_button("table-export-cancel", "取消")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{} 行", self.rows.len())),
                    ),
            )
    }
}
