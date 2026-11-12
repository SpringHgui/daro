//! Access（Jet / ACE）只读驱动：纯 Rust 解析 `.mdb` / `.accdb` 页面。
//!
//! 不装 ACE / ODBC：`jetdb` 直接读文件页，目录、列元数据、索引和整表数据都
//! 能在任何平台拿到；代价是没有 SQL、没有写回，所以这一档的连接恒为只读。
//!
//! 取数按「整表物化」交给界面：Access 是本地文件，全表扫描在几十万行内都是
//! 毫秒级，页签用本地筛选 / 排序（服务端没有可下推的 WHERE / ORDER BY）。
//! 连接按名字注册进进程级表，`PageReader` 是 `&mut` 独占读，一次只服务一条
//! 取数；目录读取也共用它，所以所有访问都从同一把连接锁过。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

use jetdb::{
    calculated_column_types, find_table, index_flags, index_type, read_catalog, read_table_def,
    read_table_rows, table_names, CatalogEntry, ColumnDef, ColumnType, FileError, ObjectType,
    PageReader, TableDef, TableRows, Value,
};

use crate::db_types::{ColMeta, SelectPage};

/// 与其它驱动同口径的单次取数上限（本档不执行 SQL，保留给接口对齐）。
pub const QUERY_ROW_LIMIT: usize = 5000;

struct DbFile {
    path: PathBuf,
    reader: PageReader,
    /// 用户表（`table_names` 已排除系统表 / 隐藏表和链接表）。
    tables: Vec<String>,
    catalog: Vec<CatalogEntry>,
    /// 表结构按需解析后缓存：列 / 索引都在 TDEF 页里，重复读文件没必要。
    defs: HashMap<String, TableDef>,
}

static CONNS: LazyLock<Mutex<HashMap<String, Arc<Mutex<DbFile>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn open_error(e: FileError) -> String {
    match e {
        FileError::PasswordRequired => "该数据库已加密，请在连接里填写密码".to_string(),
        FileError::InvalidPassword => "数据库密码不正确".to_string(),
        other => format!("无法打开 Access 数据库：{other}"),
    }
}

fn handle(key: &str) -> Result<Arc<Mutex<DbFile>>, String> {
    CONNS
        .lock()
        .unwrap()
        .get(key)
        .cloned()
        .ok_or_else(|| format!("连接「{key}」未打开"))
}

fn with_db<T>(key: &str, f: impl FnOnce(&mut DbFile) -> Result<T, String>) -> Result<T, String> {
    let db = handle(key)?;
    let mut db = db.lock().unwrap();
    f(&mut db)
}

impl DbFile {
    fn load(path: &str, password: Option<&str>) -> Result<Self, String> {
        let path = PathBuf::from(path.trim());
        if path.as_os_str().is_empty() {
            return Err("连接没有数据库文件路径".to_string());
        }
        if !path.is_file() {
            return Err(format!("找不到 Access 数据库文件：{}", path.display()));
        }
        let mut reader = PageReader::open_with_password(&path, password).map_err(open_error)?;
        let catalog =
            read_catalog(&mut reader).map_err(|e| format!("读取 Access 目录失败：{e}"))?;
        let tables = table_names(&mut reader).map_err(|e| format!("读取 Access 表清单失败：{e}"))?;
        Ok(Self {
            path,
            reader,
            tables,
            catalog,
            defs: HashMap::new(),
        })
    }

    /// 取表结构（首次读 TDEF 并缓存）。
    fn table_def(&mut self, table: &str) -> Result<&TableDef, String> {
        if !self.defs.contains_key(table) {
            let entry = find_table(&self.catalog, table)
                .ok()
                .filter(|e| e.object_type == ObjectType::Table)
                .ok_or_else(|| format!("表「{table}」不存在"))?;
            let def = read_table_def(&mut self.reader, &entry.name, entry.table_page)
                .map_err(|e| format!("读取表「{table}」结构失败：{e}"))?;
            self.defs.insert(table.to_string(), def);
        }
        Ok(self.defs.get(table).unwrap())
    }
}

