//! 「选项 → MCP 服务」页。
//!
//! 服务本体跑在独立的后台进程 `daro-mcpd` 里，这一页只做三件事：开关、改档 /
//! 端口 / 令牌、把客户端配置复制出去。运行状态靠后台进程每秒写的心跳判断，
//! 本页也每秒重读一次——后台自己挂掉或退出，这里会在一秒内显示出来。
//!
//! 这页的改动立即落盘（按钮 / 开关本身就是动作），不参与选项窗的「确定 / 取消」；
//! 选项窗的确定只提交其他页的草稿。

use gpui_kit::component::input::InputState;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use daro_rs::mcp::{self, policy::Mode, Status};

use crate::palette::Palette;
use crate::zed_icons::{self, ZedIcon};
use crate::{to_hsla, ui};

pub(crate) struct McpPage {
    dark: bool,
    status: Status,
    mode: Mode,
    port: Entity<InputState>,
    /// 端口校验这类本地提示
    notice: Option<String>,
    /// 动作失败的原因（比如 daro-mcpd 拉不起来）。轮询会覆盖 `status.error`，
    /// 但拉进程失败没有心跳可写，只能单独留到服务真的跑起来为止。
    action_error: Option<String>,
}

impl McpPage {
    pub(crate) fn new(dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let status = mcp::client_status();
        let mode = mcp::current_config()
            .map(|cfg| cfg.default_mode)
            .unwrap_or_default();
        let port = cx.new(|cx| {
            InputState::new(window, cx).default_value(status.port.to_string())
        });
        let this = Self {
            dark,
            status,
            mode,
            port,
            notice: None,
            action_error: None,
        };
        // 心跳每秒一写，这里也每秒一读；选项窗关掉后 update 失败即退出。
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(1))
                .await;
            let status =
                cx.background_executor()
                    .spawn(async { mcp::client_status() })
                    .await;
            let alive = this
                .update(cx, |this, cx| {
                    if status.running {
                        this.action_error = None;
                    }
                    this.status = status;
                    cx.notify();
                })
                .is_ok();
            if !alive {
                break;
            }
        })
        .detach();
        this
    }

    fn pal(&self) -> Palette {
        Palette::of(self.dark)
    }

    /// 写配置文件 / 拉起后台这类操作放后台线程，回来再更新一份状态。
    fn run_task(
        &mut self,
        task: impl FnOnce() -> Result<Status, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let fut = cx.background_executor().spawn(async move { task() });
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            let _ = this.update(cx, |this, cx| {
                match res {
                    Ok(status) => {
                        this.notice = None;
                        // set_enabled 会把「拉不起进程」的提示塞进 status.error
                        this.action_error = status.error.clone();
                        this.status = status;
                    }
                    Err(e) => this.notice = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_enabled(&mut self, on: bool, cx: &mut Context<Self>) {
        // 先亮开关，写盘 / 拉进程在后台完成
        self.status.enabled = on;
        cx.notify();
        self.run_task(move || mcp::set_enabled(on), cx);
    }

    fn apply_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        cx.notify();
        self.run_task(move || mcp::set_mode(mode), cx);
    }

    fn apply_port(&mut self, cx: &mut Context<Self>) {
        let raw = self.port.read(cx).value().trim().to_string();
        match raw.parse::<u16>() {
            Ok(port) if port >= 1024 => self.run_task(move || mcp::set_port(port), cx),
            Ok(_) => {
                self.notice = Some("端口要大于等于 1024".into());
                cx.notify();
            }
            Err(_) => {
                self.notice = Some("端口必须是 1024–65535 的整数".into());
                cx.notify();
            }
        }
    }

    fn client_config(&self) -> String {
        format!(
            "{{\n  \"mcpServers\": {{\n    \"daro\": {{\n      \"type\": \"http\",\n      \
             \"url\": \"{}\",\n      \"headers\": {{ \"Authorization\": \"Bearer {}\" }}\n    \
             }}\n  }}\n}}",
            self.status.url, self.status.token
        )
    }

    /// 一行设置：左侧标题 / 说明，右侧控件；行间一条发丝线。
    fn row(
        &self,
        title: impl Into<SharedString>,
        desc: impl Into<SharedString>,
        control: AnyElement,
        last: bool,
    ) -> AnyElement {
        let p = self.pal();
        h_flex()
            .w_full()
            .items_center()
            .gap(px(24.))
            .py(px(11.))
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
                            .text_size(px(13.))
                            .text_color(to_hsla(p.foreground))
                            .child(title.into()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(to_hsla(p.muted_foreground))
                            .child(desc.into()),
                    ),
            )
            .child(div().flex_none().flex().items_center().child(control))
            .into_any_element()
    }

    fn group_heading(&self, title: &'static str) -> AnyElement {
        let p = self.pal();
        v_flex()
            .mt(px(18.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(to_hsla(p.muted_foreground))
                    .child(title),
            )
            .child(
                div()
                    .mt(px(6.))
                    .h(px(ui::HAIRLINE))
                    .bg(to_hsla(p.border)),
            )
            .into_any_element()
    }

    fn enabled_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let me = cx.entity().clone();
        Switch::new("mcp-enabled")
            .checked(self.status.enabled)
            .color(to_hsla(p.control_fill))
            .on_change(move |checked, _, cx| {
                let on = *checked;
                me.update(cx, |this, cx| this.apply_enabled(on, cx));
            })
            .into_any_element()
    }

    fn status_control(&self) -> AnyElement {
        let p = self.pal();
        let (dot, text) = match (self.status.enabled, self.status.running) {
            (false, _) => (p.subtle_foreground, "未启用".to_string()),
            (true, true) if self.status.active > 0 => (p.accent, format!("运行中 · {}", self.status.active)),
            (true, true) => (p.accent, "运行中".to_string()),
            (true, false) => (p.subtle_foreground, "启动中".to_string()),
        };
        h_flex()
            .id("mcp-status")
            .test_support()
            .gap(px(6.))
            .items_center()
            .child(div().size(px(7.)).rounded_full().bg(to_hsla(dot)))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(to_hsla(p.muted_foreground))
                    .child(text),
            )
            .into_any_element()
    }

    fn mode_control(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap(px(2.))
            .children([Mode::Readonly, Mode::ReadWrite, Mode::Full].into_iter().map(
                |mode| {
                    let me = cx.entity().clone();
                    let picked = mode == self.mode;
                    ui::segment(
                        format!("mcp-mode-{}", mode.wire()),
                        mode.label(),
                        picked,
                        self.dark,
                        move |_, _, app| {
                            me.update(app, |this: &mut McpPage, cx: &mut Context<McpPage>| {
                                if mode != this.mode {
                                    this.apply_mode(mode, cx);
                                }
                            });
                        },
                    )
                    .test_support()
                },
            ))
            .into_any_element()
    }

    fn port_control(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .w(px(220.))
            .gap(px(6.))
            .items_center()
            .child(div().flex_1().min_w_0().child(ui::field_input(
                &self.port,
                self.dark,
                window,
                cx,
            )))
            .child(
                ui::compact_button("mcp-port-apply", "应用")
                    .on_click(cx.listener(|this, _, _, cx| this.apply_port(cx))),
            )
            .into_any_element()
    }

    fn token_control(&self, cx: &mut Context<Self>) -> AnyElement {
        ui::compact_button("mcp-token-rotate", "重新生成")
            .on_click(cx.listener(|this, _, _, cx| {
                this.run_task(mcp::rotate_token, cx);
            }))
            .into_any_element()
    }

    fn content(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.pal();
        let snippet = self.client_config();
        let notice = self
            .notice
            .clone()
            .or_else(|| self.action_error.clone())
            .or_else(|| self.status.error.clone());
        v_flex()
            .id("mcp-content")
            .child(
                div()
                    .text_size(px(17.))
                    .text_color(to_hsla(p.foreground))
                    .child("MCP 服务"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(11.5))
                    .text_color(to_hsla(p.muted_foreground))
                    .child("把已存连接的数据能力提供给 Qoder / Claude 等外部 agent；本页改动立即生效。"),
            )
            .when_some(notice, |s, msg| {
                s.child(
                    h_flex()
                        .id("mcp-notice")
                        .test_support()
                        .mt(px(10.))
                        .gap(px(6.))
                        .items_center()
                        .child(zed_icons::ic(
                            ZedIcon::Warning,
                            12.,
                            to_hsla(p.muted_foreground),
                        ))
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(to_hsla(p.foreground))
                                .child(msg),
                        ),
                )
            })
            .child(self.group_heading("服务"))
            .child(
                v_flex()
                    .child(self.row(
                        "启用 MCP 服务",
                        "后台进程 daro-mcpd 只在打开后运行，默认关闭。",
                        self.enabled_control(cx),
                        false,
                    ))
                    .child(self.row(
                        "运行状态",
                        self.status.url.clone(),
                        self.status_control(),
                        false,
                    ))
                    .child(self.row(
                        "访问模式",
                        "只读仅放行查询；数据读写放行带条件的增删改；完全访问放行 DDL。",
                        self.mode_control(cx),
                        false,
                    ))
                    .child(self.row(
                        "监听端口",
                        "只绑 127.0.0.1，改完由后台进程自己重绑。",
                        self.port_control(window, cx),
                        false,
                    ))
                    .child(self.row(
                        "访问令牌",
                        "客户端须带 Authorization: Bearer …；重新生成后旧令牌立即失效。",
                        self.token_control(cx),
                        true,
                    )),
            )
            .child(self.group_heading("客户端"))
            .child(
                v_flex()
                    .mt(px(10.))
                    .gap(px(8.))
                    .child(
                        div()
                            .p(px(10.))
                            .rounded(px(6.))
                            .bg(to_hsla(p.dialog_surface))
                            .border(px(ui::HAIRLINE))
                            .border_color(to_hsla(p.border_soft))
                            .font_family("Consolas")
                            .text_size(px(11.))
                            .text_color(to_hsla(p.foreground))
                            .child(snippet),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                ui::compact_button("mcp-copy-config", "复制配置").on_click(
                                    cx.listener(|this, _, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            this.client_config(),
                                        ));
                                    }),
                                ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(to_hsla(p.subtle_foreground))
                                    .child(self.status.config_file.clone()),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for McpPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.content(window, cx)
    }
}
