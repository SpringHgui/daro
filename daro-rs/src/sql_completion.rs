//! SQL 补全（迁移自 Flutter 版 `lib/data/sql_completions.dart`，再按 gpui 编辑器能力加强）。
//!
//! 词表仍然是五类：关键字 / 函数 / 表 / 列；但匹配与排序不再只看「整篇文档里
//! 光标前的一个词」：
//!
//! - **限定名**：`u.` / `users.` 只提示该表的列，插入时只替换点号后面的部分，
//!   不会把 `u.` 一起吞掉（编辑器按 `text_edit` 的 range 替换）；
//! - **别名**：`FROM users u` 后，`u.` 解析到 `users`；`FROM a JOIN b` 后不带
//!   限定的列只从这两张表里取；
//! - **子句感知**：`FROM` / `JOIN` 后表名优先，`SELECT` / `WHERE` / `ON` 后列优先，
//!   语句开头关键字优先；
//! - **模糊匹配**：前缀 > 包含 > 子序列（`usn` 能命中 `user_name`），同档按词长
//!   排序，弹层里先出现更贴切的候选。
//!
//! 仍不做的事：函数插入不带括号占位、不做类型推导、不解析 CTE 的列（CTE 名会被
//! 当成普通表名，取不到列时回落到全库列）。

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{CompletionProvider, Rope};
use gpui_kit::{App, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    Position, TextEdit,
};

/// SQL 关键字（与 Flutter `kSqlKeywords` 同一份）
pub const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "NULL", "IS", "IN", "LIKE", "AS", "ON",
    "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "CROSS", "FULL", "GROUP", "BY", "ORDER", "HAVING",
    "LIMIT", "OFFSET", "UNION", "ALL", "DISTINCT", "INSERT", "INTO", "VALUES", "UPDATE", "SET",
    "DELETE", "CREATE", "TABLE", "DROP", "ALTER", "ADD", "COLUMN", "INDEX", "VIEW", "WITH", "CASE",
    "WHEN", "THEN", "ELSE", "END", "EXISTS", "BETWEEN", "ASC", "DESC", "PRIMARY", "KEY", "FOREIGN",
    "REFERENCES", "DEFAULT", "UNIQUE", "RETURNING", "EXPLAIN", "TRUNCATE", "DATABASE", "SCHEMA",
    "GRANT", "REVOKE", "TRIGGER", "PROCEDURE", "FUNCTION", "CASCADE", "CONSTRAINT", "CHECK",
    "AUTO_INCREMENT", "IF", "USING", "CAST", "TRUE", "FALSE",
];

