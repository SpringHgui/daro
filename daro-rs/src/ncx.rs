//! `.ncx` 兼容格式 导入/导出。
//!
//! 格式与 Flutter 版的 `.ncx` 数据模块完全对齐：
//! - XML 属性平铺，密码走 AES-128-CBC + PKCS#7（libcc 常量密钥）
//! - 分组用 daro 私有属性 `Group="..."`，其它工具会忽略
//! - 支持 MySQL / PostgreSQL / SQL Server；其余类型导出时跳过

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use crate::mock::DbKind;

/// 该格式内置密码加密密钥（libcc 常量，16 字节 ASCII）
const LIBCC_KEY: &[u8; 16] = b"libcckeylibcckey";

/// 该格式内置密码加密 IV（libcc 常量，末尾含一个空格，16 字节）
const LIBCC_IV: &[u8; 16] = b"libcciv libcciv ";

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

/// 密码解密结果状态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordState {
    /// 该连接本就不需要密码（如 SQLite 未加密、SQL Server Windows 身份验证）
    NoPassword,
    /// 密码已解密
    Decrypted,
    /// 导出端未勾选「保存密码」，导出文件里没有密文
    NotSaved,
    /// 有密文但解不出来（旧版加密方式等），导入后需手填
    Undecryptable,
}

/// 解析自导出文件的一条连接
#[derive(Clone, Debug)]
pub struct NcxConnection {
    /// `<Connection>` 元素上的全部属性（`.ano` 的 JSON 条目也归一到这张表）
    pub attributes: std::collections::HashMap<String, String>,
    /// 密码解密结果状态
    pub password_state: PasswordState,
    /// 明文密码，仅当 `password_state == Decrypted` 时有意义
    pub password: String,
    /// Redis 集群版标记（`.ano` 的 `cluster`）。daro 暂不支持集群，
    /// 这类条目在导入向导里灰显，导出时也不写 `cluster: true`。
    pub cluster: bool,
}

impl NcxConnection {
    pub fn attr(&self, key: &str) -> &str {
        self.attributes.get(key).map(|s| s.as_str()).unwrap_or("")
    }

    /// 连接显示名
    pub fn name(&self) -> &str {
        self.attr("ConnectionName")
    }

    /// 参考界面原始类型（MYSQL / POSTGRESQL / SQLSERVER / SQLITE ...）
    pub fn conn_type(&self) -> &str {
        self.attr("ConnType")
    }

    /// daro 私有的分组属性
    pub fn group(&self) -> &str {
        self.attr("Group").trim()
    }

    /// 归一到 daro 的 DbKind
    pub fn db_kind(&self) -> Option<DbKind> {
        match self.conn_type().to_uppercase().as_str() {
            "MYSQL" | "MARIADB" => Some(DbKind::MySql),
            "POSTGRESQL" => Some(DbKind::PostgreSql),
            "SQLSERVER" => Some(DbKind::SqlServer),
            "MONGODB" => Some(DbKind::MongoDb),
            "REDIS" => Some(DbKind::Redis),
            _ => None,
        }
    }

    /// SQLite / Access：寻址方式是文件路径而非 host:port
    pub fn is_file_based(&self) -> bool {
        matches!(
            self.conn_type().to_uppercase().as_str(),
            "SQLITE" | "ACCESS"
        )
    }

    /// 文件型连接的数据库文件路径
    pub fn file_path(&self) -> &str {
        let v = self.attr("DatabaseFileName");
        if v.is_empty() {
            self.attr("Database")
        } else {
            v
        }
    }

    pub fn host(&self) -> &str {
        if self.is_file_based() {
            self.file_path()
        } else {
            self.attr("Host")
        }
    }

    pub fn port(&self) -> &str {
        self.attr("Port")
    }

    pub fn database(&self) -> &str {
        if self.is_file_based() {
            self.file_path()
        } else {
            self.attr("Database")
        }
    }

    pub fn user_name(&self) -> &str {
        self.attr("UserName")
    }

    /// 需要用户在导入后自行补填密码
    pub fn needs_manual_password(&self) -> bool {
        self.password_state == PasswordState::NotSaved
            || self.password_state == PasswordState::Undecryptable
    }

    /// SQL Server 验证方式
    pub fn auth_method(&self) -> &str {
        if self.conn_type().to_uppercase() != "SQLSERVER" {
            return "";
        }
        if self.attr("MSSQLAuthenMode").to_uppercase() == "WINDOWS" {
            "windows"
        } else {
            "sql"
        }
    }

