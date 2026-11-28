//! 标题栏右上角的更新入口（照 VS Code 的更新提示做的）。
//!
//! 查到比当前新的版本后，`DaroApp::update` 存一份状态，标题栏出蓝色「更新」
//! 按钮；悬浮按钮即展开详情面板并开始后台下载（面板关掉下载也继续）。面板里
//! 能直接点「发布地址」跳到该版本的 GitHub Release 页，「发布说明」把正文按
//! Markdown 开成主窗口页签；下载完成后面板给「重启并更新」，走 [update::install]
//! 拉起安装程序。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::component::*;
use gpui_kit::*;

use crate::release_notes::ReleaseNotes;
use crate::update::{self, DownloadProgress, Latest};
use crate::zed_icons::ZedIcon;
use crate::{DaroApp, TITLE_H, WIN_BTN_W, db_icons, to_hsla, ui};

/// 详情面板的宽度。
const PANEL_W: f32 = 380.;
/// 悬浮展开面板的延时：鼠标只是路过按钮时别急着弹。
const HOVER_OPEN_DELAY: Duration = Duration::from_millis(220);
/// 鼠标从按钮挪向面板的宽限：这段时间里先别收面板。
const HOVER_BRIDGE_DELAY: Duration = Duration::from_millis(320);
/// 指针彻底离开面板后的收起延时。
const HOVER_CLOSE_DELAY: Duration = Duration::from_millis(180);

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// 有更新，还没开始下载
    Idle,
    Downloading,
    /// 安装包已下完，等用户点「重启并更新」
    Ready,
    Installing,
    Failed,
}

enum DownloadMsg {
    Progress(DownloadProgress),
    Done(Result<PathBuf, String>),
}

/// 更新按钮背后的状态；`DaroApp::update` 为 Some 时按钮才出现。
pub struct UpdateBadge {
    latest: Latest,
    phase: Phase,
    progress: Option<DownloadProgress>,
    /// 下完的安装包路径；下载前 / 失败时为 None
    setup: Option<PathBuf>,
    status: SharedString,
    cancel: Arc<AtomicBool>,
    task: Option<Task<()>>,
    /// 详情面板是否展开
    popover: bool,
    /// 面板是否由点击打开：点开的面板钉住，鼠标移开也不收
    sticky: bool,
    /// 指针是否在「更新」按钮上
    hover_button: bool,
    /// 指针是否在面板上
    hover_panel: bool,
    /// 悬停开合的延时任务；换任务即取消上一个
    hover_task: Option<Task<()>>,
}

impl Drop for UpdateBadge {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl UpdateBadge {
    fn percent(&self) -> Option<f32> {
        let progress = self.progress.as_ref()?;
        let total = progress.total.filter(|total| *total > 0)?;
        Some((progress.downloaded as f32 / total as f32 * 100.).clamp(0., 100.))
    }

    fn stats(&self) -> Option<String> {
        let progress = self.progress.as_ref()?;
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
        Some(parts.join(" · "))
    }
}

impl DaroApp {
    /// 检查到新版本：存下详情并让标题栏出更新按钮。同一版本重复上报只忽略。
    pub fn set_available_update(&mut self, latest: Latest, cx: &mut Context<Self>) {
        if self
            .update
            .as_ref()
            .is_some_and(|badge| badge.latest.version == latest.version)
        {
            return;
        }
        self.update = Some(UpdateBadge {
            latest,
            phase: Phase::Idle,
            progress: None,
            setup: None,
            status: SharedString::default(),
            cancel: Default::default(),
            task: None,
            popover: false,
            sticky: false,
            hover_button: false,
            hover_panel: false,
            hover_task: None,
        });
        cx.notify();
    }

    /// 详情面板是否展开（悬浮或点击打开都算）。
    pub fn update_popover_open(&self) -> bool {
        self.update.as_ref().is_some_and(|badge| badge.popover)
    }

    /// 面板是否由点击打开：只有这种才铺遮罩挡外部点击。
    ///
    /// 悬浮打开的面板不能铺遮罩——那会把整窗的交互都吃掉，用户只是想看一眼
    /// 更新内容。
    pub fn update_popover_sticky(&self) -> bool {
        self.update
            .as_ref()
            .is_some_and(|badge| badge.popover && badge.sticky)
    }

    /// 更新是不是已经在下载（悬浮只预览，不动下载）。
    #[cfg(test)]
    pub fn update_downloading(&self) -> bool {
        self.update
            .as_ref()
            .is_some_and(|badge| badge.phase == Phase::Downloading)
    }

