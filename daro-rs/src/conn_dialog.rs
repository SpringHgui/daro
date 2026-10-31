//! 新建连接向导。Navicat 里这是独立的顶层窗口，这里同样开新窗口，
//! 而不是在主窗口内叠一层遮罩。

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::DaroApp;
use crate::mock::{Connection, DbKind};
use crate::palette::Palette;
use crate::to_hsla;

const TITLE_H: f32 = 24.;
const LEFT_W: f32 = 250.;
const TILE_W: f32 = 96.;
const TILE_H: f32 = 92.;
const COLS: usize = 5;
const FOOTER_H: f32 = 46.;

struct TypeTile {
    name: &'static str,
    kind: Option<DbKind>,
    color: u32,
    mark: &'static str,
}

/// 只有接入了驱动的类型可选；其余照 Navicat 摆出来但置灰。
const TILES: [TypeTile; 9] = [
    TypeTile { name: "MySQL", kind: Some(DbKind::MySql), color: 0x00758f, mark: "My" },
    TypeTile { name: "PostgreSQL", kind: Some(DbKind::PostgreSql), color: 0x336791, mark: "Pg" },
    TypeTile { name: "SQL Server", kind: Some(DbKind::SqlServer), color: 0xcc2927, mark: "Ss" },
    TypeTile { name: "Oracle", kind: None, color: 0xc74634, mark: "Or" },
    TypeTile { name: "SQLite", kind: None, color: 0x003b57, mark: "Sq" },
    TypeTile { name: "MariaDB", kind: None, color: 0x003545, mark: "Ma" },
    TypeTile { name: "MongoDB", kind: Some(DbKind::MongoDb), color: 0x4faa41, mark: "Mo" },
    TypeTile { name: "Redis", kind: Some(DbKind::Redis), color: 0xd82c20, mark: "Re" },
    TypeTile { name: "Snowflake", kind: None, color: 0x29b5e8, mark: "Sn" },
];

const VENDORS: [&str; 8] = [
    "阿里云",
    "华为",
    "腾讯云",
    "达梦",
    "电科金仓",
    "AWS",
    "Google Cloud",
    "Microsoft",
];

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Choose,
    Form,
}

/// 同一个窗口承担「新建」和「编辑」：Navicat 的连接属性对话框与向导共用一套表单，
/// 编辑态只是跳过类型选择、并在保存时回写原连接。
#[derive(Clone, Copy)]
pub enum ConnDialogMode {
    Create,
    Edit(usize),
}

pub fn open_new_connection(parent: Entity<DaroApp>, dark: bool, cx: &mut App) {
    open_connection_dialog(parent, dark, ConnDialogMode::Create, cx);
}

