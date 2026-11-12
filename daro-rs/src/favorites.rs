//! 收藏夹：把当前页签记成 1–9 号快速入口，落盘 `favorites.json`
//! （与 `connections.json` / `queries.json` 同目录，受 `DARO_DATA_DIR` 覆盖）。
//!
//! 参考界面的收藏夹固定 9 个槽位（Ctrl+1…Ctrl+9），本模块按槽位存
//! `Vec<Option<Favorite>>`：空槽序列化成 `null`，读取时补齐到 9 个。
//! 目标用名字而不是下标（连接增删会让下标漂移），打开时再回查目录。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 收藏槽位数：与 Ctrl+1…Ctrl+9 对齐。
pub const SLOTS: usize = 9;

/// 一条收藏指向的页签身份。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FavoriteTarget {
    /// 表 / 视图 / 实体化视图 / 函数的页签（数据网格或设计器）。
    Object {
        connection: String,
        database: String,
        schema: String,
        category: usize,
        /// 目录里的对象名（字段不叫 `name`：外层 Favorite 已有 `name`，
        /// `#[serde(flatten)]` 下会撞成 duplicate field）。
        object: String,
        design: bool,
    },
    /// 查询页签：连库上下文 + 编辑器里的 SQL 全文。
    Query {
        connection: String,
        database: String,
        schema: String,
        sql: String,
    },
    /// 角色设计页签。
    Role { connection: String, role: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Favorite {
    pub name: String,
    #[serde(flatten)]
    pub target: FavoriteTarget,
}

/// 落盘路径：与 `connections.json` 同目录。
fn favorites_path() -> PathBuf {
    daro_rs::data_dir().join("favorites.json")
}

#[derive(Serialize, Deserialize)]
struct FavoritesFile {
    version: u32,
    slots: Vec<Option<Favorite>>,
}

/// 读取 9 个槽位；文件不存在 / 损坏按全空处理，多出的槽位截断。
pub fn load() -> Vec<Option<Favorite>> {
    let mut slots = match std::fs::read_to_string(favorites_path()) {
        Ok(data) => match serde_json::from_str::<FavoritesFile>(&data) {
            Ok(file) => file.slots,
            Err(e) => {
                eprintln!("[favorites] favorites.json 损坏，按空收藏夹处理：{e}");
                Vec::new()
            }
        },
        Err(_) => Vec::new(),
    };
    slots.truncate(SLOTS);
    slots.resize(SLOTS, None);
    slots
}

/// 全量覆写保存（不足 9 个槽位补 `null`）。
pub fn save(slots: &[Option<Favorite>]) {
    let mut slots = slots.to_vec();
    slots.truncate(SLOTS);
    slots.resize(SLOTS, None);
    let file = FavoritesFile { version: 1, slots };
    match serde_json::to_string_pretty(&file) {
        Ok(json) => {
            if let Err(e) = std::fs::write(favorites_path(), json) {
                eprintln!("[favorites] 写入 favorites.json 失败：{e}");
            }
        }
        Err(e) => eprintln!("[favorites] 序列化失败：{e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_pads_to_nine_slots() {
        // 私有路径在测试里不直接改，这里只验序列化结构：空槽位往返后仍是 9 个。
        let slots: Vec<Option<Favorite>> = (0..SLOTS).map(|_| None).collect();
        let json = serde_json::to_string(&FavoritesFile {
            version: 1,
            slots: slots.clone(),
        })
        .unwrap();
        let back: FavoritesFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.slots.len(), SLOTS);
    }

    #[test]
    fn round_trips_all_target_kinds() {
        let slots = vec![
            Some(Favorite {
                name: "会员表".into(),
                target: FavoriteTarget::Object {
                    connection: "本地 MySQL".into(),
                    database: "app".into(),
                    schema: String::new(),
                    category: 0,
                    object: "member".into(),
                    design: false,
                },
            }),
            None,
            Some(Favorite {
                name: "日报".into(),
                target: FavoriteTarget::Query {
                    connection: "本地 MySQL".into(),
                    database: "app".into(),
                    schema: String::new(),
                    sql: "select * from member".into(),
                },
            }),
            Some(Favorite {
                name: "admin".into(),
                target: FavoriteTarget::Role {
                    connection: "本地 PG".into(),
                    role: "admin".into(),
                },
            }),
        ];
        let file = FavoritesFile {
            version: 1,
            slots: slots.clone(),
        };
        let json = serde_json::to_string(&file).unwrap();
        let back: FavoritesFile = serde_json::from_str(&json).unwrap();
        assert_eq!(&back.slots[..4], &slots[..]);
        assert!(back.slots[1].is_none(), "空槽位写成 null 并能读回");
    }
}
