//! Redis 字符串值的格式视图：探测 / 渲染 / 编码 / 大小文案。
//!
//! 口径照 Another Redis Desktop Manager：JSON 显示按 4 空格缩进、保存前校验并
//! 压成紧凑；Hex 可编辑并解析回字节；其余按文本原样。
//!
//! 这里**不用 serde_json 做 round-trip**：serde_json 默认把对象键按字典序重排，
//! 超出 u64/i64 的整数直接报「number out of range」，浮点也会换成最短表示 ——
//! 拿它 parse 再 serialize 等于悄悄改坏用户存的配置。所以校验与重排缩进都走
//! 下面的扫描器，token 一律原样搬运，只有 token 之间的空白会变。

/// 字符串值的展示与写回格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Text,
    Hex,
    Json,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Text, Format::Hex, Format::Json];

    /// 下拉与状态栏里的名字（与 ARDM 一致，首字母大写，不做本地化）。
    pub fn label(self) -> &'static str {
        match self {
            Format::Text => "Text",
            Format::Hex => "Hex",
            Format::Json => "Json",
        }
    }

    /// 编辑器高亮用的语言名；空串 = 不高亮。
    pub fn highlight_lang(self) -> &'static str {
        match self {
            Format::Json => "json",
            _ => "",
        }
    }
}

/// 首次载入时自动选格式。
///
/// 只自动认 Json：十六进制串（如 `"ABCDEF12"`）与十进制文本无法区分，猜成 Hex
/// 会把能读的值显示成一串码，所以 Hex 一律由用户手选。
pub fn detect(bytes: &[u8]) -> Format {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Format::Text;
    };
    let trimmed = text.trim_start();
    if (trimmed.starts_with('{') || trimmed.starts_with('[')) && validate(trimmed).is_ok() {
        Format::Json
    } else {
        Format::Text
    }
}

/// 按格式把字节渲染成编辑器里的文本。
pub fn render(bytes: &[u8], f: Format) -> String {
    match f {
        Format::Text => lossy_text(bytes),
        Format::Hex => hex_upper(bytes),
        Format::Json => match std::str::from_utf8(bytes) {
            // 排版失败时照 ARDM 回落成原文，让用户能直接改，而不是甩一个错误页
            Ok(text) => reindent(text, Some(4)).unwrap_or_else(|_| text.to_string()),
            Err(_) => hex_upper(bytes),
        },
    }
}

/// 按格式把编辑器文本编码回要写进 Redis 的字节。错误文案直接面向用户。
pub fn encode(text: &str, f: Format) -> Result<Vec<u8>, String> {
    match f {
        Format::Text => Ok(text.as_bytes().to_vec()),
        Format::Hex => decode_hex(text),
        Format::Json => {
            if text.trim().is_empty() {
                return Err("Json 格式不接受空值：要清空请切到 Text".to_string());
            }
            validate(text)?;
            Ok(reindent(text, None)?.into_bytes())
        }
    }
}

/// 只校验语法。
pub fn validate(text: &str) -> Result<(), String> {
    reindent(text, None).map(|_| ())
}

/// 重新排版：`Some(n)` 按 n 空格换行缩进，`None` 压成紧凑一行。
/// token（键名、字符串、数字）原样搬运，只改 token 之间的空白。
pub fn reindent(text: &str, indent: Option<usize>) -> Result<String, String> {
    let mut s = Scanner { src: text.as_bytes(), i: 0 };
    let mut out = String::with_capacity(text.len());
    s.ws();
    s.value(&mut out, 0, indent)?;
    s.ws();
    if s.i < s.src.len() {
        return Err(err(s.i, "JSON 结束后还有多余内容"));
    }
    Ok(out)
}

