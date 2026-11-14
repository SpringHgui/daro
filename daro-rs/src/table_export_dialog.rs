//! 「导出表数据」对话框（迁移自 Flutter `lib/data/db_export.dart` 的导出表路径）。
//!
//! 与「导出结果」的区别：这里多 SQL 格式，且写盘走 [table_export] 的分块写入接口。
//! 远程表的整表取数和写盘都在后台线程跑：取数按页回填进度，写盘把
//! [table_export::export_table] 现成的 progress / cancel 参数接上——界面只负责
//! 定时读进度和推取消标志，一行数据都不在这条线程上搬。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;

use crate::export_format::DelimitedStyle;
use crate::palette::Palette;
use crate::table_export::{self, ExportError, TableExportSettings, TableFormat};
use crate::{db_types, mock, to_hsla, ui, DaroApp};

use crate::ui::TITLE_H;

/// 远程整表：导出时按页现取现写，不先物化整张表。
#[derive(Clone)]
pub struct ExportFetch {
    pub conn: mock::Connection,
    pub db: String,
    pub schema: String,
    pub table: String,
    pub where_sql: Option<String>,
    pub orders: Vec<(String, bool)>,
}

/// 导出文件名：去掉 Windows 不允许的路径字符。
fn safe_file_stem(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
}

fn default_path(table: &str, format: TableFormat) -> String {
    let name = format!("{}.{}", safe_file_stem(table), format.ext());
    dirs::desktop_dir()
        .map(|p| p.join(&name).display().to_string())
        .unwrap_or(name)
}

pub fn open_table_export(
    parent: Entity<DaroApp>,
    dark: bool,
    is_mysql: bool,
    table: String,
    columns: Vec<String>,
    // 本地工作集（mock / Access）：调用方移交行所有权，弹窗内按 Arc 共享
    rows: Vec<db_types::Row>,
    fetch: Option<ExportFetch>,
    cx: &mut App,
) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(560.), px(360.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(660.), px(500.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("导出表数据".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            TableExportDialog::new(parent, dark, is_mysql, table, columns, rows, fetch, window, cx)
        })
    });
    if opened.is_err() {
        eprintln!("failed to open table export window");
    }
}

struct TableExportDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    _esc: Subscription,
    table: String,
    columns: Vec<String>,
    /// 整表行：可能很长，按 Arc 共享给后台导出线程
    rows: Arc<Vec<db_types::Row>>,
    /// 远程整表：导出时按页现取现写；None = 行已经在手
    fetch: Option<ExportFetch>,
    /// 后台是否在导出
    busy: bool,
    /// 后台任务的取消标志（界面上按「取消」置位）
    cancel: Arc<AtomicBool>,
    /// 后台进度 (已处理行, 总行数)；远程整表总行数未知时是 None
    progress: Arc<Mutex<(usize, Option<usize>)>>,
    // SQL 档的标识符引号：只有 MySQL 用反引号，SQLite / PG 都走双引号
    is_mysql: bool,
    format: TableFormat,
    style: DelimitedStyle,
    with_header: bool,
    bom: bool,
    pretty_json: bool,
    path: Entity<InputState>,
    auto_path: bool,
    error: Option<String>,
    done: Option<String>,
    /// 本窗口句柄：回执落地用它，`active_window()` 失焦时可能为空
    win: AnyWindowHandle,
}

