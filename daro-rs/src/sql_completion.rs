//! SQL 补全（迁移自 Flutter 版 `lib/data/sql_completions.dart`）。
//!
//! Flutter 版把补全词分成五类：关键字 / 函数 / 表 / 视图 / 列，匹配规则是
//! **大小写不敏感的前缀匹配**，且「输入等于整个词」时不再提示
//! （`_matchWord`）。插入内容始终是词本身，函数不带括号占位。
//!
//! 这里先迁「词表 + 匹配 + 取词前缀」三段纯逻辑（可测），再实现 gpui 编辑器的
//! [`CompletionProvider`]，交给编辑器自带的补全弹层渲染。
//!
//! 尚未迁移：Flutter 的「表别名解析」（`FROM t AS a` 后按别名限定的列提示）。

use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{CompletionProvider, Rope};
use gpui_kit::{App, Task, Window};
use lsp_types::{CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse};

/// SQL 关键字（与 Flutter `kSqlKeywords` 同一份）
pub const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "NULL", "IS", "IN", "LIKE", "AS", "ON",
    "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "CROSS", "FULL", "GROUP", "BY", "ORDER",
    "HAVING", "LIMIT", "OFFSET", "UNION", "ALL", "DISTINCT", "INSERT", "INTO", "VALUES",
    "UPDATE", "SET", "DELETE", "CREATE", "TABLE", "DROP", "ALTER", "ADD", "COLUMN", "INDEX",
    "VIEW", "WITH", "CASE", "WHEN", "THEN", "ELSE", "END", "EXISTS", "BETWEEN", "ASC", "DESC",
    "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "DEFAULT", "UNIQUE", "RETURNING", "EXPLAIN",
    "TRUNCATE", "DATABASE", "SCHEMA", "GRANT", "REVOKE", "TRIGGER", "PROCEDURE", "FUNCTION",
    "CASCADE", "CONSTRAINT", "CHECK", "AUTO_INCREMENT", "IF", "USING", "CAST", "TRUE", "FALSE",
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

/// 全量候选：关键字 → 函数 → 表 → 列（顺序影响弹层里的先后）
pub fn all_prompts(tables: &[String], columns: &[String]) -> Vec<Prompt> {
    let mut out: Vec<Prompt> = Vec::with_capacity(
        KEYWORDS.len() + FUNCTIONS.len() + tables.len() + columns.len(),
    );
    for k in KEYWORDS {
        out.push(Prompt::new(*k, PromptKind::Keyword, "关键字"));
    }
    for (f, detail) in FUNCTIONS {
        out.push(Prompt::new(*f, PromptKind::Function, *detail));
    }
    for t in tables {
        let first = if t.is_empty() { "" } else { "表" };
        out.push(Prompt::new(t.clone(), PromptKind::Table, first));
    }
    for c in columns {
        out.push(Prompt::new(c.clone(), PromptKind::Column, "列"));
    }
    out
}

/// Flutter `_matchWord` 口径：空输入全匹配；大小写不敏感前缀匹配；
/// 输入已等于整词时不再提示。
pub fn match_word(word: &str, input: &str) -> bool {
    if input.is_empty() {
        return true;
    }
    let w = word.to_lowercase();
    let i = input.to_lowercase();
    w != i && w.starts_with(&i)
}

/// 取出光标前的词前缀（字节偏移口径，与编辑器一致）。
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

/// 过滤候选（保持原顺序），`limit` 限制弹层条目数。
pub fn filtered(prompts: &[Prompt], input: &str, limit: usize) -> Vec<Prompt> {
    prompts
        .iter()
        .filter(|p| match_word(&p.word, input))
        .take(limit)
        .cloned()
        .collect()
}

