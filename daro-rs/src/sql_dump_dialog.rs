//! 「转储 SQL 文件」对话框：选落点、跑后台转储、显示进度与回执。
//!
//! 真正的结构 / 数据生成在 [crate::sql_dump]：这里只负责把取消标志与进度
//! 搬到界面上，一行数据都不在 UI 线程上搬。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;

use crate::palette::Palette;
use crate::sql_dump::{self, DumpError, DumpOutcome, DumpProgress, DumpRequest, DumpScope};
use crate::ui::TITLE_H;
use crate::{to_hsla, ui};

fn safe_file_stem(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
}

fn dump_label(req: &DumpRequest) -> String {
    match req.scope {
        DumpScope::Table => format!("表 {}.{}", req.db, req.table),
        DumpScope::Schema => format!("模式 {}.{}", req.db, req.schema),
        DumpScope::Database => format!("库 {}", req.db),
    }
}

fn default_path(req: &DumpRequest) -> String {
    let stem = match req.scope {
        DumpScope::Table => req.table.clone(),
        DumpScope::Schema => format!("{}-{}", req.db, req.schema),
        DumpScope::Database => req.db.clone(),
    };
    let name = format!("{}.sql", safe_file_stem(&stem));
    dirs::desktop_dir()
        .map(|p| p.join(&name).display().to_string())
        .unwrap_or(name)
}

