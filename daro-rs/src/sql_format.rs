//! SQL 美化（查询页「美化 SQL」）。
//!
//! 词法感知的轻量格式化，不做解析级重排：字符串 / 注释 / 方括号标识符
//! 原样保留，关键字大写，顶层子句各自换行，AND / OR 缩进续行，顶层逗号
//! 后换行，触发器体 BEGIN … END 整体缩进。除了空白与关键字大小写，其余
//! 一律不动——美化过的语句语义必须与原文一致。
//!
//! 语句拆分复用 `crate::split_sql_statements`（已带触发器体感知）。

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Word(String),
    Sym(char),
    /// 原子：字符串 / 引号标识符 / 数字 / 美元引号 / 注释，原样输出
    Atom(String),
}

fn lex(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        if s[i..].starts_with("--") {
            let end = s[i..].find('\n').map(|p| i + p).unwrap_or(s.len());
            out.push(Tok::Atom(s[i..end].to_string()));
            i = end;
            continue;
        }
        if s[i..].starts_with("/*") {
            // PG 风格嵌套块注释
            let mut depth = 0usize;
            let mut j = i;
            while j < s.len() {
                if s[j..].starts_with("/*") {
                    depth += 1;
                    j += 2;
                } else if s[j..].starts_with("*/") {
                    depth = depth.saturating_sub(1);
                    j += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    j += s[j..].chars().next().unwrap().len_utf8();
                }
            }
            out.push(Tok::Atom(s[i..j].to_string()));
            i = j;
            continue;
        }
        if c == '$'
            && let Some(tag) = dollar_tag(&s[i..])
        {
            let rest = &s[i + tag.len()..];
            let end = rest
                .find(&tag)
                .map(|p| i + tag.len() + p + tag.len())
                .unwrap_or(s.len());
            out.push(Tok::Atom(s[i..end].to_string()));
            i = end;
            continue;
        }
        if matches!(c, '\'' | '"' | '`') {
            let mut j = i + c.len_utf8();
            while j < s.len() {
                let ch = s[j..].chars().next().unwrap();
                j += ch.len_utf8();
                if ch == c {
                    // 双写转义不算闭合
                    if s[j..].starts_with(c) {
                        j += c.len_utf8();
                    } else {
                        break;
                    }
                }
            }
            let j = j.min(s.len());
            out.push(Tok::Atom(s[i..j].to_string()));
            i = j;
            continue;
        }
        if c == '[' {
            let end = s[i..].find(']').map(|p| i + p + 1).unwrap_or(s.len());
            out.push(Tok::Atom(s[i..end].to_string()));
            i = end;
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < s.len() {
                let ch = s[i..].chars().next().unwrap();
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '@' || ch == '#' {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            out.push(Tok::Word(s[start..i].to_string()));
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < s.len() {
                let ch = s[i..].chars().next().unwrap();
                if ch.is_ascii_alphanumeric() || ch == '.' {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            out.push(Tok::Atom(s[start..i].to_string()));
            continue;
        }
        out.push(Tok::Sym(c));
        i += c.len_utf8();
    }
    out
}

/// `$$` / `$tag$` 定界符（含首尾 $）；不构成定界符（如 `$1`）返回 None。
fn dollar_tag(s: &str) -> Option<String> {
    let mut tag = String::from("$");
    for c in s.chars().skip(1) {
        if c == '$' {
            tag.push('$');
            return Some(tag);
        }
        if c.is_ascii_alphabetic() || c == '_' {
            tag.push(c);
        } else if c.is_ascii_digit() && tag.len() > 1 {
            tag.push(c);
        } else {
            return None;
        }
    }
    None
}

/// 需要大写的词表（其余单词按原文输出，避免改坏标识符大小写口径）。
const KEYWORDS: &[&str] = &[
    "select", "distinct", "from", "where", "group", "by", "having", "order", "limit", "offset",
    "union", "all", "except", "intersect", "insert", "into", "values", "update", "set", "delete",
    "and", "or", "not", "as", "on", "in", "is", "null", "true", "false", "like", "glob", "match",
    "regexp", "between", "exists", "case", "when", "then", "else", "end", "join", "inner",
    "left", "right", "full", "cross", "natural", "using", "asc", "desc", "create", "drop",
    "alter", "table", "view", "index", "trigger", "before", "after", "instead", "of", "procedure",
    "function", "begin", "commit", "rollback", "transaction", "primary", "key", "foreign",
    "references", "unique", "check", "default", "constraint", "with", "recursive", "returning",
    "conflict", "do", "nothing", "replace", "temporary", "temp", "if", "cascade", "restrict",
    "attach", "detach", "vacuum", "analyze", "reindex", "pragma", "explain", "query", "plan",
    "cast", "collate", "escape", "over", "partition", "filter", "within", "rows", "range",
    "preceding", "following", "current", "row", "grant", "revoke", "rename", "add", "column",
    "modify", "change", "truncate",
];

fn upper_keyword(w: &str) -> String {
    if is_keyword(w) {
        w.to_ascii_uppercase()
    } else {
        w.to_string()
    }
}

fn is_keyword(w: &str) -> bool {
    KEYWORDS.iter().any(|k| w.eq_ignore_ascii_case(k))
}

/// 换行的子句关键字（`BY` 跟着 `GROUP` / `ORDER` 走同一行，见 `sticky`）。
fn is_clause(up: &str) -> bool {
    matches!(
        up,
        "SELECT"
            | "FROM"
            | "WHERE"
            | "GROUP"
            | "HAVING"
            | "ORDER"
            | "LIMIT"
            | "OFFSET"
            | "UNION"
            | "EXCEPT"
            | "INTERSECT"
            | "VALUES"
            | "SET"
            | "JOIN"
            | "INNER"
            | "LEFT"
            | "RIGHT"
            | "FULL"
            | "CROSS"
    )
}

/// 固定搭配不换行（`DELETE FROM`、`UNION ALL`、`GROUP BY` 之类）。
fn sticky(prev: &str, cur: &str) -> bool {
    matches!(
        (prev, cur),
        ("DELETE", "FROM")
            | ("INSERT", "INTO")
            | ("INSERT", "OVERWRITE")
            | ("UNION", "ALL")
            | ("EXCEPT", "ALL")
            | ("INTERSECT", "ALL")
            | ("GROUP", "BY")
            | ("ORDER", "BY")
            | ("PARTITION", "BY")
            | ("CREATE", "TEMP")
            | ("CREATE", "TEMPORARY")
            | ("IS", "NOT")
            | ("NOT", "NULL")
            | ("DISTINCT", "ON")
            | ("ON", "CONFLICT")
            | ("INSTEAD", "OF")
    )
}

/// 行首缩进（两级空格一档）。
fn indent(n: usize) -> String {
    " ".repeat(n * 2)
}

/// 是否需要先补一个空格：行内已有正文，且不以 `(` / `.` 这类「贴左」符号收尾。
fn needs_space(cur: &str) -> bool {
    !cur.trim().is_empty() && !cur.ends_with(' ') && !cur.ends_with('(') && !cur.ends_with('.')
}

fn push_text(cur: &mut String, text: &str) {
    if needs_space(cur) {
        cur.push(' ');
    }
    cur.push_str(text);
}

fn flush(lines: &mut Vec<String>, cur: &mut String) {
    let t = std::mem::take(cur);
    if !t.trim().is_empty() {
        lines.push(t);
    }
}

/// 美化一条语句（不含结尾分号以外的多语句拆分）。
pub fn pretty_statement(stmt: &str) -> String {
    let toks = lex(stmt);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    let mut base_indent = 0usize;
    let mut prev_word = String::new();
    // 触发器体：BEGIN 之后整体缩进一级，END 收回
    let mut saw_trigger = false;
    let mut in_body = false;
    for tok in toks {
        match tok {
            Tok::Sym(';') => {
                // 只有触发器体内才会走到这（外层已由 splitter 拆过）
                cur.push(';');
                flush(&mut lines, &mut cur);
                cur = indent(base_indent);
            }
            Tok::Sym('(') => {
                // 函数调用紧贴（count(…），关键字之后留空格（IN (…）
                if cur.trim().is_empty() {
                    cur = indent(base_indent);
                    cur.push('(');
                } else if is_keyword(&prev_word) {
                    cur.push_str(" (");
                } else {
                    cur.push('(');
                }
                depth += 1;
            }
            Tok::Sym(')') => {
                depth = depth.saturating_sub(1);
                cur.push(')');
            }
            Tok::Sym(',') => {
                cur.push(',');
                if depth == 0 {
                    flush(&mut lines, &mut cur);
                    cur = indent(base_indent + 1);
                } else {
                    cur.push(' ');
                }
            }
            Tok::Sym('.') => {
                while cur.ends_with(' ') {
                    cur.pop();
                }
                cur.push('.');
            }
            Tok::Sym(c) => push_text(&mut cur, &c.to_string()),
            Tok::Atom(a) => push_text(&mut cur, &a),
            Tok::Word(w) => {
                let up = w.to_ascii_uppercase();
                let brk = depth == 0
                    && (is_clause(&up) || up == "AND" || up == "OR")
                    && !prev_word.is_empty()
                    && !sticky(&prev_word, &up);
                if depth == 0 && up == "BEGIN" && saw_trigger {
                    flush(&mut lines, &mut cur);
                    lines.push(format!("{}BEGIN", indent(base_indent)));
                    base_indent += 1;
                    in_body = true;
                    cur = indent(base_indent);
                } else if depth == 0 && up == "END" && in_body {
                    flush(&mut lines, &mut cur);
                    base_indent = base_indent.saturating_sub(1);
                    if base_indent == 0 {
                        in_body = false;
                    }
                    cur = format!("{}END", indent(base_indent));
                    saw_trigger = false;
                } else if brk {
                    flush(&mut lines, &mut cur);
                    let extra = usize::from(up == "AND" || up == "OR");
                    cur = format!("{}{}", indent(base_indent + extra), upper_keyword(&w));
                } else {
                    if up == "TRIGGER" {
                        saw_trigger = true;
                    }
                    push_text(&mut cur, &upper_keyword(&w));
                }
                prev_word = up;
            }
        }
    }
    flush(&mut lines, &mut cur);
    lines
        .iter()
        .map(|l| l.trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 美化整段脚本：逐条语句格式化，语句之间空一行。
pub fn pretty(script: &str) -> String {
    crate::split_sql_statements(script)
        .iter()
        .map(|s| pretty_statement(s))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atoms_survive_untouched() {
        // 字符串 / 注释里的关键字不大写、也不当成子句
        let out = pretty("select 'from where' as a, -- note: select\n  b from t where b = 'x'");
        assert!(out.contains("'from where'"), "{out}");
        assert!(out.contains("-- note: select"), "{out}");
        assert!(out.contains("'x'"), "{out}");
        assert_eq!(out.lines().count(), 4, "{out}");
    }

    #[test]
    fn clauses_split_and_top_level_commas_wrap() {
        let out = pretty("select id, name from member where score > 80 order by id desc");
        assert_eq!(
            out,
            "SELECT id,\n  name\nFROM member\nWHERE score > 80\nORDER BY id DESC"
        );
    }

    #[test]
    fn delete_from_stays_on_one_line() {
        let out = pretty("delete from member where id = 1");
        assert_eq!(out, "DELETE FROM member\nWHERE id = 1");
    }

    #[test]
    fn and_or_indents() {
        let out = pretty("select 1 from t where a = 1 and b = 2 or c = 3");
        assert_eq!(
            out,
            "SELECT 1\nFROM t\nWHERE a = 1\n  AND b = 2\n  OR c = 3"
        );
    }

    #[test]
    fn nested_parens_do_not_split() {
        let out = pretty("select count(a, b), max(x) from t where id in (1,2,3)");
        assert!(out.contains("count(a, b)"), "{out}");
        assert!(out.contains("IN (1, 2, 3)"), "{out}");
        assert!(!out.contains("count ("), "{out}");
    }

    #[test]
    fn qualified_names_stay_glued() {
        let out = pretty("select m.name from main.member as m");
        assert!(out.contains("main.member"), "{out}");
        assert!(out.contains("m.name"), "{out}");
    }

    #[test]
    fn trigger_body_indents_and_keeps_semicolons() {
        let out = pretty_statement(
            "create trigger t after insert on m begin update m set a = 1 where a > 0; end",
        );
        assert_eq!(
            out,
            "CREATE TRIGGER t AFTER INSERT ON m\nBEGIN\n  UPDATE m\n  SET a = 1\n  WHERE a > 0;\nEND"
        );
    }

    #[test]
    fn multiple_statements_get_a_blank_line() {
        assert_eq!(pretty("select 1;select 2"), "SELECT 1\n\nSELECT 2");
    }

    #[test]
    fn identifiers_are_not_renamed() {
        let out = pretty("select MyCol from MyTable where MyCol is not null");
        assert!(out.contains("MyCol"), "{out}");
        assert!(out.contains("MyTable"), "{out}");
        assert!(out.contains("IS NOT NULL"), "{out}");
    }

    #[test]
    fn formatting_is_idempotent() {
        // 美化两次结果一致，才不会越点越乱
        let src = "select a,b, count(*) c from t where a>1 and b<2 order by c desc limit 10";
        let once = pretty(src);
        assert_eq!(pretty(&once), once, "{once}");
    }
}