/// 打开（或重开）一个 Access 库文件；密码只对加密库生效。
pub fn open(key: &str, path: &str, password: Option<&str>) -> Result<(), String> {
    let db = DbFile::load(path, password)?;
    CONNS
        .lock()
        .unwrap()
        .insert(key.to_string(), Arc::new(Mutex::new(db)));
    Ok(())
}

/// 连接是否已在注册表里。UI 惰性建连：树展开 / 打表前先 ensure。
pub fn is_open(key: &str) -> bool {
    CONNS.lock().unwrap().contains_key(key)
}

/// 关一条连接：只摘句柄，正在跑的取数持有 `Arc`，跑完自行释放。
pub fn close(key: &str) {
    let removed = CONNS.lock().unwrap().remove(key);
    drop(removed);
}

/// 惰性建连：没开过就开。
pub fn ensure(key: &str, path: &str, password: Option<&str>) -> Result<(), String> {
    if is_open(key) {
        return Ok(());
    }
    open(key, path, password)
}

/// 「测试连接」：只读打开并读一遍目录，不注册进连接表。
pub fn probe(path: &str, password: Option<&str>) -> Result<(), String> {
    let mut db = DbFile::load(path, password)?;
    table_names(&mut db.reader).map_err(|e| format!("读取 Access 表清单失败：{e}"))?;
    Ok(())
}

// ── 目录清单 ─────────────────────────────────────────────────────

/// 用户表清单（名称顺序沿用文件里的目录顺序）。
pub fn try_list_tables(key: &str) -> Result<Vec<String>, String> {
    with_db(key, |db| Ok(db.tables.clone()))
}

/// 某表的列元数据。隐藏列（jetdb 口径）不展示；顺序按设计期字段位置。
pub fn try_table_columns(key: &str, table: &str) -> Result<Vec<ColMeta>, String> {
    with_db(key, |db| {
        let def = db.table_def(table)?;
        Ok(layout(def).cols)
    })
}

/// 库里「表 → 列」，查询编辑器补全用。
pub fn all_columns_by_table(key: &str) -> Vec<(String, Vec<String>)> {
    with_db(key, |db| {
        let tables = db.tables.clone();
        Ok(tables
            .into_iter()
            .map(|t| {
                let cols = db
                    .table_def(&t)
                    .ok()
                    .map(|def| layout(def).cols.into_iter().map(|c| c.name).collect())
                    .unwrap_or_default();
                (t, cols)
            })
            .collect())
    })
    .unwrap_or_default()
}

/// 表的索引 / 唯一键名（类型 2 是外键引用，不列进索引组）。
pub fn table_parts(key: &str, table: &str) -> Result<(Vec<String>, Vec<String>), String> {
    with_db(key, |db| {
        let def = db.table_def(table)?;
        let mut indexes = Vec::new();
        let mut uniques = Vec::new();
        for ix in &def.indexes {
            if ix.index_type == index_type::FOREIGN_KEY || ix.name.is_empty() {
                continue;
            }
            indexes.push(ix.name.clone());
            if ix.flags & index_flags::UNIQUE != 0 {
                uniques.push(ix.name.clone());
            }
        }
        Ok((indexes, uniques))
    })
}

// ── 行取数 ───────────────────────────────────────────────────────

/// 整表数据（界面的本地物化页签 / 导出）。
pub fn select_all(key: &str, table: &str) -> Result<SelectPage, String> {
    with_db(key, |db| {
        let def = db.table_def(table)?.clone();
        let lay = layout(&def);
        let read = read_table_rows(&mut db.reader, &def)
            .map_err(|e| format!("读取表「{table}」数据失败：{e}"))?;
        Ok(page_of(&lay, read.rows))
    })
}

