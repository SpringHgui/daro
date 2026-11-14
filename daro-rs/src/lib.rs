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
// 导出编码是纯数据层（不碰 gpui）：驱动测试里要核对导出的 INSERT 方言。
#[cfg(test)]
pub mod export_format;
#[cfg(test)]
pub mod table_export;
pub mod mcp;
pub mod mock;
pub mod mongo_driver;
pub mod mysql_driver;
pub mod pg_driver;
pub mod redis_driver;
pub mod sqlite_driver;
pub mod sqlserver_driver;

thread_local! {
    /// 当前线程私有的数据目录。测试并行跑时每个用例各占一个线程，用线程
    /// 局部覆盖才不会像进程级环境变量那样互相踩存档。
    static THREAD_DATA_DIR: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// 只给当前线程换数据目录（测试隔离用）；传 `None` 恢复默认解析。
#[doc(hidden)]
pub fn set_thread_data_dir(dir: Option<std::path::PathBuf>) {
    THREAD_DATA_DIR.with(|slot| *slot.borrow_mut() = dir);
}

/// 数据目录：线程覆盖优先（测试隔离）；其次 `DARO_DATA_DIR`（开发 / 验证用）；
/// 绿色版在 exe 同目录放一个 `daro-rs.portable` 标记，数据就落在旁边的
/// `daro-rs-data`；否则用用户本地数据目录。
pub fn data_dir() -> std::path::PathBuf {
    if let Some(dir) = THREAD_DATA_DIR.with(|slot| slot.borrow().clone()) {
        return dir;
    }
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