    pub fn close_update_popover(&mut self) {
        if let Some(badge) = self.update.as_mut() {
            badge.popover = false;
            badge.sticky = false;
        }
    }

    /// 点更新按钮：面板钉住展开（不随鼠标移开收起），再点一下收起。
    /// 第一次点开时顺带开始后台下载。
    pub fn toggle_update_popover(&mut self, cx: &mut Context<Self>) {
        let Some(badge) = self.update.as_mut() else {
            return;
        };
        let was_sticky = badge.popover && badge.sticky;
        badge.popover = !was_sticky;
        badge.sticky = badge.popover;
        badge.hover_task = None;
        let start = badge.popover && badge.phase == Phase::Idle;
        cx.notify();
        if start {
            self.start_update_download(cx);
        }
    }

    /// 指针进出「更新」按钮：悬浮展开面板，指针移开先宽限再收。
    fn update_button_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        let Some(badge) = self.update.as_mut() else {
            return;
        };
        badge.hover_button = hovered;
        if hovered {
            self.schedule_update_panel(true, HOVER_OPEN_DELAY, cx);
        } else if !badge.hover_panel {
            // 指针正从按钮挪向面板：给一段宽限，别在路上就把面板收了。
            self.schedule_update_panel(false, HOVER_BRIDGE_DELAY, cx);
        }
    }

