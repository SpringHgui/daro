//! 表数据导出（迁移自 Flutter `lib/data/db_export.dart` 的「导出表」路径）。
//!
//! 与 `export_format`（结果集导出，无 SQL 格式）的区别：
//! - 这里多一个 **SQL** 格式：按批生成 `INSERT INTO ... VALUES ...;`；
//! - 走**流式写入**：分块编码、每块检查取消标志并回报进度（对应 Flutter 的
//!   `ExportJob.onProgress` / 取消令牌）；
//! - 末了回报行数与字节数（对应 `ExportResult`）。
//!
//! 数据来源目前是 mock 行（真实驱动接入后换掉 `rows` 的来源即可，写盘逻辑不动）。

use crate::export_format::{self, DelimitedStyle, ExportFormat};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// 表数据导出支持的格式：结果集的五种 + SQL。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableFormat {
    Txt,
    Csv,
    Json,
    Xml,
    Html,
    Sql,
}

impl TableFormat {
    pub const ALL: [Self; 6] = [
        Self::Txt,
        Self::Csv,
        Self::Json,
        Self::Xml,
        Self::Html,
        Self::Sql,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Txt => "文本 (TXT)",
            Self::Csv => "CSV",
            Self::Json => "JSON",
            Self::Xml => "XML",
            Self::Html => "HTML",
            Self::Sql => "SQL",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Self::Sql => "sql",
            other => other.as_export().ext(),
        }
    }

    pub fn as_export(self) -> ExportFormat {
        match self {
            Self::Txt => ExportFormat::Txt,
            Self::Csv => ExportFormat::Csv,
            Self::Json => ExportFormat::Json,
            Self::Xml => ExportFormat::Xml,
            Self::Html => ExportFormat::Html,
            // SQL 没有对应的结果集格式；调用方不应走到这里
            Self::Sql => ExportFormat::Txt,
        }
    }
}

/// 一次表数据导出的设置。
#[derive(Clone, Debug)]
pub struct TableExportSettings {
    pub table: String,
    pub path: String,
    pub format: TableFormat,
    /// 分隔符格式（TXT / CSV）
    pub csv_style: DelimitedStyle,
    /// 是否输出列标题行
    pub with_header: bool,
    /// 是否写 UTF-8 BOM（Excel 中文）
    pub bom: bool,
    /// JSON 是否缩进
    pub pretty_json: bool,
    /// 是否给所有字段加引号（CSV）
    pub quote_all: bool,
    /// SQL 每条 INSERT 的行数
    pub batch_rows: usize,
    /// 标识符是否用反引号（MySQL）
    pub is_mysql: bool,
}

impl Default for TableExportSettings {
    fn default() -> Self {
        Self {
            table: String::new(),
            path: String::new(),
            format: TableFormat::Csv,
            csv_style: DelimitedStyle::STANDARD,
            with_header: true,
            bom: true,
            pretty_json: true,
            quote_all: false,
            batch_rows: 200,
            is_mysql: true,
        }
    }
}

/// 导出失败：IO / 取数错误或用户取消。
#[derive(Debug, PartialEq, Eq)]
pub enum ExportError {
    Io(String),
    Fetch(String),
    Cancelled,
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "写入失败：{e}"),
            Self::Fetch(e) => write!(f, "取数失败：{e}"),
            Self::Cancelled => write!(f, "已取消"),
        }
    }
}

/// 导出结果：行数 / 字节数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOutcome {
    pub rows: usize,
    pub bytes: u64,
}

/// 单元格 → SQL 字面量：`NULL` 原样，整数/浮点不加引号，其余单引号转义。
pub fn sql_literal(cell: &str) -> String {
    if export_format::is_null(cell) {
        return "NULL".into();
    }
    let t = cell.trim();
    if !t.is_empty()
        && (t.parse::<i64>().is_ok()
            || t.parse::<f64>().is_ok()
            || t.eq_ignore_ascii_case("true")
            || t.eq_ignore_ascii_case("false"))
    {
        return t.to_string();
    }
    format!("'{}'", cell.replace('\'', "''"))
}

fn quote_ident(name: &str, is_mysql: bool) -> String {
    if is_mysql {
        format!("`{name}`")
    } else {
        format!("\"{name}\"")
    }
}

/// 生成一批 INSERT：列名只出现在第一批（与 Flutter 一致：每批都带列名会影响性能，
/// 这里按批带列名，语义更明确）。
pub fn sql_insert<R: AsRef<[String]>>(
    table: &str,
    columns: &[String],
    rows: &[R],
    is_mysql: bool,
) -> Option<String> {
    if rows.is_empty() || columns.is_empty() {
        return None;
    }
    let cols: Vec<String> = columns.iter().map(|c| quote_ident(c, is_mysql)).collect();
    let mut out = String::new();
    out.push_str(&format!(
        "INSERT INTO {} ({}) VALUES\n",
        quote_ident(table, is_mysql),
        cols.join(", ")
    ));
    for (i, row) in rows.iter().enumerate() {
        let vals: Vec<String> = row.as_ref().iter().map(|c| sql_literal(c)).collect();
        out.push_str(&format!("({})", vals.join(", ")));
        if i + 1 == rows.len() {
            out.push_str(";\n");
        } else {
            out.push_str(",\n");
        }
    }
    Some(out)
}

