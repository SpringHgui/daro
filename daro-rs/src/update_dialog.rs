//! 更新中心 / 下载安装窗口（版式参考 DBX 的更新中心）。
//!
//! 启动时静默查一次：有新版本只在标题栏挂「更新」按钮（见 update_badge），
//! 不弹窗；「帮助 → 检查更新...」直接开更新中心并开查。上排标签切换
//! 「daro / 更新日志」，底栏按阶段给下载、取消、重试等动作；查到的结果
//! 也会同步给标题栏按钮。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::palette::Palette;
use crate::ui;
use crate::update::{self, Catalog, DownloadProgress, Latest, ReleaseInfo};
use crate::{to_hsla, DaroApp};

/// 启动时静默检查：有新版本且没被「不再提示」就在标题栏挂出「更新」按钮；
/// 出错只记日志，不打扰启动。
pub fn check_on_startup(parent: Entity<DaroApp>, cx: &mut App) {
    let (tx, rx) = async_channel::bounded(1);
    let current = env!("CARGO_PKG_VERSION").to_string();
    std::thread::spawn(move || {
        let _ = tx.send_blocking(update::check(&current));
    });
    cx.spawn(async move |cx| {
        let Ok(result) = rx.recv().await else {
            return;
        };
        let _ = cx.update(|cx| match result {
            Ok(catalog) => {
                let Some(latest) = catalog.latest else {
                    return;
                };
                parent.update(cx, |app, cx| {
                    if app.skipped_version() == latest.version {
                        return;
                    }
                    app.set_available_update(latest, cx);
                });
            }
            Err(e) => eprintln!("[update] 启动检查更新失败：{e}"),
        });
    })
    .detach();
}

/// 把更新中心直接停到「询问」态（测试用；正常入口是标题栏的「更新」按钮）。
#[cfg(test)]
pub fn prompt(
    parent: Entity<DaroApp>,
    dark: bool,
    latest: Latest,
    releases: Vec<ReleaseInfo>,
    cx: &mut App,
) {
    open_window(parent, dark, Phase::Ask, Some(latest), releases, true, cx);
}

/// 「帮助 → 检查更新...」：开更新中心并立即查一次。
pub fn open_center(parent: Entity<DaroApp>, dark: bool, cx: &mut App) {
    open_window(parent, dark, Phase::Checking, None, Vec::new(), false, cx);
}

/// 从「关于」或启动检查弹出来时，主窗可能刚拿回焦点：主动抢一次，
/// 否则窗口会落在主窗后面，看起来像什么都没发生。
fn open_window(
    parent: Entity<DaroApp>,
    dark: bool,
    phase: Phase,
    latest: Option<Latest>,
    releases: Vec<ReleaseInfo>,
    from_startup: bool,
    cx: &mut App,
) {
    let mut options = ui::dialog_options();
    // 不能用 Dialog：gpui 会把对话框挂到当前活动窗下并禁用父窗，而旧弹窗的
    // Win32 销毁是异步的。从「关于」点检查更新时旧窗还没消失，新窗会被当成
    // 它的子窗一起销毁，下载随即取消——这正是旧版“点更新没反应”的原因。
    options.kind = WindowKind::Normal;
    options.window_min_size = Some(size(px(560.), px(400.)));
    options.window_bounds = Some(WindowBounds::centered(size(px(760.), px(560.)), cx));
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some("更新中心".into());
    }
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        window.activate_window();
        let entry = cx.new(|_| {
            UpdateWindow::new(parent, dark, phase, latest, releases, from_startup)
        });
        if phase == Phase::Checking {
            let entry = entry.clone();
            window.defer(cx, move |_, cx| {
                let _ = entry.update(cx, |this, cx| this.start_check(cx));
            });
        }
        entry
    });
    if opened.is_err() {
        eprintln!("[update] failed to open update center window");
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// 正在查 GitHub 发布
    Checking,
    /// 发现新版本，等用户点「立即更新」
    Ask,
    /// 下载 / 安装进行中
    Working,
    /// 用户点了取消，等下载线程收尾
    Cancelling,
    /// 装完了，马上重启
    Restarting,
    /// 出错，只能手动下载或重试
    Failed,
    /// 查过了，没有比当前新的版本
    UpToDate,
    /// 检查本身失败（网络 / API）
    CheckFailed,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    App,
    Changelog,
}

