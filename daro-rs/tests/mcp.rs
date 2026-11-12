//! MCP 的端到端测试：纯函数判定矩阵 + 真起 `daro-mcpd` 走一遍 Streamable HTTP。
//!
//! 后台进程、鉴权、SQLite 查询 / 拒绝、热改模式与令牌、开关关掉后退出，
//! 都在这里用真实的 exe 与临时数据目录验证；不碰用户自己的连接文件。

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use daro_rs::mcp::{self, policy::{self, Mode, SqlClass}};
use serde_json::{json, Value};

// ------------------------------------------------------------------ 语句分类矩阵

fn assert_class(sql: &str, expected: SqlClass) {
    assert_eq!(policy::classify(sql), expected, "sql: {sql:?}");
}

#[test]
fn classify_read_only_whitelist() {
    for sql in [
        "SELECT * FROM orders LIMIT 10",
        "select id, name from users where created_at > now()",
        "SHOW TABLES",
        "DESC orders",
        "DESCRIBE orders",
        "EXPLAIN SELECT * FROM orders",
        "WITH t AS (SELECT 1 AS a) SELECT * FROM t",
        "SELECT name FROM t WHERE remark = '-- 这不是注释'",
        "SELECT * FROM t /* 行内注释 */ WHERE id = 1",
        "VALUES (1,2)",
        "PRAGMA table_info(t)",
        "SELECT $$ 分号; 在美元引用里 $$ AS s",
    ] {
        assert_class(sql, SqlClass::ReadOnly);
    }
}

#[test]
fn classify_scoped_writes() {
    for sql in [
        "INSERT INTO t (a) VALUES (1)",
        "REPLACE INTO t (a) VALUES (1)",
        "UPDATE t SET a = 1 WHERE id = 5",
        "DELETE FROM t WHERE deleted = 0",
        "UPDATE t SET a = 1 WHERE 1 = 2",
        "UPDATE t SET a = 1 WHERE id = 5 AND TRUE",
        "USE other_db",
    ] {
        assert_class(sql, SqlClass::ScopedWrite);
    }
}

#[test]
fn classify_broad_writes_as_dangerous() {
    for sql in [
        "UPDATE t SET a = 1",
        "DELETE FROM t",
        "UPDATE t SET a = 1 WHERE 1 = 1",
        "DELETE FROM t WHERE TRUE",
        "DELETE FROM t WHERE 1",
        "UPDATE t SET a = 1 WHERE TRUE = TRUE",
        "DELETE FROM t WHERE id = id",
        "UPDATE t SET a = 1 WHERE x = 1 OR 1 = 1",
        "DELETE FROM t WHERE 1 <> 0",
    ] {
        assert_class(sql, SqlClass::Dangerous);
    }
}

#[test]
fn classify_ddl_and_maintenance_as_dangerous() {
    for sql in [
        "CREATE TABLE t (a INT)",
        "ALTER TABLE t ADD COLUMN b INT",
        "DROP TABLE t",
        "TRUNCATE TABLE t",
        "REFRESH MATERIALIZED VIEW mv",
        "VACUUM",
        "GRANT SELECT ON t TO u",
        "SET ROLE r",
        "SET SESSION foo = 1",
        "CALL some_proc(1)",
        "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN UPDATE SET t.a = s.a",
        "SELECT * FROM t INTO OUTFILE '/tmp/x.csv'",
        "SELECT * FROM t FOR UPDATE",
        "EXPLAIN ANALYZE DELETE FROM t WHERE id = 1",
        "PRAGMA journal_mode = WAL",
    ] {
        assert_class(sql, SqlClass::Dangerous);
    }
}

#[test]
fn classify_unknown_as_unclassifiable() {
    for sql in ["", "   ", ";", "SELECT 1; SELECT 2", "FROBNICATE t"] {
        assert_class(sql, SqlClass::Unclassifiable);
    }
}

#[test]
fn trailing_semicolon_and_quoted_semicolon_are_not_multi_statement() {
    assert_class("SELECT 1;", SqlClass::ReadOnly);
    assert_class("SELECT 1 /* ; */ ;", SqlClass::ReadOnly);
    assert_class("SELECT ';' AS s", SqlClass::ReadOnly);
}