/// 按偏移取一页（接口对齐用；界面整表物化时不走这里）。
pub fn select_page(
    key: &str,
    table: &str,
    offset: usize,
    limit: usize,
) -> Result<SelectPage, String> {
    with_db(key, |db| {
        let def = db.table_def(table)?.clone();
        let lay = layout(&def);
        let calculated = calculated_column_types(&mut db.reader, &def);
        let mut rows = TableRows::new(&def, &calculated);
        let mut skipped = 0usize;
        while skipped < offset {
            if rows
                .next_row(&mut db.reader)
                .map_err(|e| format!("读取表「{table}」数据失败：{e}"))?
                .is_none()
            {
                return Ok(page_of(&lay, Vec::new()));
            }
            skipped += 1;
        }
        let mut values = Vec::new();
        while values.len() < limit {
            match rows
                .next_row(&mut db.reader)
                .map_err(|e| format!("读取表「{table}」数据失败：{e}"))?
            {
                Some(row) => values.push(row.values),
                None => break,
            }
        }
        Ok(page_of(&lay, values))
    })
}

/// 表的总行数（表头里存的值，不用扫表）。
pub fn count_rows(key: &str, table: &str) -> Result<usize, String> {
    with_db(key, |db| Ok(db.table_def(table)?.num_rows as usize))
}

// ── 结构换算 ─────────────────────────────────────────────────────

struct Layout {
    cols: Vec<ColMeta>,
    /// 显示列在 TableRow::values 里的下标（隐藏列已剔除）。
    val_idx: Vec<usize>,
    /// 主键列在显示列里的下标。
    pk_disp: Vec<usize>,
}

fn layout(def: &TableDef) -> Layout {
    let pk_cols: Vec<u16> = def
        .indexes
        .iter()
        .filter(|ix| ix.index_type == index_type::PRIMARY)
        .flat_map(|ix| ix.columns.iter().map(|c| c.col_num))
        .collect();
    let mut shown: Vec<(usize, &ColumnDef)> = def
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.is_hidden())
        .collect();
    // display_index 才是 Access 设计器里的字段顺序；col_num 只是创建顺序。
    shown.sort_by_key(|(_, c)| (c.display_index, c.col_num));

    let mut cols = Vec::with_capacity(shown.len());
    let mut val_idx = Vec::with_capacity(shown.len());
    let mut pk_disp = Vec::new();
    for (disp, (raw, col)) in shown.into_iter().enumerate() {
        let mut meta = ColMeta::new(
            col.name.clone(),
            type_label(col.col_type),
            type_is_numeric(col.col_type),
        );
        meta.writable = false;
        if pk_cols.contains(&col.col_num) {
            meta.pk = true;
            pk_disp.push(disp);
        }
        cols.push(meta);
        val_idx.push(raw);
    }
    Layout {
        cols,
        val_idx,
        pk_disp,
    }
}

fn type_label(ty: ColumnType) -> String {
    match ty {
        ColumnType::Boolean => "Yes/No",
        ColumnType::Byte => "Byte",
        ColumnType::Int => "Integer",
        ColumnType::Long => "Long Integer",
        ColumnType::Money => "Currency",
        ColumnType::Float => "Single",
        ColumnType::Double => "Double",
        ColumnType::Timestamp => "Date/Time",
        ColumnType::Binary => "Binary",
        ColumnType::Text => "Text",
        ColumnType::Ole => "OLE Object",
        ColumnType::Memo => "Memo",
        ColumnType::Guid => "GUID",
        ColumnType::Numeric => "Decimal",
        ColumnType::ComplexType => "Complex",
        ColumnType::BigInt => "Big Integer",
        ColumnType::DateTimeExtended => "Date/Time Extended",
        ColumnType::Unknown(b) => return format!("Type {b}"),
        _ => return "Unknown".to_string(),
    }
    .to_string()
}

fn type_is_numeric(ty: ColumnType) -> bool {
    matches!(
        ty,
        ColumnType::Byte
            | ColumnType::Int
            | ColumnType::Long
            | ColumnType::Money
            | ColumnType::Float
            | ColumnType::Double
            | ColumnType::Numeric
            | ColumnType::BigInt
    )
}

