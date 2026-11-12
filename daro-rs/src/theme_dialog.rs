//! 「选择主题」浮层：照 Zed 的 Select Theme 命令面板——一张无标题栏的卡片，
//! 顶上一行平底搜索框，下面一列主题。当前主题右侧打勾，悬停一行即预览，
//! 点一行落盘收窗；Esc / 点回主窗则退回开窗那一档。
//!
//! 主题清单与色板在 [`crate::palette`]；这里只做选择界面。

use std::time::Duration;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Sizable as _, Size as ComponentSize};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::palette::{self, Palette};
use crate::zed_icons::{self, ZedIcon};
use crate::{DaroApp, to_hsla, ui};

/// Zed 的命令面板宽 34rem（16px 基准）。
const WIDTH: f32 = 544.;
/// 搜索行：Zed 的 h_9。
const SEARCH_H: f32 = 36.;
/// 列表行：Zed 的 ListItem Sparse。
const ROW_H: f32 = 28.;
/// 列表四周留白：Zed 的 uniform_list py_1 + 行 inset。
const LIST_PAD: f32 = 4.;
/// 浮层离主窗顶边的距离：Zed 的命令面板压在编辑器上方。
const TOP_GAP: f32 = 72.;
/// 卡片圆角：Zed 的 `rounded_lg`（0.5rem）。
const CORNER_R: f32 = 8.;

fn content_height(rows: usize) -> f32 {
    SEARCH_H + ui::HAIRLINE + LIST_PAD * 2. + rows as f32 * ROW_H
}

pub(crate) fn open_theme_dialog(
    parent: Entity<DaroApp>,
    dark: bool,
    anchor: &Window,
    cx: &mut App,
) {
    let height = content_height(palette::THEMES.len());
    let host = anchor.window_bounds().get_bounds();
    let offset = (
        ((host.size.width - px(WIDTH)) * 0.5).max(px(0.)).as_f32(),
        TOP_GAP,
    );

    let mut options = WindowOptions::default();
    options.window_bounds = Some(WindowBounds::Windowed(Bounds::new(
        host.origin + point(px(offset.0), px(offset.1)),
        size(px(WIDTH), px(height)),
    )));
    // 无标题栏置顶浮层（同下拉列表）：不占任务栏，Esc / 点选即关；
    // 尺寸跟着过滤结果走，见 `ThemeDialog` 的搜索订阅。
    options.titlebar = None;
    options.kind = WindowKind::PopUp;
    options.focus = true;
    options.is_resizable = false;
    options.is_movable = false;
    options.is_minimizable = false;

    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| ThemeDialog::new(parent, dark, window, cx))
    });
    let Ok((handle, _)) = opened else {
        eprintln!("failed to open theme window");
        return;
    };
    // 建窗 bounds 被平台按外框解释，客户区会偏半圈边框；按锚窗口客户区再贴一次。
    // 卡片按 Zed 的命令面板做圆角，工具窗默认方角，得让 DWM 现补一次圆角。
    let _ = cx.update_window(handle, |_, window, _| {
        ui::place_client_at_anchor(window, anchor, offset);
        ui::round_corners(window);
    });
}

struct ThemeDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    /// 当前预览档的配色：本窗只按它重画，不跟主窗的全局色板抢帧
    pal: Palette,
    /// 开窗时那一档：悬浮预览移开 / 取消时退回它，右侧的勾也一直停在它身上
    original: usize,
    /// 已点选并落盘；收窗不再退回
    committed: bool,
    search: Entity<InputState>,
    _esc: Subscription,
    _release: Subscription,
}

