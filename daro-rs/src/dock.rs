//! 主工作区多窗格分栏（dock）。
//!
//! 参考界面的「窗口」菜单里有层叠 / 水平平铺 / 垂直平铺；daro-rs 用页签条承载
//! 多个对象页，这里把「一个页签条 + 一块内容」抽象成窗格（[`Pane`]），窗格之间
//! 用二叉分栏树（[`Layout`]）排布：
//!
//! ```text
//! Split(Row)                      ┌──────────┬──────────┐
//! ├─ Leaf(home)         ＝＝>      │  home    │  新窗格  │
//! └─ Leaf(other)                  └──────────┴──────────┘
//! ```
//!
//! 交互口径（照 IDE 的通用习惯，参考界面本身没有可拖的页签，这一层是 daro-rs
//! 自己的扩展）：
//!
//! - 拖页签时，指针所在窗格按「左 / 右 / 上 / 下 / 中间」五区高亮提示落点；
//! - 落在左/右/上/下 → 以该窗格为基准新开一栏（上/下是上下分栏）；
//! - 落在中间 → 页签并入该窗格；
//! - 分隔条可拖动改比例（用组件库的 resizable，见 [`crate::DaroApp::render_node`]）。
//!
//! 窗格用 `home` 标记第一块：它承载常驻的「对象」页（`active == 0`），且永不因
//! 拖空页签而消失；其它窗格空了就塌掉、由分栏树把兄弟节点提上来。

use crate::zed_icons::ZedIcon;
use crate::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::*;

/// 拖放落点五区。`Center` = 并入目标窗格，其余 = 朝对应方向新开一栏。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropZone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

impl DropZone {
    /// 落在左/上时新窗格排在前（first），右/下排在后。
    fn leading(self) -> bool {
        matches!(self, DropZone::Left | DropZone::Top)
    }
}

/// 分栏方向：`Row` = 左右并排；`Col` = 上下叠放。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplitAxis {
    Row,
    Col,
}

/// 分栏树。叶子是窗格 id，内部节点一分为二 —— 只存结构，比例交给
/// resizable 组件自己的 keyed state 记住（拖动时不重排结构，不需要回写）。
#[derive(Clone, Debug)]
pub enum Layout {
    Leaf(u64),
    Split {
        axis: SplitAxis,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}

/// 一块窗格：页签顺序 + 当前页。`active` 沿用 `DaroApp::active_tab` 的口径
/// （0 = 对象页，n = `opened[n - 1]`），两个字段永远同步在聚焦窗格上。
pub struct Pane {
    pub id: u64,
    pub tabs: Vec<usize>,
    pub active: usize,
    /// 页签条横向滚动句柄：溢出时右端箭头用它滚动，激活页签靠
    /// `scroll_to_item` 滚进视野（每个页签是滚动容器的直接子元素）。
    pub tab_scroll: ScrollHandle,
}

/// 分栏工作区的模型。纯状态，不碰渲染。
pub struct Dock {
    pub panes: Vec<Pane>,
    pub root: Layout,
    /// 当前聚焦的窗格 id（键盘、菜单动作、`active_tab` 都作用于它）
    pub focus: u64,
    /// 第一块窗格：常驻「对象」页，不因拖空而塌。
    pub home: u64,
    next_id: u64,
}

/// 页签拖拽时挂在 `cx.active_drag` 上的载荷。
#[derive(Clone, Copy)]
pub struct DragTab {
    /// 页签的 1 基下标（同 [`Pane::active`] 的口径）。
    pub tab: usize,
}

impl Dock {
    pub fn new() -> Self {
        let home = 1;
        Self {
            panes: vec![Pane {
                id: home,
                tabs: Vec::new(),
                active: 0,
                tab_scroll: ScrollHandle::new(),
            }],
            root: Layout::Leaf(home),
            focus: home,
            home,
            next_id: 2,
        }
    }

    pub fn pane(&self, id: u64) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }

