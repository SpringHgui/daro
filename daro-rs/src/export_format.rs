//! 结果集导出格式（迁移自 Flutter 版 `lib/data/csv_codec.dart` 与
//! `lib/data/db_export.dart` 的文本类 writer）。
//!
//! 范围与 Flutter 的「导出结果」对话框一致：txt / csv / json / xml / html。
//! 结果集没有目标表，所以不提供 SQL（Flutter 同样把 sql 排除在结果导出之外）。
//!
//! CSV 口径逐条照搬 Flutter：
//! - 含分隔符 / 引号 / 换行 / 首尾空白的值加引号，内部引号双写；
//! - `quote_all` 时一律加引号（NULL 只在其文本非空且含特殊字符时加）；
//! - NULL 输出 `style.null_as`（默认空串）；
//! - `bom` 控制是否写 UTF-8 BOM（Excel 中文场景默认要）。
//!
//! 约定：数据层沿用 daro-rs 现有口径，单元格文本 `NULL` 视为真 NULL
//! （与 `copy_as_insert` 判空一致）。

/// 分隔风格：导出编码与导入解码共用
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelimitedStyle {
    /// 字段分隔符（单字符）
    pub delimiter: char,
    /// 字段引号字符（单字符，空字符 = 不做引号识别）
    pub quote: char,
    /// 行尾序列
    pub eol: &'static str,
    /// NULL 的输出文本
    pub null_as: &'static str,
}

impl Default for DelimitedStyle {
    fn default() -> Self {
        Self {
            delimiter: ',',
            quote: '"',
            eol: "\r\n",
            null_as: "",
        }
    }
}

impl DelimitedStyle {
    pub const STANDARD: Self = Self {
        delimiter: ',',
        quote: '"',
        eol: "\r\n",
        null_as: "",
    };
    pub const SEMICOLON: Self = Self {
        delimiter: ';',
        ..Self::STANDARD
    };
    pub const TAB: Self = Self {
        delimiter: '\t',
        ..Self::STANDARD
    };

    pub fn delimiter_label(self) -> &'static str {
        match self.delimiter {
            ',' => "逗号 (,)",
            ';' => "分号 (;)",
            '\t' => "制表符 (Tab)",
            '|' => "竖线 (|)",
            _ => "自定义",
        }
    }
}

/// 结果导出支持的格式
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Txt,
    Csv,
    Json,
    Xml,
    Html,
}

impl ExportFormat {
    pub const ALL: [Self; 5] = [
        Self::Txt,
        Self::Csv,
        Self::Json,
        Self::Xml,
        Self::Html,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Txt => "文本 (TXT)",
            Self::Csv => "CSV",
            Self::Json => "JSON",
            Self::Xml => "XML",
            Self::Html => "HTML",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Xml => "xml",
            Self::Html => "html",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Txt => "制表符分隔的纯文本，适合直接粘进表格软件",
            Self::Csv => "逗号分隔，可配分隔符 / 表头 / BOM；Excel 中文建议保留 BOM",
            Self::Json => "每行一个对象组成的数组，NULL 写成 null",
            Self::Xml => "每行一个 <row>，每个字段一个 <field>",
            Self::Html => "带样式的表格网页，可直接用浏览器打开",
        }
    }

    /// 是否走分隔文本 writer（txt / csv）
    pub fn delimited(self) -> bool {
        matches!(self, Self::Txt | Self::Csv)
    }

    pub fn style(self) -> DelimitedStyle {
        match self {
            Self::Txt => DelimitedStyle::TAB,
            _ => DelimitedStyle::STANDARD,
        }
    }
}

/// 单元格是否代表 NULL（数据层沿用 `copy_as_insert` 的口径）
pub fn is_null(cell: &str) -> bool {
    cell == "NULL"
}

fn contains_special(v: &str, delim: char, quote: char) -> bool {
    if v.contains(delim) {
        return true;
    }
    if quote != '\0' && v.contains(quote) {
        return true;
    }
    if v.contains('\n') || v.contains('\r') {
        return true;
    }
    // 首尾空白需要引号才能保真
    v.starts_with(' ') || v.ends_with(' ')
}