struct UpdateWindow {
    parent: Entity<DaroApp>,
    dark: bool,
    tab: Tab,
    /// 查到 / 传入的新版本；没有更新时为 None
    latest: Option<Latest>,
    releases: Vec<ReleaseInfo>,
    phase: Phase,
    status: SharedString,
    task: Option<Task<()>>,
    check_task: Option<Task<()>>,
    progress: Option<DownloadProgress>,
    /// 本次是不是接着上次的半截文件在续传
    resumed: bool,
    /// 窗口关掉或点取消就置位，下载线程看到后留着半截文件退出，等下次续传
    cancel: Arc<AtomicBool>,
    /// 启动检查路径才给「该版本不再提示」
    from_startup: bool,
    /// 上次自动安装的退出码（安装包还在才认），用来解释「为什么重启后还是旧版」
    install_error: Option<u32>,
}

impl UpdateWindow {
    fn new(
        parent: Entity<DaroApp>,
        dark: bool,
        phase: Phase,
        latest: Option<Latest>,
        releases: Vec<ReleaseInfo>,
        from_startup: bool,
    ) -> Self {
        let install_error = latest
            .as_ref()
            .and_then(|latest| update::install_failure(&latest.version));
        Self {
            parent,
            dark,
            tab: Tab::App,
            latest,
            releases,
            phase,
            status: SharedString::default(),
            task: None,
            check_task: None,
            progress: None,
            resumed: false,
            cancel: Default::default(),
            from_startup,
            install_error,
        }
    }

    fn version(&self) -> &str {
        self.latest
            .as_ref()
            .map(|latest| latest.version.as_str())
            .unwrap_or_default()
    }

    fn downloading(&self) -> SharedString {
        format!("正在下载 daro v{}…", self.version()).into()
    }

    /// 开查 / 重查：清掉上一次的结果，查完再决定落到 Ask 还是 UpToDate。
    fn start_check(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.phase,
            Phase::Working | Phase::Cancelling | Phase::Restarting
        ) {
            return;
        }
        self.phase = Phase::Checking;
        self.tab = Tab::App;
        self.latest = None;
        self.progress = None;
        self.resumed = false;
        cx.notify();

