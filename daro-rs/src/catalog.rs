//! 目录分发层：连接树的三级清单（库 → 模式 → 对象）统一从这里出。
//!
//! mock 档直接转 `mock` 的静态清单；真驱动档（SQLite / PostgreSQL）查真库。
//! 真清单要喂给既有树渲染的 `&'static str` 口径，因此用全局字符串驻留表把
//! 名字固化成 `&'static str`——同一个名字只泄漏一次，「刷新」只清索引，
//! 不随收展 / 执行 SQL 持续涨内存。
//!
//! 两个引擎的层级不一样：
//! - SQLite：库 = 主库 + ATTACH 进来的库，没有模式层，对象按库查
//! - PostgreSQL：库 = 服务器上可连接的库（连一次列一次），对象还要模式限定，
//!   因此每个库各有一条连接（键的拼法见 `pg_driver::key`）
//!
//! 查询失败不再静默吞掉：记进 LAST_ERROR，UI 在连接 / 刷新后 `take_error`
//! 上屏，避免「空树看起来像空库」。
//!
//! **这一层绝不碰数据库**（对调用方而言）：`databases` / `schemas` / `objects`
//! / `table_columns` 只读缓存，未命中就把活儿丢进 `PENDING` 队列并立刻返回空
//! 清单。它们是在树和面板的渲染路径里被调的，同步查库等于每帧冻结界面。
//! 界面层（`DaroApp::pump_catalog`）取出队列投到后台线程跑 `run`，跑完写回
//! 缓存再重绘；`Pending::key` 同时是「这一行正在载入」的标记键。
//! 失败的清单会按空值固化（否则每帧重试会把服务器打爆），用户刷新即可重来。

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use crate::backend;
use crate::db_types::ColMeta;
use crate::mock::{self, Connection, DbKind};
use crate::{pg_driver, sqlite_driver};

/// 一次待办的目录加载。`key()` 既用于队列去重，也用于界面标「载入中」。
#[derive(Clone)]
pub enum Pending {
    /// 打开连接：建连（TCP 握手 / 开库文件）+ 顺带列库清单。
    /// 建成之后界面层才把连接标成已连接，所以它也是「连接中」的标记键。
    Connect(Connection),
    Databases(Connection),
    Schemas(Connection, String),
    Objects(Connection, String, String, usize),
    Columns(Connection, String, String, String),
}

/// 下面四个键是缓存键，也是界面层的 loading 标记键，两边必须一致。
pub fn open_key(conn: &str) -> String {
    format!("conn|{conn}")
}

pub fn databases_key(conn: &str) -> String {
    format!("db|{conn}")
}

pub fn schemas_key(conn: &str, db: &str) -> String {
    format!("schema|{conn}|{db}")
}

pub fn objects_key(conn: &str, db: &str, schema: &str, cat: usize) -> String {
    format!("obj|{conn}|{db}|{schema}|{cat}")
}

pub fn columns_key(conn: &str, db: &str, schema: &str, table: &str) -> String {
    format!("cols|{conn}|{db}|{schema}|{table}")
}

impl Pending {
    pub fn key(&self) -> String {
        match self {
            Self::Connect(c) => open_key(&c.name),
            Self::Databases(c) => databases_key(&c.name),
            Self::Schemas(c, db) => schemas_key(&c.name, db),
            Self::Objects(c, db, schema, cat) => objects_key(&c.name, db, schema, *cat),
            Self::Columns(c, db, schema, table) => columns_key(&c.name, db, schema, table),
        }
    }

    /// 该任务属于哪条连接：后台跑完时界面按名字回查下标，避免下标在这期间漂移。
    pub fn conn(&self) -> &Connection {
        match self {
            Self::Connect(c) => c,
            Self::Databases(c) => c,
            Self::Schemas(c, _) => c,
            Self::Objects(c, _, _, _) => c,
            Self::Columns(c, _, _, _) => c,
        }
    }

    /// 在飞期间要占住的标记键。建连顺带把库清单也填了，所以两条共用，
    /// 否则「连接中」和「库清单载入中」会各跑一遍、互相打脸。
    /// 界面层的 `loading` 也用同一组键，收展时才知道哪行该标「载入中」。
    pub fn marks(&self) -> Vec<String> {
        match self {
            Self::Connect(c) => vec![open_key(&c.name), databases_key(&c.name)],
            p => vec![p.key()],
        }
    }