/// 把一行编码为 CSV 文本（不含行尾）。
pub fn csv_encode_row(cells: &[String], style: DelimitedStyle, quote_all: bool) -> String {
    let delim = style.delimiter;
    let q = style.quote;
    let mut out = String::new();
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            out.push(delim);
        }
        let null = is_null(cell);
        let raw = if null { style.null_as } else { cell.as_str() };
        let quoted = q != '\0'
            && if null {
                !style.null_as.is_empty() && contains_special(style.null_as, delim, q)
            } else {
                quote_all || contains_special(raw, delim, q)
            };
        if !quoted {
            out.push_str(raw);
        } else {
            out.push(q);
            out.push_str(&raw.replace(q, &format!("{q}{q}")));
            out.push(q);
        }
    }
    out
}

/// 编码整张表（txt / csv）。`with_header` 控制首行列名，`bom` 只在开头写一次。
pub fn encode_delimited(
    columns: &[String],
    rows: &[Vec<String>],
    style: DelimitedStyle,
    with_header: bool,
    quote_all: bool,
    bom: bool,
) -> String {
    let mut out = String::new();
    if bom {
        out.push('\u{feff}');
    }
    if with_header && !columns.is_empty() {
        out.push_str(&csv_encode_row(columns, style, quote_all));
        out.push_str(style.eol);
    }
    for row in rows {
        out.push_str(&csv_encode_row(row, style, quote_all));
        out.push_str(style.eol);
    }
    out
}

fn json_escape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for ch in v.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// 每行一个对象的 JSON 数组；NULL 写成 `null`。
pub fn encode_json(columns: &[String], rows: &[Vec<String>], pretty: bool) -> String {
    let mut out = String::from("[");
    if pretty {
        out.push('\n');
    }
    for (rix, row) in rows.iter().enumerate() {
        if rix > 0 {
            out.push(',');
        }
        if pretty {
            out.push_str("  ");
        }
        out.push('{');
        for (cix, col) in columns.iter().enumerate() {
            if cix > 0 {
                out.push_str(", ");
            }
            out.push('"');
            out.push_str(&json_escape(col));
            out.push_str("\": ");
            let cell = row.get(cix).map(|s| s.as_str()).unwrap_or("");
            if is_null(cell) {
                out.push_str("null");
            } else {
                out.push('"');
                out.push_str(&json_escape(cell));
                out.push('"');
            }
        }
        out.push('}');
        if pretty {
            out.push('\n');
        }
    }
    out.push(']');
    if pretty {
        out.push('\n');
    }
    out
}

fn xml_escape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for ch in v.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

fn html_escape(v: &str) -> String {
    xml_escape(v)
}

/// XML：`<table name="...">` 下每行一个 `<row>`，每字段一个 `<field>`。
pub fn encode_xml(table: &str, columns: &[String], rows: &[Vec<String>]) -> String {
    let eol = DelimitedStyle::STANDARD.eol;
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    out.push_str(eol);
    out.push_str(&format!("<table name=\"{}\">", xml_escape(table)));
    out.push_str(eol);
    for row in rows {
        out.push_str("  <row>");
        for (cix, col) in columns.iter().enumerate() {
            let cell = row.get(cix).map(|s| s.as_str()).unwrap_or("");
            if is_null(cell) {
                out.push_str(&format!("<field name=\"{}\" null=\"true\" />", xml_escape(col)));
            } else {
                out.push_str(&format!(
                    "<field name=\"{}\">{}</field>",
                    xml_escape(col),
                    xml_escape(cell)
                ));
            }
        }
        out.push_str("</row>");
        out.push_str(eol);
    }
    out.push_str("</table>");
    out.push_str(eol);
    out
}

/// HTML：带样式的表格（表头可选），NULL 单元格用 `td.null` 标出。
pub fn encode_html(table: &str, columns: &[String], rows: &[Vec<String>]) -> String {
    let eol = DelimitedStyle::STANDARD.eol;
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>");
    out.push_str(eol);
    out.push_str("<html lang=\"zh-CN\">\n<head><meta charset=\"UTF-8\">");
    out.push_str(&format!("<title>{}</title>", html_escape(table)));
    out.push_str(eol);
    out.push_str("<style>body{font-family:\"Microsoft YaHei\",sans-serif}table{border-collapse:collapse}th,td{border:1px solid #bbb;padding:2px 8px;font-weight:normal}th{background:#f2f2f2}td.null{color:#999;font-style:italic}</style>");
    out.push_str("</head>\n<body>");
    out.push_str(&format!("<h3>{}</h3>", html_escape(table)));
    out.push_str(eol);
    out.push_str("<table>");
    out.push_str(eol);
    out.push_str("<thead><tr>");
    for col in columns {
        out.push_str(&format!("<th>{}</th>", html_escape(col)));
    }
    out.push_str("</tr></thead>");
    out.push_str(eol);
    out.push_str("<tbody>");
    out.push_str(eol);
    for row in rows {
        out.push_str("<tr>");
        for (cix, _) in columns.iter().enumerate() {
            let cell = row.get(cix).map(|s| s.as_str()).unwrap_or("");
            if is_null(cell) {
                out.push_str("<td class=\"null\"></td>");
            } else {
                out.push_str(&format!("<td>{}</td>", html_escape(cell)));
            }
        }
        out.push_str("</tr>");
        out.push_str(eol);
    }
    out.push_str("</tbody>");
    out.push_str(eol);
    out.push_str("</table>");
    out.push_str(eol);
    out.push_str("</body>");
    out.push_str(eol);
    out.push_str("</html>");
    out.push_str(eol);
    out
}

