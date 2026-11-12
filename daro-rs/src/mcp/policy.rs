//! MCP 的语句分类与执行模式：判定一条 SQL 至少需要哪一档权限。
//!
//! 口径沿用 Flutter 版的定稿（白名单）：只有明确只读的语句算只读，识别不了的
//! 一律要求完全访问。判定基于归一化文本——注释剔除、字符串折成 `?`、引号标识符
//! 折成 `@`——所以 `WHERE remark = '-- 不是注释'` 不会被误判。
//!
//! 永真条件只认封闭清单（`TRUE` / `1` / `1=1` / `a=a` / `1<2` …）；清单外的
//! WHERE 一律当作有效条件，避免误伤正常更新。

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 三档执行模式，权限从低到高。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    Readonly,
    ReadWrite,
    Full,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "readonly" => Some(Mode::Readonly),
            "readWrite" => Some(Mode::ReadWrite),
            "full" => Some(Mode::Full),
            _ => None,
        }
    }

    pub fn wire(self) -> &'static str {
        match self {
            Mode::Readonly => "readonly",
            Mode::ReadWrite => "readWrite",
            Mode::Full => "full",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Readonly => "只读",
            Mode::ReadWrite => "数据读写",
            Mode::Full => "完全访问",
        }
    }

    /// 该档下一条语句的超时（秒）。本地文件库无法中途取消，超时后可能仍在跑。
    pub fn timeout_secs(self) -> u64 {
        match self {
            Mode::Readonly => 30,
            Mode::ReadWrite => 60,
            Mode::Full => 300,
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Readonly
    }
}

impl Serialize for Mode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.wire())
    }
}

/// 认不出的档位按只读处理——配置文件坏一点不该把权限放大。
impl<'de> Deserialize<'de> for Mode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Mode::parse(&s).unwrap_or(Mode::Readonly))
    }
}

/// 语句分类，以及它要求的最低执行模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlClass {
    ReadOnly,
    ScopedWrite,
    Dangerous,
    Unclassifiable,
}

impl SqlClass {
    pub fn minimum_mode(self) -> Mode {
        match self {
            SqlClass::ReadOnly => Mode::Readonly,
            SqlClass::ScopedWrite => Mode::ReadWrite,
            SqlClass::Dangerous | SqlClass::Unclassifiable => Mode::Full,
        }
    }
}

/// 当前档位能不能执行该分类的语句。
pub fn allows(mode: Mode, class: SqlClass) -> bool {
    mode >= class.minimum_mode()
}

/// 分类 [sql] 需要的最低执行模式。
pub fn classify(sql: &str) -> SqlClass {
    let statements = split_normalized(sql);
    // 多语句脚本一次调用里既可能读也可能写，不拆开判定
    if statements.len() != 1 {
        return SqlClass::Unclassifiable;
    }
    classify_one(&statements[0])
}

