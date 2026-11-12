//! 「工具 → 选项...」设置窗。版面直接照搬 Zed `crates/settings_ui`：
//! 左侧 226px 侧栏（搜索框 + 根页/页内分组两级树 + Ctrl-Shift-E 脚注），
//! 右侧内容区（文件范围条 + 16px 页标题 + 等宽小标题分组 + 14/12px 设置行）。
//!
//! 只摆 daro 自己的、真正会落盘到 `connections.json` 的开关；参考客户端与
//! Zed 的设置项不照搬，没实现的项也不留入口。改动走草稿态，「确定」才应用
//! 并写盘，「取消」/ 关闭 / Esc 一律丢弃。

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::menu::{DropdownMenu as _, PopupMenuItem};
use crate::mcp_page::McpPage;
use crate::palette::{self, Palette};
use crate::zed_icons::{self, ZedIcon};
use crate::{to_hsla, DaroApp, ui};

const TITLE_H: f32 = 24.;
/// 侧栏宽度（Zed `SIDEBAR_WIDTH`）
const NAV_W: f32 = 226.;
/// 设置窗内容区设计尺寸（另加 24 的自绘标题栏）。
const CONTENT_SIZE: (f32, f32) = (840., 620.);
/// 页标题：Zed 导航的根项；子项是页内分组，点击滚到对应分组。
const PAGES: [&str; 5] = ["常规", "外观", "窗口与布局", "查询", "扩展"];
/// 每页包含哪些分组（`SECTIONS` 的下标）。
const PAGE_SECTIONS: [&[usize]; 5] = [&[0], &[1, 2], &[3], &[4], &[5]];

#[derive(Clone, Copy)]
enum RowKind {
    DataDir,
    Version,
    Theme,
    UiFont,
    ShowTree,
    ShowInfo,
    ShowMenus,
    ObjectView,
    RunPane,
}

struct RowDef {
    kind: RowKind,
    title: &'static str,
    desc: &'static str,
}

const ROWS_GENERAL: &[RowDef] = &[
    RowDef {
        kind: RowKind::DataDir,
        title: "数据目录",
        desc: "",
    },
    RowDef {
        kind: RowKind::Version,
        title: "当前版本",
        desc: "",
    },
];

const ROWS_THEME: &[RowDef] = &[RowDef {
    kind: RowKind::Theme,
    title: "界面主题",
    desc: "One / Ayu / Gruvbox，深浅由主题本身决定。",
}];

const ROWS_UI_FONT: &[RowDef] = &[RowDef {
    kind: RowKind::UiFont,
    title: "界面字体",
    desc: "作用于菜单、按钮与表格文字；SQL 编辑器仍用等宽 Consolas。",
}];

const ROWS_LAYOUT: &[RowDef] = &[
    RowDef {
        kind: RowKind::ShowTree,
        title: "显示导航窗格",
        desc: "左侧连接树；与「查看 ▸ 导航窗格」同步。",
    },
    RowDef {
        kind: RowKind::ShowInfo,
        title: "显示信息窗格",
        desc: "右侧对象信息卡；与「查看 ▸ 信息窗格」同步。",
    },
    RowDef {
        kind: RowKind::ShowMenus,
        title: "常显菜单栏",
        desc: "标题栏一直铺开「文件 / 编辑 / 查看…」，不再收进 ☰；开启后面包屑让位。",
    },
    RowDef {
        kind: RowKind::ObjectView,
        title: "对象面板视图",
        desc: "对象按图标网格或一行一个列出。",
    },
];

const ROWS_RUN: &[RowDef] = &[RowDef {
    kind: RowKind::RunPane,
    title: "执行后默认显示",
    desc: "查询执行完成后默认激活的结果页。",
}];

/// （分组名，所属页，行）
const SECTIONS: [(&str, usize, &[RowDef]); 6] = [
    ("通用", 0, ROWS_GENERAL),
    ("主题", 1, ROWS_THEME),
    ("界面字体", 1, ROWS_UI_FONT),
    ("面板与视图", 2, ROWS_LAYOUT),
    ("结果", 3, ROWS_RUN),
    // MCP 服务整页由 `McpPage` 自绘，这里只留导航项。
    ("MCP 服务", 4, &[]),
];