struct Scanner<'a> {
    src: &'a [u8],
    i: usize,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.i).copied()
    }

    fn err(&self, what: &str) -> String {
        err(self.i, what)
    }

    /// 跳过空白
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    /// 写一个值
    fn value(&mut self, out: &mut String, depth: usize, indent: Option<usize>) -> Result<(), String> {
        match self.peek() {
            Some(b'{') => self.object(out, depth, indent),
            Some(b'[') => self.array(out, depth, indent),
            _ => self.copy_atom(out),
        }
    }

    fn newline(&self, out: &mut String, depth: usize, indent: Option<usize>) {
        let Some(step) = indent else { return };
        out.push('\n');
        for _ in 0..depth * step {
            out.push(' ');
        }
    }

    fn object(&mut self, out: &mut String, depth: usize, indent: Option<usize>) -> Result<(), String> {
        self.i += 1; // {
        out.push('{');
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            out.push('}');
            return Ok(());
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("对象的键必须是字符串"));
            }
            self.newline(out, depth + 1, indent);
            self.copy_atom(out)?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(self.err("键后缺少 `:`"));
            }
            self.i += 1;
            out.push(':');
            self.ws();
            self.value(out, depth + 1, indent)?;
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                    out.push(',');
                }
                Some(b'}') => {
                    self.i += 1;
                    self.newline(out, depth, indent);
                    out.push('}');
                    return Ok(());
                }
                _ => return Err(self.err("对象缺少 `}`")),
            }
        }
    }

    fn array(&mut self, out: &mut String, depth: usize, indent: Option<usize>) -> Result<(), String> {
        self.i += 1; // [
        out.push('[');
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            out.push(']');
            return Ok(());
        }
        loop {
            self.ws();
            self.newline(out, depth + 1, indent);
            self.value(out, depth + 1, indent)?;
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                    out.push(',');
                }
                Some(b']') => {
                    self.i += 1;
                    self.newline(out, depth, indent);
                    out.push(']');
                    return Ok(());
                }
                _ => return Err(self.err("数组缺少 `]`")),
            }
        }
    }

    /// 标量与字符串：原样搬运字节 span
    fn copy_atom(&mut self, out: &mut String) -> Result<(), String> {
        let start = self.i;
        self.atom()?;
        // 语法字符全是 ASCII，非 ASCII 只可能落在字符串内部，按 UTF-8 取回即可
        let s = std::str::from_utf8(&self.src[start..self.i])
            .map_err(|_| err(start, "字符串含非法 UTF-8"))?;
        out.push_str(s);
        Ok(())
    }

    /// 走位一个标量（含字符串），不产出文本
    fn atom(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(b'"') => self.string(),
            Some(b't') => self.lit("true"),
            Some(b'f') => self.lit("false"),
            Some(b'n') => self.lit("null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(c) => Err(self.err(&format!("无法解析的字符 `{}`", c as char))),
            None => Err(self.err("内容意外结束")),
        }
    }

    fn lit(&mut self, word: &str) -> Result<(), String> {
        if self.src[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(())
        } else {
            Err(self.err("不是合法的字面量"))
        }
    }

    fn string(&mut self) -> Result<(), String> {
        self.i += 1; // 开引号
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err("字符串未闭合"));
            };
            self.i += 1;
            match c {
                b'"' => return Ok(()),
                b'\\' => {
                    let Some(e) = self.peek() else {
                        return Err(self.err("转义符后缺少字符"));
                    };
                    self.i += 1;
                    match e {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                        b'u' => {
                            for _ in 0..4 {
                                let Some(h) = self.peek() else {
                                    return Err(self.err("\\u 后不足 4 位十六进制"));
                                };
                                if !h.is_ascii_hexdigit() {
                                    return Err(self.err("\\u 后必须是 4 位十六进制"));
                                }
                                self.i += 1;
                            }
                        }
                        _ => return Err(self.err("非法转义符")),
                    }
                }
                c if c < 0x20 => return Err(self.err("字符串里有未转义的控制字符")),
                // UTF-8 续字节都 ≥ 0x80，不会误撞引号或反斜杠
                _ => {}
            }
        }
    }

    fn number(&mut self) -> Result<(), String> {
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.err("数字格式不对")),
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            let before = self.i;
            self.digits();
            if self.i == before {
                return Err(self.err("`小数点` 后缺少数字"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            let before = self.i;
            self.digits();
            if self.i == before {
                return Err(self.err("`指数` 后缺少数字"));
            }
        }
        Ok(())
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
    }
}

fn err(pos: usize, what: &str) -> String {
    format!("不是合法 JSON（第 {} 字节：{}）", pos + 1, what)
}

fn lossy_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(e) => {
            let mut s = String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned();
            let rest = bytes.len() - e.valid_up_to();
            s.push_str(&format!("\n…此处起 {rest} 字节不是合法 UTF-8，请切到 Hex 查看"));
            s
        }
    }
}

fn hex_upper(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for x in bytes {
        s.push_str(&format!("{x:02X}"));
    }
    s
}

/// 十六进制文本 → 字节；容忍空白与 `HEX:` 前缀。
pub fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    let clean: String = text
        .trim()
        .strip_prefix("HEX:")
        .unwrap_or(text.trim())
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if clean.len() % 2 != 0 {
        return Err("十六进制长度需为偶数位".to_string());
    }
    let bytes = clean.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char)
            .to_digit(16)
            .ok_or("十六进制里出现了非 0-9A-F 字符")?;
        let lo = (bytes[i + 1] as char)
            .to_digit(16)
            .ok_or("十六进制里出现了非 0-9A-F 字符")?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Ok(out)
}

