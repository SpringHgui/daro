//! 查询历史：自动记录已执行的 SQL（查询页 / 设计器保存 / 工具栏执行）。
//!
//! 与 `connections.json` 同目录落盘 `history.json`。文件不存在 / 损坏按空列表
//! 处理，保存为全量覆写。内存只保留最近 [`MAX_ENTRIES`] 条（落盘前截断）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 单条查询历史记录。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u64,
    /// Unix 秒。
    pub ts: i64,
    pub conn: String,
    pub db: String,
    pub schema: String,
    pub sql: String,
    pub ok: bool,
    pub rows: usize,
    pub ms: u64,
}

/// 内存与磁盘保留上限（最近 N 条）。
pub const MAX_ENTRIES: usize = 5000;

fn history_path() -> PathBuf {
    daro_rs::data_dir().join("history.json")
}

/// 当前 Unix 秒。
pub fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 读取全部历史；文件不存在或损坏时返回空列表。
pub fn load() -> Vec<HistoryEntry> {
    let data = match std::fs::read_to_string(history_path()) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_str::<HistoryFile>(&data) {
        Ok(file) => file.entries,
        Err(e) => {
            eprintln!("[history] history.json 损坏，按空列表处理：{e}");
            Vec::new()
        }
    }
}

/// 全量覆写保存（调用方负责截断到 [`MAX_ENTRIES`]）。
pub fn save(entries: &[HistoryEntry]) {
    let file = HistoryFile {
        entries: entries.to_vec(),
    };
    match serde_json::to_string_pretty(&file) {
        Ok(json) => {
            if let Err(e) = std::fs::write(history_path(), json) {
                eprintln!("[history] 写入 history.json 失败：{e}");
            }
        }
        Err(e) => eprintln!("[history] 序列化失败：{e}"),
    }
}

#[derive(Serialize, Deserialize)]
struct HistoryFile {
    entries: Vec<HistoryEntry>,
}