    /// 真查一次并把结果写进缓存。**只在后台线程调用**（建连 + 查询都可能几秒）。
    /// 返回错误文案供界面上屏；失败同样固化成空清单，避免渲染每帧重试。
    pub fn run(&self) -> Option<String> {
        match self.exec() {
            Ok(()) => None,
            Err(e) => {
                // 失败也占住缓存：空清单 + 错误上屏，刷新（invalidate）后才重试
                match self {
                    Self::Connect(conn) | Self::Databases(conn) => {
                        put(&DATABASES_CACHE, &databases_key(&conn.name), &[])
                    }
                    Self::Schemas(conn, db) => {
                        put(&SCHEMAS_CACHE, &schemas_key(&conn.name, db), &[])
                    }
                    Self::Objects(conn, db, schema, cat) => put(
                        &OBJECTS_CACHE,
                        &objects_key(&conn.name, db, schema, *cat),
                        &[],
                    ),
                    Self::Columns(conn, db, schema, table) => {
                        COLUMNS_CACHE.lock().unwrap().insert(
                            columns_key(&conn.name, db, schema, table),
                            Vec::new(),
                        );
                    }
                }
                set_error(e.clone());
                Some(e)
            }
        }
    }

    fn exec(&self) -> Result<(), String> {
        match self {
            Self::Connect(conn) => {
                backend::open_connection(conn)?;
                fill_databases(conn)
            }
            Self::Databases(conn) => fill_databases(conn),
            Self::Schemas(conn, db) => {
                let v = query_schemas(conn, db)?;
                put(&SCHEMAS_CACHE, &schemas_key(&conn.name, db), interned(v));
                Ok(())
            }
            Self::Objects(conn, db, schema, cat) => {
                let v = query_objects(conn, db, schema, *cat)?;
                put(
                    &OBJECTS_CACHE,
                    &objects_key(&conn.name, db, schema, *cat),
                    interned(v),
                );
                Ok(())
            }
            Self::Columns(conn, db, schema, table) => {
                let v = query_columns(conn, db, schema, table)?;
                COLUMNS_CACHE
                    .lock()
                    .unwrap()
                    .insert(columns_key(&conn.name, db, schema, table), v);
                Ok(())
            }
        }
    }
}

fn fill_databases(conn: &Connection) -> Result<(), String> {
    let v = query_databases(conn)?;
    put(&DATABASES_CACHE, &databases_key(&conn.name), interned(v));
    Ok(())
}

/// 取出待办并标记为「已在飞」；同一件事在跑完前不会重复入队。
pub fn take_pending() -> Vec<Pending> {
    let mut queue = PENDING.lock().unwrap();
    let mut inflight = IN_FLIGHT.lock().unwrap();
    let jobs: Vec<Pending> = queue.drain(..).collect();
    for p in &jobs {
        for m in p.marks() {
            inflight.insert(m);
        }
    }
    jobs
}

/// 一个加载跑完：放开在飞标记。界面层紧接着重绘，故不需要额外通知。
pub fn finish(p: &Pending) {
    let mut inflight = IN_FLIGHT.lock().unwrap();
    for m in p.marks() {
        inflight.remove(&m);
    }
}

/// 缓存读不到就入队。已经在飞的同类活儿（含「建连顺带列库」）不重复排。
fn schedule(p: Pending) {
    let key = p.key();
    {
        let inflight = IN_FLIGHT.lock().unwrap();
        if inflight.iter().any(|m| *m == key) {
            return;
        }
    }
    let mut queue = PENDING.lock().unwrap();
    if queue.iter().any(|q| q.marks().iter().any(|m| *m == key)) {
        return;
    }
    queue.push(p);
}

/// 用户点「打开连接」：直接排一条建连任务（顺带列库），不等渲染碰到缓存才排。
/// 返回标记键，界面层拿去插进 `loading`。
pub fn request_open(conn: &Connection) -> String {
    schedule(Pending::Connect(conn.clone()));
    open_key(&conn.name)
}

