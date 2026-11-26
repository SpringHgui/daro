//! Redis Hash 单字段编辑弹窗。表格里点某行的「编辑」按钮打开，
//! Field 单行输入 + Value 代码编辑器（JSON 高亮 / 格式下拉 / Size / Copy），
//! 点 OK 把 field 与 value 的字节交回主应用写回 Redis。
//!
//! 与确认框一样是独立顶层窗口（避免在主窗口叠遮罩）。写回逻辑不在这里：
//! 弹窗只收集「用户输入 → 字节」，真正 HSET 由 DaroApp 处理并刷新值面板。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState, InputHighlighter, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::json_highlight;
use crate::menu::{DropdownMenu as _, PopupMenuItem};
use crate::palette::Palette;
use crate::value_format;
use crate::{to_hsla, DaroApp, ui};

/// OK 回调：旧字段名字节（新增时为空）、新字段名字节、新值字节。
/// 字段名允许改（Redis 无 rename，改名 = HDEL 旧 field + HSET 新 field），
/// 是否改名由主应用比对决定。
type OnApply = Box<dyn Fn(&mut DaroApp, &mut Context<DaroApp>, Vec<u8>, Vec<u8>, Vec<u8>)>;

/// 打开字段编辑弹窗。
///
/// - `old_field`：被编辑字段的原始字节；新增字段时传 `None`（此时弹窗里 field 初始为空）。
/// - `field_text` / `value_bytes`：编辑框初值，展示口径与主面板一致。
pub fn open_hash_value_dialog(
    parent: Entity<DaroApp>,
    dark: bool,
    old_field: Option<Vec<u8>>,
    field_text: String,
    value_bytes: Vec<u8>,
    on_apply: impl Fn(&mut DaroApp, &mut Context<DaroApp>, Vec<u8>, Vec<u8>, Vec<u8>) + 'static,
    cx: &mut App,
) {
    let title: SharedString = if old_field.is_some() {
        "编辑字段".into()
    } else {
        "新增字段".into()
    };
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(520.), px(360.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(680.), px(460.)), cx));
    if let Some(tb) = options.titlebar.as_mut() {
        tb.title = Some(title);
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        let on_apply: OnApply = Box::new(on_apply);
        cx.new(|cx| {
            HashValueDialog::new(
                parent,
                dark,
                old_field,
                field_text,
                value_bytes,
                on_apply,
                window,
                cx,
            )
        })
    });
    if opened.is_err() {
        eprintln!("[hash_value_dialog] 打开字段编辑窗失败");
    }
}

struct HashValueDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    /// 被编辑字段的原始字节；None = 新增
    old_field: Option<Vec<u8>>,
    field_input: Entity<InputState>,
    value_editor: Entity<EditorState>,
    format: value_format::Format,
    on_apply: OnApply,
    note: Option<(String, bool)>,
    /// Enter / 按钮可能同时到达，提交只认第一次
    applied: bool,
    _esc: Subscription,
}

impl HashValueDialog {
    #[allow(clippy::too_many_arguments)]
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        old_field: Option<Vec<u8>>,
        field_text: String,
        value_bytes: Vec<u8>,
        on_apply: OnApply,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field_input =
            cx.new(|cx| InputState::new(window, cx).default_value(field_text));
        let format = value_format::detect(&value_bytes);
        let initial = value_format::render(&value_bytes, format);
        let value_editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .placeholder("（空值）")
                .line_number(true)
                .searchable(true);
            state.set_value(initial, window, cx);
            // Json 上色；Text / Hex 走工厂返回 None 保持正文色
            state.set_highlighter_factory(
                std::rc::Rc::new(|lang: &str| {
                    if lang == "json" {
                        Some(Box::new(json_highlight::JsonHighlighter::new())
                            as Box<dyn InputHighlighter>)
                    } else {
                        None
                    }
                }),
                cx,
            );
            state.set_highlighter(format.highlight_lang(), cx);
            state
        });
        let esc = ui::close_on_escape(window, cx);
        Self {
            parent,
            dark,
            old_field,
            field_input,
            value_editor,
            format,
            on_apply,
            note: None,
            applied: false,
            _esc: esc,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    /// 手选格式：内容不变，只换渲染与高亮（与主面板 kv_set_format 同口径）。
    fn set_format(
        &mut self,
        fmt: value_format::Format,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.format == fmt {
            return;
        }
        let text = self.value_editor.read(cx).value().to_string();
        // 用当前格式把文本还原成字节，再按新格式渲染：Text/Hex 无损，
        // Json 切换时按当前文本重排，用户正在输入的内容不会因为切格式丢掉。
        let bytes = value_format::encode(&text, self.format).unwrap_or_else(|_| text.into_bytes());
        self.format = fmt;
        let rendered = value_format::render(&bytes, fmt);
        self.value_editor.update(cx, |s, cx| {
            s.set_value(rendered, window, cx);
            s.set_highlighter(fmt.highlight_lang(), cx);
        });
        cx.notify();
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.applied {
            return;
        }
        let field_text = self.field_input.read(cx).value().to_string();
        if field_text.trim().is_empty() {
            self.note = Some(("字段名不能为空".to_string(), true));
            cx.notify();
            return;
        }
        let Some(field) = crate::redis_driver::bytes_from_input(&field_text) else {
            self.note = Some(("字段名含非法的 HEX: 转义".to_string(), true));
            cx.notify();
            return;
        };
        let text = self.value_editor.read(cx).value().to_string();
        let value = match value_format::encode(&text, self.format) {
            Ok(v) => v,
            Err(e) => {
                self.note = Some((e, true));
                cx.notify();
                return;
            }
        };
        self.applied = true;
        let parent = self.parent.clone();
        let old = self.old_field.clone().unwrap_or_default();
        parent.update(cx, |app, cx| (self.on_apply)(app, cx, old, field, value));
        window.remove_window();
    }

    fn title_bar(&self) -> impl IntoElement {
        let p = self.pal();
        TitleBar::new().h(px(ui::TITLE_H)).border_b_0().child(
            h_flex()
                .h_full()
                .items_center()
                .gap(px(6.))
                .pl(px(4.))
                .child(
                    div()
                        .text_xs()
                        .text_color(to_hsla(p.foreground))
                        .child(if self.old_field.is_some() {
                            "编辑字段"
                        } else {
                            "新增字段"
                        }),
                ),
        )
    }
}