    pub fn pane_mut(&mut self, id: u64) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|p| p.id == id)
    }

    pub fn focused(&self) -> &Pane {
        self.pane(self.focus).unwrap_or(&self.panes[0])
    }

    /// 聚焦窗格当前该显示的页（写回 `DaroApp::active_tab` 用）。
    pub fn focused_active(&self) -> usize {
        self.focused().active
    }

    pub fn focus_pane(&mut self, id: u64) {
        if self.pane(id).is_some() {
            self.focus = id;
        }
    }

    /// 页签落在哪个窗格里（1 基下标）。
    pub fn pane_of(&self, tab: usize) -> Option<u64> {
        self.panes
            .iter()
            .find(|p| p.tabs.contains(&tab))
            .map(|p| p.id)
    }

    /// 聚焦某页签所属窗格并激活它。
    pub fn focus_tab(&mut self, tab: usize) {
        if let Some(id) = self.pane_of(tab) {
            self.focus = id;
            if let Some(p) = self.pane_mut(id) {
                p.active = tab;
                if let Some(pos) = p.tabs.iter().position(|t| *t == tab) {
                    p.tab_scroll.scroll_to_item(pos);
                }
            }
        }
    }

    /// 聚焦 home 窗格并回到「对象」页。
    pub fn focus_home(&mut self) {
        self.focus = self.home;
        if let Some(p) = self.pane_mut(self.home) {
            p.active = 0;
        }
    }

    /// 新页签追加到聚焦窗格并激活。
    pub fn add_tab(&mut self, tab: usize) {
        let id = self.focus;
        if let Some(p) = self.pane_mut(id) {
            if !p.tabs.contains(&tab) {
                p.tabs.push(tab);
            }
            p.active = tab;
            if let Some(pos) = p.tabs.iter().position(|t| *t == tab) {
                p.tab_scroll.scroll_to_item(pos);
            }
        }
    }

    /// 关页签：从所有窗格摘掉，并把大于它的下标整体前移一位。
    /// `tab` 是 1 基下标；调用方随后用 [`Self::focused_active`] 回写 `active_tab`。
    pub fn remove_tab(&mut self, tab: usize) {
        let mut emptied: Vec<u64> = Vec::new();
        for i in 0..self.panes.len() {
            let (id, had, next_active, is_empty) = {
                let p = &mut self.panes[i];
                let Some(pos) = p.tabs.iter().position(|t| *t == tab) else {
                    // 不在这个窗格里：只需要把更大的下标前移
                    p.tabs.iter_mut().for_each(|t| {
                        if *t > tab {
                            *t -= 1;
                        }
                    });
                    if p.active > tab {
                        p.active -= 1;
                    }
                    continue;
                };
                p.tabs.remove(pos);
                p.tabs.iter_mut().for_each(|t| {
                    if *t > tab {
                        *t -= 1;
                    }
                });
                let next_active = if p.tabs.is_empty() {
                    0
                } else if p.active == tab {
                    p.tabs[pos.min(p.tabs.len() - 1)]
                } else if p.active > tab {
                    p.active - 1
                } else {
                    p.active
                };
                p.active = next_active;
                (p.id, true, next_active, p.tabs.is_empty())
            };
            let _ = (had, next_active);
            if is_empty && id != self.home {
                emptied.push(id);
            }
        }
        for id in emptied {
            self.remove_pane(id);
        }
    }

    /// 拖放落定：把页签挪进目标窗格（`Center`），或朝 `zone` 方向新开一栏。
    pub fn move_tab(&mut self, tab: usize, target: u64, zone: DropZone) -> bool {
        if self.pane(target).is_none() || self.pane_of(tab).is_none() {
            return false;
        }
        if zone == DropZone::Center {
            if let Some(src) = self.pane_of(tab) {
                self.detach_tab(tab);
                // 源窗格可能因此塌掉；目标窗格仍在（同窗格时已提前返回）
                if src == target && self.pane(target).is_none() {
                    return false;
                }
            }
            let Some(p) = self.pane_mut(target) else {
                return false;
            };
            if !p.tabs.contains(&tab) {
                p.tabs.push(tab);
            }
            p.active = tab;
            self.focus = target;
            return true;
        }

        let axis = match zone {
            DropZone::Left | DropZone::Right => SplitAxis::Row,
            DropZone::Top | DropZone::Bottom => SplitAxis::Col,
            DropZone::Center => unreachable!(),
        };
        let new_id = self.next_id;
        self.next_id += 1;
        if !Self::split_leaf(&mut self.root, target, axis, new_id, zone.leading()) {
            return false;
        }
        self.panes.push(Pane {
            id: new_id,
            tabs: vec![tab],
            active: tab,
            tab_scroll: ScrollHandle::new(),
        });
        // 从老窗格摘掉页签；老窗格拖空了（且不是 home）就让分栏树塌掉它
        if let Some(old) = self.pane_of(tab).filter(|id| *id != new_id) {
            let empty = if let Some(p) = self.pane_mut(old) {
                if let Some(pos) = p.tabs.iter().position(|t| *t == tab) {
                    p.tabs.remove(pos);
                }
                if p.active == tab {
                    p.active = if p.tabs.is_empty() {
                        0
                    } else {
                        p.tabs[0]
                    };
                }
                p.tabs.is_empty()
            } else {
                false
            };
            if empty && old != self.home {
                self.remove_pane(old);
            }
        }
        self.focus = new_id;
        true
    }

    /// 把页签从它所在的窗格摘掉（不移动下标；拖拽场景里 `opened` 不变）。
    fn detach_tab(&mut self, tab: usize) {
        let Some(id) = self.pane_of(tab) else { return };
        let mut empty = false;
        if let Some(p) = self.pane_mut(id) {
            if let Some(pos) = p.tabs.iter().position(|t| *t == tab) {
                p.tabs.remove(pos);
            }
            if p.active == tab {
                p.active = if p.tabs.is_empty() {
                    0
                } else {
                    p.tabs[0]
                };
            }
            empty = p.tabs.is_empty();
        }
        if empty && id != self.home {
            self.remove_pane(id);
        }
    }

    fn remove_pane(&mut self, id: u64) {
        self.panes.retain(|p| p.id != id);
        Self::take_leaf(&mut self.root, id);
        if self.focus == id {
            self.focus = self.panes.first().map(|p| p.id).unwrap_or(self.home);
        }
    }

    /// 分栏树里把叶子 `id` 换成一半：`new_first` 决定新窗格在左/上还是右/下。
    fn split_leaf(
        node: &mut Layout,
        id: u64,
        axis: SplitAxis,
        new_id: u64,
        new_first: bool,
    ) -> bool {
        match node {
            Layout::Leaf(x) if *x == id => {
                let old = Layout::Leaf(id);
                let new = Layout::Leaf(new_id);
                let (first, second) = if new_first { (new, old) } else { (old, new) };
                *node = Layout::Split {
                    axis,
                    first: Box::new(first),
                    second: Box::new(second),
                };
                true
            }
            Layout::Split { first, second, .. } => {
                Self::split_leaf(first, id, axis, new_id, new_first)
                    || Self::split_leaf(second, id, axis, new_id, new_first)
            }
            Layout::Leaf(_) => false,
        }
    }

    /// 塌掉叶子：父节点换成它的兄弟。整棵树只剩这一块时返回 false（不动）。
    fn take_leaf(node: &mut Layout, id: u64) -> bool {
        match node {
            Layout::Leaf(x) => *x == id,
            Layout::Split { first, second, .. } => {
                if matches!(**first, Layout::Leaf(x) if x == id) {
                    let sibling = (**second).clone();
                    *node = sibling;
                    return true;
                }
                if matches!(**second, Layout::Leaf(x) if x == id) {
                    let sibling = (**first).clone();
                    *node = sibling;
                    return true;
                }
                Self::take_leaf(first, id) || Self::take_leaf(second, id)
            }
        }
    }
}

