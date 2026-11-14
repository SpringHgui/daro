# 数据页「开始事务」与「历史」设计

> 状态：设计已确认，开始实施（阶段 A 后端事务 API 进行中）。
> 关联：数据页工具条 `开始事务`(`main.rs` 约 35693，`tbtn("txn",…,false)`)、`历史`(约 35718，`tbtn("hist",…,false)`)。

## 0. 现状矛盾

- 后端 `Target` 是无状态的：每次 `execute`/`select` 都走 `with_conn(key, f)` 全局缓存，按 `(连接名, 库)` 复用同一条物理连接，默认自动提交。
- `rollback_open_tx`(`backend.rs:652`) 注释明确："UI 这条连接没有跨语句事务，恒 false"——PG/MySQL/SQLServer/Mongo/Access 脚本跑完即自动提交/回滚，事务不跨语句存活。仅 SQLite 持有单条常驻连接，但也没被 UI 暴露。
- **核心矛盾**：若直接在缓存连接上 `BEGIN`，会因"同一 key 的目录查询、网格翻页也复用该连接"而污染无关读操作，甚至造成锁等待。

**结论**：用户事务必须用一条与读缓存隔离的专用连接，否则无法实现。

## 1. 「开始事务」设计

### 1.1 专用事务连接（与读缓存隔离）

各驱动开一条**独立**物理连接（不进 `with_conn` 缓存）：

- SQLite：再开一个到同文件的 `RawConn`（SQLite 允许多句柄同文件）。
- PG：`pg_driver::connect(params)` 新 `Client`，不进缓存。
- MySQL：`mysql_driver::connect(params)` 新 `Conn`，不进缓存。
- SQLServer：`sqlserver_driver::connect(params)` 新 `Conn`，不进缓存。
- Access（只读）/Mongo（无 SQL）：不支持，`supports_tx()==false`。

### 1.2 后端 API（不破坏现有 `execute`）

```rust
// backend.rs
pub trait TxBackend: Send {
    fn execute(&mut self, stmt: &str, max_rows: usize) -> Result<ExecOutcome, String>;
    fn commit(&mut self) -> Result<(), String>;
    fn rollback(&mut self) -> Result<(), String>;
}

pub struct TxHandle {
    inner: Box<dyn TxBackend>,
    done: bool,   // 已提交/回滚后置 true，Drop 不再回滚
}

impl TxHandle {
    pub fn execute(&mut self, stmt: &str, max_rows: usize) -> Result<ExecOutcome, String>;
    pub fn commit(mut self) -> Result<(), String>;
    pub fn rollback(mut self) -> Result<(), String>;
    pub fn active(&self) -> bool;
}
impl Drop for TxHandle {
    fn drop(&mut self) { if !self.done { let _ = self.inner.rollback(); } } // 异常路径兜底
}

impl Target {
    pub fn supports_tx(&self) -> bool;
    pub fn begin_tx(&self) -> Result<TxHandle, String>;  // 各驱动 begin_tx 复用 execute_on
}
```

- `TxHandle` 内部按引擎存各驱动专用连接结构（`SqliteTx`/`PgTx`/`MySqlTx`/`SqlServerTx`），各自 `impl TxBackend` 并复用该驱动既有
  `execute_on(&mut conn, stmt, max)`（SQLite 387 / PG 710 / MySQL 609 / SQLServer 801，SQLServer 内部已 `block_on` 同步）。
- `TxHandle` 内部用 SQL 字符串 `BEGIN`/`COMMIT`/`ROLLBACK` 控制事务（SQLServer 用 `BEGIN TRAN`）。
- `supports_tx()==false` 时 UI 把"开始事务"置灰 + tooltip 说明。

### 1.3 UI 接线（`DaroApp` + 数据页工具条）—— 阶段 B