        let (tx, rx) = async_channel::bounded::<Result<Catalog, String>>(1);
        #[cfg(not(test))]
        std::thread::spawn(move || {
            let _ = tx.send_blocking(update::check(env!("CARGO_PKG_VERSION")));
        });
        // 无头用例里不打真网络：请求线程会活过 TestAppContext，把后续用例带崩。
        #[cfg(test)]
        std::thread::spawn(move || {
            let _ = tx.send_blocking(Ok(Catalog::default()));
        });
        self.check_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = rx.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| match result {
                Ok(catalog) => {
                    this.releases = catalog.releases;
                    // 手动检查也把结果同步到标题栏按钮，关掉更新中心后入口还在
                    if let Some(latest) = catalog.latest.clone() {
                        this.parent.update(cx, |app, cx| {
                            app.set_available_update(latest, cx)
                        });
                    }
                    match catalog.latest {
                        Some(latest) => {
                            this.install_error =
                                update::install_failure(&latest.version);
                            this.latest = Some(latest);
                            this.phase = Phase::Ask;
                        }
                        None => {
                            this.latest = None;
                            this.phase = Phase::UpToDate;
                        }
                    }
                    cx.notify();
                }
                Err(error) => {
                    this.phase = Phase::CheckFailed;
                    this.status = format!("检查更新失败：{error}").into();
                    cx.notify();
                }
            });
        }));
    }

    /// 「立即更新」：留在同一个窗口里开始下载。
    fn begin(&mut self, cx: &mut Context<Self>) {
        if self.phase != Phase::Ask || self.latest.is_none() {
            return;
        }
        self.phase = Phase::Working;
        self.progress = None;
        self.resumed = false;
        self.cancel = Default::default();
        self.status = self.downloading();
        self.start(cx);
        cx.notify();
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        if self.latest.is_none() {
            return;
        }
        self.phase = Phase::Working;
        self.progress = None;
        self.resumed = false;
        self.cancel = Default::default();
        self.status = self.downloading();
        self.start(cx);
        cx.notify();
    }

    fn cancel_download(&mut self, cx: &mut Context<Self>) {
        if self.phase != Phase::Working {
            return;
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.phase = Phase::Cancelling;
        cx.notify();
    }

    fn finish_cancel(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Ask;
        self.progress = None;
        self.resumed = false;
        self.cancel = Default::default();
        cx.notify();
    }

    fn fail(&mut self, error: String, cx: &mut Context<Self>) {
        self.phase = Phase::Failed;
        self.status = format!("自动更新失败：{error}").into();
        cx.notify();
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let Some(latest) = self.latest.clone() else {
            return;
        };
        let (tx, rx) = async_channel::unbounded();
        let cancel = self.cancel.clone();
        let cancelled = cancel.clone();
        std::thread::spawn(move || {
            let result = update::download(&latest, &cancel, |progress| {
                let _ = tx.try_send(DownloadMsg::Progress(progress));
            });
            let _ = tx.try_send(DownloadMsg::Done(result));
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let setup = loop {
                match rx.recv().await {
                    Ok(DownloadMsg::Progress(progress)) => {
                        let _ = this.update(cx, |this, cx| {
                            if progress.resumed_from > 0 && !this.resumed {
                                this.resumed = true;
                                this.status = format!(
                                    "正在下载 daro v{}…（断点续传）",
                                    this.version()
                                )
                                .into();
                            }
                            this.progress = Some(progress);
                            cx.notify();
                        });
                    }
                    Ok(DownloadMsg::Done(Ok(path))) => {
                        // 取消和下载完成撞在一起：取消优先，安装包留到下次
                        if cancelled.load(Ordering::Relaxed) {
                            let _ = this.update(cx, |this, cx| this.finish_cancel(cx));
                            return;
                        }
                        break path;
                    }
                    Ok(DownloadMsg::Done(Err(error))) => {
                        let stopped = cancelled.load(Ordering::Relaxed);
                        let _ = this.update(cx, |this, cx| {
                            if stopped {
                                this.finish_cancel(cx);
                            } else {
                                this.fail(error, cx);
                            }
                        });
                        return;
                    }
                    Err(_) => return,
                }
            };
            // install 只负责把安装程序挂到本进程退出之后，返回很快
            let spawned = update::install(&setup);
            let ok = this
                .update(cx, |this, cx| match spawned {
                    Ok(()) => {
                        this.phase = Phase::Restarting;
                        this.status = format!(
                            "正在安装 daro v{}，装完会自动重启。",
                            this.version()
                        )
                        .into();
                        cx.notify();
                        true
                    }
                    Err(error) => {
                        this.fail(error, cx);
                        false
                    }
                })
                .unwrap_or(false);
            if ok {
                // 让状态文字画出来再退出，安装程序在本进程消失后才动手
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(900))
                    .await;
                cx.update(|cx| cx.quit());
            }
        }));
    }

    fn percent(&self) -> Option<f32> {
        let progress = self.progress.as_ref()?;
        let total = progress.total.filter(|total| *total > 0)?;
        Some((progress.downloaded as f32 / total as f32 * 100.).clamp(0., 100.))
    }

    fn header(&self) -> impl IntoElement {
        let p = Palette::of(self.dark);
        let pill: SharedString = match (self.latest.is_some(), self.phase) {
            (true, _) => "1 项可用更新".into(),
            (false, Phase::Checking) => "正在检查更新".into(),
            (false, Phase::UpToDate | Phase::Ask) => "已是最新版本".into(),
            (false, _) => "更新中心".into(),
        };
        v_flex()
            .px(px(20.))
            .pt(px(16.))
            .pb(px(12.))
            .gap(px(8.))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("更新中心"),
                    )
                    .child(pill_text(pill, self.dark)),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(to_hsla(p.muted_foreground))
                    .child(format!("当前版本 v{}", env!("CARGO_PKG_VERSION"))),
            )
    }

    fn tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::of(self.dark);
        let count = usize::from(self.latest.is_some());
        h_flex()
            .h(px(ui::TAB_STRIP_H))
            .items_end()
            .bg(to_hsla(p.tab_face))
            .border_b(px(ui::HAIRLINE))
            .border_color(to_hsla(p.dialog_line))
            .child(tab_item(
                "update-tab-app",
                "daro",
                count,
                self.tab == Tab::App,
                self.dark,
                cx.listener(|this, _, _, cx| {
                    if this.tab != Tab::App {
                        this.tab = Tab::App;
                        cx.notify();
                    }
                }),
            ))
            .child(tab_item(
                "update-tab-log",
                "更新日志",
                0,
                self.tab == Tab::Changelog,
                self.dark,
                cx.listener(|this, _, _, cx| {
                    if this.tab != Tab::Changelog {
                        this.tab = Tab::Changelog;
                        cx.notify();
                    }
                }),
            ))
    }

    fn body(&self) -> AnyElement {
        let p = Palette::of(self.dark);
        let inner: AnyElement = match self.tab {
            Tab::App => self.update_tab().into_any_element(),
            Tab::Changelog => self.changelog_tab().into_any_element(),
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .bg(to_hsla(p.background))
            .child(inner)
            .into_any_element()
    }

    fn update_tab(&self) -> impl IntoElement {
        let p = Palette::of(self.dark);
        let muted = to_hsla(p.muted_foreground);
        let current = env!("CARGO_PKG_VERSION");

        let mut body = v_flex()
            .id("update-body")
            .flex_1()
            .min_h_0()
            .px(px(20.))
            .py(px(16.))
            .gap(px(10.));

        match self.phase {
            Phase::Ask => {
                let version = self.version().to_string();
                if let Some(code) = self.install_error {
                    body = body.child(
                        div()
                            .text_size(px(13.))
                            .child(format!(
                                "上次安装未成功（安装器退出码 {code}）。安装包已下载好，点「立即更新」会直接重试安装。"
                            )),
                    );
                }
                body = body
                    .child(
                        div()
                            .text_size(px(13.))
                            .child(format!(
                                "daro v{version} 已发布，当前版本为 v{current}。"
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("立即更新会下载并安装新版本，完成后 daro 自动重启。"),
                    )
                    .child(notes_card(
                        self.latest.as_ref().map(|latest| latest.notes.as_str()),
                        self.dark,
                    ));
            }
            Phase::Checking => {
                body = body.child(status_line("正在检查更新…", muted));
            }
            Phase::UpToDate => {
                body = body.child(status_line(
                    format!("daro v{current} 已是最新版本。"),
                    to_hsla(p.foreground),
                ));
            }
            Phase::CheckFailed => {
                body = body
                    .child(status_line(self.status.clone(), to_hsla(p.foreground)))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("可以点底栏「检查更新」重试，或打开下载页手动下载。"),
                    );
            }
            Phase::Working | Phase::Cancelling => {
                let value = self.percent().unwrap_or(0.);
                let stats = self.progress.as_ref().map(download_stats);
                let (stats_text, progress_label): (SharedString, SharedString) = match &stats {
                    Some(stats) => (
                        SharedString::from(stats.clone()),
                        SharedString::from(format!("下载进度 {stats}")),
                    ),
                    None => (SharedString::from("正在连接…"), SharedString::from("下载进度")),
                };
                body = body
                    .child(status_line(self.status.clone(), to_hsla(p.foreground)))
                    .child(
                        div().pt(px(2.)).child(
                            Progress::new("update-progress")
                                .loading(self.progress.is_none())
                                .value(value)
                                .color(to_hsla(p.accent))
                                .accessibility_label(progress_label),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(muted)
                            .child(stats_text),
                    );
            }
            Phase::Restarting | Phase::Failed => {
                body = body.child(status_line(self.status.clone(), to_hsla(p.foreground)));
            }
        }
        body.overflow_y_scrollbar()
    }

    fn changelog_tab(&self) -> impl IntoElement {
        let p = Palette::of(self.dark);
        let muted = to_hsla(p.muted_foreground);
        let latest = self.version().to_string();

        let mut body = v_flex()
            .id("update-changelog")
            .flex_1()
            .min_h_0()
            .px(px(20.))
            .py(px(16.))
            .gap(px(10.));
        if self.releases.is_empty() {
            body = body.child(status_line("暂无更新日志。", muted));
            return body.overflow_y_scrollbar();
        }
        for release in &self.releases {
            let new = !latest.is_empty() && release.version == latest;
            body = body.child(
                v_flex()
                    .gap(px(6.))
                    .p(px(12.))
                    .rounded(px(6.))
                    .border(px(ui::HAIRLINE))
                    .border_color(to_hsla(p.dialog_line))
                    .bg(to_hsla(p.chrome))
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(format!("v{}", release.version)),
                            )
                            .when(new, |row| row.child(pill_text("NEW", self.dark)))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(muted)
                                    .child(release_date(release.published_at.as_deref())),
                            ),
                    )
                    .child(notes_blocks(&release.notes, self.dark)),
            );
        }
        body.overflow_y_scrollbar()
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let mut footer = ui::footer_bar(dark);

        if let Some(latest) = self.latest.as_ref() {
            let url = latest.url.clone();
            footer = footer.child(
                ui::secondary_button("update-open-web", "打开下载页")
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            );
        }
        if self.phase == Phase::Ask && self.from_startup && self.latest.is_some() {
            let version = self.version().to_string();
            footer = footer.child(
                ui::secondary_button("update-prompt-skip", "该版本不再提示")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.parent.update(cx, |app, _| {
                            app.set_skipped_version(version.clone());
                        });
                        window.remove_window();
                    })),
            );
        }

        footer = footer.child(div().flex_1());

        match self.phase {
            Phase::Ask => {
                footer = footer
                    .child(
                        ui::secondary_button("update-prompt-ignore", "关闭")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        ui::primary_button("update-prompt-update", "立即更新", dark, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.begin(cx))),
                    );
            }
            Phase::Working => {
                let label: SharedString = match self.percent() {
                    Some(percent) => format!("下载中 {percent:.0}%").into(),
                    None => "下载中…".into(),
                };
                footer = footer
                    .child(
                        ui::secondary_button("update-cancel-download", "取消下载")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_download(cx))),
                    )
                    .child(
                        ui::primary_button("update-downloading", label, dark, cx).disabled(true),
                    );
            }
            Phase::Cancelling => {
                footer = footer.child(
                    ui::primary_button("update-cancelling", "正在取消…", dark, cx)
                        .loading(true)
                        .disabled(true),
                );
            }
            Phase::Restarting => {
                footer = footer.child(
                    ui::primary_button("update-installing", "正在安装…", dark, cx)
                        .loading(true)
                        .disabled(true),
                );
            }
            Phase::Failed => {
                let url = self
                    .latest
                    .as_ref()
                    .map(|latest| latest.url.clone())
                    .unwrap_or_default();
                footer = footer
                    .child(
                        ui::secondary_button("update-open", "打开下载页")
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
                    .child(
                        ui::secondary_button("update-close", "关闭")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        ui::primary_button("update-retry", "重试", dark, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                    );
            }
            Phase::Checking => {
                footer = footer
                    .child(
                        ui::secondary_button("update-close", "关闭")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        ui::primary_button("update-checking", "检查中…", dark, cx)
                            .loading(true)
                            .disabled(true),
                    );
            }
            Phase::UpToDate | Phase::CheckFailed => {
                footer = footer
                    .child(
                        ui::secondary_button("update-close", "关闭")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        ui::primary_button("update-check", "检查更新", dark, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.start_check(cx))),
                    );
            }
        }
        footer
    }
}

