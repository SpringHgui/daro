//! 新建连接向导。参考界面里这是独立的顶层窗口，这里同样开新窗口，
//! 而不是在主窗口内叠一层遮罩。

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::DaroApp;
use crate::art;
use crate::db_icons;
use crate::ui;
use crate::mock::{Connection, DbKind};
use crate::palette::Palette;
use crate::to_hsla;

const TITLE_H: f32 = 24.;
const LEFT_W: f32 = 180.;
const TILE_W: f32 = 96.;
const TILE_H: f32 = 92.;
const COLS: usize = 5;

/// 表单页（参考界面「常规」页）的布局常量。数值来自参考截图（900×700 内容区）。
const TABS: [&str; 7] = ["常规", "高级", "数据库", "SSL", "SSH", "HTTP", "备注"];
const TABS_SQLITE: [&str; 5] = ["常规", "高级", "附加的数据库", "HTTP", "备注"];
const SQLITE_TYPES: [&str; 3] = ["现有的数据库文件", "新建 SQLite 3", "新建 SQLite 2"];
const LABEL_COL_W: f32 = 172.;
const FORM_W: f32 = 624.;
const WIDE_W: f32 = 428.;
const MID_W: f32 = 198.;
const BROWSE_W: f32 = 20.;

pub(crate) struct TypeTile {
    pub(crate) name: &'static str,
    pub(crate) kind: Option<DbKind>,
    /// 老版 daro 的引擎图标资源名
    pub(crate) icon: &'static str,
}

/// 只有接入了驱动的类型可选；其余照参考界面摆出来但置灰。
/// 连接树底部的筛选弹层复用同一份清单（见 main.rs）。
pub(crate) const TILES: [TypeTile; 9] = [
    TypeTile { name: "MySQL", kind: Some(DbKind::MySql), icon: "mysql" },
    TypeTile { name: "PostgreSQL", kind: Some(DbKind::PostgreSql), icon: "postgresql" },
    TypeTile { name: "SQL Server", kind: Some(DbKind::SqlServer), icon: "sqlserver" },
    TypeTile { name: "Oracle", kind: None, icon: "oracle" },
    TypeTile { name: "SQLite", kind: Some(DbKind::SQLite), icon: "sqlite" },
    TypeTile { name: "MariaDB", kind: None, icon: "mariadb" },
    TypeTile { name: "MongoDB", kind: Some(DbKind::MongoDb), icon: "mongodb" },
    TypeTile { name: "Redis", kind: Some(DbKind::Redis), icon: "redis" },
    TypeTile { name: "Snowflake", kind: None, icon: "snowflake" },
];

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Choose,
    Form,
}

/// 同一个窗口承担「新建」和「编辑」：参考界面的连接属性对话框与向导共用一套表单，
/// 编辑态只是跳过类型选择、并在保存时回写原连接。
#[derive(Clone, Copy)]
pub enum ConnDialogMode {
    Create,
    /// 类型已定（「扩展」页按驱动开），跳过类型选择直接进表单
    CreateWithKind(DbKind),
    Edit(usize),
}

pub fn open_new_connection(parent: Entity<DaroApp>, dark: bool, cx: &mut App) {
    open_connection_dialog(parent, dark, ConnDialogMode::Create, cx);
}

pub fn open_new_connection_of_kind(
    parent: Entity<DaroApp>,
    dark: bool,
    kind: DbKind,
    cx: &mut App,
) {
    open_connection_dialog(parent, dark, ConnDialogMode::CreateWithKind(kind), cx);
}