fn classify_one(s: &str) -> SqlClass {
    if s.is_empty() {
        return SqlClass::Unclassifiable;
    }
    match first_word(s) {
        "SELECT" => {
            // `SELECT ... INTO`（建表 / OUTFILE）与锁定读都会改东西
            if has_clause(s, "INTO")
                || has_clause(s, "FOR UPDATE")
                || has_clause(s, "LOCK IN SHARE MODE")
                || has_clause(s, "FOR NO KEY UPDATE")
                || has_clause(s, "FOR SHARE")
                || has_clause(s, "FOR KEY SHARE")
            {
                SqlClass::Dangerous
            } else {
                SqlClass::ReadOnly
            }
        }
        "SHOW" | "VALUES" | "LISTEN" => SqlClass::ReadOnly,
        "DESC" | "DESCRIBE" => {
            if looks_like_object_name(s) {
                SqlClass::ReadOnly
            } else {
                classify_explain(s)
            }
        }
        // PG 的 `TABLE foo` == SELECT * FROM foo
        "TABLE" => SqlClass::ReadOnly,
        // 无 `=` 的是读取，有 `=` 的是设置（部分 pragma 会改存储格式）
        "PRAGMA" => {
            if s.contains('=') {
                SqlClass::Dangerous
            } else {
                SqlClass::ReadOnly
            }
        }
        "WITH" => {
            if has_clause(s, "DELETE") {
                write_class(s)
            } else if has_clause(s, "UPDATE") {
                write_class(s)
            } else if has_clause(s, "INSERT") {
                SqlClass::ScopedWrite
            } else if has_clause(s, "MERGE") {
                SqlClass::Dangerous
            } else {
                SqlClass::ReadOnly
            }
        }
        "INSERT" | "REPLACE" => SqlClass::ScopedWrite,
        "UPDATE" | "DELETE" => write_class(s),
        // 只改 MCP 专用连接的会话上下文
        "USE" => SqlClass::ScopedWrite,
        "EXPLAIN" => classify_explain(s),
        "ANALYZE" | "MERGE" | "TRUNCATE" | "DROP" | "CREATE" | "ALTER" | "RENAME" | "GRANT"
        | "REVOKE" | "VACUUM" | "OPTIMIZE" | "REPAIR" | "CHECKPOINT" | "REFRESH" | "REINDEX"
        | "CLUSTER" | "KILL" | "SHUTDOWN" | "RESET" | "SET" | "CALL" | "EXEC" | "EXECUTE"
        | "DO" | "COPY" | "LOAD" | "LOCK" | "UNLOCK" | "COMMENT" | "SECURITY" | "DISCARD"
        | "PREPARE" | "DEALLOCATE" => SqlClass::Dangerous,
        _ => SqlClass::Unclassifiable,
    }
}

/// UPDATE / DELETE：没有 WHERE，或 WHERE 永真 → 全表操作，按高风险处理。
fn write_class(s: &str) -> SqlClass {
    match where_body(s) {
        None => SqlClass::Dangerous,
        Some(body) if is_tautology(&body) => SqlClass::Dangerous,
        Some(_) => SqlClass::ScopedWrite,
    }
}

/// `EXPLAIN <语句>`：内层只读才判只读；写语句（含 `EXPLAIN ANALYZE INSERT`
/// 这种会真跑的）一律高风险。
fn classify_explain(s: &str) -> SqlClass {
    let rest = strip_head(s, &["EXPLAIN", "DESC", "DESCRIBE"]);
    if rest.is_empty() {
        return SqlClass::Unclassifiable;
    }
    if classify_one(rest) == SqlClass::ReadOnly {
        SqlClass::ReadOnly
    } else {
        SqlClass::Dangerous
    }
}

/// 数据读写档下被拦的全表 UPDATE/DELETE，错误码用 WHERE_TOO_BROAD
/// （提示补过滤条件），与笼统的 MODE_DENIED（提示换档）区分开。
pub fn is_broad_write(sql: &str) -> bool {
    let statements = split_normalized(sql);
    if statements.len() != 1 {
        return false;
    }
    let s = &statements[0];
    let head = first_word(s);
    if head != "UPDATE" && head != "DELETE" {
        return false;
    }
    match where_body(s) {
        None => true,
        Some(body) => is_tautology(&body),
    }
}

// ------------------------------------------------------------- 文本处理 ----

fn split_normalized(sql: &str) -> Vec<String> {
    normalize(sql)
        .split(';')
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .collect()
}

