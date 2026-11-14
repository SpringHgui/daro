//! 「导入向导」对话框：把 CSV / JSON 文件导入一张**已存在**的表。
//!
//! 三步（照 Flutter 版 `lib/widgets/data_import_wizard.dart`）：
//! 1. **源文件**：选文件 + 解析方式（CSV 分隔符 / 表头 / 跳行 / 修剪）+ 预览；
//! 2. **字段映射**：目标列 ← 源列（同名自动匹配，可手工改）；
//! 3. **执行导入**：空值转 NULL / 批大小 / 先清空 + 执行（后台跑，带进度与取消）。
//!
//! 解析与 SQL 组装都在 [crate::data_import] 的纯函数里；这里只管界面与
//! 后台调度。写入走 [crate::backend::Target::execute]，演示连接 / mock 引擎
//! 走不到真库，如实报「演示数据，不落库」。

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use crate::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::data_import::{
    auto_map, build_insert_batches, import_rows, read_sample, ImportColumn, ImportFormat,
    ImportOptions, ImportResult, ImportSource,
};
use crate::mock::Connection;
use crate::palette::Palette;
use crate::{backend, catalog, to_hsla, ui, DaroApp};

use crate::ui::TITLE_H;

/// 导入目标：一张已存在的表
#[derive(Clone)]
pub struct ImportTarget {
    pub conn: Connection,
    pub db: String,
    pub schema: String,
    pub table: String,
}

/// 三个步骤的标题
const STEPS: [&str; 3] = ["源文件", "字段映射", "执行导入"];

