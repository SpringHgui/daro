//! 「另存为」保存查询对话框，样式按参考界面截图 1:1 复刻。
//!
//! 收起态（402×187）只有查询名称与保存位置；点位置行右侧箭头后窗口向下
//! 长到 402×321，露出连接 / 目录 / 模式三个下拉，箭头翻转朝上。
//! 下拉列表放在单独的无边框置顶窗口里，像原生命令一样可以盖到对话框外面，
//! 对话框自身的尺寸不受列表长度影响。
//! 尺寸由截图（2× 物理像素）折算成逻辑像素：按钮 74×24，正文 12px，
//! 左右留白 18，下拉行与查询窗口同款 22 高、行距 47。

use std::time::Duration;

use crate::zed_icons::ZedIcon;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::DaroApp;
use crate::art::{self, Glyph};
use crate::catalog;
use crate::combo;
use crate::combo::{PAD_Y as MENU_PAD_Y, ROW_H as MENU_ROW_H};
use crate::db_icons;
use crate::mock::{Connection, DbKind};
use crate::to_hsla;
use crate::ui;

/// 窗口宽度（截图 804 的一半）
const WIN_W: f32 = 402.;
/// 收起态高度
const WIN_H: f32 = 187.;
/// 展开态高度（截图 636 的一半，含连接 / 目录 / 模式三行）
const WIN_H_OPEN: f32 = 321.;
/// 自绘标题栏高
const BAR_H: f32 = 32.;
/// 正文左右留白
const MARGIN_X: f32 = 18.;
/// 标题栏 → 第一个标签
const PAD_TOP: f32 = 17.;
/// 标签行高（12px 文字的行盒）
const LABEL_H: f32 = 16.;
/// 标签 → 查询名称输入框
const LABEL_TO_INPUT: f32 = 1.;
/// 输入框白底高（不含下划线）
const INPUT_H: f32 = 18.;
/// 下划线高
const UNDERLINE_H: f32 = 1.;
/// 输入框 → 下一个标签
const INPUT_TO_LABEL: f32 = 9.;
/// 标签 → 保存位置行
const LABEL_TO_LOC: f32 = 4.;
/// 保存位置行（= 箭头按钮）高
const LOC_H: f32 = 20.;
/// 保存位置行顶边到窗口顶的距离
const LOC_TOP: f32 = BAR_H
    + PAD_TOP
    + LABEL_H
    + LABEL_TO_INPUT
    + INPUT_H
    + UNDERLINE_H
    + INPUT_TO_LABEL
    + LABEL_H
    + LABEL_TO_LOC;
/// 位置行 → 第一个下拉标签
const COMBO_FIRST_GAP: f32 = 2.;
/// 标签 → 下拉
const COMBO_LABEL_GAP: f32 = 2.;
/// 下拉 → 下一个标签
const COMBO_BLOCK_GAP: f32 = 7.;
/// 下拉框高（与查询窗口的运行上下文下拉一致）
const COMBO_H: f32 = 22.;
/// 相邻下拉顶边的间距
const COMBO_PITCH: f32 = COMBO_H + COMBO_BLOCK_GAP + LABEL_H + COMBO_LABEL_GAP;
/// 第一个下拉的顶边
const COMBO1_TOP: f32 = LOC_TOP + LOC_H + COMBO_FIRST_GAP + LABEL_H + COMBO_LABEL_GAP;
/// 按钮尺寸（截图 148×48 的一半）
const BTN_W: f32 = 74.;
const BTN_H: f32 = 24.;
/// OK 与 Cancel 的间距
const BTN_GAP: f32 = 6.;
/// 按钮行 → 窗口底
const PAD_BOTTOM: f32 = 17.;
/// 浮层最多完整露出多少行；内容高不超过查询窗口的 420
const MENU_MAX_ROWS: usize = 18;

/// 浮层高度：行数 × 行高 + 上下留白 + 0.5px 边框。
fn popup_height(rows: usize) -> f32 {
    let shown = rows.min(MENU_MAX_ROWS) as f32;
    shown * MENU_ROW_H + 2. * MENU_PAD_Y + 1.
}

/// 大小写不敏感的子串查找，返回原串里的字节区间（命中高亮用）。
fn find_ci(hay: &str, needle: &str) -> Option<std::ops::Range<usize>> {
    let needle = needle.to_lowercase();
    if needle.is_empty() {
        return None;
    }
    let lower = hay.to_lowercase();
    let start = lower.find(&needle)?;
    Some(start..start + needle.len())
}

fn contains_ci(hay: &str, needle: &str) -> bool {
    find_ci(hay, needle).is_some()
}

/// 打开对话框时的当前上下文。库 / 模式清单按连接现查，查不到时回落到
/// 连接配置里的库与当前模式。
pub struct SaveInit {
    pub conns: Vec<Connection>,
    pub connection: String,
    pub database: String,
    pub schema: String,
}

/// 点 OK 交回的结果：名称 + 连接 / 目录 / 模式。
pub struct SaveChoice {
    pub name: String,
    pub connection: String,
    pub database: String,
    pub schema: String,
}

type OnOk = Box<dyn Fn(&mut DaroApp, SaveChoice)>;

/// 对话框配色：浅色全部取自截图实测，深色按同一层次换算。
#[derive(Clone, Copy)]
struct Skin {
    bar: u32,
    body: u32,
    text: u32,
    field: u32,
    field_edge: u32,
    accent: u32,
    btn: u32,
    btn_edge: u32,
    combo_edge: u32,
    hover: u32,
    /// 箭头、次要文字
    muted: u32,
    /// 实心主按钮上的文字色
    on_accent: u32,
}

impl Skin {
    /// 截图实测：标题栏 #f3f3f3、正文 #f0f0f0、输入框白底、下划线 #0067c0、
    /// 按钮 #fdfdfd 配 #d0d0d0 描边、下拉 #8d8d8d 描边、默认按钮描边 #0078d4。
    const LIGHT: Self = Self {
        bar: 0xdcdcdd,
        body: 0xdcdcdd,
        text: 0x242529,
        field: 0xebebec,
        field_edge: 0xc9c9ca,
        accent: 0x5c78e2,
        btn: 0xebebec,
        btn_edge: 0xc9c9ca,
        combo_edge: 0xc9c9ca,
        hover: 0xdfdfe0,
        muted: 0x58585a,
        on_accent: 0xffffff,
    };

    const DARK: Self = Self {
        bar: 0x3b414d,
        body: 0x3b414d,
        text: 0xdce0e5,
        field: 0x2e343e,
        field_edge: 0x464b57,
        accent: 0x74ade8,
        btn: 0x2e343e,
        btn_edge: 0x464b57,
        combo_edge: 0x464b57,
        hover: 0x363c46,
        muted: 0xa9afbc,
        on_accent: 0x1f2328,
    };

    fn of(dark: bool) -> Self {
        if dark { Self::DARK } else { Self::LIGHT }
    }
}

