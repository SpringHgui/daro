//! 「运行 SQL 文件」对话框（迁移自 Flutter 版 `lib/widgets/sql_file_run_dialog.dart`
//! + `lib/data/sql_file_run.dart`）。
//!
//! 选 `.sql` 文件 → 选项（遇错即停）→ 逐条执行到目标连接 / 库 / 模式，
//! 进度与错误明细实时回显，可中途取消。口径照 Flutter 版：默认继续执行
//! （转储里「表已存在」一类可容忍报错很多），失败批记进日志不静默吞。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::*;

use crate::backend;
use crate::palette::Palette;
use crate::{split_sql_statements, to_hsla, DaroApp};

use crate::ui::{self, TITLE_H};

/// 运行目标：连接名 / 库 / 模式（无模式层引擎模式为空串）。
pub struct RunSqlTarget {
    pub conn: String,
    pub db: String,
    pub schema: String,
}

pub fn open_run_sql_file(
    parent: Entity<DaroApp>,
    dark: bool,
    target: RunSqlTarget,
    cx: &mut App,
) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(560.), px(360.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(660.), px(470.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("运行 SQL 文件".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| RunSqlDialog::new(parent, dark, target, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open run sql file window");
    }
}

/// 一次执行的结果汇总（回显）。
struct RunResult {
    succeeded: usize,
    failed: usize,
    errors: Vec<String>,
    cancelled: bool,
}

struct RunSqlDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    target: RunSqlTarget,
    path: Entity<InputState>,
    stop_on_error: bool,
    running: bool,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<String>>,
    result: Option<RunResult>,
    error: Option<String>,
}

impl RunSqlDialog {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        target: RunSqlTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = cx.new(|cx| InputState::new(window, cx).placeholder("选择或直接粘贴 .sql 转储文件路径"));
        cx.subscribe(&path, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.error = None;
                cx.notify();
            }
        })
        .detach();
        Self {
            parent,
            dark,
            target,
            path,
            stop_on_error: false,
            running: false,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(String::new())),
            result: None,
            error: None,
        }
    }

    fn path_text(&self, cx: &App) -> String {
        self.path.read(cx).value().to_string().trim().to_string()
    }

    fn has_path(&self, cx: &App) -> bool {
        let p = self.path_text(cx);
        !p.is_empty() && std::path::Path::new(&p).exists()
    }

    /// 后台执行：读文件 → 拆语句 → 逐条交给目标连接执行。
    fn run(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let path = self.path_text(cx);
        if path.is_empty() {
            self.error = Some("请先选择 SQL 文件".into());
            cx.notify();
            return;
        }
        self.running = true;
        self.cancel.store(false, Ordering::SeqCst);
        self.result = None;
        self.error = None;
        self.progress.lock().unwrap().clear();

        let stop_on_error = self.stop_on_error;
        let conn = self.target.conn.clone();
        let db = self.target.db.clone();
        let schema = self.target.schema.clone();
        let parent = self.parent.clone();
        let cancel = self.cancel.clone();
        let progress = self.progress.clone();
        let me = cx.entity().clone();

        cx.spawn(async move |_, cx| {
            let mut result = RunResult {
                succeeded: 0,
                failed: 0,
                errors: Vec::new(),
                cancelled: false,
            };
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    me.update(cx, |this, cx| {
                        this.running = false;
                        this.error = Some(format!("无法读取文件：{e}"));
                        cx.notify();
                    });
                    return;
                }
            };
            // 去掉 BOM
            let script = content.strip_prefix('\u{feff}').unwrap_or(&content);
            let stmts = split_sql_statements(script);
            let tg = parent.read_with(cx, |app, _| {
                let c = app.conns.iter().find(|c| c.name == conn).cloned();
                c.and_then(|c| backend::Target::of(&c, &db, &schema))
            });
            let target = match tg {
                Some(t) => t,
                None => {
                    me.update(cx, |this, cx| {
                        this.running = false;
                        this.error = Some("目标连接不可用（演示连接不落库，语句不会真正执行）".into());
                        cx.notify();
                    });
                    return;
                }
            };

            for (i, stmt) in stmts.iter().enumerate() {
                if cancel.load(Ordering::SeqCst) {
                    result.cancelled = true;
                    break;
                }
                let sql = stmt.trim().to_string();
                if sql.is_empty() {
                    continue;
                }
                *progress.lock().unwrap() =
                    format!("正在执行第 {} 条 / 共 {} 条…", i + 1, stmts.len());
                match target.execute(&sql, 0) {
                    Ok(_) => result.succeeded += 1,
                    Err(e) => {
                        result.failed += 1;
                        if result.errors.len() < 50 {
                            result.errors.push(format!("第 {} 条失败：{}\n{}", i + 1, e, sql));
                        }
                        if stop_on_error {
                            break;
                        }
                    }
                }
                // 让出一点时间，避免极端情况下界面饿死
                cx.background_executor().timer(Duration::from_millis(1)).await;
            }

            me.update(cx, |this, cx| {
                this.running = false;
                this.result = Some(result);
                cx.notify();
            });
        })
        .detach();
    }

    fn cancel_run(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 用 rfd 选文件（原生对话框，阻塞当前点击；与导出连接对话框同款）
        let picked = rfd::FileDialog::new()
            .add_filter("SQL 文件", &["sql", "txt"])
            .add_filter("所有文件", &["*"])
            .set_title("选择 SQL 文件")
            .pick_file();
        if let Some(p) = picked {
            let p = p.display().to_string();
            self.path.update(cx, |s, cx| s.set_value(p, window, cx));
            self.error = None;
            cx.notify();
        }
    }
}