impl Render for HashValueDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let soft = to_hsla(p.border_soft);
        let chrome = to_hsla(p.chrome);
        let bg = to_hsla(p.background);
        let me = cx.entity().downgrade();
        let fmt = self.format;
        let text = self.value_editor.read(cx).value().to_string();
        let size = value_format::human_size(
            value_format::encode(&text, fmt)
                .map(|v| v.len())
                .unwrap_or_else(|_| text.len()),
        );

        let fmt_btn = Button::new("hash-dlg/fmt")
            .ghost()
            .with_size(gpui_kit::component::Size::XSmall)
            .dropdown_caret(true)
            .label(fmt.label())
            .dropdown_menu(move |menu, _window, _cx| {
                let mut m = menu;
                for f in value_format::Format::ALL {
                    let ent = me.clone();
                    m = m.item(
                        PopupMenuItem::new(f.label())
                            .checked(f == fmt)
                            .on_click(move |_, window, app| {
                                let _ = ent.update(app, |this, cx| {
                                    this.set_format(f, window, cx)
                                });
                            }),
                    );
                }
                m
            });

        let copy_btn = h_flex()
            .id("hash-dlg/copy")
            .h(px(20.))
            .px(px(6.))
            .items_center()
            .rounded(px(2.))
            .cursor_pointer()
            .hover(move |s| s.bg(to_hsla(p.hover)))
            .on_click(cx.listener(|this, _, _, cx| {
                let text = this.value_editor.read(cx).value().to_string();
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }))
            .child(div().text_size(px(11.5)).text_color(fg).child("Copy"));

        let note = self.note.as_ref().map(|(n, bad)| {
            div()
                .truncate()
                .min_w_0()
                .text_size(px(11.5))
                .text_color(if *bad { to_hsla(0xDC2626) } else { muted })
                .child(n.clone())
        });

        v_flex()
            .id("hash-dlg/root")
            .size_full()
            .bg(chrome)
            .text_color(fg)
            .child(self.title_bar())
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(12.))
                    .gap(px(8.))
                    .child(
                        h_flex()
                            .h(px(24.))
                            .flex_none()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                div()
                                    .w(px(48.))
                                    .flex_none()
                                    .text_size(px(12.))
                                    .text_color(fg)
                                    .child("Field"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(ui::field_input(
                                        &self.field_input,
                                        self.dark,
                                        window,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        h_flex()
                            .h(px(24.))
                            .flex_none()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                div()
                                    .w(px(48.))
                                    .flex_none()
                                    .text_size(px(12.))
                                    .text_color(fg)
                                    .child("Value"),
                            )
                            .child(fmt_btn)
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(muted)
                                    .child(format!("Size: {size}")),
                            )
                            .child(copy_btn),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .border_1()
                            .border_color(soft)
                            .rounded(px(2.))
                            .bg(bg)
                            .child(
                                Editor::new(&self.value_editor)
                                    .appearance(false)
                                    .bordered(false)
                                    .h_full()
                                    .text_size(px(12.5)),
                            ),
                    )
                    .when_some(note, |s, n| s.child(n)),
            )
            .child(
                ui::footer_bar(self.dark)
                    .child(div().flex_1())
                    .child(
                        ui::secondary_button("hash-dlg/cancel", "取消")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        ui::primary_button("hash-dlg/ok", "确定", self.dark, cx)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply(window, cx)
                            })),
                    ),
            )
    }
}
