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
//! 失败先就地补一轮（socket 抖动 / 并发建连被服务端掐掉，见 `Pending::run`），
//! 补不回来才按空值固化——否则每帧重试会把服务器打爆，用户刷新即可重来。

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::backend;
use crate::db_types::ColMeta;
use crate::mock::{self, Connection, DbKind};
use crate::{
    access_driver, mongo_driver, mysql_driver, pg_driver, redis_driver, sqlite_driver,
    sqlserver_driver,
};

/// 瞬时失败补一轮的间隔。
const RETRY_BACKOFF: Duration = Duration::from_millis(400);

/// 「快到不像真失败」的时间窗：只有首轮在窗内失败才补重试（见 [Pending::run]）。
const RETRY_GRACE: Duration = Duration::from_secs(2);

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
    /// 表节点下除字段外的七个分组（索引 / 外键 / 唯一键 / 检查 / 排除 /
    /// 规则 / 触发器）的成员名。一次查回来，展开任一组都直接命中。
    Parts(Connection, String, String, String),
}

/// 表展开后各分组的成员名。字段组仍走列缓存（[table_columns]），这里只装
/// 其余七组；组序号与界面 `OBJ_GROUPS` 对齐（0 字段、1 索引…7 触发器）。
#[derive(Clone, Default)]
pub struct TableParts {
    indexes: Vec<&'static str>,
    fks: Vec<&'static str>,
    uniques: Vec<&'static str>,
    checks: Vec<&'static str>,
    excludes: Vec<&'static str>,
    rules: Vec<&'static str>,
    triggers: Vec<&'static str>,
}