- `main.rs:35693` 的"开始事务"启用；点击取当前活动数据/查询 tab 的 `Target::of(conn,db,schema)` → 校验 `supports_tx` → `begin_tx` → 存入 `OpenTab.tx: Option<TxHandle>`。
- 工具条状态切换：显示「提交」「回滚」两枚，高亮指示"事务进行中"。
- **自动回滚触发**：tab 关闭 / 连接断开 / 切换连接 → `rollback_tx` + 释放。
- 引擎注意：PG 事务内 `CREATE DATABASE` 报错；MySQL/PG 的 DDL 会隐式提交——UI 给警告（阶段 B 处理）。

### 1.4 边界与风险

- 长事务持有专用连接 → 资源占用；可加超时自动回滚/提示。
- 专用连接与读连接针对同一库可能出现锁（事务未提交时读连接要写同表）——数据库正常行为，报错即提示，UI 不特殊处理。
- 多 tab 各自开事务 → 各自独立专用连接，互不影响（都不走缓存）。

## 2. 「历史」设计（查询历史）

### 2.1 范围界定

- "历史" = **自动记录已执行的 SQL**（查询页 / 脚本运行 / 工具栏执行），与现 `saved_queries.rs`(queries.json, 用户主动保存) 区分。
- **不**覆盖单元格编辑撤销（那是网格 `grid_undo_state`，已存在）。

### 2.2 数据模型与落盘（新增 `history.rs`）—— 阶段 C

```rust
struct QueryHistoryEntry {
    id: u64, ts: i64, conn: String, db: String, schema: String,
    sql: String, ok: bool, rows: usize, ms: u64,
}
```

- 落盘 `history.json`（与 connections.json 同目录，受 `DARO_DATA_DIR` 覆盖）；内存上限（如最近 5000 条，超出截断）。
- 写：在 `ScriptRun` 结果回传处（约 `main.rs:12658/16170/26241/31787` 那几个回调）追加记录；失败也记（`ok=false`）便于排查。单语句工具栏执行（`execute_query_stmt` 约 6742）作为次要路径可选记录。

### 2.3 UI（`history_dialog.rs`）—— 阶段 D

- `main.rs:35718` 的"历史"启用；点击打开 `history_dialog`：
  - 列表：时间、连接、库、SQL 预览、成功/失败标记、耗时。
  - 筛选：按连接下拉 + 关键字搜索。
  - 操作：双击/"重新运行" → SQL 灌进当前查询页签并执行；"复制" → 剪贴板；"保存到已保存查询" → 桥接 `saved_queries`。
  - 清空：按连接或全量（带确认）。
- 复用 `about_dialog` 的 `open_window` + `footer_bar` 模式。

### 2.4 集成点（关键）

- 历史记录**只接在 `ScriptRun` 结果回传这一处**，不散落各 `execute` 调用点。实施阶段 C 第一步先 grep 确认具体收口函数。

## 3. 实施顺序（每阶段可独立编译验收）

| 阶段 | 内容 | 风险 |
|---|---|---|
| **A** | 后端事务：`TxHandle` + `begin/commit/rollback/execute_in_tx/supports_tx`，先 SQLite+MySQL，再 PG+SQLServer | 中（纯后端，可单测） |
| **B** | 事务 UI：`OpenTab.tx` + 工具条「开始事务/提交/回滚」+ 自动回滚；接线 35693 | 中 |
| **C** | 历史后端：`history.rs` + `history.json` 读写 + 在收口处记录 | 低 |
| **D** | 历史 UI：`history_dialog.rs` + 接线 35718 | 低 |

> 注：本沙箱无法 `cargo build`（不能 spawn 子进程），所有改动仅人工核对语法/借用，需在正常编译环境验收。

## 4. 验收要点（可编译环境）

- **事务**：BEGIN 后 INSERT，提交前在另一读连接查不到（隔离性）；提交后查到；回滚后查不到。PG 事务内建库报错；连接断开自动回滚。
- **历史**：执行若干 SQL 后打开历史能看到记录；重新运行可复现；落盘 `history.json` 重启后仍在。
