//! 发布说明页签：更新浮层里点「发布说明」在主窗口开的只读文档页。
//!
//! 内容来自 GitHub 发布正文（Markdown）。这里按块级语法铺：标题、列表、引用、
//! 代码块、分隔线、段落；行内认 `**粗体**`、`` `代码` ``、`[链接](url)`。不追求
//! 完整 Markdown——发布说明的常见写法够用，也不引第三方解析器。

use std::ops::Range;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::InteractiveElement as _;
use gpui_kit::*;

use crate::palette::Palette;
use crate::{DaroApp, OpenTab, to_hsla};

/// 一个版本的发布说明快照（打开页签后不再跟更新状态一起变）。
#[derive(Clone)]
pub struct ReleaseNotes {
    pub version: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub url: String,
}

impl DaroApp {
    /// 打开（或激活已有的）发布说明页签。页签不挂连接，`db` / `cat` 只是占位；
    /// 渲染分派按 `notes.is_some()` 走 [Self::release_notes_page]。
    pub fn open_release_notes_tab(&mut self, notes: ReleaseNotes, cx: &mut Context<Self>) {
        let dup = self
            .opened
            .iter()
            .position(|t| t.notes.as_ref().is_some_and(|n| n.version == notes.version));
        if let Some(ix) = dup {
            self.active_tab = ix + 1;
            if self.dock.pane_of(ix + 1).is_some() {
                self.dock.focus_tab(ix + 1);
            }
            cx.notify();
            return;
        }
        self.opened.push(OpenTab {
            conn_name: String::new(),
            db: "",
            schema: 0,
            cat: 4,
            obj: 0,
            cell: (0, 0),
            design: false,
            did: 0,
            dstate: None,
            role: false,
            rid: 0,
            rstate: None,
            query: false,
            qid: 0,
            qstate: None,
            kv: false,
            kid: 0,
            kstate: None,
            sort: None,
            head_hover: None,
            head_menu: None,
            notes: Some(notes),
            tx: crate::TxSlot(None),
            data: Vec::new(),
            baseline: Vec::new(),
            cols: Vec::new(),
            row_keys: Vec::new(),
            baseline_row_keys: Vec::new(),
            inserted: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            filter: crate::filter::FilterState::default(),
            page: None,
            load: None,
            limit_open: false,
            limit_on: true,
            limit_size: 1000,
            limit_input: None,
            sel: None,
            col_order: Vec::new(),
            col_widths: std::collections::HashMap::new(),
            scroll: ScrollHandle::new(),
        });
        let ix = self.opened.len();
        self.dock.add_tab(ix);
        self.active_tab = ix;
        cx.notify();
    }