pub fn open_connection_dialog(
    parent: Entity<DaroApp>,
    dark: bool,
    mode: ConnDialogMode,
    cx: &mut App,
) {
    let editing = matches!(mode, ConnDialogMode::Edit(_));
    let title = if editing { "编辑连接" } else { "新建连接" };
    let mut options = TitleBar::window_options();
    options.window_min_size = Some(size(px(820.), px(560.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(1000.), px(720.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some(title.into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| NewConnectionDialog::new(parent, dark, mode, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open connection window");
    }
}

pub struct NewConnectionDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    mode: ConnDialogMode,
    step: Step,
    selected: Option<usize>,
    list_mode: bool,
    type_filters: [bool; TILES.len()],
    vendor_filters: [bool; VENDORS.len()],
    kind: DbKind,
    uri_mode: bool,
    last_title: Option<String>,
    save_password: bool,
    status: Option<SharedString>,
    search: Entity<InputState>,
    uri: Entity<InputState>,
    name: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    database: Entity<InputState>,
    user: Entity<InputState>,
    password: Entity<InputState>,
}

impl NewConnectionDialog {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        mode: ConnDialogMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editing = match mode {
            ConnDialogMode::Create => None,
            ConnDialogMode::Edit(ix) => parent.read(cx).conns.get(ix).cloned(),
        };
        let initial = |pick: fn(&Connection) -> String, fallback: String| match &editing {
            Some(conn) => pick(conn),
            None => fallback,
        };
        let kind = editing.as_ref().map(|c| c.kind).unwrap_or(DbKind::MySql);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索"));
        let uri = cx.new(|cx| {
            InputState::new(window, cx).placeholder("mysql://user:password@localhost:3306/db")
        });
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial(|c| c.name.clone(), String::new()))
        });
        let host = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial(|c| c.host.clone(), String::new()))
        });
        let port = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial(|c| c.port.to_string(), String::new()))
        });
        let database = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial(|c| c.database.clone(), String::new()))
        });
        let user = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial(|c| c.user.clone(), String::new()))
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial(|c| c.password.clone(), String::new()))
                .masked(true)
        });
        let save_password = editing
            .as_ref()
            .map(|c| !c.password.is_empty())
            .unwrap_or(false);
        let selected = TILES.iter().position(|t| t.kind == Some(kind));

        let notify = |_: &mut Self, _: Entity<InputState>, _: &InputEvent, cx: &mut Context<Self>| {
            cx.notify();
        };
        cx.subscribe(&search, notify).detach();
        cx.subscribe(&uri, notify).detach();
        cx.subscribe(&name, notify).detach();
        cx.subscribe(&host, notify).detach();
        cx.subscribe(&port, notify).detach();
        cx.subscribe(&database, notify).detach();
        cx.subscribe(&user, notify).detach();

        Self {
            parent,
            dark,
            mode,
            step: if editing.is_some() {
                Step::Form
            } else {
                Step::Choose
            },
            selected,
            list_mode: false,
            type_filters: [false; TILES.len()],
            vendor_filters: [false; VENDORS.len()],
            kind,
            uri_mode: false,
            last_title: None,
            save_password,
            status: None,
            search,
            uri,
            name,
            host,
            port,
            database,
            user,
            password,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    /// Navicat 在选定类型后把类型名并进窗口标题（新建连接 (MySQL)）；编辑态不带类型。
    fn title_text(&self) -> String {
        let base = match self.mode {
            ConnDialogMode::Create => "新建连接",
            ConnDialogMode::Edit(_) => "编辑连接",
        };
        match self.step {
            Step::Choose => base.to_string(),
            Step::Form => match TILES.iter().find(|t| t.kind == Some(self.kind)) {
                Some(tile) => format!("{base} ({})", tile.name),
                None => base.to_string(),
            },
        }
    }

    fn sync_title(&mut self, window: &mut Window) {
        let text = self.title_text();
        if self.last_title.as_deref() != Some(text.as_str()) {
            window.set_window_title(&text);
            self.last_title = Some(text);
        }
    }

    fn tile_visible(&self, ix: usize, needle: &str) -> bool {
        // 供应商筛选里的厂商都还没有接入，勾上任何一个都意味着没有结果
        if self.vendor_filters.iter().any(|on| *on) {
            return false;
        }
        if self.type_filters.iter().any(|on| *on) && !self.type_filters[ix] {
            return false;
        }
        let tile = &TILES[ix];
        needle.is_empty() || tile.name.to_lowercase().contains(needle)
    }

    fn choose_tile(&mut self, ix: usize) {
        if TILES[ix].kind.is_some() {
            self.selected = Some(ix);
        }
    }

    fn goto_form(&mut self, cx: &mut Context<Self>) {
        let Some(kind) = self.selected.and_then(|ix| TILES[ix].kind) else {
            return;
        };
        self.kind = kind;
        self.uri_mode = false;
        self.status = None;
        self.step = Step::Form;
        cx.notify();
    }

    fn goto_uri_form(&mut self, cx: &mut Context<Self>) {
        self.uri_mode = true;
        self.status = None;
        self.step = Step::Form;
        cx.notify();
    }

    fn can_save(&self, cx: &Context<Self>) -> bool {
        if self.name.read(cx).value().trim().is_empty() {
            return false;
        }
        if self.uri_mode {
            return !self.uri.read(cx).value().trim().is_empty();
        }
        !self.host.read(cx).value().trim().is_empty() && self.port.read(cx).value().parse::<u16>().is_ok()
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_save(cx) {
            return;
        }
        let name = self.name.read(cx).value().trim().to_string();
        let (kind, host, port, database, user, password) = if self.uri_mode {
            let raw = self.uri.read(cx).value().trim().to_string();
            match parse_uri(&raw) {
                Some(parts) => parts,
                None => {
                    self.status = Some("无法识别的连接字符串".into());
                    cx.notify();
                    return;
                }
            }
        } else {
            (
                self.kind,
                self.host.read(cx).value().trim().to_string(),
                self.port.read(cx).value().parse::<u16>().unwrap_or(self.kind.default_port()),
                self.database.read(cx).value().trim().to_string(),
                self.user.read(cx).value().trim().to_string(),
                self.password.read(cx).value().to_string(),
            )
        };
        match self.mode {
            ConnDialogMode::Create => {
                let conn =
                    Connection::new(name, kind, host, port, database, user, password, "", false);
                self.parent.update(cx, |app, cx| {
                    app.push_connection(conn);
                    cx.notify();
                });
            }
            ConnDialogMode::Edit(ix) => {
                self.parent.update(cx, |app, cx| {
                    app.apply_connection_edit(
                        ix, name, kind, host, port, database, user, password,
                    );
                    cx.notify();
                });
            }
        }
        window.remove_window();
    }

    fn title_bar(&self) -> impl IntoElement {
        let p = self.pal();
        let title = self.title_text();
        TitleBar::new().h(px(TITLE_H)).child(
            h_flex()
                .h_full()
                .items_center()
                .gap(px(6.))
                .pl(px(4.))
                .child(
                    Icon::new(IconName::Database)
                        .with_size(px(14.))
                        .text_color(to_hsla(p.accent)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(to_hsla(p.foreground))
                        .child(title),
                ),
        )
    }

    fn choose(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let needle = self.search.read(cx).value().to_lowercase();
        let visible: Vec<usize> = (0..TILES.len())
            .filter(|ix| self.tile_visible(*ix, &needle))
            .collect();
        let recent: Vec<usize> = visible.iter().copied().filter(|ix| *ix == 0).collect();
        let me = cx.entity().clone();

        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.filters(cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(self.choose_header(&me))
                            .child(
                                div()
                                    .id("type-scroll")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scrollbar()
                                    .child(
                                        v_flex()
                                            .p(px(14.))
                                            .gap(px(16.))
                                            .when(!recent.is_empty(), |s| {
                                                s.child(self.section("最近使用过的", &recent, cx))
                                            })
                                            .when(visible.is_empty(), |s| {
                                                s.child(
                                                    div()
                                                        .py(px(24.))
                                                        .text_xs()
                                                        .text_color(to_hsla(p.muted_foreground))
                                                        .child("没有匹配的连接类型"),
                                                )
                                            })
                                            .when(!visible.is_empty(), |s| {
                                                s.child(self.section("全部", &visible, cx))
                                            }),
                                    ),
                            ),
                    ),
            )
            .child(self.choose_footer(cx))
    }

    fn section(&self, title: &str, indexes: &[usize], cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        v_flex()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(to_hsla(p.foreground))
                    .child(title.to_string()),
            )
            .child(self.grid(indexes, cx))
    }

    fn grid(&self, indexes: &[usize], cx: &mut Context<Self>) -> impl IntoElement {
        if self.list_mode {
            return v_flex()
                .gap(px(1.))
                .children(indexes.iter().map(|ix| self.list_row(*ix, cx)))
                .into_any_element();
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        for chunk in indexes.chunks(COLS) {
            rows.push(
                h_flex()
                    .gap(px(10.))
                    .children(chunk.iter().map(|ix| self.tile(*ix, cx)))
                    .into_any_element(),
            );
        }
        v_flex().gap(px(10.)).children(rows).into_any_element()
    }

    fn tile(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let tile = &TILES[ix];
        let enabled = tile.kind.is_some();
        let selected = self.selected == Some(ix);
        let me = cx.entity().clone();
        let me2 = me.clone();
        v_flex()
            .id(("tile", ix))
            .w(px(TILE_W))
            .h(px(TILE_H))
            .items_center()
            .justify_center()
            .gap(px(6.))
            .rounded(px(3.))
            .border_1()
            .border_color(if selected {
                to_hsla(p.accent)
            } else {
                transparent_black()
            })
            .when(selected, |s| s.bg(to_hsla(p.selection)))
            .when(!selected && enabled, {
                let hover = to_hsla(p.hover);
                move |s| s.hover(move |s| s.bg(hover))
            })
            .when(!enabled, |s| s.opacity(0.38))
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                me.update(cx, |this, cx| {
                    this.choose_tile(ix);
                    cx.notify();
                });
            })
            .on_double_click(move |_, _, cx| {
                me2.update(cx, |this, cx| {
                    this.choose_tile(ix);
                    this.goto_form(cx);
                });
            })
            .child(logo(tile.color, tile.mark, 42.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(to_hsla(p.foreground))
                    .child(tile.name),
            )
            .into_any_element()
    }

    fn list_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let tile = &TILES[ix];
        let enabled = tile.kind.is_some();
        let selected = self.selected == Some(ix);
        let me = cx.entity().clone();
        let me2 = me.clone();
        h_flex()
            .id(("type-row", ix))
            .h(px(30.))
            .px(px(8.))
            .gap(px(10.))
            .items_center()
            .rounded(px(3.))
            .when(selected, |s| s.bg(to_hsla(p.selection)))
            .when(!selected && enabled, {
                let hover = to_hsla(p.hover);
                move |s| s.hover(move |s| s.bg(hover))
            })
            .when(!enabled, |s| s.opacity(0.38))
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                me.update(cx, |this, cx| {
                    this.choose_tile(ix);
                    cx.notify();
                });
            })
            .on_double_click(move |_, _, cx| {
                me2.update(cx, |this, cx| {
                    this.choose_tile(ix);
                    this.goto_form(cx);
                });
            })
            .child(logo(tile.color, tile.mark, 18.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(to_hsla(p.foreground))
                    .child(tile.name),
            )
            .into_any_element()
    }

    fn filters(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let me = cx.entity().clone();
        let me2 = me.clone();
        let type_rows = TILES.iter().enumerate().map({
            let me = me.clone();
            move |(ix, tile)| {
                let me = me.clone();
                Checkbox::new(("type-filter", ix))
                    .checked(self.type_filters[ix])
                    .label(tile.name)
                    .on_click(move |checked, _, cx| {
                        let checked = *checked;
                        me.update(cx, |this, cx| {
                            this.type_filters[ix] = checked;
                            cx.notify();
                        });
                    })
            }
        });
        let vendor_rows = VENDORS.iter().enumerate().map(move |(ix, vendor)| {
            let me = me2.clone();
            Checkbox::new(("vendor-filter", ix))
                .checked(self.vendor_filters[ix])
                .label(*vendor)
                .on_click(move |checked, _, cx| {
                    let checked = *checked;
                    me.update(cx, |this, cx| {
                        this.vendor_filters[ix] = checked;
                        cx.notify();
                    });
                })
        });

        v_flex()
            .w(px(LEFT_W))
            .h_full()
            .px(px(12.))
            .py(px(10.))
            .gap(px(6.))
            .border_r_1()
            .border_color(to_hsla(p.border_soft))
            .child(self.filter_title("连接筛选"))
            .child(v_flex().gap(px(1.)).children(type_rows))
            .child(div().h(px(10.)))
            .child(self.filter_title("供应商筛选"))
            .child(v_flex().gap(px(1.)).children(vendor_rows))
    }

    fn filter_title(&self, title: &str) -> impl IntoElement {
        let p = self.pal();
        div()
            .py(px(3.))
            .text_size(px(12.))
            .text_color(to_hsla(p.foreground))
            .child(title.to_string())
    }

    fn choose_header(&self, me: &Entity<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, muted, border, hover) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.border_soft),
            to_hsla(p.hover),
        );
        let toggle = |id: &'static str, icon: IconName, active: bool, list: bool| {
            let me = me.clone();
            div()
                .id(id)
                .size(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(2.))
                .cursor_pointer()
                .when(active, |s| s.bg(to_hsla(p.selection)).border_1().border_color(to_hsla(p.accent)))
                .when(!active, move |s| s.hover(move |s| s.bg(hover)))
                .on_click(move |_, _, cx| {
                    me.update(cx, |this, cx| {
                        this.list_mode = list;
                        cx.notify();
                    });
                })
                .child(Icon::new(icon).with_size(px(13.)).text_color(if active { fg } else { muted }))
        };

        h_flex()
            .h(px(44.))
            .px(px(14.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(fg)
                    .child("选择一个连接类型:"),
            )
            .child(div().flex_1())
            .child(toggle("view-grid", IconName::LayoutGrid, !self.list_mode, false))
            .child(toggle("view-list", IconName::LayoutList, self.list_mode, true))
            .child(
                div()
                    .w(px(210.))
                    .child(Input::new(&self.search).h(px(24.))),
            )
    }

    fn choose_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let enabled = self.selected.and_then(|ix| TILES[ix].kind).is_some();
        let next = self
            .action_button("next", "下一步", cx)
            .disabled(!enabled)
            .on_click(cx.listener(|this, _, _, cx| this.goto_form(cx)));
        h_flex()
            .h(px(FOOTER_H))
            .px(px(12.))
            .gap(px(8.))
            .items_center()
            .border_t_1()
            .border_color(to_hsla(p.border))
            .child(
                Button::new("uri")
                    .label("通过 URI 新建连接...")
                    .h(px(26.))
                    .on_click(cx.listener(|this, _, _, cx| this.goto_uri_form(cx))),
            )
            .child(div().flex_1())
            .child(next)
            .child(
                Button::new("cancel")
                    .label("取消")
                    .h(px(26.))
                    .px(px(16.))
                    .on_click(|_, window, _| window.remove_window()),
            )
    }

    /// 默认动作键：Navicat 里它是带强调描边的浅色按钮，这里用选中的淡蓝底近似。
    fn action_button(&self, id: &'static str, label: &'static str, cx: &App) -> Button {
        let p = self.pal();
        let variant = ButtonCustomVariant::new(cx)
            .color(to_hsla(p.selection))
            .foreground(to_hsla(p.foreground))
            .hover(to_hsla(0xb8daf6))
            .active(to_hsla(0xa4cff3));
        Button::new(id)
            .label(label)
            .h(px(26.))
            .px(px(18.))
            .custom(variant)
    }

    fn form(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (fg, muted, border, subtle) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.border),
            to_hsla(p.subtle_foreground),
        );
        let can_save = self.can_save(cx);
        let tile = TILES
            .iter()
            .find(|t| t.kind == Some(self.kind))
            .unwrap_or(&TILES[0]);

        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                h_flex()
                    .h(px(40.))
                    .px(px(16.))
                    .gap(px(8.))
                    .items_center()
                    .border_b_1()
                    .border_color(border)
                    .child(logo(tile.color, tile.mark, 20.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(fg)
                            .child(tile.name.to_string()),
                    )
                    .child(div().text_xs().text_color(muted).child("常规")),
            )
            .child(
                v_flex()
                    .p(px(18.))
                    .gap(px(10.))
                    .when(self.uri_mode, |s| {
                        s.child(field("连接字符串", &fg, Input::new(&self.uri).h(px(24.))))
                    })
                    .child(field("连接名称", &fg, Input::new(&self.name).h(px(24.))))
                    .when(!self.uri_mode, |s| {
                        s.child(field("主机", &fg, Input::new(&self.host).h(px(24.))))
                            .child(field("端口", &fg, Input::new(&self.port).h(px(24.))))
                            .child(field("初始数据库", &fg, Input::new(&self.database).h(px(24.))))
                            .child(field("用户名", &fg, Input::new(&self.user).h(px(24.))))
                            .child(
                                h_flex()
                                    .gap(px(10.))
                                    .items_center()
                                    .child(div().w(px(84.)).text_xs().text_color(fg).child("密码"))
                                    .child(
                                        div().w(px(340.)).child(
                                            Input::new(&self.password).h(px(24.)).mask_toggle(),
                                        ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap(px(10.))
                                    .items_center()
                                    .child(div().w(px(84.)))
                                    .child(
                                        Checkbox::new("save-password")
                                            .checked(self.save_password)
                                            .label("保存密码")
                                            .on_click(cx.listener(|this, checked, _, cx| {
                                                this.save_password = *checked;
                                                cx.notify();
                                            })),
                                    ),
                            )
                    })
                    .child(
                        div()
                            .h(px(18.))
                            .text_xs()
                            .text_color(subtle)
                            .when_some(self.status.clone(), |s, text| s.child(text)),
                    ),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .h(px(FOOTER_H))
                    .px(px(12.))
                    .gap(px(8.))
                    .items_center()
                    .border_t_1()
                    .border_color(border)
                    .child(
                        Button::new("test")
                            .label("测试连接")
                            .h(px(26.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.status = Some("尚未接入数据库驱动，无法测试连接".into());
                                cx.notify();
                            })),
                    )
                    // 编辑态没有类型选择这一步，就不留「上一步」的死键
                    .when(!matches!(self.mode, ConnDialogMode::Edit(_)), |s| {
                        s.child(
                            Button::new("back")
                                .label("上一步")
                                .h(px(26.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.step = Step::Choose;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        self.action_button("ok", "确定", cx)
                            .disabled(!can_save)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    )
                    .child(
                        Button::new("cancel-form")
                            .label("取消")
                            .h(px(26.))
                            .px(px(16.))
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}

impl Render for NewConnectionDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_title(window);
        let p = self.pal();
        v_flex()
            .size_full()
            .bg(to_hsla(p.background))
            .text_color(to_hsla(p.foreground))
            .child(self.title_bar())
            .child(match self.step {
                Step::Choose => self.choose(cx).into_any_element(),
                Step::Form => self.form(cx).into_any_element(),
            })
    }
}

fn field(label: &str, fg: &Hsla, control: impl IntoElement) -> impl IntoElement {
    h_flex()
        .gap(px(10.))
        .items_center()
        .child(
            div()
                .w(px(84.))
                .text_xs()
                .text_color(*fg)
                .child(label.to_string()),
        )
        .child(div().w(px(340.)).child(control))
}

/// 纯色方块 + 双字母缩写：正式品牌图标要按官方素材接入，先用可辨识的占位。
fn logo(color: u32, mark: &str, size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .rounded(px((size * 0.22).round()))
        .bg(to_hsla(color))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(px((size * 0.34).round()))
                .font_bold()
                .text_color(to_hsla(0xffffff))
                .child(mark.to_string()),
        )
}

type UriParts = (DbKind, String, u16, String, String, String);

fn parse_uri(raw: &str) -> Option<UriParts> {
    let (scheme, rest) = raw.split_once("://")?;
    let kind = match scheme.to_ascii_lowercase().as_str() {
        "mysql" => DbKind::MySql,
        "postgres" | "postgresql" => DbKind::PostgreSql,
        "sqlserver" | "mssql" => DbKind::SqlServer,
        "mongodb" => DbKind::MongoDb,
        "redis" => DbKind::Redis,
        _ => return None,
    };
    let (credentials, location) = match rest.rsplit_once('@') {
        Some((credentials, location)) => (Some(credentials), location),
        None => (None, rest),
    };
    let (authority, database) = match location.split_once('/') {
        Some((authority, database)) => (authority, database),
        None => (location, ""),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().ok()?),
        None => (authority, kind.default_port()),
    };
    if host.is_empty() {
        return None;
    }
    let (user, password) = match credentials {
        Some(credentials) => match credentials.split_once(':') {
            Some((user, password)) => (user.to_string(), password.to_string()),
            None => (credentials.to_string(), String::new()),
        },
        None => (kind.default_user().to_string(), String::new()),
    };
    Some((
        kind,
        host.to_string(),
        port,
        database.to_string(),
        user,
        password,
    ))
}