#[test]
fn minimum_mode_matrix() {
    assert_eq!(SqlClass::ReadOnly.minimum_mode(), Mode::Readonly);
    assert_eq!(SqlClass::ScopedWrite.minimum_mode(), Mode::ReadWrite);
    assert_eq!(SqlClass::Dangerous.minimum_mode(), Mode::Full);
    assert_eq!(SqlClass::Unclassifiable.minimum_mode(), Mode::Full);
    assert!(policy::allows(Mode::ReadWrite, SqlClass::ScopedWrite));
    assert!(!policy::allows(Mode::Readonly, SqlClass::ScopedWrite));
    assert!(!policy::allows(Mode::ReadWrite, SqlClass::Dangerous));
}

#[test]
fn broad_write_flag() {
    assert!(policy::is_broad_write("DELETE FROM t"));
    assert!(policy::is_broad_write("UPDATE t SET a = 1 WHERE 1 = 1"));
    assert!(!policy::is_broad_write("DELETE FROM t WHERE id = 3"));
}

// ------------------------------------------------------------------ 配置与鉴权

#[test]
fn config_defaults_are_safe() {
    let cfg = mcp::Config::default();
    assert!(!cfg.enabled);
    assert_eq!(cfg.default_mode, Mode::Readonly);
    assert_eq!(cfg.port, mcp::DEFAULT_PORT);
    assert!(!cfg.token.is_empty());
    assert!(cfg.connection_scope_all);
}

#[test]
fn config_clamps_rows() {
    let cfg = mcp::Config::default();
    assert_eq!(cfg.clamp_rows(None), mcp::DEFAULT_MAX_ROWS);
    assert_eq!(cfg.clamp_rows(Some(0)), mcp::DEFAULT_MAX_ROWS);
    assert_eq!(cfg.clamp_rows(Some(50)), 50);
    assert_eq!(cfg.clamp_rows(Some(5000)), mcp::DEFAULT_HARD_CAP);
}

#[test]
fn config_connection_scope() {
    let mut cfg = mcp::Config::default();
    assert!(cfg.visible_connection("anything"));
    cfg.connection_scope_all = false;
    cfg.connections = vec!["dev".into()];
    assert!(cfg.visible_connection("dev"));
    assert!(!cfg.visible_connection("prod"));
}

#[test]
fn bearer_check_accepts_prefix_and_bare_token() {
    let hv = |s: &str| axum::http::HeaderValue::from_str(s).unwrap();
    assert!(mcp::bearer_ok(Some(&hv("Bearer abc")), "abc"));
    assert!(mcp::bearer_ok(Some(&hv("abc")), "abc"));
    assert!(!mcp::bearer_ok(Some(&hv("Bearer abd")), "abc"));
    assert!(!mcp::bearer_ok(Some(&hv("Bearer ")), "abc"));
    assert!(!mcp::bearer_ok(None, "abc"));
    assert!(!mcp::bearer_ok(Some(&hv("Bearer abc")), ""));
}

