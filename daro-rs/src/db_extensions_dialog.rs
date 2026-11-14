//! 数据库扩展管理对话框（库节点右键「管理扩展...」入口）。
//!
//! 仅 PostgreSQL 有数据库扩展的概念。上半段是已安装清单（每行可卸载），
//! 下半段是可安装清单（每行可安装）。安装 / 卸载在后台执行，回执后就地重查
//! 清单；动作进行中置灰按钮，防连点。

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::backend;
use crate::palette::Palette;
use crate::prompt_dialog::open_notice;
use crate::ui;
use crate::to_hsla;

/// 打开扩展管理窗。`target` 绑定到目标库，安装 / 卸载 / 重查都走它。
pub fn open(
    dark: bool,
    target: backend::Target,
    lists: backend::ExtensionLists,
    cx: &mut App,
) {
    let mut options = ui::dialog_options();
    options.window_min_size = Some(size(px(460.), px(380.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(520.), px(560.)), cx));
    if let Some(tb) = options.titlebar.as_mut() {
        tb.title = Some("管理扩展".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            let esc = ui::close_on_escape(window, cx);
            DbExtensionsDialog {
                dark,
                target,
                lists,
                busy: false,
                _esc: esc,
            }
        })
    });
    if opened.is_err() {
        eprintln!("[db_extensions_dialog] 打开扩展管理窗失败");
    }
}

struct DbExtensionsDialog {
    dark: bool,
    /// 绑定到目标库的执行目标：安装 / 卸载 / 重查都走它
    target: backend::Target,
    /// 已安装 + 可安装两份清单（开窗前由后台拉好）
    lists: backend::ExtensionLists,
    /// 动作在后台跑时置灰按钮，防连点
    busy: bool,
    _esc: Subscription,
}

impl DbExtensionsDialog {
    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    /// 动作回执后就地重查清单。两条目录小查询，同步跑与树刷新同口径。
    fn reload(&mut self, cx: &mut Context<Self>) {
        match self.target.list_extensions() {
            Ok(lists) => self.lists = lists,
            Err(e) => open_notice(self.dark, "管理扩展", e, cx),
        }
        cx.notify();
    }

    /// 安装一个扩展：后台跑 `CREATE EXTENSION`，回执后重查。
    fn install(&mut self, name: String, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let target = self.target.clone();
        let ent = cx.entity().downgrade();
        let dark = self.dark;
        cx.spawn(async move |_, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { target.create_extension(&name) })
                .await;
            if let Some(this) = ent.upgrade() {
                this.update(cx, |this, cx| {
                    this.busy = false;
                    match res {
                        Ok(()) => this.reload(cx),
                        Err(e) => open_notice(dark, "安装扩展失败", e, cx),
                    }
                });
            }
        })
        .detach();
    }

    /// 卸载一个扩展：后台跑 `DROP EXTENSION`，回执后重查。
    /// plpgsql 等核心扩展服务器会拒绝，报错原样透出。
    fn uninstall(&mut self, name: String, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let target = self.target.clone();
        let ent = cx.entity().downgrade();
        let dark = self.dark;
        cx.spawn(async move |_, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { target.drop_extension(&name) })
                .await;
            if let Some(this) = ent.upgrade() {
                this.update(cx, |this, cx| {
                    this.busy = false;
                    match res {
                        Ok(()) => this.reload(cx),
                        Err(e) => open_notice(dark, "卸载扩展失败", e, cx),
                    }
                });
            }
        })
        .detach();
    }

    fn title_bar(&self) -> impl IntoElement {
        let p = self.pal();
        TitleBar::new()
            .h(px(ui::TITLE_H))
            .border_b_0()
            .child(
                h_flex()
                    .h_full()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(4.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(to_hsla(p.foreground))
                            .child("管理扩展"),
                    ),
            )
    }

    /// 小节标题：名字 + 数量角标。
    fn section(&self, label: &str, count: usize) -> AnyElement {
        let p = self.pal();
        h_flex()
            .items_center()
            .gap(px(6.))
            .pt(px(6.))
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(to_hsla(p.foreground))
                    .child(label.to_string()),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(to_hsla(p.subtle_foreground))
                    .child(count.to_string()),
            )
            .into_any_element()
    }
}

impl Render for DbExtensionsDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.pal();
        let fg = to_hsla(p.foreground);
        let muted = to_hsla(p.muted_foreground);
        let hover = to_hsla(p.hover);
        let busy = self.busy;

        let installed: Vec<AnyElement> = self
            .lists
            .installed
            .iter()
            .enumerate()
            .map(|(i, (name, ver))| {
                h_flex()
                    .id(format!("dbext-installed-{i}"))
                    .h(px(30.))
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .hover(move |s| s.bg(hover))
                    .child(div().text_size(px(12.5)).text_color(fg).child(name.clone()))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(muted)
                            .child(format!("v{ver}")),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::compact_button(format!("dbext-un-{i}"), "卸载")
                            .disabled(busy)
                            .on_click(cx.listener({
                                let name = name.clone();
                                move |this, _, _, cx| this.uninstall(name.clone(), cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect();

        let available: Vec<AnyElement> = self
            .lists
            .available
            .iter()
            .enumerate()
            .map(|(i, (name, ver))| {
                h_flex()
                    .id(format!("dbext-available-{i}"))
                    .h(px(30.))
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .hover(move |s| s.bg(hover))
                    .child(div().text_size(px(12.5)).text_color(fg).child(name.clone()))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(muted)
                            .child(format!("默认 v{ver}")),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::compact_button(format!("dbext-in-{i}"), "安装")
                            .disabled(busy)
                            .on_click(cx.listener({
                                let name = name.clone();
                                move |this, _, _, cx| this.install(name.clone(), cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect();

        v_flex()
            .id("dbext-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(fg)
            .child(self.title_bar())
            .child(
                v_flex()
                    .id("dbext-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(12.))
                    .gap(px(4.))
                    .child(self.section("已安装", self.lists.installed.len()))
                    .when(installed.is_empty(), |s| {
                        s.child(
                            div()
                                .py(px(6.))
                                .px(px(8.))
                                .text_size(px(11.5))
                                .text_color(muted)
                                .child("库中尚未安装任何扩展。"),
                        )
                    })
                    .children(installed)
                    .child(self.section("可安装", self.lists.available.len()))
                    .when(available.is_empty(), |s| {
                        s.child(
                            div()
                                .py(px(6.))
                                .px(px(8.))
                                .text_size(px(11.5))
                                .text_color(muted)
                                .child("所有可用扩展均已安装。"),
                        )
                    })
                    .children(available),
            )
            .child(
                ui::footer_bar(self.dark)
                    .child(div().flex_1())
                    .child(
                        ui::secondary_button("dbext-close", "关闭")
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}
