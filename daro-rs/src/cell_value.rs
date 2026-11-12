//! 单元格值的多视图（文本 / JSON / 十六进制 / 图像 / 网页）纯逻辑。
//!
//! 表数据页底部的「单元格编辑器」把当前选中单元格按几种视图呈现；这里只做
//! 「字符串 → 视图数据」的判定与转换，不含任何 UI，便于单测。

use serde_json::Value;

/// 抽屉页签文案与下标（顺序即显示顺序）。
pub const VIEW_TABS: [&str; 5] = ["文本", "JSON", "十六进制", "图像", "网页"];

pub const TAB_TEXT: usize = 0;
pub const TAB_JSON: usize = 1;
pub const TAB_HEX: usize = 2;
pub const TAB_IMAGE: usize = 3;
pub const TAB_WEB: usize = 4;

/// 十六进制视图的一行：偏移 + 本行字节 + 可打印字符侧栏。
pub struct HexLine {
    pub offset: String,
    pub hex: String,
    pub ascii: String,
}

/// 十六进制转储：每行 `bytes_per_line` 字节。按 UTF-8 取值——表格里的值本来
/// 就是文本，转储后能看出多字节字符与不可打印字符的真实分布。
///
/// 行形如 `0x00  31 37 …  1760…`：末行不足一行时右侧补空格，侧栏才能对齐。
pub fn hex_lines(value: &str, bytes_per_line: usize) -> Vec<HexLine> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes_per_line == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(bytes.len().div_ceil(bytes_per_line));
    for (row, chunk) in bytes.chunks(bytes_per_line).enumerate() {
        let mut hex = String::with_capacity(bytes_per_line * 3);
        let mut ascii = String::with_capacity(chunk.len());
        for (i, &b) in chunk.iter().enumerate() {
            if i > 0 {
                hex.push(' ');
            }
            hex.push_str(&format!("{b:02X}"));
            ascii.push(if (0x20..=0x7e).contains(&b) {
                b as char
            } else {
                '.'
            });
        }
        // 末行补到整行宽度，十六进制与 ASCII 两栏才对得齐
        let full = bytes_per_line * 3 - 1;
        if hex.len() < full {
            hex.push_str(&" ".repeat(full - hex.len()));
        }
        out.push(HexLine {
            offset: format!("0x{:02X}", row * bytes_per_line),
            hex,
            ascii,
        });
    }
    out
}

/// 合法 JSON 就格式化（缩进两格）；解析失败返回 None。
pub fn json_pretty(value: &str) -> Option<String> {
    let v: Value = serde_json::from_str(value.trim()).ok()?;
    serde_json::to_string_pretty(&v).ok()
}

/// JSON 行内着色用的记号类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JsonTok {
    /// 标点 / 空白
    Punct,
    /// 对象的键（字符串后跟冒号）
    Key,
    /// 字符串值
    Str,
    /// 数字
    Num,
    /// true / false / null
    Literal,
}

/// 把一行 JSON 切成着色片段。字符串 / 数字 / 字面量识别不出来的一律当标点，
/// 所以非 JSON 的普通文本也能安全着色（裸数字会按数字上色）。
pub fn json_spans(line: &str) -> Vec<(String, JsonTok)> {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<(String, JsonTok)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let tok;
        let text: String;
        if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() {
                match chars[i] {
                    '\\' => i = (i + 2).min(chars.len()),
                    '"' => {
                        i += 1;
                        break;
                    }
                    _ => i += 1,
                }
            }
            text = chars[start..i].iter().collect();
            // 跳过空白看下一个字符是不是冒号：是则这是键
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            tok = if j < chars.len() && chars[j] == ':' {
                JsonTok::Key
            } else {
                JsonTok::Str
            };
        } else if c.is_ascii_digit() || matches!(c, '-' | '+' | '.') {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit() || matches!(chars[i], '-' | '+' | '.' | 'e' | 'E'))
            {
                i += 1;
            }
            text = chars[start..i].iter().collect();
            tok = JsonTok::Num;
        } else if let Some(kw) = ["true", "false", "null"]
            .into_iter()
            .find(|kw| starts_with(&chars[i..], kw))
        {
            i += kw.chars().count();
            text = kw.to_string();
            tok = JsonTok::Literal;
        } else {
            i += 1;
            text = c.to_string();
            tok = JsonTok::Punct;
        }
        match out.last_mut() {
            Some((prev, prev_tok)) if *prev_tok == tok => prev.push_str(&text),
            _ => out.push((text, tok)),
        }
    }
    out
}

