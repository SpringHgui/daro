//! 标题栏右上角的更新入口（照 VS Code 的更新提示做的）。
//!
//! 查到比当前新的版本后，`DaroApp::update` 存一份状态，标题栏出蓝色「更新」
//! 按钮；点一下弹出详情浮层并开始后台下载（浮层关掉下载也继续）。「发布说明」
//! 把该版本的 GitHub 正文按 Markdown 开成主窗口页签；下载完成后浮层给
//! 「重启并更新」，走 [update::install] 拉起安装程序。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::component::progress::Progress;
use gpui_kit::component::*;
use gpui_kit::*;

use crate::release_notes::ReleaseNotes;
use crate::update::{self, DownloadProgress, Latest};
use crate::zed_icons::ZedIcon;
use crate::{DaroApp, TITLE_H, WIN_BTN_W, db_icons, to_hsla, ui};

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
    /// 详情浮层是否展开（关掉浮层不影响后台下载）
    popover: bool,
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
        });
        cx.notify();
    }

    pub fn update_popover_open(&self) -> bool {
        self.update.as_ref().is_some_and(|badge| badge.popover)
    }

    pub fn close_update_popover(&mut self) {
        if let Some(badge) = self.update.as_mut() {
            badge.popover = false;
        }
    }

    /// 点更新按钮：开 / 关详情浮层；第一次打开时顺带开始后台下载。
    pub fn toggle_update_popover(&mut self, cx: &mut Context<Self>) {
        let Some(badge) = self.update.as_mut() else {
            return;
        };
        badge.popover = !badge.popover;
        let start = badge.popover && badge.phase == Phase::Idle;
        cx.notify();
        if start {
            self.start_update_download(cx);
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
        let tip: SharedString = match badge.phase {
            Phase::Failed => "下载失败，点开查看并重试".into(),
            Phase::Ready => "更新已下载，重启后安装".into(),
            _ => format!("发现新版本 v{}", badge.latest.version).into(),
        };
        h_flex()
            .id("update-button")
            .test_support()
            .flex_none()
            .h(px(20.))
            .px(px(8.))
            .gap(px(4.))
            .items_center()
            .rounded(px(10.))
            .bg(bg)
            .cursor_pointer()
            .hover(move |s| s.opacity(0.86))
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_update_popover(cx)))
            .child(
                Icon::new(ZedIcon::Download)
                    .with_size(px(11.))
                    .text_color(fg),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(fg)
                    .child(label),
            )
            .into_any_element()
    }

    /// 按钮下方的详情浮层：版本信息、下载进度、发布说明与主动作。
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
            .w(px(340.))
            .p(px(14.))
            .gap(px(10.))
            .bg(to_hsla(p.background))
            .border(px(1.))
            .border_color(to_hsla(p.border))
            .rounded(px(6.))
            .shadow_lg()
            .text_color(fg)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_update_popover();
                                cx.notify();
                            }))
                            .child(
                                Icon::new(ZedIcon::Close)
                                    .with_size(px(11.))
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
                    Progress::new("update-badge-progress")
                        .loading(badge.progress.is_none())
                        .value(value)
                        .color(to_hsla(p.accent))
                        .accessibility_label("更新下载进度"),
                ),
            );
            if let Some(stats) = badge.stats() {
                card = card.child(
                    div()
                        .text_size(px(11.))
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

fn kv_row(label: &'static str, value: &str, muted: Hsla, fg: Hsla) -> AnyElement {
    h_flex()
        .gap(px(8.))
        .child(
            div()
                .w(px(52.))
                .flex_none()
                .text_size(px(11.))
                .text_color(muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(11.))
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
