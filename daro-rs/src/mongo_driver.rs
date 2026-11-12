//! MongoDB 驱动（只读）：列库 / 列集合 / 分页读文档。
//!
//! 与 Flutter 版 `mongo_driver.dart` 同口径：
//! - 一条连接一个客户端，URI 里只带认证，不带默认库
//! - 集合当表看：列 = 本页文档键的并集，值按 BSON 展示
//! - 不支持 SQL 筛选，排序只接受单列；写回 / 查询页明确拒绝

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use mongodb::bson::{Bson, Document, doc};
use mongodb::results::CollectionType;
use mongodb::sync::Client;

use crate::db_types::{ColMeta, SelectPage};

/// 一次取回的文档数上限。
pub const QUERY_ROW_LIMIT: usize = 5000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Params {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

static CLIENTS: LazyLock<Mutex<HashMap<String, Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn percent(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn uri(p: &Params) -> String {
    let host = p.host.trim();
    let port = if p.port == 0 { 27017 } else { p.port };
    let auth = if p.user.trim().is_empty() {
        String::new()
    } else if p.password.is_empty() {
        format!("{}@", percent(p.user.trim()))
    } else {
        format!("{}:{}@", percent(p.user.trim()), percent(&p.password))
    };
    format!("mongodb://{auth}{host}:{port}/?authSource=admin")
}

fn connect(p: &Params) -> Result<Client, String> {
    if p.host.trim().is_empty() {
        return Err("MongoDB 连接缺少主机地址".to_string());
    }
    Client::with_uri_str(uri(p)).map_err(|e| format!("MongoDB 连接失败：{e}"))
}

pub fn open(conn: &str, p: &Params) -> Result<(), String> {
    let client = connect(p)?;
    // 建连是惰性的；跑一条 ping 才能确认服务端可达
    ping(&client)?;
    CLIENTS.lock().unwrap().insert(conn.to_string(), client);
    Ok(())
}

pub fn is_open(conn: &str) -> bool {
    CLIENTS.lock().unwrap().contains_key(conn)
}

pub fn ensure(conn: &str, p: &Params) -> Result<(), String> {
    if is_open(conn) {
        return Ok(());
    }
    open(conn, p)
}

pub fn close(conn: &str) {
    CLIENTS.lock().unwrap().remove(conn);
}

pub fn probe(p: &Params) -> Result<(), String> {
    ping(&connect(p)?)
}

fn ping(client: &Client) -> Result<(), String> {
    client
        .database("admin")
        .run_command(doc! { "ping": 1 })
        .run()
        .map(|_| ())
        .map_err(|e| format!("MongoDB 连接失败：{e}"))
}

fn client(conn: &str, p: &Params) -> Result<Client, String> {
    ensure(conn, p)?;
    CLIENTS
        .lock()
        .unwrap()
        .get(conn)
        .cloned()
        .ok_or_else(|| "MongoDB 连接已关闭".to_string())
}

pub fn list_databases(conn: &str, p: &Params) -> Result<Vec<String>, String> {
    let client = client(conn, p)?;
    let mut names = client
        .list_database_names()
        .run()
        .map_err(|e| format!("列出 MongoDB 数据库失败：{e}"))?;
    names.sort();
    Ok(names)
}

/// 一个库里的集合：表与视图分开，系统库 / system.* 不列。
pub fn list_collections(
    conn: &str,
    p: &Params,
    db: &str,
    want_views: bool,
) -> Result<Vec<String>, String> {
    let client = client(conn, p)?;
    let database = client.database(db);
    let mut out = Vec::new();
    let cursor = database
        .list_collections()
        .run()
        .map_err(|e| format!("列出 MongoDB 集合失败：{e}"))?;
    for spec in cursor {
        let spec = spec.map_err(|e| format!("列出 MongoDB 集合失败：{e}"))?;
        if spec.name.starts_with("system.") {
            continue;
        }
        let is_view = matches!(spec.collection_type, CollectionType::View);
        if is_view == want_views {
            out.push(spec.name);
        }
    }
    out.sort();
    Ok(out)
}

pub fn count_docs(conn: &str, p: &Params, db: &str, coll: &str) -> Result<usize, String> {
    let client = client(conn, p)?;
    let c = client.database(db).collection::<Document>(coll);
    c.count_documents(doc! {})
        .run()
        .map(|n| n as usize)
        .map_err(|e| format!("统计 MongoDB 文档数失败：{e}"))
}

/// 集合的字段（取一条样本文档推断）：树上展开字段用；空集合返回空清单。
pub fn sample_columns(
    conn: &str,
    p: &Params,
    db: &str,
    coll: &str,
) -> Result<Vec<ColMeta>, String> {
    let client = client(conn, p)?;
    let c = client.database(db).collection::<Document>(coll);
    let doc = c
        .find_one(doc! {})
        .run()
        .map_err(|e| format!("读取 MongoDB 文档失败：{e}"))?;
    let Some(doc) = doc else {
        return Ok(Vec::new());
    };
    let numeric = |v: &Bson| {
        matches!(
            v,
            Bson::Double(_) | Bson::Int32(_) | Bson::Int64(_) | Bson::Decimal128(_)
        )
    };
    let mut cols: Vec<ColMeta> = Vec::new();
    if let Some(v) = doc.get("_id") {
        let mut m = ColMeta::new("_id", bson_type_name(v), numeric(v));
        m.writable = false;
        cols.push(m);
    }
    for (k, v) in doc.iter() {
        if k == "_id" {
            continue;
        }
        let mut m = ColMeta::new(k, bson_type_name(v), numeric(v));
        m.writable = false;
        cols.push(m);
    }
    Ok(cols)
}

/// 分页读文档：`orders` 只接受一列（与 Flutter 版同口径）。
pub fn select_docs(
    conn: &str,
    p: &Params,
    db: &str,
    coll: &str,
    where_sql: Option<&str>,
    orders: &[(String, bool)],
    offset: usize,
    limit: usize,
) -> Result<SelectPage, String> {
    if where_sql.is_some_and(|w| !w.trim().is_empty()) {
        return Err("MongoDB 集合不支持 SQL 筛选条件".to_string());
    }
    if orders.len() > 1 {
        return Err("MongoDB 集合最多按一列排序".to_string());
    }
    let client = client(conn, p)?;
    let c = client.database(db).collection::<Document>(coll);
    let mut find = c.find(doc! {});
    if let Some((col, asc)) = orders.first() {
        find = find.sort(doc! { col.as_str(): if *asc { 1 } else { -1 } });
    }
    let cursor = find
        .skip(offset as u64)
        .limit(limit as i64)
        .run()
        .map_err(|e| format!("读取 MongoDB 文档失败：{e}"))?;
    let mut docs = Vec::new();
    let mut truncated = false;
    for doc in cursor {
        if docs.len() >= limit {
            truncated = true;
            break;
        }
        docs.push(doc.map_err(|e| format!("读取 MongoDB 文档失败：{e}"))?);
    }
    Ok(docs_to_page(&docs, truncated))
}

/// 文档 → 网格：列取本页文档键的并集（`_id` 优先），缺键 / null 都打 `NULL`。
fn docs_to_page(docs: &[Document], truncated: bool) -> SelectPage {
    let mut names: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let push = |name: &str, names: &mut Vec<String>, seen: &mut std::collections::HashSet<String>| {
        if seen.insert(name.to_string()) {
            names.push(name.to_string());
        }
    };
    for d in docs {
        if d.contains_key("_id") {
            push("_id", &mut names, &mut seen);
            break;
        }
    }
    for d in docs {
        for k in d.keys() {
            push(k, &mut names, &mut seen);
        }
    }
    let columns: Vec<ColMeta> = names
        .iter()
        .map(|name| {
            let ty = docs
                .iter()
                .find_map(|d| d.get(name))
                .map(|v| {
                    let n = bson_type_name(v);
                    let numeric = matches!(
                        v,
                        Bson::Int32(_) | Bson::Int64(_) | Bson::Double(_) | Bson::Decimal128(_)
                    );
                    (n, numeric)
                })
                .unwrap_or(("NULL".to_string(), false));
            let mut m = ColMeta::new(name, ty.0, ty.1);
            m.writable = false;
            m
        })
        .collect();
    let rows: Vec<Vec<String>> = docs
        .iter()
        .map(|d| {
            names
                .iter()
                .map(|n| d.get(n).map(bson_text).unwrap_or_else(|| "NULL".to_string()))
                .collect()
        })
        .collect();
    SelectPage {
        columns,
        rows,
        keys: Vec::new(),
        truncated,
    }
}

pub fn page_sql(
    _db: &str,
    coll: &str,
    orders: &[(String, bool)],
    limit: usize,
    offset: usize,
) -> String {
    let sort = orders
        .first()
        .map(|(c, asc)| format!("{{{}: {}}}", json_str(c), if *asc { 1 } else { -1 }))
        .unwrap_or_else(|| "{}".to_string());
    format!(
        "db.getCollection({}).find({{}}).sort({sort}).skip({offset}).limit({limit})",
        json_str(coll)
    )
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn bson_type_name(v: &Bson) -> String {
    match v {
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::Document(_) => "object",
        Bson::Array(_) => "array",
        Bson::Binary(_) => "binData",
        Bson::ObjectId(_) => "objectId",
        Bson::Boolean(_) => "bool",
        Bson::DateTime(_) => "date",
        Bson::Null => "null",
        Bson::RegularExpression(_) => "regex",
        Bson::JavaScriptCode(_) | Bson::JavaScriptCodeWithScope(_) => "javascript",
        Bson::Int32(_) => "int",
        Bson::Int64(_) => "long",
        Bson::Timestamp(_) => "timestamp",
        Bson::Decimal128(_) => "decimal",
        Bson::Symbol(_) => "symbol",
        Bson::Undefined => "undefined",
        Bson::MinKey => "minKey",
        Bson::MaxKey => "maxKey",
        Bson::DbPointer(_) => "dbPointer",
    }
    .to_string()
}

/// 单元格文本：与 Flutter 版 `cell` 的口径一致（ObjectId 十六进制、
/// 日期 ISO、文档 / 数组紧凑 JSON、二进制带长度和前缀）。
pub fn bson_text(v: &Bson) -> String {
    match v {
        Bson::Null | Bson::Undefined => "NULL".to_string(),
        Bson::String(s) => s.clone(),
        Bson::Int32(i) => i.to_string(),
        Bson::Int64(i) => i.to_string(),
        Bson::Double(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{f:.0}")
            } else {
                f.to_string()
            }
        }
        Bson::Boolean(b) => b.to_string(),
        Bson::ObjectId(oid) => oid.to_hex(),
        Bson::DateTime(dt) => dt.try_to_rfc3339_string().unwrap_or_else(|_| dt.to_string()),
        Bson::Binary(b) => {
            let hex: String = b.bytes.iter().take(32).map(|x| format!("{x:02X}")).collect();
            format!("{}B {hex}{}", b.bytes.len(), if b.bytes.len() > 32 { "…" } else { "" })
        }
        Bson::Array(_) | Bson::Document(_) => v.clone().into_relaxed_extjson().to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bson_text_renders_scalars_and_documents() {
        assert_eq!(bson_text(&Bson::Null), "NULL");
        assert_eq!(bson_text(&Bson::Int64(7)), "7");
        assert_eq!(bson_text(&Bson::Double(2.5)), "2.5");
        assert!(bson_text(&Bson::Document(doc! { "a": 1 })).contains("\"a\""));
    }

    #[test]
    fn page_sql_uses_first_sort_and_skip_limit() {
        let sql = page_sql("db", "items", &[("score".into(), false)], 20, 40);
        assert!(sql.contains(".sort({\"score\": -1})"), "{sql}");
        assert!(sql.contains(".skip(40).limit(20)"), "{sql}");
        assert!(page_sql("db", "items", &[], 10, 0).contains(".sort({})"));
    }
}