    /// 不能导入的原因（None = 可导入）。
    ///
    /// 口径照 Flutter 版 `NcxConnection.unsupportedReason`：引擎没实现的
    /// 类型、以及 Redis 集群版都挡住，前者给引擎名、后者给专门文案。
    pub fn unsupported_reason(&self) -> Option<String> {
        if self.db_kind().is_none() {
            return Some(if self.conn_type().is_empty() {
                "引擎未实现".to_string()
            } else {
                format!("引擎未实现({})", self.conn_type())
            });
        }
        if self.cluster {
            return Some("Redis 集群版暂不支持".to_string());
        }
        None
    }
}

/// 一个导出文件的解析结果
#[derive(Clone, Debug)]
pub struct NcxFile {
    /// 全部连接（保持文件中的原始顺序）
    pub connections: Vec<NcxConnection>,
    /// `<Connections Ver="1.5">` 里的导出格式版本
    pub format_version: String,
    /// 读到的文件格式（`.ncx` XML / `.ano` base64 JSON）
    pub format: ExportFormat,
}

/// 导出文件格式
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    /// `.ncx` 连接文件（XML）
    Ncx,
    /// Another Redis Desktop Manager 的 `.ano`（base64 JSON）
    Ano,
}

impl ExportFormat {
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Ncx => "连接文件 (.ncx)",
            ExportFormat::Ano => "Another Redis Desktop Manager (.ano)",
        }
    }

    /// 默认文件名（导出对话框的初始路径）
    pub fn default_file_name(self) -> &'static str {
        match self {
            ExportFormat::Ncx => "connections.ncx",
            ExportFormat::Ano => "connections.ano",
        }
    }

    /// 该格式能装下的连接类型：`.ncx` 有各引擎的属性模板（Redis 除外），
    /// `.ano` 只认 Redis。
    pub fn accepts(self, kind: DbKind) -> bool {
        match self {
            ExportFormat::Ncx => kind != DbKind::Redis,
            ExportFormat::Ano => kind == DbKind::Redis,
        }
    }

    /// 该格式装不下时的原因文案
    pub fn blocker(self, kind: DbKind) -> Option<&'static str> {
        if self.accepts(kind) {
            return None;
        }
        Some(match self {
            ExportFormat::Ncx => "Redis 按 .ano 导出，.ncx 里没有它的属性模板",
            ExportFormat::Ano => ".ano 只装 Redis 连接",
        })
    }
}

impl NcxFile {
    /// 解析导出文件文本，按**内容**认格式（后缀不可信，用户常手改扩展名）。
    ///
    /// 根元素是 `<` 走 XML（`.ncx`），否则按 base64 JSON 试（`.ano`）。
    pub fn parse(text: &str) -> Result<Self, String> {
        let trimmed = text.trim_start();
        if trimmed.starts_with('<') {
            return Self::parse_ncx(text);
        }
        Self::parse_ano(text)
    }