    /// 指针进出面板：进来就取消待收起的任务，出去且不在按钮上才收。
    fn update_panel_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        let Some(badge) = self.update.as_mut() else {
            return;
        };
        badge.hover_panel = hovered;
        if hovered {
            // 掉任务即取消：待收起的延时任务在这里作废。
            if let Some(badge) = self.update.as_mut() {
                badge.hover_task = None;
            }
        } else if !badge.hover_button {
            self.schedule_update_panel(false, HOVER_CLOSE_DELAY, cx);
        }
    }

    /// 面板的开合都走延时任务：换任务即取消上一个，避免鼠标快速来回时闪。
    fn schedule_update_panel(&mut self, open: bool, delay: Duration, cx: &mut Context<Self>) {
        let Some(badge) = self.update.as_mut() else {
            return;
        };
        let already = badge.popover == open;
        // 丢掉旧任务即取消它；顺带把「已经到位」的情况挡在任务之外。
        badge.hover_task = None;
        if already {
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |app, cx| {
                let Some(badge) = app.update.as_mut() else {
                    return;
                };
                if badge.popover == open {
                    return;
                }
                if !open && (badge.sticky || badge.hover_button || badge.hover_panel) {
                    return;
                }
                badge.popover = open;
                if !open {
                    badge.sticky = false;
                }
                cx.notify();
            });
        });
        if let Some(badge) = self.update.as_mut() {
            badge.hover_task = Some(task);
        }
    }

    /// 「重试」：把失败态退回初始态再走一遍下载。
    fn retry_update_download(&mut self, cx: &mut Context<Self>) {
        if let Some(badge) = self.update.as_mut() {
            if badge.phase != Phase::Failed {
                return;
            }
            badge.phase = Phase::Idle;
            badge.status = SharedString::default();
        }
        self.start_update_download(cx);
    }

    fn start_update_download(&mut self, cx: &mut Context<Self>) {
        if !self
            .update
            .as_ref()
            .is_some_and(|badge| badge.phase == Phase::Idle)
        {
            return;
        }
        let version = self
            .update
            .as_ref()
            .map(|badge| badge.latest.version.clone())
            .unwrap_or_default();
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = cancel.clone();
        if let Some(badge) = self.update.as_mut() {
            badge.phase = Phase::Downloading;
            badge.progress = None;
            badge.setup = None;
            badge.status = "正在下载更新，请稍候…".into();
            badge.cancel = cancel;
        }
        cx.notify();

        // 无头用例不打真网络：直接把状态推到「下载完成」，安装动作仍要用户点。
        #[cfg(test)]
        {
            if let Some(badge) = self.update.as_mut() {
                badge.phase = Phase::Ready;
                badge.setup = Some(std::env::temp_dir().join("daro-update-setup-test.exe"));
                badge.status = "下载完成，重启 daro 完成安装。".into();
            }
            cx.notify();
            return;
        }

        #[cfg(not(test))]
        {
            let Some(latest) = self.update.as_ref().map(|badge| badge.latest.clone()) else {
                return;
            };
            let cancelled = thread_cancel.clone();
            let (tx, rx) = async_channel::unbounded();
            std::thread::spawn(move || {
                let result = update::download(&latest, &thread_cancel, |progress| {
                    let _ = tx.try_send(DownloadMsg::Progress(progress));
                });
                let _ = tx.try_send(DownloadMsg::Done(result));
            });
            let task = cx.spawn(async move |this, cx| {
                while let Ok(msg) = rx.recv().await {
                    match msg {
                        DownloadMsg::Progress(progress) => {
                            let _ = this.update(cx, |app, cx| {
                                let Some(badge) = app
                                    .update
                                    .as_mut()
                                    .filter(|b| b.latest.version == version)
                                else {
                                    return;
                                };
                                if badge.phase != Phase::Downloading {
                                    return;
                                }
                                badge.progress = Some(progress);
                                cx.notify();
                            });
                        }
                        DownloadMsg::Done(Ok(path)) => {
                            let _ = this.update(cx, |app, cx| {
                                let Some(badge) = app
                                    .update
                                    .as_mut()
                                    .filter(|b| b.latest.version == version)
                                else {
                                    return;
                                };
                                badge.progress = None;
                                if cancelled.load(Ordering::Relaxed) {
                                    return;
                                }
                                badge.phase = Phase::Ready;
                                badge.setup = Some(path);
                                badge.status = "下载完成，重启 daro 完成安装。".into();
                                cx.notify();
                            });
                            return;
                        }
                        DownloadMsg::Done(Err(error)) => {
                            let _ = this.update(cx, |app, cx| {
                                let Some(badge) = app
                                    .update
                                    .as_mut()
                                    .filter(|b| b.latest.version == version)
                                else {
                                    return;
                                };
                                badge.progress = None;
                                if cancelled.load(Ordering::Relaxed) {
                                    return;
                                }
                                badge.phase = Phase::Failed;
                                badge.status = format!("下载失败：{error}").into();
                                cx.notify();
                            });
                            return;
                        }
                    }
                }
            });
            if let Some(badge) = self.update.as_mut() {
                badge.task = Some(task);
            }
        }
    }

    /// 「重启并更新」：拉起安装程序，稍等状态文字画出来再退出进程。
    fn install_update(&mut self, cx: &mut Context<Self>) {
        let Some(setup) = self
            .update
            .as_ref()
            .and_then(|badge| badge.setup.clone())
        else {
            return;
        };
        match update::install(&setup) {
            Ok(()) => {
                if let Some(badge) = self.update.as_mut() {
                    badge.phase = Phase::Installing;
                    badge.status = "正在安装，完成后 daro 会自动重启。".into();
                }
                cx.notify();
                cx.spawn(async move |_, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(900))
                        .await;
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
            Err(error) => {
                if let Some(badge) = self.update.as_mut() {
                    badge.phase = Phase::Failed;
                    badge.status = format!("安装失败：{error}").into();
                }
                cx.notify();
            }
        }
    }

    /// 「发布说明」：按当前版本的 GitHub 正文开页签，并收起浮层。
    fn open_update_release_notes(&mut self, cx: &mut Context<Self>) {
        let Some(badge) = self.update.as_ref() else {
            return;
        };
        let notes = ReleaseNotes {
            version: badge.latest.version.clone(),
            notes: badge.latest.notes.clone(),
            published_at: badge.latest.published_at.clone(),
            url: badge.latest.url.clone(),
        };
        self.open_release_notes_tab(notes, cx);
        self.close_update_popover();
        cx.notify();
    }

    /// 标题栏右侧的蓝色小按钮（有更新才渲染）。
    ///
    /// 悬浮即展开详情面板：面板是窗口里的真元素，鼠标能挪进去点「发布地址」
    /// 的链接，移出按钮也不会立刻消失。
    pub fn update_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(badge) = self.update.as_ref() else {
            return div().into_any_element();
        };
        let p = self.pal();
        let (bg, fg) = (to_hsla(p.accent), to_hsla(p.on_accent));
        let label: SharedString = match badge.phase {
            Phase::Idle => "更新".into(),
            Phase::Downloading => match badge.percent() {
                Some(percent) => format!("下载中 {percent:.0}%").into(),
                None => "下载中…".into(),
            },
            Phase::Ready => "重启更新".into(),
            Phase::Installing => "安装中…".into(),
            Phase::Failed => "重试更新".into(),
        };
        h_flex()
            .id("update-button")
            .test_support()
            .flex_none()
            // Zed `Chip` / `ButtonSize::Compact`：18 高、`px_1` 4、`gap_0p5` 2、
            // `rounded_sm` 4，图标 XSmall 12、标签 Small 12
            .h(px(ui::COMPACT_H))
            .px(px(4.))
            .gap(px(2.))
            .items_center()
            .rounded(px(ui::RADIUS))
            .bg(bg)
            .cursor_pointer()
            .hover(move |s| s.opacity(0.86))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.update_button_hover(*hovered, cx)
            }))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_update_popover(cx)))
            .child(
                Icon::new(ZedIcon::Download)
                    .with_size(px(12.))
                    .text_color(fg),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(fg)
                    .child(label),
            )
            .into_any_element()
    }

    /// 按钮下方的详情面板：版本信息、发布说明、下载进度与主动作。
    ///
    /// 面板是窗口里的普通元素（不是 tooltip），所以鼠标能挪进来点链接；
    /// 悬浮打开的会随指针离开收起，点按钮打开的钉住，外部点击才收。
    pub fn update_popover(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(badge) = self.update.as_ref() else {
            return div().into_any_element();
        };
        let p = self.pal();
        let (fg, muted, hover) = (
            to_hsla(p.foreground),
            to_hsla(p.muted_foreground),
            to_hsla(p.hover),
        );
        // 浮层右缘对在更新按钮右缘：窗口按钮（3×46）+ 应用菜单 + 主题 / 仓库两个圆钮。
        let right = if crate::draw_window_controls(window) {
            WIN_BTN_W * 3. + 94.
        } else {
            94.
        };
        let title: SharedString = match badge.phase {
            Phase::Idle => "可用更新".into(),
            Phase::Downloading => "正在下载更新".into(),
            Phase::Ready => "更新已就绪".into(),
            Phase::Installing => "正在安装更新".into(),
            Phase::Failed => "更新下载失败".into(),
        };
        let date = badge
            .latest
            .published_at
            .as_deref()
            .and_then(|value| value.get(..10))
            .unwrap_or_default()
            .to_string();
        let notes = badge.latest.notes.clone();
        // 「发布地址」用 API 返回的 Release 页链接，单拎一行：正文里的链接行
        // 在长说明里会被截掉，而它恰恰是用户最想点开的地方。
        let release_url = (!badge.latest.url.trim().is_empty()).then(|| badge.latest.url.clone());
        let preview = notes_preview(&notes);

        let mut info = v_flex().gap(px(2.)).child(
            div()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(fg)
                .child(format!("daro {}", badge.latest.version)),
        );
        info = info
            .child(kv_row("当前版本", env!("CARGO_PKG_VERSION"), muted, fg))
            .child(kv_row("最新版本", &badge.latest.version, muted, fg));
        if !date.is_empty() {
            info = info.child(kv_row("发布日期", &date, muted, fg));
        }

        let mut card = v_flex()
            .id("update-popover")
            .test_support()
            .absolute()
            .top(px(TITLE_H + 4.))
            .right(px(right))
            .w(px(PANEL_W))
            // Zed `Popover` 的 `elevation_2`：8 圆角、1px `border_variant` 描边、
            // ElevatedSurface 双层投影，内容留白 8
            .p(px(8.))
            .gap(px(8.))
            .bg(to_hsla(p.dialog_surface))
            .border(px(1.))
            .border_color(to_hsla(p.border_soft))
            .rounded(px(crate::ui::FLOAT_RADIUS))
            .shadow(crate::ui::float_shadow(self.dark))
            .text_color(fg)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.update_panel_hover(*hovered, cx)
            }))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("update-popover-close")
                            .size(px(ui::CONTROL_HIT))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(ui::RADIUS))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_update_popover();
                                cx.notify();
                            }))
                            .child(
                                Icon::new(ZedIcon::Close)
                                    .with_size(px(ui::CONTROL_ICON))
                                    .text_color(muted),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .items_start()
                    .gap(px(10.))
                    .child(db_icons::logo(38.))
                    .child(info),
            );

        // 发布说明预览：正文里的链接（含裸 URL）点了就开浏览器。
        if !preview.trim().is_empty() {
            card = card.child(
                div()
                    .id("update-notes")
                    .test_support()
                    .w_full()
                    .min_w_0()
                    .child(crate::release_notes::markdown(&preview, self.dark)),
            );
        }
        if let Some(url) = release_url {
            let link = url.clone();
            card = card.child(
                h_flex()
                    .id("update-release")
                    .test_support()
                    .w_full()
                    .min_w_0()
                    .gap(px(6.))
                    .items_start()
                    .cursor_pointer()
                    .on_click(move |_, _, cx| cx.open_url(&link))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("发布地址"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(to_hsla(p.accent))
                            .child(url),
                    ),
            );
        }

        card = card.child(
            div()
                .text_size(px(12.))
                .line_height(px(18.))
                .text_color(muted)
                .child(badge.status.clone()),
        );
        if badge.phase == Phase::Downloading {
            let value = badge.percent().unwrap_or(0.);
            card = card.child(
                div().child(
                    ui::progress_bar(
                        "update-badge-progress",
                        value,
                        to_hsla(p.accent),
                        badge.progress.is_none(),
                    )
                    .accessibility_label("更新下载进度"),
                ),
            );
            if let Some(stats) = badge.stats() {
                card = card.child(
                    div()
                        .text_size(px(12.))
                        .text_color(muted)
                        .child(stats),
                );
            }
        }

        let mut footer = h_flex()
            .items_center()
            .gap(px(8.))
            .pt(px(2.))
            .child(
                ui::secondary_button("update-release-notes", "发布说明")
                    .on_click(cx.listener(|this, _, _, cx| this.open_update_release_notes(cx))),
            )
            .child(div().flex_1());
        footer = match badge.phase {
            Phase::Idle => footer.child(
                ui::primary_button("update-badge-start", "立即下载", self.dark, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.start_update_download(cx))),
            ),
            Phase::Downloading => footer.child(
                ui::primary_button_enabled("update-badge-downloading", "下载中…", self.dark, false, cx)
                    .loading(true),
            ),
            Phase::Ready => footer.child(
                ui::primary_button("update-badge-install", "重启并更新", self.dark, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.install_update(cx))),
            ),
            Phase::Installing => footer.child(
                ui::primary_button_enabled("update-badge-installing", "安装中…", self.dark, false, cx)
                    .loading(true),
            ),
            Phase::Failed => footer.child(
                ui::primary_button("update-badge-retry", "重试", self.dark, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.retry_update_download(cx))),
            ),
        };
        card.child(footer).into_any_element()
    }
}

