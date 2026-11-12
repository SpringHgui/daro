//! 「导出向导」对话框（结果集 / 表数据共用，按参考客户端导出向导第一步）。
//!
//! 两步：先是格式选择（照参考截图：单选列表，企业版的 Excel / Access / DBase /
//! Parquet 接通入口、点击弹「敬请期待」但单选位不给选），再是导出路径与格式选项。
//! 结果集没有目标表，SQL 脚本格式
//! 额外要一个目标表名（默认取导出标签，可改）。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::export_format::{encode, DelimitedStyle};
use crate::palette::Palette;
use crate::table_export::{sql_insert, TableFormat};
use crate::{to_hsla, DaroApp};

use crate::ui::{self, TITLE_H};

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
fn default_path(label: &str, format: TableFormat) -> String {
    let name = format!("{}.{}", safe_label(label), format.ext());
    dirs::desktop_dir()
        .map(|p| p.join(&name).display().to_string())
        .unwrap_or(name)
}

/// 格式名按参考客户端向导的口径写（后缀也一并列出）
fn wizard_label(format: TableFormat) -> &'static str {
    match format {
        TableFormat::Txt => "文本文件 (*.txt)",
        TableFormat::Csv => "CSV 文件 (*.csv)",
        TableFormat::Json => "JSON 文件 (*.json)",
        TableFormat::Xml => "XML 文件 (*.xml)",
        TableFormat::Html => "HTML 文件 (*.htm;*.html)",
        TableFormat::Sql => "SQL 脚本文件 (*.sql)",
    }
}

/// 截图里锁定的企业版格式
const ENT_FORMATS: [&str; 5] = [
    "Excel 数据表 (*.xls)",
    "Excel 文件（2007 或更高版本）(*.xlsx)",
    "MS Access 2007 数据库 (*.accdb)",
    "DBase 文件 (*.dbf)",
    "Parquet 文件 (*.parquet)",
];

pub fn open_export_result(
    parent: Entity<DaroApp>,
    dark: bool,
    is_mysql: bool,
    label: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    cx: &mut App,
) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(560.), px(440.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(640.), px(640.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("导出向导".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            ExportResultDialog::new(parent, dark, is_mysql, label, columns, rows, window, cx)
        })
    });
    if opened.is_err() {
        eprintln!("failed to open result export window");
    }
}