    /// 解析 NCX（XML）文本。格式不符时返回 Err。
    pub fn parse_ncx(text: &str) -> Result<Self, String> {
        let mut reader = Reader::from_str(text);
        reader.config_mut().trim_text(true);

        let mut connections = Vec::new();
        let mut format_version = String::new();
        let mut buf = Vec::new();

        // 检查根节点
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) if e.name().as_ref() == b"Connections" => {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"Ver" {
                            format_version = String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }
                    break;
                }
                Ok(Event::Eof) => return Err("文件不是合法的 NCX：缺少 <Connections> 根节点".into()),
                Err(e) => return Err(format!("XML 解析错误：{e}")),
                _ => {}
            }
            buf.clear();
        }

        // 解析连接条目
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Empty(ref e)) if e.name().as_ref() == b"Connection" => {
                    let mut attrs = std::collections::HashMap::new();
                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                        // 属性值是 XML 转义过的（名称里的 & 写成 &amp;），必须
                        // unescape 再入库，否则连接名会带着实体原样显示。
                        let val = attr
                            .unescape_value()
                            .map(|v| v.to_string())
                            .unwrap_or_else(|_| {
                                String::from_utf8_lossy(&attr.value).to_string()
                            });
                        attrs.insert(key, val);
                    }
                    let saved = attrs
                        .get("SavePassword")
                        .map(|v| v.to_lowercase() == "true")
                        .unwrap_or(false);
                    let password_hex = attrs.get("Password").cloned().unwrap_or_default();
                    let decoded = decrypt_ncx_password(&password_hex, saved);
                    connections.push(NcxConnection {
                        attributes: attrs,
                        password_state: decoded.0,
                        password: decoded.1,
                        cluster: false,
                    });
                }
                Ok(Event::End(ref e)) if e.name().as_ref() == b"Connections" => break,
                Ok(Event::Eof) => break,
                Err(e) => return Err(format!("XML 解析错误：{e}")),
                _ => {}
            }
            buf.clear();
        }

        if connections.is_empty() {
            return Err("文件里没有 <Connection> 条目，可能未导出任何连接".into());
        }

        Ok(NcxFile {
            connections,
            format_version,
            format: ExportFormat::Ncx,
        })
    }

    /// 解析 `.ano`（base64 JSON）文本。
    ///
    /// 这是 Another Redis Desktop Manager 的「设置 → 导出连接」产物，字段是
    /// `host / port / auth / username / name / connectionName / separator /
    /// cluster / connectionReadOnly / key / order / groupId`。
    ///
    /// 根节点两种形：**新版** `{connections: [...], groups: [...]}`（groups 里
    /// `{id, name}` 与连接的 `groupId` 配对），**旧版**是裸数组（无分组）。
    ///
    /// `auth` 是**明文口令**，不存在解密环节 —— 绝不进 AES 那条分支
    /// （十六进制形状的口令会解出垃圾）。`separator` / `connectionReadOnly`
    /// 在 daro 里没有对应字段，导入时丢弃；`cluster=true` 的条目保留标记，
    /// 由 [NcxConnection::unsupported_reason] 挡住。
    pub fn parse_ano(text: &str) -> Result<Self, String> {
        let bytes = decode_base64_lenient(text)?;
        let json = String::from_utf8(bytes)
            .map_err(|_| ".ano 解码后不是合法的 UTF-8 文本".to_string())?;
        let root: serde_json::Value = serde_json::from_str(json.trim_start_matches('\u{feff}'))
            .map_err(|e| format!("无法识别的文件格式：既不是 XML 导出(.ncx)，也不是 base64 JSON 导出(.ano)（{e}）"))?;

        let list: &[serde_json::Value] = match &root {
            serde_json::Value::Array(a) => a,
            serde_json::Value::Object(o) => match o.get("connections") {
                Some(serde_json::Value::Array(a)) => a,
                _ => {
                    return Err(".ano 的 JSON 根节点既不是数组，也没有 connections 列表".into());
                }
            },
            _ => {
                return Err(".ano 的 JSON 根节点既不是数组，也没有 connections 列表".into());
            }
        };

        // groups：id → name（没有 id 的分组条目直接丢，否则查不到时会冒充
        // 「未分组」的连接的目标）
        let mut group_names: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        if let serde_json::Value::Object(o) = &root
            && let Some(serde_json::Value::Array(gs)) = o.get("groups")
        {
            for g in gs {
                let Some(id) = g.get("id") else { continue };
                let id = ano_scalar(id);
                if id.trim().is_empty() {
                    continue;
                }
                let name = g.get("name").map(ano_scalar).unwrap_or_default();
                group_names.insert(id.trim().to_string(), name.trim().to_string());
            }
        }

        let mut connections = Vec::new();
        for item in list {
            let serde_json::Value::Object(map) = item else {
                continue;
            };
            let name = {
                let n = map.get("connectionName").map(ano_scalar).unwrap_or_default();
                if n.trim().is_empty() {
                    map.get("name").map(ano_scalar).unwrap_or_default()
                } else {
                    n
                }
            };
            let auth = map.get("auth").map(ano_scalar).unwrap_or_default();
            let group_id = map.get("groupId").map(ano_scalar).unwrap_or_default();
            let group = group_names
                .get(group_id.trim())
                .cloned()
                .unwrap_or_default();

            let mut attrs = std::collections::HashMap::new();
            // 这份文件没有类型字段：它是 Redis 连接管理器的导出，只可能指向 Redis
            attrs.insert("ConnType".into(), "REDIS".into());
            attrs.insert("ConnectionName".into(), name);
            attrs.insert("Host".into(), map.get("host").map(ano_scalar).unwrap_or_default());
            // 真实文件里 port 数字与字符串都出现过，一律按字符串收下
            attrs.insert("Port".into(), map.get("port").map(ano_scalar).unwrap_or_default());
            attrs.insert(
                "UserName".into(),
                map.get("username").map(ano_scalar).unwrap_or_default(),
            );
            if !group.trim().is_empty() {
                attrs.insert("Group".into(), group);
            }

            let cluster = map
                .get("cluster")
                .map(|v| matches!(v, serde_json::Value::Bool(true)))
                .unwrap_or(false);

            connections.push(NcxConnection {
                attributes: attrs,
                password_state: if auth.is_empty() {
                    PasswordState::NoPassword
                } else {
                    PasswordState::Decrypted
                },
                password: auth,
                cluster,
            });
        }

        if connections.is_empty() {
            return Err("文件里没有任何连接条目".into());
        }

        Ok(NcxFile {
            connections,
            format_version: String::new(),
            format: ExportFormat::Ano,
        })
    }
}