impl TableExportDialog {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        is_mysql: bool,
        table: String,
        columns: Vec<String>,
        rows: Vec<db_types::Row>,
        fetch: Option<ExportFetch>,
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
        // Esc：导出跑着时先叫停，空闲才关窗（与原来挂在内容根上的口径一致）
        let this_window = window.window_handle();
        let me = cx.entity().downgrade();
        let esc = cx.intercept_keystrokes(move |ev, window, cx| {
            if ev.keystroke.key != "escape" || window.window_handle() != this_window {
                return;
            }
            cx.stop_propagation();
            let Some(this) = me.upgrade() else {
                return;
            };
            if this.read(cx).busy {
                this.update(cx, |this, cx| {
                    this.cancel.store(true, Ordering::Relaxed);
                    cx.notify();
                });
            } else {
                window.remove_window();
            }
        });
        let this = Self {
            parent,
            dark,
            _esc: esc,
            table,
            columns,
            rows: Arc::new(rows),
            fetch,
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new((0, None))),
            is_mysql,
            format,
            style: DelimitedStyle::STANDARD,
            with_header: true,
            bom: true,
            pretty_json: true,
            path,
            auto_path: true,
            error: None,
            done: None,
            win: window.window_handle(),
        };
        this
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
        let dialog = rfd::AsyncFileDialog::new()
            .set_title("导出表数据")
            .set_file_name(format!("{}.{}", safe_file_stem(&self.table), self.format.ext()))
            .add_filter(self.format.label(), &[self.format.ext()]);
        ui::save_file(window, dialog, cx, |this, window, cx, path| {
            let text = path.display().to_string();
            this.path
                .update(cx, |state, cx| state.set_value(text, window, cx));
            this.auto_path = false;
            this.error = None;
            cx.notify();
        });
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
            is_mysql: self.is_mysql,
        }
    }

    /// 写盘：本地工作集按块流式编码；远程表按页现取现写。两者都只让
    /// 一页行和一个编码缓冲区驻留内存。
    fn export(&mut self, cx: &mut Context<Self>) {
        if self.done.is_some() || self.busy {
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
        let handle = cx.active_window().unwrap_or(self.win);
        let entity = cx.entity().downgrade();
        let columns = self.columns.clone();
        let rows = self.rows.clone();
        let fetch = self.fetch.clone();
        let total_hint = fetch.is_none().then_some(rows.len());
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new((0, total_hint)));
        let finished = Arc::new(AtomicBool::new(false));
        self.busy = true;
        self.cancel = cancel.clone();
        self.progress = progress.clone();
        self.error = None;
        cx.notify();

        {
            let cancel = cancel.clone();
            let progress = progress.clone();
            let finished = finished.clone();
            let path = settings.path.clone();
            cx.spawn(async move |_, cx| {
                let res = cx
                    .background_executor()
                    .spawn(async move {
                        let out = if let Some(job) = fetch {
                            let mut offset = 0usize;
                            table_export::export_table_stream(
                                &settings,
                                &columns,
                                None,
                                || {
                                    let page = crate::fetch_table_page(
                                        &job.conn,
                                        &job.db,
                                        &job.schema,
                                        &job.table,
                                        job.where_sql.as_deref(),
                                        &job.orders,
                                        offset,
                                        crate::REMOTE_PAGE_SIZE,
                                    )?;
                                    offset += page.rows.len();
                                    if page.rows.is_empty() {
                                        Ok(None)
                                    } else {
                                        Ok(Some(
                                            page.rows
                                                .into_iter()
                                                .map(std::sync::Arc::new)
                                                .collect(),
                                        ))
                                    }
                                },
                                &mut |done, total| {
                                    *progress.lock().unwrap() = (done, total);
                                },
                                &cancel,
                            )
                        } else {
                            table_export::export_table(
                                &settings,
                                &columns,
                                rows.as_slice(),
                                &mut |done, total| {
                                    *progress.lock().unwrap() = (done, Some(total));
                                },
                                &cancel,
                            )
                        };
                        finished.store(true, Ordering::Relaxed);
                        out
                    })
                    .await;
                let _ = handle.update(cx, |_, _, app| {
                    let Some(ent) = entity.upgrade() else { return };
                    ent.update(app, |d, cx| d.export_done(res, &path, cx));
                });
            })
            .detach();
        }
        self.track_progress(handle, finished, cx);
    }

    fn export_done(
        &mut self,
        res: Result<table_export::ExportOutcome, ExportError>,
        path: &str,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        match res {
            Ok(outcome) => {
                self.done = Some(format!(
                    "已导出 {} 行 × {} 列（{} 字节）到 {}",
                    outcome.rows,
                    self.columns.len(),
                    outcome.bytes,
                    path
                ));
                self.error = None;
                let parent = self.parent.clone();
                parent.update(cx, |_, cx| cx.notify());
            }
            Err(ExportError::Cancelled) => self.error = Some("已取消".into()),
            Err(ExportError::Io(e)) => self.error = Some(format!("写出文件失败：{e}")),
            Err(ExportError::Fetch(e)) => self.error = Some(format!("取数失败：{e}")),
        }
        cx.notify();
    }

    /// 后台有活在跑时，每 120ms 通知界面重绘一次，把共享的进度原子量画出来。
    /// 跨线程进度只能是「后台写共享量 + 界面定时读」，不必自造事件队列。
    fn track_progress(
        &mut self,
        handle: AnyWindowHandle,
        finished: Arc<AtomicBool>,
        cx: &mut Context<Self>,
    ) {
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            while !finished.load(Ordering::Relaxed) {
                cx.background_executor().timer(Duration::from_millis(120)).await;
                let _ = handle.update(cx, |_, _, app| {
                    let Some(ent) = entity.upgrade() else { return };
                    ent.update(app, |_, cx| cx.notify());
                });
            }
            // 收尾那次重绘由 export_done 负责
        })
        .detach();
    }

    fn title_bar(&self) -> impl IntoElement {
        let p = self.pal();
        TitleBar::new().h(px(TITLE_H)).border_b_0().child(
            h_flex()
                .h_full()
                .items_center()
                .gap(px(6.))
                .pl(px(4.))
                .child(div().text_xs().text_color(to_hsla(p.foreground)).child("导出表数据")),
        )
    }

    fn format_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        h_flex()
            .gap(px(6.))
            .items_center()
            .children(TableFormat::ALL.into_iter().map(|fmt| {
                let me = cx.entity().clone();
                let selected = self.format == fmt;
                ui::segment(
                    ("tfmt", fmt as usize),
                    fmt.label(),
                    selected,
                    dark,
                    move |_, window, cx| {
                        me.update(cx, |this, cx| this.set_format(fmt, window, cx));
                    },
                )
            }))
    }

    fn options(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let muted = to_hsla(p.muted_foreground);
        let dark = self.dark;
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
                    row = row.child(ui::segment(
                        ("tdelim", style.delimiter as usize),
                        style.delimiter_label(),
                        selected,
                        dark,
                        move |_, _, cx| {
                            me.update(cx, |this, cx| {
                                this.style = style;
                                cx.notify();
                            });
                        },
                    ));
                }
            }
            let me2 = me.clone();
            let me3 = me.clone();
            row = row
                .child(ui::checkbox(
                    "theader",
                    "含列标题",
                    self.with_header,
                    true,
                    dark,
                    move |_, _, cx| {
                        me2.update(cx, |this, cx| {
                            this.with_header = !this.with_header;
                            this.done = None;
                            cx.notify();
                        });
                    },
                ))
                .child(ui::checkbox(
                    "tbom",
                    "UTF-8 BOM",
                    self.bom,
                    true,
                    dark,
                    move |_, _, cx| {
                        me3.update(cx, |this, cx| {
                            this.bom = !this.bom;
                            this.done = None;
                            cx.notify();
                        });
                    },
                ));
            block = block.child(row);
        } else if self.format == TableFormat::Json {
            let me4 = me.clone();
            block = block.child(
                h_flex().gap(px(8.)).items_center().child(ui::checkbox(
                    "tpretty",
                    "缩进美化",
                    self.pretty_json,
                    true,
                    dark,
                    move |_, _, cx| {
                        me4.update(cx, |this, cx| {
                            this.pretty_json = !this.pretty_json;
                            this.done = None;
                            cx.notify();
                        });
                    },
                )),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, muted) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
        );
        let status = match self.busy {
            true => {
                let (done, total) = *self.progress.lock().unwrap();
                match total {
                    Some(total) if total > 0 => format!("导出中… {done} / {total} 行"),
                    _ => format!("导出中… 已处理 {done} 行"),
                }
            }
            false => self
                .done
                .clone()
                .or_else(|| self.error.clone())
                .unwrap_or_default(),
        };
        let busy = self.busy;
        let is_error = !busy && self.error.is_some() && self.done.is_none();
        let finished = self.done.is_some();
        let count = if self.fetch.is_some() {
            "整表（按页读取）".to_string()
        } else {
            format!("{} 行", self.rows.len())
        };

        v_flex()
            .id("table-export-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                if ev.keystroke.key == "enter" {
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
                    .child(div().text_xs().text_color(fg).child(format!(
                        "导出 {}：{} 列 × {}",
                        self.table,
                        self.columns.len(),
                        count
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
                            .child(
                            div()
                                .flex_1()
                                .child(ui::field_input(&self.path, self.dark, window, cx)),
                        )
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
                ui::footer_bar(self.dark)
                    .child(
                        ui::primary_button_enabled(
                            "table-export-ok",
                            if finished { "关闭" } else { "导出" },
                            self.dark,
                            !busy,
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
                        ui::secondary_button("table-export-cancel", "取消").on_click(
                            cx.listener(|this, _, window, cx| {
                                if this.busy {
                                    this.cancel.store(true, Ordering::Relaxed);
                                    cx.notify();
                                } else {
                                    window.remove_window();
                                }
                            }),
                        ),
                    )
                    .child(div().flex_1())
                    .child(div().text_xs().text_color(muted).child(count)),
            )
    }
}