impl Render for RunSqlDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (bg, fg, muted, border, _chrome) = (
            to_hsla(p.chrome),
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.border),
            to_hsla(p.chrome),
        );
        let dark = self.dark;
        let has_path = self.has_path(cx);
        let progress = self.progress.lock().unwrap().clone();
        let me = cx.entity().clone();

        let target_label = if self.target.schema.is_empty() {
            format!("{} / {}", self.target.conn, self.target.db)
        } else {
            format!(
                "{} / {}.{}",
                self.target.conn, self.target.db, self.target.schema
            )
        };

        let status = if let Some(r) = &self.result {
            if r.cancelled {
                format!("已取消：成功 {} 条 / 失败 {} 条", r.succeeded, r.failed)
            } else if r.failed == 0 {
                format!("执行完成：成功 {} 条", r.succeeded)
            } else {
                format!("执行完成：成功 {} 条 / 失败 {} 条", r.succeeded, r.failed)
            }
        } else if let Some(e) = &self.error {
            e.clone()
        } else if self.running {
            progress
        } else {
            "选择 .sql 文件后开始执行".to_string()
        };

        v_flex()
            .id("runsql-root")
            .size_full()
            .bg(bg)
            .text_color(fg)
            .on_key_down(cx.listener(|_, ev: &KeyDownEvent, window, _cx| {
                if ev.keystroke.key == "escape" && !window.is_fullscreen() {
                    window.remove_window();
                }
            }))
            .child(self.title_bar())
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(14.))
                    .gap(px(10.))
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .items_center()
                            .child(
                                div()
                                    .w(px(64.))
                                    .text_xs()
                                    .text_color(muted)
                                    .child("SQL 文件："),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .child(ui::field_input(&self.path, self.dark, window, cx)),
                            )
                            .child(
                                ui::secondary_button("runsql-browse", "浏览…").on_click(
                                    cx.listener(|this, _, window, cx| this.browse(window, cx)),
                                ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .items_center()
                            .child(
                                div()
                                    .w(px(64.))
                                    .text_xs()
                                    .text_color(muted)
                                    .child("目标："),
                            )
                            .child(div().flex_1().text_xs().text_color(muted).child(target_label)),
                    )
                    .child(ui::checkbox(
                        "runsql-stop",
                        "遇错即停（默认继续，转储里常见可容忍报错）",
                        self.stop_on_error,
                        !self.running,
                        self.dark,
                        {
                            let me = me.clone();
                            move |_, _, cx| {
                                me.update(cx, |this, cx| {
                                    this.stop_on_error = !this.stop_on_error;
                                    cx.notify();
                                });
                            }
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .id("runsql-log")
                            .border_1()
                            .border_color(border)
                            .p(px(6.))
                            .text_xs()
                            .text_color(if self.error.is_some() {
                                to_hsla(0xc0392b)
                            } else {
                                muted
                            })
                            .child(
                                if let Some(r) = &self.result {
                                    if r.errors.is_empty() {
                                        status.clone()
                                    } else {
                                        r.errors.join("\n\n")
                                    }
                                } else {
                                    status.clone()
                                },
                            ),
                    ),
            )
            .child(
                ui::footer_bar(dark)
                    .child(
                        if self.running {
                            ui::secondary_button("runsql-cancel", "停止").on_click(cx.listener(
                                |this, _, _, _cx| this.cancel_run(),
                            ))
                        } else {
                            ui::primary_button("runsql-start", "开始执行", dark, cx)
                                .disabled(!has_path)
                                .on_click(cx.listener(|this, _, _, cx| this.run(cx)))
                        },
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(status),
                    ),
            )
    }
}

impl RunSqlDialog {
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
                        .child("运行 SQL 文件"),
                ),
        )
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }
}
