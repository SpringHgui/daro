//! JSON 语法高亮：给 Redis 字符串值面板的 Json 视图用。
//!
//! 形态照 [`crate::sql_highlight`]：词法与配色都是纯函数，再套一层
//! gpui 编辑器的 [`InputHighlighter`]。配色沿用 SQL 那套主题无关的中调色
//! （明亮近似 VS Code Light+，暗黑近似 Dark+），键名用 VS Code JSON 的蓝。
//!
//! 偏移口径与 SQL 一致：gpui 编辑器按**字节**偏移询问样式，词法产出的
//! range 也是字节偏移。

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::component::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, Rope,
};
use gpui_kit::{Context, HighlightStyle, Hsla, Rgba, SharedString, Window};

/// 当前是否暗色主题（与 SQL 高亮同一条更新路径）
static JSON_DARK: AtomicBool = AtomicBool::new(false);

pub fn set_dark(dark: bool) {
    JSON_DARK.store(dark, Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 对象里冒号左边的字符串
    Key,
    String,
    Number,
    /// true / false / null
    Keyword,
}

fn rgb(hex: u32) -> Hsla {
    let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
    let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
    let b = (hex & 0xFF) as f32 / 255.0;
    Hsla::from(Rgba { r, g, b, a: 1.0 })
}

pub fn color_for(kind: Kind, dark: bool) -> Hsla {
    let hex = match (kind, dark) {
        (Kind::Key, false) => 0x0451A5,
        (Kind::Key, true) => 0x9CDCFE,
        (Kind::String, false) => 0xA31515,
        (Kind::String, true) => 0xCE9178,
        (Kind::Number, false) => 0x098658,
        (Kind::Number, true) => 0xB5CEA8,
        (Kind::Keyword, false) => 0x0000FF,
        (Kind::Keyword, true) => 0x569CD6,
    };
    rgb(hex)
}

fn style_for(kind: Kind) -> HighlightStyle {
    HighlightStyle {
        color: Some(color_for(kind, JSON_DARK.load(Ordering::Relaxed))),
        ..Default::default()
    }
}

/// JSON 词法：只染会变的四类，标点（`{}[]:,`）用正文色。
///
/// 不追求严格校验（合法性由 [`crate::value_format::validate`] 在保存时把关），
/// 用户改到一半的残缺 JSON 也要能染色，所以遇到意外字符就跳过继续扫。
pub fn lex_json(src: &str) -> Vec<(Range<usize>, Kind)> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() {
                    match b[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                // 后面紧跟冒号（可隔空白）的是键名
                let mut j = i;
                while j < b.len() && matches!(b[j], b' ' | b'\t' | b'\r' | b'\n') {
                    j += 1;
                }
                let kind = if j < b.len() && b[j] == b':' { Kind::Key } else { Kind::String };
                out.push((start..i.min(b.len()), kind));
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                i += 1;
                while i < b.len() && matches!(b[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') {
                    i += 1;
                }
                out.push((start..i, Kind::Number));
            }
            b't' if src[i..].starts_with("true") => {
                out.push((i..i + 4, Kind::Keyword));
                i += 4;
            }
            b'f' if src[i..].starts_with("false") => {
                out.push((i..i + 5, Kind::Keyword));
                i += 5;
            }
            b'n' if src[i..].starts_with("null") => {
                out.push((i..i + 4, Kind::Keyword));
                i += 4;
            }
            _ => i += 1,
        }
    }
    out
}

/// gpui 编辑器的 JSON 高亮适配器
pub struct JsonHighlighter {
    tokens: Vec<(Range<usize>, Kind)>,
}

impl Default for JsonHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonHighlighter {
    pub fn new() -> Self {
        Self { tokens: Vec::new() }
    }
}

impl InputHighlighter for JsonHighlighter {
    fn language(&self) -> SharedString {
        "json".into()
    }

    fn update(
        &mut self,
        _edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        let src: String = text.chars().collect();
        self.tokens = lex_json(&src);
    }

    /// 返回完整覆盖 `range` 的样式段：命中 token 用对应颜色，其余用默认样式。
    fn styles(
        &self,
        range: &Range<usize>,
        _resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let mut out: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
        let mut cursor = range.start;
        for (span, kind) in &self.tokens {
            if span.end <= range.start || span.start >= range.end {
                continue;
            }
            let start = span.start.max(range.start);
            let end = span.end.min(range.end);
            if cursor < start {
                out.push((cursor..start, HighlightStyle::default()));
            }
            out.push((start..end, style_for(*kind)));
            cursor = end;
        }
        if cursor < range.end {
            out.push((cursor..range.end, HighlightStyle::default()));
        }
        out
    }

    /// 折叠暂未开启（值面板的编辑器没开 `folding`），保留空实现满足接口。
    fn fold_ranges(&self, _text: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(&str, Kind)> {
        lex_json(src)
            .into_iter()
            .map(|(r, k)| (&src[r], k))
            .collect()
    }

    #[test]
    fn keys_and_value_strings_are_distinguished() {
        let src = r#"{"name": "x", "nested": {"k":"v"}}"#;
        let got = kinds(src);
        assert!(got.contains(&("\"name\"", Kind::Key)));
        assert!(got.contains(&("\"x\"", Kind::String)));
        assert!(got.contains(&("\"k\"", Kind::Key)));
        assert!(got.contains(&("\"v\"", Kind::String)));
    }

    #[test]
    fn escaped_quote_does_not_end_the_string() {
        let src = r#"{"a": "he said \"hi\" ok", "b": 1}"#;
        let got = kinds(src);
        assert!(got.contains(&("\"he said \\\"hi\\\" ok\"", Kind::String)));
        assert!(got.contains(&("\"b\"", Kind::Key)));
    }

    #[test]
    fn numbers_and_literals() {
        let src = "[1, -0.5e-3, true, false, null]";
        let got = kinds(src);
        assert!(got.contains(&("1", Kind::Number)));
        assert!(got.contains(&("-0.5e-3", Kind::Number)));
        assert_eq!(got.iter().filter(|(_, k)| *k == Kind::Keyword).count(), 3);
    }

    #[test]
    fn half_edited_json_still_lexes_instead_of_bailing() {
        // 用户改到一半：缺右括号、键后没冒号，都不该让整个编辑器失去染色
        let src = r#"{"a": 12, "pe"#;
        let got = kinds(src);
        assert!(got.contains(&("\"a\"", Kind::Key)));
        assert!(got.contains(&("12", Kind::Number)));
        assert!(got.contains(&("\"pe", Kind::String)));
    }
}