pub fn open_sql_dump(dark: bool, req: DumpRequest, cx: &mut App) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(560.), px(320.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(640.), px(380.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("转储 SQL 文件".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| SqlDumpDialog::new(dark, req, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open sql dump window");
    }
}

struct SqlDumpDialog {
    dark: bool,
    _esc: Subscription,
    req: DumpRequest,
    path: Entity<InputState>,
    auto_path: bool,
    busy: bool,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<DumpProgress>>,
    error: Option<String>,
    done: Option<String>,
    /// 本窗口句柄：回执落地用它，`active_window()` 失焦时可能为空
    win: AnyWindowHandle,
}

impl SqlDumpDialog {
    fn new(
        dark: bool,
        req: DumpRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path =
            cx.new(|cx| InputState::new(window, cx).default_value(default_path(&req)));
        cx.subscribe(&path, |this, _, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.start(cx);
            } else {
                this.auto_path = false;
                if this.error.is_some() {
                    this.error = None;
                }
                cx.notify();
            }
        })
        .detach();
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
        Self {
            dark,
            _esc: esc,
            req,
            path,
            auto_path: true,
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(DumpProgress::default())),
            error: None,
            done: None,
            win: window.window_handle(),
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = rfd::AsyncFileDialog::new()
            .set_title("转储 SQL 文件")
            .set_file_name(format!("{}.sql", safe_file_stem(&dump_label(&self.req))))
            .add_filter("SQL", &["sql"]);
        ui::save_file(window, dialog, cx, |this, window, cx, path| {
            let text = path.display().to_string();
            this.path
                .update(cx, |state, cx| state.set_value(text, window, cx));
            this.auto_path = false;
            this.error = None;
            cx.notify();
        });
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.done.is_some() {
            return;
        }
        let path = self.path.read(cx).value().trim().to_string();
        if path.is_empty() {
            self.error = Some("请先选择转储路径".into());
            cx.notify();
            return;
        }
        if let Some(dir) = std::path::Path::new(&path).parent()
            && !dir.as_os_str().is_empty()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            self.error = Some(format!("创建目录失败：{e}"));
            cx.notify();
            return;
        }
        let mut req = self.req.clone();
        req.path = path.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(DumpProgress::default()));
        let finished = Arc::new(AtomicBool::new(false));
        self.busy = true;
        self.cancel = cancel.clone();
        self.progress = progress.clone();
        self.error = None;
        self.done = None;
        cx.notify();

        let handle = cx.active_window().unwrap_or(self.win);
        let entity = cx.entity().downgrade();
        {
            let cancel = cancel.clone();
            let progress = progress.clone();
            let finished = finished.clone();
            cx.spawn(async move |_, cx| {
                let res = cx
                    .background_executor()
                    .spawn(async move {
                        let out = sql_dump::run(
                            &req,
                            &mut |p| {
                                if let Ok(mut slot) = progress.lock() {
                                    *slot = p;
                                }
                            },
                            &cancel,
                        );
                        finished.store(true, Ordering::Relaxed);
                        out
                    })
                    .await;
                let _ = handle.update(cx, |_, _, app| {
                    let Some(ent) = entity.upgrade() else { return };
                    ent.update(app, |d, cx| d.finish(res, &path, cx));
                });
            })
            .detach();
        }
        self.track_progress(handle, finished, cx);
    }

    fn finish(
        &mut self,
        res: Result<DumpOutcome, DumpError>,
        path: &str,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        match res {
            Ok(outcome) => {
                let mut text = format!(
                    "已转储 {} 张表、{} 个视图、{} 行（{} 字节）到 {}",
                    outcome.tables, outcome.views, outcome.rows, outcome.bytes, path
                );
                if !outcome.failed.is_empty() {
                    let sample = outcome
                        .failed
                        .iter()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("；");
                    text.push_str(&format!(
                        "；{} 个对象失败：{}",
                        outcome.failed.len(),
                        sample
                    ));
                }
                self.done = Some(text);
                self.error = None;
            }
            Err(DumpError::Cancelled) => self.error = Some("已取消，未保留半成品文件".into()),
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

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
                .child(
                    div()
                        .text_xs()
                        .text_color(to_hsla(p.foreground))
                        .child("转储 SQL 文件"),
                ),
        )
    }

    fn status(&self) -> (String, bool) {
        if self.busy {
            let p = self
                .progress
                .lock()
                .map(|p| p.clone())
                .unwrap_or_default();
            let what = if p.current.is_empty() {
                "准备中".to_string()
            } else {
                format!("正在处理 {}", p.current)
            };
            if p.total == 0 {
                return (format!("{what}…"), false);
            }
            return (
                format!(
                    "{what}… {}/{} · {} 行",
                    p.done, p.total, p.rows
                ),
                false,
            );
        }
        if let Some(done) = &self.done {
            return (done.clone(), false);
        }
        if let Some(err) = &self.error {
            return (err.clone(), true);
        }
        (
            "生成 .sql：先建表，再按 200 行一批写 INSERT，外键统一放到文件末尾。".into(),
            false,
        )
    }
}

impl Render for SqlDumpDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, muted) = (to_hsla(p.foreground), to_hsla(p.muted_foreground));
        let (status, is_error) = self.status();
        let busy = self.busy;
        let finished = self.done.is_some();
        let mode = if self.req.include_data {
            "结构和数据"
        } else {
            "仅结构"
        };
        let label = dump_label(&self.req);

        v_flex()
            .id("sql-dump-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                if ev.keystroke.key == "enter" {
                    this.start(cx);
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
                        "转储 {label}（{mode}）"
                    )))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(ui::form_label("转储到", muted))
                            .child(
                                div()
                                    .flex_1()
                                    .child(ui::field_input(&self.path, self.dark, window, cx)),
                            )
                            .child(
                                ui::secondary_button("sql-dump-browse", "…")
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
                ui::footer_bar(self.dark)
                    .child(
                        ui::primary_button_enabled(
                            "sql-dump-ok",
                            if finished { "关闭" } else { "开始转储" },
                            self.dark,
                            !busy,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.done.is_some() {
                                window.remove_window();
                            } else {
                                this.start(cx);
                            }
                        })),
                    )
                    .child(
                        ui::secondary_button("sql-dump-cancel", "取消").on_click(
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
                    .child(div().text_xs().text_color(muted).child(label)),
            )
    }
}
