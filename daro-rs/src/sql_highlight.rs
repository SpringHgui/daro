//! SQL 语法高亮（迁移自 Flutter 版 `lib/theme/sql_highlight.dart` 的配色与
//! re_highlight 的 SQL 语法口径）。
//!
//! Flutter 版用 `re_highlight` 的 `langSql` 语法 + 两张固定配色表
//! （明亮近似 VS Code Light+，暗黑近似 Dark+）。这里把「词法 + 配色」都
//! 落成纯函数，再实现 gpui 编辑器的 [`InputHighlighter`] 接口：
//!
//! - 词法只认 SQL 里会变色的大类：关键字 / 字面量 / 类型 / 内置函数 /
//!   字符串 / 数字 / 注释；标识符与运算符用正文色（与 Flutter 一致）；
//! - 配色是**主题无关的固定中调色**，不进主题定制弹窗；明暗由
//!   [`set_dark`] 更新一个全局标志，编辑器在下一帧重新取色，不用重建编辑器；
//! - 每次文本变更整段重扫（SQL 编辑器量级足够小），增量留到以后再说。
//!
//! 偏移口径：gpui 编辑器按**字节**偏移询问样式（`element.rs` 里传的是
//! `byte_start..byte_end`），所以词法产出的 range 也是字节偏移。

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::component::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, Rope,
};
use gpui_kit::{Context, FontStyle, HighlightStyle, Hsla, Rgba, SharedString, Window};

/// 当前是否暗色主题（由 `apply_palette` 更新）
static SQL_DARK: AtomicBool = AtomicBool::new(false);

pub fn set_dark(dark: bool) {
    SQL_DARK.store(dark, Ordering::Relaxed);
}

/// 词法大类 → 配色表的 scope
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keyword,
    Literal,
    Type,
    Builtin,
    String,
    Number,
    Comment,
}

impl Kind {
    /// 对应 Flutter 配色表的键
    pub fn scope(self) -> &'static str {
        match self {
            Kind::Keyword => "keyword",
            Kind::Literal => "literal",
            Kind::Type => "type",
            Kind::Builtin => "built_in",
            Kind::String => "string",
            Kind::Number => "number",
            Kind::Comment => "comment",
        }
    }
}

fn rgb(hex: u32) -> Hsla {
    let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
    let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
    let b = (hex & 0xFF) as f32 / 255.0;
    Hsla::from(Rgba { r, g, b, a: 1.0 })
}

/// 配色表：与 Flutter `sqlLightTheme` / `sqlDarkTheme` 逐条对齐
pub fn color_for(kind: Kind, dark: bool) -> Hsla {
    let hex = match (kind, dark) {
        (Kind::Keyword, false) => 0x0000ff,
        (Kind::Keyword, true) => 0x569cd6,
        (Kind::Literal, false) => 0x001080,
        (Kind::Literal, true) => 0x569cd6,
        (Kind::Type, false) => 0x267f99,
        (Kind::Type, true) => 0x4ec9b0,
        (Kind::Builtin, false) => 0x795e26,
        (Kind::Builtin, true) => 0xdcdcaa,
        (Kind::String, false) => 0xa31515,
        (Kind::String, true) => 0xce9178,
        (Kind::Number, false) => 0x098658,
        (Kind::Number, true) => 0xb5cea8,
        (Kind::Comment, false) => 0x008000,
        (Kind::Comment, true) => 0x6a9955,
    };
    rgb(hex)
}

fn style_for(kind: Kind) -> HighlightStyle {
    let dark = SQL_DARK.load(Ordering::Relaxed);
    let mut style = HighlightStyle {
        color: Some(color_for(kind, dark)),
        ..Default::default()
    };
    if kind == Kind::Comment {
        style.font_style = Some(FontStyle::Italic);
    }
    style
}

