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
use crate::{db_types, FetchAbort, load_all_table_rows, mock, to_hsla, ui, DaroApp};

use crate::ui::TITLE_H;

/// 对话框要自己在后台取的整表：打开时只带列名和取数条件，行稍后到。
#[derive(Clone)]
pub struct ExportFetch {
    pub conn: mock::Connection,
    pub db: String,
    pub schema: String,
    pub table: String,
    pub where_sql: Option<String>,
    pub orders: Vec<(String, bool)>,
}

/// 后台正在干什么（决定状态行文案和按钮可用性）。
#[derive(Clone, Copy, PartialEq)]
enum Busy {
    Fetch,
    Export,
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
    rows: Vec<Vec<String>>,
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
    table: String,
    columns: Vec<String>,
    /// 整表行：后台取回来之后可能很长，克隆一次就是几十 MB，故按 Arc 共享
    rows: Arc<Vec<Vec<String>>>,
    /// 还没取回的行（远程整表）；None = 行已经在手
    fetch: Option<ExportFetch>,
    /// 行是否已就绪（空表也是「就绪」，不能拿 rows.is_empty() 判断）
    fetched: bool,
    /// 后台在跑什么：取数 / 写盘 / 空
    busy: Option<Busy>,
    /// 后台任务的取消标志（界面上按「取消」置位）
    cancel: Arc<AtomicBool>,
    /// 后台进度 (已处理行, 总行数)，后台线程写、界面定时读
    progress: Arc<Mutex<(usize, usize)>>,
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
        rows: Vec<Vec<String>>,
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
        let fetched = fetch.is_none();
        let mut this = Self {
            parent,
            dark,
            table,
            columns,
            rows: Arc::new(rows),
            fetch,
            fetched,
            busy: None,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new((0, 0))),
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
        this.start_fetch(window, cx);
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
            is_mysql: self.is_mysql,
        }
    }

    /// 远程整表取数：打开对话框就开跑，行回来之前导出按钮不亮。
    fn start_fetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(job) = self.fetch.clone() else {
            return;
        };
        let handle = window.window_handle();
        let entity = cx.entity().downgrade();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new((0, 0)));
        let finished = Arc::new(AtomicBool::new(false));
        self.busy = Some(Busy::Fetch);
        self.cancel = cancel.clone();
        self.progress = progress.clone();
        // 约定：先 notify 再占后台——按钮和「取数中」要在取数开始前就画出来
        cx.notify();

        {
            let cancel = cancel.clone();
            let progress = progress.clone();
            let finished = finished.clone();
            cx.spawn(async move |_, cx| {
                let res = cx
                    .background_executor()
                    .spawn(async move {
                        let out = load_all_table_rows(
                            &job.conn,
                            &job.db,
                            &job.schema,
                            &job.table,
                            job.where_sql.as_deref(),
                            &job.orders,
                            &mut |done, total| *progress.lock().unwrap() = (done, total),
                            &cancel,
                        );
                        finished.store(true, Ordering::Relaxed);
                        out
                    })
                    .await;
                let _ = handle.update(cx, |_, _, app| {
                    let Some(ent) = entity.upgrade() else { return };
                    ent.update(app, |d, cx| d.fetch_done(res, cx));
                });
            })
            .detach();
        }
        self.track_progress(handle, finished, cx);
    }

    fn fetch_done(&mut self, res: Result<db_types::SelectPage, FetchAbort>, cx: &mut Context<Self>) {
        self.busy = None;
        match res {
            Ok(page) => {
                self.rows = Arc::new(page.rows);
                self.fetched = true;
                self.fetch = None;
                self.error = None;
            }
            Err(FetchAbort::Cancelled) => self.error = Some("已取消取数，数据未就绪".into()),
            Err(FetchAbort::Failed(e)) => self.error = Some(format!("取数失败：{e}")),
        }
        cx.notify();
    }

    /// 写盘：`export_table` 的分块写入本身带 progress / cancel，这里只把它
    /// 挪到后台，并定时把进度搬上屏。
    fn export(&mut self, cx: &mut Context<Self>) {
        if self.done.is_some() || self.busy.is_some() {
            return;
        }
        if !self.fetched {
            self.error = Some("数据还没取回来".into());
            cx.notify();
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
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new((0, self.rows.len())));
        let finished = Arc::new(AtomicBool::new(false));
        self.busy = Some(Busy::Export);
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
                        let out = table_export::export_table(
                            &settings,
                            &columns,
                            rows.as_slice(),
                            &mut |done, total| *progress.lock().unwrap() = (done, total),
                            &cancel,
                        );
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
        self.busy = None;
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
            // 收尾那次重绘由 fetch_done / export_done 负责
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
            Some(kind) => {
                let (done, total) = *self.progress.lock().unwrap();
                let what = if kind == Busy::Fetch { "取数中" } else { "导出中" };
                if total == 0 {
                    format!("{what}…")
                } else {
                    format!("{what}… {done} / {total} 行")
                }
            }
            None => self
                .done
                .clone()
                .or_else(|| self.error.clone())
                .unwrap_or_default(),
        };
        let busy = self.busy.is_some();
        let is_error = !busy && self.error.is_some() && self.done.is_none();
        let finished = self.done.is_some();
        let count = if self.fetched {
            format!("{} 行", self.rows.len())
        } else {
            "取数中…".to_string()
        };

        v_flex()
            .id("table-export-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    if this.busy.is_some() {
                        this.cancel.store(true, Ordering::Relaxed);
                        cx.notify();
                    } else {
                        window.remove_window();
                    }
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
                            !(busy || (!finished && !self.fetched)),
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
                            cx.listener(|this, _, window, cx| match this.busy {
                                // 后台在跑时「取消」只置标志位，等回执回来再收尾
                                Some(_) => {
                                    this.cancel.store(true, Ordering::Relaxed);
                                    cx.notify();
                                }
                                None => window.remove_window(),
                            }),
                        ),
                    )
                    .child(div().flex_1())
                    .child(div().text_xs().text_color(muted).child(count)),
            )
    }
}