// ------------------------------------------------------------------ 真起后台进程

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("daro-mcpd-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Daemon {
    child: Child,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn write_json(path: &Path, value: &Value) {
    daro_rs::daemon::write_atomic(path, &value.to_string()).unwrap();
}

fn update_json(path: &Path, f: impl FnOnce(&mut Value)) {
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    f(&mut value);
    write_json(path, &value);
}

fn read_heartbeat(data: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(data.join("mcpd.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn wait_listening(data: &Path, port: u16, timeout: Duration) -> Value {
    let deadline = Instant::now() + timeout;
    let mut last = None;
    while Instant::now() < deadline {
        if let Some(beat) = read_heartbeat(data) {
            if beat["listening"].as_bool() == Some(true) && beat["port"].as_u64() == Some(port as u64) {
                return beat;
            }
            last = Some(beat);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("daro-mcpd did not start listening on {port}; last heartbeat: {last:?}");
}

struct Reply {
    status: u16,
    session: Option<String>,
    body: String,
}

fn rpc(
    url: &str,
    token: Option<&str>,
    session: Option<&str>,
    payload: &Value,
) -> Reply {
    let mut req = ureq::post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-06-18");
    if let Some(token) = token {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    if let Some(session) = session {
        req = req.header("mcp-session-id", session);
    }
    let mut resp = req
        .config()
        .http_status_as_error(false)
        .build()
        .send(payload.to_string())
        .expect("http send");
    let status = resp.status().as_u16();
    let session = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    Reply { status, session, body }
}

fn json_body(reply: &Reply) -> Value {
    // 客户端同时接受 JSON 和 SSE，服务端可能按 SSE 回：逐条 data 帧里取 JSON。
    for line in reply.body.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            if let Ok(value) = serde_json::from_str::<Value>(data.trim()) {
                return value;
            }
        }
    }
    serde_json::from_str(&reply.body).unwrap_or_else(|e| {
        panic!("bad json ({e}) status={} body={:?}", reply.status, reply.body)
    })
}

fn tool_text(value: &Value) -> String {
    value["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn tool_error_code(value: &Value) -> Option<String> {
    if value["result"]["isError"].as_bool() != Some(true) {
        return None;
    }
    let payload: Value = serde_json::from_str(&tool_text(value)).ok()?;
    payload["error"]["code"].as_str().map(str::to_owned)
}

fn sqlite_fixture(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\
         INSERT INTO items (name) VALUES ('alpha'), ('beta'), ('gamma');",
    )
    .unwrap();
}

#[test]
fn daemon_end_to_end() {
    let dir = TempDir::new();
    let data = dir.path().to_path_buf();
    let db_path = data.join("fixture.sqlite");
    sqlite_fixture(&db_path);

    let connections = json!({
        "version": 1,
        "connections": [{
            "name": "fixture-sqlite",
            "kind": "SQLite",
            "host": "",
            "port": 0,
            "database": db_path.to_string_lossy(),
            "user": "",
            "password": "",
            "group": "",
            "connected": false,
            "demo": false
        }]
    });
    write_json(&data.join("connections.json"), &connections);

    let port = free_port();
    let token = "test-token-0123456789";
    write_json(
        &data.join("mcp.json"),
        &json!({ "enabled": true, "port": port, "token": token }),
    );

    let child = Command::new(env!("CARGO_BIN_EXE_daro-mcpd"))
        .env("DARO_DATA_DIR", &data)
        .spawn()
        .expect("spawn daro-mcpd");
    let mut daemon = Daemon { child };
    wait_listening(&data, port, Duration::from_secs(20));

    let url = format!("http://127.0.0.1:{port}/mcp");

    // 没令牌直接 401
    let unauthorized = rpc(
        &url,
        None,
        None,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    );
    assert_eq!(unauthorized.status, 401, "missing token must be rejected");

    // 初始化 + initialized
    let init = rpc(
        &url,
        Some(token),
        None,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "daro-test", "version": "0"}
            }
        }),
    );
    assert_eq!(init.status, 200, "initialize failed: {:?}", init.body);
    let session = init.session.expect("initialize returns mcp-session-id");

    let notified = rpc(
        &url,
        Some(token),
        Some(&session),
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    assert!(
        notified.status == 200 || notified.status == 202,
        "initialized notification: {} {:?}",
        notified.status,
        notified.body
    );

    let list = rpc(
        &url,
        Some(token),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    );
    assert_eq!(list.status, 200);
    let list = json_body(&list);
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for expected in [
        "daro_list_connections",
        "daro_list_databases",
        "daro_list_schemas",
        "daro_list_tables",
        "daro_describe_table",
        "daro_execute_query",
        "daro_preview_table",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}: {names:?}");
    }

    // 连接清单里只有连接信息，没有口令字段
    let conns = rpc(
        &url,
        Some(token),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"daro_list_connections","arguments":{}}
        }),
    );
    let conns = json_body(&conns);
    assert_eq!(tool_error_code(&conns), None, "{conns}");
    let conns_text = tool_text(&conns);
    assert!(conns_text.contains("fixture-sqlite"), "{conns_text}");
    assert!(!conns_text.contains("password\""), "password must not be exposed: {conns_text}");

    // 列库 / 列表
    let databases = rpc(
        &url,
        Some(token),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"daro_list_databases","arguments":{"connection":"fixture-sqlite"}}
        }),
    );
    let databases = json_body(&databases);
    assert_eq!(tool_error_code(&databases), None, "{databases}");
    assert!(tool_text(&databases).contains("main"), "{}", tool_text(&databases));

    let tables = rpc(
        &url,
        Some(token),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":5,"method":"tools/call",
            "params":{"name":"daro_list_tables","arguments":{"connection":"fixture-sqlite","database":"main"}}
        }),
    );
    let tables = json_body(&tables);
    assert_eq!(tool_error_code(&tables), None, "{tables}");
    assert!(tool_text(&tables).contains("items"), "{}", tool_text(&tables));

    // 只读档：查询放行，写入被模式拒绝
    let select = rpc(
        &url,
        Some(token),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":6,"method":"tools/call",
            "params":{"name":"daro_execute_query","arguments":{
                "connection":"fixture-sqlite","database":"main","sql":"SELECT id, name FROM items ORDER BY id"
            }}
        }),
    );
    let select = json_body(&select);
    assert_eq!(tool_error_code(&select), None, "{select}");
    let select_text = tool_text(&select);
    assert!(select_text.contains("alpha"), "{select_text}");

    let denied = rpc(
        &url,
        Some(token),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":7,"method":"tools/call",
            "params":{"name":"daro_execute_query","arguments":{
                "connection":"fixture-sqlite","database":"main",
                "sql":"INSERT INTO items (name) VALUES ('delta')"
            }}
        }),
    );
    let denied = json_body(&denied);
    assert_eq!(
        tool_error_code(&denied).as_deref(),
        Some("MODE_DENIED"),
        "{denied}"
    );

    // 热改模式：下一句就按 readWrite 判，不用重启进程
    update_json(&data.join("mcp.json"), |cfg| {
        cfg["defaultMode"] = json!("readWrite");
    });
    let inserted = rpc(
        &url,
        Some(token),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":8,"method":"tools/call",
            "params":{"name":"daro_execute_query","arguments":{
                "connection":"fixture-sqlite","database":"main",
                "sql":"INSERT INTO items (name) VALUES ('delta')"
            }}
        }),
    );
    let inserted = json_body(&inserted);
    assert_eq!(tool_error_code(&inserted), None, "{inserted}");
    assert!(tool_text(&inserted).contains("affectedRows"), "{}", tool_text(&inserted));

    let check = rusqlite::Connection::open(&db_path).unwrap();
    let count: i64 = check
        .query_row("SELECT COUNT(*) FROM items WHERE name = 'delta'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "insert should be committed");

    // 全表删除在 readWrite 档仍是 WHERE_TOO_BROAD
    let broad = rpc(
        &url,
        Some(token),
        Some(&session),
        &json!({
            "jsonrpc":"2.0","id":9,"method":"tools/call",
            "params":{"name":"daro_execute_query","arguments":{
                "connection":"fixture-sqlite","database":"main","sql":"DELETE FROM items"
            }}
        }),
    );
    let broad = json_body(&broad);
    assert_eq!(
        tool_error_code(&broad).as_deref(),
        Some("WHERE_TOO_BROAD"),
        "{broad}"
    );

    // 换令牌：旧令牌 1 秒内失效，新令牌可用
    let rotated = "rotated-token-9876543210";
    update_json(&data.join("mcp.json"), |cfg| {
        cfg["token"] = json!(rotated);
    });
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let old = rpc(
            &url,
            Some(token),
            Some(&session),
            &json!({"jsonrpc":"2.0","id":10,"method":"tools/list"}),
        );
        if old.status == 401 {
            break;
        }
        assert!(Instant::now() < deadline, "old token still accepted after rotation");
        std::thread::sleep(Duration::from_millis(100));
    }
    let fresh = rpc(
        &url,
        Some(rotated),
        Some(&session),
        &json!({"jsonrpc":"2.0","id":11,"method":"tools/list"}),
    );
    assert_eq!(fresh.status, 200, "new token rejected: {:?}", fresh.body);

    // 关掉开关：后台进程自己收工
    update_json(&data.join("mcp.json"), |cfg| {
        cfg["enabled"] = json!(false);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match daemon.child.try_wait().unwrap() {
            Some(_) => break,
            None if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            None => panic!("daro-mcpd kept running after disable"),
        }
    }
}