/// 指针落在窗格哪一区：取到四边最近的一条，超过 1/4 就算中间。
fn zone_at(bounds: Bounds<Pixels>, pos: Point<Pixels>) -> DropZone {
    let w = f32::from(bounds.size.width).max(1.);
    let h = f32::from(bounds.size.height).max(1.);
    let x = (f32::from(pos.x) - f32::from(bounds.origin.x)) / w;
    let y = (f32::from(pos.y) - f32::from(bounds.origin.y)) / h;
    let (dl, dr, dt, db) = (x, 1. - x, y, 1. - y);
    let m = dl.min(dr).min(dt).min(db);
    if m > 0.25 {
        return DropZone::Center;
    }
    if m == dl {
        DropZone::Left
    } else if m == dr {
        DropZone::Right
    } else if m == dt {
        DropZone::Top
    } else {
        DropZone::Bottom
    }
}

/// 拖拽时跟着指针的小页签片。
struct TabChip {
    label: SharedString,
    dark: bool,
}

impl Render for TabChip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::of(self.dark);
        let _ = cx;
        h_flex()
            .px(px(9.))
            .h(px(22.))
            .items_center()
            .rounded(px(3.))
            .bg(to_hsla(p.chrome))
            .border(px(0.5))
            .border_color(to_hsla(p.border))
            .text_size(px(11.))
            .text_color(to_hsla(p.foreground))
            .child(self.label.clone())
    }
}

impl DaroApp {
    /// 主工作区：分栏树 → 元素。`body()` 里替换原来的单栏 `center()`。
    pub fn workspace(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        self.render_node(&self.dock.root, "", window, cx)
    }

    fn render_node(&self, node: &Layout, path: &str, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        match node {
            Layout::Leaf(id) => self.pane_view(*id, window, cx),
            Layout::Split {
                axis,
                first,
                second,
            } => {
                let a = self.render_node(first, &format!("{path}0"), window, cx);
                let b = self.render_node(second, &format!("{path}1"), window, cx);
                let id = format!("dock/{path}");
                match axis {
                    SplitAxis::Row => h_resizable(id)
                        .child(resizable_panel().child(a))
                        .child(resizable_panel().child(b))
                        .into_any_element(),
                    SplitAxis::Col => v_resizable(id)
                        .child(resizable_panel().child(a))
                        .child(resizable_panel().child(b))
                        .into_any_element(),
                }
            }
        }
    }