/// 发布说明预览：跳过 HTML 注释，最多保留 14 行、每行 160 字。
/// 超出时末尾补省略号；代码块被截断也能交给 Markdown 铺完。
///
/// 「完整变更」旧格式（compare 地址）和「发布地址」那行都不收进来：面板底部
/// 单独给一行 Release 页链接，保证看得到、点得到。
fn notes_preview(notes: &str) -> String {
    const MAX_LINES: usize = 14;
    const MAX_LINE_CHARS: usize = 160;
    let mut out = String::new();
    let mut taken = 0usize;
    let mut in_comment = false;
    for raw in notes.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if in_comment {
            if line.contains("-->") {
                in_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("<!--") {
            in_comment = !line.contains("-->");
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if line.contains("/compare/") || line.contains("/releases/tag/") {
            continue;
        }
        if taken == MAX_LINES {
            out.push('…');
            break;
        }
        if line.chars().count() > MAX_LINE_CHARS {
            out.extend(line.chars().take(MAX_LINE_CHARS));
            out.push('…');
        } else {
            out.push_str(line);
        }
        out.push('\n');
        taken += 1;
    }
    out
}

fn kv_row(label: &'static str, value: &str, muted: Hsla, fg: Hsla) -> AnyElement {
    h_flex()
        .gap(px(8.))
        .child(
            div()
                .w(px(52.))
                .flex_none()
                .text_size(px(12.))
                .text_color(muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(fg)
                .child(value.to_string()),
        )
        .into_any_element()
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

#[cfg(test)]
mod hover_preview_tests {
    use super::notes_preview;

    #[test]
    fn notes_preview_skips_comments_and_caps_size() {
        let notes = "<!-- 隐藏 -->\n# 更新说明\n\n- 支持 Markdown\n- 修复问题\n\n**发布地址**：https://github.com/SpringHgui/daro/releases/tag/daro-rs-v1.0.9\n**完整变更**：https://github.com/SpringHgui/daro/compare/daro-rs-v1.0.8...daro-rs-v1.0.9";
        let preview = notes_preview(notes);
        assert!(!preview.contains("隐藏"), "HTML 注释不该出现在预览里");
        assert!(preview.contains("# 更新说明"));
        assert!(preview.contains("- 修复问题"));
        assert!(
            !preview.contains("releases/tag") && !preview.contains("/compare/"),
            "链接行由面板底部单独一行给出，预览里不该重复：{preview}"
        );

        let long: String = (0..20).map(|i| format!("第{i}行\n")).collect();
        let preview = notes_preview(&long);
        assert_eq!(preview.lines().count(), 15, "14 行正文 + 省略号行");
        assert!(preview.ends_with('…'));

        let wide = "字".repeat(200);
        assert_eq!(
            notes_preview(&wide),
            format!("{}…\n", "字".repeat(160)),
            "超长单行要截到 160 字"
        );
    }
}
