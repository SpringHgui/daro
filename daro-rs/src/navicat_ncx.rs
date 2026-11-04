//! Navicat `.ncx` 导入/导出。
//!
//! 格式与 Flutter 版 `lib/data/navicat_*.dart` 完全对齐：
//! - XML 属性平铺，密码走 AES-128-CBC + PKCS#7（libcc 常量密钥）
//! - 分组用 daro 私有属性 `Group="..."`，Navicat 侧忽略
//! - 支持 MySQL / PostgreSQL / SQL Server；其余类型导出时跳过

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use crate::mock::DbKind;

/// Navicat 内置密码加密密钥（libcc 常量，16 字节 ASCII）
const LIBCC_KEY: &[u8; 16] = b"libcckeylibcckey";

/// Navicat 内置密码加密 IV（libcc 常量，末尾含一个空格，16 字节）
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
    /// Navicat 端未勾选「保存密码」，导出文件里没有密文
    NotSaved,
    /// 有密文但解不出来（旧版加密方式等），导入后需手填
    Undecryptable,
}

/// 解析自 Navicat 导出文件的一条连接
#[derive(Clone, Debug)]
pub struct NavicatConnection {
    /// `<Connection>` 元素上的全部属性
    pub attributes: std::collections::HashMap<String, String>,
    /// 密码解密结果状态
    pub password_state: PasswordState,
    /// 明文密码，仅当 `password_state == Decrypted` 时有意义
    pub password: String,
}

impl NavicatConnection {
    pub fn attr(&self, key: &str) -> &str {
        self.attributes.get(key).map(|s| s.as_str()).unwrap_or("")
    }

    /// 连接显示名
    pub fn name(&self) -> &str {
        self.attr("ConnectionName")
    }

    /// Navicat 原始类型（MYSQL / POSTGRESQL / SQLSERVER / SQLITE ...）
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
}

/// 一个导出文件的解析结果
#[derive(Clone, Debug)]
pub struct NavicatNcx {
    /// 全部连接（保持文件中的原始顺序）
    pub connections: Vec<NavicatConnection>,
    /// `<Connections Ver="1.5">` 里的导出格式版本
    pub format_version: String,
}

impl NavicatNcx {
    /// 解析 NCX（XML）文本。格式不符时返回 Err。
    pub fn parse(text: &str) -> Result<Self, String> {
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
                    let decoded = decrypt_navicat_password(&password_hex, saved);
                    connections.push(NavicatConnection {
                        attributes: attrs,
                        password_state: decoded.0,
                        password: decoded.1,
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

        Ok(NavicatNcx {
            connections,
            format_version,
        })
    }
}

/// AES-128-CBC 解密 + PKCS#7 去填充后的明文
pub fn decrypt_navicat_password(cipher_hex: &str, saved: bool) -> (PasswordState, String) {
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

/// 用 Navicat 的 libcc 常量加密明文口令，输出大写十六进制密文
pub fn encrypt_navicat_password(plain: &str) -> String {
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

/// Navicat 按 UTF-8 存密码字节，解出的字节必须能按 UTF-8 解码且不含控制字符
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
pub fn navicat_export_blocker(kind: DbKind) -> Option<&'static str> {
    match kind {
        DbKind::Redis => Some("Redis 在 Navicat 侧按 .ano 导出，.ncx 里没有它的属性模板"),
        _ => None,
    }
}

/// 一次导出的结果
#[derive(Clone, Debug)]
pub struct NavicatExportResult {
    /// 待写盘的 .ncx 文本（UTF-8，CRLF）
    pub xml: String,
    /// 实际写进文件的连接名
    pub exported: Vec<String>,
    /// 因 Navicat 无对应类型而跳过的连接（名称 + 原因）
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
pub fn build_navicat_ncx(
    connections: &[ExportConnection],
    include_passwords: bool,
    settings_save_root: &str,
) -> NavicatExportResult {
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
        if let Some(reason) = navicat_export_blocker(conn.kind) {
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
    NavicatExportResult {
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
        encrypt_navicat_password(&conn.password)
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
        let encrypted = encrypt_navicat_password(plain);
        let (state, decrypted) = decrypt_navicat_password(&encrypted, true);
        assert_eq!(state, PasswordState::Decrypted);
        assert_eq!(decrypted, plain);
    }

    #[test]
    fn empty_password() {
        let (state, decrypted) = decrypt_navicat_password("", true);
        assert_eq!(state, PasswordState::NoPassword);
        assert_eq!(decrypted, "");
    }

    #[test]
    fn not_saved_password() {
        let (state, decrypted) = decrypt_navicat_password("", false);
        assert_eq!(state, PasswordState::NotSaved);
        assert_eq!(decrypted, "");
    }

    #[test]
    fn known_cipher() {
        // Navicat 空密码的已知密文
        let (state, decrypted) =
            decrypt_navicat_password("E191AF42327478CC5F143EF279EC4D81", true);
        assert_eq!(state, PasswordState::Decrypted);
        assert_eq!(decrypted, "");
    }

    #[test]
    fn export_round_trips_name_group_and_password() {
        // 导出再解析回来：名称里的 & 与中文、分组私有属性、密码密文
        // 都必须原样往返（Flutter 版 navicat_export_test 的同款口径）。
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
        let result = build_navicat_ncx(&[conn], true, r"C:\Users\测试\Documents\Navicat");
        assert_eq!(result.exported, vec!["演示 & 连接".to_string()]);
        assert!(result.skipped.is_empty());

        let parsed = NavicatNcx::parse(&result.xml).expect("round-trip parse");
        assert_eq!(parsed.connections.len(), 1);
        let c = &parsed.connections[0];
        assert_eq!(c.name(), "演示 & 连接");
        assert_eq!(c.group(), "测试组");
        assert_eq!(c.host(), "db.example.com");
        assert_eq!(c.password_state, PasswordState::Decrypted);
        assert_eq!(c.password, "p@ss");
    }

    #[test]
    fn parse_unescapes_attribute_entities() {
        // 连接名里的 & 在文件里是 &amp;，解析后必须还原，
        // 否则导入向导和连接树会显示成「a&amp;b」。
        let ncx = NavicatNcx::parse(
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