/// 拉取一页行（空页 / None = 结束）。取数在后台线程按需调用，返回的
/// `Arc` 行可以直接编码，不再深拷到一整张表的 `Vec<Vec<String>>`。
pub type RowPage = Vec<crate::db_types::Row>;

/// 流式导出：按页拉取 → 分块编码 → 写盘，内存里最多驻留一页加一个编码
/// 缓冲区。`total_hint` 已知总行数时用于进度分母；整表流式取数给 `None`。
///
/// `progress(已处理行, 总行)` 每页调用一次；取消后返回
/// `ExportError::Cancelled`（已写入的半成品文件会被删除，避免留下残缺文件）。
pub fn export_table_stream(
    settings: &TableExportSettings,
    columns: &[String],
    total_hint: Option<usize>,
    mut next_page: impl FnMut() -> Result<Option<RowPage>, String>,
    progress: &mut dyn FnMut(usize, Option<usize>),
    cancel: &AtomicBool,
) -> Result<ExportOutcome, ExportError> {
    use std::io::Write;

    if settings.path.trim().is_empty() {
        return Err(ExportError::Io("导出路径为空".into()));
    }
    let path = Path::new(settings.path.trim());
    let file = std::fs::File::create(path).map_err(|e| ExportError::Io(e.to_string()))?;
    let mut out = std::io::BufWriter::new(file);
    let mut written = 0u64;
    let mut done = 0usize;

    let canceled = || cancel.load(Ordering::Relaxed);

    let result = (|| -> Result<(), ExportError> {
        if settings.format == TableFormat::Sql {
            let batch = settings.batch_rows.max(1);
            let mut pending: RowPage = Vec::with_capacity(batch);
            loop {
                if canceled() {
                    return Err(ExportError::Cancelled);
                }
                let Some(rows) = next_page().map_err(ExportError::Fetch)? else {
                    break;
                };
                if rows.is_empty() {
                    break;
                }
                done += rows.len();
                pending.extend(rows);
                while pending.len() >= batch {
                    let chunk: RowPage = pending.drain(..batch).collect();
                    let slices: Vec<&[String]> =
                        chunk.iter().map(|r| r.as_slice()).collect();
                    if let Some(sql) =
                        sql_insert(&settings.table, columns, &slices, settings.is_mysql)
                    {
                        out.write_all(sql.as_bytes())
                            .map_err(|e| ExportError::Io(e.to_string()))?;
                        written += sql.len() as u64;
                    }
                }
                progress(done, total_hint);
            }
            if !pending.is_empty() {
                let slices: Vec<&[String]> = pending.iter().map(|r| r.as_slice()).collect();
                if let Some(sql) =
                    sql_insert(&settings.table, columns, &slices, settings.is_mysql)
                {
                    out.write_all(sql.as_bytes())
                        .map_err(|e| ExportError::Io(e.to_string()))?;
                    written += sql.len() as u64;
                }
            }
        } else {
            let mut enc = export_format::RowEncoder::new(
                settings.format.as_export(),
                &settings.table,
                columns,
                settings.csv_style,
                settings.with_header,
                settings.quote_all,
                settings.bom,
                settings.pretty_json,
            );
            let head = enc.start();
            out.write_all(head.as_bytes())
                .map_err(|e| ExportError::Io(e.to_string()))?;
            written += head.len() as u64;
            let mut buf = String::new();
            loop {
                if canceled() {
                    return Err(ExportError::Cancelled);
                }
                let Some(rows) = next_page().map_err(ExportError::Fetch)? else {
                    break;
                };
                if rows.is_empty() {
                    break;
                }
                buf.clear();
                for row in &rows {
                    enc.push_row(row, &mut buf);
                }
                out.write_all(buf.as_bytes())
                    .map_err(|e| ExportError::Io(e.to_string()))?;
                written += buf.len() as u64;
                done += rows.len();
                progress(done, total_hint);
            }
            let tail = enc.finish();
            out.write_all(tail.as_bytes())
                .map_err(|e| ExportError::Io(e.to_string()))?;
            written += tail.len() as u64;
        }
        out.flush().map_err(|e| ExportError::Io(e.to_string()))?;
        Ok(())
    })();

    if let Err(e) = result {
        drop(out);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    Ok(ExportOutcome {
        rows: done,
        bytes: written,
    })
}

/// 已经在手的一批行：按页交给 `export_table_stream`，不额外复制单元格。
pub fn export_table(
    settings: &TableExportSettings,
    columns: &[String],
    rows: &[crate::db_types::Row],
    progress: &mut dyn FnMut(usize, usize),
    cancel: &AtomicBool,
) -> Result<ExportOutcome, ExportError> {
    const CHUNK: usize = 1000;
    let total = rows.len();
    let mut offset = 0usize;
    export_table_stream(
        settings,
        columns,
        Some(total),
        || {
            if offset >= rows.len() {
                return Ok(None);
            }
            let end = (offset + CHUNK).min(rows.len());
            let page = rows[offset..end].to_vec();
            offset = end;
            Ok(Some(page))
        },
        &mut |done, hint| progress(done, hint.unwrap_or(total)),
        cancel,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn sql_literal_quotes_and_numbers() {
        assert_eq!(sql_literal("NULL"), "NULL");
        assert_eq!(sql_literal("42"), "42");
        assert_eq!(sql_literal("3.14"), "3.14");
        assert_eq!(sql_literal("true"), "true");
        assert_eq!(sql_literal("it's"), "'it''s'");
        assert_eq!(sql_literal("abc"), "'abc'");
        assert_eq!(sql_literal(""), "''");
    }

    #[test]
    fn sql_insert_batches_and_quotes_identifiers() {
        let cols = row(&["id", "name"]);
        let rows = vec![row(&["1", "a"]), row(&["2", "NULL"])];
        let sql = sql_insert("t", &cols, &rows, true).unwrap();
        assert_eq!(
            sql,
            "INSERT INTO `t` (`id`, `name`) VALUES\n(1, 'a'),\n(2, NULL);\n"
        );
        let pg = sql_insert("t", &cols, &rows, false).unwrap();
        assert!(pg.starts_with("INSERT INTO \"t\" (\"id\", \"name\") VALUES"));
    }

    #[test]
    fn export_table_stream_batches_sql_across_pages() {
        let dir = std::env::temp_dir().join(format!("daro-rs-export-sql-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.sql");
        let cols = row(&["id", "name"]);
        let arc = |v: Vec<String>| std::sync::Arc::new(v);
        let pages = vec![
            vec![arc(row(&["1", "a"])), arc(row(&["2", "b"])), arc(row(&["3", "NULL"]))],
            vec![arc(row(&["4", "d"])), arc(row(&["5", "e"]))],
        ];
        let settings = TableExportSettings {
            table: "t".into(),
            path: path.display().to_string(),
            format: TableFormat::Sql,
            batch_rows: 2,
            is_mysql: true,
            ..Default::default()
        };
        let mut ix = 0;
        let mut seen = Vec::new();
        let outcome = export_table_stream(
            &settings,
            &cols,
            None,
            || {
                if ix >= pages.len() {
                    return Ok(None);
                }
                let page = pages[ix].clone();
                ix += 1;
                Ok(Some(page))
            },
            &mut |done, total| seen.push((done, total)),
            &AtomicBool::new(false),
        )
        .expect("导出应成功");
        let all: Vec<Vec<String>> = pages.iter().flatten().map(|r| (**r).clone()).collect();
        // 跨页也要按 batch_rows 继续攒批，不能每页各收一次尾
        let expected = [
            sql_insert("t", &cols, &all[0..2], true).unwrap(),
            sql_insert("t", &cols, &all[2..4], true).unwrap(),
            sql_insert("t", &cols, &all[4..5], true).unwrap(),
        ]
        .concat();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
        assert_eq!(outcome.rows, 5);
        assert_eq!(outcome.bytes, expected.len() as u64);
        assert_eq!(seen, vec![(3, None), (5, None)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_table_writes_csv_with_bom_and_cleans_up_on_cancel() {
        let dir = std::env::temp_dir().join(format!("daro-rs-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.csv");
        let cols = row(&["id", "name"]);
        let rows: Vec<crate::db_types::Row> = vec![
            std::sync::Arc::new(row(&["1", "a"])),
            std::sync::Arc::new(row(&["2", "b"])),
        ];

        let settings = TableExportSettings {
            table: "t".into(),
            path: path.display().to_string(),
            format: TableFormat::Csv,
            bom: true,
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);
        let mut seen = Vec::new();
        let outcome = export_table(&settings, &cols, &rows, &mut |d, t| seen.push((d, t)), &cancel)
            .expect("导出应成功");
        assert_eq!(outcome.rows, 2);
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "应写 UTF-8 BOM");
        let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
        assert_eq!(text, "id,name\r\n1,a\r\n2,b\r\n");
        let _ = std::fs::remove_file(&path);

        // 取消：半成品文件不应留下
        let path2 = dir.join("t2.csv");
        let settings2 = TableExportSettings {
            path: path2.display().to_string(),
            ..settings.clone()
        };
        let cancel2 = AtomicBool::new(true);
        let err = export_table(&settings2, &cols, &rows, &mut |_, _| {}, &cancel2).unwrap_err();
        assert_eq!(err, ExportError::Cancelled);
        assert!(!path2.exists(), "取消后应删除半成品");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
