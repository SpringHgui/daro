//! MCP 眼里的连接清单：每次工具调用都从 `connections.json` 现读，
//! 并按参数指纹关掉改过凭据的旧句柄。
//!
//! 后台进程和界面是两个进程：用户在界面里改完连接，MCP 这边下一句查询就得用新
//! 参数。驱动层按连接名缓存句柄，`ensure()` 不会发现「同名但换了密码/主机」，
//! 所以这里比对指纹，变了先把旧句柄关掉再重连。

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use crate::backend;
use crate::mock::{Connection, DbKind};

#[derive(serde::Deserialize)]
struct StoreFile {
    #[serde(default)]
    connections: Vec<Connection>,
}

/// 数据目录：规则见 [`crate::data_dir`]（环境变量 / 绿色版标记 / 用户本地目录）。
pub fn data_dir() -> Result<std::path::PathBuf, String> {
    Ok(crate::data_dir())
}

pub fn connections_path() -> Result<std::path::PathBuf, String> {
    Ok(data_dir()?.join("connections.json"))
}

/// 读全部已存连接。文件不存在算「还没有连接」，不当错误。
pub fn load_connections() -> Result<Vec<Connection>, String> {
    let path = connections_path()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("读 {} 失败: {e}", path.display())),
    };
    let file: StoreFile =
        serde_json::from_str(&text).map_err(|e| format!("解析 {} 失败: {e}", path.display()))?;
    Ok(file.connections)
}

/// 按名字找连接：先精确，再忽略大小写。找不到时和「未授权」合并成同一条消息，
/// 不给 agent 探测连接名是否存在的机会。
pub fn find_connection(name: &str) -> Result<Connection, String> {
    let all = load_connections()?;
    let want = name.trim();
    all.iter()
        .find(|c| c.name == want)
        .or_else(|| all.iter().find(|c| c.name.eq_ignore_ascii_case(want)))
        .cloned()
        .ok_or_else(|| format!("连接 {want:?} 不存在或未授权给 MCP（见 daro_list_connections）"))
}

/// 连接参数指纹。密码只进哈希，不在字符串里留副本。
fn fingerprint(conn: &Connection) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    conn.password.hash(&mut h);
    format!(
        "{}|{}|{}|{}|{}|{}|{:x}|{}",
        conn.name,
        conn.kind.label(),
        conn.host.trim(),
        conn.port,
        conn.database.trim(),
        conn.user,
        h.finish(),
        conn.demo
    )
}

static SEEN: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const ALL_KINDS: [DbKind; 6] = [
    DbKind::SQLite,
    DbKind::PostgreSql,
    DbKind::MySql,
    DbKind::SqlServer,
    DbKind::MongoDb,
    DbKind::Redis,
];

/// 工具调用前同步一次连接状态：参数变了的、从清单里删掉的，先关旧句柄。
/// 返回的句柄会在下一次 `ensure()` 时按新参数重建。
pub fn sync_handles(all: &[Connection]) {
    let mut seen = SEEN.lock().unwrap();
    let alive: HashSet<&str> = all.iter().map(|c| c.name.as_str()).collect();
    let gone: Vec<String> = seen
        .keys()
        .filter(|name| !alive.contains(name.as_str()))
        .cloned()
        .collect();
    for name in gone {
        // 引擎未知：按全部引擎各关一次，关不到的是空操作
        for kind in ALL_KINDS {
            backend::close_named(&name, kind);
        }
        seen.remove(&name);
    }
    for conn in all {
        let fp = fingerprint(conn);
        if seen.get(&conn.name) == Some(&fp) {
            continue;
        }
        // 第一次见到也一样：界面那边可能已经用过这个连接名，先关一次保证参数干净
        backend::close_named(&conn.name, conn.kind);
        seen.insert(conn.name.clone(), fp);
    }
}

/// 当前 Unix 秒
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}