/// 字节数 → `19.97KB` 这样的展示文案（1024 进制，保留两位）。
pub fn human_size(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    format!("{:.2}{}", v, UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_json_only_when_object_or_array() {
        assert_eq!(detect(b"{\"a\":1}"), Format::Json);
        assert_eq!(detect(b"  [1,2]  "), Format::Json);
        // 裸标量不认成 Json，避免把 "42" 这类值显示成缩进块
        assert_eq!(detect(b"42"), Format::Text);
        assert_eq!(detect(b"{bad json"), Format::Text);
        assert_eq!(detect(&[0xff, 0xfe]), Format::Text);
    }

    #[test]
    fn pretty_keeps_key_order_and_number_text() {
        let src = r#"{"b":1e100,"a":1.50,"z":[10000000000000000000000]}"#;
        let out = reindent(src, Some(4)).unwrap();
        // 键序与数字原文都不动（serde_json 会重排键、把 1.50 写成 1.5、大整数直接报错）
        assert!(out.contains("\"b\":1e100"), "{out}");
        assert!(out.contains("\"a\":1.50"), "{out}");
        assert!(out.contains("10000000000000000000000"), "{out}");
        assert!(out.starts_with("{\n    \"b\""), "{out}");
        assert!(out.ends_with("\n}"), "{out}");
    }

    #[test]
    fn compact_strips_only_insignificant_space() {
        let src = "{ \"a b\" : \"c  d\", \"e\" : [ 1 , 2 ] }";
        assert_eq!(reindent(src, None).unwrap(), r#"{"a b":"c  d","e":[1,2]}"#);
    }

    #[test]
    fn empty_containers_stay_empty() {
        assert_eq!(reindent("{ }", None).unwrap(), "{}");
        assert_eq!(reindent("[ ]", None).unwrap(), "[]");
        assert_eq!(reindent("{}", Some(4)).unwrap(), "{}");
    }

    #[test]
    fn validate_rejects_common_mistakes() {
        for bad in [
            "{\"a\":1,}",
            "{'a':1}",
            "{\"a\":1}{\"b\":2}",
            "{a:1}",
            "[1,2",
            "\"未闭合",
            "01",
            "1.",
            "1e",
            "{\"a\":0x1}",
            "{\"a\":1 extra}",
        ] {
            assert!(validate(bad).is_err(), "{bad} 应当不合法");
        }
        for ok in [
            r#"{"a":1}"#,
            "[1, 2, 3]",
            r#""just a string""#,
            "-0.5e-3",
            r#"{"嵌套":{"深":[{},[]]}}"#,
            r#"{"转义":"\"\\\n\t","u":"\u00e9"}"#,
        ] {
            assert!(validate(ok).is_ok(), "{ok} 应当合法：{}", validate(ok).err().unwrap());
        }
    }

    #[test]
    fn json_encode_compacts_and_hex_round_trips() {
        let bytes = encode("{ \"a\" : 1 }", Format::Json).unwrap();
        assert_eq!(bytes, r#"{"a":1}"#.as_bytes());
        assert!(encode("", Format::Json).is_err());
        assert!(encode("{\"a\":}", Format::Json).is_err());

        assert_eq!(encode("48656C6C6F", Format::Hex).unwrap(), b"Hello");
        assert_eq!(encode("HEX: 48 65", Format::Hex).unwrap(), b"He");
        assert!(encode("4865F", Format::Hex).is_err());
        assert!(encode("zz", Format::Hex).is_err());

        assert_eq!(encode("plain", Format::Text).unwrap(), b"plain");
    }

    #[test]
    fn render_matches_format() {
        assert_eq!(render(b"{\"a\":1}", Format::Json), "{\n    \"a\":1\n}");
        // 不合法的 JSON 在 Json 视图里回落成原文，仍可编辑
        assert_eq!(render(b"{oops", Format::Json), "{oops");
        assert_eq!(render(b"Hello", Format::Hex), "48656C6C6F");
        assert_eq!(render(b"Hi", Format::Text), "Hi");
        assert!(render(&[0xff, 0x00], Format::Text).contains("不是合法 UTF-8"));
    }

    #[test]
    fn human_size_units() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(20445), "19.97KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.00MB");
    }
}