/// base64 解码（容忍换行 / 空白 / 缺 padding —— 导出工具会折行写出）
fn decode_base64_lenient(text: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let trimmed = cleaned.trim_end_matches('=');
    let mut padded = trimmed.to_string();
    while padded.len() % 4 != 0 {
        padded.push('=');
    }
    base64::engine::general_purpose::STANDARD
        .decode(padded.as_bytes())
        .map_err(|e| format!("无法识别的文件格式：既不是 XML 导出(.ncx)，也不是 base64 JSON 导出(.ano)（{e}）"))
}

/// JSON 标量 → 文本（数字 / 布尔 / 字符串都按文本收下）
fn ano_scalar(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// AES-128-CBC 解密 + PKCS#7 去填充后的明文
pub fn decrypt_ncx_password(cipher_hex: &str, saved: bool) -> (PasswordState, String) {
    if !saved {
        return (PasswordState::NotSaved, String::new());
    }
    if cipher_hex.is_empty() {
        return (PasswordState::NoPassword, String::new());
    }
    let cipher = match try_parse_hex(cipher_hex) {
        Some(c) if !c.is_empty() && c.len() % 16 == 0 => c,
        _ => return (PasswordState::Undecryptable, String::new()),
    };

    let mut buf = cipher.clone();
    let decrypted = match Aes128CbcDec::new(LIBCC_KEY.into(), LIBCC_IV.into()).decrypt_padded_mut::<Pkcs7>(&mut buf) {
        Ok(d) => d,
        Err(_) => return (PasswordState::Undecryptable, String::new()),
    };

    match decode_printable(decrypted) {
        Some(text) => (PasswordState::Decrypted, text),
        None => (PasswordState::Undecryptable, String::new()),
    }
}

/// 用该格式的 libcc 常量加密明文口令，输出大写十六进制密文
pub fn encrypt_ncx_password(plain: &str) -> String {
    let mut buf = vec![0u8; plain.len() + 16];
    buf[..plain.len()].copy_from_slice(plain.as_bytes());
    let encrypted = match Aes128CbcEnc::new(LIBCC_KEY.into(), LIBCC_IV.into()).encrypt_padded_mut::<Pkcs7>(&mut buf, plain.len()) {
        Ok(e) => e,
        Err(_) => return String::new(),
    };
    let mut hex = String::with_capacity(encrypted.len() * 2);
    for b in encrypted {
        hex.push_str(&format!("{:02X}", b));
    }
    hex
}

/// 该格式按 UTF-8 存密码字节，解出的字节必须能按 UTF-8 解码且不含控制字符
fn decode_printable(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return Some(String::new());
    }
    for &b in bytes {
        if b < 0x20 || b == 0x7f {
            return None;
        }
    }
    String::from_utf8(bytes.to_vec()).ok()
}

