//! 数据导入：源文件解析（CSV / JSON）、字段映射与批量 INSERT。
//!
//! 口径照 Flutter 版 `lib/data/db_import.dart`：
//! - 只有一条解析管线：跳行、表头剥离、字段修剪都在管线内处理，
//!   预览与实际导入共用，保证「预览即所导」；
//! - 映射按**目标表列顺序**给，`source_index = None` 的列不参与写入
//!   （由数据库默认值填）；
//! - 写入按批合并成多值 INSERT；批内失败整批未写入。
//!
//! 与 Flutter 版的差别：Flutter 是流式（边读文件边攒批），这里先把文件
//! 读进内存再解析 —— 纯函数、易测；十万行以内没有区别。

use crate::mock::DbKind;
use crate::table_export::sql_literal;

/// 导入源格式
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportFormat {
    /// CSV / 文本（按分隔符切）
    Csv,
    /// JSON（顶层数组：对象数组或二维数组）
    Json,
}

impl ImportFormat {
    pub fn label(self) -> &'static str {
        match self {
            ImportFormat::Csv => "CSV / 文本",
            ImportFormat::Json => "JSON",
        }
    }
}

/// 一次导入请求的配置
#[derive(Clone, Debug)]
pub struct ImportOptions {
    pub format: ImportFormat,
    /// CSV 分隔符（第一个字符）
    pub delimiter: char,
    /// 跳过的行数（在表头判定之前生效）
    pub skip_rows: usize,
    /// 首行（跳行之后）是否为列名
    pub has_header: bool,
    /// 去除字段首尾空白
    pub trim_fields: bool,
    /// 空字段导入为 NULL（false 时按空串写入）
    pub empty_as_null: bool,
    /// 每条 INSERT 合并的行数
    pub batch_size: usize,
    /// 导入前清空目标表
    pub truncate_first: bool,
    /// 识别为 NULL 的文本（空 = 不识别）
    pub null_as: String,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            format: ImportFormat::Csv,
            delimiter: ',',
            skip_rows: 0,
            has_header: true,
            trim_fields: false,
            empty_as_null: true,
            batch_size: 200,
            truncate_first: false,
            null_as: String::new(),
        }
    }
}

/// 源文件结构：列标题 + 预览数据行 + 解析告警
#[derive(Clone, Debug, Default)]
pub struct ImportSource {
    /// 列标题（来自表头行；无表头时是「列1 / 列2」占位）
    pub headers: Vec<String>,
    /// 预览数据行
    pub sample: Vec<Vec<String>>,
    /// 解析告警（列数不一致等）
    pub warning: Option<String>,
}

/// 一个目标列的导入映射
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportColumn {
    pub target_column: String,
    /// 目标列类型展示文本
    pub type_label: String,
    /// 源文件里的列下标；None = 该列不参与导入
    pub source_index: Option<usize>,
    pub nullable: bool,
}

impl ImportColumn {
    pub fn new(target_column: impl Into<String>, type_label: impl Into<String>) -> Self {
        Self {
            target_column: target_column.into(),
            type_label: type_label.into(),
            source_index: None,
            nullable: true,
        }
    }
}

/// 导入执行结果
#[derive(Clone, Debug, Default)]
pub struct ImportResult {
    /// 成功写入的行数（失败批次按整批未写入计）
    pub rows_inserted: usize,
    /// 从源文件读取的数据行数
    pub rows_read: usize,
    /// 失败批次数
    pub failed_batches: usize,
    /// 错误信息（最多保留 [ImportResult::MAX_ERRORS] 条）
    pub errors: Vec<String>,
    pub cancelled: bool,
}

impl ImportResult {
    /// 保留的错误条数上限（防止大文件下无界增长）
    pub const MAX_ERRORS: usize = 20;

    pub fn ok(&self) -> bool {
        self.failed_batches == 0 && !self.cancelled
    }
}

/// 解析源文件的**原始记录**（未应用跳行 / 表头 / 修剪）。
///
/// CSV 逐字符解析（引号内的分隔符与换行不算分隔）；JSON 顶层数组时，
/// 对象数组先取首条记录的键序列作为隐式表头，再把每条记录转成值列表。
pub fn parse_records(text: &str, opts: &ImportOptions) -> Result<Vec<Vec<String>>, String> {
    match opts.format {
        ImportFormat::Csv => Ok(parse_csv_records(text, opts.delimiter)),
        ImportFormat::Json => parse_json_records(text, opts.has_header),
    }
}