struct ExportResultDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    /// 标识符引用风格：MySQL 用反引号，其余用双引号（只影响 SQL 脚本）
    is_mysql: bool,
    label: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    /// 0 = 选格式，1 = 导出路径与选项
    step: usize,
    format: TableFormat,
    style: DelimitedStyle,
    with_header: bool,
    bom: bool,
    quote_all: bool,
    pretty_json: bool,
    /// SQL 脚本的目标表名（结果集本身没有表名，默认取导出标签）
    sql_table: Entity<InputState>,
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
        is_mysql: bool,
        label: String,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let format = TableFormat::Txt;
        let path = cx.new(|cx| {
            InputState::new(window, cx).default_value(default_path(&label, format))
        });
        let sql_table = cx.new(|cx| {
            InputState::new(window, cx).default_value(safe_label(&label))
        });
        cx.subscribe(&path, |this, _, event: &InputEvent, cx| match event {
            InputEvent::PressEnter { .. } => {
                if this.step == 1 {
                    this.export(cx);
                }
            }
            _ => {
                this.auto_path = false;
                if this.error.is_some() {
                    this.error = None;
                }
                cx.notify();
            }
        })
        .detach();
        cx.subscribe(&sql_table, |this, _, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                if this.step == 1 {
                    this.export(cx);
                }
            } else if this.error.is_some() {
                this.error = None;
                cx.notify();
            }
        })
        .detach();
        Self {
            parent,
            dark,
            is_mysql,
            label,
            columns,
            rows,
            step: 0,
            format,
            style: DelimitedStyle::STANDARD,
            with_header: true,
            bom: true,
            quote_all: false,
            pretty_json: false,
            sql_table,
            path,
            auto_path: true,
            error: None,
            done: None,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn set_step(&mut self, step: usize, cx: &mut Context<Self>) {
        self.step = step;
        self.error = None;
        cx.notify();
    }

    fn set_format(&mut self, format: TableFormat, window: &mut Window, cx: &mut Context<Self>) {
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
            .set_title("导出向导")
            .set_file_name(format!("{}.{}", safe_label(&self.label), self.format.ext()))
            .add_filter(wizard_label(self.format), &[self.format.ext()])
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

    /// 生成本次要写出的文本；SQL 脚本走 INSERT，其余交给 export_format。
    fn content(&self, cx: &Context<Self>) -> Result<String, String> {
        if self.format == TableFormat::Sql {
            let table = self.sql_table.read(cx).value().trim().to_string();
            if table.is_empty() {
                return Err("请先填写目标表名".into());
            }
            return sql_insert(&table, &self.columns, &self.rows, self.is_mysql)
                .ok_or_else(|| "结果集为空，没有可导出的行".into());
        }
        Ok(encode(
            self.format.as_export(),
            &self.label,
            &self.columns,
            &self.rows,
            self.style,
            self.with_header,
            self.quote_all,
            self.bom && self.format != TableFormat::Txt,
            self.pretty_json,
        ))
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
        let content = match self.content(cx) {
            Ok(content) => content,
            Err(message) => {
                self.error = Some(message);
                cx.notify();
                return;
            }
        };
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
                        .child("导出向导"),
                ),
        )
    }

    /// 第一步：照参考截图的格式单选列表（企业版格式接通入口，点击弹「敬请期待」）
    fn step_format(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let dark = self.dark;
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let fg_heading = to_hsla(p.foreground);

        let mut list = v_flex().gap(px(2.));
        for (i, format) in TableFormat::ALL.into_iter().enumerate() {
            let me = cx.entity().clone();
            let me_row = me.clone();
            list = list.child(
                div()
                    .id(("expfmt-row", i))
                    .test_support()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .rounded(px(ui::RADIUS))
                    .hover(move |this| this.bg(to_hsla(p.hover)))
                    .on_click(move |_, window, cx| {
                        me_row.update(cx, |this, cx| this.set_format(format, window, cx));
                    })
                    .child(ui::radio(
                        ("expfmt", i),
                        wizard_label(format),
                        self.format == format,
                        true,
                        dark,
                        move |_, window, cx| {
                            me.update(cx, |this, cx| this.set_format(format, window, cx));
                        },
                    )),
            );
        }
        for (i, label) in ENT_FORMATS.into_iter().enumerate() {
            list = list.child(
                h_flex()
                    .id(("expfmt-ent-row", i))
                    .test_support()
                    .h(px(24.))
                    .gap(px(6.))
                    .items_center()
                    .cursor_pointer()
                    .child(ui::radio(
                        ("expfmt-ent", i),
                        label,
                        false,
                        false,
                        dark,
                        |_, _, _| {},
                    ))
                    .on_click(move |_, _, cx| {
                        // 企业版格式：入口接通，选中位留给真导出落地后
                        crate::ent_coming_soon(dark, cx);
                    }),
            );
        }

        v_flex()
            .id("export-wiz-format")
            .test_support()
            .flex_1()
            .min_h_0()
            .px(px(20.))
            .pt(px(22.))
            .gap(px(20.))
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(fg_heading)
                    .child("向导可以让你指定导出数据的细节。你要使用哪一种导出格式？"),
            )
            .child(
                v_flex()
                    .gap(px(10.))
                    .child(div().text_size(px(13.)).text_color(fg).child("导出格式:"))
                    .child(list),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(muted)
                    .child("开发中，敬请期待。"),
            )
    }

    /// 第二步：导出路径与所选格式的选项
    fn step_options(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let muted = to_hsla(p.muted_foreground);
        let me = cx.entity().clone();
        let mut block = v_flex()
            .id("export-wiz-options")
            .test_support()
            .flex_1()
            .min_h_0()
            .px(px(20.))
            .pt(px(18.))
            .gap(px(12.))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(to_hsla(p.foreground))
                    .child(format!("导出格式：{}", wizard_label(self.format))),
            )
            .child(div().text_xs().text_color(muted).child(format!(
                "{} 列 × {} 行 · {}",
                self.columns.len(),
                self.rows.len(),
                self.format_hint()
            )));
        if matches!(self.format, TableFormat::Txt | TableFormat::Csv) {
            let fixed = self.format == TableFormat::Txt;
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
                    delim_row = delim_row.child(ui::segment(
                        ("delim", style.delimiter as usize),
                        style.delimiter_label(),
                        selected,
                        self.dark,
                        move |_, _, cx| {
                            me.update(cx, |this, cx| {
                                this.style = style;
                                this.done = None;
                                cx.notify();
                            });
                        },
                    ));
                }
            }
            block = block.child(delim_row).child(
                h_flex()
                    .gap(px(14.))
                    .items_center()
                    .child(
                        ui::checkbox(
                            "exp-header",
                            "含列名",
                            self.with_header,
                            true,
                            self.dark,
                            {
                                let me = me.clone();
                                move |_, _, cx| {
                                    me.update(cx, |this, cx| {
                                        this.with_header = !this.with_header;
                                        cx.notify();
                                    });
                                }
                            },
                        ),
                    )
                    .child(
                        ui::checkbox(
                            "exp-bom",
                            "UTF-8 BOM",
                            self.bom,
                            true,
                            self.dark,
                            {
                                let me = me.clone();
                                move |_, _, cx| {
                                    me.update(cx, |this, cx| {
                                        this.bom = !this.bom;
                                        cx.notify();
                                    });
                                }
                            },
                        ),
                    )
                    .child(
                        ui::checkbox(
                            "exp-quote-all",
                            "全部加引号",
                            self.quote_all,
                            true,
                            self.dark,
                            {
                                let me = me.clone();
                                move |_, _, cx| {
                                    me.update(cx, |this, cx| {
                                        this.quote_all = !this.quote_all;
                                        cx.notify();
                                    });
                                }
                            },
                        ),
                    ),
            );
        } else if self.format == TableFormat::Json {
            block = block.child(
                ui::checkbox(
                    "exp-pretty",
                    "格式化输出",
                    self.pretty_json,
                    true,
                    self.dark,
                    {
                        let me = me.clone();
                        move |_, _, cx| {
                            me.update(cx, |this, cx| {
                                this.pretty_json = !this.pretty_json;
                                cx.notify();
                            });
                        }
                    },
                ),
            );
        }
        if self.format == TableFormat::Sql {
            block = block.child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(ui::form_label("目标表名", muted))
                    .child(div()
                        .w(px(220.))
                        .child(ui::field_input(&self.sql_table, self.dark, window, cx))),
            );
        }
        block.child(
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
                    ui::secondary_button("exp-result-browse", "…")
                        .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
                ),
        )
    }

    fn format_hint(&self) -> &'static str {
        match self.format {
            TableFormat::Sql => "逐行生成 INSERT 语句；标识符按连接方言引用",
            other => other.as_export().hint(),
        }
    }
}