    /// 发布说明页：页头是版本 / 日期 / GitHub 链接，正文按 Markdown 铺。
    pub fn release_notes_page(&self, t: &OpenTab, _cx: &mut Context<Self>) -> AnyElement {
        let Some(notes) = t.notes.as_ref() else {
            return div().into_any_element();
        };
        let p = self.pal();
        let muted = to_hsla(p.muted_foreground);
        let date = notes
            .published_at
            .as_deref()
            .and_then(|value| value.get(..10))
            .unwrap_or_default()
            .to_string();
        let url = notes.url.clone();

        let mut head = h_flex().items_center().gap(px(10.));
        head = head
            .child(
                div()
                    .text_size(px(24.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(to_hsla(p.foreground))
                    .child("Release Notes"),
            )
            .child(
                div()
                    .h(px(20.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded(px(10.))
                    .bg(to_hsla(p.selection))
                    .text_size(px(11.))
                    .text_color(to_hsla(p.muted_foreground))
                    .child(format!("v{}", notes.version)),
            );
        if !date.is_empty() {
            head = head.child(
                div()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child(date),
            );
        }
        if !url.is_empty() {
            head = head.child(
                div()
                    .id("release-notes-web")
                    .ml(px(4.))
                    .text_size(px(12.))
                    .text_color(to_hsla(p.accent))
                    .cursor_pointer()
                    .child("在 GitHub 上查看")
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            );
        }

        let body = markdown(&notes.notes, self.dark);
        let inner = v_flex()
            .id("release-notes-page")
            .test_support()
            .w_full()
            .max_w(px(840.))
            .gap(px(10.))
            .child(head)
            .child(div().h(px(1.)).w_full().bg(to_hsla(p.border)))
            .child(body);

        v_flex()
            .size_full()
            .bg(to_hsla(p.background))
            .overflow_y_scrollbar()
            .child(
                h_flex()
                    .w_full()
                    .justify_center()
                    .child(div().w_full().px(px(40.)).py(px(26.)).child(inner)),
            )
            .into_any_element()
    }
}

/// 行内文本样式：粗体 / 行内代码 / 链接。返回铺好的文本和块里第一个链接。
fn inline(text: &str, dark: bool) -> (StyledText, Option<String>) {
    let p = Palette::of(dark);
    let mut out = String::new();
    let mut spans: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    let mut link = None;
    let mut i = 0;
    let push = |out: &mut String, text: &str| out.push_str(text);

    while i < text.len() {
        let tail = &text[i..];
        // HTML 标签 / 注释直接丢掉：GitHub 正文里偶尔混着 <sub>、<br>
        if tail.starts_with('<') {
            if let Some(end) = tail.find('>') {
                i += end + 1;
                continue;
            }
        }
        if tail.starts_with("![") {
            // 图片只留说明文字，按链接处理
            i += 1;
            continue;
        }
        if let Some(rest) = tail.strip_prefix("**").or_else(|| tail.strip_prefix("__")) {
            let close = rest.find("**").or_else(|| rest.find("__"));
            if let Some(end) = close {
                let start = out.len();
                push(&mut out, &rest[..end]);
                spans.push((
                    start..out.len(),
                    HighlightStyle {
                        font_weight: Some(gpui::FontWeight::SEMIBOLD),
                        ..Default::default()
                    },
                ));
                i += 2 + end + 2;
                continue;
            }
        }
        if tail.starts_with('`') {
            if let Some(end) = tail[1..].find('`') {
                let start = out.len();
                push(&mut out, &tail[1..1 + end]);
                spans.push((
                    start..out.len(),
                    HighlightStyle {
                        background_color: Some(to_hsla(p.selection)),
                        ..Default::default()
                    },
                ));
                i += 1 + end + 1;
                continue;
            }
        }
        if tail.starts_with('[') {
            if let Some(mid) = tail.find("](") {
                if let Some(close) = tail[mid + 2..].find(')') {
                    let label = &tail[1..mid];
                    let url = tail[mid + 2..mid + 2 + close].trim().to_string();
                    let start = out.len();
                    push(&mut out, label);
                    spans.push((
                        start..out.len(),
                        HighlightStyle {
                            color: Some(to_hsla(p.accent)),
                            underline: Some(gpui::UnderlineStyle {
                                thickness: px(1.),
                                color: Some(to_hsla(p.accent)),
                                wavy: false,
                            }),
                            ..Default::default()
                        },
                    ));
                    if link.is_none() {
                        link = Some(url);
                    }
                    i += mid + 2 + close + 1;
                    continue;
                }
            }
        }
        let ch = tail.chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }

    (StyledText::new(out).with_highlights(spans), link)
}

/// 段落：有链接时整块可点（发布说明里的链接通常一句话一个）。
fn paragraph(text: &str, dark: bool) -> AnyElement {
    let p = Palette::of(dark);
    let (styled, link) = inline(text, dark);
    let el = div()
        .text_size(px(13.))
        .line_height(px(21.))
        .text_color(to_hsla(p.foreground))
        .child(styled);
    match link {
        Some(url) => el
            .id(format!("notes-link-{url}"))
            .cursor_pointer()
            .on_click(move |_, _, cx| cx.open_url(&url))
            .into_any_element(),
        None => el.into_any_element(),
    }
}

/// 块级 Markdown：逐个消费行，铺成纵向流。
fn markdown(notes: &str, dark: bool) -> Div {
    let p = Palette::of(dark);
    let muted = to_hsla(p.muted_foreground);
    let lines: Vec<&str> = notes.lines().collect();
    let mut col = v_flex().w_full().gap(px(6.));
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }
        if trimmed.starts_with("<!--") {
            while i < lines.len() && !lines[i].contains("-->") {
                i += 1;
            }
            i += 1;
            continue;
        }
        if trimmed.starts_with("```") {
            let mut code = String::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                code.push_str(lines[i]);
                code.push('\n');
                i += 1;
            }
            i += 1;
            col = col.child(
                div()
                    .w_full()
                    .p(px(10.))
                    .rounded(px(4.))
                    .bg(to_hsla(p.chrome))
                    .border(px(0.5))
                    .border_color(to_hsla(p.border))
                    .font_family("Consolas")
                    .text_size(px(12.))
                    .text_color(to_hsla(p.foreground))
                    .child(code.trim_end_matches('\n').to_string()),
            );
            continue;
        }
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count().min(3);
            let text = trimmed[level..].trim();
            let size = match level {
                1 => 20.,
                2 => 16.,
                _ => 14.,
            };
            let (styled, _) = inline(text, dark);
            col = col.child(
                div()
                    .pt(px(if level == 1 { 6. } else { 10. }))
                    .text_size(px(size))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(to_hsla(p.foreground))
                    .child(styled),
            );
            i += 1;
            continue;
        }
        if is_rule(trimmed) {
            col = col.child(div().h(px(1.)).w_full().my(px(4.)).bg(to_hsla(p.border)));
            i += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('>') {
            let (styled, _) = inline(rest.trim(), dark);
            col = col.child(
                h_flex()
                    .items_start()
                    .gap(px(8.))
                    .child(div().w(px(2.)).self_stretch().bg(to_hsla(p.border)))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.5))
                            .line_height(px(20.))
                            .text_color(muted)
                            .child(styled),
                    ),
            );
            i += 1;
            continue;
        }
        let indent = raw.len() - trimmed.len();
        if let Some(text) = bullet(trimmed) {
            col = col.child(list_row("•", text, indent, dark));
            i += 1;
            continue;
        }
        if let Some((marker, text)) = numbered(trimmed) {
            col = col.child(list_row(&marker, text, indent, dark));
            i += 1;
            continue;
        }
        // 普通段落：一直吃到空行或下一个块开头，折行按空格接回去
        let mut para = String::new();
        while i < lines.len() {
            let next = lines[i].trim();
            if next.is_empty() || starts_block(next) {
                break;
            }
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(next);
            i += 1;
        }
        if !para.is_empty() {
            col = col.child(paragraph(&para, dark));
        }
    }
    col
}