    /// 一块窗格：页签条 + 内容 + 拖放落点提示。
    fn pane_view(&self, pane_id: u64, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let bg = to_hsla(p.background);
        let (tabs, active) = self
            .dock
            .pane(pane_id)
            .map(|pn| (pn.tabs.clone(), pn.active))
            .unwrap_or_default();
        let is_home = pane_id == self.dock.home;
        let has_tab = active > 0 && self.opened.get(active - 1).is_some();
        // 「用户 / 事件」工具面板只占首页窗格的「对象」页，自带工具条与滚动。
        let tool_view = (!has_tab && is_home).then_some(self.tool_view).flatten();

        // 首页无页签时的对象面板走独立子视图：滚动容器由它自己渲染，
        // 滚轮只 notify 那个 Entity，不会把整个 DaroApp 拖进每一帧重建。
        let pane_view = !has_tab && tool_view.is_none() && is_home;

        let content = if has_tab {
            let t = &self.opened[active - 1];
            let alive = self.conns.iter().any(|c| c.name == t.conn_name);
            if t.notes.is_some() {
                // 发布说明页不挂连接，先于 alive 判断，否则会被当成死页签留白
                self.release_notes_page(t, cx).into_any_element()
            } else if !alive {
                div().into_any_element()
            } else if t.kv {
                self.kv_page(t, window, cx).into_any_element()
            } else if t.query {
                self.query_page(t, cx).into_any_element()
            } else if t.design {
                self.designer_page(t, cx).into_any_element()
            } else if t.role {
                self.role_page(t, cx).into_any_element()
            } else {
                self.table_page(t, cx).into_any_element()
            }
        } else if let Some(view) = tool_view {
            self.tool_pane(view, cx)
        } else if pane_view {
            self.objects_pane_element()
        } else if self.list_view {
            self.object_list(cx).into_any_element()
        } else {
            div()
                .p(px(8.))
                .child(self.object_grid(cx))
                .into_any_element()
        };

        // 页签页自己管滚动（表格 / 查询 / 设计都有定高的内部滚动区）；外层再
        // 套竖向滚动会让 flex_1 拿到无界高度，网格长到整页高、内部滚动失效。
        // 首页的对象列表 / 网格没有自管滚动，仍由外层兜底。
        let scroll: AnyElement = if pane_view {
            // 子视图自带滚动容器与右键菜单，外层再套一层滚动会双层滚动。
            content
        } else if has_tab || tool_view.is_some() {
            div()
                .id(format!("pane-scroll/{pane_id}"))
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(content)
                .into_any_element()
        } else {
            div()
                .id(format!("pane-scroll/{pane_id}"))
                .flex_1()
                .min_h_0()
                .child(content)
                // track_scroll 要挂在转成 Scrollable 之前；对象列表按这个偏移
                // 算视口窗口，只建可见行。行仍是滚动内容的直接子元素，
                // 空白处右键菜单不受影响。
                .track_scroll(&self.object_scroll)
                .overflow_y_scrollbar()
                // 对象列表面板的右键菜单挂在这层（不是行上）：不满一屏时下面
                // 的空白处也要能右键出分类菜单，和参考界面一致。
                // 行自己会在按下时认领目标（`select_for_context`）。
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, _, _| this.blank_for_context(crate::BlankPane::Objects)),
                )
                .context_menu(crate::node_context_menu(cx.entity().clone()))
                .into_any_element()
        };

        let mut col = v_flex()
            .id(("pane", pane_id))
            .role(Role::TabPanel)
            .aria_label("窗格")
            .size_full()
            .bg(bg)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if this.dock.focus != pane_id {
                        this.dock.focus_pane(pane_id);
                        this.active_tab = this.dock.focused_active();
                        cx.notify();
                    }
                }),
            )
            .on_drag_move::<DragTab>(cx.listener(
                move |this, ev: &DragMoveEvent<DragTab>, _window, cx| {
                    let zone = zone_at(ev.bounds, ev.event.position);
                    if this.drag_hover != Some((pane_id, zone)) {
                        this.drag_hover = Some((pane_id, zone));
                        cx.notify();
                    }
                },
            ))
            .on_drop::<DragTab>(cx.listener(
                move |this, drag: &DragTab, window, cx| {
                    let zone = this
                        .drag_hover
                        .filter(|(id, _)| *id == pane_id)
                        .map(|(_, z)| z)
                        .unwrap_or(DropZone::Center);
                    this.apply_tab_drop(drag.tab, pane_id, zone, window, cx);
                },
            ))
            .child(self.pane_tab_bar(pane_id, &tabs, active, is_home, cx))
            .when(!has_tab && tool_view.is_none(), |s| s.child(self.action_bar(cx)))
            .child(scroll);

        if let Some((id, zone)) = self.drag_hover
            && id == pane_id
            && is_home_or_any(self, pane_id)
        {
            col = col.child(self.drop_hint(zone, &p));
        }
        col.into_any_element()
    }

    /// 页签条：只有 home 窗格带常驻的「对象」页。每个页签都可以拖到别的窗格。
    /// 页签总宽超过条宽时整段横向可滚（滚轮 + 右端两个箭头），活动页签
    /// 激活时自动滚进视野。
    fn pane_tab_bar(
        &self,
        pane_id: u64,
        tabs: &[usize],
        active: usize,
        is_home: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.pal();
        let (bg, fg, muted, accent) = (
            to_hsla(p.tab_face),
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            p.accent,
        );
        // Zed 页签：条带是 panel 底，活动页签换成编辑器底并盖住下边线，
        // 于是选中页与下方内容连成一片。
        let active_bg = to_hsla(p.background);
        let tab_line = to_hsla(p.border);
        let dark = self.dark;
        let tab_scroll = self
            .dock
            .pane(pane_id)
            .map(|p| p.tab_scroll.clone())
            .unwrap_or_default();

        // 工具面板开着时这个页签就跟着改名（参考界面的「用户 / 事件」页）；
        // 再点一下收回对象清单——面板没有别的关闭入口。
        let object_label = self.tool_view.map(|v| v.label()).unwrap_or("对象");

        let object_tab = h_flex()
            .id(("tab-objects", pane_id))
            .role(Role::Tab)
            .aria_selected(active == 0)
            .aria_label(object_label)
            .h_full()
            .px(px(8.))
            .items_center()
            .flex_none()
            .relative()
            // 条带里只有它一个时补右框线；有数据页签时由后者的左框线分界
            .when(tabs.is_empty(), |s| s.border_r(px(1.)).border_color(tab_line))
            .cursor_pointer()
            .when(active == 0, move |s| s.bg(active_bg))
            .when(active > 0, move |s| {
                s.border_b(px(1.))
                    .border_color(tab_line)
                    .hover(move |s| s.bg(to_hsla(p.hover)))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dock.focus_home();
                this.active_tab = 0;
                this.tool_view = None;
                cx.notify();
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(if active == 0 { fg } else { muted })
                    .child(object_label),
            )
            .test_support();

        let chips = tabs.iter().enumerate().filter_map(|(pos, &ix)| {
            let Some(t) = self.opened.get(ix - 1) else {
                return None;
            };
            let glyph = DaroApp::tab_glyph(t);
            let is_active = active == ix;
            let title = self.tab_label(t);
            let chip_label = title.clone();
            // 悬停提示：页签标题被截断时，像网页 title 一样给出完整名称
            let tip_label = title.clone();
            // 页签跟着所属连接的颜色标签走：顶边一条原色细线 + 淡色底，
            // 没设颜色的连接保持原来的灰白页签。
            let tint = self.conn_tint_for(&t.conn_name);
            let chip_active_bg = tint.map_or(active_bg, |c| c.tab_active);
            let chip_idle_bg = tint.map(|c| c.tab);
            let chip_hover = tint.map_or(to_hsla(p.hover), |c| c.tab_hover);
            // × 单独再深一档：非活动页签的 × 平时就是页签 hover 色，
            // 指针落到 × 上时不变色的话会看不出这里能点
            let close_hover = to_hsla(p.selection);
            Some(
                h_flex()
                    .id(format!("tab/{ix}"))
                    .role(Role::Tab)
                    .aria_selected(is_active)
                    .aria_label(chip_label.clone())
                    .h_full()
                    .px(px(8.))
                    .gap(px(4.))
                    .items_center()
                    // 条带放不下时先收缩页签（文字截断出省略号），
                    // 缩到最小宽仍放不下才整段滚动；最小宽压得比较小，
                    // 让短标题的页签把宽度让给长标题的页签
                    .flex_shrink_1()
                    .min_w(px(84.))
                    // 活动页签的 × 实体占位，窄到这个程度标题就没地方了，
                    // 单独给它留出放完整短标题的宽度
                    .when(is_active, |s| s.min_w(px(108.)))
                    .cursor_pointer()
                    .relative()
                    // Zed 页签分格：相邻页签共用一条 1px 全高竖线，由右侧页签
                    // 的左框线画；条带最左端不描边，最后一个页签补右框线收口。
                    // 活动页签左右框线都在、只去掉底边线，于是和下方内容连成一片。
                    .group("tab")
                    .when(pos > 0 || is_home, |s| {
                        s.border_l(px(1.)).border_color(tab_line)
                    })
                    .when(pos + 1 == tabs.len(), |s| {
                        s.border_r(px(1.)).border_color(tab_line)
                    })
                    .when(is_active, move |s| s.bg(chip_active_bg))
                    .when(!is_active, move |s| {
                        s.border_b(px(1.))
                            .border_color(tab_line)
                            .when_some(chip_idle_bg, |s, c| s.bg(c))
                            .hover(move |s| s.bg(chip_hover))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // × 的点击会冒泡到页签本体，刚关完一页时下标可能越界
                        if ix > this.opened.len() {
                            return;
                        }
                        this.dock.focus_tab(ix);
                        this.active_tab = ix;
                        cx.notify();
                    }))
                    .on_drag(
                        DragTab { tab: ix },
                        move |_drag, _offset, _window, cx| {
                            let label = chip_label.clone();
                            cx.new(|_| TabChip {
                                label: label.into(),
                                dark,
                            })
                        },
                    )
                    .test_support()
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tip_label.clone())
                            // 字号与页签一致、内边距收到最小；外边距保持默认，
                            // 它同时是浮层跟指针之间的距离
                            .text_xs()
                            .py(px(2.))
                            .px(px(6.))
                            // 弹层默认的白面比内容区还亮，边界和阴影都看不出来；
                            // 换成工具栏面 + 深一档描边，浮层和背后界面分开
                            .bg(to_hsla(p.button_face))
                            .border_color(to_hsla(p.field_border))
                            .shadow_lg()
                            .build(window, cx)
                    })
                    .context_menu(tab_context_menu(cx.entity().clone(), pane_id, Some(ix)))
                    .child(div().flex_none().child(art::icon(
                        glyph,
                        15.,
                        if is_active { to_hsla(accent) } else { muted },
                    )))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(if is_active { fg } else { muted })
                            .child(title),
                    )
                    .child(
                        // 活动页签的 × 正常占位（不会被悬停遮住标题）；
                        // 非活动页签的 × 悬浮在右端、悬停才显形，平时不占
                        // 宽度，那 18px 让给标题文字
                        h_flex()
                            .id(format!("tab-close/{ix}"))
                            .items_center()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.close_tab(ix - 1);
                                this.active_tab = this.dock.focused_active();
                                cx.notify();
                            }))
                            .when(is_active, |s| s.flex_none())
                            .when(!is_active, |s| {
                                s.absolute()
                                    .top(px(0.))
                                    .right(px(1.))
                                    .h_full()
                                    .opacity(0.)
                                    .group_hover("tab", |s| s.opacity(1.))
                            })
                            .child(
                                h_flex()
                                    .id(format!("tab-close-box/{ix}"))
                                    .size(px(18.))
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(2.))
                                    // 非活动页签的 × 是一块实底遮罩：显形时把压在
                                    // 它下面的标题文字完全盖住，点哪里都不会误触标题
                                    .when(!is_active, move |s| s.bg(chip_hover))
                                    // 指针直接落到 × 上时再深一档，给出可点的回执
                                    .hover(move |s| s.bg(close_hover))
                                    .child(
                                        Icon::new(ZedIcon::Close)
                                            .with_size(px(13.))
                                            .text_color(muted),
                                    ),
                            ),
                    )
                    // 顶边色条：绝对定位，不占页签的 flex 宽度
                    .when_some(tint, |s, t| {
                        s.child(
                            div()
                                .absolute()
                                .top(px(0.))
                                .left(px(0.))
                                .right(px(0.))
                                .h(px(2.))
                                .bg(t.solid),
                        )
                    }),
            )
        });

        // 页签区：每个页签是滚动容器的直接子元素（ScrollHandle 的
        // child_bounds 只记直接子元素，中间不能再套一层 h_flex，否则
        // scroll_to_item 找不到页签）。页签先收缩、缩到最小宽仍放不下
        // 才溢出滚动；on_scroll 把滚轮滚动通知回主视图，箭头明暗跟着刷新。
        // 右键空白处弹同一套页签菜单（无目标页签的条目置灰）。
        let strip = div()
            .id(("tab-scroll", pane_id))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .overflow_x_scroll()
            .track_scroll(&tab_scroll)
            .on_scroll({
                let me = cx.entity().clone();
                move |_e, _window, cx| {
                    let _ = me.update(cx, |_, cx| cx.notify());
                }
            })
            .context_menu(tab_context_menu(cx.entity().clone(), pane_id, None))
            .children(chips)
            // 页签右侧的空白条也保留底边线；选中页签自身不画线，
            // 于是只有它那一段缺口，与下方工具栏连成一片
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .border_b(px(1.))
                    .border_color(tab_line),
            );

        // 页签缩到最小宽仍溢出时，条带两端各出一个箭头（参考界面同款）：
        // 点一下滚一档（约一个视口宽，留 16px 让被切半的页签接着看），到边变暗
        let mut left_arrow = None;
        let mut right_arrow = None;
        let max_x = tab_scroll.max_offset().x;
        if max_x > px(0.5) {
            // gpui 的滚动 offset 向右滚为负、停在最左为 0，换算成正的「已滚距离」
            let scrolled = -tab_scroll.offset().x;
            for (left, id, icon, enabled, backward) in [
                (
                    true,
                    "tab-arrow-left",
                    ZedIcon::ChevronLeft,
                    scrolled > px(0.5),
                    true,
                ),
                (
                    false,
                    "tab-arrow-right",
                    ZedIcon::ChevronRight,
                    scrolled < max_x - px(0.5),
                    false,
                ),
            ] {
                let arrow = div()
                    .id((id, pane_id))
                    .w(px(16.))
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_b(px(1.))
                    .border_color(tab_line)
                    // 到边只暗图标，底边线始终与页签同色：
                    // 整个格子叠 transparency 会把这条 1px 线也一起淡掉，
                    // 两端的箭头看起来就一条深一条浅
                    .when(enabled, |s| s.cursor_pointer().hover(move |s| s.bg(to_hsla(p.hover))))
                    .when(enabled, |s| {
                        s.on_click(cx.listener(move |this, _, _, cx| {
                            let Some(pn) = this.dock.pane_mut(pane_id) else {
                                return;
                            };
                            let mut o = pn.tab_scroll.offset();
                            let max = pn.tab_scroll.max_offset().x;
                            let view = pn.tab_scroll.bounds().size.width;
                            let step = if view > px(16.) { view - px(16.) } else { px(160.) };
                            // offset 落在 [-max, 0]：左箭头朝 0 回，右箭头朝 -max 走
                            o.x = if backward {
                                (o.x + step).min(Pixels::ZERO)
                            } else {
                                (o.x - step).max(-max)
                            };
                            pn.tab_scroll.set_offset(o);
                            cx.notify();
                        }))
                    })
                    .child(
                        Icon::new(icon)
                            .with_size(px(11.))
                            .text_color(if enabled { muted } else { to_hsla(p.subtle_foreground) }),
                    );
                if left {
                    left_arrow = Some(arrow);
                } else {
                    right_arrow = Some(arrow);
                }
            }
        }

        let bar = h_flex()
            .id(("tab-bar", pane_id))
            .role(Role::TabList)
            .aria_label("页签")
            .h(px(32.))
            .flex_none()
            .bg(bg)
            .when(is_home, |s| s.child(object_tab))
            .when_some(left_arrow, |s, a| s.child(a))
            .child(strip)
            .when_some(right_arrow, |s, a| s.child(a));

        bar.into_any_element()
    }

    /// 拖拽落点提示：目标半边铺一层 accent 淡底 + 描边，中间区铺满整块。
    fn drop_hint(&self, zone: DropZone, p: &Palette) -> AnyElement {
        let accent = to_hsla(p.accent);
        let rect = div()
            .absolute()
            .bg(accent)
            .opacity(0.14)
            .border_1()
            .border_color(accent);
        let rect = match zone {
            DropZone::Left => rect.left_0().top_0().w(relative(0.5)).h_full(),
            DropZone::Right => rect.right_0().top_0().w(relative(0.5)).h_full(),
            DropZone::Top => rect.left_0().top_0().w_full().h(relative(0.5)),
            DropZone::Bottom => rect.left_0().bottom_0().w_full().h(relative(0.5)),
            DropZone::Center => rect.left_0().top_0().w_full().h_full(),
        };
        div()
            .absolute()
            .left_0()
            .top_0()
            .w_full()
            .h_full()
            .child(rect)
            .into_any_element()
    }

    /// 落在窗格上：`Center` 并入、其余新开一栏。
    pub fn apply_tab_drop(
        &mut self,
        tab: usize,
        target: u64,
        zone: DropZone,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_hover = None;
        if tab == 0 || self.opened.get(tab - 1).is_none() {
            cx.notify();
            return;
        }
        if self.dock.move_tab(tab, target, zone) {
            self.active_tab = self.dock.focused_active();
        }
        cx.notify();
    }

    /// 关掉除指定页签（1 基）外的所有数据 / 查询页签；「对象」页不受影响。
    fn close_other_tabs(&mut self, keep: usize) {
        let mut gone: Vec<usize> = (0..self.opened.len())
            .filter(|&i| i + 1 != keep)
            .collect();
        gone.sort_unstable_by(|a, b| b.cmp(a));
        for i in gone {
            self.close_tab(i);
        }
    }

    /// 关掉指定窗格里排在目标页签右侧的所有页签（从大到小关，下标前移不互相干扰）。
    fn close_tabs_right(&mut self, pane_id: u64, ix: usize) {
        let mut right: Vec<usize> = self
            .dock
            .pane(pane_id)
            .map(|p| p.tabs.iter().copied().filter(|&t| t > ix).collect())
            .unwrap_or_default();
        right.sort_unstable_by(|a, b| b.cmp(a));
        for t in right {
            self.close_tab(t - 1);
        }
    }

    /// 关掉全部数据 / 查询页签，回到「对象」页。
    pub(crate) fn close_all_tabs(&mut self) {
        for i in (0..self.opened.len()).rev() {
            self.close_tab(i);
        }
    }

    /// 页签右键「在导航窗格中显示」：把连接 → 库 → 模式 → 分类一路展开，
    /// 对象在树上标成选中，并切回「对象」页。查询页签没有目录项，
    /// 落到它的分类节点上。
    fn reveal_in_nav(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(t) = self.opened.get(ix - 1) else {
            return;
        };
        let (conn_name, db_name, s, cat, obj) = (t.conn_name.clone(), t.db, t.schema, t.cat, t.obj);
        let Some(c) = self.conns.iter().position(|c| c.name == conn_name) else {
            return;
        };
        if !self.conns[c].connected {
            self.open_connection(c, cx);
        }
        let kind = self.conns[c].kind;
        let d = catalog::databases(&self.conns[c])
            .iter()
            .position(|name| *name == db_name)
            .unwrap_or(0);
        let schema_name = catalog::schemas(&self.conns[c], db_name)
            .get(s)
            .copied()
            .unwrap_or("public");
        let db_key = node_key(&conn_name, db_name);
        self.expanded.insert(conn_name.clone());
        self.visited_dbs.insert(db_key.clone());
        self.expanded.insert(db_key.clone());
        if catalog::has_schemas(kind) {
            let schema_key = node_key(&db_key, schema_name);
            self.expanded.insert(schema_key.clone());
            self.expanded.insert(node_key(&schema_key, CATEGORIES[cat.min(4)].0));
        } else {
            self.expanded.insert(node_key(&db_key, CATEGORIES[cat.min(4)].0));
        }
        // 查询页签的 obj 没有目录项（obj=0 哨兵），选分类节点而不是不存在的对象
        let has_obj = catalog::objects(&self.conns[c], db_name, schema_name, cat).len() > obj;
        self.sel = if has_obj {
            Act::Obj(c, d, s, cat, obj)
        } else {
            Act::Cat(c, d, s, cat)
        };
        self.dock.focus_home();
        self.active_tab = 0;
        cx.notify();
    }
}