/// CSV 解析：RFC4180 风格（双引号转义 ""、引号内可含分隔符与换行）。
pub fn parse_csv_records(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => in_quotes = true,
            '\r' => {
                // 吞掉 CRLF 的 CR
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            c if c == delimiter => {
                row.push(std::mem::take(&mut field));
            }
            c => field.push(c),
        }
    }
    // 文件末尾没有换行时补上最后一行
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// JSON 顶层数组 → 记录列表
///
/// 对象数组：首条记录的键序列就是隐式表头。有表头时把它放在**第一行**，
/// 这样「剥表头」在 CSV 与 JSON 走同一条管线（预览即所导）；
/// 没有表头时键序列不落行，全部记录都是数据行。
pub fn parse_json_records(text: &str, has_header: bool) -> Result<Vec<Vec<String>>, String> {
    let trimmed = text.trim_start_matches('\u{feff}');
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|e| format!("JSON 解析失败：{e}"))?;
    let serde_json::Value::Array(items) = value else {
        return Err("JSON 顶层必须是数组（对象数组或二维数组）".into());
    };
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut keys: Option<Vec<String>> = None;
    for item in items {
        match item {
            serde_json::Value::Array(cells) => {
                rows.push(cells.iter().map(json_cell).collect());
            }
            serde_json::Value::Object(map) => {
                let ks = keys.get_or_insert_with(|| map.keys().cloned().collect());
                rows.push(ks.iter().map(|k| json_cell(map.get(k).unwrap_or(&serde_json::Value::Null))).collect());
            }
            _ => {}
        }
    }
    if has_header {
        if let Some(ks) = keys {
            rows.insert(0, ks);
        }
    }
    Ok(rows)
}