impl Drop for UpdateWindow {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

enum DownloadMsg {
    Progress(DownloadProgress),
    Done(Result<std::path::PathBuf, String>),
}

impl Render for UpdateWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::of(self.dark);
        v_flex()
            .id("update-root")
            .size_full()
            .bg(to_hsla(p.chrome))
            .text_color(to_hsla(p.foreground))
            .on_key_down(cx.listener(|_, ev: &KeyDownEvent, window, _cx| {
                if ev.keystroke.key == "escape" {
                    window.remove_window();
                }
            }))
            .child(title_bar("更新中心", self.dark))
            .child(self.header())
            .child(self.tabs(cx))
            .child(self.body())
            .child(self.footer(cx))
    }
}

fn title_bar(label: &'static str, dark: bool) -> impl IntoElement {
    let p = Palette::of(dark);
    TitleBar::new().h(px(ui::TITLE_H)).border_b_0().child(
        h_flex()
            .h_full()
            .items_center()
            .gap(px(6.))
            .pl(px(4.))
            .child(
                div()
                    .text_xs()
                    .text_color(to_hsla(p.foreground))
                    .child(label),
            ),
    )
}

fn tab_item(
    id: &'static str,
    label: &'static str,
    count: usize,
    on: bool,
    dark: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let p = Palette::of(dark);
    let mut item = div()
        .id(id)
        .on_click(click)
        .h(px(ui::TAB_H))
        .px(px(ui::TAB_PAD_X))
        .flex()
        .items_center()
        .gap(px(6.))
        .cursor_pointer()
        .text_size(px(13.))
        .text_color(to_hsla(if on {
            p.foreground
        } else {
            p.muted_foreground
        }))
        .bg(to_hsla(if on { p.background } else { p.tab_face }))
        .when(on, |item| {
            item.child(
                div()
                    .absolute()
                    .left(px(0.))
                    .right(px(0.))
                    .bottom(px(-ui::HAIRLINE))
                    .h(px(ui::HAIRLINE))
                    .bg(to_hsla(p.background)),
            )
        })
        .child(label);
    if count > 0 {
        item = item.child(pill_text(count.to_string(), dark));
    }
    item.test_support()
}