fn page_of(lay: &Layout, rows: Vec<Vec<Value>>) -> SelectPage {
    let mut out_rows = Vec::with_capacity(rows.len());
    let mut keys = Vec::with_capacity(rows.len());
    for values in rows {
        let row: Vec<String> = lay
            .val_idx
            .iter()
            .map(|&i| values.get(i).map(value_text).unwrap_or_else(|| "NULL".to_string()))
            .collect();
        keys.push(lay.pk_disp.iter().filter_map(|&i| row.get(i).cloned()).collect());
        out_rows.push(row);
    }
    SelectPage {
        columns: lay.cols.clone(),
        rows: out_rows,
        keys,
        truncated: false,
    }
}

/// 值渲染：NULL → `NULL`，BLOB → `<BLOB n 字节>`，日期按 Access 的
/// `yyyy-mm-dd hh:mm:ss` 出，其余原样。
pub fn value_text(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => (if *b { "True" } else { "False" }).to_string(),
        Value::Byte(n) => n.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Long(n) => n.to_string(),
        Value::BigInt(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Text(s) => s.clone(),
        Value::Binary(b) => format!("<BLOB {} 字节>", b.len()),
        Value::Money(s) | Value::Numeric(s) | Value::Guid(s) | Value::DateTimeExtended(s) => {
            s.clone()
        }
        Value::Timestamp(days) => format_timestamp(*days),
        other => format!("{other:?}"),
    }
}

/// Jet 时间戳：1899-12-30 起的天数（f64）。Access 只显示到秒，四舍五入。
fn format_timestamp(days: f64) -> String {
    if !days.is_finite() {
        return days.to_string();
    }
    let secs = (days * 86_400.0).round();
    if secs.abs() > i64::MAX as f64 {
        return days.to_string();
    }
    let base = chrono::NaiveDate::from_ymd_opt(1899, 12, 30)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .expect("1899-12-30 00:00:00 是合法日期");
    match base.checked_add_signed(chrono::TimeDelta::seconds(secs as i64)) {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => days.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(1);

    fn fixture() -> (String, PathBuf) {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("daro_access_{}_{}.accdb", std::process::id(), n));
        std::fs::write(&path, include_bytes!("../assets/testdata/access_fixture.accdb")).unwrap();
        (format!("access-test-{n}"), path)
    }

    #[test]
    fn reads_tables_columns_and_rows() {
        let (key, path) = fixture();
        open(&key, path.to_str().unwrap(), None).unwrap();
        assert_eq!(try_list_tables(&key).unwrap(), vec!["member", "no_key"]);

        let cols = try_table_columns(&key, "member").unwrap();
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["id", "name", "score", "active", "born", "remark"]);
        assert!(cols[0].pk, "id 是主键");
        assert!(cols.iter().all(|c| !c.writable), "Access 连接只读");
        assert!(cols[2].numeric && !cols[1].numeric, "数值列按类型标记");

        let page = select_all(&key, "member").unwrap();
        assert_eq!(page.rows.len(), 3);
        assert_eq!(page.rows[0], ["1", "Alice", "98.5", "True", "1990-01-02 00:00:00", "first line"]);
        assert_eq!(page.rows[1][1], "中文");
        assert_eq!(page.rows[1][3], "False");
        assert_eq!(page.rows[1][4], "2001-12-31 23:59:59");
        assert_eq!(page.rows[2][1], "NULL");
        assert_eq!(page.keys[0], ["1"], "主键值作行身份");

        let no_key = select_all(&key, "no_key").unwrap();
        assert_eq!(no_key.rows.len(), 2);
        assert!(no_key.keys.iter().all(|k| k.is_empty()), "无主键不造身份");
        assert_eq!(count_rows(&key, "member").unwrap(), 3);

        close(&key);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pages_and_parts() {
        let (key, path) = fixture();
        open(&key, path.to_str().unwrap(), None).unwrap();
        let page = select_page(&key, "member", 1, 1).unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0][1], "中文");

        let (indexes, uniques) = table_parts(&key, "member").unwrap();
        assert_eq!(indexes.len(), 1, "主键索引");
        assert_eq!(uniques.len(), 1, "主键也是唯一索引");

        close(&key);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_reports_path() {
        let err = probe("C:/definitely/not/here.accdb", None).unwrap_err();
        assert!(err.contains("找不到 Access 数据库文件"), "{err}");
    }
}