fn json_cell(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// 读取源文件的列标题与前 `sample_rows` 行数据（供向导预览与映射）
pub fn read_sample(
    text: &str,
    opts: &ImportOptions,
    sample_rows: usize,
) -> Result<ImportSource, String> {
    let mut records = parse_records(text, opts)?;
    // 修剪字段
    for r in records.iter_mut() {
        if opts.trim_fields {
            for c in r.iter_mut() {
                *c = c.trim().to_string();
            }
        }
    }
    let mut idx = 0usize;
    for _ in 0..opts.skip_rows {
        if idx >= records.len() {
            break;
        }
        idx += 1;
    }
    let headers = if opts.has_header && idx < records.len() {
        let h = records[idx].clone();
        idx += 1;
        h
    } else {
        Vec::new()
    };
    let mut sample: Vec<Vec<String>> = Vec::new();
    let mut ragged = false;
    while idx < records.len() && sample.len() < sample_rows {
        let row = records[idx].clone();
        if !headers.is_empty() && row.len() != headers.len() {
            ragged = true;
        }
        sample.push(row);
        idx += 1;
    }
    let width = headers
        .len()
        .max(sample.iter().map(|r| r.len()).max().unwrap_or(0));
    let final_headers = if headers.is_empty() {
        (1..=width).map(|i| format!("列{i}")).collect()
    } else {
        headers
    };
    let final_width = final_headers.len();
    Ok(ImportSource {
        headers: final_headers,
        sample: sample
            .into_iter()
            .map(|r| fit_width(r, final_width))
            .collect(),
        warning: if ragged {
            Some("存在列数与表头不一致的行，已按表头宽度补齐 / 截断".into())
        } else {
            None
        },
    })
}

/// 解析源文件的**数据行**（已应用跳行 / 表头剥离 / 字段修剪）
pub fn import_rows(text: &str, opts: &ImportOptions) -> Result<Vec<Vec<String>>, String> {
    let mut records = parse_records(text, opts)?;
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut skipped = 0usize;
    let mut header_taken = false;
    for mut row in records.drain(..) {
        if opts.trim_fields {
            for c in row.iter_mut() {
                *c = c.trim().to_string();
            }
        }
        if skipped < opts.skip_rows {
            skipped += 1;
            continue;
        }
        if !header_taken && opts.has_header {
            header_taken = true;
            continue;
        }
        header_taken = true;
        out.push(row);
    }
    Ok(out)
}

/// 同名自动匹配：源列名与目标列名相等（忽略大小写）即配对，
/// 一个源列只配一次。
pub fn auto_map(headers: &[String], targets: &[String]) -> Vec<Option<usize>> {
    let mut used: Vec<bool> = vec![false; headers.len()];
    let mut out = Vec::with_capacity(targets.len());
    for t in targets {
        let mut found = None;
        for (ix, h) in headers.iter().enumerate() {
            if !used[ix] && h.trim().eq_ignore_ascii_case(t.trim()) {
                used[ix] = true;
                found = Some(ix);
                break;
            }
        }
        out.push(found);
    }
    out
}

/// 单元格 → SQL 字面量：命中 NULL 标记或（按选项）空串时输出 NULL
pub fn cell_literal(cell: &str, opts: &ImportOptions) -> String {
    if !opts.null_as.is_empty() && cell == opts.null_as {
        return "NULL".into();
    }
    if cell.is_empty() && opts.empty_as_null {
        return "NULL".into();
    }
    sql_literal(cell)
}

/// 目标表限定名（PG / SQL Server 带模式，MySQL 不带）
pub fn qualified_name(kind: DbKind, schema: &str, table: &str) -> String {
    let schema = schema.trim();
    match kind {
        DbKind::PostgreSql | DbKind::SqlServer => {
            if schema.is_empty() {
                quote_ident(kind, table)
            } else {
                format!(
                    "{}.{}",
                    quote_ident(kind, schema),
                    quote_ident(kind, table)
                )
            }
        }
        _ => quote_ident(kind, table),
    }
}

fn quote_ident(kind: DbKind, name: &str) -> String {
    match kind {
        DbKind::MySql => format!("`{}`", name.replace('`', "``")),
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// 按映射把数据行组装成批 INSERT 语句。
///
/// `mapping` 是目标表列顺序的映射列表；`source_index = None` 的列不写入。
pub fn build_insert_batches(
    rows: &[Vec<String>],
    mapping: &[ImportColumn],
    opts: &ImportOptions,
    kind: DbKind,
    schema: &str,
    table: &str,
) -> Vec<String> {
    let enabled: Vec<&ImportColumn> = mapping
        .iter()
        .filter(|m| m.source_index.is_some())
        .collect();
    if enabled.is_empty() || rows.is_empty() {
        return Vec::new();
    }
    let target = qualified_name(kind, schema, table);
    let cols: Vec<String> = enabled
        .iter()
        .map(|m| quote_ident(kind, &m.target_column))
        .collect();
    let prefix = format!("INSERT INTO {target} ({}) VALUES ", cols.join(", "));
    let batch_size = opts.batch_size.max(1);

    let mut out = Vec::new();
    let mut batch: Vec<String> = Vec::new();
    for row in rows {
        let mut tuple = String::from("(");
        for (i, m) in enabled.iter().enumerate() {
            if i > 0 {
                tuple.push_str(", ");
            }
            let src = m.source_index.unwrap_or(0);
            let value = row.get(src).map(|s| s.as_str()).unwrap_or("");
            tuple.push_str(&cell_literal(value, opts));
        }
        tuple.push(')');
        batch.push(tuple);
        if batch.len() >= batch_size {
            out.push(format!("{prefix}{};", batch.join(", ")));
            batch.clear();
        }
    }
    if !batch.is_empty() {
        out.push(format!("{prefix}{};", batch.join(", ")));
    }
    out
}

/// 按映射把数据行整理成「目标列顺序」的行（供 INSERT 组装与预览共用）
pub fn project_row(row: &[String], mapping: &[ImportColumn]) -> Vec<String> {
    mapping
        .iter()
        .map(|m| match m.source_index {
            Some(ix) => row.get(ix).cloned().unwrap_or_default(),
            None => String::new(),
        })
        .collect()
}

fn fit_width(row: Vec<String>, width: usize) -> Vec<String> {
    if row.len() == width {
        return row;
    }
    if row.len() < width {
        let mut r = row;
        r.resize(width, String::new());
        r
    } else {
        row.into_iter().take(width).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv_opts() -> ImportOptions {
        ImportOptions {
            format: ImportFormat::Csv,
            delimiter: ',',
            has_header: true,
            ..Default::default()
        }
    }

    #[test]
    fn csv_parses_quotes_and_embedded_delimiters() {
        let rows = parse_csv_records("a,b,c\n1,\"x,y\",\"he said \"\"hi\"\"\"\n", ',');
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["a", "b", "c"]);
        assert_eq!(rows[1], vec!["1", "x,y", "he said \"hi\""]);
    }

    #[test]
    fn csv_handles_crlf_and_missing_trailing_newline() {
        let rows = parse_csv_records("a,b\r\n1,2", ',');
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1], vec!["1", "2"]);
    }

    #[test]
    fn sample_skips_rows_and_strips_header() {
        let opts = ImportOptions {
            skip_rows: 1,
            ..csv_opts()
        };
        let src = read_sample("junk\nid,name\n1,张三\n2,李四\n", &opts, 10).expect("sample");
        assert_eq!(src.headers, vec!["id", "name"]);
        assert_eq!(src.sample.len(), 2);
        assert_eq!(src.sample[0], vec!["1", "张三"]);
        assert!(src.warning.is_none());
    }

    #[test]
    fn sample_without_header_uses_placeholder_columns() {
        let opts = ImportOptions {
            has_header: false,
            ..csv_opts()
        };
        let src = read_sample("1,2\n3,4\n", &opts, 10).expect("sample");
        assert_eq!(src.headers, vec!["列1", "列2"]);
        assert_eq!(src.sample.len(), 2);
    }

    #[test]
    fn ragged_rows_are_padded_and_warned() {
        let src = read_sample("a,b,c\n1,2\n", &csv_opts(), 10).expect("sample");
        assert_eq!(src.headers, vec!["a", "b", "c"]);
        assert_eq!(src.sample[0], vec!["1", "2", ""]);
        assert!(src.warning.unwrap().contains("列数与表头不一致"));
    }

    #[test]
    fn json_object_array_uses_first_record_keys_as_header() {
        let opts = ImportOptions {
            format: ImportFormat::Json,
            has_header: true,
            ..Default::default()
        };
        let text = r#"[{"id":1,"name":"a"},{"id":2,"name":"b","extra":9}]"#;
        let rows = import_rows(text, &opts).expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["1", "a"]);
        assert_eq!(rows[1], vec!["2", "b"]);

        // 表头取首条记录的键序（不是字典序），后续记录多出来的键忽略
        let src = read_sample(text, &opts, 10).expect("sample");
        assert_eq!(src.headers, vec!["id", "name"]);
        let src2 = read_sample(r#"[{"name":"a","id":1}]"#, &opts, 10).expect("sample");
        assert_eq!(src2.headers, vec!["name", "id"], "键序按文件里的出现顺序");
        assert_eq!(src2.sample[0], vec!["a", "1"]);
    }

    #[test]
    fn auto_map_matches_same_name_case_insensitively() {
        let headers = vec!["ID".to_string(), "name".to_string(), "x".to_string()];
        let targets = vec!["id".to_string(), "name".to_string(), "other".to_string()];
        let m = auto_map(&headers, &targets);
        assert_eq!(m, vec![Some(0), Some(1), None]);
    }

    #[test]
    fn cell_literal_honors_null_rules() {
        let mut opts = ImportOptions {
            empty_as_null: true,
            ..Default::default()
        };
        assert_eq!(cell_literal("", &opts), "NULL");
        assert_eq!(cell_literal("x", &opts), "'x'");
        opts.empty_as_null = false;
        assert_eq!(cell_literal("", &opts), "''");
        opts.null_as = "NULL".into();
        assert_eq!(cell_literal("NULL", &opts), "NULL");
    }

    #[test]
    fn insert_batches_respect_mapping_and_batch_size() {
        let mapping = vec![
            ImportColumn {
                target_column: "id".into(),
                type_label: "int4".into(),
                source_index: Some(0),
                nullable: false,
            },
            ImportColumn {
                target_column: "name".into(),
                type_label: "text".into(),
                source_index: Some(1),
                nullable: true,
            },
            ImportColumn {
                target_column: "skipped".into(),
                type_label: "text".into(),
                source_index: None,
                nullable: true,
            },
        ];
        let opts = ImportOptions {
            batch_size: 2,
            ..Default::default()
        };
        let rows = vec![
            vec!["1".to_string(), "张三".to_string()],
            vec!["2".to_string(), "李四".to_string()],
            vec!["3".to_string(), "王五".to_string()],
        ];
        let batches = build_insert_batches(
            &rows,
            &mapping,
            &opts,
            DbKind::PostgreSql,
            "public",
            "t",
        );
        assert_eq!(batches.len(), 2);
        assert!(batches[0].starts_with("INSERT INTO \"public\".\"t\" (\"id\", \"name\") VALUES "));
        assert!(batches[0].contains("(1, '张三'), (2, '李四')"), "{}", batches[0]);
        assert!(batches[1].contains("(3, '王五')"), "{}", batches[1]);
        assert!(!batches[0].contains("skipped"), "未映射的列不写进 SQL");
    }

    #[test]
    fn mysql_insert_uses_backticks_and_no_schema() {
        let mapping = vec![ImportColumn {
            target_column: "id".into(),
            type_label: "int".into(),
            source_index: Some(0),
            nullable: false,
        }];
        let rows = vec![vec!["7".to_string()]];
        let sql = build_insert_batches(&rows, &mapping, &ImportOptions::default(), DbKind::MySql, "", "t");
        assert!(sql[0].starts_with("INSERT INTO `t` (`id`) VALUES (7);"), "{}", sql[0]);
    }
}