fn starts_block(line: &str) -> bool {
    line.starts_with('#')
        || line.starts_with("```")
        || line.starts_with('>')
        || line == "---"
        || line == "***"
        || bullet(line).is_some()
        || numbered(line).is_some()
}

fn is_rule(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    compact.len() >= 3 && compact.chars().all(|c| c == '-' || c == '*' || c == '_')
}

fn bullet(line: &str) -> Option<&str> {
    ["- ", "* ", "+ "]
        .iter()
        .find_map(|marker| line.strip_prefix(marker))
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn numbered(line: &str) -> Option<(String, &str)> {
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let rest = line[digits..].strip_prefix(". ")?;
    Some((format!("{}.", &line[..digits]), rest.trim()))
}

fn list_row(marker: &str, text: &str, indent: usize, dark: bool) -> AnyElement {
    let p = Palette::of(dark);
    let (styled, link) = inline(text, dark);
    let row = h_flex()
        .w_full()
        .items_start()
        .gap(px(8.))
        .pl(px(indent as f32 * 4.))
        .child(
            div()
                .flex_none()
                .w(px(14.))
                .text_right()
                .text_size(px(13.))
                .line_height(px(21.))
                .text_color(to_hsla(p.muted_foreground))
                .child(marker.to_string()),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(13.))
                .line_height(px(21.))
                .text_color(to_hsla(p.foreground))
                .child(styled),
        );
    match link {
        Some(url) => row
            .id(format!("notes-item-{url}"))
            .cursor_pointer()
            .on_click(move |_, _, cx| cx.open_url(&url))
            .into_any_element(),
        None => row.into_any_element(),
    }
}