/// 剔除注释、把字符串与引号标识符折成占位符、ASCII 字母统一大写。
fn normalize(sql: &str) -> String {
    let b = sql.as_bytes();
    let n = b.len();
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let mut i = 0;
    let mut block_depth = 0usize;
    while i < n {
        let ch = b[i];
        if block_depth > 0 {
            if ch == b'/' && i + 1 < n && b[i + 1] == b'*' {
                block_depth += 1;
                i += 2;
                continue;
            }
            if ch == b'*' && i + 1 < n && b[i + 1] == b'/' {
                block_depth -= 1;
                i += 2;
                if block_depth == 0 {
                    out.push(b' ');
                }
                continue;
            }
            i += 1;
            continue;
        }
        if ch == b'-' && i + 1 < n && b[i + 1] == b'-' {
            i = line_end(b, i);
            continue;
        }
        if ch == b'#' {
            i = line_end(b, i);
            continue;
        }
        if ch == b'/' && i + 1 < n && b[i + 1] == b'*' {
            block_depth = 1;
            i += 2;
            continue;
        }
        if ch == b'\'' {
            i = skip_quoted(b, i, b'\'', &mut out, b'?');
            continue;
        }
        // MySQL 里是字符串、PG 里是标识符；两种都折成 `@`
        if ch == b'"' || ch == b'`' {
            i = skip_quoted(b, i, ch, &mut out, b'@');
            continue;
        }
        // PG 美元引用 `$tag$ ... $tag$`
        if ch == b'$' {
            if let Some(tag) = dollar_tag_at(b, i) {
                let end = find_sub(b, i + tag.len(), &tag);
                i = match end {
                    Some(e) => e + tag.len(),
                    None => n,
                };
                out.push(b'?');
                continue;
            }
        }
        out.push(ch.to_ascii_uppercase());
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn line_end(b: &[u8], from: usize) -> usize {
    b[from..]
        .iter()
        .position(|&c| c == b'\n')
        .map(|p| from + p)
        .unwrap_or(b.len())
}

fn skip_quoted(b: &[u8], start: usize, quote: u8, out: &mut Vec<u8>, placeholder: u8) -> usize {
    let n = b.len();
    let mut i = start + 1;
    while i < n {
        let c = b[i];
        if c == b'\\' {
            i += 2;
            continue;
        }
        if c == quote {
            if i + 1 < n && b[i + 1] == quote {
                i += 2;
                continue;
            }
            out.push(placeholder);
            return i + 1;
        }
        i += 1;
    }
    // 引号未闭合：整段吞到结尾，避免把后面的内容当语句结构
    out.push(placeholder);
    n
}

fn dollar_tag_at(b: &[u8], i: usize) -> Option<Vec<u8>> {
    let mut j = i + 1;
    while j < b.len() {
        let c = b[j];
        if c == b'$' {
            return Some(b[i..=j].to_vec());
        }
        let is_ident = c == b'_'
            || c.is_ascii_alphabetic()
            || (j > i + 1 && c.is_ascii_digit());
        if !is_ident {
            return None;
        }
        j += 1;
    }
    None
}

fn find_sub(b: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from >= b.len() {
        return None;
    }
    b[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

fn first_word(s: &str) -> &str {
    let b = s.as_bytes();
    let start = b.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(b.len());
    let mut end = start;
    while end < b.len() && (b[end].is_ascii_alphanumeric() || b[end] == b'_') {
        end += 1;
    }
    &s[start..end]
}

/// `DESC 表名` / `DESCRIBE 表名` 形态（后面不是另一条 DML 关键字）。
fn looks_like_object_name(s: &str) -> bool {
    let rest = strip_head(s, &["DESC", "DESCRIBE"]);
    let head = first_word(rest);
    !head.is_empty()
        && !matches!(
            head,
            "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "WITH" | "VALUES" | "TABLE"
        )
}

/// 去掉开头的某个关键字（不区分大小写）及其后的空白。
fn strip_head<'a>(s: &'a str, heads: &[&str]) -> &'a str {
    for head in heads {
        let Some(prefix) = s.get(..head.len()) else {
            continue;
        };
        if prefix.eq_ignore_ascii_case(head) {
            let rest = &s[head.len()..];
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                return rest.trim_start();
            }
        }
    }
    s
}

/// 作为独立词出现的短语（归一化文本已大写）。
fn has_clause(s: &str, clause: &str) -> bool {
    word_positions(s, clause).next().is_some()
}

/// 找出所有独立词短语的起始位置。
fn word_positions<'a>(s: &'a str, phrase: &'a str) -> impl Iterator<Item = usize> + 'a {
    s.match_indices(phrase).filter_map(move |(at, _)| {
        let before_ok = at == 0
            || !s.as_bytes()[at - 1].is_ascii_alphanumeric()
                && s.as_bytes()[at - 1] != b'_';
        let end = at + phrase.len();
        let after_ok = end >= s.len()
            || !s.as_bytes()[end].is_ascii_alphanumeric() && s.as_bytes()[end] != b'_';
        (before_ok && after_ok).then_some(at)
    })
}

/// 取第一个顶层 WHERE 之后的条件文本（截到后续子句为止）。
fn where_body(s: &str) -> Option<String> {
    let at = word_positions(s, "WHERE").next()?;
    let body = &s[at + "WHERE".len()..];
    const STOPS: [&str; 17] = [
        "GROUP", "ORDER", "HAVING", "LIMIT", "OFFSET", "FETCH", "WINDOW", "RETURNING",
        "UNION", "INTERSECT", "EXCEPT", "FOR", "LOCK", "PROCEDURE", "INTO", "ON", "USING",
    ];
    let mut cut = body.len();
    for stop in STOPS {
        if let Some(p) = word_positions(body, stop).next() {
            cut = cut.min(p);
        }
    }
    Some(body[..cut].trim().to_string())
}

/// 永真条件：只认封闭清单。
fn is_tautology(where_clause: &str) -> bool {
    if where_clause.is_empty() {
        return true;
    }
    // 带括号的表达式不做结构化解析，一律当作有效条件
    if where_clause.contains('(') || where_clause.contains(')') {
        return false;
    }
    for or_group in split_word(where_clause, "OR") {
        let segments = split_word(&or_group, "AND");
        if segments.iter().all(|seg| is_tautology_segment(seg.trim())) {
            return true;
        }
    }
    false
}

fn is_tautology_segment(seg: &str) -> bool {
    if seg.is_empty() {
        return true;
    }
    if seg == "TRUE" || seg == "1" || seg == "?" {
        return true;
    }
    if let Some((lhs, op, rhs)) = split_cmp(seg) {
        match op {
            "=" => return lhs == rhs && is_word_or_placeholder(lhs),
            "<>" | "<" | ">" | "<=" | ">=" => {
                if let (Ok(l), Ok(r)) = (lhs.parse::<u64>(), rhs.parse::<u64>()) {
                    return match op {
                        "<>" => l != r,
                        "<" => l < r,
                        ">" => l > r,
                        "<=" => l <= r,
                        ">=" => l >= r,
                        _ => false,
                    };
                }
            }
            _ => {}
        }
        return false;
    }
    false
}

fn is_word_or_placeholder(s: &str) -> bool {
    !s.is_empty()
        && s
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'?')
}