const KEYWORDS: &[&str] = &[
    "ADD", "ALL", "ALTER", "AND", "ANY", "AS", "ASC", "BEGIN", "BETWEEN", "BY", "CASE",
    "CAST", "CHECK", "COLUMN", "COMMIT", "CONSTRAINT", "CREATE", "CROSS", "CURRENT_DATE",
    "CURRENT_TIME", "CURRENT_TIMESTAMP", "DATABASE", "DEFAULT", "DELETE", "DESC", "DISTINCT",
    "DROP", "ELSE", "END", "EXISTS", "EXPLAIN", "FALSE", "FOR", "FOREIGN", "FROM", "FULL",
    "GROUP", "HAVING", "IF", "IN", "INDEX", "INNER", "INSERT", "INTERVAL", "INTO", "IS",
    "JOIN", "KEY", "LEFT", "LIKE", "LIMIT", "LOCK", "MERGE", "NATURAL", "NOT", "NULL",
    "OFFSET", "ON", "OR", "ORDER", "OUTER", "PRIMARY", "REFERENCES", "RENAME", "REPLACE",
    "RETURNING", "RIGHT", "ROLLBACK", "SCHEMA", "SELECT", "SET", "SHOW", "TABLE", "THEN",
    "TRANSACTION", "TRIGGER", "TRUE", "TRUNCATE", "UNION", "UNIQUE", "UPDATE", "USE",
    "USING", "VALUES", "VIEW", "WHEN", "WHERE", "WITH",
];

const TYPES: &[&str] = &[
    "BIGINT", "BINARY", "BIT", "BLOB", "BOOLEAN", "CHAR", "DATE", "DATETIME", "DECIMAL",
    "DOUBLE", "ENUM", "FLOAT", "INT", "INTEGER", "JSON", "LONGTEXT", "MEDIUMINT", "NUMERIC",
    "REAL", "SET", "SMALLINT", "TEXT", "TIME", "TIMESTAMP", "TINYINT", "UUID", "VARBINARY",
    "VARCHAR",
];

const BUILTINS: &[&str] = &[
    "ABS", "AVG", "CEIL", "COALESCE", "CONCAT", "COUNT", "CURDATE", "DATE_ADD", "DATE_SUB",
    "FLOOR", "FORMAT", "GROUP_CONCAT", "IFNULL", "LENGTH", "LOWER", "MAX", "MIN", "NOW",
    "NULLIF", "ROUND", "SUBSTRING", "SUM", "TRIM", "UPPER",
];

fn classify(word: &str) -> Option<Kind> {
    let upper = word.to_ascii_uppercase();
    if KEYWORDS.contains(&upper.as_str()) {
        Some(Kind::Keyword)
    } else if TYPES.contains(&upper.as_str()) {
        Some(Kind::Type)
    } else if BUILTINS.contains(&upper.as_str()) {
        Some(Kind::Builtin)
    } else {
        None
    }
}