/// 严格十六进制解析
fn try_parse_hex(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || text.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = hex_digit(bytes[i])?;
        let lo = hex_digit(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

// ── 导出 ──────────────────────────────────────────────────────

/// 不可导出的 daro 连接类型 → 原因
pub fn ncx_export_blocker(kind: DbKind) -> Option<&'static str> {
    match kind {
        DbKind::Redis => Some("Redis 按 .ano 导出，.ncx 里没有它的属性模板"),
        _ => None,
    }
}

/// 一次 `.ano` 导出的结果
#[derive(Clone, Debug)]
pub struct AnoExportResult {
    /// 待写盘的 base64 文本（单行）
    pub text: String,
    /// 实际写进文件的连接名
    pub exported: Vec<String>,
    /// 因格式装不下而跳过的连接（名称 + 原因）
    pub skipped: Vec<(String, String)>,
}

/// 生成 `.ano` 文本（base64 的 JSON）。
///
/// 结构与 Another Redis Desktop Manager 的导出对齐：
/// `{connections: [...], groups: [{id, name}]}`。连接条目字段：
/// `host / port / auth / username / name / connectionName / separator /
/// cluster / connectionReadOnly / key / order / groupId`。
///
/// `auth` 是**明文**（该格式没有加密环节）；`cluster` 恒 false（daro 不支持
/// 集群）；`separator` / `connectionReadOnly` / `key` 是那个工具的显示糖，
/// 写出默认值即可。
pub fn build_ano(
    connections: &[ExportConnection],
    include_passwords: bool,
) -> AnoExportResult {
    use base64::Engine;
    let mut exported = Vec::new();
    let mut skipped = Vec::new();
    let mut items: Vec<serde_json::Value> = Vec::new();
    let mut groups: Vec<serde_json::Value> = Vec::new();
    let mut seen_groups: Vec<String> = Vec::new();

    for (ix, conn) in connections.iter().enumerate() {
        if conn.kind != DbKind::Redis {
            skipped.push((conn.name.clone(), ".ano 只装 Redis 连接".to_string()));
            continue;
        }
        let group = conn.group.trim();
        if !group.is_empty() && !seen_groups.iter().any(|g| g == group) {
            seen_groups.push(group.to_string());
            groups.push(serde_json::json!({ "id": group, "name": group }));
        }
        let port = if conn.port.trim().is_empty() {
            conn.kind.default_port().to_string()
        } else {
            conn.port.trim().to_string()
        };
        items.push(serde_json::json!({
            "host": conn.host,
            "port": port,
            "auth": if include_passwords { conn.password.clone() } else { String::new() },
            "username": conn.username,
            "name": conn.name,
            "connectionName": conn.name,
            "separator": "",
            "cluster": false,
            "connectionReadOnly": true,
            "key": "",
            "order": ix,
            "groupId": group,
        }));
        exported.push(conn.name.clone());
    }

    let root = serde_json::json!({ "connections": items, "groups": groups });
    let json = root.to_string();
    let text = base64::engine::general_purpose::STANDARD.encode(json.as_bytes());
    AnoExportResult {
        text,
        exported,
        skipped,
    }
}

/// 一次导出的结果
#[derive(Clone, Debug)]
pub struct NcxExportResult {
    /// 待写盘的 .ncx 文本（UTF-8，CRLF）
    pub xml: String,
    /// 实际写进文件的连接名
    pub exported: Vec<String>,
    /// 因目标格式无对应类型而跳过的连接（名称 + 原因）
    pub skipped: Vec<(String, String)>,
}

/// 导出连接数据结构（简化版，对应 ConnectionInfo）
#[derive(Clone, Serialize, Deserialize)]
pub struct ExportConnection {
    pub name: String,
    pub kind: DbKind,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub database: String,
    pub auth_method: String,
    pub group: String,
}

/// 生成 .ncx 文本
pub fn build_ncx_file(
    connections: &[ExportConnection],
    include_passwords: bool,
    settings_save_root: &str,
) -> NcxExportResult {
    let mut exported = Vec::new();
    let mut skipped = Vec::new();
    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b'\t', 1);

    // XML 声明
    writer
        .write_event(Event::Decl(quick_xml::events::BytesDecl::new("1.0", Some("UTF-8"), None)))
        .unwrap();
    writer.write_event(Event::Text(BytesText::new("\r\n"))).unwrap();

    // 根节点
    let mut root = BytesStart::new("Connections");
    root.push_attribute(("Ver", "1.5"));
    writer.write_event(Event::Start(root)).unwrap();
    writer.write_event(Event::Text(BytesText::new("\r\n"))).unwrap();

    for conn in connections {
        if let Some(reason) = ncx_export_blocker(conn.kind) {
            skipped.push((conn.name.clone(), reason.to_string()));
            continue;
        }

        let entry = build_entry(conn, include_passwords, settings_save_root);
        let mut elem = BytesStart::new("Connection");
        for (k, v) in &entry {
            elem.push_attribute((k.as_str(), v.as_str()));
        }
        writer.write_event(Event::Empty(elem)).unwrap();
        writer.write_event(Event::Text(BytesText::new("\r\n"))).unwrap();
        exported.push(conn.name.clone());
    }

    writer.write_event(Event::End(BytesEnd::new("Connections"))).unwrap();
    writer.write_event(Event::Text(BytesText::new("\r\n\r\n"))).unwrap();

    let xml = String::from_utf8(writer.into_inner().into_inner()).unwrap();
    NcxExportResult {
        xml,
        exported,
        skipped,
    }
}