impl ThemeDialog {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let pal = Palette::of(dark);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("选择主题..."));
        let this_window = window.window_handle();
        cx.subscribe(&search, move |this: &mut Self, _: Entity<InputState>, ev: &InputEvent, cx| {
            match ev {
                InputEvent::Change => {
                    // 过滤后原来悬停的那行可能已经不在，预览退回开窗档；
                    // 浮层高度跟着结果条数收放（Zed 的 picker 同样按内容缩）
                    this.revert_preview(cx);
                    let rows = this.matched(cx).max(1);
                    let _ = cx.update_window(this_window, |_, window, _| {
                        window.resize(size(px(WIDTH), px(content_height(rows))));
                    });
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    this.commit(palette::active_index(), cx);
                    let _ =
                        cx.update_window(this_window, |_, window, _| window.remove_window());
                }
                _ => {}
            }
        })
        .detach();
        // 开窗直接落在搜索框，键入即筛
        let focus = search.clone();
        cx.defer_in(window, move |_, window, cx| {
            focus.update(cx, |state, cx| state.focus(window, cx));
        });
        // 搜索框聚焦时 InputState 会把 Esc 当动作吃掉，冒泡阶段收不到；
        // 按键拦截器跑在动作派发之前，按窗口 id 限定只关本窗。
        let esc = cx.intercept_keystrokes(move |ev, window, _cx| {
            if ev.keystroke.key == "escape" && window.window_handle() == this_window {
                window.remove_window();
            }
        });
        // 没有点选就收窗（Esc / 点回主窗 / 直接关窗）：悬停预览退回开窗那一档
        let release = cx.on_release(|this, cx| {
            if this.committed || palette::active_index() == this.original {
                return;
            }
            let original = this.original;
            this.parent
                .update(cx, |app, cx| app.preview_theme(original, cx));
        });
        // 浮层失活（点了主窗 / 切到别的应用）就收起，和 Zed 的命令面板一致
        cx.observe_window_activation(window, |_, window, cx| {
            if window.is_window_active() {
                return;
            }
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let _ = this.update_in(cx, |_, window, _| {
                    if !window.is_window_active() {
                        window.remove_window();
                    }
                });
            })
            .detach();
        })
        .detach();
        Self {
            parent,
            dark,
            pal,
            original: palette::active_index(),
            committed: false,
            search,
            _esc: esc,
            _release: release,
        }
    }

    /// 搜索词过滤后的条数（与 `render` 同一口径）。
    fn matched(&self, cx: &App) -> usize {
        let needle = self.search.read(cx).value().trim().to_lowercase();
        palette::THEMES
            .iter()
            .filter(|t| needle.is_empty() || t.name.to_lowercase().contains(&needle))
            .count()
    }

    /// 悬停某一行：立即把主窗换成这一档看效果，不落盘。
    fn preview(&mut self, ix: usize, cx: &mut Context<Self>) {
        if palette::active_index() == ix {
            return;
        }
        self.parent.update(cx, |app, cx| app.preview_theme(ix, cx));
        let theme = &palette::THEMES[ix];
        self.dark = theme.dark;
        self.pal = theme.palette;
        cx.notify();
    }

    /// 指针离开清单 / 改搜索词时退回开窗那一档。
    fn revert_preview(&mut self, cx: &mut Context<Self>) {
        if palette::active_index() == self.original {
            return;
        }
        self.parent
            .update(cx, |app, cx| app.preview_theme(self.original, cx));
        let theme = &palette::THEMES[self.original];
        self.dark = theme.dark;
        self.pal = theme.palette;
        cx.notify();
    }

    /// 点选 / 回车：这一档落盘，之后收窗不再退回。
    fn commit(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.committed = true;
        self.parent.update(cx, |app, cx| app.set_theme(ix, cx));
    }

    fn row(&self, ix: usize, name: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal;
        let on = ix == palette::active_index();
        // 勾停在开窗那一档：悬停换的是高亮行，不是已选主题
        let checked = ix == self.original;
        h_flex()
            .id(format!("theme-row-{ix}"))
            .test_support()
            .h(px(ROW_H))
            .flex_none()
            .px(px(6.))
            .gap(px(6.))
            .items_center()
            .rounded(px(ui::RADIUS))
            .cursor_pointer()
            .text_size(px(13.))
            .text_color(to_hsla(p.foreground))
            .when(on, |s| s.bg(to_hsla(p.selection)))
            .when(!on, |s| s.hover(move |s| s.bg(to_hsla(p.hover))))
            // 悬浮即预览：主窗和本窗一起换成这一档，点下去才落盘
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.preview(ix, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.commit(ix, cx);
                window.remove_window();
            }))
            .child(div().flex_1().min_w_0().truncate().child(name))
            .child(
                div()
                    .w(px(14.))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .when(checked, |s| {
                        s.child(zed_icons::ic(
                            ZedIcon::Check,
                            12.,
                            to_hsla(p.muted_foreground),
                        ))
                    }),
            )
            .into_any_element()
    }
}

impl Render for ThemeDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal;
        let needle = self.search.read(cx).value().trim().to_lowercase();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (ix, theme) in palette::THEMES.iter().enumerate() {
            if !needle.is_empty() && !theme.name.to_lowercase().contains(&needle) {
                continue;
            }
            rows.push(self.row(ix, theme.name, cx));
        }

        v_flex()
            .id("theme-dialog-root")
            .size_full()
            .bg(to_hsla(p.dialog_surface))
            .text_color(to_hsla(p.foreground))
            // Zed 的 elevation_3：圆角卡片 + 1px border_variant 描边
            .rounded(px(CORNER_R))
            .border_1()
            .border_color(to_hsla(p.border_soft))
            .overflow_hidden()
            .child(
                h_flex()
                    .id("theme-search")
                    .test_support()
                    .h(px(SEARCH_H))
                    // 输入框自己带 6px 内边距，这里只补到与行文字同一条左边线
                    .pl(px(4.))
                    .pr(px(10.))
                    .flex_shrink_0()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.search)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false)
                                .with_size(ComponentSize::Medium),
                        ),
                    ),
            )
            .child(
                div()
                    .h(px(ui::HAIRLINE))
                    .flex_shrink_0()
                    .bg(to_hsla(p.border_soft)),
            )
            .child(
                v_flex()
                    .id("theme-list")
                    .flex_1()
                    .min_h_0()
                    .px(px(LIST_PAD))
                    .py(px(LIST_PAD))
                    // 指针离开清单（去搜索框）：预览退回开窗那一档
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !*hovered {
                            this.revert_preview(cx);
                        }
                    }))
                    .when(rows.is_empty(), |s| {
                        s.child(
                            h_flex()
                                .id("theme-empty")
                                .test_support()
                                .h(px(ROW_H))
                                .flex_none()
                                .px(px(6.))
                                .items_center()
                                .text_size(px(13.))
                                .text_color(to_hsla(p.muted_foreground))
                                .child("没有匹配的主题。"),
                        )
                    })
                    .children(rows),
            )
    }
}
