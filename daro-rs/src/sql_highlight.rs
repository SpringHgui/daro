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
        (Kind::Keyword, false) => 0x097bf7,
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

fn style_for(kind: Kind, filter: bool) -> HighlightStyle {
    let dark = SQL_DARK.load(Ordering::Relaxed);
    let color = if filter && kind == Kind::String {
        // 筛选面板照参考图的鲜红；SQL 编辑器沿用 Flutter 版的暗红
        rgb(if dark { 0xff5c7a } else { 0xff1744 })
    } else {
        color_for(kind, dark)
    };
    let mut style = HighlightStyle {
        color: Some(color),
        ..Default::default()
    };
    if kind == Kind::Comment {
        style.font_style = Some(FontStyle::Italic);
    }
    style
}

const KEYWORDS: &[&str] = &[
    "ADD", "ALL", "ALTER", "AND", "ANY", "AS", "ASC", "BEGIN", "BETWEEN", "BY", "CASE",
    "CAST", "CHECK", "COLUMN", "COMMIT", "CONCURRENTLY", "CONSTRAINT", "CREATE", "CROSS",
    "CURRENT_DATE",
    "CURRENT_TIME", "CURRENT_TIMESTAMP", "DATABASE", "DEFAULT", "DELETE", "DESC", "DISTINCT",
    "DROP", "ELSE", "END", "EXISTS", "EXPLAIN", "FALSE", "FOR", "FOREIGN", "FROM", "FULL",
    "GROUP", "HAVING", "IF", "IN", "INDEX", "INNER", "INSERT", "INTERVAL", "INTO", "IS",
    "JOIN", "KEY", "LEFT", "LIKE", "LIMIT", "LOCK", "MATERIALIZED", "MERGE", "NATURAL",
    "NOT", "NULL",
    "OFFSET", "ON", "OR", "ORDER", "OUTER", "PRIMARY", "REFERENCES", "REFRESH", "RENAME",
    "REPLACE",
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
    lex_sql_with(src, true)
}

/// 筛选面板文本模式的口径：只有单引号是字符串，双引号 / 反引号 / 方括号
/// 都当引用标识符（正文色）。参考图里 `"id" = '15'` 只有 '15' 是红的。
pub fn lex_sql_filter(src: &str) -> Vec<(Range<usize>, Kind)> {
    lex_sql_with(src, false)
}