/// 单条连接的属性装配
fn build_entry(
    conn: &ExportConnection,
    include_passwords: bool,
    settings_save_root: &str,
) -> Vec<(String, String)> {
    let conn_type = match conn.kind {
        DbKind::MySql => "MYSQL",
        DbKind::PostgreSql => "POSTGRESQL",
        DbKind::SqlServer => "SQLSERVER",
        DbKind::SQLite => "SQLITE",
        DbKind::MongoDb => "MONGODB",
        DbKind::Redis => "REDIS",
    };
    let folder = match conn.kind {
        DbKind::MySql => "MySQL",
        DbKind::PostgreSql => "PostgreSQL",
        DbKind::SqlServer => "SQL Server",
        DbKind::SQLite => "SQLite",
        DbKind::MongoDb => "MongoDB",
        DbKind::Redis => "Redis",
    };

    let is_file_based = false; // daro 当前不支持 SQLite 导出
    let host = if is_file_based {
        if !conn.database.is_empty() {
            &conn.database
        } else {
            &conn.host
        }
    } else {
        &conn.host
    };
    let port = if !conn.port.is_empty() {
        conn.port.clone()
    } else {
        match conn_type {
            "MYSQL" => "3306".to_string(),
            "POSTGRESQL" => "5432".to_string(),
            "SQLSERVER" => "1433".to_string(),
            _ => String::new(),
        }
    };

    let root = settings_save_root.trim_end_matches('\\');
    let settings_save_path = format!("{root}\\{folder}\\Servers\\{}", conn.name);

    let is_windows_auth = conn_type == "SQLSERVER" && conn.auth_method == "windows";
    let password_cipher = if !include_passwords {
        String::new()
    } else if conn.password.is_empty() && !is_windows_auth {
        String::new()
    } else {
        encrypt_ncx_password(&conn.password)
    };

    let save_password = if include_passwords
        && (!conn.password.is_empty() || is_windows_auth)
    {
        "true"
    } else {
        "false"
    };

    let mut attrs = vec![
        ("ConnectionName".into(), conn.name.clone()),
        ("ProjectUUID".into(), String::new()),
        ("ConnType".into(), conn_type.into()),
        ("ServiceProvider".into(), "Default".into()),
    ];

    match conn_type {
        "MYSQL" => {
            attrs.extend([
                ("Host".into(), host.into()),
                ("Port".into(), port),
                ("UserName".into(), conn.username.clone()),
                ("Password".into(), password_cipher),
                ("SavePassword".into(), save_password.into()),
                ("SettingsSavePath".into(), settings_save_path),
                ("SessionLimit".into(), "0".into()),
                ("InitialSessionQueries".into(), String::new()),
                ("ClientDriverVersion".into(), "Default".into()),
                ("ClientCharacterSet".into(), String::new()),
                ("ClientEncoding".into(), "65001".into()),
                ("Keepalive".into(), "false".into()),
                ("UseConnectionTimeout".into(), "true".into()),
                ("ConnectionTimeoutSeconds".into(), "30".into()),
                ("UseReadTimeout".into(), "false".into()),
                ("UseWriteTimeout".into(), "true".into()),
                ("WriteTimeoutSeconds".into(), "30".into()),
                ("Encoding".into(), "65001".into()),
                ("MySQLCharacterSet".into(), "true".into()),
                ("Compression".into(), "false".into()),
                ("AutoConnect".into(), "false".into()),
                ("NamedPipe".into(), "false".into()),
                ("UseAdvanced".into(), "false".into()),
                ("SSL".into(), "false".into()),
                ("SSH".into(), "false".into()),
                ("HTTP".into(), "false".into()),
                ("Compatibility".into(), "false".into()),
                ("Remarks".into(), String::new()),
            ]);
        }
        "POSTGRESQL" => {
            attrs.extend([
                ("Host".into(), host.into()),
                ("Port".into(), port),
                ("Database".into(), conn.database.clone()),
                ("UserName".into(), conn.username.clone()),
                ("Password".into(), password_cipher),
                ("SavePassword".into(), save_password.into()),
                ("HostTypePreference".into(), "Default".into()),
                ("SettingsSavePath".into(), settings_save_path),
                ("ConnectionParameters".into(), String::new()),
                ("SessionLimit".into(), "0".into()),
                ("InitialSessionQueries".into(), String::new()),
                ("Keepalive".into(), "false".into()),
                ("AutoConnect".into(), "false".into()),
                ("ClientDriverEncoding".into(), String::new()),
                ("ClientEncoding".into(), "65001".into()),
                ("UseAdvanced".into(), "false".into()),
                ("SSL".into(), "false".into()),
                ("SSH".into(), "false".into()),
                ("HTTP".into(), "false".into()),
                ("Remarks".into(), String::new()),
            ]);
        }
        "SQLSERVER" => {
            attrs.extend([
                ("Host".into(), host.into()),
                ("Port".into(), port),
                ("PortSpecified".into(), "false".into()),
                ("Database".into(), conn.database.clone()),
                (
                    "MSSQLAuthenMode".into(),
                    if conn.auth_method == "windows" {
                        "WINDOWS".into()
                    } else {
                        "SQLSERVER".into()
                    },
                ),
                ("MSSQLAuthenWindowsDomain".into(), String::new()),
                ("UserName".into(), conn.username.clone()),
                ("Password".into(), password_cipher),
                ("SavePassword".into(), save_password.into()),
                ("SettingsSavePath".into(), settings_save_path),
                ("ConnectionParameters".into(), String::new()),
                ("SessionLimit".into(), "0".into()),
                ("InitialSessionQueries".into(), String::new()),
                ("TimeoutReconnection".into(), "false".into()),
                ("UseConnectionTimeout".into(), "false".into()),
                ("UseExecutionTimeout".into(), "false".into()),
                ("TrustServerCertificate".into(), "false".into()),
                ("AutoConnect".into(), "false".into()),
                ("UseAdvanced".into(), "false".into()),
                ("UseEncryption".into(), "false".into()),
                ("NativeClientDriver".into(), String::new()),
                ("SSH".into(), "false".into()),
                ("Remarks".into(), String::new()),
            ]);
        }
        _ => {}
    }

    // daro 私有分组属性
    let group = conn.group.trim();
    if !group.is_empty() {
        attrs.push(("Group".into(), group.into()));
    }

    attrs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip() {
        let plain = "test_password_123";
        let encrypted = encrypt_ncx_password(plain);
        let (state, decrypted) = decrypt_ncx_password(&encrypted, true);
        assert_eq!(state, PasswordState::Decrypted);
        assert_eq!(decrypted, plain);
    }

    #[test]
    fn empty_password() {
        let (state, decrypted) = decrypt_ncx_password("", true);
        assert_eq!(state, PasswordState::NoPassword);
        assert_eq!(decrypted, "");
    }

    #[test]
    fn not_saved_password() {
        let (state, decrypted) = decrypt_ncx_password("", false);
        assert_eq!(state, PasswordState::NotSaved);
        assert_eq!(decrypted, "");
    }

    #[test]
    fn known_cipher() {
        // 该格式空密码的已知密文
        let (state, decrypted) =
            decrypt_ncx_password("E191AF42327478CC5F143EF279EC4D81", true);
        assert_eq!(state, PasswordState::Decrypted);
        assert_eq!(decrypted, "");
    }

    #[test]
    fn export_round_trips_name_group_and_password() {
        // 导出再解析回来：名称里的 & 与中文、分组私有属性、密码密文
        // 都必须原样往返（Flutter 版导出测试的同款口径）。
        let conn = ExportConnection {
            name: "演示 & 连接".into(),
            kind: DbKind::MySql,
            host: "db.example.com".into(),
            port: "3306".into(),
            username: "root".into(),
            password: "p@ss".into(),
            database: "app".into(),
            auth_method: String::new(),
            group: "测试组".into(),
        };
        let result = build_ncx_file(&[conn], true, r"C:\Users\测试\Documents\Connections");
        assert_eq!(result.exported, vec!["演示 & 连接".to_string()]);
        assert!(result.skipped.is_empty());

        let parsed = NcxFile::parse(&result.xml).expect("round-trip parse");
        assert_eq!(parsed.connections.len(), 1);
        let c = &parsed.connections[0];
        assert_eq!(c.name(), "演示 & 连接");
        assert_eq!(c.group(), "测试组");
        assert_eq!(c.host(), "db.example.com");
        assert_eq!(c.password_state, PasswordState::Decrypted);
        assert_eq!(c.password, "p@ss");
    }

    #[test]
    fn ano_round_trip_redis_connections() {
        let conn = ExportConnection {
            name: "本地 Redis".into(),
            kind: DbKind::Redis,
            host: "127.0.0.1".into(),
            port: "6379".into(),
            username: String::new(),
            password: "s3cret".into(),
            database: String::new(),
            auth_method: String::new(),
            group: "开发环境".into(),
        };
        let result = build_ano(&[conn], true);
        assert_eq!(result.exported, vec!["本地 Redis".to_string()]);
        assert!(result.skipped.is_empty());

        // 导出的 base64 解回来是 JSON
        let raw = decode_base64_lenient(&result.text).expect("base64");
        let json: serde_json::Value = serde_json::from_slice(&raw).expect("json");
        assert_eq!(json["connections"][0]["host"], "127.0.0.1");
        assert_eq!(json["connections"][0]["auth"], "s3cret");
        assert_eq!(json["groups"][0]["name"], "开发环境");

        // 再走一遍导入解析：格式按内容认出是 .ano
        let parsed = NcxFile::parse(&result.text).expect("round-trip parse");
        assert_eq!(parsed.format, ExportFormat::Ano);
        assert_eq!(parsed.connections.len(), 1);
        let c = &parsed.connections[0];
        assert_eq!(c.name(), "本地 Redis");
        assert_eq!(c.host(), "127.0.0.1");
        assert_eq!(c.port(), "6379");
        assert_eq!(c.group(), "开发环境");
        assert_eq!(c.password_state, PasswordState::Decrypted);
        assert_eq!(c.password, "s3cret", ".ano 的 auth 是明文");
        assert!(matches!(c.db_kind(), Some(DbKind::Redis)));
    }

    #[test]
    fn ano_parses_legacy_bare_array() {
        // 旧版导出是裸数组（没有 groups / connections 包装）
        let json = serde_json::json!([
            {"host": "10.0.0.5", "port": "9099", "name": "缓存 A", "auth": ""},
        ]);
        let text = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(json.to_string().as_bytes())
        };
        let parsed = NcxFile::parse(&text).expect("parse legacy .ano");
        assert_eq!(parsed.format, ExportFormat::Ano);
        assert_eq!(parsed.connections.len(), 1);
        let c = &parsed.connections[0];
        assert_eq!(c.name(), "缓存 A", "没有 connectionName 时回落到 name");
        assert_eq!(c.port(), "9099", "端口是字符串也照收");
        assert_eq!(c.password_state, PasswordState::NoPassword);
        assert_eq!(c.group(), "");
    }

    #[test]
    fn ano_marks_cluster_entries_unsupported() {
        let json = serde_json::json!({
            "connections": [
                {"host": "h", "port": 6379, "connectionName": "集群", "cluster": true, "groupId": "g1"},
            ],
            "groups": [{"id": "g1", "name": "生产"}],
        });
        let text = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(json.to_string().as_bytes())
        };
        let parsed = NcxFile::parse(&text).expect("parse");
        let c = &parsed.connections[0];
        assert!(c.cluster);
        assert_eq!(c.group(), "生产", "groupId → groups 里的名字");
        assert_eq!(
            c.unsupported_reason().as_deref(),
            Some("Redis 集群版暂不支持")
        );
    }

    #[test]
    fn parse_detects_format_by_content_not_suffix() {
        // XML 根 → .ncx
        let ncx = NcxFile::parse(
            r#"<?xml version="1.0"?><Connections Ver="1.5"><Connection ConnectionName="a" ConnType="MYSQL"/></Connections>"#,
        )
        .expect("parse ncx");
        assert_eq!(ncx.format, ExportFormat::Ncx);
        assert_eq!(ncx.format_version, "1.5");

        // 既不是 XML 也不是 base64 → 说清两种格式都认不出
        let err = NcxFile::parse("这不是任何一种导出文件").unwrap_err();
        assert!(err.contains(".ncx") && err.contains(".ano"), "{err}");
    }

    #[test]
    fn parse_unescapes_attribute_entities() {
        // 连接名里的 & 在文件里是 &amp;，解析后必须还原，
        // 否则导入向导和连接树会显示成「a&amp;b」。
        let ncx = NcxFile::parse(
            r#"<?xml version="1.0"?>
<Connections Ver="1.5">
  <Connection ConnectionName="a&amp;b" ConnType="MYSQL" Host="h" Port="3306" UserName="u"/>
</Connections>"#,
        )
        .expect("parse");
        assert_eq!(ncx.connections.len(), 1);
        assert_eq!(ncx.connections[0].name(), "a&b");
        assert!(matches!(
            ncx.connections[0].db_kind(),
            Some(DbKind::MySql)
        ));
    }
}