/// 按格式分派到对应 writer。
pub fn encode(
    format: ExportFormat,
    table: &str,
    columns: &[String],
    rows: &[Vec<String>],
    csv_style: DelimitedStyle,
    with_header: bool,
    quote_all: bool,
    bom: bool,
    pretty_json: bool,
) -> String {
    match format {
        ExportFormat::Txt | ExportFormat::Csv => encode_delimited(
            columns,
            rows,
            if format == ExportFormat::Txt { ExportFormat::Txt.style() } else { csv_style },
            with_header,
            quote_all,
            bom,
        ),
        ExportFormat::Json => encode_json(columns, rows, pretty_json),
        ExportFormat::Xml => encode_xml(table, columns, rows),
        ExportFormat::Html => encode_html(table, columns, rows),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn csv_quotes_only_when_needed() {
        let style = DelimitedStyle::STANDARD;
        let row = vec![s("a"), s("b,c"), s("d\"e"), s(" pad "), s("line\nbreak")];
        assert_eq!(
            csv_encode_row(&row, style, false),
            "a,\"b,c\",\"d\"\"e\",\" pad \",\"line\nbreak\""
        );
    }

    #[test]
    fn csv_quote_all_and_null() {
        let style = DelimitedStyle::STANDARD;
        let row = vec![s("x"), s("NULL"), s("")];
        assert_eq!(csv_encode_row(&row, style, true), "\"x\",,\"\"");
        let with_null = DelimitedStyle {
            null_as: "\\N",
            ..DelimitedStyle::STANDARD
        };
        assert_eq!(csv_encode_row(&row, with_null, false), "x,\\N,");
    }

    #[test]
    fn delimited_header_and_bom() {
        let style = DelimitedStyle::STANDARD;
        let cols = vec![s("id"), s("名称")];
        let rows = vec![vec![s("1"), s("张三")]];
        let out = encode_delimited(&cols, &rows, style, true, false, true);
        assert!(out.starts_with('\u{feff}'));
        assert_eq!(out, "\u{feff}id,名称\r\n1,张三\r\n");
        let no_header = encode_delimited(&cols, &rows, style, false, false, false);
        assert_eq!(no_header, "1,张三\r\n");
    }

    #[test]
    fn json_pretty_and_null() {
        let cols = vec![s("id"), s("name")];
        let rows = vec![vec![s("1"), s("NULL")], vec![s("2"), s("a\"b")]];
        let compact = encode_json(&cols, &rows, false);
        assert_eq!(compact, "[{\"id\": \"1\", \"name\": null},{\"id\": \"2\", \"name\": \"a\\\"b\"}]");
        let pretty = encode_json(&cols, &rows, true);
        assert!(pretty.starts_with("[\n"));
        assert!(pretty.ends_with("]\n"));
    }

    #[test]
    fn xml_escapes_and_marks_null() {
        let cols = vec![s("a&b")];
        let rows = vec![vec![s("<x>")], vec![s("NULL")]];
        let out = encode_xml("t<1>", &cols, &rows);
        assert!(out.contains("<table name=\"t&lt;1&gt;\">"));
        assert!(out.contains("<field name=\"a&amp;b\">&lt;x&gt;</field>"));
        assert!(out.contains("<field name=\"a&amp;b\" null=\"true\" />"));
    }

    #[test]
    fn html_has_header_and_null_class() {
        let cols = vec![s("id")];
        let rows = vec![vec![s("NULL")]];
        let out = encode_html("t", &cols, &rows);
        assert!(out.contains("<th>id</th>"));
        assert!(out.contains("<td class=\"null\"></td>"));
    }
}