fn lex_sql_with(src: &str, all_quotes: bool) -> Vec<(Range<usize>, Kind)> {
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
        // 字符串：单引号（'' 转义）/ 双引号 / 反引号 / 方括号。
        // 筛选口径下除单引号外都是引用标识符，整段跳过、不产出配色。
        if matches!(c, '\'' | '"' | '`' | '[') {
            let close = if c == '[' { ']' } else { c };
            let start = i;
            i += len;
            if !all_quotes && c != '\'' {
                while i < n {
                    let ch = src[i..].chars().next().unwrap();
                    i += ch.len_utf8();
                    if ch == close {
                        break;
                    }
                }
                continue;
            }
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

/// 折叠块种类：括号组由 `)` 关闭，其余按关键字配对关闭。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    Paren,
    Begin,
    Case,
    If,
    Loop,
    While,
    Repeat,
}

fn line_starts(src: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in src.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// 若 `i` 处开始的是注释 / 字符串 / 引用标识符 / 美元引号，返回结束偏移。
/// 折叠扫描与关键字前瞻都用它跳过「不该当成代码」的内容。
fn skip_quoted_or_comment(src: &str, i: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let c = src[i..].chars().next()?;
    if (c == '-' && bytes.get(i + 1) == Some(&b'-')) || c == '#' {
        let mut j = i;
        while j < n && bytes[j] != b'\n' {
            j += 1;
        }
        return Some(j);
    }
    if c == '/' && bytes.get(i + 1) == Some(&b'*') {
        let mut j = i;
        let mut depth = 0usize;
        while j < n {
            if src[j..].starts_with("/*") {
                depth += 1;
                j += 2;
            } else if src[j..].starts_with("*/") {
                depth = depth.saturating_sub(1);
                j += 2;
                if depth == 0 {
                    break;
                }
            } else {
                j += src[j..].chars().next().unwrap().len_utf8();
            }
        }
        return Some(j.min(n));
    }
    if matches!(c, '\'' | '"' | '`' | '[') {
        let close = if c == '[' { ']' } else { c };
        let mut j = i + c.len_utf8();
        while j < n {
            let ch = src[j..].chars().next().unwrap();
            j += ch.len_utf8();
            if ch == close {
                if close != ']' && src[j..].starts_with(close) {
                    j += close.len_utf8();
                    continue;
                }
                break;
            }
            if ch == '\\' && j < n {
                j += src[j..].chars().next().unwrap().len_utf8();
            }
        }
        return Some(j.min(n));
    }
    // PostgreSQL 美元引号：$tag$ ... $tag$（`$1` 这类参数不会命中）
    if c == '$' {
        let mut j = i + 1;
        while j < n && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
            j += 1;
        }
        if j < n && bytes[j] == b'$' {
            let delim = &src[i..=j];
            return Some(match src[j + 1..].find(delim) {
                Some(k) => j + 1 + k + delim.len(),
                None => n,
            });
        }
    }
    None
}

/// 从 `from` 扫到顶层 `;`（语句结束）或另一个块开始关键字之前，
/// 判断 `keyword` 是否先出现。用来区分 `IF ... THEN` 块与 `IF(...)` 函数 /
/// `IF EXISTS`，以及 `WHILE ... DO`。
fn keyword_precedes_terminator(src: &str, mut i: usize, keyword: &str) -> bool {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut depth = 0usize;
    while i < n {
        if let Some(j) = skip_quoted_or_comment(src, i) {
            i = j.max(i + 1);
            continue;
        }
        let c = src[i..].chars().next().unwrap();
        if is_word_start(c) {
            let start = i;
            let mut end = i;
            while end < n && is_word_char(bytes[end] as char) {
                end += 1;
            }
            if depth == 0 {
                let word = src[start..end].to_ascii_uppercase();
                if word == keyword {
                    return true;
                }
                if matches!(
                    word.as_str(),
                    "BEGIN" | "END" | "LOOP" | "REPEAT" | "WHILE" | "CASE"
                ) {
                    return false;
                }
            }
            i = end;
            continue;
        }
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => return false,
            _ => {}
        }
        i += c.len_utf8();
    }
    false
}

