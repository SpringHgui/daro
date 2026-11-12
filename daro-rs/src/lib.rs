//! daro 的无界面半边：驱动层 + 后台 MCP 服务。
//!
//! 界面（`daro-rs` bin）与后台进程（`daro-mcpd` bin）共用这一份代码。这里只放
//! 不碰 gpui 的模块——驱动、目录缓存、连接分发、MCP 服务与进程管理；任何
//! `use gpui` 的文件都不许出现在这棵树的依赖链上，否则后台进程会把整个 UI
//! 框架链进去。

#![allow(dead_code)]

pub mod access_driver;
pub mod backend;
pub mod catalog;
pub mod daemon;
pub mod db_types;
pub mod mcp;
pub mod mock;
pub mod mongo_driver;
pub mod mysql_driver;
pub mod pg_driver;
pub mod redis_driver;
pub mod sqlite_driver;
pub mod sqlserver_driver;

/// 数据目录：`DARO_DATA_DIR` 优先（开发 / 验证用）；绿色版在 exe 同目录放一个
/// `daro-rs.portable` 标记，数据就落在旁边的 `daro-rs-data`；否则用用户本地数据目录。
pub fn data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("DARO_DATA_DIR") {
        return std::path::PathBuf::from(dir);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("daro-rs.portable").is_file() {
                return dir.join("daro-rs-data");
            }
        }
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("daro-rs")
}