/// 对话框配色映射给共用的组合框组件。
fn combo_colors(dark: bool) -> combo::Colors {
    let s = Skin::of(dark);
    let p = crate::palette::Palette::of(dark);
    combo::Colors {
        trigger_bg: to_hsla(s.field),
        menu_bg: to_hsla(s.field),
        text: to_hsla(s.text),
        muted: to_hsla(s.muted),
        edge: to_hsla(s.combo_edge),
        focus: to_hsla(p.field_focus),
        selected: to_hsla(p.selection),
        hover: to_hsla(p.hover),
        highlight: to_hsla(p.accent).opacity(0.35),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn open(
    parent: Entity<DaroApp>,
    dark: bool,
    title: &'static str,
    default_name: String,
    start: SaveInit,
    on_ok: impl Fn(&mut DaroApp, SaveChoice) + 'static,
    cx: &mut App,
) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(WIN_W), px(WIN_H)));
    options.window_bounds = Some(WindowBounds::centered(size(px(WIN_W), px(WIN_H)), cx));
    // 参考界面的另存为不可拉伸，标题栏也只有 ×
    options.is_resizable = false;
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some(title.into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            SaveAsDialog::new(
                parent,
                dark,
                title,
                default_name,
                start,
                Box::new(on_ok),
                window,
                cx,
            )
        })
    });
    if opened.is_err() {
        eprintln!("failed to open save-as window");
    }
}

/// 展开后露出的三行下拉。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Combo {
    Conn,
    Db,
    Schema,
}

impl Combo {
    /// 下拉框下沿到窗口顶边的距离（也就是浮层顶边）。
    fn menu_top(self) -> f32 {
        let row = match self {
            Combo::Conn => 0.,
            Combo::Db => 1.,
            Combo::Schema => 2.,
        };
        COMBO1_TOP + row * COMBO_PITCH + COMBO_H
    }
}

/// 浮层里的一行：图标 + 文本。图标只存种类，绘制时与查询窗口下拉用同一套资源。
#[derive(Clone, Copy)]
enum PopupIcon {
    Conn(DbKind),
    Db,
    Schema,
}

struct PopupRow {
    /// 在完整候选清单里的下标：筛选后行号会变，id 与选中都按这个走
    index: usize,
    icon: Option<PopupIcon>,
    text: String,
}

/// 下拉浮层：独立的无边框置顶窗口。Gaia 的窗口内浮层会被对话框边界裁掉，
/// 只有另开窗口才能像原生下拉一样露到对话框外面。点选后由浮层自己关窗，
/// 对话框通过 observe_release 收回状态。
///
/// 打开时窗口不抢激活（`WindowOptions::focus = false`）：焦点留在对话框里
/// 触发器的筛选输入框上，键入即过滤；浮层被点中后才会激活，再失活就关闭。
struct ComboPopup {
    owner: WeakEntity<SaveAsDialog>,
    kind: Combo,
    dark: bool,
    _esc: Subscription,
    /// 已按筛选词过滤过的行（键入时由对话框推过来）
    rows: Vec<PopupRow>,
    /// 当前高亮行（筛选后的行号）；键入后回到无高亮，与查询窗口一致
    active: Option<usize>,
    /// 筛选词：命中子串铺黄底
    needle: String,
    /// 窗口激活过一次之后再失去激活（点别处 / 切到别的应用）就关闭
    seen_active: bool,
    /// 接收 Esc / 方向键的焦点（浮层被点中后生效）
    focus: FocusHandle,
    /// 选中后把焦点还给对话框的名称框
    dialog_win: AnyWindowHandle,
    name: Entity<InputState>,
}

impl ComboPopup {
    #[allow(clippy::too_many_arguments)]
    fn new(
        owner: WeakEntity<SaveAsDialog>,
        kind: Combo,
        dark: bool,
        rows: Vec<PopupRow>,
        active: Option<usize>,
        needle: String,
        dialog_win: AnyWindowHandle,
        name: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let seen_active = window.is_window_active();
        // Win11 建窗时就会套圆角遮罩，属性要在显示前设；这里失败还有建窗后的兜底调用
        ui::square_corners(window);
        cx.observe_window_activation(window, |this, window, _cx| {
            if window.is_window_active() {
                this.seen_active = true;
            } else if this.seen_active {
                window.remove_window();
            }
        })
        .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let esc = ui::close_on_escape(window, cx);
        Self {
            owner,
            kind,
            dark,
            _esc: esc,
            rows,
            active,
            needle,
            seen_active,
            focus,
            dialog_win,
            name,
        }
    }

    /// 键盘操作落在浮层上时（点过列表之后），把焦点交回对话框名称框。
    fn focus_dialog(&self, cx: &mut Context<Self>) {
        let name = self.name.clone();
        let _ = cx.update_window(self.dialog_win, |_, window, cx| {
            window.activate_window();
            name.update(cx, |state, cx| state.focus(window, cx));
        });
    }
}

impl Render for ComboPopup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = combo_colors(self.dark);
        let rows: Vec<AnyElement> = self
            .rows
            .iter()
            .enumerate()
            .map(|(ix, row)| {
                let icon = row.icon.map(|icon| match icon {
                    PopupIcon::Conn(kind) => db_icons::engine_bar(kind, 16.),
                    PopupIcon::Db => db_icons::database(16.),
                    PopupIcon::Schema => art::icon(Glyph::Schema, 16., colors.muted),
                });
                let kind = self.kind;
                let name = row.text.clone();
                combo::row(
                    combo::RowSpec {
                        id: format!("saveas-popup/{}", row.index),
                        label: row.text.clone(),
                        icon,
                        enabled: true,
                        highlight: find_ci(&row.text, &self.needle),
                        active: self.active == Some(ix),
                        hint: None,
                    },
                    colors,
                    cx,
                    move |this, window, cx| {
                        let _ = this
                            .owner
                            .update(cx, |dialog, cx| dialog.choose(kind, &name, cx));
                        this.focus_dialog(cx);
                        window.remove_window();
                    },
                )
            })
            .collect();
        div()
            .id("saveas-popup")
            .test_support()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                // 列表被点中后键盘落在浮层上：行为与对话框里的筛选输入框一致
                match ev.keystroke.key.as_str() {
                    "enter" => {
                        let _ = this.owner.update(cx, |dialog, cx| dialog.pick_active(cx));
                        this.focus_dialog(cx);
                        window.remove_window();
                        cx.stop_propagation();
                    }
                    "up" => {
                        let _ = this.owner.update(cx, |dialog, cx| dialog.move_active(false, cx));
                        cx.stop_propagation();
                    }
                    "down" => {
                        let _ = this.owner.update(cx, |dialog, cx| dialog.move_active(true, cx));
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .child(combo::panel(rows, combo::PanelFit::Fill, colors))
    }
}

struct SaveAsDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    _esc: Subscription,
    title: &'static str,
    name: Entity<InputState>,
    conns: Vec<Connection>,
    conn_ix: usize,
    db_ix: usize,
    schema_ix: usize,
    init_conn: String,
    init_db: String,
    init_schema: String,
    /// 是否展开了连接 / 目录 / 模式
    open: bool,
    /// 当前展开的哪个下拉列表
    combo: Option<Combo>,
    /// 打开态正文里的筛选输入框（三个下拉共用一个，与查询窗口同款）
    filter: Entity<InputState>,
    /// 当前的筛选词；打开时为空（回填的当前值不算筛选词）
    needle: String,
    /// 筛选后列表里的高亮行
    active: Option<usize>,
    /// 浮层窗口句柄（列表浮在对话框外面）
    popup: Option<AnyWindowHandle>,
    /// 浮层实体：键入过滤时把新行推给它
    popup_entity: Option<Entity<ComboPopup>>,
    /// 本对话框自己的窗口句柄：浮层关闭后把焦点还回来
    win: AnyWindowHandle,
    /// 标题栏按下后要先让窗口自己接管拖动，直接 start_window_move 会吃掉 × 的点击
    dragging: bool,
    on_ok: OnOk,
    /// Enter 与按钮可能同时到达，提交只认第一次
    submitted: bool,
}