/// 标题旁的灰色小药丸（「1 项可用更新」「NEW」）。
fn pill_text(label: impl Into<SharedString>, dark: bool) -> Div {
    let p = Palette::of(dark);
    h_flex()
        .h(px(20.))
        .px(px(8.))
        .items_center()
        .rounded(px(10.))
        .bg(to_hsla(p.selection))
        .text_size(px(11.))
        .text_color(to_hsla(p.muted_foreground))
        .child(label.into())
}

/// 更新说明外框：参考图里正文装在一张描边卡片里。
fn notes_card(notes: Option<&str>, dark: bool) -> impl IntoElement {
    let p = Palette::of(dark);
    v_flex()
        .id("update-notes")
        .p(px(12.))
        .rounded(px(6.))
        .border(px(ui::HAIRLINE))
        .border_color(to_hsla(p.dialog_line))
        .bg(to_hsla(p.chrome))
        .child(notes_blocks(notes.unwrap_or_default(), dark))
        .test_support()
}

/// 把 GitHub 发布正文（Markdown）按行铺成纯文本：标题加粗、列表加圆点，
/// 去掉 `**`、反引号这类标记；正文为空时给一句兜底。
fn notes_blocks(notes: &str, dark: bool) -> Div {
    let p = Palette::of(dark);
    let mut col = v_flex().gap(px(4.));
    let mut any = false;
    for line in notes.lines() {
        if line.trim().is_empty() {
            continue;
        }
        any = true;
        let heading = line.starts_with('#');
        let mut text = line.trim_start_matches('#').trim().to_string();
        if let Some(rest) = text.strip_prefix("- ").or_else(|| text.strip_prefix("* ")) {
            text = format!("• {rest}");
        }
        text = text.replace("**", "").replace("__", "").replace('`', "");
        col = col.child(
            div()
                .text_size(px(12.))
                .when(heading, |line| {
                    line.font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(to_hsla(p.foreground))
                })
                .child(text),
        );
    }
    if !any {
        col = col.child(
            div()
                .text_size(px(12.))
                .text_color(to_hsla(p.muted_foreground))
                .child("该版本未提供更新说明。"),
        );
    }
    col
}

fn status_line(text: impl Into<SharedString>, color: Hsla) -> Div {
    div().text_size(px(12.)).text_color(color).child(text.into())
}

/// ISO 8601 的日期部分；老发布没有时间就不显示。
fn release_date(value: Option<&str>) -> String {
    value
        .and_then(|value| value.get(..10))
        .unwrap_or_default()
        .to_string()
}

/// 「3.2 / 13.8 MB · 23% · 1.2 MB/s」，总量未知时省掉百分比。
fn download_stats(progress: &DownloadProgress) -> String {
    let mut parts = Vec::new();
    match progress.total.filter(|total| *total > 0) {
        Some(total) => {
            parts.push(format!(
                "{} / {}",
                human_size(progress.downloaded as f64),
                human_size(total as f64)
            ));
            parts.push(format!(
                "{:.0}%",
                progress.downloaded as f64 / total as f64 * 100.
            ));
        }
        None => parts.push(human_size(progress.downloaded as f64)),
    }
    if progress.bytes_per_sec > 0. {
        parts.push(format!("{}/s", human_size(progress.bytes_per_sec)));
    }
    parts.join(" · ")
}

fn human_size(bytes: f64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes.max(0.);
    let mut unit = 0;
    while value >= 1024. && unit < UNITS.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