/// SQLite 档某连接的库文件路径（连接的 database 字段即文件路径）。
pub fn sqlite_path(conn: &Connection) -> String {
    conn.database.trim().to_string()
}

/// 树上连接节点下的库清单。缓存命中即返回，未命中只负责排队。
pub fn databases(conn: &Connection) -> &'static [&'static str] {
    if !backend::is_real(conn.kind) {
        return mock::databases(conn.kind);
    }
    get(&DATABASES_CACHE, &databases_key(&conn.name)).unwrap_or_else(|| {
        schedule(Pending::Databases(conn.clone()));
        &[]
    })
}

/// 是否有「模式」层（SQLite 没有）。
pub fn has_schemas(kind: DbKind) -> bool {
    kind != DbKind::SQLite && mock::has_schemas(kind)
}

/// 某库的模式清单。SQLite 恒空；PG 连到该库列可见模式（系统模式除外）。
pub fn schemas(conn: &Connection, db: &str) -> &'static [&'static str] {
    if conn.kind == DbKind::SQLite {
        return &[];
    }
    if conn.kind != DbKind::PostgreSql {
        return mock::schemas(conn.kind);
    }
    get(&SCHEMAS_CACHE, &schemas_key(&conn.name, db)).unwrap_or_else(|| {
        schedule(Pending::Schemas(conn.clone(), db.to_string()));
        &[]
    })
}

/// 某库某模式某分类（0 表 / 1 视图 / 2 物化视图 / 3 例程 / 4 查询）的对象清单。
pub fn objects(conn: &Connection, db: &str, schema: &str, cat: usize) -> &'static [&'static str] {
    if !backend::is_real(conn.kind) {
        return mock::objects(conn, db, cat);
    }
    get(&OBJECTS_CACHE, &objects_key(&conn.name, db, schema, cat))
        .unwrap_or_else(|| {
            schedule(Pending::Objects(
                conn.clone(),
                db.to_string(),
                schema.to_string(),
                cat,
            ));
            &[]
        })
}

/// 某表的列元数据（树上展开字段 / 对象面板预览）。
pub fn table_columns(conn: &Connection, db: &str, schema: &str, table: &str) -> Vec<ColMeta> {
    if !backend::is_real(conn.kind) {
        return mock_columns();
    }
    let key = columns_key(&conn.name, db, schema, table);
    if let Some(cols) = COLUMNS_CACHE.lock().unwrap().get(&key) {
        return cols.clone();
    }
    schedule(Pending::Columns(
        conn.clone(),
        db.to_string(),
        schema.to_string(),
        table.to_string(),
    ));
    Vec::new()
}

// ── 真查询（只在后台线程走） ─────────────────────────────────────

fn query_databases(conn: &Connection) -> Result<Vec<String>, String> {
    match conn.kind {
        DbKind::SQLite => {
            let path = sqlite_path(conn);
            sqlite_driver::ensure(&conn.name, &path)?;
            let mut names = sqlite_driver::list_databases(&conn.name);
            if names.is_empty() {
                names.push("main".to_string());
            }
            Ok(names)
        }
        _ => pg_conn(conn, &conn.database).and_then(|k| pg_driver::try_list_databases(&k)),
    }
}

fn query_schemas(conn: &Connection, db: &str) -> Result<Vec<String>, String> {
    pg_conn(conn, db).and_then(|k| pg_driver::try_list_schemas(&k))
}

fn query_objects(conn: &Connection, db: &str, schema: &str, cat: usize) -> Result<Vec<String>, String> {
    match conn.kind {
        DbKind::SQLite => {
            let path = sqlite_path(conn);
            sqlite_driver::ensure(&conn.name, &path)?;
            match cat {
                0 => sqlite_driver::try_list_tables(&conn.name, db),
                1 => sqlite_driver::try_list_views(&conn.name, db),
                _ => Ok(Vec::new()),
            }
        }
        _ => {
            let k = pg_conn(conn, db)?;
            let s = pg_schema(schema);
            match cat {
                0 => pg_driver::try_list_tables(&k, s),
                1 => pg_driver::try_list_views(&k, s),
                2 => pg_driver::try_list_matviews(&k, s),
                3 => pg_driver::try_list_routines(&k, s),
                _ => Ok(Vec::new()),
            }
        }
    }
}