fn starts_with(chars: &[char], kw: &str) -> bool {
    kw.chars()
        .enumerate()
        .all(|(i, c)| chars.get(i) == Some(&c))
}

/// 识别出的图片格式（gpui 的 `ImageFormat` 在 UI 层映射）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Webp,
}

/// 按魔数识别图片格式；不认识返回 None。
pub fn image_kind(bytes: &[u8]) -> Option<ImageKind> {
    if bytes.len() < 12 {
        return None;
    }
    if bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        return Some(ImageKind::Png);
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(ImageKind::Jpeg);
    }
    if bytes.starts_with(b"GIF8") {
        return Some(ImageKind::Gif);
    }
    if bytes.starts_with(b"BM") {
        return Some(ImageKind::Bmp);
    }
    if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(ImageKind::Webp);
    }
    None
}

/// 剥掉 `data:image/png;base64,` 这类 data URI 前缀；无前缀时原样返回。
pub fn strip_data_uri(value: &str) -> &str {
    match value.find(',') {
        Some(comma) => {
            let head = value[..comma].to_ascii_lowercase();
            if head.starts_with("data:") && head.contains("base64") {
                &value[comma + 1..]
            } else {
                value
            }
        }
        None => value,
    }
}

/// base64 解码且首部是已知图片魔数时返回字节；普通字符串只会得到 None。
pub fn decode_base64_image(value: &str) -> Option<(ImageKind, Vec<u8>)> {
    use base64::Engine as _;
    let raw = strip_data_uri(value).trim();
    if raw.len() < 8 {
        return None;
    }
    let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() < 8 {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(compact.as_bytes())
        .ok()?;
    let kind = image_kind(&bytes)?;
    Some((kind, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_dump_splits_16_bytes_and_aligns_ascii() {
        let lines = hex_lines("17602985767634944", 16);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].offset, "0x00");
        assert_eq!(lines[0].ascii, "1760298576763494");
        assert_eq!(lines[0].hex.split(' ').count(), 16);
        assert_eq!(lines[1].offset, "0x10");
        assert_eq!(lines[1].ascii, "4");
        assert_eq!(lines[1].hex.trim_end(), "34", "末行补空格后仍能对齐");
    }

    #[test]
    fn hex_dump_marks_multibyte_and_control_bytes() {
        let lines = hex_lines("中\n", 16);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].ascii, "....", "三个多字节字节 + 换行都不可打印");
        assert!(lines[0].hex.starts_with("E4 B8 AD 0A"));
        assert!(hex_lines("", 16).is_empty());
    }

    #[test]
    fn json_pretty_formats_objects_and_keeps_scalars() {
        let obj = json_pretty(r#"{"a":1,"b":[true,null]}"#).unwrap();
        assert!(obj.contains("\n  \"a\": 1"));
        assert_eq!(json_pretty("17602985767634944").as_deref(), Some("17602985767634944"));
        assert_eq!(json_pretty("  [1, 2]  ").as_deref(), Some("[\n  1,\n  2\n]"));
        assert!(json_pretty("not json").is_none());
        assert!(json_pretty("a < b").is_none());
    }

    #[test]
    fn json_spans_classifies_keys_values_and_literals() {
        let spans = json_spans(r#"  "name": "阿离", "age": 26, "ok": true"#);
        let find = |text: &str| {
            spans
                .iter()
                .find(|(s, _)| s == text)
                .map(|(_, t)| *t)
                .unwrap_or(JsonTok::Punct)
        };
        assert_eq!(find("\"name\""), JsonTok::Key);
        assert_eq!(find("\"阿离\""), JsonTok::Str);
        assert_eq!(find("26"), JsonTok::Num);
        assert_eq!(find("true"), JsonTok::Literal);
        // 相邻标点合并成一个片段，避免逐字符建元素
        assert!(spans.iter().any(|(s, t)| *t == JsonTok::Punct && s.contains(':')));
    }

    #[test]
    fn base64_image_is_recognized_by_magic() {
        // 1×1 透明 PNG
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let (kind, bytes) = decode_base64_image(png).expect("PNG 应能解码");
        assert_eq!(kind, ImageKind::Png);
        assert_eq!(&bytes[1..4], b"PNG");
        let data_uri = format!("data:image/png;base64,{png}");
        assert!(decode_base64_image(&data_uri).is_some());
        assert!(decode_base64_image("17602985767634944").is_none(), "数字串不是图片");
        assert!(decode_base64_image("").is_none());
    }
}