impl SaveAsDialog {
    #[allow(clippy::too_many_arguments)]
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        title: &'static str,
        default_name: String,
        start: SaveInit,
        on_ok: OnOk,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let SaveInit {
            conns,
            connection,
            database,
            schema,
        } = start;
        let name = cx.new(|cx| InputState::new(window, cx).default_value(default_name));
        let window_handle = window.window_handle();
        // 组合框的筛选输入：打开下拉时挂进触发器并聚焦，键入即过滤候选。
        // set_value 不触发 Change，所以打开时回填当前值不会被当成筛选词。
        let filter = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe(&filter, move |this, input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change)
                && let Some(kind) = this.combo
            {
                this.needle = input.read(cx).value().trim().to_string();
                this.active = None;
                this.refresh_popup(kind, true, cx);
                cx.notify();
            }
        })
        .detach();
        // 对话框被直接关掉时，浮层不能留在桌面上
        cx.on_release(|this, cx| {
            if let Some(handle) = this.popup.take() {
                let _ = cx.update_window(handle, |_, window, _| window.remove_window());
            }
        })
        .detach();
        // 切到别的应用时收起浮层；点浮层自身也会让对话框短暂失活，
        // 等一拍再判断，避免把正要选中的列表关掉。
        cx.observe_window_activation(window, move |this, window, cx| {
            if window.is_window_active() || this.combo.is_none() {
                return;
            }
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let _ = this.update_in(cx, |this, window, cx| {
                    let by_popup = this
                        .popup
                        .is_some_and(|handle| cx.active_window() == Some(handle));
                    if this.combo.is_some() && !window.is_window_active() && !by_popup {
                        this.combo = None;
                        this.needle.clear();
                        this.active = None;
                        this.close_popup(cx);
                        cx.notify();
                    }
                });
            })
            .detach();
        })
        .detach();
        cx.subscribe(&name, move |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                // 订阅回调拿不到 Window，靠窗口句柄关窗（参考界面的确认框 Enter = 确定）
                if this.combo.is_none() && this.commit(cx) {
                    let _ = cx.update_window(window_handle, |_, window, _| window.remove_window());
                }
            } else {
                cx.notify();
            }
        })
        .detach();
        cx.defer_in(window, |this, window, cx| {
            this.name.update(cx, |state, cx| {
                state.focus(window, cx);
                // 预填名打开即全选：直接打字即替换
                state.select_all(window, cx);
            });
        });
        // Esc 分层收：先收浮层 / 展开态，都没有才关窗（与原来的冒泡口径一致）
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
            this.update(cx, |this, cx| {
                if this.combo.is_some() {
                    this.close_combo(window, cx);
                } else if this.open {
                    this.set_open(false, window, cx);
                } else {
                    window.remove_window();
                }
            });
        });
        let mut this = Self {
            parent,
            dark,
            _esc: esc,
            title,
            name,
            conns,
            conn_ix: 0,
            db_ix: 0,
            schema_ix: 0,
            init_conn: connection,
            init_db: database,
            init_schema: schema,
            open: false,
            combo: None,
            filter,
            needle: String::new(),
            active: None,
            popup: None,
            popup_entity: None,
            win: window_handle,
            dragging: false,
            on_ok,
            submitted: false,
        };
        this.conn_ix = this
            .conns
            .iter()
            .position(|c| c.name == this.init_conn)
            .unwrap_or(0);
        let db_ix = this.dbs().iter().position(|d| *d == this.init_db);
        this.db_ix = db_ix.unwrap_or(0);
        let schema_ix = this.schemas().iter().position(|s| *s == this.init_schema);
        this.schema_ix = schema_ix.unwrap_or(0);
        // 模态对话框在 Windows 上会按错误的边框缓存缩小（见 ui::fit_dialog_client），
        // 先拉回设计尺寸，否则收起态底部按钮行直接落在视口外。
        ui::fit_dialog_client(window, (px(WIN_W), px(WIN_H)));
        this
    }

    fn skin(&self) -> Skin {
        Skin::of(self.dark)
    }

    /// 当前连接可选的库：真实连接可能还没异步拉回库清单，先回落到配置里的库。
    fn dbs(&self) -> Vec<String> {
        let Some(conn) = self.conns.get(self.conn_ix) else {
            return Vec::new();
        };
        let list = catalog::databases(conn);
        if !list.is_empty() {
            return list.iter().map(|db| (*db).to_string()).collect();
        }
        let fallback = if conn.name == self.init_conn {
            self.init_db.as_str()
        } else {
            conn.database.as_str()
        };
        if fallback.is_empty() {
            Vec::new()
        } else {
            vec![fallback.to_string()]
        }
    }

    /// 当前连接可选的模式：只有 PG / SQL Server 有这一层。
    fn schemas(&self) -> Vec<String> {
        let Some(conn) = self.conns.get(self.conn_ix) else {
            return Vec::new();
        };
        if !catalog::has_schemas(conn.kind) {
            return Vec::new();
        }
        let db = self.current_db();
        let list = catalog::schemas(conn, &db);
        if !list.is_empty() {
            return list.iter().map(|s| (*s).to_string()).collect();
        }
        // 模式清单要么还没从真实连接拉回来，要么是演示连接：先给该库的默认模式
        if conn.name == self.init_conn && db == self.init_db && !self.init_schema.is_empty() {
            return vec![self.init_schema.clone()];
        }
        crate::mock::schemas(conn.kind)
            .first()
            .map(|s| vec![(*s).to_string()])
            .unwrap_or_default()
    }

    fn current_db(&self) -> String {
        self.dbs().get(self.db_ix).cloned().unwrap_or_default()
    }

    fn current_schema(&self) -> String {
        self.schemas().get(self.schema_ix).cloned().unwrap_or_default()
    }

    /// 取名 + 当前保存位置交给调用方落盘；名为空不提交（OK 保持禁用态）。
    fn commit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.submitted {
            return false;
        }
        let name = self.name.read(cx).value().trim().to_string();
        if name.is_empty() {
            return false;
        }
        let Some(conn) = self.conns.get(self.conn_ix) else {
            return false;
        };
        let connection = conn.name.clone();
        let database = self.current_db();
        if database.is_empty() {
            return false;
        }
        let schema = self.current_schema();
        self.submitted = true;
        let parent = self.parent.clone();
        let on_ok = &self.on_ok;
        parent.update(cx, |app, cx| {
            on_ok(
                app,
                SaveChoice {
                    name,
                    connection,
                    database,
                    schema,
                },
            );
            cx.notify();
        });
        true
    }

    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.commit(cx) {
            window.remove_window();
        }
    }

    /// 基础高度：收起 187，展开 321。
    fn base_h(&self) -> f32 {
        if self.open { WIN_H_OPEN } else { WIN_H }
    }

    /// 窗口只有收起 / 展开两个高度，浮层不参与布局。
    fn resize_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let h = self.base_h();
        if ui::fit_dialog_client(window, (px(WIN_W), px(h))) {
            // 原生窗口已经改完尺寸，但 gpui 视口还停在旧高度：真机上展开态正文会
            // 继续按收起高度排版（目录 / 模式 / 按钮被裁掉），这里同步一次视口
            window.bounds_changed(cx);
            return;
        }
        if (h - window.viewport_size().height.as_f32()).abs() < 0.5 {
            return;
        }
        window.resize(size(px(WIN_W), px(h)));
        // 平台 resize 回调是异步的，先同步一次视口，避免首帧仍按旧高度排版（测试环境也没有回调）
        window.bounds_changed(cx);
    }

    /// 展开 / 收起三行下拉：窗口跟着长高或缩回。
    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        self.combo = None;
        self.needle.clear();
        self.active = None;
        self.close_popup(cx);
        self.resize_base(window, cx);
        if !open {
            self.name.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    /// 收起浮层窗口（如果开着）。
    fn close_popup(&mut self, cx: &mut Context<Self>) {
        self.popup_entity = None;
        let Some(handle) = self.popup.take() else {
            return;
        };
        let _ = cx.update_window(handle, |_, window, _| window.remove_window());
    }

    fn close_combo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.combo = None;
        self.needle.clear();
        self.active = None;
        self.close_popup(cx);
        self.name.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn toggle_combo(&mut self, combo: Combo, window: &mut Window, cx: &mut Context<Self>) {
        if self.combo == Some(combo) {
            self.close_combo(window, cx);
        } else {
            self.open_popup(combo, window, cx);
        }
    }

    /// 组合框当前值（打开时回填到筛选输入框）。
    fn label_of(&self, combo: Combo) -> String {
        match combo {
            Combo::Conn => self
                .conns
                .get(self.conn_ix)
                .map(|c| c.name.clone())
                .unwrap_or_default(),
            Combo::Db => self.current_db(),
            Combo::Schema => self.current_schema(),
        }
    }

    /// 组合框的完整候选清单（连接名 / 库名 / 模式名）。
    fn popup_rows(&self, combo: Combo) -> Vec<PopupRow> {
        let icon = match combo {
            Combo::Conn => None,
            Combo::Db => Some(PopupIcon::Db),
            Combo::Schema => Some(PopupIcon::Schema),
        };
        let names: Vec<(usize, String)> = match combo {
            Combo::Conn => self
                .conns
                .iter()
                .enumerate()
                .map(|(i, conn)| (i, conn.name.clone()))
                .collect(),
            Combo::Db => self.dbs().into_iter().enumerate().collect(),
            Combo::Schema => self.schemas().into_iter().enumerate().collect(),
        };
        names
            .into_iter()
            .map(|(index, text)| PopupRow {
                index,
                icon: match combo {
                    Combo::Conn => self
                        .conns
                        .get(index)
                        .map(|c| PopupIcon::Conn(c.kind)),
                    _ => icon,
                },
                text,
            })
            .collect()
    }

    /// 按筛选词过滤候选（大小写不敏感的子串匹配）。
    fn filtered_rows(&self, combo: Combo) -> Vec<PopupRow> {
        let rows = self.popup_rows(combo);
        if self.needle.is_empty() {
            return rows;
        }
        rows.into_iter()
            .filter(|row| contains_ci(&row.text, &self.needle))
            .collect()
    }

    /// 把筛选后的行与高亮推给浮层；行数变了就同步窗口高度。
    fn refresh_popup(&mut self, combo: Combo, resize: bool, cx: &mut Context<Self>) {
        let rows = self.filtered_rows(combo);
        let n = rows.len();
        if self.active.is_some_and(|ix| ix >= n) {
            self.active = None;
        }
        let active = self.active;
        let needle = self.needle.clone();
        if let Some(popup) = self.popup_entity.clone() {
            popup.update(cx, |popup, cx| {
                if resize {
                    popup.rows = rows;
                }
                popup.active = active;
                popup.needle = needle;
                cx.notify();
            });
        }
        if resize && let Some(handle) = self.popup {
            let h = popup_height(n);
            let _ = cx.update_window(handle, |_, window, cx| {
                window.resize(size(px(WIN_W - 2. * MARGIN_X), px(h)));
                // 平台 resize 回调是异步的，先同步一次视口（测试环境也没有回调）
                window.bounds_changed(cx);
            });
        }
    }

    /// 方向键在筛选后的候选里移动高亮；还没高亮时从首 / 尾进入。
    fn move_active(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(combo) = self.combo else {
            return;
        };
        let n = self.filtered_rows(combo).len();
        if n == 0 {
            return;
        }
        self.active = Some(match (self.active, down) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1).min(n - 1),
            (Some(i), false) => i.saturating_sub(1),
        });
        self.refresh_popup(combo, false, cx);
        cx.notify();
    }

    /// 回车确认：高亮项优先，否则取筛选后的第一项；空列表就只收起。
    fn pick_active(&mut self, cx: &mut Context<Self>) {
        let Some(combo) = self.combo else {
            return;
        };
        let names: Vec<String> = self
            .filtered_rows(combo)
            .into_iter()
            .map(|row| row.text)
            .collect();
        let name = self.active.and_then(|ix| names.get(ix).cloned()).or_else(|| {
            if self.active.is_none() {
                names.first().cloned()
            } else {
                None
            }
        });
        if let Some(name) = name {
            self.choose(combo, &name, cx);
        }
    }

    /// 在下拉框正下方另开一个无边框置顶窗口放列表：列表可以盖到对话框外面，
    /// 对话框本身的尺寸始终不变。
    fn open_popup(&mut self, combo: Combo, window: &mut Window, cx: &mut Context<Self>) {
        self.close_popup(cx);
        let rows = self.popup_rows(combo);
        let cur = match combo {
            Combo::Conn => self.conn_ix,
            Combo::Db => self.db_ix,
            Combo::Schema => self.schema_ix,
        }
        .min(rows.len().saturating_sub(1));
        self.needle.clear();
        self.active = if rows.is_empty() { None } else { Some(cur) };
        let origin = window.window_bounds().get_bounds().origin
            + point(px(MARGIN_X), px(combo.menu_top()));
        let popup_size = size(px(WIN_W - 2. * MARGIN_X), px(popup_height(rows.len())));

        let mut options = WindowOptions::default();
        options.window_bounds = Some(WindowBounds::Windowed(Bounds::new(origin, popup_size)));
        // 无标题栏、无最小尺寸；PopUp 在 Windows 上是置顶工具窗，不占任务栏
        options.titlebar = None;
        options.kind = WindowKind::PopUp;
        // 不抢激活：焦点要留在对话框的筛选输入框上，否则键入不进过滤器
        options.focus = false;
        options.is_resizable = false;
        options.is_movable = false;
        options.is_minimizable = false;

        let owner = cx.entity().downgrade();
        let dark = self.dark;
        let active = self.active;
        let dialog_win = self.win;
        let name = self.name.clone();
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                ComboPopup::new(
                    owner, combo, dark, rows, active, String::new(), dialog_win, name, window, cx,
                )
            })
        });
        let Ok((handle, popup)) = opened else {
            eprintln!("failed to open save-as popup");
            return;
        };
        self.combo = Some(combo);
        self.popup = Some(handle);
        self.popup_entity = Some(popup.clone());

        // 建窗的 bounds 被平台按外框解释，客户区会偏左上半圈边框；按锚窗口客户区
        // 实测搬一次，浮层才正好贴在触发器下沿、与输入框左对齐
        let _ = cx.update_window(handle, |_, popup_win, _| {
            ui::place_client_at_anchor(popup_win, window, (MARGIN_X, combo.menu_top()));
            // Win11 默认圆顶层窗口四角，下拉列表要是方角
            ui::square_corners(popup_win);
        });

        cx.observe_release(&popup, move |this, _popup, cx| {
            // 浮层自己关掉（点外面 / Esc / 选中）：收回状态，焦点还给名称框
            if this.popup != Some(handle) {
                return;
            }
            this.popup = None;
            this.popup_entity = None;
            this.combo = None;
            this.needle.clear();
            this.active = None;
            let name = this.name.clone();
            let _ = cx.update_window(dialog_win, |_, window, cx| {
                if window.is_window_active() {
                    name.update(cx, |state, cx| state.focus(window, cx));
                }
            });
            cx.notify();
        })
        .detach();
        // 回填当前值并全选：键入即替换；焦点留在对话框里的筛选输入框上
        let label = self.label_of(combo);
        self.filter.update(cx, |state, cx| {
            state.set_value(label, window, cx);
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        cx.notify();
    }

    /// 浮层里选中一项：更新归属；浮层窗口由它自己关闭。
    fn choose(&mut self, combo: Combo, name: &str, cx: &mut Context<Self>) {
        match combo {
            Combo::Conn => {
                if let Some(ix) = self.conns.iter().position(|c| c.name == name)
                    && ix != self.conn_ix
                {
                    self.conn_ix = ix;
                    self.db_ix = 0;
                    self.schema_ix = 0;
                    // 选回初始连接时贴回当初的库 / 模式
                    let back_home = self.conns[ix].name == self.init_conn;
                    if back_home {
                        let db = self.dbs().iter().position(|d| *d == self.init_db);
                        self.db_ix = db.unwrap_or(0);
                        if db.is_some() {
                            let sc = self.schemas().iter().position(|s| *s == self.init_schema);
                            self.schema_ix = sc.unwrap_or(0);
                        }
                    }
                }
            }
            Combo::Db => {
                if let Some(ix) = self.dbs().iter().position(|d| d == name)
                    && ix != self.db_ix
                {
                    self.db_ix = ix;
                    self.schema_ix = 0;
                    let here = self.conns.get(self.conn_ix).map(|c| c.name.clone());
                    if here.as_deref() == Some(self.init_conn.as_str())
                        && self.current_db() == self.init_db
                    {
                        let sc = self.schemas().iter().position(|s| *s == self.init_schema);
                        self.schema_ix = sc.unwrap_or(0);
                    }
                }
            }
            Combo::Schema => {
                if let Some(ix) = self.schemas().iter().position(|s| s == name) {
                    self.schema_ix = ix;
                }
            }
        }
        self.combo = None;
        self.needle.clear();
        self.active = None;
        self.popup_entity = None;
        cx.notify();
    }

    /// 自绘标题栏：左标题、右 ×，标题区交给系统当拖动区。
    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.skin();
        h_flex()
            .id("saveas-bar")
            .test_support()
            .h(px(BAR_H))
            .flex_none()
            .w_full()
            .items_center()
            .bg(to_hsla(s.bar))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = true),
            )
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.dragging = false))
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if this.dragging {
                    this.dragging = false;
                    window.start_window_move();
                }
            }))
            .child(
                h_flex()
                    .h_full()
                    .flex_1()
                    .items_center()
                    .pl(px(9.))
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        div()
                            .text_xs()
                            .line_height(px(LABEL_H))
                            .text_color(to_hsla(s.text))
                            .child(self.title),
                    ),
            )
            .child(
                div()
                    .id("saveas-close")
                    .test_support()
                    .w(px(30.))
                    .h(px(BAR_H))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|el| el.bg(to_hsla(s.hover)))
                    .occlude()
                    .on_click(|_, window, _| window.remove_window())
                    .child(
                        Icon::new(ZedIcon::Close)
                            .with_size(px(12.))
                            .text_color(to_hsla(s.text)),
                    ),
            )
    }

    /// 字段标签：12px，行盒在控件上方。
    fn label(&self, id: &'static str, text: &'static str, mt: f32) -> impl IntoElement {
        let s = self.skin();
        div()
            .id(id)
            .test_support()
            .w_full()
            .flex_none()
            .h(px(LABEL_H))
            .mt(px(mt))
            .text_xs()
            .line_height(px(LABEL_H))
            .text_color(to_hsla(s.text))
            .child(text)
    }

    /// 查询名称：白底 + 上/左/右 1px 灰边 + 底部 1px 蓝色下划线。
    fn name_field(&self) -> impl IntoElement {
        let s = self.skin();
        v_flex()
            .w_full()
            .flex_none()
            .mt(px(LABEL_TO_INPUT))
            .child(
                div()
                    .id("saveas-name-field")
                    .test_support()
                    .w_full()
                    .h(px(INPUT_H))
                    .border_t_1()
                    .border_l_1()
                    .border_r_1()
                    .border_color(to_hsla(s.field_edge))
                    .bg(to_hsla(s.field))
                    // 截图里文字贴左边框：Input 自带的 10px 内边距要抹掉；
                    // 行高也必须压到框内，否则单行文本按 1.25rem 排版会下溢出去
                    .child(
                        Input::new(&self.name)
                            .id("saveas-name-input")
                            .appearance(false)
                            .text_xs()
                            .line_height(px(INPUT_H - 2.))
                            .max_h(px(INPUT_H - 2.))
                            .px(px(0.))
                            .py(px(0.)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .h(px(UNDERLINE_H))
                    .flex_none()
                    .bg(to_hsla(s.field_edge)),
            )
    }

    /// 保存位置：左侧「连接/库」文本，右侧 20×20 箭头按钮，整行可点。
    fn location_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.skin();
        let value = match self.conns.get(self.conn_ix) {
            Some(conn) => format!("{}/{}", conn.name, self.current_db()),
            None => "（无可选位置）".to_string(),
        };
        let open = self.open;
        h_flex()
            .id("saveas-loc")
            .test_support()
            .w_full()
            .h(px(LOC_H))
            .flex_none()
            .mt(px(LABEL_TO_LOC))
            .items_center()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.set_open(!open, window, cx);
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .line_height(px(LOC_H))
                    .text_color(to_hsla(s.text))
                    .overflow_hidden()
                    .child(value),
            )
            .child(
                div()
                    .id("saveas-loc-arrow")
                    .test_support()
                    .w(px(LOC_H))
                    .h(px(LOC_H))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(to_hsla(s.btn))
                    .border_1()
                    .border_color(to_hsla(s.btn_edge))
                    .occlude()
                    .hover(|el| el.bg(to_hsla(s.hover)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_open(!open, window, cx);
                    }))
                    .child(
                        Icon::new(if open {
                            ZedIcon::ChevronUp
                        } else {
                            ZedIcon::ChevronDown
                        })
                        .with_size(px(12.))
                        .text_color(to_hsla(s.text)),
                    ),
            )
    }

    /// 展开后的一行：标签 + 带图标与箭头的下拉框。
    fn combo_block(&self, kind: Combo, cx: &mut Context<Self>) -> AnyElement {
        let s = self.skin();
        let (id, label_id, label, mt) = match kind {
            Combo::Conn => ("saveas-conn", "saveas-conn-label", "连接:", COMBO_FIRST_GAP),
            Combo::Db => ("saveas-db", "saveas-db-label", "目录:", COMBO_BLOCK_GAP),
            Combo::Schema => (
                "saveas-schema",
                "saveas-schema-label",
                "模式:",
                COMBO_BLOCK_GAP,
            ),
        };
        let (icon, text) = match kind {
            Combo::Conn => {
                let conn = self.conns.get(self.conn_ix);
                (
                    conn.map(|c| db_icons::engine_bar(c.kind, 16.)),
                    conn.map(|c| c.name.clone()).unwrap_or_default(),
                )
            }
            Combo::Db => {
                let db = self.current_db();
                let icon = if db.is_empty() {
                    None
                } else {
                    Some(db_icons::database(16.))
                };
                (icon, db)
            }
            Combo::Schema => {
                let schema = self.current_schema();
                let icon = if schema.is_empty() {
                    None
                } else {
                    Some(art::icon(Glyph::Schema, 16., to_hsla(s.muted)))
                };
                (icon, schema)
            }
        };
        // 与查询窗口的运行上下文下拉同一个组件：打开时正文变筛选输入框，
        // 键入即过滤候选，回车取高亮项；浮层仍是单独的窗口。
        let expanded = self.combo == Some(kind);
        let trigger = combo::trigger(
            combo::TriggerSpec {
                id: id.to_string(),
                width: None,
                icon: icon.unwrap_or_else(|| div().into_any_element()),
                label: text,
                open: expanded,
                input: Some(self.filter.clone()),
            },
            combo_colors(self.dark),
            cx,
            // 已展开时点正文是给输入框落光标，不再收起；箭头负责开 / 关
            move |this, window, cx| {
                if !expanded {
                    this.toggle_combo(kind, window, cx);
                }
            },
            move |this, window, cx| {
                if expanded {
                    this.close_combo(window, cx);
                } else {
                    this.toggle_combo(kind, window, cx);
                }
                cx.stop_propagation();
            },
            move |this, ev: &KeyDownEvent, window, cx| match ev.keystroke.key.as_str() {
                "escape" => {
                    this.close_combo(window, cx);
                    cx.stop_propagation();
                }
                "enter" => {
                    this.pick_active(cx);
                    this.close_combo(window, cx);
                    cx.stop_propagation();
                }
                "up" => {
                    this.move_active(false, cx);
                    cx.stop_propagation();
                }
                "down" => {
                    this.move_active(true, cx);
                    cx.stop_propagation();
                }
                _ => {}
            },
        );
        // 点对话框里别处（名称框 / 按钮 / 另一行下拉）就收起浮层；
        // 点浮层本身在另一个窗口里，不会触发这里。
        v_flex()
            .w_full()
            .flex_none()
            .child(self.label(label_id, label, mt))
            .child(
                div()
                    .id(format!("{id}-wrap"))
                    .mt(px(COMBO_LABEL_GAP))
                    .on_mouse_down_out(cx.listener(move |this, _, window, cx| {
                        if this.combo == Some(kind) {
                            this.close_combo(window, cx);
                        }
                    }))
                    .child(trigger),
            )
            .into_any_element()
    }

    /// 底部居中成对的 OK / Cancel：OK 实心强调色，Cancel 面板底。
    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_submit = !self.name.read(cx).value().trim().is_empty();
        h_flex()
            .w_full()
            .flex_none()
            .pb(px(PAD_BOTTOM))
            .justify_center()
            .gap(px(BTN_GAP))
            .child(self.text_button(
                "saveas-ok",
                "OK",
                true,
                can_submit,
                cx,
                |this, window, cx| this.accept(window, cx),
            ))
            .child(self.text_button(
                "saveas-cancel",
                "Cancel",
                false,
                true,
                cx,
                |_, window, _| window.remove_window(),
            ))
    }

    fn text_button(
        &self,
        id: &'static str,
        label: &'static str,
        primary: bool,
        enabled: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let s = self.skin();
        div()
            .id(id)
            .test_support()
            .w(px(BTN_W))
            .h(px(BTN_H))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(ui::RADIUS))
            .when(primary && enabled, |el| {
                el.bg(to_hsla(s.accent)).text_color(to_hsla(s.on_accent))
            })
            // 禁用态不能整块降透明度：白字叠在半透明蓝底上会一起洗掉，
            // 换成灰面 + muted 字，跟其他对话框的主按钮禁用态一致
            .when(primary && !enabled, |el| {
                el.bg(to_hsla(s.btn)).text_color(to_hsla(s.muted))
            })
            .when(!primary, |el| {
                el.bg(to_hsla(s.btn)).text_color(if enabled {
                    to_hsla(s.text)
                } else {
                    to_hsla(s.muted)
                })
            })
            .text_xs()
            .when(enabled, |el| {
                el.cursor_pointer().hover(|el| el.bg(to_hsla(s.hover)))
            })
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| {
                if enabled {
                    on_click(this, window, cx);
                }
            }))
            .into_any_element()
    }
}