fn item_kind(kind: PromptKind) -> CompletionItemKind {
    match kind {
        PromptKind::Keyword => CompletionItemKind::KEYWORD,
        PromptKind::Function => CompletionItemKind::FUNCTION,
        PromptKind::Table => CompletionItemKind::STRUCT,
        PromptKind::Column => CompletionItemKind::FIELD,
    }
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

/// gpui 编辑器的补全提供者：每次 Ctrl+Space / 触发时按当前词前缀过滤。
pub struct SqlCompletionProvider {
    prompts: Vec<Prompt>,
}

impl SqlCompletionProvider {
    pub fn new(tables: &[String], columns: &[String]) -> Self {
        Self {
            prompts: all_prompts(tables, columns),
        }
    }

    pub fn with_prompts(prompts: Vec<Prompt>) -> Self {
        Self { prompts }
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
        let prefix = word_prefix(&src, offset).to_string();
        let items: Vec<CompletionItem> = filtered(&self.prompts, &prefix, 50)
            .into_iter()
            .map(|p| CompletionItem {
                label: p.word,
                kind: Some(item_kind(p.kind)),
                detail: if p.detail.is_empty() { None } else { Some(p.detail) },
                ..Default::default()
            })
            .collect();
        Task::ready(Ok(CompletionResponse::Array(items)))
    }
}

/// 给查询页签构造一个补全提供者（关键字 + 函数 + 当前库的表与列）。
pub fn provider_for(tables: &[String], columns: &[String]) -> Rc<dyn CompletionProvider> {
    Rc::new(SqlCompletionProvider::new(tables, columns))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_word_is_case_insensitive_prefix_and_excludes_exact() {
        assert!(match_word("SELECT", "sel"));
        assert!(!match_word("SELECT", "select")); // 输入等于整词 → 不提示
        assert!(match_word("SELECT", "SELEC")); // 仍是前缀 → 提示
        assert!(match_word("FROM", ""));
        assert!(!match_word("COUNT", "count"));
    }

    #[test]
    fn word_prefix_reads_back_to_non_word_char() {
        let sql = "SELECT id, name FROM t";
        assert_eq!(word_prefix(sql, 9), "id");
        let sql2 = "select cou";
        assert_eq!(word_prefix(sql2, sql2.len()), "cou");
        let sql3 = "select a.b";
        assert_eq!(word_prefix(sql3, sql3.len()), "a.b");
    }

    #[test]
    fn filtering_keeps_order_and_limit() {
        let prompts = vec![
            Prompt::new("SELECT", PromptKind::Keyword, "关键字"),
            Prompt::new("SET", PromptKind::Keyword, "关键字"),
            Prompt::new("SUM", PromptKind::Function, "聚合"),
            Prompt::new("SUBSTRING", PromptKind::Function, "字符串"),
        ];
        let got: Vec<String> = filtered(&prompts, "s", 3)
            .into_iter()
            .map(|p| p.word)
            .collect();
        assert_eq!(got, vec!["SELECT", "SET", "SUM"]);
        // 输入等于整词 → 不再提示
        assert!(filtered(&prompts, "set", 10).is_empty());
    }

    #[test]
    fn trigger_only_looks_at_the_inserted_text() {
        // 插入单个词字符 / 点号 → 触发；插入空格、括号、空串 → 不触发。
        assert!(is_trigger_text("s"));
        assert!(is_trigger_text("中"));
        assert!(is_trigger_text("."));
        assert!(!is_trigger_text(" "));
        assert!(!is_trigger_text("("));
        assert!(!is_trigger_text(""));
    }

    #[test]
    fn all_prompts_include_tables_and_columns_with_details() {
        let prompts = all_prompts(&["bx_cart".to_string()], &["id".to_string()]);
        assert!(prompts.iter().any(|p| p.word == "bx_cart" && p.kind == PromptKind::Table));
        assert!(prompts.iter().any(|p| p.word == "id" && p.kind == PromptKind::Column));
        assert!(prompts.iter().any(|p| p.word == "COUNT" && p.detail == "聚合"));
    }
}