pub fn open_connection_dialog(
    parent: Entity<DaroApp>,
    dark: bool,
    mode: ConnDialogMode,
    cx: &mut App,
) {
    let editing = matches!(mode, ConnDialogMode::Edit(_));
    let title = if editing { "编辑连接" } else { "新建连接" };
    let mut options = ui::dialog_options();
    // 参考图内容区 900×700，另加 24 的自绘标题栏
    options.window_min_size = Some(size(px(900.), px(724.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(900.), px(724.)), cx));
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
    kind: DbKind,
    uri_mode: bool,
    /// SQLite 页的「类型」单选：0 现有文件 / 1 新建 SQLite 3 / 2 新建 SQLite 2。
    /// rusqlite 只认 SQLite 3 格式，选 2 时明确告知并阻止保存。
    sqlite_new: u8,
    last_title: Option<String>,
    save_password: bool,
    status: Option<SharedString>,
    /// 探针在后台跑：期间按钮置灰，避免连点堆出多个探测连接。
    testing: bool,
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
            ConnDialogMode::Create | ConnDialogMode::CreateWithKind(_) => None,
            ConnDialogMode::Edit(ix) => parent.read(cx).conns.get(ix).cloned(),
        };
        let initial = |pick: fn(&Connection) -> String, fallback: String| match &editing {
            Some(conn) => pick(conn),
            None => fallback,
        };
        let kind = match mode {
            ConnDialogMode::CreateWithKind(kind) => kind,
            _ => editing.as_ref().map(|c| c.kind).unwrap_or(DbKind::MySql),
        };
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索"));
        let uri = cx.new(|cx| {
            InputState::new(window, cx).placeholder("mysql://user:password@localhost:3306/db")
        });
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial(|c| c.name.clone(), String::new()))
        });
        // 按类型直开表单时，空字段先补上该类型的默认值（与 goto_form 同口径）
        let preset = matches!(mode, ConnDialogMode::CreateWithKind(_)).then_some(kind);
        let host = cx.new(|cx| {
            InputState::new(window, cx).default_value(initial(
                |c| c.host.clone(),
                preset.map(|_| "localhost".to_string()).unwrap_or_default(),
            ))
        });
        let port = cx.new(|cx| {
            InputState::new(window, cx).default_value(initial(
                |c| c.port.to_string(),
                preset
                    .map(|kind| kind.default_port().to_string())
                    .unwrap_or_default(),
            ))
        });
        let database = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial(|c| c.database.clone(), String::new()))
        });
        let user = cx.new(|cx| {
            InputState::new(window, cx).default_value(initial(
                |c| c.user.clone(),
                if preset == Some(DbKind::MySql) {
                    "root".to_string()
                } else {
                    String::new()
                },
            ))
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial(|c| c.password.clone(), String::new()))
                .masked(true)
        });
        // 参考界面新建连接时「保存密码」默认勾选
        let save_password = editing
            .as_ref()
            .map(|c| !c.password.is_empty())
            .unwrap_or(true);
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

        // 编辑态 / 按类型直开都直接从表单起步，同样把焦点落在连接名称上
        if editing.is_some() || preset.is_some() {
            let name = name.clone();
            cx.defer_in(window, move |_, window, cx| {
                name.update(cx, |state, cx| state.focus(window, cx));
            });
        }

        Self {
            parent,
            dark,
            mode,
            step: if editing.is_some() || matches!(mode, ConnDialogMode::CreateWithKind(_)) {
                Step::Form
            } else {
                Step::Choose
            },
            selected,
            list_mode: false,
            type_filters: [false; TILES.len()],
            kind,
            uri_mode: false,
            sqlite_new: 0,
            last_title: None,
            save_password,
            status: None,
            testing: false,
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

    /// 参考界面在选定类型后把类型名并进窗口标题（新建连接 (MySQL)）；编辑态不带类型。
    fn title_text(&self) -> String {
        let base = match self.mode {
            ConnDialogMode::Create | ConnDialogMode::CreateWithKind(_) => "新建连接",
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

    fn goto_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = self.selected.and_then(|ix| TILES[ix].kind) else {
            return;
        };
        let old_kind = self.kind;
        self.kind = kind;
        // 参考界面的习惯：进表单时空字段补上该类型的默认值
        // （主机 localhost、端口按类型，MySQL 再补用户名 root）。
        let port_value = self.port.read(cx).value().to_string();
        if port_value.is_empty() || port_value == old_kind.default_port().to_string() {
            let port = kind.default_port().to_string();
            self.port.update(cx, |s, cx| s.set_value(port, window, cx));
        }
        if self.host.read(cx).value().trim().is_empty() {
            let host = "localhost".to_string();
            self.host.update(cx, |s, cx| s.set_value(host, window, cx));
        }
        if kind == DbKind::MySql && self.user.read(cx).value().trim().is_empty() {
            let user = "root".to_string();
            self.user.update(cx, |s, cx| s.set_value(user, window, cx));
        }
        self.uri_mode = false;
        self.status = None;
        self.step = Step::Form;
        // 参考界面打开表单时把焦点放在连接名称上（聚焦底线为蓝）
        let name = self.name.clone();
        cx.defer_in(window, move |_, window, cx| {
            name.update(cx, |state, cx| state.focus(window, cx));
        });
        cx.notify();
    }

    fn goto_uri_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.uri_mode = true;
        self.status = None;
        self.step = Step::Form;
        let uri = self.uri.clone();
        cx.defer_in(window, move |_, window, cx| {
            uri.update(cx, |state, cx| state.focus(window, cx));
        });
        cx.notify();
    }

    fn can_save(&self, cx: &Context<Self>) -> bool {
        if self.name.read(cx).value().trim().is_empty() {
            return false;
        }
        if self.uri_mode {
            return !self.uri.read(cx).value().trim().is_empty();
        }
        if self.kind == DbKind::SQLite {
            // rusqlite 只支持 SQLite 3 文件格式，选 2 时不给保存
            return self.sqlite_new != 2 && !self.database.read(cx).value().trim().is_empty();
        }
        !self.host.read(cx).value().trim().is_empty() && self.port.read(cx).value().parse::<u16>().is_ok()
    }

    /// 表单 → 连接参数：「确定」和「测试连接」共用一套取字段口径。
    fn form_parts(&self, cx: &Context<Self>) -> Option<UriParts> {
        if self.uri_mode {
            return parse_uri(self.uri.read(cx).value().trim());
        }
        Some((
            self.kind,
            self.host.read(cx).value().trim().to_string(),
            self.port.read(cx).value().parse::<u16>().unwrap_or(self.kind.default_port()),
            self.database.read(cx).value().trim().to_string(),
            self.user.read(cx).value().trim().to_string(),
            self.password.read(cx).value().to_string(),
        ))
    }

    /// 「测试连接」：按当前表单临时连一次，跑一条探针语句就走。
    /// 探针会阻塞（TCP 超时 / 认证往返），必须丢后台，否则整个窗口白屏。
    /// 不注册进驱动的连接表，也不写回列表——点了测试不保存同样不留痕。
    fn test_connection(&mut self, cx: &mut Context<Self>) {
        if self.testing {
            return;
        }
        let say = |this: &mut Self, text: String, cx: &mut Context<Self>| {
            this.status = Some(text.into());
            cx.notify();
        };
        let Some((kind, host, port, database, user, password)) = self.form_parts(cx) else {
            say(self, "无法识别的连接字符串".into(), cx);
            return;
        };
        if !crate::backend::is_real(kind) {
            say(self, "尚未接入数据库驱动，无法测试连接".into(), cx);
            return;
        }
        if kind == DbKind::SQLite && database.trim().is_empty() {
            say(self, "请填写数据库文件路径".into(), cx);
            return;
        }
        let conn = Connection::new(
            String::new(),
            kind,
            host,
            port,
            database,
            user,
            password,
            "",
            false,
        );
        // 约定：先把「测试中」推上屏，再去后台占线程
        self.testing = true;
        say(self, "测试中…".into(), cx);
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { crate::backend::probe(&conn) })
                .await;
            let text = match res {
                Ok(()) => "连接成功".to_string(),
                Err(e) => format!("连接失败：{e}"),
            };
            let _ = this.update(cx, |this, cx| {
                this.testing = false;
                this.status = Some(text.into());
                cx.notify();
            });
        })
        .detach();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_save(cx) {
            return;
        }
        let name = self.name.read(cx).value().trim().to_string();
        let (kind, host, port, database, user, password) = match self.form_parts(cx) {
            Some(parts) => parts,
            None => {
                self.status = Some("无法识别的连接字符串".into());
                cx.notify();
                return;
            }
        };
        match self.mode {
            ConnDialogMode::Create | ConnDialogMode::CreateWithKind(_) => {
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
                        ix, name, kind, host, port, database, user, password, false,
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
        TitleBar::new().h(px(TITLE_H)).border_b_0().child(
            h_flex()
                .h_full()
                .items_center()
                .gap(px(6.))
                .pl(px(4.))
                .child(crate::art::icon(crate::art::Glyph::Db, 14., to_hsla(p.accent)))
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
                ui::dialog_surface(self.dark)
                    .child(
                        h_flex()
                            .size_full()
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
                                                        s.child(self.section(
                                                            "最近使用过的",
                                                            &recent,
                                                            cx,
                                                        ))
                                                    })
                                                    .when(visible.is_empty(), |s| {
                                                        s.child(
                                                            div()
                                                                .py(px(24.))
                                                                .text_xs()
                                                                .text_color(to_hsla(
                                                                    p.muted_foreground,
                                                                ))
                                                                .child("没有匹配的连接类型"),
                                                        )
                                                    })
                                                    .when(!visible.is_empty(), |s| {
                                                        s.child(self.section("全部", &visible, cx))
                                                    }),
                                            ),
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
            .on_double_click(move |_, window, cx| {
                me2.update(cx, |this, cx| {
                    this.choose_tile(ix);
                    this.goto_form(window, cx);
                });
            })
            .child(db_icons::vector_tile(tile.icon, 42.))
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
            .on_double_click(move |_, window, cx| {
                me2.update(cx, |this, cx| {
                    this.choose_tile(ix);
                    this.goto_form(window, cx);
                });
            })
            .child(db_icons::vector_tile(tile.icon, 18.))
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
        let (filters, dark) = (self.type_filters, self.dark);
        let type_rows = TILES.iter().enumerate().map({
            let me = me.clone();
            move |(ix, tile)| {
                let me = me.clone();
                ui::checkbox(
                    ("type-filter", ix),
                    tile.name,
                    filters[ix],
                    true,
                    dark,
                    move |_, _, cx| {
                        me.update(cx, |this, cx| {
                            this.type_filters[ix] = !this.type_filters[ix];
                            cx.notify();
                        });
                    },
                )
            }
        });

        v_flex()
            .w(px(LEFT_W))
            .h_full()
            .px(px(12.))
            .py(px(10.))
            .gap(px(6.))
            .border_r(px(ui::HAIRLINE))
            .border_color(to_hsla(p.dialog_line))
            .child(self.filter_title("连接筛选"))
            .child(v_flex().gap(px(1.)).children(type_rows))
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
        let (grid_glyph, list_glyph) = if self.dark {
            ("GLYPH_GRIDVIEW_DARK", "GLYPH_LISTVIEW_DARK")
        } else {
            ("GLYPH_GRIDVIEW", "GLYPH_LISTVIEW")
        };
        let toggle = |id: &'static str, glyph: &'static str, active: bool, list: bool| {
            let me = me.clone();
            div()
                .id(id)
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(ui::RADIUS))
                .cursor_pointer()
                .when(active, |s| s.bg(to_hsla(p.selection)))
                .when(!active, move |s| s.hover(move |s| s.bg(hover)))
                .on_click(move |_, _, cx| {
                    me.update(cx, |this, cx| {
                        this.list_mode = list;
                        cx.notify();
                    });
                })
                .child(art::icon(art::Glyph::Asset(glyph), 13., muted))
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
            .child(toggle("view-grid", grid_glyph, !self.list_mode, false))
            .child(toggle("view-list", list_glyph, self.list_mode, true))
            .child(
                div()
                    .w(px(210.))
                    .child(Input::new(&self.search).h(px(24.))),
            )
    }

    fn choose_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.selected.and_then(|ix| TILES[ix].kind).is_some();
        ui::footer_bar(self.dark)
            .child(
                ui::link_button("uri", "通过 URI 新建连接...", self.dark)
                    .on_click(cx.listener(|this, _, window, cx| this.goto_uri_form(window, cx))),
            )
            .child(div().flex_1())
            .child(
                ui::primary_button_enabled("next", "下一步", self.dark, enabled, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.goto_form(window, cx))),
            )
            .child(
                ui::secondary_button("cancel", "取消")
                    .on_click(|_, window, _| window.remove_window()),
            )
    }

    fn tabs(&self) -> &'static [&'static str] {
        if self.kind == DbKind::SQLite && !self.uri_mode {
            &TABS_SQLITE
        } else {
            &TABS
        }
    }

    /// 标签列（172）+ 控件；冒号随标签一起给出。
    fn form_line(&self, label: &'static str, control: impl IntoElement) -> Div {
        h_flex()
            .items_center()
            .child(ui::dialog_label(format!("{label}:"), self.dark, LABEL_COL_W))
            .child(control)
    }

    /// 与输入列对齐的缩进行（复选框 / 提示）。
    fn indent(&self, child: impl IntoElement) -> Div {
        h_flex()
            .items_center()
            .child(div().w(px(LABEL_COL_W)))
            .child(child)
    }

    fn browse_button(&self, cx: &mut Context<Self>) -> Button {
        ui::secondary_button("browse-db", "...")
            .h(px(BROWSE_W))
            .w(px(BROWSE_W))
            .min_w(px(BROWSE_W))
            .px(px(0.))
            .text_size(px(12.))
            .on_click(cx.listener(|this, _, window, cx| {
                let dialog = rfd::FileDialog::new()
                    .set_title("选择 SQLite 数据库文件")
                    .add_filter("SQLite 数据库", &["db", "sqlite", "sqlite3"]);
                let picked = if this.sqlite_new == 0 {
                    dialog.pick_file()
                } else {
                    dialog.set_file_name("新建数据库.db").save_file()
                };
                if let Some(path) = picked {
                    let text = path.display().to_string();
                    this.database
                        .update(cx, |s, cx| s.set_value(text, window, cx));
                    cx.notify();
                }
            }))
    }

    fn sqlite_types(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().gap(px(8.)).children((0..3u8).map(|ix| {
            ui::radio(
                ("sqlite-type", ix as usize),
                SQLITE_TYPES[ix as usize],
                self.sqlite_new == ix,
                true,
                self.dark,
                cx.listener(move |this, _, _, cx| {
                    this.sqlite_new = ix;
                    this.status = if ix == 2 {
                        Some("SQLite 2 文件格式已不再支持，请选择「新建 SQLite 3」".into())
                    } else {
                        None
                    };
                    cx.notify();
                }),
            )
        }))
    }

    /// 表单页：参考图的 900×700 内容区 —— 页签条、
    /// 标签列 + 428 宽输入列、底部动作栏。
    fn form(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let subtle = to_hsla(p.subtle_foreground);

        let mut body = v_flex()
            .w(px(FORM_W))
            .pt(px(24.))
            .child(self.form_line(
                "连接名称",
                div()
                    .w(px(WIDE_W))
                    .child(ui::field_input(&self.name, self.dark, window, cx)),
            ));

        if self.uri_mode {
            body = body.child(
                self.form_line(
                    "连接字符串",
                    div()
                        .w(px(WIDE_W))
                        .child(ui::field_input(&self.uri, self.dark, window, cx)),
                )
                .mt(px(38.)),
            );
        } else if self.kind == DbKind::SQLite {
            body = body
                .child(
                    h_flex()
                        .items_start()
                        .mt(px(38.5))
                        .child(ui::dialog_label("类型:", self.dark, LABEL_COL_W))
                        .child(self.sqlite_types(cx)),
                )
                .child(
                    self.form_line(
                        "数据库文件",
                        h_flex()
                            .items_center()
                            .gap(px(4.))
                            .w(px(WIDE_W + 4. + BROWSE_W))
                            .child(
                                div()
                                    .w(px(WIDE_W))
                                    .child(ui::field_input(&self.database, self.dark, window, cx)),
                            )
                            .child(self.browse_button(cx)),
                    )
                    .mt(px(5.5)),
                )
                .child(
                    self.form_line(
                        "用户名",
                        div()
                            .w(px(MID_W))
                            .child(ui::field_input(&self.user, self.dark, window, cx)),
                    )
                    .mt(px(8.)),
                )
                .child(
                    self.form_line(
                        "密码",
                        div()
                            .w(px(MID_W))
                            .child(ui::field_input(&self.password, self.dark, window, cx)),
                    )
                    .mt(px(8.)),
                )
                .child(
                    self.indent(ui::checkbox(
                        "save-password",
                        "保存密码",
                        self.save_password,
                        true,
                        self.dark,
                        cx.listener(|this, _, _, cx| {
                            this.save_password = !this.save_password;
                            cx.notify();
                        }),
                    ))
                    .mt(px(10.)),
                );
        } else {
            body = body
                .child(
                    self.form_line(
                        "主机",
                        div()
                            .w(px(WIDE_W))
                            .child(ui::field_input(&self.host, self.dark, window, cx)),
                    )
                    .mt(px(38.)),
                )
                .child(
                    self.form_line(
                        "端口",
                        div()
                            .w(px(MID_W))
                            .child(ui::field_input(&self.port, self.dark, window, cx)),
                    )
                    .mt(px(8.)),
                )
                .child(
                    self.form_line(
                        "初始数据库",
                        div()
                            .w(px(WIDE_W))
                            .child(ui::field_input(&self.database, self.dark, window, cx)),
                    )
                    .mt(px(8.)),
                )
                .child(
                    self.form_line(
                        "用户名",
                        div()
                            .w(px(MID_W))
                            .child(ui::field_input(&self.user, self.dark, window, cx)),
                    )
                    .mt(px(8.)),
                )
                .child(
                    self.form_line(
                        "密码",
                        div()
                            .w(px(MID_W))
                            .child(ui::field_input(&self.password, self.dark, window, cx)),
                    )
                    .mt(px(8.)),
                )
                .child(
                    self.indent(ui::checkbox(
                        "save-password",
                        "保存密码",
                        self.save_password,
                        true,
                        self.dark,
                        cx.listener(|this, _, _, cx| {
                            this.save_password = !this.save_password;
                            cx.notify();
                        }),
                    ))
                    .mt(px(10.)),
                );
        }

        body = body.child(
            self.indent(
                div()
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .text_color(subtle)
                    .when_some(self.status.clone(), |s, text| s.child(text)),
            )
            .mt(px(6.)),
        );

        v_flex()
            .flex_1()
            .min_h_0()
            .child(ui::page_tabs(self.tabs(), 0, self.dark))
            .child(
                ui::dialog_surface(self.dark)
                    .child(v_flex().w_full().items_center().child(body)),
            )
            .child(self.form_footer(cx))
    }

    /// 表单页底部动作栏：左边测试连接 / URI，右边上一步 / 确定 / 取消。
    fn form_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_save = self.can_save(cx);
        ui::footer_bar(self.dark)
            .child(
                ui::secondary_button("test", "测试连接")
                    .w(px(148.))
                    .disabled(self.testing)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.test_connection(cx);
                    })),
            )
            .child(
                ui::secondary_button("uri", "URI...")
                    .on_click(cx.listener(|this, _, window, cx| this.goto_uri_form(window, cx))),
            )
            .child(div().flex_1())
            // 编辑态与「扩展」页按类型直开的表单都没有类型选择这一步，不留「上一步」的死键
            .when(matches!(self.mode, ConnDialogMode::Create), |s| {
                s.child(
                    ui::secondary_button("back", "上一步").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.step = Step::Choose;
                            cx.notify();
                        },
                    )),
                )
            })
            .child(
                ui::primary_button_enabled("ok", "确定", self.dark, can_save, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
            .child(
                ui::secondary_button("cancel-form", "取消")
                    .on_click(|_, window, _| window.remove_window()),
            )
    }
}

impl Render for NewConnectionDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_title(window);
        let p = self.pal();
        v_flex()
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(to_hsla(p.foreground))
            .on_key_down(cx.listener(|_, ev: &KeyDownEvent, window, _cx| {
                if ev.keystroke.key == "escape" {
                    window.remove_window();
                }
            }))
            .child(self.title_bar())
            .child(match self.step {
                Step::Choose => self.choose(cx).into_any_element(),
                Step::Form => self.form(window, cx).into_any_element(),
            })
    }
}

/// 纯色方块 + 双字母缩写：正式品牌图标要按官方素材接入，先用可辨识的占位。
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