fn query_columns(conn: &Connection, db: &str, schema: &str, table: &str) -> Result<Vec<ColMeta>, String> {
    match conn.kind {
        DbKind::SQLite => {
            let path = sqlite_path(conn);
            sqlite_driver::ensure(&conn.name, &path)?;
            sqlite_driver::try_table_columns(&conn.name, db, table)
        }
        DbKind::PostgreSql => {
            let k = pg_conn(conn, db)?;
            pg_driver::try_table_columns(&k, pg_schema(schema), table)
        }
        _ => Ok(mock_columns()),
    }
}

fn mock_columns() -> Vec<ColMeta> {
    mock::MOCK_COLUMNS
        .iter()
        .map(|m| ColMeta::new(m.name, m.ty, m.numeric))
        .collect()
}

/// 「刷新」失效目录缓存：只清索引，已驻留的名字由 INTERN 去重，不会
/// 随刷新次数增长（唯一名字总量才是上界）。
pub fn invalidate() {
    DATABASES_CACHE.lock().unwrap().clear();
    SCHEMAS_CACHE.lock().unwrap().clear();
    OBJECTS_CACHE.lock().unwrap().clear();
    COLUMNS_CACHE.lock().unwrap().clear();
}

/// 只失效某一张表的列缓存：建表 / 改表后不必把整棵树重查一遍。
pub fn invalidate_columns(conn: &str, db: &str, schema: &str, table: &str) {
    let prefix = format!("cols|{conn}|{db}|{schema}|");
    let exact = columns_key(conn, db, schema, table);
    COLUMNS_CACHE
        .lock()
        .unwrap()
        .retain(|k, _| *k != exact && !k.starts_with(&prefix));
}

/// 取走最近一次目录查询错误（连接 / 刷新后由上屏层消费）。
pub fn take_error() -> Option<String> {
    LAST_ERROR.lock().unwrap().take()
}

fn set_error(e: String) {
    *LAST_ERROR.lock().unwrap() = Some(e);
}

/// PG：确保「这条连接的这个库」有句柄，返回驱动注册表的键。
fn pg_conn(conn: &Connection, db: &str) -> Result<String, String> {
    let params = backend::pg_params(conn, db);
    let key = pg_driver::key(&conn.name, &params.dbname);
    pg_driver::ensure(&key, &params)?;
    Ok(key)
}

/// 模式名兜底：没有选中模式层时按 PG 的默认模式。
fn pg_schema(schema: &str) -> &str {
    let s = schema.trim();
    if s.is_empty() { "public" } else { s }
}

/// 名字驻留 + 固化成树渲染用的 `'static` 切片。
fn interned(list: Vec<String>) -> &'static [&'static str] {
    let names: Vec<&'static str> = list.iter().map(|s| intern(s)).collect();
    crate::db_types::leak_slice(names)
}

/// 名字驻留：同一字符串只泄漏一次，供 `&'static [&'static str]` 渲染口径使用。
fn intern(s: &str) -> &'static str {
    let mut set = INTERN.lock().unwrap();
    if let Some(v) = set.get(s) {
        return *v;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

fn get(
    cache: &Mutex<HashMap<String, &'static [&'static str]>>,
    key: &str,
) -> Option<&'static [&'static str]> {
    cache.lock().unwrap().get(key).copied()
}

fn put(
    cache: &Mutex<HashMap<String, &'static [&'static str]>>,
    key: &str,
    v: &'static [&'static str],
) {
    cache.lock().unwrap().insert(key.to_string(), v);
}

static DATABASES_CACHE: LazyLock<Mutex<HashMap<String, &'static [&'static str]>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SCHEMAS_CACHE: LazyLock<Mutex<HashMap<String, &'static [&'static str]>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static OBJECTS_CACHE: LazyLock<Mutex<HashMap<String, &'static [&'static str]>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static COLUMNS_CACHE: LazyLock<Mutex<HashMap<String, Vec<ColMeta>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static INTERN: LazyLock<Mutex<HashSet<&'static str>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static LAST_ERROR: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));
/// 待办队列 + 在飞集合（键相同即视为同一件事）
static PENDING: LazyLock<Mutex<Vec<Pending>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