/// 应用级设置的一份快照：开窗时读入做草稿，「确定」时整份写回。
#[derive(Clone)]
pub(crate) struct AppOptions {
    pub(crate) dark: bool,
    /// 主题下标（`palette::THEMES`）；深浅由主题自身决定
    pub(crate) theme: usize,
    pub(crate) show_tree: bool,
    pub(crate) show_menus: bool,
    pub(crate) show_info: bool,
    pub(crate) list_view: bool,
    pub(crate) run_pane: u8,
    /// 界面字体族名；空串表示用默认（Segoe UI）
    pub(crate) ui_font: String,
}

pub(crate) fn open_options_dialog(parent: Entity<DaroApp>, cx: &mut App) {
    let draft = parent.read(cx).app_options();
    let dark = draft.dark;
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(720.), px(480.)));
    options.window_bounds = Some(WindowBounds::centered(
        size(px(CONTENT_SIZE.0), px(CONTENT_SIZE.1 + TITLE_H)),
        cx,
    ));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("选项".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| OptionsDialog::new(parent, dark, draft, window, cx))
    });
    if opened.is_err() {
        eprintln!("failed to open options window");
    }
}

struct OptionsDialog {
    parent: Entity<DaroApp>,
    /// 窗口自身配色：开窗时应用的明暗。草稿里换主题只让控件状态变化，
    /// 免得半途改全局主题把主窗和本窗一起闪一下。
    dark: bool,
    draft: AppOptions,
    page: usize,
    /// 当前选中的页内分组（`SECTIONS` 下标）；None = 选中根页。
    section: Option<usize>,
    /// 每个根页的展开状态；默认全开。
    nav_open: [bool; 5],
    /// 侧栏搜索；按标题 / 说明过滤导航与设置行。
    search: Entity<InputState>,
    /// 内容区焦点，Ctrl-Shift-E 与侧栏互相切换。
    content_focus: FocusHandle,
    /// 分组跳转要按子元素序号滚，handle 只认直接子元素（标题是 0）。
    content_scroll: ScrollHandle,
    /// 本机已安装的字体族（`@` 开头的竖排 CJK 变体去掉），开窗时取一次。
    ui_fonts: Vec<SharedString>,
    /// MCP 页自己管即时生效与心跳轮询；嵌在选项窗里，不再单独开窗。
    mcp: Entity<McpPage>,
    _esc: Subscription,
}