/// 按比较运算符拆开（`<>` / `<=` / `>=` 优先于单字符）。
fn split_cmp(seg: &str) -> Option<(&str, &str, &str)> {
    let mut found: Option<(usize, &str)> = None;
    for op in ["<>", "<=", ">=", "=", "<", ">"] {
        if let Some(at) = seg.find(op) {
            // 同一位置优先两字符运算符（`<=` 不能被拆成 `<`）
            let take = match found {
                None => true,
                Some((f, prev)) => at < f || (at == f && op.len() > prev.len()),
            };
            if take {
                found = Some((at, op));
            }
        }
    }
    let (at, op) = found?;
    Some((seg[..at].trim(), op, seg[at + op.len()..].trim()))
}

/// 按独立词 [kw] 拆分，保留空段（`A OR` 这种残句也要参与判定）。
fn split_word(s: &str, kw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut search = 0;
    while let Some(rel) = s[search..].find(kw) {
        let at = search + rel;
        let before_ok = at == 0
            || !s.as_bytes()[at - 1].is_ascii_alphanumeric()
                && s.as_bytes()[at - 1] != b'_';
        let end = at + kw.len();
        let after_ok = end >= s.len()
            || !s.as_bytes()[end].is_ascii_alphanumeric() && s.as_bytes()[end] != b'_';
        if before_ok && after_ok {
            out.push(s[start..at].to_string());
            start = end;
        }
        search = at + kw.len().max(1);
        if search > s.len() {
            break;
        }
    }
    out.push(s[start..].to_string());
    out
}