/// 常用函数（词 + 分类说明，与 Flutter `kSqlFunctionPrompts` 对齐）
pub const FUNCTIONS: &[(&str, &str)] = &[
    // 聚合
    ("COUNT", "聚合"),
    ("SUM", "聚合"),
    ("AVG", "聚合"),
    ("MIN", "聚合"),
    ("MAX", "聚合"),
    ("GROUP_CONCAT", "聚合"),
    ("STRING_AGG", "聚合"),
    // 字符串
    ("CONCAT", "字符串"),
    ("CONCAT_WS", "字符串"),
    ("SUBSTRING", "字符串"),
    ("SUBSTR", "字符串"),
    ("LENGTH", "字符串"),
    ("CHAR_LENGTH", "字符串"),
    ("UPPER", "字符串"),
    ("LOWER", "字符串"),
    ("TRIM", "字符串"),
    ("REPLACE", "字符串"),
    ("LEFT", "字符串"),
    ("RIGHT", "字符串"),
    ("INSTR", "字符串"),
    ("LOCATE", "字符串"),
    // 日期时间
    ("NOW", "日期"),
    ("CURDATE", "日期"),
    ("CURTIME", "日期"),
    ("CURRENT_DATE", "日期"),
    ("CURRENT_TIMESTAMP", "日期"),
    ("DATE_FORMAT", "日期"),
    ("DATEDIFF", "日期"),
    ("DATE_ADD", "日期"),
    ("DATE_SUB", "日期"),
    ("EXTRACT", "日期"),
    // 数值
    ("ABS", "数值"),
    ("ROUND", "数值"),
    ("FLOOR", "数值"),
    ("CEIL", "数值"),
    ("MOD", "数值"),
    ("RAND", "数值"),
    // 控制流 / 转换
    ("COALESCE", "控制流"),
    ("NULLIF", "控制流"),
    ("IFNULL", "控制流"),
    ("IF", "控制流"),
    ("CAST", "转换"),
    ("CONVERT", "转换"),
    // 窗口函数
    ("ROW_NUMBER", "窗口"),
    ("RANK", "窗口"),
    ("DENSE_RANK", "窗口"),
    ("LAG", "窗口"),
    ("LEAD", "窗口"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Keyword,
    Function,
    Table,
    Column,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub word: String,
    pub kind: PromptKind,
    pub detail: String,
}

impl Prompt {
    fn new(word: impl Into<String>, kind: PromptKind, detail: impl Into<String>) -> Self {
        Self {
            word: word.into(),
            kind,
            detail: detail.into(),
        }
    }
}

/// 补全用的库结构快照：表名 + 每张表的列。
///
/// 真库在开页签时后台扫一次（`Target::columns_by_table`），演示连接 / 还没取到
/// 结构时用 `fallback_columns` 兜底，至少保持旧版「全库列名」体验。
#[derive(Clone, Debug, Default)]
pub struct SqlSchema {
    tables: Vec<String>,
    /// 小写表名 → 列名（保留原始大小写）
    columns: HashMap<String, Vec<String>>,
    fallback_columns: Vec<String>,
}

impl SqlSchema {
    pub fn empty() -> Self {
        Self::default()
    }

    /// 只有表名、还没有列结构时用（开页签的即时状态 / 演示连接）。
    pub fn tables_only(tables: Vec<String>, fallback_columns: Vec<String>) -> Self {
        Self {
            tables,
            columns: HashMap::new(),
            fallback_columns,
        }
    }

    /// 后台取回「表 → 列」后构造；`tables` 里的表即使没扫到列也会保留。
    pub fn from_pairs(tables: Vec<String>, pairs: Vec<(String, Vec<String>)>) -> Self {
        let mut tables = tables;
        let mut columns: HashMap<String, Vec<String>> = HashMap::new();
        for (table, cols) in pairs {
            if !tables.iter().any(|t| t.eq_ignore_ascii_case(&table)) {
                tables.push(table.clone());
            }
            columns.entry(table.to_lowercase()).or_default().extend(cols);
        }
        tables.sort_by_key(|t| t.to_lowercase());
        tables.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        Self {
            tables,
            columns,
            fallback_columns: Vec::new(),
        }
    }

    pub fn table_columns(&self, table: &str) -> Option<&[String]> {
        self.columns.get(&table.to_lowercase()).map(Vec::as_slice)
    }

    /// 按表归组的全库列（refs 为空或解析不到列时的回落）。
    fn all_columns(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for table in &self.tables {
            if let Some(cols) = self.table_columns(table) {
                for c in cols {
                    out.push((table.clone(), c.clone()));
                }
            }
        }
        for c in &self.fallback_columns {
            out.push((String::new(), c.clone()));
        }
        out
    }
}

/// 语句里解析出的一处表引用：`users AS u` → qualifier=u / table=users。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRef {
    pub qualifier: String,
    pub table: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Word(String),
    Dot,
    Comma,
    Open,
    Close,
}

/// 解析 SELECT / FROM / JOIN 子句里出现的表与别名。
///
/// 只做补全需要的那一层：字符串、注释、引号标识符都跳过或归一；子查询整体跳过
/// （CTE / 派生表的列无法从这里知道，取不到时由上层的全库列兜底）。
pub fn table_refs(stmt: &str) -> Vec<TableRef> {
    let toks = tokenize(stmt);
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let Some(Tok::Word(head)) = toks.get(i) else {
            i += 1;
            continue;
        };
        if !is_table_head(head) {
            i += 1;
            continue;
        }
        let comma_list = head.eq_ignore_ascii_case("FROM");
        i += 1;
        loop {
            if matches!(toks.get(i), Some(Tok::Open)) {
                i = skip_balanced(&toks, i);
                // 派生表别名：吃掉，但拿不到它的列
                if let Some(Tok::Word(w)) = toks.get(i)
                    && !is_alias_stop(w)
                {
                    i += 1;
                }
            } else {
                let Some(table) = read_name(&toks, &mut i) else {
                    break;
                };
                if is_alias_stop(&table) {
                    break;
                }
                if matches!(toks.get(i), Some(Tok::Word(w)) if w.eq_ignore_ascii_case("AS")) {
                    i += 1;
                }
                let mut alias = None;
                if let Some(Tok::Word(w)) = toks.get(i)
                    && !is_alias_stop(w)
                {
                    alias = Some(w.clone());
                    i += 1;
                }
                out.push(TableRef {
                    qualifier: alias.unwrap_or_else(|| table.clone()),
                    table,
                });
            }
            if comma_list && matches!(toks.get(i), Some(Tok::Comma)) {
                i += 1;
                continue;
            }
            break;
        }
    }
    out
}

