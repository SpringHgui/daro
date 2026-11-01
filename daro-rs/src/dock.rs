//! 主工作区多窗格分栏（dock）。
//!
//! Navicat 的「窗口」菜单里有层叠 / 水平平铺 / 垂直平铺；daro-rs 用页签条承载
//! 多个对象页，这里把「一个页签条 + 一块内容」抽象成窗格（[`Pane`]），窗格之间
//! 用二叉分栏树（[`Layout`]）排布：
//!
//! ```text
//! Split(Row)                      ┌──────────┬──────────┐
//! ├─ Leaf(home)         ＝＝>      │  home    │  新窗格  │
//! └─ Leaf(other)                  └──────────┴──────────┘
//! ```
//!
//! 交互口径（照 IDE 的通用习惯，Navicat 本身没有可拖的页签，这一层是 daro-rs
//! 自己的扩展）：
//!
//! - 拖页签时，指针所在窗格按「左 / 右 / 上 / 下 / 中间」五区高亮提示落点；
//! - 落在左/右/上/下 → 以该窗格为基准新开一栏（上/下是上下分栏）；
//! - 落在中间 → 页签并入该窗格；
//! - 分隔条可拖动改比例（用组件库的 resizable，见 [`crate::DaroApp::render_node`]）。
//!
//! 窗格用 `home` 标记第一块：它承载常驻的「对象」页（`active == 0`），且永不因
//! 拖空页签而消失；其它窗格空了就塌掉、由分栏树把兄弟节点提上来。

use gpui_kit::assets::IconName;
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
            .border_1()
            .border_color(to_hsla(p.border))
            .text_size(px(11.))
            .text_color(to_hsla(p.foreground))
            .child(self.label.clone())
    }
}

impl DaroApp {
    /// 主工作区：分栏树 → 元素。`body()` 里替换原来的单栏 `center()`。
    pub fn workspace(&self, cx: &mut Context<Self>) -> AnyElement {
        self.render_node(&self.dock.root, "", cx)
    }

    fn render_node(&self, node: &Layout, path: &str, cx: &mut Context<Self>) -> AnyElement {
        match node {
            Layout::Leaf(id) => self.pane_view(*id, cx),
            Layout::Split {
                axis,
                first,
                second,
            } => {
                let a = self.render_node(first, &format!("{path}0"), cx);
                let b = self.render_node(second, &format!("{path}1"), cx);
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
    fn pane_view(&self, pane_id: u64, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let bg = to_hsla(p.background);
        let border = to_hsla(p.border);
        let (tabs, active) = self
            .dock
            .pane(pane_id)
            .map(|pn| (pn.tabs.clone(), pn.active))
            .unwrap_or_default();
        let is_home = pane_id == self.dock.home;
        let has_tab = active > 0 && self.opened.get(active - 1).is_some();

        let content = if has_tab {
            let t = &self.opened[active - 1];
            let alive = self.conns.iter().any(|c| c.name == t.conn_name);
            if !alive {
                div().into_any_element()
            } else if t.query {
                self.query_page(t, cx).into_any_element()
            } else if t.design {
                self.designer_page(t, cx).into_any_element()
            } else {
                self.table_page(t, cx).into_any_element()
            }
        } else if self.list_view {
            self.object_list(cx).into_any_element()
        } else {
            div()
                .p(px(8.))
                .child(self.object_grid(cx))
                .into_any_element()
        };

        let scroll = div()
            .id(format!("pane-scroll/{pane_id}"))
            .flex_1()
            .min_h_0()
            .child(content)
            .overflow_y_scrollbar();

        let mut col = v_flex()
            .id(("pane", pane_id))
            .size_full()
            .bg(bg)
            .border_1()
            .border_color(border)
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
            .when(!has_tab, |s| s.child(self.action_bar(cx)))
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
    fn pane_tab_bar(
        &self,
        pane_id: u64,
        tabs: &[usize],
        active: usize,
        is_home: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.pal();
        let (bg, border, fg, muted, accent) = (
            to_hsla(p.background),
            to_hsla(p.border),
            to_hsla(p.foreground),
            to_hsla(p.subtle_foreground),
            p.accent,
        );
        // Navicat 取样：页签条 #f0f0f0、活动页签纯白并压住下边线
        let strip = to_hsla(p.chrome);
        let active_bg = to_hsla(p.background);
        let tab_line = to_hsla(p.border);
        let dark = self.dark;

        let object_tab = h_flex()
            .id(("tab-objects", pane_id))
            .h_full()
            .px(px(12.))
            .items_center()
            .cursor_pointer()
            .when(active == 0, move |s| s.bg(active_bg))
            .when(active > 0, move |s| s.hover(move |s| s.bg(to_hsla(p.hover))))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dock.focus_home();
                this.active_tab = 0;
                cx.notify();
            }))
            .child(div().text_xs().text_color(fg).child("对象"));

        let mut bar = h_flex()
            .h(px(26.))
            .flex_none()
            .bg(bg)
            .border_b_1()
            .border_color(border)
            .when(is_home, |s| s.child(object_tab));

        for &ix in tabs {
            let Some(t) = self.opened.get(ix - 1) else {
                continue;
            };
            let glyph = if t.query {
                art::Glyph::Query
            } else if t.design {
                art::Glyph::DesignTable
            } else if t.cat == 0 {
                art::Glyph::Table
            } else if t.cat == 2 {
                art::Glyph::MatView
            } else {
                art::Glyph::View
            };
            let is_active = active == ix;
            let title = self.tab_label(t);
            let chip_label = title.clone();
            let chip = h_flex()
                .id(format!("tab/{ix}"))
                .h_full()
                .px(px(10.))
                .gap(px(5.))
                .items_center()
                .cursor_pointer()
                .when(is_active, move |s| {
                    s.bg(active_bg)
                        .border_l_1()
                        .border_r_1()
                        .border_color(tab_line)
                })
                .when(!is_active, move |s| s.hover(move |s| s.bg(to_hsla(p.hover))))
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
                .child(art::icon(glyph, 15., to_hsla(accent)))
                .child(div().text_xs().text_color(fg).child(title))
                .child(
                    h_flex()
                        .id(format!("tab-close/{ix}"))
                        .size(px(14.))
                        .items_center()
                        .justify_center()
                        .rounded(px(2.))
                        .cursor_pointer()
                        .when(!is_active, |s| s.opacity(0.))
                        .hover(move |s| s.opacity(1.).bg(to_hsla(p.hover)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.close_tab(ix - 1);
                            this.active_tab = this.dock.focused_active();
                            cx.notify();
                        }))
                        .child(
                            Icon::new(IconName::X)
                                .with_size(px(11.))
                                .text_color(muted),
                        ),
                );
            bar = bar.child(chip);
        }

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
}

/// home 窗格永远可以接收拖放；其它窗格存在即可（占位函数，便于以后加条件）。
fn is_home_or_any(_app: &DaroApp, _pane_id: u64) -> bool {
    true
}
