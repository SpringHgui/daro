//! 「帮助 → 关于...」弹窗（迁移自 Flutter 版 `lib/widgets/about_dialog.dart`）。
//!
//! 启动画面式版面：居中 Logo + 名称 + 副标题 + 版本 + 项目主页（双平台）+ 问题反馈 +
//! 版权。版本号取 `CARGO_PKG_VERSION`（不要在此硬编码）。链接点击用系统默认
//! 应用打开（`open` crate），启动失败仅记录日志（不做静默兜底）。

use gpui_kit::component::*;
use gpui_kit::*;

use crate::db_icons;
use crate::palette::Palette;
use crate::ui;
use crate::{to_hsla, DaroApp};

/// 项目主页：同一仓库在 GitHub / Gitee 的双平台镜像，两个入口都摆在「关于」里。
pub const GITHUB_HOME: &str = "https://github.com/SpringHgui/daro";
const GITEE_HOME: &str = "https://gitee.com/SpringHgui/daro";
/// 问题反馈：走 GitHub Issues（与菜单「帮助 → 提供 daro 反馈意见...」指向同一地址）。
const ISSUES_URL: &str = "https://github.com/SpringHgui/daro/issues";

/// 菜单「帮助 → 提供 daro 反馈意见...」：直接用系统默认浏览器打开 GitHub Issues。
pub fn open_issue_tracker() {
    open_link(ISSUES_URL);
}

pub fn open_daro_about(parent: Entity<DaroApp>, dark: bool, cx: &mut App) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(360.), px(320.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(400.), px(360.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("关于 daro".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            let esc = ui::close_on_escape(window, cx);
            AboutDialog {
                parent,
                dark,
                _esc: esc,
            }
        })
    });
    if opened.is_err() {
        eprintln!("failed to open about window");
    }
}

struct AboutDialog {
    parent: Entity<DaroApp>,
    dark: bool,
    _esc: Subscription,
}

impl AboutDialog {
    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    fn title_bar(&self) -> impl IntoElement {
        let p = self.pal();
        TitleBar::new().h(px(crate::ui::TITLE_H)).border_b_0().child(
            h_flex()
                .h_full()
                .items_center()
                .gap(px(6.))
                .pl(px(4.))
                .child(
                    div()
                        .text_xs()
                        .text_color(to_hsla(p.foreground))
                        .child("关于 daro"),
                ),
        )
    }
}

impl Render for AboutDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let (bg, fg, muted, border, _chrome) = (
            to_hsla(p.chrome),
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.border),
            to_hsla(p.chrome),
        );
        let dark = self.dark;
        let version = env!("CARGO_PKG_VERSION");

        let logo = div()
            .w(px(96.))
            .h(px(96.))
            .rounded(px(22.))
            .bg(to_hsla(p.dialog_surface))
            .border_1()
            .border_color(border)
            .child(
                div()
                    .flex()
                    .w_full()
                    .h_full()
                    .items_center()
                    .justify_center()
                    .child(db_icons::logo(52.)),
            );

        let link_btn = |label: &'static str, url: &'static str| {
            ui::link_button(label, label, dark)
                .on_click(cx.listener(move |_, _, _, _cx| {
                    open_link(url);
                }))
        };

        v_flex()
            .id("about-root")
            .size_full()
            .bg(bg)
            .text_color(fg)
            .child(self.title_bar())
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .py(px(20.))
                    .child(logo)
                    .child(
                        div()
                            .text_size(px(38.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(fg)
                            .child("daro"),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(muted)
                            .child("桌面数据库管理工具"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("版本 {version}")),
                    )
                    .child(
                        h_flex()
                            .mt(px(10.))
                            .gap(px(8.))
                            .child(link_btn("GitHub", GITHUB_HOME))
                            .child(link_btn("Gitee", GITEE_HOME))
                            .child(link_btn("问题反馈", ISSUES_URL)),
                    ),
            )
            .child(
                ui::footer_bar(dark)
                    .child(
                        ui::secondary_button("about-update", "检查更新")
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.remove_window();
                                crate::update_dialog::open_center(
                                    this.parent.clone(),
                                    this.dark,
                                    cx,
                                );
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::secondary_button("about-close", "关闭")
                            .on_click(cx.listener(|_, _, window, _cx| {
                                window.remove_window();
                            })),
                    ),
            )
    }
}

/// 用系统默认应用打开链接；失败（无默认浏览器等）仅记录，避免点击无反馈。
pub fn open_link(url: &str) {
    if let Err(e) = open::that(url) {
        eprintln!("[about_dialog] 打开链接失败 {url}: {e}");
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
    fn about_dialog_closes_on_escape_without_focus(cx: &mut TestAppContext) {
        isolate_store("aboutesc");
        let (_main, app) = open_app(cx);
        let handle = cx
            .update(|cx| {
                open_daro_about(app, false, cx);
                cx.windows().last().copied()
            })
            .expect("about window opened");
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press("escape", cx);
        })
        .unwrap();
        assert_eq!(cx.windows().len(), 1, "无聚焦元素时 Esc 也要关窗");
    }
}
