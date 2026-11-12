//! 「扩展」窗：照 Zed 的 Extensions 页，把 daro 的扩展单元摆成卡片清单。
//!
//! daro 的扩展就是数据库驱动：六个已接入的驱动随应用内置，另外三个在「新建
//! 连接」里能看到的引擎还没接驱动。这里不摆安装 / 下载数字这类假入口，卡片
//! 只给真实状态（内置 / 未接入），内置驱动一键开新建连接。
//! 搜索、状态筛选、分类筛选都是纯前端过滤。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::db_icons;
use crate::mock::DbKind;
use crate::palette::Palette;
use crate::zed_icons::{self, ZedIcon};
use crate::{DaroApp, to_hsla, ui};

/// 设置窗内容区设计尺寸（另加 32 的自绘标题栏）。
const CONTENT_SIZE: (f32, f32) = (820., 620.);

#[derive(Clone, Copy, PartialEq)]
enum Cat {
    Relational,
    Document,
    KeyValue,
}

impl Cat {
    fn label(self) -> &'static str {
        match self {
            Cat::Relational => "关系型",
            Cat::Document => "文档型",
            Cat::KeyValue => "键值型",
        }
    }
}

struct Extension {
    name: &'static str,
    /// 元素 id 后缀与引擎瓦片资源名
    slug: &'static str,
    /// 已接入驱动才有类型；None 表示引擎列在「新建连接」里但驱动未接入
    kind: Option<DbKind>,
    cat: Cat,
    desc: &'static str,
}

const EXTENSIONS: [Extension; 9] = [
    Extension {
        name: "MySQL",
        slug: "mysql",
        kind: Some(DbKind::MySql),
        cat: Cat::Relational,
        desc: "连接 MySQL 服务器：SQL 查询、表格编辑与导入导出。",
    },
    Extension {
        name: "PostgreSQL",
        slug: "postgresql",
        kind: Some(DbKind::PostgreSql),
        cat: Cat::Relational,
        desc: "连接 PostgreSQL 服务器：SQL 查询、主键写回与导出。",
    },
    Extension {
        name: "SQL Server",
        slug: "sqlserver",
        kind: Some(DbKind::SqlServer),
        cat: Cat::Relational,
        desc: "连接 SQL Server：SQL 查询与结果网格编辑。",
    },
    Extension {
        name: "SQLite",
        slug: "sqlite",
        kind: Some(DbKind::SQLite),
        cat: Cat::Relational,
        desc: "直接打开本地 SQLite 3 文件，无需服务端。",
    },
    Extension {
        name: "MongoDB",
        slug: "mongodb",
        kind: Some(DbKind::MongoDb),
        cat: Cat::Document,
        desc: "只读浏览数据库、集合与文档，不支持 SQL 与写回。",
    },
    Extension {
        name: "Redis",
        slug: "redis",
        kind: Some(DbKind::Redis),
        cat: Cat::KeyValue,
        desc: "浏览键空间并读写字符串、哈希、列表、集合与有序集合。",
    },
    Extension {
        name: "Oracle",
        slug: "oracle",
        kind: None,
        cat: Cat::Relational,
        desc: "驱动尚未接入，暂不可用。",
    },
    Extension {
        name: "MariaDB",
        slug: "mariadb",
        kind: None,
        cat: Cat::Relational,
        desc: "驱动尚未接入，暂不可用。",
    },
    Extension {
        name: "Snowflake",
        slug: "snowflake",
        kind: None,
        cat: Cat::Relational,
        desc: "驱动尚未接入，暂不可用。",
    },
];

const STATE_TABS: [&str; 3] = ["全部", "已内置", "未接入"];
const CAT_TABS: [&str; 4] = ["全部", "关系型", "文档型", "键值型"];

pub fn open_extensions(parent: Entity<DaroApp>, dark: bool, cx: &mut App) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(640.), px(480.)));
    options.window_bounds = Some(WindowBounds::centered(
        size(px(CONTENT_SIZE.0), px(CONTENT_SIZE.1 + ui::TITLE_H)),
        cx,
    ));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("扩展".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| ExtensionsDialog::new(parent, dark, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open extensions window");
    }
}

struct ExtensionsDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    search: Entity<InputState>,
    /// 0 全部 / 1 已内置 / 2 未接入
    state: usize,
    /// 0 全部 / 1 关系型 / 2 文档型 / 3 键值型
    cat: usize,
    _esc: Subscription,
}