impl OptionsDialog {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        draft: AppOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mcp = cx.new(|cx| McpPage::new(dark, window, cx));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索设置"));
        cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_search_changed(cx);
            }
        })
        .detach();
        // 端口 / 搜索输入框聚焦时 InputState 会把 Esc 吃掉，冒泡阶段收不到；
        // 按键拦截器跑在动作派发之前，按窗口 id 限定只关本窗。
        let this_window = window.window_handle();
        let me = cx.entity().downgrade();
        let esc = cx.intercept_keystrokes(move |ev, window, cx| {
            if window.window_handle() != this_window {
                return;
            }
            if ev.keystroke.key == "escape" {
                window.remove_window();
                return;
            }
            // 与 Zed 设置窗相同的 Ctrl-Shift-E：侧栏 / 内容来回聚焦。
            if ev.keystroke.key == "e"
                && ev.keystroke.modifiers.control
                && ev.keystroke.modifiers.shift
            {
                let handles = me.update(cx, |this, cx| {
                    (
                        this.search.read(cx).focus_handle(cx),
                        this.content_focus.clone(),
                    )
                });
                if let Ok((search, content)) = handles {
                    if search.is_focused(window) {
                        content.focus(window, cx);
                    } else {
                        search.focus(window, cx);
                    }
                    me.update(cx, |_, cx| cx.notify()).ok();
                }
            }
        });
        let mut ui_fonts: Vec<SharedString> = cx
            .text_system()
            .all_font_names()
            .into_iter()
            .filter(|name| !name.starts_with('@'))
            .map(SharedString::from)
            .collect();
        // 存档里的字体可能已经卸载；补到清单最前，免得下拉里看不到当前值。
        let current: SharedString = draft.ui_font.clone().into();
        if !current.is_empty() && !ui_fonts.contains(&current) {
            ui_fonts.insert(0, current);
        }
        Self {
            parent,
            dark,
            draft,
            page: 0,
            section: None,
            nav_open: [true; 5],
            search,
            content_focus: cx.focus_handle(),
            content_scroll: ScrollHandle::new(),
            ui_fonts,
            mcp,
            _esc: esc,
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.draft.clone();
        self.parent
            .update(cx, |app, cx| app.apply_app_options(draft, cx));
        window.remove_window();
    }

    fn needle(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_lowercase()
    }

    fn page_matches(&self, page: usize, needle: &str) -> bool {
        PAGES[page].to_lowercase().contains(needle)
            || !self.visible_sections(page, needle).is_empty()
    }

    /// 搜索命中的分组：页标题命中则整页都算命中。
    fn visible_sections(&self, page: usize, needle: &str) -> Vec<usize> {
        let page_hit = PAGES[page].to_lowercase().contains(needle);
        PAGE_SECTIONS[page]
            .iter()
            .copied()
            .filter(|sec| {
                if page_hit {
                    return true;
                }
                let (name, _, rows) = SECTIONS[*sec];
                name.to_lowercase().contains(needle)
                    || rows.iter().any(|row| self.row_matches(row, needle))
            })
            .collect()
    }

    fn visible_rows(&self, sec: usize, needle: &str) -> Vec<&'static RowDef> {
        let (name, page, rows) = SECTIONS[sec];
        let page_hit = PAGES[page].to_lowercase().contains(needle);
        let section_hit = name.to_lowercase().contains(needle);
        rows.iter()
            .filter(|row| page_hit || section_hit || self.row_matches(row, needle))
            .collect()
    }

    fn row_matches(&self, row: &RowDef, needle: &str) -> bool {
        row.title.to_lowercase().contains(needle) || row.desc.to_lowercase().contains(needle)
    }

    fn on_search_changed(&mut self, cx: &mut Context<Self>) {
        let needle = self.needle(cx);
        if !needle.is_empty() && !self.page_matches(self.page, &needle) {
            if let Some(page) = (0..PAGES.len()).find(|page| self.page_matches(*page, &needle)) {
                if self.page != page {
                    self.page = page;
                    self.section = None;
                }
            }
        }
        cx.notify();
    }

    fn open_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.page = page;
        self.section = None;
        self.content_scroll.scroll_to_top_of_item(0);
        cx.notify();
    }

    fn open_section(&mut self, sec: usize, cx: &mut Context<Self>) {
        let needle = self.needle(cx);
        let page = SECTIONS[sec].1;
        // 滚动容器的直接子元素：标题占 0，分组从 1 开始。
        let ordinal = self
            .visible_sections(page, &needle)
            .iter()
            .position(|visible| *visible == sec)
            .unwrap_or(0);
        self.page = page;
        self.section = Some(sec);
        self.content_scroll.scroll_to_top_of_item(ordinal + 1);
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
                .child(zed_icons::ic(
                    ZedIcon::Settings,
                    14.,
                    to_hsla(p.muted_foreground),
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(to_hsla(p.foreground))
                        .child("选项"),
                ),
        )
    }

    fn search_box(&self, has_query: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let search = self.search.clone();
        let me = cx.entity().downgrade();
        h_flex()
            .id("options-search")
            .test_support()
            .h(px(28.))
            .mb(px(12.))
            .pl(px(6.))
            .pr(px(2.))
            .gap(px(6.))
            .items_center()
            .rounded(px(ui::RADIUS))
            .bg(to_hsla(p.background))
            .border(px(ui::HAIRLINE))
            .border_color(to_hsla(p.border))
            .child(zed_icons::ic(
                ZedIcon::MagnifyingGlass,
                14.,
                to_hsla(p.muted_foreground),
            ))
            .child(
                div().flex_1().min_w_0().child(Styled::h(
                    Input::new(&self.search)
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false)
                        .text_size(px(13.)),
                    px(24.),
                )),
            )
            .when(has_query, |this| {
                this.child(
                    div()
                        .id("options-search-clear")
                        .test_support()
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(to_hsla(p.hover)))
                        .on_click(move |_, window, cx| {
                            search.update(cx, |state, cx| state.set_value("", window, cx));
                            me.update(cx, |this, cx| this.on_search_changed(cx)).ok();
                        })
                        .child(zed_icons::ic(
                            ZedIcon::Close,
                            12.,
                            to_hsla(p.muted_foreground),
                        )),
                )
            })
    }

    fn nav_footer(&self, window: &Window, cx: &App) -> AnyElement {
        let p = self.pal();
        let search_focused = self.search.read(cx).focus_handle(cx).is_focused(window);
        let suffix = if search_focused {
            "聚焦内容"
        } else {
            "聚焦导航"
        };
        h_flex()
            .w_full()
            .h(px(32.))
            .flex_shrink_0()
            .px(px(2.))
            .pb(px(2.))
            .gap(px(2.))
            .items_center()
            .border_t(px(ui::HAIRLINE))
            .border_color(to_hsla(p.border_soft))
            .child(key_chip("Ctrl", p))
            .child(key_chip("Shift", p))
            .child(key_chip("E", p))
            .child(
                div()
                    .pl(px(4.))
                    .text_size(px(11.))
                    .text_color(to_hsla(p.muted_foreground))
                    .child(suffix),
            )
            .into_any_element()
    }

    fn nav(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let needle = self.needle(cx);
        let has_query = !needle.is_empty();
        v_flex()
            .w(px(NAV_W))
            .flex_none()
            .h_full()
            .p(px(10.))
            .bg(to_hsla(p.dialog_surface))
            .border_r(px(ui::HAIRLINE))
            .border_color(to_hsla(p.border_soft))
            .child(self.search_box(has_query, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(2.))
                    .overflow_hidden()
                    .children(
                        (0..PAGES.len())
                            .filter(|page| self.page_matches(*page, &needle))
                            .map(|page| {
                                let open = self.nav_open[page] || has_query;
                                let selected = self.page == page && self.section.is_none();
                                v_flex()
                                    .gap(px(1.))
                                    .child(
                                        h_flex()
                                            .id(format!("options-page-{page}"))
                                            .test_support()
                                            .h(px(28.))
                                            .w_full()
                                            .pl(px(2.))
                                            .pr(px(4.))
                                            .gap(px(8.))
                                            .items_center()
                                            .rounded(px(ui::RADIUS))
                                            .border(px(ui::HAIRLINE))
                                            .border_color(if selected {
                                                to_hsla(p.border_soft)
                                            } else {
                                                to_hsla(p.dialog_surface)
                                            })
                                            .when(selected, move |s| s.bg(to_hsla(p.selection)))
                                            .when(!selected, move |s| {
                                                s.hover(move |s| s.bg(to_hsla(p.hover)))
                                            })
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.open_page(page, cx);
                                            }))
                                            .child(
                                                div()
                                                    .id(format!("options-group-{page}"))
                                                    .test_support()
                                                    .size(px(16.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .rounded(px(3.))
                                                    .hover(move |s| s.bg(to_hsla(p.hover)))
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.nav_open[page] =
                                                                !this.nav_open[page];
                                                            cx.stop_propagation();
                                                            cx.notify();
                                                        },
                                                    ))
                                                    .child(zed_icons::ic(
                                                        if open {
                                                            ZedIcon::ChevronDown
                                                        } else {
                                                            ZedIcon::ChevronRight
                                                        },
                                                        12.,
                                                        to_hsla(p.muted_foreground),
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .text_color(to_hsla(if selected {
                                                        p.foreground
                                                    } else {
                                                        p.muted_foreground
                                                    }))
                                                    .child(PAGES[page]),
                                            ),
                                    )
                                    .when(open, |this| {
                                        this.child(
                                            v_flex().gap(px(1.)).children(
                                                self.visible_sections(page, &needle).into_iter().map(
                                                    |sec| {
                                                        let selected = self.section == Some(sec);
                                                        h_flex()
                                                            .id(format!("options-nav-{sec}"))
                                                            .test_support()
                                                            .h(px(28.))
                                                            .w_full()
                                                            .pl(px(2.))
                                                            .pr(px(4.))
                                                            .gap(px(8.))
                                                            .items_center()
                                                            .rounded(px(ui::RADIUS))
                                                            .border(px(ui::HAIRLINE))
                                                            .border_color(if selected {
                                                                to_hsla(p.border_soft)
                                                            } else {
                                                                to_hsla(p.dialog_surface)
                                                            })
                                                            .when(selected, move |s| {
                                                                s.bg(to_hsla(p.selection))
                                                            })
                                                            .when(!selected, move |s| {
                                                                s.hover(move |s| {
                                                                    s.bg(to_hsla(p.hover))
                                                                })
                                                            })
                                                            .cursor_pointer()
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    this.open_section(sec, cx);
                                                                },
                                                            ))
                                                            .child(
                                                                div()
                                                                    .w(px(22.))
                                                                    .h_full()
                                                                    .flex_none()
                                                                    .flex()
                                                                    .justify_center()
                                                                    .child(
                                                                        div()
                                                                            .w(px(1.))
                                                                            .h_full()
                                                                            .bg(to_hsla(
                                                                                p.border_soft,
                                                                            )),
                                                                    ),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(px(13.))
                                                                    .text_color(to_hsla(
                                                                        if selected {
                                                                            p.foreground
                                                                        } else {
                                                                            p.muted_foreground
                                                                        },
                                                                    ))
                                                                    .child(SECTIONS[sec].0),
                                                            )
                                                    },
                                                ),
                                            ),
                                        )
                                    })
                            }),
                    ),
            )
            .child(self.nav_footer(window, cx))
    }

    /// 一行设置：左侧标题 / 说明，右侧控件；行间一条发丝线。
    fn row(&self, row: &RowDef, last: bool, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let desc = match row.kind {
            RowKind::DataDir => crate::store_path()
                .parent()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default(),
            RowKind::Version => format!("daro-rs {}", env!("CARGO_PKG_VERSION")),
            _ => row.desc.to_string(),
        };
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .gap(px(24.))
            .px(px(32.))
            .pt(px(16.))
            .pb(px(if last { 40. } else { 16. }))
            .when(!last, |s| {
                s.border_b(px(ui::HAIRLINE))
                    .border_color(to_hsla(p.border_soft))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(to_hsla(p.foreground))
                            .child(row.title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(to_hsla(p.muted_foreground))
                            .child(desc),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(self.row_control(row, cx)),
            )
            .into_any_element()
    }

    fn row_control(&self, row: &RowDef, cx: &mut Context<Self>) -> AnyElement {
        match row.kind {
            RowKind::DataDir => ui::compact_button("options-open-data-dir", "打开目录")
                .on_click(|_, _, _| {
                    if let Some(dir) = crate::store_path().parent() {
                        if let Err(e) = open::that(dir) {
                            eprintln!("[options_dialog] 打开数据目录失败：{e}");
                        }
                    }
                })
                .into_any_element(),
            RowKind::Version => div().into_any_element(),
            RowKind::Theme => self.theme_control(cx),
            RowKind::UiFont => self.ui_font_control(cx),
            RowKind::ShowTree => Self::switch(
                self.draft.show_tree,
                "options-show-tree",
                self.dark,
                |draft, on| draft.show_tree = on,
                cx,
            ),
            RowKind::ShowInfo => Self::switch(
                self.draft.show_info,
                "options-show-info",
                self.dark,
                |draft, on| draft.show_info = on,
                cx,
            ),
            RowKind::ShowMenus => Self::switch(
                self.draft.show_menus,
                "options-show-menus",
                self.dark,
                |draft, on| draft.show_menus = on,
                cx,
            ),
            RowKind::ObjectView => self.object_view_control(cx),
            RowKind::RunPane => self.run_pane_control(cx),
        }
    }

    /// 主题清单：Zed 三族（One / Ayu / Gruvbox）十一档 + VS Code 2026 明暗两档，
    /// 深浅跟着主题走。
    /// 「确定」才应用；顶栏月亮 / 太阳仍是最快的明暗切换。
    fn theme_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let me = cx.entity().clone();
        let current_ix = self.draft.theme.min(palette::THEMES.len() - 1);
        Button::new("options-theme")
            .label(format!("{} ▾", palette::THEMES[current_ix].name))
            .h(px(22.))
            .px(px(8.))
            .text_size(px(12.))
            .max_w(px(220.))
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.max_h(px(320.)).scrollable(true);
                for (ix, theme) in palette::THEMES.iter().enumerate() {
                    let me = me.clone();
                    menu = menu.item(
                        PopupMenuItem::new(theme.name)
                            .checked(ix == current_ix)
                            .on_click(move |_, _, cx| {
                                me.update(cx, |this, cx| {
                                    if this.draft.theme != ix {
                                        this.draft.theme = ix;
                                        this.draft.dark = palette::THEMES[ix].dark;
                                        cx.notify();
                                    }
                                });
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// 界面字体下拉：列本机已安装字体（竖排 CJK 变体不列），当前项打勾。
    /// 只改 `Theme::font_family`；SQL 编辑器 / 控制台仍用等宽字体。
    fn ui_font_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let me = cx.entity().clone();
        let fonts = self.ui_fonts.clone();
        let current = self.draft.ui_font.clone();
        Button::new("options-ui-font")
            .label(format!("{current} ▾"))
            .h(px(22.))
            .px(px(8.))
            .text_size(px(12.))
            .max_w(px(240.))
            .dropdown_menu(move |menu, _, _| {
                // max_h 只在 scrollable 时生效：不加会在屏幕上铺开整份字体清单。
                let mut menu = menu.max_h(px(320.)).scrollable(true);
                for name in &fonts {
                    let me = me.clone();
                    let picked = name.to_string();
                    menu = menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(*name == current)
                            .on_click(move |_, _, cx| {
                                let picked = picked.clone();
                                me.update(cx, |this, cx| {
                                    if this.draft.ui_font != picked {
                                        this.draft.ui_font = picked;
                                        cx.notify();
                                    }
                                });
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    fn object_view_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let grid = cx.entity().clone();
        let list = cx.entity().clone();
        h_flex()
            .gap(px(2.))
            .child(
                ui::segment(
                    "options-object-view-grid",
                    "图标",
                    !self.draft.list_view,
                    self.dark,
                    move |_, _, app| {
                        grid.update(
                            app,
                            |this: &mut OptionsDialog, cx: &mut Context<OptionsDialog>| {
                                if this.draft.list_view {
                                    this.draft.list_view = false;
                                    cx.notify();
                                }
                            },
                        );
                    },
                )
                .test_support(),
            )
            .child(
                ui::segment(
                    "options-object-view-list",
                    "列表",
                    self.draft.list_view,
                    self.dark,
                    move |_, _, app| {
                        list.update(
                            app,
                            |this: &mut OptionsDialog, cx: &mut Context<OptionsDialog>| {
                                if !this.draft.list_view {
                                    this.draft.list_view = true;
                                    cx.notify();
                                }
                            },
                        );
                    },
                )
                .test_support(),
            )
            .into_any_element()
    }

    fn run_pane_control(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap(px(2.))
            .children((0..3).map(|ix| {
                let me = cx.entity().clone();
                let label = match ix {
                    0 => "消息",
                    1 => "摘要",
                    _ => "结果/解释",
                };
                ui::segment(
                    match ix {
                        0 => "options-run-message",
                        1 => "options-run-summary",
                        _ => "options-run-result",
                    },
                    label,
                    self.draft.run_pane == ix,
                    self.dark,
                    move |_, _, app| {
                        me.update(app, |this: &mut OptionsDialog, cx: &mut Context<OptionsDialog>| {
                            if this.draft.run_pane != ix {
                                this.draft.run_pane = ix;
                                cx.notify();
                            }
                        });
                    },
                )
                .test_support()
            }))
            .into_any_element()
    }

    fn switch(
        value: bool,
        id: &'static str,
        dark: bool,
        set: impl Fn(&mut AppOptions, bool) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let me = cx.entity().clone();
        Switch::new(id)
            .checked(value)
            .color(to_hsla(Palette::of(dark).control_fill))
            .on_change(move |checked, _, cx| {
                me.update(cx, |this, cx| {
                    set(&mut this.draft, *checked);
                    cx.notify();
                });
            })
            .into_any_element()
    }

    fn section_block(
        &self,
        sec: usize,
        rows: &[&'static RowDef],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.pal();
        let count = rows.len();
        v_flex()
            .w_full()
            .child(
                v_flex()
                    .px(px(32.))
                    .gap(px(6.))
                    .child(
                        // Zed 的小节标题用 buffer 字体（等宽）12px、次要色。
                        div()
                            .font_family("Consolas")
                            .text_size(px(12.))
                            .text_color(to_hsla(p.muted_foreground))
                            .child(SECTIONS[sec].0),
                    )
                    .child(div().w_full().h(px(ui::HAIRLINE)).bg(to_hsla(p.border_soft))),
            )
            .children(
                rows.iter()
                    .enumerate()
                    .map(|(ix, row)| self.row(row, ix + 1 == count, cx)),
            )
            .into_any_element()
    }

    fn content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let needle = self.needle(cx);
        let body = if self.page == 4 {
            div()
                .id("options-mcp-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(v_flex().p(px(24.)).child(self.mcp.clone()))
                .into_any_element()
        } else {
            let sections = self.visible_sections(self.page, &needle);
            let mut list = v_flex()
                .id("options-content")
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.content_scroll)
                .track_focus(&self.content_focus)
                .pt(px(24.))
                .pb(px(24.))
                .gap(px(16.))
                .child(
                    div().px(px(32.)).child(
                        div()
                            .mt(px(8.))
                            .mb(px(12.))
                            .text_size(px(16.))
                            .text_color(to_hsla(p.foreground))
                            .child(PAGES[self.page]),
                    ),
                );
            if sections.is_empty() {
                list = list.child(
                    div()
                        .px(px(32.))
                        .text_size(px(12.))
                        .text_color(to_hsla(p.muted_foreground))
                        .child("没有匹配的设置"),
                );
            } else {
                for sec in sections {
                    let rows = self.visible_rows(sec, &needle);
                    list = list.child(self.section_block(sec, &rows, cx));
                }
            }
            list.into_any_element()
        };

        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(to_hsla(p.background))
            .child(body)
    }
}

fn key_chip(label: &'static str, p: Palette) -> Div {
    div()
        .h(px(16.))
        .px(px(4.))
        .flex()
        .items_center()
        .rounded(px(3.))
        .border(px(ui::HAIRLINE))
        .border_color(to_hsla(p.border))
        .text_size(px(10.))
        .text_color(to_hsla(p.muted_foreground))
        .child(label)
}

impl Render for OptionsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        v_flex()
            .id("options-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(to_hsla(p.foreground))
            .child(self.title_bar())
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.nav(window, cx))
                    .child(self.content(cx)),
            )
            .child(
                ui::footer_bar(self.dark)
                    .child(div().flex_1())
                    .child(
                        ui::primary_button("options-ok", "确定", self.dark, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
                    )
                    .child(
                        ui::secondary_button("options-cancel", "取消")
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}