/// 安装包卸载前跑的 `--uninstall`：在跑的后台进程必须收工，开关落成关闭，
/// 否则安装目录里的 daro-mcpd.exe 删不掉。
#[test]
fn uninstall_stops_running_daemon() {
    let dir = TempDir::new();
    let data = dir.path().to_path_buf();
    let port = free_port();
    write_json(
        &data.join("mcp.json"),
        &json!({ "enabled": true, "port": port, "token": "uninstall-token" }),
    );

    let child = Command::new(env!("CARGO_BIN_EXE_daro-mcpd"))
        .env("DARO_DATA_DIR", &data)
        .spawn()
        .expect("spawn daro-mcpd");
    let mut daemon = Daemon { child };
    wait_listening(&data, port, Duration::from_secs(20));

    let out = Command::new(env!("CARGO_BIN_EXE_daro-mcpd"))
        .env("DARO_DATA_DIR", &data)
        .arg("--uninstall")
        .output()
        .expect("run --uninstall");
    assert!(
        out.status.success(),
        "--uninstall failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match daemon.child.try_wait().unwrap() {
            Some(_) => break,
            None if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            None => panic!("daro-mcpd kept running after --uninstall"),
        }
    }

    let cfg: Value =
        serde_json::from_str(&std::fs::read_to_string(data.join("mcp.json")).unwrap()).unwrap();
    assert_eq!(cfg["enabled"], json!(false), "uninstall must leave the switch off");
}