impl ExtensionsDialog {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索扩展..."));
        let notify = |_: &mut Self, _: Entity<InputState>, _: &InputEvent, cx: &mut Context<Self>| {
            cx.notify();
        };
        cx.subscribe(&search, notify).detach();
        // 开窗直接落在搜索框，键入即筛
        let focus = search.clone();
        cx.defer_in(window, move |_, window, cx| {
            focus.update(cx, |state, cx| state.focus(window, cx));
        });
        // 搜索框聚焦时 InputState 会把 Esc 当动作吃掉，冒泡阶段收不到；
        // 按键拦截器跑在动作派发之前，按窗口 id 限定只关本窗。
        let this_window = window.window_handle();
        let esc = cx.intercept_keystrokes(move |ev, window, _cx| {
            if ev.keystroke.key == "escape" && window.window_handle() == this_window {
                window.remove_window();
            }
        });
        Self {
            parent,
            dark,
            search,
            state: 0,
            cat: 0,
            _esc: esc,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn matches(&self, ext: &Extension, needle: &str) -> bool {
        let state_ok = match self.state {
            1 => ext.kind.is_some(),
            2 => ext.kind.is_none(),
            _ => true,
        };
        let cat_ok = match self.cat {
            1 => ext.cat == Cat::Relational,
            2 => ext.cat == Cat::Document,
            3 => ext.cat == Cat::KeyValue,
            _ => true,
        };
        cat_ok && state_ok && (needle.is_empty() || ext.name.to_lowercase().contains(needle))
    }

    fn title_bar(&self) -> impl IntoElement {
        let p = self.pal();
        TitleBar::new().h(px(ui::TITLE_H)).border_b_0().child(
            h_flex()
                .h_full()
                .items_center()
                .gap(px(6.))
                .pl(px(4.))
                .child(zed_icons::ic(
                    ZedIcon::Blocks,
                    14.,
                    to_hsla(p.muted_foreground),
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(to_hsla(p.foreground))
                        .child("扩展"),
                ),
        )
    }

    fn badge(&self, text: &'static str, muted: bool) -> AnyElement {
        let p = self.pal();
        div()
            .h(px(17.))
            .px(px(5.))
            .flex()
            .items_center()
            .rounded(px(3.))
            .bg(to_hsla(p.hover))
            .text_size(px(10.5))
            .text_color(to_hsla(if muted {
                p.subtle_foreground
            } else {
                p.muted_foreground
            }))
            .child(text)
            .into_any_element()
    }

    fn card(&self, ext: &Extension, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let builtin = ext.kind.is_some();
        let action = match ext.kind {
            Some(kind) => ui::compact_button(format!("ext-connect-{}", ext.slug), "新建连接")
                .on_click(cx.listener(move |this, _, _, cx| {
                    crate::conn_dialog::open_new_connection_of_kind(
                        this.parent.clone(),
                        this.dark,
                        kind,
                        cx,
                    );
                })),
            None => ui::compact_button(format!("ext-missing-{}", ext.slug), "未接入")
                .disabled(true),
        };
        h_flex()
            .id(format!("ext-card-{}", ext.slug))
            .test_support()
            .w_full()
            .items_start()
            .gap(px(14.))
            .p(px(14.))
            .rounded(px(6.))
            .bg(to_hsla(p.dialog_surface))
            .border(px(ui::HAIRLINE))
            .border_color(to_hsla(p.border_soft))
            .child(db_icons::vector_tile(ext.slug, 26.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.))
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(7.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(to_hsla(p.foreground))
                                    .child(ext.name),
                            )
                            .child(self.badge(ext.cat.label(), false))
                            .child(if builtin {
                                self.badge("内置", false)
                            } else {
                                self.badge("未接入", true)
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(to_hsla(p.muted_foreground))
                            .child(ext.desc),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(5.))
                            .child(zed_icons::ic(
                                ZedIcon::Person,
                                12.,
                                to_hsla(p.subtle_foreground),
                            ))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(to_hsla(p.subtle_foreground))
                                    .child("SpringHgui"),
                            ),
                    ),
            )
            .child(v_flex().flex_none().items_end().child(action))
            .into_any_element()
    }

    fn segment_row(
        &self,
        prefix: &'static str,
        tabs: &'static [&'static str],
        pick: fn(&Self) -> usize,
        set: fn(&mut Self, usize),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = pick(self);
        h_flex()
            .flex_none()
            .gap(px(2.))
            .children(tabs.iter().enumerate().map(|(ix, label)| {
                let me = cx.entity().clone();
                ui::segment(
                    format!("{prefix}-{ix}"),
                    *label,
                    ix == active,
                    self.dark,
                    move |_, _, app| {
                        me.update(app, |this: &mut ExtensionsDialog, cx| {
                            if ix != pick(this) {
                                set(this, ix);
                                cx.notify();
                            }
                        });
                    },
                )
                .test_support()
            }))
            .into_any_element()
    }
}

impl Render for ExtensionsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let needle = self.search.read(cx).value().trim().to_lowercase();
        let builtin = EXTENSIONS.iter().filter(|e| e.kind.is_some()).count();
        let missing = EXTENSIONS.len() - builtin;
        let cards: Vec<AnyElement> = EXTENSIONS
            .iter()
            .filter(|ext| self.matches(ext, &needle))
            .map(|ext| self.card(ext, cx))
            .collect();

        v_flex()
            .id("extensions-root")
            .size_full()
            .bg(to_hsla(p.background))
            .text_color(to_hsla(p.foreground))
            .child(self.title_bar())
            .child(
                h_flex()
                    .px(px(24.))
                    .pt(px(18.))
                    .pb(px(12.))
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .text_color(to_hsla(p.foreground))
                            .child("扩展"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(to_hsla(p.muted_foreground))
                            .child(format!("内置 {builtin} · 未接入 {missing}")),
                    ),
            )
            .child(
                h_flex()
                    .px(px(24.))
                    .pb(px(10.))
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(ui::field_input(&self.search, self.dark, window, cx)),
                    )
                    .child(self.segment_row(
                        "ext-state",
                        &STATE_TABS,
                        |this| this.state,
                        |this, ix| this.state = ix,
                        cx,
                    )),
            )
            .child(h_flex().px(px(24.)).pb(px(10.)).child(self.segment_row(
                "ext-cat",
                &CAT_TABS,
                |this| this.cat,
                |this, ix| this.cat = ix,
                cx,
            )))
            .child(
                v_flex()
                    .id("ext-list")
                    .flex_1()
                    .min_h_0()
                    .px(px(24.))
                    .pb(px(16.))
                    .gap(px(10.))
                    .overflow_y_scrollbar()
                    .when(cards.is_empty(), |s| {
                        s.child(
                            div()
                                .id("ext-empty")
                                .test_support()
                                .py(px(24.))
                                .text_size(px(12.))
                                .text_color(to_hsla(p.muted_foreground))
                                .child("没有匹配的扩展。"),
                        )
                    })
                    .children(cards),
            )
    }
}