fn is_table_head(word: &str) -> bool {
    ["FROM", "JOIN", "UPDATE", "INTO"]
        .iter()
        .any(|h| word.eq_ignore_ascii_case(h))
}

/// 别名后面不该再吃词的边界（子句关键字 / 连接词）。
fn is_alias_stop(word: &str) -> bool {
    const STOP: &[&str] = &[
        "WHERE", "GROUP", "ORDER", "HAVING", "LIMIT", "OFFSET", "UNION", "JOIN", "LEFT", "RIGHT",
        "INNER", "OUTER", "CROSS", "FULL", "NATURAL", "ON", "USING", "SET", "VALUES", "RETURNING",
        "AS", "SELECT", "WITH", "WINDOW", "FOR", "LOCK", "INTO", "FROM",
    ];
    STOP.iter().any(|s| word.eq_ignore_ascii_case(s))
}

fn skip_balanced(toks: &[Tok], mut i: usize) -> usize {
    let mut depth = 0usize;
    while i < toks.len() {
        match &toks[i] {
            Tok::Open => depth += 1,
            Tok::Close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// 读一个可能带 `schema.` 前缀的名字，返回最后一段（补全按表名本身匹配）。
fn read_name(toks: &[Tok], i: &mut usize) -> Option<String> {
    let Tok::Word(mut name) = toks.get(*i)?.clone() else {
        return None;
    };
    *i += 1;
    while matches!(toks.get(*i), Some(Tok::Dot)) {
        *i += 1;
        if let Tok::Word(next) = toks.get(*i)? {
            name = next.clone();
            *i += 1;
        } else {
            break;
        }
    }
    Some(name)
}

fn tokenize(src: &str) -> Vec<Tok> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    let n = bytes.len();
    while i < n {
        let c = src[i..].chars().next().unwrap();
        let len = c.len_utf8();
        if c.is_whitespace() {
            i += len;
            continue;
        }
        // 行注释
        if (c == '-' && bytes.get(i + 1) == Some(&b'-')) || c == '#' {
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // 块注释（可嵌套，与语法高亮同口径）
        if c == '/' && bytes.get(i + 1) == Some(&b'*') {
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
            continue;
        }
        // 字符串字面量
        if c == '\'' {
            i += len;
            while i < n {
                let ch = src[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == '\'' {
                    if i < n && src[i..].starts_with('\'') {
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        // 引号标识符：内容仍当普通词（`"user"` / `` `user` `` / `[user]`）
        if matches!(c, '"' | '`' | '[') {
            let close = if c == '[' { ']' } else { c };
            let mut value = String::new();
            i += len;
            while i < n {
                let ch = src[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == close {
                    if close != ']' && i < n && src[i..].starts_with(close) {
                        value.push(close);
                        i += close.len_utf8();
                        continue;
                    }
                    break;
                }
                value.push(ch);
            }
            if !value.is_empty() {
                out.push(Tok::Word(value));
            }
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < n {
                let ch = src[i..].chars().next().unwrap();
                if ch.is_alphanumeric() || ch == '_' || ch == '$' {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            out.push(Tok::Word(src[start..i].to_string()));
            continue;
        }
        match c {
            '.' => out.push(Tok::Dot),
            ',' => out.push(Tok::Comma),
            '(' => out.push(Tok::Open),
            ')' => out.push(Tok::Close),
            _ => {}
        }
        i += len;
    }
    out
}

/// 光标前最近的子句关键字（用于决定「先列表还是先列列」）。
fn last_clause(stmt: &str) -> Option<String> {
    const CLAUSES: &[&str] = &[
        "SELECT", "FROM", "WHERE", "JOIN", "ON", "GROUP", "ORDER", "BY", "HAVING", "LIMIT",
        "OFFSET", "UNION", "SET", "VALUES", "UPDATE", "INTO", "USING", "RETURNING", "INSERT",
        "DELETE", "AND", "OR", "WHEN", "THEN", "ELSE", "AS", "WITH", "CASE", "DISTINCT", "ALL",
        "LEFT", "RIGHT", "INNER", "OUTER", "CROSS", "FULL", "NATURAL", "EXISTS", "NOT", "LIKE",
        "BETWEEN",
    ];
    for tok in tokenize(stmt).iter().rev() {
        if let Tok::Word(w) = tok {
            let upper = w.to_ascii_uppercase();
            if CLAUSES.contains(&upper.as_str()) {
                return Some(upper);
            }
        }
    }
    None
}

/// 取出光标前的词前缀（字节偏移口径，与编辑器一致；`.` 属于前缀）。
pub fn word_prefix(text: &str, offset: usize) -> &str {
    let end = offset.min(text.len());
    let bytes = text.as_bytes();
    let mut start = end;
    while start > 0 {
        let b = bytes[start - 1];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'.' {
            start -= 1;
        } else {
            break;
        }
    }
    // start/end 落在 ASCII 边界上，切片安全
    &text[start..end]
}

/// 把前缀拆成「限定符 + 正在输入的词」：`u.na` → (`u`, `na`)。
pub fn split_prefix(prefix: &str) -> (&str, &str) {
    match prefix.rfind('.') {
        Some(i) => (&prefix[..i], &prefix[i + 1..]),
        None => ("", prefix),
    }
}

/// 匹配档位：0 前缀、1 包含、2 子序列；`None` 表示不候选。
/// 输入已经等于整词时不再提示（沿用 Flutter 版 `_matchWord` 的手感）。
pub fn match_score(word: &str, input: &str) -> Option<u8> {
    if input.is_empty() {
        return Some(0);
    }
    let w = word.to_lowercase();
    let i = input.to_lowercase();
    if w == i {
        return None;
    }
    if w.starts_with(&i) {
        return Some(0);
    }
    if w.contains(&i) {
        return Some(1);
    }
    if i.chars().count() > 1 && is_subsequence(&i, &w) {
        return Some(2);
    }
    None
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|n| it.any(|h| h == n))
}

fn sort_scored(items: &mut [(u8, Prompt)]) {
    items.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.word.len().cmp(&b.1.word.len()))
            .then_with(|| a.1.word.cmp(&b.1.word))
    });
}

fn push_unique(
    out: &mut Vec<(u8, Prompt)>,
    seen: &mut HashSet<String>,
    word: &str,
    kind: PromptKind,
    detail: String,
    input: &str,
) {
    let Some(score) = match_score(word, input) else {
        return;
    };
    if seen.insert(word.to_lowercase()) {
        out.push((score, Prompt::new(word, kind, detail)));
    }
}

/// 一次补全的结果：候选 + 本次插入应替换的起始字节（限定名时是点号之后）。
#[derive(Clone, Debug, Default)]
pub struct CompletionSet {
    pub items: Vec<Prompt>,
    pub replace_from: usize,
}

pub fn completions_at(
    schema: &SqlSchema,
    text: &str,
    offset: usize,
    limit: usize,
) -> CompletionSet {
    let offset = floor_boundary(text, offset);
    let prefix = word_prefix(text, offset);
    let (qualifier, partial) = split_prefix(prefix);
    let replace_from = offset - partial.len();

    let stmt = statement_before(text, offset);
    let refs = table_refs(stmt);

    if !qualifier.is_empty()
        && let Some(table) = resolve_qualifier(schema, &refs, qualifier)
    {
        let cols = schema
            .table_columns(table)
            .unwrap_or(schema.fallback_columns.as_slice());
        return CompletionSet {
            items: column_items(cols, table, partial, limit),
            replace_from,
        };
    }

    CompletionSet {
        items: unqualified(schema, &refs, stmt, partial, limit),
        replace_from,
    }
}

fn resolve_qualifier<'a>(
    schema: &'a SqlSchema,
    refs: &'a [TableRef],
    qualifier: &str,
) -> Option<&'a str> {
    if let Some(r) = refs
        .iter()
        .find(|r| r.qualifier.eq_ignore_ascii_case(qualifier))
    {
        return Some(&r.table);
    }
    if let Some(r) = refs.iter().find(|r| r.table.eq_ignore_ascii_case(qualifier)) {
        return Some(&r.table);
    }
    schema
        .tables
        .iter()
        .find(|t| t.eq_ignore_ascii_case(qualifier))
        .map(String::as_str)
}

fn column_items(cols: &[String], table: &str, input: &str, limit: usize) -> Vec<Prompt> {
    let mut scored = Vec::new();
    let mut seen = HashSet::new();
    let detail = if table.is_empty() {
        "列".to_string()
    } else {
        format!("列 · {table}")
    };
    for c in cols {
        push_unique(&mut scored, &mut seen, c, PromptKind::Column, detail.clone(), input);
    }
    sort_scored(&mut scored);
    scored
        .into_iter()
        .take(limit)
        .map(|(_, p)| p)
        .collect()
}

fn unqualified(
    schema: &SqlSchema,
    refs: &[TableRef],
    stmt: &str,
    input: &str,
    limit: usize,
) -> Vec<Prompt> {
    let clause = last_clause(stmt);
    let tables_first = match clause.as_deref() {
        Some("FROM") | Some("JOIN") => true,
        Some("UPDATE") | Some("INTO") => refs.is_empty(),
        _ => false,
    };

    // 上下文列：FROM / JOIN 里出现的表；解析不到列时回落到全库列。
    let mut scoped: Vec<(String, String)> = Vec::new();
    for r in refs {
        if let Some(cols) = schema.table_columns(&r.table) {
            for c in cols {
                scoped.push((r.table.clone(), c.clone()));
            }
        }
    }
    if scoped.is_empty() {
        scoped = schema.all_columns();
    }

    let mut col_scored = Vec::new();
    let mut col_seen = HashSet::new();
    for (table, col) in &scoped {
        let detail = if table.is_empty() {
            "列".to_string()
        } else {
            format!("列 · {table}")
        };
        push_unique(
            &mut col_scored,
            &mut col_seen,
            col,
            PromptKind::Column,
            detail,
            input,
        );
    }
    sort_scored(&mut col_scored);

    let mut tbl_scored = Vec::new();
    let mut tbl_seen = HashSet::new();
    for t in &schema.tables {
        push_unique(
            &mut tbl_scored,
            &mut tbl_seen,
            t,
            PromptKind::Table,
            "表".to_string(),
            input,
        );
    }
    sort_scored(&mut tbl_scored);

    let mut fn_scored = Vec::new();
    let mut fn_seen = HashSet::new();
    for (f, detail) in FUNCTIONS {
        push_unique(
            &mut fn_scored,
            &mut fn_seen,
            f,
            PromptKind::Function,
            format!("函数 · {detail}"),
            input,
        );
    }
    sort_scored(&mut fn_scored);

    let mut kw_scored = Vec::new();
    let mut kw_seen = HashSet::new();
    for k in KEYWORDS {
        push_unique(
            &mut kw_scored,
            &mut kw_seen,
            k,
            PromptKind::Keyword,
            "关键字".to_string(),
            input,
        );
    }
    sort_scored(&mut kw_scored);

    let mut by_kind = [col_scored, tbl_scored, fn_scored, kw_scored];
    let index = |kind: PromptKind| match kind {
        PromptKind::Column => 0,
        PromptKind::Table => 1,
        PromptKind::Function => 2,
        PromptKind::Keyword => 3,
    };
    let order = if tables_first {
        [
            PromptKind::Table,
            PromptKind::Column,
            PromptKind::Function,
            PromptKind::Keyword,
        ]
    } else if refs.is_empty() {
        [
            PromptKind::Keyword,
            PromptKind::Function,
            PromptKind::Table,
            PromptKind::Column,
        ]
    } else {
        [
            PromptKind::Column,
            PromptKind::Function,
            PromptKind::Table,
            PromptKind::Keyword,
        ]
    };

    let mut out: Vec<Prompt> = Vec::with_capacity(limit);
    let mut seen = HashSet::new();
    for kind in order {
        for (_, p) in std::mem::take(&mut by_kind[index(kind)]) {
            if out.len() >= limit {
                return out;
            }
            if seen.insert(p.word.to_lowercase()) {
                out.push(p);
            }
        }
    }
    out
}

fn statement_before(text: &str, offset: usize) -> &str {
    let head = &text[..offset];
    match head.rfind(';') {
        Some(i) => &head[i + 1..],
        None => head,
    }
}

fn floor_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// 字节偏移 → LSP Position（行号 + **字符**列，编辑器按字符数换算）。
fn position_of(text: &str, offset: usize) -> Position {
    let head = &text[..offset];
    let line = head.matches('\n').count();
    let character = head
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .chars()
        .count();
    Position::new(line as u32, character as u32)
}

/// 本次插入文本是否该触发补全：末字符是词字符或 `.`。
///
/// 单独抽成纯函数是因为 `is_completion_trigger` 的参数 `text` 是**本次插入的
/// 文本**而不是整篇文档——早期的实现按「整篇文档 + 绝对偏移」去截，导致打字
/// 永远不触发，实机表现为补全弹层根本不出现。这里用纯函数把这个口径钉住。
pub fn is_trigger_text(text: &str) -> bool {
    text.chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

/// gpui 编辑器的补全提供者：每次 Ctrl+Space / 触发时按上下文生成候选。
pub struct SqlCompletionProvider {
    schema: SqlSchema,
}

impl SqlCompletionProvider {
    pub fn new(schema: SqlSchema) -> Self {
        Self { schema }
    }
}

impl CompletionProvider for SqlCompletionProvider {
    /// 边打字边提示。
    ///
    /// 注意 `text` 是**本次插入的文本**（不是整篇文档），`offset` 才是文档里的
    /// 绝对位置——第一版把 `text` 当整篇文档去截 `offset`，长度不够直接返回
    /// false，于是打字永远不触发。判据只看插入内容的最后一个字符。
    fn is_completion_trigger(&self, _offset: usize, text: &str, _cx: &mut App) -> bool {
        is_trigger_text(text)
    }

    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        let src: String = text.chars().collect();
        let set = completions_at(&self.schema, &src, offset, 50);
        let end = floor_boundary(&src, offset);
        let start = set.replace_from.min(end);
        let range = lsp_types::Range::new(position_of(&src, start), position_of(&src, end));
        let items: Vec<CompletionItem> = set
            .items
            .into_iter()
            .map(|p| CompletionItem {
                label: p.word.clone(),
                kind: Some(item_kind(p.kind)),
                detail: if p.detail.is_empty() {
                    None
                } else {
                    Some(p.detail)
                },
                // 限定名（`u.na`）只替换点号后的 `na`，否则会把 `u.` 一起吞掉。
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: range.clone(),
                    new_text: p.word,
                })),
                ..Default::default()
            })
            .collect();
        Task::ready(Ok(CompletionResponse::Array(items)))
    }
}

fn item_kind(kind: PromptKind) -> CompletionItemKind {
    match kind {
        PromptKind::Keyword => CompletionItemKind::KEYWORD,
        PromptKind::Function => CompletionItemKind::FUNCTION,
        PromptKind::Table => CompletionItemKind::STRUCT,
        PromptKind::Column => CompletionItemKind::FIELD,
    }
}

/// 给查询页签构造一个补全提供者（关键字 + 函数 + 当前库的表与列）。
pub fn provider_for(schema: SqlSchema) -> Rc<dyn CompletionProvider> {
    Rc::new(SqlCompletionProvider::new(schema))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> SqlSchema {
        SqlSchema::from_pairs(
            vec!["users".into(), "orders".into()],
            vec![
                (
                    "users".into(),
                    vec!["id".into(), "user_name".into(), "email".into()],
                ),
                (
                    "orders".into(),
                    vec!["id".into(), "user_id".into(), "amount".into()],
                ),
            ],
        )
    }

    fn words(items: &[Prompt]) -> Vec<&str> {
        items.iter().map(|p| p.word.as_str()).collect()
    }

    #[test]
    fn match_score_prefers_prefix_then_contains_then_subsequence() {
        assert_eq!(match_score("SELECT", "sel"), Some(0));
        assert_eq!(match_score("user_name", "name"), Some(1));
        assert_eq!(match_score("user_name", "usn"), Some(2));
        assert_eq!(match_score("SELECT", "select"), None); // 已输完整词 → 不再提示
        assert_eq!(match_score("SELECT", "xyz"), None);
    }

    #[test]
    fn word_prefix_and_split_keep_qualifier() {
        let sql = "select u.na";
        assert_eq!(word_prefix(sql, sql.len()), "u.na");
        assert_eq!(split_prefix("u.na"), ("u", "na"));
        assert_eq!(split_prefix("na"), ("", "na"));
        assert_eq!(split_prefix("u."), ("u", ""));
    }

    #[test]
    fn table_refs_reads_aliases_lists_and_joins() {
        let refs = table_refs(
            "SELECT * FROM public.users AS u, orders o JOIN items i ON i.oid = o.id",
        );
        assert_eq!(
            refs,
            vec![
                TableRef {
                    qualifier: "u".into(),
                    table: "users".into()
                },
                TableRef {
                    qualifier: "o".into(),
                    table: "orders".into()
                },
                TableRef {
                    qualifier: "i".into(),
                    table: "items".into()
                },
            ]
        );
    }

    #[test]
    fn table_refs_skips_strings_comments_and_subqueries() {
        let refs = table_refs(
            "/* from fake */ SELECT 'from x' FROM (SELECT 1) d JOIN real r ON true -- from y\n",
        );
        assert_eq!(
            refs,
            vec![TableRef {
                qualifier: "r".into(),
                table: "real".into()
            }]
        );
    }

    #[test]
    fn alias_dot_only_offers_that_tables_columns() {
        let sql = "SELECT * FROM users u JOIN orders o ON u.id = o.user_id WHERE u.";
        let set = completions_at(&schema(), sql, sql.len(), 50);
        assert_eq!(set.replace_from, sql.len());
        let got = words(&set.items);
        assert!(got.contains(&"user_name"));
        assert!(!got.contains(&"amount"));
        assert!(!got.contains(&"SELECT"));
    }

    #[test]
    fn qualified_completion_replaces_only_text_after_dot() {
        let sql = "select u.na from users u";
        let offset = "select u.na".len();
        let set = completions_at(&schema(), sql, offset, 50);
        assert_eq!(set.replace_from, offset - "na".len());
        assert!(words(&set.items).contains(&"user_name"));
    }

    #[test]
    fn from_context_puts_tables_first_and_limits_columns_to_scope() {
        let sql = "SELECT * FROM users u WHERE ";
        let set = completions_at(&schema(), sql, sql.len(), 50);
        let got = words(&set.items);
        // WHERE 后有 FROM 上下文：列优先，且只来自 users
        assert!(got.iter().any(|w| *w == "user_name"));
        assert!(!got.iter().any(|w| *w == "amount"));
        // FROM 刚打出时表名优先（同档按词长：users 比 orders 短）
        let sql = "SELECT * FROM ";
        let set = completions_at(&schema(), sql, sql.len(), 50);
        assert_eq!(words(&set.items).first().copied(), Some("users"));
    }

    #[test]
    fn empty_statement_offers_keywords_first() {
        let sql = "sel";
        let set = completions_at(&schema(), sql, sql.len(), 50);
        assert_eq!(words(&set.items).first().copied(), Some("SELECT"));
    }

    #[test]
    fn unqualified_columns_dedupe_but_keep_scope_detail() {
        let sql = "SELECT * FROM users u WHERE i";
        let set = completions_at(&schema(), sql, sql.len(), 50);
        let ids: Vec<&Prompt> = set.items.iter().filter(|p| p.word == "id").collect();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].detail, "列 · users");
    }

    #[test]
    fn trigger_only_looks_at_the_inserted_text() {
        assert!(is_trigger_text("s"));
        assert!(is_trigger_text("中"));
        assert!(is_trigger_text("."));
        assert!(!is_trigger_text(" "));
        assert!(!is_trigger_text("("));
        assert!(!is_trigger_text(""));
    }
}
