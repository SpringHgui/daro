//! 已保存查询（迁移自 Flutter 版 `lib/data/saved_query_store.dart`）。
//!
//! 与连接同目录落盘：`queries.json`（连接用 `connections.json`）。文件结构：
//! `{ "queries": [ { "name", "connection", "database", "schema", "sql" }, ... ] }`。
//! 文件不存在 / 损坏按空列表处理（不阻止应用启动），保存为全量覆写。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 一条已保存的查询：归属某连接 / 库 / 模式，保存一份 SQL 文本；
/// 双击可在查询编辑器里重新打开（带原上下文）。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SavedQuery {
    pub name: String,
    pub connection: String,
    pub database: String,
    /// 模式名（无模式层引擎为空串）
    pub schema: String,
    pub sql: String,
}

impl SavedQuery {
    /// 同一归属下的业务键（连接|库|模式|名称）；同名同归属视为同一条查询。
    pub fn key(&self) -> String {
        format!("{}|{}|{}|{}", self.connection, self.database, self.schema, self.name)
    }
}

/// 落盘路径：与 `connections.json` 同目录（受 `DARO_DATA_DIR` 覆盖）。
fn queries_path() -> PathBuf {
    daro_rs::data_dir().join("queries.json")
}

/// 读取全部已保存查询；文件不存在或损坏时返回空列表。
pub fn load() -> Vec<SavedQuery> {
    let data = match std::fs::read_to_string(queries_path()) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_str::<QueriesFile>(&data) {
        Ok(file) => file.queries,
        Err(e) => {
            eprintln!("[saved_queries] queries.json 损坏，按空列表处理：{e}");
            Vec::new()
        }
    }
}

/// 全量覆写保存。
pub fn save(queries: &[SavedQuery]) {
    let file = QueriesFile {
        queries: queries.to_vec(),
    };
    match serde_json::to_string_pretty(&file) {
        Ok(json) => {
            if let Err(e) = std::fs::write(queries_path(), json) {
                eprintln!("[saved_queries] 写入 queries.json 失败：{e}");
            }
        }
        Err(e) => eprintln!("[saved_queries] 序列化失败：{e}"),
    }
}

#[derive(Serialize, Deserialize)]
struct QueriesFile {
    queries: Vec<SavedQuery>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_uses_context_and_name() {
        let q = SavedQuery {
            name: "q1".into(),
            connection: "local".into(),
            database: "db1".into(),
            schema: "".into(),
            sql: "select 1".into(),
        };
        assert_eq!(q.key(), "local|db1||q1");
    }

    #[test]
    fn round_trips_through_json() {
        let qs = vec![
            SavedQuery {
                name: "a".into(),
                connection: "c".into(),
                database: "d".into(),
                schema: "s".into(),
                sql: "select 1".into(),
            },
            SavedQuery {
                name: "b".into(),
                connection: "c".into(),
                database: "d".into(),
                schema: "".into(),
                sql: "select 2".into(),
            },
        ];
        let json = serde_json::to_string(&QueriesFile { queries: qs.clone() }).unwrap();
        let back: QueriesFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.queries, qs);
    }
}