fn is_word_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// 全量词法：返回按偏移升序、互不重叠的 (字节区间, 大类)。
pub fn lex_sql(src: &str) -> Vec<(Range<usize>, Kind)> {
    let bytes = src.as_bytes();
    let mut out: Vec<(Range<usize>, Kind)> = Vec::new();
    let mut i = 0usize;
    let n = bytes.len();
    while i < n {
        let c = src[i..].chars().next().unwrap();
        let len = c.len_utf8();
        // 行注释：-- 或 #
        if (c == '-' && bytes.get(i + 1) == Some(&b'-')) || c == '#' {
            let start = i;
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            out.push((start..i, Kind::Comment));
            continue;
        }
        // 块注释：可嵌套（PG 口径，与 sql_split 一致）
        if c == '/' && bytes.get(i + 1) == Some(&b'*') {
            let start = i;
            let mut depth = 0usize;
            while i < n {
                if src[i..].starts_with("/*") {
                    depth += 1;
                    i += 2;
                } else if src[i..].starts_with("*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += src[i..].chars().next().unwrap().len_utf8();
                }
            }
            out.push((start..i.min(n), Kind::Comment));
            continue;
        }
        // 字符串：单引号（'' 转义）/ 双引号 / 反引号 / 方括号
        if matches!(c, '\'' | '"' | '`' | '[') {
            let close = if c == '[' { ']' } else { c };
            let start = i;
            i += len;
            while i < n {
                let ch = src[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == close {
                    if close != ']' && i < n && src[i..].starts_with(close) {
                        // 双写转义：继续留在字符串里
                        i += close.len_utf8();
                        continue;
                    }
                    break;
                }
                if ch == '\\' && i < n {
                    // 反斜杠转义下一个字符
                    let esc = src[i..].chars().next().unwrap();
                    i += esc.len_utf8();
                }
            }
            out.push((start..i, Kind::String));
            continue;
        }
        // 数字：十进制 / 小数 / 0x 十六进制
        if c.is_ascii_digit() {
            let start = i;
            if c == '0' && matches!(bytes.get(i + 1), Some(b'x') | Some(b'X')) {
                i += 2;
                while i < n && (bytes[i] as char).is_ascii_hexdigit() {
                    i += 1;
                }
            } else {
                while i < n && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                if i < n && (bytes[i] == b'e' || bytes[i] == b'E') {
                    i += 1;
                    if i < n && (bytes[i] == b'+' || bytes[i] == b'-') {
                        i += 1;
                    }
                    while i < n && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            out.push((start..i, Kind::Number));
            continue;
        }
        // 变量：@name / @@global / :param
        if c == '@' || c == ':' {
            let start = i;
            i += len;
            while i < n && is_word_char(src[i..].chars().next().unwrap()) {
                i += src[i..].chars().next().unwrap().len_utf8();
            }
            if i > start + len {
                out.push((start..i, Kind::Literal));
            }
            continue;
        }
        // 单词
        if is_word_start(c) {
            let start = i;
            while i < n && is_word_char(src[i..].chars().next().unwrap()) {
                i += src[i..].chars().next().unwrap().len_utf8();
            }
            if let Some(kind) = classify(&src[start..i]) {
                out.push((start..i, kind));
            }
            continue;
        }
        i += len;
    }
    out
}

/// gpui 编辑器的 SQL 高亮适配器
pub struct SqlHighlighter {
    tokens: Vec<(Range<usize>, Kind)>,
}

impl Default for SqlHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl SqlHighlighter {
    pub fn new() -> Self {
        Self {
            tokens: Vec::new(),
        }
    }
}

impl InputHighlighter for SqlHighlighter {
    fn language(&self) -> SharedString {
        "sql".into()
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
        self.tokens = lex_sql(&src);
    }

    /// 返回完整覆盖 `range` 的样式段：命中 token 用对应颜色，其余用默认样式
    /// （默认样式 = 正文色，由编辑器自身的 editor_style 决定）。
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

    fn fold_ranges(&self, _text: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(Range<usize>, Kind)> {
        lex_sql(src)
    }

    #[test]
    fn lexes_keywords_strings_numbers_comments() {
        let src = "SELECT name, 'a''b' FROM t -- tail\nWHERE id = 42 AND x = 0x1F";
        let toks = kinds(src);
        assert!(toks.contains(&(0..6, Kind::Keyword))); // SELECT
        assert!(toks.iter().any(|(r, k)| *k == Kind::String && &src[r.clone()] == "'a''b'"));
        assert!(toks.iter().any(|(r, k)| *k == Kind::Number && &src[r.clone()] == "42"));
        assert!(toks.iter().any(|(r, k)| *k == Kind::Number && &src[r.clone()] == "0x1F"));
        assert!(toks
            .iter()
            .any(|(r, k)| *k == Kind::Comment && src[r.clone()].starts_with("-- tail")));
        assert!(toks.iter().any(|(r, k)| *k == Kind::Keyword && &src[r.clone()] == "WHERE"));
    }

    #[test]
    fn lexes_types_builtins_and_nested_block_comments() {
        let src = "/* a /* b */ c */ CREATE TABLE t (id INT, n VARCHAR(32), c COUNT(*))";
        let toks = kinds(src);
        assert_eq!(toks[0].1, Kind::Comment);
        assert_eq!(&src[toks[0].0.clone()], "/* a /* b */ c */");
        assert!(toks.iter().any(|(r, k)| *k == Kind::Type && &src[r.clone()] == "VARCHAR"));
        assert!(toks.iter().any(|(r, k)| *k == Kind::Builtin && &src[r.clone()] == "COUNT"));
    }

    #[test]
    fn backtick_and_bracket_identifiers_are_strings() {
        let src = "SELECT `a b`, [c d] FROM t";
        let toks = kinds(src);
        assert!(toks.iter().any(|(r, k)| *k == Kind::String && &src[r.clone()] == "`a b`"));
        assert!(toks.iter().any(|(r, k)| *k == Kind::String && &src[r.clone()] == "[c d]"));
    }

    #[test]
    fn styles_cover_the_requested_range_without_gaps() {
        let mut h = SqlHighlighter::new();
        h.tokens = lex_sql("SELECT 1");
        let styles = h.styles(&(0..8), &NoResolver);
        assert_eq!(styles.first().unwrap().0.start, 0);
        assert_eq!(styles.last().unwrap().0.end, 8);
        let mut cursor = 0;
        for (r, _) in &styles {
            assert_eq!(r.start, cursor);
            cursor = r.end;
        }
    }

    struct NoResolver;
    impl HighlightStyleResolver for NoResolver {
        fn style(&self, _: &str) -> Option<HighlightStyle> {
            None
        }
    }
}