impl Render for SaveAsDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.skin();
        let expanded = self.open;

        v_flex()
            .id("saveas-root")
            .test_support()
            .relative()
            .size_full()
            .bg(to_hsla(s.body))
            .text_color(to_hsla(s.text))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                let m = ev.keystroke.modifiers;
                let plain = !m.control && !m.alt && !m.platform;
                if ev.keystroke.key == "enter" && plain {
                    if this.combo.is_some() {
                        this.pick_active(cx);
                        this.close_combo(window, cx);
                    } else {
                        this.accept(window, cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .child(self.title_bar(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(MARGIN_X))
                    .pt(px(PAD_TOP))
                    .child(self.label("saveas-name-label", "查询名称:", 0.))
                    .child(self.name_field())
                    .child(self.label("saveas-loc-label", "保存位置:", INPUT_TO_LABEL))
                    .child(self.location_row(cx))
                    .when(expanded, |body| {
                        body.child(self.combo_block(Combo::Conn, cx))
                            .child(self.combo_block(Combo::Db, cx))
                            .child(self.combo_block(Combo::Schema, cx))
                    })
                    .child(div().flex_1())
                    .child(self.footer(cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::DbKind;
    use crate::saved_queries::SavedQuery;
    use crate::ui_test_support::{isolate_store, open_app};
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt;

    // 显式引回标准 test 宏：`use super::*` 会把 gpui 的同名 test 宏带进作用域，
    // 不然 #[gpui_kit::test] 生成的内部 #[test] 会再次命中 gpui 宏 → 无限递归
    use core::prelude::v1::test;

    fn init() -> SaveInit {
        SaveInit {
            conns: vec![
                Connection::new(
                    "数从",
                    DbKind::MySql,
                    "127.0.0.1",
                    3306,
                    "bx11",
                    "root",
                    "",
                    "开发环境",
                    true,
                )
                .as_demo(),
                Connection::new(
                    "测试 PG",
                    DbKind::PostgreSql,
                    "127.0.0.1",
                    5432,
                    "daro_dev",
                    "postgres",
                    "",
                    "测试环境",
                    false,
                )
                .as_demo(),
            ],
            connection: "数从".into(),
            database: "bx11".into(),
            schema: String::new(),
        }
    }

    /// 打开对话框并返回它的窗口句柄（主窗口之外多出来的那一个）。
    fn open_dialog(cx: &mut TestAppContext, app: &Entity<DaroApp>) -> AnyWindowHandle {
        let app = app.clone();
        cx.update(|cx| {
            open(
                app,
                false,
                "另存为",
                "查询-1".into(),
                init(),
                move |a, choice| {
                    a.save_named_query(SavedQuery {
                        name: choice.name,
                        connection: choice.connection,
                        database: choice.database,
                        schema: choice.schema,
                        sql: "select 1".into(),
                    });
                },
                cx,
            );
            let windows = cx.windows();
            assert_eq!(windows.len(), 2, "另存为窗口已打开");
            *windows.last().unwrap()
        })
    }

    /// 刚打开的下拉浮层：主窗口与对话框之外的第三个窗口。
    fn popup_window(cx: &mut TestAppContext) -> AnyWindowHandle {
        let windows = cx.windows();
        assert_eq!(windows.len(), 3, "下拉浮层窗口已打开");
        *windows.last().unwrap()
    }

    /// 关键尺寸与参照截图（参考界面另存为，2× 截图折半后的逻辑像素）对齐：
    /// 收起态 187 高，展开后 321 高、三行下拉顶边 154 / 201 / 248。
    #[gpui_kit::test]
    fn save_as_layout_matches_reference_shot(cx: &mut TestAppContext) {
        isolate_store("saveas_geo");
        let (_main, app) = open_app(cx);
        let dlg = open_dialog(cx, &app);
        cx.update_window(dlg, |_, window, cx| {
            window.render_frame(cx);
            let near = |a: Pixels, b: f32| (a.as_f32() - b).abs() < 0.6;
            let root = window.find("saveas-root").bounds();

            assert!(near(root.size.height, WIN_H), "收起态高 187");
            assert!(
                window.try_find("saveas-conn").is_none(),
                "收起时不显示三行下拉"
            );

            let ok = window.find("saveas-ok").bounds();
            let cancel = window.find("saveas-cancel").bounds();
            assert!(
                near(ok.size.width, BTN_W) && near(cancel.size.width, BTN_W),
                "按钮宽 74"
            );
            assert!(
                near(ok.size.height, BTN_H) && near(cancel.size.height, BTN_H),
                "按钮高 24"
            );
            assert!(
                near(cancel.origin.x - ok.origin.x, BTN_W + BTN_GAP),
                "OK 与 Cancel 间隔 6"
            );
            assert!(
                near((ok.origin.x + cancel.origin.x + cancel.size.width) / 2., WIN_W / 2.),
                "按钮成对水平居中"
            );

            let close = window.find("saveas-close").bounds();
            assert!(
                near(close.origin.x + close.size.width, (root.origin.x + root.size.width).as_f32()),
                "关闭按钮贴右"
            );
            assert!(near(close.origin.y, root.origin.y.as_f32()), "关闭按钮在标题栏内");

            let field = window.find("saveas-name-field").bounds();
            let frame = window.find("saveas-name-input").bounds();
            let text = window.find("input-state").bounds();
            assert!(
                near(frame.origin.y, (field.origin.y + px(1.)).as_f32()),
                "文字行贴字段上沿"
            );
            assert!(near(frame.size.height, INPUT_H - 2.), "文字行高 16");
            assert!(
                text.origin.y >= field.origin.y && text.origin.y + text.size.height <= field.origin.y + field.size.height,
                "文字不越过下划线"
            );
            let loc = window.find("saveas-loc").bounds();
            assert!(near(field.origin.x, MARGIN_X), "输入框左边距 18");
            assert!(
                near(field.size.width, WIN_W - 2. * MARGIN_X),
                "输入框占满左右留白"
            );
            assert!(
                near(field.origin.y, BAR_H + PAD_TOP + LABEL_H + LABEL_TO_INPUT),
                "输入框紧贴标签"
            );
            assert!(near(loc.size.height, LOC_H), "位置行 20 高");
            assert!(
                near(
                    window.find("saveas-loc-arrow").bounds().origin.x + px(LOC_H),
                    WIN_W - MARGIN_X
                ),
                "箭头按钮贴右留白"
            );

            window.click("saveas-loc", cx);
            window.render_frame(cx);
            let root = window.find("saveas-root").bounds();
            assert!(near(root.size.height, WIN_H_OPEN), "展开态高 321");
            let conn = window.find("saveas-conn").bounds();
            let db = window.find("saveas-db").bounds();
            let schema = window.find("saveas-schema").bounds();
            assert!(near(conn.size.height, COMBO_H), "下拉框与查询窗口同高 22");
            assert!(near(conn.origin.y, COMBO1_TOP), "连接行顶边 150");
            assert!(near(db.origin.y, COMBO1_TOP + COMBO_PITCH), "目录行顶边 197");
            assert!(
                near(schema.origin.y, COMBO1_TOP + 2. * COMBO_PITCH),
                "模式行顶边 244"
            );
            assert!(
                near(conn.size.height, 22.),
                "与查询窗口的运行上下文下拉同高"
            );
            assert!(
                near(
                    window.find("saveas-db-label").bounds().origin.y
                        + px(LABEL_H + COMBO_LABEL_GAP),
                    db.origin.y.as_f32()
                ),
                "目录标签紧贴下拉"
            );

            let ok = window.find("saveas-ok").bounds();
            assert!(
                near(
                    root.origin.y + root.size.height - ok.origin.y - ok.size.height,
                    PAD_BOTTOM
                ),
                "展开后按钮仍贴底留白 17"
            );
        })
        .unwrap();
    }

    /// 下拉列表做成独立浮层窗口：列表长度不受对话框高度限制，对话框尺寸不变。
    #[gpui_kit::test]
    fn save_as_dropdown_floats_outside_the_dialog(cx: &mut TestAppContext) {
        isolate_store("saveas_popup");
        let (_main, app) = open_app(cx);
        let dlg = open_dialog(cx, &app);
        cx.update_window(dlg, |_, window, cx| {
            window.render_frame(cx);
            window.click("saveas-loc", cx);
            window.render_frame(cx);
            let root = window.find("saveas-root").bounds();
            assert!(
                (root.size.height - px(WIN_H_OPEN)).as_f32().abs() < 0.6,
                "展开态高 321"
            );
            window.click("saveas-db", cx);
        })
        .unwrap();

        // 浮层是第 3 个窗口：演示 MySQL 连接共 6 个库，列表按 6 行出高
        let popup = popup_window(cx);
        let dlg_origin = cx
            .update_window(dlg, |_, window, _| window.window_bounds().get_bounds())
            .unwrap()
            .origin;
        let popup_bounds = cx
            .update_window(popup, |_, window, _| window.window_bounds().get_bounds())
            .unwrap();
        let dx = (popup_bounds.origin.x - dlg_origin.x - px(MARGIN_X)).as_f32();
        let dy = (popup_bounds.origin.y - dlg_origin.y - px(Combo::Db.menu_top())).as_f32();
        assert!(
            dx.abs() < 0.6 && dy.abs() < 0.6,
            "浮层贴在目录框下沿、与输入框左对齐"
        );
        assert!(
            (popup_bounds.origin.y + popup_bounds.size.height)
                > (dlg_origin.y + px(WIN_H_OPEN)),
            "浮层伸出对话框下沿"
        );
        cx.update_window(popup, |_, window, cx| {
            window.render_frame(cx);
            let list = window.find("saveas-popup").bounds();
            assert!(
                (list.size.height - px(6. * MENU_ROW_H + 2. * MENU_PAD_Y + 1.))
                    .as_f32()
                    .abs()
                    < 0.6,
                "浮层按行数出高，不截断"
            );
            let last = window.find("saveas-popup/5").bounds();
            assert!(
                last.origin.y + last.size.height <= list.origin.y + list.size.height,
                "最后一行完整可见"
            );
            // 行样式跟查询窗口的自绘下拉对齐：22 高、首行距上沿 4（外加 0.5 边框）
            let first = window.find("saveas-popup/0").bounds();
            assert!(
                (first.size.height - px(MENU_ROW_H)).as_f32().abs() < 0.6,
                "行高与查询窗口一致"
            );
            assert!(
                (first.origin.y - list.origin.y - px(MENU_PAD_Y + 0.5)).as_f32().abs() < 0.6,
                "首行上留白 4"
            );
            window.click("saveas-popup/2", cx);
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 2, "选中后浮层窗口关闭");
        cx.update_window(dlg, |_, window, cx| {
            window.render_frame(cx);
            let root = window.find("saveas-root").bounds();
            assert!(
                (root.size.height - px(WIN_H_OPEN)).as_f32().abs() < 0.6,
                "对话框没被撑高"
            );
            assert!(window.try_find("saveas-menu").is_none(), "列表不在对话框里");
            window.click("saveas-db", cx);
        })
        .unwrap();

        // Esc 只收浮层，对话框还留在展开态（焦点在对话框的筛选输入框上）
        popup_window(cx);
        cx.update_window(dlg, |_, window, cx| {
            window.render_frame(cx);
            window.press("escape", cx);
        })
        .unwrap();
        assert_eq!(cx.windows().len(), 2, "Esc 只收起浮层");
        cx.update_window(dlg, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                (window.find("saveas-root").bounds().size.height - px(WIN_H_OPEN))
                    .as_f32()
                    .abs()
                    < 0.6,
                "对话框仍是展开态"
            );
        })
        .unwrap();
    }

    /// 与查询工具栏的组合框一致：打开时正文变输入框并全选当前值，键入即过滤
    /// 候选（浮层跟着缩短），回车取筛选后的首项。
    #[gpui_kit::test]
    fn save_as_combo_filters_typing_and_enter(cx: &mut TestAppContext) {
        isolate_store("saveas_filter");
        let (_main, app) = open_app(cx);
        let dlg = open_dialog(cx, &app);
        cx.update_window(dlg, |_, window, cx| {
            window.render_frame(cx);
            window.click("saveas-loc", cx);
            window.render_frame(cx);
            window.click("saveas-db", cx);
        })
        .unwrap();

        // 演示 MySQL 连接 6 个库；打开时列出全部候选，当前值 bx11 高亮但不算筛选词
        let popup = popup_window(cx);
        cx.update_window(popup, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.try_find("saveas-popup/1").is_some(),
                "打开时列出全部候选"
            );
        })
        .unwrap();

        // 全选态直接键入 = 替换，只剩子串命中的 mysql
        cx.update_window(dlg, |_, window, cx| {
            window.render_frame(cx);
            window.input("mysql", cx);
        })
        .unwrap();

        // Change 订阅走 effect 队列，冲刷发生在 update 边界
        cx.update_window(popup, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.try_find("saveas-popup/3").is_some(),
                "mysql 命中保留"
            );
            assert!(window.try_find("saveas-popup/0").is_none(), "bx11 被筛掉");
            assert!(
                window.try_find("saveas-popup/2").is_none(),
                "information_schema 被筛掉"
            );
            let list = window.find("saveas-popup").bounds();
            assert!(
                (list.size.height - px(popup_height(1))).as_f32().abs() < 0.6,
                "浮层按筛选后的行数出高"
            );
        })
        .unwrap();

        // 回车取筛选后的首项并收起浮层
        cx.update_window(dlg, |_, window, cx| {
            window.press("enter", cx);
            window.render_frame(cx);
        })
        .unwrap();
        assert_eq!(cx.windows().len(), 2, "回车后浮层关闭");

        cx.update_window(dlg, |_, window, cx| {
            window.render_frame(cx);
            window.click("saveas-ok", cx);
        })
        .unwrap();
        assert_eq!(cx.windows().len(), 1, "OK 后对话框关闭");
        cx.update(|cx| {
            let qs = &app.read(cx).saved_queries;
            assert_eq!(qs.len(), 1, "落盘一条已保存查询");
            assert_eq!(qs[0].database, "mysql", "回车回填筛选出的 mysql");
        });
    }

    /// 展开 → 换连接 → 换库 → OK：落盘归属跟着三行下拉走，窗口收起。
    #[gpui_kit::test]
    fn save_as_writes_chosen_location(cx: &mut TestAppContext) {
        isolate_store("saveas_ok");
        let (_main, app) = open_app(cx);
        let dlg = open_dialog(cx, &app);
        cx.update_window(dlg, |_, window, cx| {
            window.render_frame(cx);
            window.click("saveas-loc-arrow", cx);
            window.render_frame(cx);
            window.click("saveas-conn", cx);
        })
        .unwrap();
        let popup = popup_window(cx);
        cx.update_window(popup, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("saveas-popup/1").is_some(), "连接列表在浮层里");
            window.click("saveas-popup/1", cx);
        })
        .unwrap();
        assert_eq!(cx.windows().len(), 2, "选中连接后浮层关闭");

        cx.update_window(dlg, |_, window, cx| {
            window.render_frame(cx);
            window.click("saveas-db", cx);
        })
        .unwrap();
        let popup = popup_window(cx);
        cx.update_window(popup, |_, window, cx| {
            window.render_frame(cx);
            window.click("saveas-popup/1", cx);
        })
        .unwrap();

        cx.update_window(dlg, |_, window, cx| {
            window.render_frame(cx);
            window.click("saveas-ok", cx);
        })
        .unwrap();
        assert_eq!(cx.windows().len(), 1, "OK 后对话框关闭");
        cx.update(|cx| {
            let qs = &app.read(cx).saved_queries;
            assert_eq!(qs.len(), 1, "落盘一条已保存查询");
            assert_eq!(qs[0].name, "查询-1", "预填名直接沿用");
            assert_eq!(qs[0].connection, "测试 PG", "连接跟着下拉改写");
            assert_eq!(qs[0].database, "datawarehouse", "目录跟着下拉改写");
            assert_eq!(qs[0].schema, "public", "模式按新库取默认");
        });
    }
}