/// 折叠候选：整行闭区间（0 基）。只返回跨行区间；折叠后首行与末行仍显示、
/// 中间行收起，与 gpui 编辑器 fold map 的约定一致。
pub fn fold_ranges_sql(src: &str) -> Vec<(usize, usize)> {
    let starts = line_starts(src);
    let line_of = |off: usize| starts.partition_point(|s| *s <= off).saturating_sub(1);
    let bytes = src.as_bytes();
    let n = bytes.len();
    let last_line = line_of(n.saturating_sub(1));
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut stack: Vec<(Block, usize)> = Vec::new();
    let mut i = 0usize;
    while i < n {
        if let Some(j) = skip_quoted_or_comment(src, i) {
            // 多行块注释本身值得折叠；行注释 / 字符串不产生候选
            let c = src[i..].chars().next().unwrap();
            if c == '/' && bytes.get(i + 1) == Some(&b'*') {
                let a = line_of(i);
                let b = line_of((j - 1).max(i));
                if b > a {
                    out.push((a, b));
                }
            }
            i = j.max(i + 1);
            continue;
        }
        let c = src[i..].chars().next().unwrap();
        if is_word_start(c) {
            let start = i;
            let mut end = i;
            while end < n && is_word_char(bytes[end] as char) {
                end += 1;
            }
            match src[start..end].to_ascii_uppercase().as_str() {
                "BEGIN" => {
                    // BEGIN; / BEGIN WORK / BEGIN TRANSACTION 是事务语句，不是块
                    let (next, _) = next_word(src, end);
                    let mut k = end;
                    while k < n && bytes[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    let bare = k >= n || bytes[k] == b';';
                    let transaction =
                        matches!(next.as_deref(), Some("TRANSACTION") | Some("WORK"));
                    if !bare && !transaction {
                        stack.push((Block::Begin, start));
                    }
                }
                "CASE" => stack.push((Block::Case, start)),
                "IF" => {
                    if keyword_precedes_terminator(src, end, "THEN") {
                        stack.push((Block::If, start));
                    }
                }
                "WHILE" => {
                    if keyword_precedes_terminator(src, end, "DO") {
                        stack.push((Block::While, start));
                    }
                }
                "LOOP" => stack.push((Block::Loop, start)),
                "REPEAT" => {
                    // REPEAT(str, n) 是函数：后面直接跟 '(' 就不当块开始
                    let mut k = end;
                    while k < n && bytes[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    if k >= n || bytes[k] != b'(' {
                        stack.push((Block::Repeat, start));
                    }
                }
                "END" => {
                    // END IF / END LOOP / END WHILE / END REPEAT / END CASE
                    let (next, next_end) = next_word(src, end);
                    let kind = match next.as_deref() {
                        Some("IF") => Some(Block::If),
                        Some("LOOP") => Some(Block::Loop),
                        Some("WHILE") => Some(Block::While),
                        Some("REPEAT") => Some(Block::Repeat),
                        Some("CASE") => Some(Block::Case),
                        _ => None,
                    };
                    if kind.is_some() {
                        end = next_end;
                    }
                    let matched = match kind {
                        Some(k) => stack.iter().rposition(|(b, _)| *b == k),
                        // 裸 END 关闭最近的 BEGIN / CASE（CASE 表达式也以 END 收尾）
                        None => stack
                            .iter()
                            .rposition(|(b, _)| matches!(b, Block::Begin | Block::Case)),
                    }
                    .or_else(|| stack.iter().rposition(|(b, _)| *b == Block::Begin));
                    if let Some(pos) = matched {
                        let close_line = line_of(start);
                        // 写法不规整时，END 之前的未闭合块一并收尾
                        while stack.len() > pos {
                            let (_, open) = stack.pop().unwrap();
                            let open_line = line_of(open);
                            if close_line > open_line {
                                out.push((open_line, close_line));
                            }
                        }
                    }
                }
                _ => {}
            }
            i = end;
            continue;
        }
        match c {
            '(' => stack.push((Block::Paren, i)),
            ')' => {
                if let Some(pos) = stack.iter().rposition(|(b, _)| *b == Block::Paren) {
                    let (_, open) = stack.remove(pos);
                    let open_line = line_of(open);
                    let close_line = line_of(i);
                    if close_line > open_line {
                        out.push((open_line, close_line));
                    }
                }
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    // 未闭合的块折到文末，编辑到一半也能收起
    while let Some((_, open)) = stack.pop() {
        let open_line = line_of(open);
        if last_line > open_line {
            out.push((open_line, last_line));
        }
    }
    // 同一首行只留最宽的区间（fold map 按 start_line 去重）
    out.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    out.dedup_by_key(|r| r.0);
    out
}

/// 跳过水平空白后的下一个单词（大写）及其结束偏移；跨行不算，避免把两条
/// 语句粘成一个配对（`END\nIF ...`）。
fn next_word(src: &str, mut i: usize) -> (Option<String>, usize) {
    let bytes = src.as_bytes();
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r') {
        i += 1;
    }
    let start = i;
    while i < bytes.len() && is_word_char(bytes[i] as char) {
        i += 1;
    }
    if i > start {
        (Some(src[start..i].to_ascii_uppercase()), i)
    } else {
        (None, start)
    }
}

/// gpui 编辑器的 SQL 高亮适配器
pub struct SqlHighlighter {
    tokens: Vec<(Range<usize>, Kind)>,
    filter: bool,
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
            filter: false,
        }
    }

    /// 筛选面板文本模式：引号标识符不染色，字符串用参考图的鲜红。
    pub fn filter() -> Self {
        Self {
            tokens: Vec::new(),
            filter: true,
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
        self.tokens = if self.filter {
            lex_sql_filter(&src)
        } else {
            lex_sql(&src)
        };
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
            out.push((start..end, style_for(*kind, self.filter)));
            cursor = end;
        }
        if cursor < range.end {
            out.push((cursor..range.end, HighlightStyle::default()));
        }
        out
    }

    fn fold_ranges(&self, text: &Rope) -> Vec<FoldRange> {
        if self.filter {
            return Vec::new();
        }
        let src: String = text.chars().collect();
        fold_ranges_sql(&src)
            .into_iter()
            .map(|(start_line, end_line)| FoldRange::new(start_line, end_line))
            .collect()
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

    #[test]
    fn filter_lexer_leaves_quoted_identifiers_plain() {
        let src = "\"id\" = '15'";
        let toks = lex_sql_filter(src);
        // 双引号标识符不产出 token（正文色），单引号才是字符串
        assert!(!toks.iter().any(|(r, _)| &src[r.clone()] == "\"id\""));
        assert_eq!(
            toks.iter()
                .find(|(_, k)| *k == Kind::String)
                .map(|(r, _)| &src[r.clone()]),
            Some("'15'")
        );
        // 通用口径仍把双引号当字符串
        assert!(lex_sql(src).iter().any(|(r, k)| *k == Kind::String && &src[r.clone()] == "\"id\""));
    }

    #[test]
    fn folds_procedural_blocks_nested() {
        let src = "BEGIN\n  IF x THEN\n    SELECT 1;\n  END IF;\nEND";
        assert_eq!(fold_ranges_sql(src), vec![(0, 4), (1, 3)]);
    }

    #[test]
    fn folds_case_expressions_and_parenthesized_groups() {
        let src = "SELECT CASE\n  WHEN a THEN 1\n  ELSE 2\nEND,\n  (\n    SELECT 1\n  ) FROM t";
        assert_eq!(fold_ranges_sql(src), vec![(0, 3), (4, 6)]);
    }

    #[test]
    fn folds_loop_while_repeat_and_multiline_comments() {
        let src = "LOOP\n  WHILE x DO\n    SET y = 1;\n  END WHILE;\nEND LOOP;\n/* a\nb */\nREPEAT\n  SET z = 1;\nUNTIL done END REPEAT";
        assert_eq!(
            fold_ranges_sql(src),
            vec![(0, 4), (1, 3), (5, 6), (7, 9)]
        );
    }

    #[test]
    fn ignores_keywords_in_strings_comments_and_plain_statements() {
        // 字符串 / 注释里的 BEGIN…END、事务型 BEGIN;、IF() 函数、IF EXISTS、
        // REPEAT() 函数都不该产生折叠
        let src = "BEGIN;\nSELECT 'BEGIN\nEND', \"END\", REPEAT('x', 3), IF(a, 1, 2) FROM t;\n\
                   DROP TABLE IF EXISTS x;\n-- BEGIN\nEND";
        assert!(fold_ranges_sql(src).is_empty());
    }

    #[test]
    fn unclosed_blocks_fold_to_the_last_line() {
        let src = "BEGIN\n  SELECT 1";
        assert_eq!(fold_ranges_sql(src), vec![(0, 1)]);
    }

    #[test]
    fn highlighter_exports_fold_ranges_and_skips_filter_mode() {
        let rope: Rope = "BEGIN\n  SELECT 1\nEND".into();
        let folds = SqlHighlighter::new().fold_ranges(&rope);
        assert_eq!(folds.len(), 1);
        assert_eq!((folds[0].start_line, folds[0].end_line), (0, 2));
        assert!(SqlHighlighter::filter().fold_ranges(&rope).is_empty());
    }

    struct NoResolver;
    impl HighlightStyleResolver for NoResolver {
        fn style(&self, _: &str) -> Option<HighlightStyle> {
            None
        }
    }
}