impl Render for ExportResultDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, muted) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
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
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    window.remove_window();
                    cx.stop_propagation();
                } else if ev.keystroke.key == "enter" {
                    if this.step == 0 {
                        this.set_step(1, cx);
                    } else {
                        this.export(cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .child(self.title_bar())
            .child(if self.step == 0 {
                self.step_format(cx).into_any_element()
            } else {
                self.step_options(window, cx).into_any_element()
            })
            .child(
                v_flex()
                    .px(px(20.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(if is_error { to_hsla(0xc0392b) } else { muted })
                            .child(status),
                    ),
            )
            .child(
                ui::footer_bar(self.dark)
                    .child(div().flex_1())
                    .when(self.step == 0, |footer| {
                        footer
                            .child(
                                div()
                                    .id("exp-result-next-box")
                                    .test_support()
                                    .child(
                                        ui::primary_button(
                                            "exp-result-next",
                                            "下一步 >",
                                            self.dark,
                                            cx,
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.set_step(1, cx)
                                        })),
                                    ),
                            )
                            .child(
                                div()
                                    .id("exp-result-last-box")
                                    .test_support()
                                    .child(
                                        ui::secondary_button("exp-result-last", ">>").on_click(
                                            cx.listener(|this, _, _, cx| this.set_step(1, cx)),
                                        ),
                                    ),
                            )
                    })
                    .when(self.step == 1, |footer| {
                        footer
                            .child(
                                div()
                                    .id("exp-result-back-box")
                                    .test_support()
                                    .child(
                                        ui::secondary_button("exp-result-back", "< 上一步")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_step(0, cx)
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .id("exp-result-ok-box")
                                    .test_support()
                                    .child(
                                        ui::primary_button(
                                            "exp-result-ok",
                                            if finished { "关闭" } else { "开始" },
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
                                    ),
                            )
                    })
                    .child(
                        ui::secondary_button("exp-result-cancel", "取消")
                            .on_click(|_, window, _| window.remove_window()),
                    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_test_support::{isolate_store, open_app};
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt;

    // 显式引回标准 test 宏：`use super::*` 会把 gpui 的同名 test 宏带进作用域
    use core::prelude::v1::test;

    #[gpui_kit::test]
    fn export_wizard_selects_sql_and_writes_inserts(cx: &mut TestAppContext) {
        isolate_store("exportwizard");
        let (_main, app) = open_app(cx);
        let out = std::env::temp_dir().join(format!(
            "daro_rs_export_wizard_{}.sql",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&out);
        let out_text = out.display().to_string();
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![vec!["1".to_string(), "阿离".to_string()]];

        let (handle, dialog) = cx.update(|cx| {
            let mut options = TitleBar::window_options();
            options.window_bounds = Some(WindowBounds::centered(size(px(640.), px(640.)), cx));
            let cols = columns.clone();
            let data = rows.clone();
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| {
                    ExportResultDialog::new(
                        app.clone(),
                        false,
                        true,
                        "查询结果 1".into(),
                        cols,
                        data,
                        window,
                        cx,
                    )
                })
            })
            .expect("打开导出向导")
        });

        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.try_find("export-wiz-format").is_some(),
                "默认停在格式选择页"
            );
            assert!(
                window.try_find("export-wiz-options").is_none(),
                "格式选择页不渲染路径与选项"
            );

            // 第 6 项是 SQL 脚本（前 5 项：文本 / CSV / JSON / XML / HTML）
            window.click(("expfmt-row", 5usize), cx);
            window.render_frame(cx);
            dialog.update(cx, |this, _| {
                assert_eq!(this.format, TableFormat::Sql, "点第 6 项选中 SQL");
            });

            // 选格式会按自动路径换扩展名，测试里把落点改回临时目录
            dialog.update(cx, |this, cx| {
                this.path
                    .update(cx, |state, cx| state.set_value(out_text.clone(), window, cx));
            });
            window.click("exp-result-next-box", cx);
            window.render_frame(cx);
            assert!(
                window.try_find("export-wiz-options").is_some(),
                "下一步进入导出选项页"
            );
            assert!(
                window.try_find("exp-result-back-box").is_some(),
                "选项页提供上一步"
            );

            window.click("exp-result-ok-box", cx);
            window.render_frame(cx);
        })
        .unwrap();

        let written = std::fs::read_to_string(&out).expect("SQL 脚本已写出");
        let expected = sql_insert("查询结果 1", &columns, &rows, true).unwrap();
        assert_eq!(written, expected, "SQL 脚本按 MySQL 反引号引用");
        let done = cx.update(|cx| dialog.read(cx).done.clone());
        assert!(done.is_some(), "写出成功后给出完成状态");
    }
}