/// home 窗格永远可以接收拖放；其它窗格存在即可（占位函数，便于以后加条件）。
fn is_home_or_any(_app: &DaroApp, _pane_id: u64) -> bool {
    true
}

/// 页签右键菜单：关闭组 + 移动到新窗口 / 收藏夹（未实现，置灰）+
/// 在导航窗格中显示 + 已打开对象清单（左侧 ✓ 标当前页）。
/// 条目、分隔线按参考界面实测还原；关闭 / 全部关闭挂全局键位
/// （Ctrl+W / Ctrl+Shift+W），菜单里由键位系统自动补快捷键角标。
/// `ix = None` 表示右键落在条带空白处（滑轨）：无目标页签的条目
/// （关闭 / 关闭其他 / 关闭右侧 / 在导航窗格中显示）置灰，
/// 全部关闭与对象清单仍可用。
fn tab_context_menu(
    me: Entity<DaroApp>,
    pane_id: u64,
    ix: Option<usize>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu: PopupMenu, _window: &mut Window, _cx: &mut Context<PopupMenu>| {
        let has_target = ix.is_some();
        // 清单在菜单弹出那一刻取一次快照：关闭组执行后菜单即收起，不必跟着活
        let (active_ix, entries): (usize, Vec<(usize, String)>) = {
            let this = me.read(_cx);
            let entries = this
                .opened
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    // 查询 / 设计 / 角色页签的标题自带语义（查询-N / (设计) / (连接)），
                    // 数据页签补「- 表 / 视图…」后缀（参考界面同款）
                    let label = if t.query || t.design || t.role || t.kv {
                        this.tab_label(t)
                    } else {
                        format!("{} - {}", this.tab_label(t), CATEGORIES[t.cat.min(4)].0)
                    };
                    (i + 1, label)
                })
                .collect();
            (this.active_tab, entries)
        };

        let can_fav = {
            let app = me.read(_cx);
            ix.is_some_and(|ix| app.favorite_candidate(ix - 1, _cx).is_some())
        };

        let me_close = me.clone();
        let me_others = me.clone();
        let me_right = me.clone();
        let me_all = me.clone();
        let me_nav = me.clone();
        let me_fav = me.clone();

        let mut menu = menu
            .item(
                PopupMenuItem::new("关闭")
                    .action(Box::new(CloseTab))
                    .disabled(!has_target)
                    .on_click(move |_, window, cx| {
                        let me = me_close.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| {
                                if let Some(ix) = ix {
                                    if ix > 0 && ix <= this.opened.len() {
                                        this.close_tab(ix - 1);
                                        cx.notify();
                                    }
                                }
                            });
                        });
                    }),
            )
            .item(
                PopupMenuItem::new("关闭其他选项卡")
                    .disabled(!has_target)
                    .on_click(move |_, window, cx| {
                        let me = me_others.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| {
                                if let Some(ix) = ix {
                                    this.close_other_tabs(ix);
                                    cx.notify();
                                }
                            });
                        });
                    }),
            )
            .item(
                PopupMenuItem::new("关闭右侧的选项卡")
                    .disabled(!has_target)
                    .on_click(move |_, window, cx| {
                        let me = me_right.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| {
                                if let Some(ix) = ix {
                                    this.close_tabs_right(pane_id, ix);
                                    cx.notify();
                                }
                            });
                        });
                    }),
            )
            .item(
                PopupMenuItem::new("全部关闭")
                    .action(Box::new(CloseAllTabs))
                    .on_click(move |_, window, cx| {
                        let me = me_all.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| {
                                this.close_all_tabs();
                                cx.notify();
                            });
                        });
                    }),
            )
            .separator()
            .item(PopupMenuItem::new("将选项卡移动到新窗口").disabled(true))
            .separator()
            .item(
                PopupMenuItem::new("添加到收藏夹…")
                    .disabled(!can_fav)
                    .on_click(move |_, window, cx| {
                        let me = me_fav.clone();
                        window.defer(cx, move |_window, cx| {
                            if let Some(ix) = ix {
                                me.update(cx, |app, cx| {
                                    app.start_add_favorite_for_tab(ix - 1, cx);
                                });
                            }
                        });
                    }),
            )
            .item(
                PopupMenuItem::new("在导航窗格中显示")
                    .disabled(!has_target)
                    .on_click(move |_, window, cx| {
                        let me = me_nav.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| {
                                if let Some(ix) = ix {
                                    this.reveal_in_nav(ix, cx);
                                }
                            });
                        });
                    }),
            )
            .separator()
            .item(PopupMenuItem::label("对象"));
        for (i, label) in entries {
            let me_switch = me.clone();
            menu = menu.item(
                PopupMenuItem::new(label)
                    .checked(i == active_ix)
                    .on_click(move |_, window, cx| {
                        let me = me_switch.clone();
                        window.defer(cx, move |_window, cx| {
                            me.update(cx, |this, cx| {
                                this.dock.focus_tab(i);
                                this.active_tab = i;
                                cx.notify();
                            });
                        });
                    }),
            );
        }
        menu
    }
}