/// 没人在跑时 `--uninstall` 也是成功的空操作，别让安装包因为「没东西可停」报错。
#[test]
fn uninstall_is_idempotent() {
    let dir = TempDir::new();
    let data = dir.path().to_path_buf();
    write_json(
        &data.join("mcp.json"),
        &json!({ "enabled": true, "port": free_port(), "token": "idle-token" }),
    );

    let out = Command::new(env!("CARGO_BIN_EXE_daro-mcpd"))
        .env("DARO_DATA_DIR", &data)
        .arg("--uninstall")
        .output()
        .expect("run --uninstall");
    assert!(
        out.status.success(),
        "--uninstall failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let cfg: Value =
        serde_json::from_str(&std::fs::read_to_string(data.join("mcp.json")).unwrap()).unwrap();
    assert_eq!(cfg["enabled"], json!(false));
    assert!(read_heartbeat(&data).is_none(), "no daemon should leave a heartbeat");
}

/// 自动更新要覆盖 daro-mcpd.exe：装之前先让后台进程退场，否则安装器停在
/// 「无法关闭应用程序」静默中止，更新看起来像没装。收尾必须把开关注销回原值，
/// 不然一次更新就把用户的 MCP 悄悄关掉了。
#[test]
fn update_pause_stops_daemon_and_keeps_switch_on() {
    let dir = TempDir::new();
    let data = dir.path().to_path_buf();
    let port = free_port();
    write_json(
        &data.join("mcp.json"),
        &json!({ "enabled": true, "port": port, "token": "pause-token" }),
    );

    let child = Command::new(env!("CARGO_BIN_EXE_daro-mcpd"))
        .env("DARO_DATA_DIR", &data)
        .spawn()
        .expect("spawn daro-mcpd");
    let mut daemon = Daemon { child };
    wait_listening(&data, port, Duration::from_secs(20));

    // 库函数按进程环境找数据目录；跑完立刻还原，别影响别的用例
    unsafe { std::env::set_var("DARO_DATA_DIR", &data) };
    let paused = mcp::pause_for_update();
    unsafe { std::env::remove_var("DARO_DATA_DIR") };
    paused.expect("pause_for_update");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match daemon.child.try_wait().unwrap() {
            Some(_) => break,
            None if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            None => panic!("daro-mcpd kept running after pause_for_update"),
        }
    }

    let cfg: Value =
        serde_json::from_str(&std::fs::read_to_string(data.join("mcp.json")).unwrap()).unwrap();
    assert_eq!(cfg["enabled"], json!(true), "更新后开关要保持原值");
}