impl TableParts {
    pub fn group(&self, gix: usize) -> &[&'static str] {
        match gix {
            1 => &self.indexes,
            2 => &self.fks,
            3 => &self.uniques,
            4 => &self.checks,
            5 => &self.excludes,
            6 => &self.rules,
            7 => &self.triggers,
            _ => &[],
        }
    }
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

pub fn parts_key(conn: &str, db: &str, schema: &str, table: &str) -> String {
    format!("parts|{conn}|{db}|{schema}|{table}")
}

impl Pending {
    pub fn key(&self) -> String {
        match self {
            Self::Connect(c) => open_key(&c.name),
            Self::Databases(c) => databases_key(&c.name),
            Self::Schemas(c, db) => schemas_key(&c.name, db),
            Self::Objects(c, db, schema, cat) => objects_key(&c.name, db, schema, *cat),
            Self::Columns(c, db, schema, table) => columns_key(&c.name, db, schema, table),
            Self::Parts(c, db, schema, table) => parts_key(&c.name, db, schema, table),
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
            Self::Parts(c, _, _, _) => c,
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
    /// 返回错误文案供界面上屏；瞬时失败先就地补一次，补不回来才固化成空清单。
    pub fn run(&self) -> Option<String> {
        let name = &self.conn().name;
        // 关闭动作可能发生在排队之后：这条活儿作废，既不查库也不上屏
        if is_closed(name) {
            return None;
        }
        let started = Instant::now();
        let mut epoch = epoch_of(name);
        let mut tries = 0;
        let err = loop {
            tries += 1;
            if let Err(e) = self.exec() {
                // 关闭期间失败：这条连接的活儿已经作废，不上屏、不固化缓存
                if is_closed(name) {
                    return None;
                }
                // 期间「关闭 → 重新打开」：手里还是上一轮的句柄，按新一轮重跑
                let current = epoch_of(name);
                if current != epoch && tries < 3 {
                    epoch = current;
                    continue;
                }
                // 只补「一眨眼就失败」的那种：socket 抖动、并发建连被服务端掐掉。
                // 驱动在这种错误后会把坏句柄摘掉，所以补的这轮会真重连。
                // 连不上是慢失败（建连自带上限），不再补第二轮，否则离线服务器的
                // 报错要等两遍超时才上屏。
                if tries == 1 && started.elapsed() < RETRY_GRACE {
                    std::thread::sleep(RETRY_BACKOFF);
                    continue;
                }
                break e;
            }
            self.settle();
            return None;
        };
        if self.is_stale() {
            self.settle();
            return None;
        }
        self.cache_empty();
        set_error(err.clone());
        Some(err)
    }

    /// 这条活儿所属的连接在跑的过程中被关掉了吗。
    fn is_stale(&self) -> bool {
        is_closed(&self.conn().name)
    }

    /// 关闭期间跑完的收尾：驱动句柄可能被 `ensure` 又建回来了，收掉；
    /// 缓存 / 错误一概不写——这条连接的活儿已经作废。
    fn settle(&self) {
        if self.is_stale() {
            backend::close_connection(self.conn());
        }
    }

    /// 失败兜底：把这一层固化成空清单，否则渲染每帧都排队重查、把服务器打爆。
    /// 用户刷新（`invalidate`）或重连才会重来。
    fn cache_empty(&self) {
        match self {
            Self::Connect(conn) | Self::Databases(conn) => {
                put(&DATABASES_CACHE, &databases_key(&conn.name), &[])
            }
            Self::Schemas(conn, db) => put(&SCHEMAS_CACHE, &schemas_key(&conn.name, db), &[]),
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
            Self::Parts(conn, db, schema, table) => {
                PARTS_CACHE
                    .lock()
                    .unwrap()
                    .insert(parts_key(&conn.name, db, schema, table), TableParts::default());
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
            Self::Parts(conn, db, schema, table) => {
                let v = query_table_parts(conn, db, schema, table)?;
                PARTS_CACHE
                    .lock()
                    .unwrap()
                    .insert(parts_key(&conn.name, db, schema, table), v);
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
    // 连接已关：状态栏、面包屑这类渲染路径仍会来要库清单，排进去只会空跑一轮
    // 又把连接行标回「载入中」（缓存为空 → 每帧再排），关掉期间一概不排。
    if is_closed(&p.conn().name) {
        return;
    }
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
    // 重新打开就把「已关闭」标记撤掉、轮次往前走：上一次断开留下的在飞任务
    // 回来后据此重跑，而不是把旧那一轮的失败当结果
    CLOSED.lock().unwrap().remove(&conn.name);
    bump_epoch(&conn.name);
    schedule(Pending::Connect(conn.clone()));
    open_key(&conn.name)
}

/// 断开 / 删除 / 改名时叫停这条连接的目录任务。
///
/// 一是把还排着队的活儿丢掉——否则「列库 / 列表」在关闭后照跑，驱动层的
/// `ensure` 顺手又把连接建回来；二是立个「已关闭」标记，在飞的建连回执
/// 回来时界面据此不再把连接标回已连接（见 `catalog_loaded`）。
pub fn request_close(conn: &str) {
    CLOSED.lock().unwrap().insert(conn.to_string());
    bump_epoch(conn);
    PENDING.lock().unwrap().retain(|p| p.conn().name != conn);
}

/// 这条连接是否已被关闭（界面据此作废建连回执）。
pub fn is_closed(conn: &str) -> bool {
    CLOSED.lock().unwrap().contains(conn)
}

/// SQLite 档某连接的库文件路径（连接的 database 字段即文件路径）。
pub fn sqlite_path(conn: &Connection) -> String {
    conn.database.trim().to_string()
}

/// Access 档某连接的库文件路径（与 SQLite 同口径：`database` 即文件路径）。
pub fn access_path(conn: &Connection) -> String {
    conn.database.trim().to_string()
}

/// Access 树上「库」节点的名字：文件名（去掉扩展名）。一个文件就是一个库，
/// 取数并不用这个名字寻址，只给树 / 面包屑显示。
pub fn access_db_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Access".to_string())
}

/// 树上连接节点下的库清单。缓存命中即返回，未命中只负责排队。
pub fn databases(conn: &Connection) -> &'static [&'static str] {
    if !backend::is_real_conn(conn) {
        return mock::databases(conn.kind);
    }
    get(&DATABASES_CACHE, &databases_key(&conn.name)).unwrap_or_else(|| {
        schedule(Pending::Databases(conn.clone()));
        &[]
    })
}

/// 是否有「模式」层（SQLite 没有）。
pub fn has_schemas(kind: DbKind) -> bool {
    !matches!(kind, DbKind::SQLite | DbKind::Access) && mock::has_schemas(kind)
}

/// 该引擎有没有这一类对象（分类序号与界面 `CATEGORIES` 对齐：0 表 / 1 视图 /
/// 2 实体化视图 / 3 函数 / 4 查询）。树、对象面板和功能区都按这道门收放：
/// 不支持就整类不出现，而不是摆一个永远为空的假分类。
pub fn has_category(kind: DbKind, cat: usize) -> bool {
    // Redis 没有表 / 视图这一层：库节点是叶子，键浏览在独立页签里
    if kind == DbKind::Redis {
        return false;
    }
    // Access 目录只有用户表（查询 / 视图不在 jetdb 的表清单里）
    if kind == DbKind::Access {
        return cat == 0;
    }
    match cat {
        // 只有 PG 有真正的 MATERIALIZED VIEW（MySQL / SQL Server / Mongo 没有）
        2 => kind == DbKind::PostgreSql,
        // SQLite / MongoDB / Redis 没有存储例程
        3 => matches!(kind, DbKind::MySql | DbKind::PostgreSql | DbKind::SqlServer),
        _ => true,
    }
}

/// 某库的模式清单。SQLite 恒空；PG 连到该库列可见模式（系统模式除外）。
pub fn schemas(conn: &Connection, db: &str) -> &'static [&'static str] {
    // 演示连接不连真库：库 / 对象两层都退 mock，这一层也要一致，
    // 否则树会把模式层排进后台去连那台演示主机。
    if !backend::is_real_conn(conn) {
        return mock::schemas(conn.kind);
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
    if !backend::is_real_conn(conn) {
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
    if !backend::is_real_conn(conn) {
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

/// 某表索引 / 外键 / 唯一键 / 检查 / 排除 / 规则 / 触发器七组的成员名。
/// 与 [table_columns] 同口径：只读缓存，未命中排后台任务并先返回空清单。
pub fn table_parts(conn: &Connection, db: &str, schema: &str, table: &str) -> TableParts {
    if !backend::is_real_conn(conn) {
        return mock_table_parts(conn.kind, table);
    }
    let key = parts_key(&conn.name, db, schema, table);
    if let Some(p) = PARTS_CACHE.lock().unwrap().get(&key) {
        return p.clone();
    }
    schedule(Pending::Parts(
        conn.clone(),
        db.to_string(),
        schema.to_string(),
        table.to_string(),
    ));
    TableParts::default()
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
        // 一个 Access 文件就是一个库：节点名取文件名
        DbKind::Access => {
            let path = access_path(conn);
            access_driver::ensure(&conn.name, &path, backend::conn_password(conn).as_deref())?;
            Ok(vec![access_db_name(&path)])
        }
        // MySQL 一条连接看全部库，SHOW DATABASES 直接出清单
        DbKind::MySql => mysql_driver::try_list_databases(&backend::mysql_params(conn)),
        // SQL Server 的登录库只是默认落点，库清单走 sys.databases
        DbKind::SqlServer => {
            sqlserver_driver::try_list_databases(
                &conn.name,
                &backend::sqlserver_params(conn, ""),
            )
        }
        DbKind::MongoDb => {
            let params = backend::mongo_params(conn);
            mongo_driver::list_databases(&conn.name, &params)
        }
        // Redis 的「库」是 db0 / db1…，键数顺带给树节点做角标
        DbKind::Redis => {
            let params = backend::redis_params(conn);
            Ok(redis_driver::list_namespaces(&conn.name, &params)?
                .into_iter()
                .map(|n| n.name)
                .collect())
        }
        _ => pg_conn(conn, &conn.database).and_then(|k| pg_driver::try_list_databases(&k)),
    }
}

fn query_schemas(conn: &Connection, db: &str) -> Result<Vec<String>, String> {
    match conn.kind {
        // MySQL / Access / MongoDB / Redis 没有模式层：库下面直接是对象
        DbKind::MySql | DbKind::Access | DbKind::MongoDb | DbKind::Redis => Ok(Vec::new()),
        DbKind::SqlServer => {
            let params = backend::sqlserver_params(conn, db);
            sqlserver_driver::try_list_schemas(&conn.name, &params)
        }
        _ => pg_conn(conn, db).and_then(|k| pg_driver::try_list_schemas(&k)),
    }
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
        DbKind::MySql => {
            let params = backend::mysql_params(conn);
            match cat {
                0 => mysql_driver::try_list_tables(&params, db),
                1 => mysql_driver::try_list_views(&params, db),
                // MySQL 没有物化视图；函数 / 存储过程都在 information_schema.ROUTINES
                3 => mysql_driver::try_list_routines(&params, db),
                _ => Ok(Vec::new()),
            }
        }
        DbKind::SqlServer => {
            let params = backend::sqlserver_params(conn, db);
            match cat {
                0 => sqlserver_driver::try_list_tables(&conn.name, &params, schema),
                1 => sqlserver_driver::try_list_views(&conn.name, &params, schema),
                3 => sqlserver_driver::try_list_routines(&conn.name, &params, schema),
                _ => Ok(Vec::new()),
            }
        }
        DbKind::Access => {
            let path = access_path(conn);
            access_driver::ensure(&conn.name, &path, backend::conn_password(conn).as_deref())?;
            match cat {
                0 => access_driver::try_list_tables(&conn.name),
                _ => Ok(Vec::new()),
            }
        }
        DbKind::MongoDb => {
            let params = backend::mongo_params(conn);
            match cat {
                0 => mongo_driver::list_collections(&conn.name, &params, db, false),
                1 => mongo_driver::list_collections(&conn.name, &params, db, true),
                _ => Ok(Vec::new()),
            }
        }
        DbKind::Redis => Ok(Vec::new()),
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
        DbKind::Access => {
            let path = access_path(conn);
            access_driver::ensure(&conn.name, &path, backend::conn_password(conn).as_deref())?;
            access_driver::try_table_columns(&conn.name, table)
        }
        DbKind::PostgreSql => {
            let k = pg_conn(conn, db)?;
            pg_driver::try_table_columns(&k, pg_schema(schema), table)
        }
        DbKind::MySql => {
            let params = backend::mysql_params(conn);
            mysql_driver::try_table_columns(&params, db, table)
        }
        DbKind::SqlServer => {
            let params = backend::sqlserver_params(conn, db);
            sqlserver_driver::try_table_columns(&conn.name, &params, schema, table)
        }
        // 集合的字段从一条样本文档推：只用于树上展开字段，网格按本页文档现算
        DbKind::MongoDb => {
            let params = backend::mongo_params(conn);
            mongo_driver::sample_columns(&conn.name, &params, db, table)
        }
        DbKind::Redis => Ok(Vec::new()),
    }
}

fn query_table_parts(
    conn: &Connection,
    db: &str,
    schema: &str,
    table: &str,
) -> Result<TableParts, String> {
    let Some(tg) = backend::Target::of(conn, db, schema) else {
        return Ok(mock_table_parts(conn.kind, table));
    };
    let sql = match conn.kind {
        DbKind::PostgreSql => {
            // regclass 字面量：表名里的单引号双写即可（限定名内部是双引号）
            let qtn = format!("'{}'::regclass", tg.qualified_table(table).replace('\'', "''"));
            format!(
                "SELECT 'index', c.relname::text FROM pg_class c \
                   JOIN pg_index i ON i.indexrelid = c.oid \
                 WHERE i.indrelid = {qtn} \
                 UNION ALL \
                 SELECT CASE con.contype WHEN 'f' THEN 'fk' WHEN 'u' THEN 'unique' \
                    WHEN 'c' THEN 'check' WHEN 'x' THEN 'exclude' ELSE 'skip' END, \
                    con.conname::text FROM pg_constraint con WHERE con.conrelid = {qtn} \
                 UNION ALL \
                 SELECT 'rule', r.rulename::text FROM pg_rewrite r \
                 WHERE r.ev_class = {qtn} AND r.rulename <> '_RETURN' \
                 UNION ALL \
                 SELECT 'trigger', t.tgname::text FROM pg_trigger t \
                 WHERE t.tgrelid = {qtn} AND NOT t.tgisinternal \
                 ORDER BY 2"
            )
        }
        DbKind::MySql => {
            let db = db.replace('\'', "''");
            let table = table.replace('\'', "''");
            format!(
                "SELECT 'index', INDEX_NAME FROM information_schema.STATISTICS \
                 WHERE TABLE_SCHEMA = '{db}' AND TABLE_NAME = '{table}' \
                 UNION ALL \
                 SELECT 'fk', CONSTRAINT_NAME FROM information_schema.KEY_COLUMN_USAGE \
                 WHERE TABLE_SCHEMA = '{db}' AND TABLE_NAME = '{table}' \
                   AND REFERENCED_TABLE_NAME IS NOT NULL \
                 UNION ALL \
                 SELECT CASE CONSTRAINT_TYPE WHEN 'UNIQUE' THEN 'unique' \
                    WHEN 'CHECK' THEN 'check' ELSE 'skip' END, CONSTRAINT_NAME \
                 FROM information_schema.TABLE_CONSTRAINTS \
                 WHERE TABLE_SCHEMA = '{db}' AND TABLE_NAME = '{table}' \
                   AND CONSTRAINT_TYPE IN ('UNIQUE', 'CHECK') \
                 UNION ALL \
                 SELECT 'trigger', TRIGGER_NAME FROM information_schema.TRIGGERS \
                 WHERE EVENT_OBJECT_SCHEMA = '{db}' AND EVENT_OBJECT_TABLE = '{table}' \
                 ORDER BY 2"
            )
        }
        DbKind::SQLite => {
            let space = tg.quote(db);
            let table = table.replace('\'', "''");
            // SQLite 的外键 / 检查没有名字，唯一约束多数是 sqlite_autoindex_*：
            // 索引与触发器列名字，唯一键只列显式写了 UNIQUE 的索引。
            format!(
                "SELECT 'index', name FROM {space}.sqlite_master \
                 WHERE type = 'index' AND tbl_name = '{table}' AND sql IS NOT NULL \
                 UNION ALL \
                 SELECT 'unique', name FROM {space}.sqlite_master \
                 WHERE type = 'index' AND tbl_name = '{table}' \
                   AND sql LIKE '%UNIQUE%' \
                 UNION ALL \
                 SELECT 'trigger', name FROM {space}.sqlite_master \
                 WHERE type = 'trigger' AND tbl_name = '{table}' \
                 ORDER BY 2"
            )
        }
        DbKind::SqlServer => {
            let obj = tg.qualified_table(table).replace('\'', "''");
            // sys.* 目录视图是「当前库」的：连接默认库未必是页签所在的库，
            // 必须挂到目标库上，否则 object_id 对不上会读到空清单 / 别的对象。
            let dp = format!("{}.", tg.quote(db));
            format!(
                "SELECT 'index', i.name FROM {dp}sys.indexes i \
                 WHERE i.object_id = OBJECT_ID(N'{obj}') AND i.name IS NOT NULL \
                 UNION ALL \
                 SELECT 'fk', fk.name FROM {dp}sys.foreign_keys fk \
                 WHERE fk.parent_object_id = OBJECT_ID(N'{obj}') \
                 UNION ALL \
                 SELECT 'unique', kc.name FROM {dp}sys.key_constraints kc \
                 WHERE kc.parent_object_id = OBJECT_ID(N'{obj}') \
                 UNION ALL \
                 SELECT 'check', cc.name FROM {dp}sys.check_constraints cc \
                 WHERE cc.parent_object_id = OBJECT_ID(N'{obj}') \
                 UNION ALL \
                 SELECT 'trigger', tr.name FROM {dp}sys.triggers tr \
                 WHERE tr.parent_id = OBJECT_ID(N'{obj}') \
                 ORDER BY 2"
            )
        }
        // Access 的索引 / 唯一键直接从 TDEF 读：外键引用不进索引组
        DbKind::Access => {
            let path = access_path(conn);
            access_driver::ensure(&conn.name, &path, backend::conn_password(conn).as_deref())?;
            let (indexes, uniques) = access_driver::table_parts(&conn.name, table)?;
            let rows: Vec<Vec<String>> = indexes
                .into_iter()
                .map(|n| vec!["index".to_string(), n])
                .chain(uniques.into_iter().map(|n| vec!["unique".to_string(), n]))
                .collect();
            return Ok(parts_from_rows(&rows));
        }
        // MongoDB / Redis 没有索引 / 约束这一层（Mongo 的索引先不进树）
        DbKind::MongoDb | DbKind::Redis => return Ok(TableParts::default()),
    };
    let out = tg.execute(&sql, 10_000)?;
    Ok(parts_from_rows(&out.select.map(|p| p.rows).unwrap_or_default()))
}

fn parts_from_rows(rows: &[Vec<String>]) -> TableParts {
    let mut p = TableParts::default();
    for r in rows {
        let (Some(kind), Some(name)) = (r.first(), r.get(1)) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let bucket = match kind.as_str() {
            "index" => &mut p.indexes,
            "fk" => &mut p.fks,
            "unique" => &mut p.uniques,
            "check" => &mut p.checks,
            "exclude" => &mut p.excludes,
            "rule" => &mut p.rules,
            "trigger" => &mut p.triggers,
            _ => continue,
        };
        let name = intern(name);
        if !bucket.contains(&name) {
            bucket.push(name);
        }
    }
    p
}

/// 演示连接的分组清单：名字随表名走，结构按引擎给（排除 / 规则只属 PG）。
fn mock_table_parts(kind: DbKind, table: &str) -> TableParts {
    let nm = |prefix: &str, suffix: &str| intern(&format!("{prefix}_{table}_{suffix}"));
    let mut p = TableParts {
        indexes: vec![nm("ix", "org"), nm("ix", "create_at")],
        fks: vec![nm("fk", "org"), nm("fk", "parent")],
        uniques: vec![nm("uk", "name")],
        checks: vec![nm("ck", "enabled")],
        triggers: vec![nm("trg", "audit")],
        ..Default::default()
    };
    if matches!(kind, DbKind::PostgreSql | DbKind::SqlServer) {
        p.excludes = vec![nm("ex", "range")];
        p.rules = vec![nm("rl", "insert")];
    }
    p
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
    PARTS_CACHE.lock().unwrap().clear();
}

/// 只失效某一段对象清单（某库某模式某分类）：表被删 / 改名后，那一层重查，
/// 其它库的清单不必跟着重来一遍。
pub fn invalidate_objects(conn: &str, db: &str, schema: &str, cat: usize) {
    OBJECTS_CACHE
        .lock()
        .unwrap()
        .remove(&objects_key(conn, db, schema, cat));
}

/// 只失效某一张表的列缓存：建表 / 改表后不必把整棵树重查一遍。
pub fn invalidate_columns(conn: &str, db: &str, schema: &str, table: &str) {
    let prefix = format!("cols|{conn}|{db}|{schema}|");
    let exact = columns_key(conn, db, schema, table);
    COLUMNS_CACHE
        .lock()
        .unwrap()
        .retain(|k, _| *k != exact && !k.starts_with(&prefix));
    let prefix = format!("parts|{conn}|{db}|{schema}|");
    PARTS_CACHE
        .lock()
        .unwrap()
        .retain(|k, _| !k.starts_with(&prefix));
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
static PARTS_CACHE: LazyLock<Mutex<HashMap<String, TableParts>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static INTERN: LazyLock<Mutex<HashSet<&'static str>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static LAST_ERROR: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));
/// 待办队列 + 在飞集合（键相同即视为同一件事）
static PENDING: LazyLock<Mutex<Vec<Pending>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
/// 已关闭（或改名作废）的连接名：在飞的目录任务回来后据此作废，见 [request_close]。
static CLOSED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
/// 每条连接「第几轮」：打开 / 关闭各算一轮。后台任务据此认出手里是上一轮的句柄。
static EPOCH: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn epoch_of(conn: &str) -> u64 {
    *EPOCH.lock().unwrap().get(conn).unwrap_or(&0)
}

fn bump_epoch(conn: &str) -> u64 {
    let mut map = EPOCH.lock().unwrap();
    let e = map.entry(conn.to_string()).or_insert(0);
    *e += 1;
    *e
}