pub fn open_data_import(parent: Entity<DaroApp>, dark: bool, target: ImportTarget, cx: &mut App) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(640.), px(480.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(900.), px(620.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("导入向导".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| ImportDialog::new(parent, dark, target, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open data import window");
    }
}

struct ImportDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    target: ImportTarget,
    _esc: Subscription,
    /// 当前步骤（0 源文件 / 1 字段映射 / 2 执行）
    step: usize,
    path: Entity<InputState>,
    /// 分隔符（CSV）
    delimiter_input: Entity<InputState>,
    /// 跳过行数
    skip_input: Entity<InputState>,
    /// 每批行数
    batch_input: Entity<InputState>,
    opts: ImportOptions,
    /// 源文件预览（列标题 + 前几行）
    source: Option<ImportSource>,
    /// 目标表列 → 源列的映射
    mapping: Vec<ImportColumn>,
    error: Option<String>,
    done: Option<String>,
    /// 导入在后台跑
    busy: bool,
    progress: Arc<Mutex<(usize, usize)>>,
    cancel: Arc<AtomicBool>,
    win: AnyWindowHandle,
}

impl ImportDialog {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        target: ImportTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = cx.new(|cx| InputState::new(window, cx).placeholder("选择 CSV / JSON 文件"));
        cx.subscribe(&path, move |this, _, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let text = this.path.read(cx).value().trim().to_string();
                this.load_source(text, cx);
            }
        })
        .detach();

        // 分隔符 / 跳行 / 每批行数：三个短输入框
        let delimiter_input = cx.new(|cx| InputState::new(window, cx).default_value(","));
        let skip_input = cx.new(|cx| InputState::new(window, cx).default_value("0"));
        let batch_input = cx.new(|cx| InputState::new(window, cx).default_value("200"));
        cx.subscribe(&delimiter_input, |this, _, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let raw = this.delimiter_input.read(cx).value().to_string();
            if let Some(c) = raw.chars().next() {
                this.opts.delimiter = c;
                this.reparse(cx);
            }
        })
        .detach();
        cx.subscribe(&skip_input, |this, _, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let raw = this.skip_input.read(cx).value().trim().to_string();
            this.opts.skip_rows = raw.parse::<usize>().unwrap_or(0);
            this.reparse(cx);
        })
        .detach();
        cx.subscribe(&batch_input, |this, _, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let raw = this.batch_input.read(cx).value().trim().to_string();
            this.opts.batch_size = raw.parse::<usize>().unwrap_or(200).max(1);
            cx.notify();
        })
        .detach();

        let win = window.window_handle();
        let esc = ui::close_on_escape(window, cx);
        let mut me = Self {
            parent,
            dark,
            target,
            _esc: esc,
            step: 0,
            path,
            delimiter_input,
            skip_input,
            batch_input,
            opts: ImportOptions::default(),
            source: None,
            mapping: Vec::new(),
            error: None,
            done: None,
            busy: false,
            progress: Arc::new(Mutex::new((0, 0))),
            cancel: Arc::new(AtomicBool::new(false)),
            win,
        };
        me.build_mapping(cx);
        me
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    /// 目标表的列 → 映射项（初次按同名自动匹配）
    fn build_mapping(&mut self, cx: &mut Context<Self>) {
        let cols = catalog::table_columns(
            &self.target.conn,
            &self.target.db,
            &self.target.schema,
            &self.target.table,
        );
        let targets: Vec<String> = cols.iter().map(|c| c.name.clone()).collect();
        let headers = self
            .source
            .as_ref()
            .map(|s| s.headers.clone())
            .unwrap_or_default();
        let auto = auto_map(&headers, &targets);
        let mut mapping: Vec<ImportColumn> = cols
            .iter()
            .zip(auto.iter())
            .map(|(c, src)| ImportColumn {
                target_column: c.name.clone(),
                type_label: c.ty.clone(),
                source_index: *src,
                nullable: true,
            })
            .collect();
        if mapping.is_empty() {
            // 目录还没就绪（演示连接）：先留一个占位项，别让映射页空掉
            mapping.push(ImportColumn::new(&self.target.table, ""));
        }
        self.mapping = mapping;
        cx.notify();
    }

    /// 读文件 → 解析预览 → 重算自动映射
    fn load_source(&mut self, path: String, cx: &mut Context<Self>) {
        self.done = None;
        if path.trim().is_empty() {
            self.source = None;
            self.error = None;
            cx.notify();
            return;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(mut t) => {
                if t.starts_with('\u{feff}') {
                    t.remove(0);
                }
                t
            }
            Err(e) => {
                self.source = None;
                self.error = Some(format!("读取文件失败：{e}"));
                cx.notify();
                return;
            }
        };
        match read_sample(&text, &self.opts, 20) {
            Ok(src) => {
                self.source = Some(src);
                self.error = None;
                self.build_mapping(cx);
            }
            Err(e) => {
                self.source = None;
                self.error = Some(e);
                cx.notify();
            }
        }
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = rfd::AsyncFileDialog::new()
            .set_title("选择导入文件")
            .add_filter("数据文件", &["csv", "txt", "json"]);
        ui::pick_file(window, dialog, cx, |this, window, cx, path| {
            let text = path.display().to_string();
            this.path
                .update(cx, |state, cx| state.set_value(text.clone(), window, cx));
            this.load_source(text, cx);
        });
    }

    /// 换格式 / 改解析选项后重读预览（文件内容已在内存里，重解析即可）
    fn reparse(&mut self, cx: &mut Context<Self>) {
        let path = self.path.read(cx).value().trim().to_string();
        if path.is_empty() {
            cx.notify();
            return;
        }
        self.load_source(path, cx);
    }

    fn set_format(&mut self, format: ImportFormat, cx: &mut Context<Self>) {
        self.opts.format = format;
        self.reparse(cx);
    }

    /// 切换某个目标列的来源列
    fn set_source_index(&mut self, target_ix: usize, source_ix: Option<usize>, cx: &mut Context<Self>) {
        if let Some(m) = self.mapping.get_mut(target_ix) {
            m.source_index = source_ix;
        }
        cx.notify();
    }

    fn mapped_count(&self) -> usize {
        self.mapping
            .iter()
            .filter(|m| m.source_index.is_some())
            .count()
    }

    /// 执行导入（后台线程跑，界面只搬进度）
    fn run(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.done.is_some() {
            return;
        }
        if self.mapping.iter().all(|m| m.source_index.is_none()) {
            self.error = Some("未选中任何可导入的列".into());
            cx.notify();
            return;
        }
        let path = self.path.read(cx).value().trim().to_string();
        if path.is_empty() {
            self.error = Some("请先选择源文件".into());
            cx.notify();
            return;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(mut t) => {
                if t.starts_with('\u{feff}') {
                    t.remove(0);
                }
                t
            }
            Err(e) => {
                self.error = Some(format!("读取文件失败：{e}"));
                cx.notify();
                return;
            }
        };
        let opts = self.opts.clone();
        let mapping = self.mapping.clone();
        let conn = self.target.conn.clone();
        let db = self.target.db.clone();
        let schema = self.target.schema.clone();
        let table = self.target.table.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new((0, 0)));
        self.cancel = cancel.clone();
        self.progress = progress.clone();
        self.busy = true;
        self.error = None;
        self.done = None;
        cx.notify();

        let handle = cx.active_window().unwrap_or(self.win);
        let entity = cx.entity().downgrade();
        {
            let cancel = cancel.clone();
            let progress = progress.clone();
            cx.spawn(async move |_, cx| {
                let res = cx
                    .background_executor()
                    .spawn(async move {
                        run_import(
                            &text,
                            &opts,
                            &mapping,
                            &conn,
                            &db,
                            &schema,
                            &table,
                            &mut |done, total| *progress.lock().unwrap() = (done, total),
                            &cancel,
                        )
                    })
                    .await;
                let _ = handle.update(cx, |_, _, app| {
                    let Some(ent) = entity.upgrade() else { return };
                    ent.update(app, |d, cx| d.import_done(res, cx));
                });
            })
            .detach();
        }
        self.track_progress(cx);
    }

    /// 定时把后台进度搬上屏
    fn track_progress(&mut self, cx: &mut Context<Self>) {
        let progress = self.progress.clone();
        let cancel = self.cancel.clone();
        let handle = cx.active_window().unwrap_or(self.win);
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let (done, total) = *progress.lock().unwrap();
                let _ = handle.update(cx, |_, _, app| {
                    let Some(ent) = entity.upgrade() else { return };
                    ent.update(app, |d, cx| {
                        d.busy = true;
                        d.progress = Arc::new(Mutex::new((done, total)));
                        cx.notify();
                    });
                });
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
            }
        })
        .detach();
    }

    fn import_done(&mut self, res: Result<ImportResult, String>, cx: &mut Context<Self>) {
        self.busy = false;
        match res {
            Ok(r) => {
                let mut summary = format!("导入完成：写入 {} 行 / 读取 {} 行", r.rows_inserted, r.rows_read);
                if r.failed_batches > 0 {
                    summary.push_str(&format!("；{} 批失败", r.failed_batches));
                }
                if r.cancelled {
                    summary.push_str("；已取消");
                }
                if !r.errors.is_empty() {
                    summary.push_str(&format!("；首个错误：{}", r.errors[0]));
                }
                self.done = Some(summary);
                self.error = None;
                // 导完立刻看到新行：让主窗口在目标表正是当前页签时重取一页
                if r.rows_inserted > 0 {
                    let target = self.target.clone();
                    self.parent.update(cx, move |app, cx| {
                        app.reload_after_import(&target, cx);
                    });
                }
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    // ── 渲染 ──────────────────────────────────────────────────

    fn step_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        h_flex()
            .h(px(32.))
            .gap(px(6.))
            .items_center()
            .children(STEPS.iter().enumerate().map(|(i, label)| {
                let active = self.step == i;
                h_flex()
                    .id(("imp-step", i))
                    .test_support()
                    .px(px(10.))
                    .h(px(22.))
                    .items_center()
                    .rounded(px(ui::RADIUS))
                    .cursor_pointer()
                    .when(active, |s| s.bg(to_hsla(p.selection)))
                    .when(!active, |s| {
                        s.hover(move |s| s.bg(to_hsla(p.hover)))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.step = i;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(if active { fg } else { muted })
                            .child(format!("{} · {label}", i + 1)),
                    )
            }))
    }

    fn step_source(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let muted = to_hsla(p.muted_foreground);
        let fg = to_hsla(p.foreground);
        let border = to_hsla(p.border);
        let me = cx.entity().clone();

        v_flex()
            .flex_1()
            .min_h_0()
            .gap(px(8.))
            .child(
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
                        ui::secondary_button("imp-browse", "浏览…").on_click(
                            cx.listener(|this, _, window, cx| this.browse(window, cx)),
                        ),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(ui::form_label("格式", muted))
                    .child(ui::segment(
                        "imp-fmt-csv",
                        "CSV / 文本",
                        self.opts.format == ImportFormat::Csv,
                        self.dark,
                        cx.listener(|this, _, _, cx| this.set_format(ImportFormat::Csv, cx)),
                    ))
                    .child(ui::segment(
                        "imp-fmt-json",
                        "JSON",
                        self.opts.format == ImportFormat::Json,
                        self.dark,
                        cx.listener(|this, _, _, cx| this.set_format(ImportFormat::Json, cx)),
                    ))
                    .child(ui::form_label("分隔符", muted))
                    .child(
                        div()
                            .w(px(46.))
                            .child(
                                Input::new(&self.delimiter_input())
                                    .id("imp-delim")
                                    .h(px(24.))
                                    .text_size(px(12.)),
                            ),
                    )
                    .child(
                        ui::checkbox(
                            "imp-header",
                            "首行是列名",
                            self.opts.has_header,
                            true,
                            self.dark,
                            {
                                let me = me.clone();
                                move |_, _, cx| {
                                    me.update(cx, |this, cx| {
                                        this.opts.has_header = !this.opts.has_header;
                                        this.reparse(cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        ui::checkbox(
                            "imp-trim",
                            "去除字段首尾空白",
                            self.opts.trim_fields,
                            true,
                            self.dark,
                            {
                                let me = me.clone();
                                move |_, _, cx| {
                                    me.update(cx, |this, cx| {
                                        this.opts.trim_fields = !this.opts.trim_fields;
                                        this.reparse(cx);
                                    });
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(ui::form_label("跳过行数", muted))
                    .child(
                        div()
                            .w(px(46.))
                            .child(
                                Input::new(&self.skip_input())
                                    .id("imp-skip")
                                    .h(px(24.))
                                    .text_size(px(12.)),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(match &self.source {
                                Some(s) => format!(
                                    "预览 {} 列 × {} 行{}",
                                    s.headers.len(),
                                    s.sample.len(),
                                    s.warning
                                        .as_ref()
                                        .map(|w| format!("（{w}）"))
                                        .unwrap_or_default()
                                ),
                                None => "还没有解析到源文件".to_string(),
                            }),
                    ),
            )
            .child(self.preview_table(border, fg, muted))
    }

    /// 分隔符 / 跳行 / 每批行数三个输入框的实体（渲染时直接取用）
    fn delimiter_input(&self) -> Entity<InputState> {
        self.delimiter_input.clone()
    }

    fn skip_input(&self) -> Entity<InputState> {
        self.skip_input.clone()
    }

    fn preview_table(
        &self,
        border: gpui_kit::Hsla,
        fg: gpui_kit::Hsla,
        muted: gpui_kit::Hsla,
    ) -> impl IntoElement {
        let Some(src) = &self.source else {
            return div()
                .flex_1()
                .min_h_0()
                .border_1()
                .border_color(border)
                .p(px(12.))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("选好文件后这里显示前几行数据"),
                )
                .into_any_element();
        };
        v_flex()
            .id("imp-preview")
            .test_support()
            .flex_1()
            .min_h_0()
            .border_1()
            .border_color(border)
            .overflow_y_scroll()
            .child(
                h_flex()
                    .h(px(24.))
                    .flex_none()
                    .border_b_1()
                    .border_color(border)
                    .children(src.headers.iter().map(|h| {
                        div()
                            .w(px(120.))
                            .flex_none()
                            .px(px(4.))
                            .truncate()
                            .text_xs()
                            .text_color(fg)
                            .child(h.clone())
                    })),
            )
            .children(src.sample.iter().map(|row| {
                h_flex()
                    .h(px(22.))
                    .flex_none()
                    .children((0..src.headers.len()).map(|i| {
                        div()
                            .w(px(120.))
                            .flex_none()
                            .px(px(4.))
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .child(row.get(i).cloned().unwrap_or_default())
                    }))
            }))
            .into_any_element()
    }

    fn step_mapping(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let muted = to_hsla(p.muted_foreground);
        let fg = to_hsla(p.foreground);
        let border = to_hsla(p.border);
        let headers = Rc::new(
            self.source
                .as_ref()
                .map(|s| s.headers.clone())
                .unwrap_or_default(),
        );
        let me = cx.entity().clone();

        v_flex()
            .flex_1()
            .min_h_0()
            .gap(px(8.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(ui::form_label("目标表", muted))
                    .child(
                        div()
                            .text_xs()
                            .text_color(fg)
                            .child(format!(
                                "{}.{}",
                                self.target.db,
                                self.target.table
                            )),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("已映射 {} / {} 列", self.mapped_count(), self.mapping.len())),
                    ),
            )
            .child(
                v_flex()
                    .id("imp-maplist")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(border)
                    .overflow_y_scroll()
                    .children(self.mapping.iter().enumerate().map(|(ix, m)| {
                        let me = me.clone();
                        let label = match m.source_index {
                            Some(s) => headers
                                .get(s)
                                .cloned()
                                .unwrap_or_else(|| format!("列{}", s + 1)),
                            None => "（不导入）".to_string(),
                        };
                        h_flex()
                            .h(px(ui::DIALOG_ROW_H))
                            .px(px(6.))
                            .gap(px(8.))
                            .items_center()
                            .hover(move |this| this.bg(to_hsla(p.hover)))
                            .child(
                                div()
                                    .w(px(180.))
                                    .flex_none()
                                    .truncate()
                                    .text_xs()
                                    .text_color(fg)
                                    .child(m.target_column.clone()),
                            )
                            .child(
                                div()
                                    .w(px(90.))
                                    .flex_none()
                                    .truncate()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(m.type_label.clone()),
                            )
                            .child(
                                Button::new(("imp-map-btn", ix))
                                    .label(format!("{label} ▾"))
                                    .h(px(22.))
                                    .text_size(px(12.))
                                    .max_w(px(220.))
                                    .dropdown_menu({
                                        let headers = headers.clone();
                                        move |menu, _, _cx| {
                                            let me = me.clone();
                                            let mut m = menu.item(
                                                PopupMenuItem::new("（不导入）").on_click({
                                                    let me = me.clone();
                                                    move |_, _, cx| {
                                                        me.update(cx, |this, cx| {
                                                            this.set_source_index(ix, None, cx)
                                                        });
                                                    }
                                                }),
                                            );
                                            for (sx, h) in headers.iter().enumerate() {
                                                let me = me.clone();
                                                m = m.item(
                                                    PopupMenuItem::new(h.clone()).on_click(move |_, _, cx| {
                                                        me.update(cx, |this, cx| {
                                                            this.set_source_index(ix, Some(sx), cx)
                                                        });
                                                    }),
                                                );
                                            }
                                            m
                                        }
                                    }),
                            )
                    })),
            )
    }

    fn step_run(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let muted = to_hsla(p.muted_foreground);
        let fg = to_hsla(p.foreground);
        let border = to_hsla(p.border);
        let me = cx.entity().clone();
        let (done, total) = *self.progress.lock().unwrap();

        v_flex()
            .flex_1()
            .min_h_0()
            .gap(px(8.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        ui::checkbox(
                            "imp-null",
                            "空字段导入为 NULL",
                            self.opts.empty_as_null,
                            true,
                            self.dark,
                            {
                                let me = me.clone();
                                move |_, _, cx| {
                                    me.update(cx, |this, cx| {
                                        this.opts.empty_as_null = !this.opts.empty_as_null;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        ui::checkbox(
                            "imp-truncate",
                            "导入前清空目标表",
                            self.opts.truncate_first,
                            true,
                            self.dark,
                            {
                                let me = me.clone();
                                move |_, _, cx| {
                                    me.update(cx, |this, cx| {
                                        this.opts.truncate_first = !this.opts.truncate_first;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(ui::form_label("每批行数", muted))
                    .child(
                        div()
                            .w(px(60.))
                            .child(
                                Input::new(&self.batch_input())
                                    .id("imp-batch")
                                    .h(px(24.))
                                    .text_size(px(12.)),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(if self.busy {
                                format!("导入中… {done} / {total} 行")
                            } else {
                                format!("每批 {} 行 INSERT", self.opts.batch_size)
                            }),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(border)
                    .p(px(12.))
                    .gap(px(6.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(fg)
                            .child(format!(
                                "目标：{}.{}（{}）",
                                self.target.db,
                                self.target.table,
                                self.target.conn.kind.label()
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(if backend::is_real_conn(&self.target.conn) {
                                "导入会真的写进数据库：按批执行 INSERT，失败批次整批不写入。"
                            } else {
                                "这条连接是演示数据 / mock 引擎，导入只统计行数，不落库。"
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("已映射 {} 列", self.mapped_count())),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        ui::primary_button_enabled(
                            "imp-run",
                            "开始导入",
                            self.dark,
                            !(self.busy || self.mapping.is_empty()),
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.run(cx))),
                    )
                    .child(
                        ui::secondary_button("imp-cancel-run", "停止")
                            .disabled(!self.busy)
                            .on_click({
                                let me = me.clone();
                                move |_, _, cx| {
                                    me.update(cx, |this, _| {
                                        this.cancel.store(true, Ordering::Relaxed);
                                    });
                                }
                            }),
                    ),
            )
    }

    fn batch_input(&self) -> Entity<InputState> {
        self.batch_input.clone()
    }
}

/// 后台跑的导入：解析 → 按映射组装批次 → 逐批执行。
fn run_import(
    text: &str,
    opts: &ImportOptions,
    mapping: &[ImportColumn],
    conn: &Connection,
    db: &str,
    schema: &str,
    table: &str,
    progress: &mut dyn FnMut(usize, usize),
    cancel: &AtomicBool,
) -> Result<ImportResult, String> {
    let rows = import_rows(text, opts)?;
    let total = rows.len();
    progress(0, total);
    let batches = build_insert_batches(&rows, mapping, opts, conn.kind, schema, table);

    let target = backend::Target::of(conn, db, schema);
    let mut result = ImportResult {
        rows_read: total,
        ..Default::default()
    };
    if target.is_none() {
        // 演示连接 / mock 引擎：没有真库落点，如实报「不落库」
        result.rows_inserted = total;
        progress(total, total);
        return Ok(result);
    }
    let target = target.expect("checked above");

    if opts.truncate_first {
        let del = format!("DELETE FROM {}", crate::data_import::qualified_name(conn.kind, schema, table));
        if let Err(e) = target.execute(&del, 0) {
            return Err(format!("清空目标表失败：{e}"));
        }
    }

    let mut written = 0usize;
    let batch_size = opts.batch_size.max(1);
    for (ix, sql) in batches.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            result.cancelled = true;
            break;
        }
        // 每批的行数：除最后一批外都是 batch_size
        let rows_in_batch = batch_size.min(total.saturating_sub(ix * batch_size));
        match target.execute(sql, 0) {
            Ok(_) => {
                written += rows_in_batch;
            }
            Err(e) => {
                result.failed_batches += 1;
                if result.errors.len() < ImportResult::MAX_ERRORS {
                    result.errors.push(format!("第 {} 批：{e}", ix + 1));
                }
            }
        }
        progress(written, total);
    }
    result.rows_inserted = written;
    Ok(result)
}

impl Render for ImportDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, muted) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
        );
        let is_error = self.error.is_some() && self.done.is_none();
        let status = self
            .done
            .clone()
            .or_else(|| self.error.clone())
            .unwrap_or_default();

        v_flex()
            .id("import-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .child(
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
                                .child("导入向导"),
                        ),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(14.))
                    .gap(px(10.))
                    .child(self.step_bar(cx))
                    .child(match self.step {
                        1 => self.step_mapping(cx).into_any_element(),
                        2 => self.step_run(cx).into_any_element(),
                        _ => self.step_source(window, cx).into_any_element(),
                    })
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
                        ui::secondary_button("imp-prev", "上一步")
                            .disabled(self.step == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.step = this.step.saturating_sub(1);
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::primary_button_enabled(
                            "imp-next",
                            "下一步",
                            self.dark,
                            !(self.step >= 2 || self.source.is_none()),
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.step = (this.step + 1).min(2);
                            cx.notify();
                        })),
                    )
                    .child(ui::secondary_button("imp-close", "关闭").on_click(
                        |_, window, _| window.remove_window(),
                    ))
                    .child(div().flex_1()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_test_support::{isolate_store, open_app};
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt;

    // 显式引回标准 test 宏：`use super::*` 会把 gpui 的同名 test 宏带进作用域，
    // 不然 #[gpui_kit::test] 生成的内部 #[test] 会再次命中 gpui 宏 → 无限递归
    use core::prelude::v1::test;

    /// 造一个只含 users 表的真 SQLite 库；连接名同时就是驱动注册表的键。
    fn sqlite_users(tag: &str) -> (Connection, String) {
        let db = std::env::temp_dir().join(format!("daro_imp_{tag}_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let path = db.to_string_lossy().to_string();
        crate::sqlite_driver::open(tag, &path).unwrap();
        crate::sqlite_driver::execute_stmt(tag, "CREATE TABLE users (id int, name text)", 1).unwrap();
        (
            Connection::new(
                tag,
                crate::mock::DbKind::SQLite,
                "",
                0,
                path.clone(),
                "",
                "",
                "",
                false,
            ),
            path,
        )
    }

    fn mapping_of(targets: &[(&str, Option<usize>)]) -> Vec<ImportColumn> {
        targets
            .iter()
            .map(|(name, src)| ImportColumn {
                target_column: (*name).to_string(),
                type_label: "text".into(),
                source_index: *src,
                nullable: true,
            })
            .collect()
    }

    /// 端到端：CSV → 解析 → 按映射 → 批量 INSERT → 真 SQLite 落库。
    #[test]
    fn import_writes_rows_into_real_sqlite_table() {
        let (conn, path) = sqlite_users("impw");
        let csv = "id,name\n1,张三\n2,李四\n";
        let opts = ImportOptions {
            batch_size: 1,
            ..Default::default()
        };
        let mapping = mapping_of(&[("id", Some(0)), ("name", Some(1))]);
        let res = run_import(
            csv,
            &opts,
            &mapping,
            &conn,
            "main",
            "",
            "users",
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .expect("import");
        assert_eq!(res.rows_read, 2, "{res:?}");
        assert_eq!(res.rows_inserted, 2, "{res:?}");
        assert_eq!(res.failed_batches, 0, "{res:?}");
        assert!(res.ok(), "{res:?}");

        let out = crate::sqlite_driver::execute_stmt(
            "impw",
            "SELECT id, name FROM users ORDER BY id",
            10,
        )
        .expect("select");
        let rows = out.select.expect("select page").rows;
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0], vec!["1", "张三"]);
        assert_eq!(rows[1], vec!["2", "李四"]);
        let _ = std::fs::remove_file(&path);
    }

    /// 导入前清空：先把表塞脏，再带 truncate_first 导一遍，旧行必须没了。
    #[test]
    fn import_truncates_target_table_when_asked() {
        let (conn, path) = sqlite_users("impt");
        crate::sqlite_driver::execute_stmt(
            "impt",
            "INSERT INTO users (id, name) VALUES (99, '旧行')",
            1,
        )
        .unwrap();
        let opts = ImportOptions {
            truncate_first: true,
            ..Default::default()
        };
        let mapping = mapping_of(&[("id", Some(0)), ("name", Some(1))]);
        let res = run_import(
            "id,name\n1,新行\n",
            &opts,
            &mapping,
            &conn,
            "main",
            "",
            "users",
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .expect("import");
        assert_eq!(res.rows_inserted, 1, "{res:?}");
        let out = crate::sqlite_driver::execute_stmt(
            "impt",
            "SELECT id, name FROM users",
            10,
        )
        .expect("select");
        let rows = out.select.expect("select page").rows;
        assert_eq!(rows, vec![vec!["1".to_string(), "新行".to_string()]], "{rows:?}");
        let _ = std::fs::remove_file(&path);
    }

    /// 无头 UI：填文件 → 预览 → 下一步到映射页 → 再到执行页。
    #[gpui_kit::test]
    fn import_wizard_previews_csv_and_maps_columns(cx: &mut TestAppContext) {
        isolate_store("impui");
        let (_app_handle, parent) = open_app(cx);
        let csv_path = std::env::temp_dir().join(format!("daro_imp_ui_{}.csv", std::process::id()));
        std::fs::write(&csv_path, "id,name\n1,张三\n2,李四\n").unwrap();

        // 演示连接：列清单直接给 mock 的 id / name，映射才有内容可验
        let conn = Connection::new(
            "演示库",
            crate::mock::DbKind::MySql,
            "127.0.0.1",
            3306,
            "demo",
            "root",
            "",
            "",
            false,
        )
        .as_demo();
        let target = ImportTarget {
            conn,
            db: "demo".into(),
            schema: String::new(),
            table: "users".into(),
        };

        let (handle, entity) = cx
            .update(|cx| {
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds {
                            origin: Default::default(),
                            size: size(px(1000.), px(700.)),
                        })),
                        ..Default::default()
                    },
                    cx,
                    move |window, cx| {
                        let target = target.clone();
                        cx.new(|cx| ImportDialog::new(parent.clone(), false, target, window, cx))
                    },
                )
            })
            .expect("open import window");
        let (handle, entity) = (handle, entity);

        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("imp-preview").is_none(), "还没选文件时没有预览表");
            assert!(window.try_find("imp-maplist").is_none(), "映射页还没上屏");

            // 走模型入口加载源文件（输入框订阅是同一条路径）
            entity.update(cx, |d, cx| {
                d.load_source(csv_path.to_string_lossy().to_string(), cx)
            });
            window.render_frame(cx);
            assert!(window.try_find("imp-preview").is_some(), "预览表已上屏");
            assert_eq!(
                entity.read(cx).source.as_ref().unwrap().headers,
                vec!["id", "name"]
            );

            // 下一步 → 映射页
            window.click("imp-next", cx);
            window.render_frame(cx);
            assert!(window.try_find("imp-maplist").is_some(), "映射页已上屏");
            assert!(
                window.try_find(("imp-map-btn", 0usize)).is_some(),
                "第一列的下拉按钮"
            );
            let mapped = entity.read(cx).mapped_count();
            assert!(mapped >= 2, "至少 id / name 两列自动匹配上，实际 {mapped}");

            // 第三步 → 执行页
            window.click(("imp-step", 2usize), cx);
            window.render_frame(cx);
            assert!(window.try_find("imp-run").is_some(), "开始导入按钮");
            assert!(window.try_find("imp-batch").is_some(), "每批行数输入框");
        })
        .expect("update import window");
    }
}

