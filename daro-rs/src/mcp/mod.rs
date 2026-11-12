//! MCP 服务：把 daro 已存连接的数据能力通过 Streamable HTTP 暴露给外部 agent
//! （Qoder / Claude Desktop 等）。
//!
//! 服务跑在独立的后台进程 `daro-mcpd` 里，默认**关闭**；用户在「工具 ▸ 选项… ▸
//! MCP 服务」里打开后，主程序登记登录自启并拉起它。主程序只写 `mcp.json`、读后台进程写的
//! `mcpd.json` 心跳，两边不按 PID 管对方。
//!
//! 三条底线：
//! - 只听 `127.0.0.1`；
//! - 每个请求都要带 `Authorization: Bearer <token>`；
//! - agent 拿不到数据库口令——工具只按连接名借用 daro 已存的凭据，输出里也
//!   不回传密码。

pub mod policy;
pub mod store;
mod tools;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    Router,
};
use rmcp::transport::{
    streamable_http_server::{session::local::LocalSessionManager, StreamableHttpService},
    StreamableHttpServerConfig,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::daemon;
use tools::DaroMcp;

/// 缺省端口。39117 被 OneCRT 占了，daro 用紧挨着的下一个。
pub const DEFAULT_PORT: u16 = 39118;

pub const DEFAULT_MAX_ROWS: usize = 100;
pub const DEFAULT_HARD_CAP: usize = 1000;

const INSTRUCTIONS: &str = "daro is a desktop database client holding saved connections. \
Call daro_list_connections first, then walk databases/schemas/tables with daro_list_databases / \
daro_list_schemas / daro_list_tables. daro_execute_query runs one statement: read-only SQL is \
allowed in readonly mode; INSERT and scoped UPDATE/DELETE need the readWrite mode; DDL and \
unscoped writes need full access. Passwords are never accepted or returned — tools reuse the \
credentials daro already stores for each connection.";

// ------------------------------------------------------------------ 配置与运行态

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// 策略文件版本；只增不减，升级不得放大既有权限
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    /// 服务开关：同时决定登录自启与是否拉起后台进程
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "random_token")]
    pub token: String,
    /// 默认执行档：只读 / 数据读写 / 完全访问
    #[serde(default)]
    pub default_mode: policy::Mode,
    /// 单次查询缺省行数上限
    #[serde(default = "default_max_rows")]
    pub max_rows: usize,
    /// 硬上限：agent 传再大的 maxRows 也夹到这里
    #[serde(default = "default_hard_cap")]
    pub hard_row_cap: usize,
    /// 连接范围：true = 所有已存连接可见
    #[serde(default = "default_true")]
    pub connection_scope_all: bool,
    /// 收紧连接范围时的白名单
    #[serde(default)]
    pub connections: Vec<String>,
}

fn schema_version() -> u32 {
    1
}
fn default_port() -> u16 {
    DEFAULT_PORT
}
fn default_max_rows() -> usize {
    DEFAULT_MAX_ROWS
}
fn default_hard_cap() -> usize {
    DEFAULT_HARD_CAP
}
fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            schema_version: schema_version(),
            enabled: false,
            port: DEFAULT_PORT,
            token: random_token(),
            default_mode: policy::Mode::Readonly,
            max_rows: DEFAULT_MAX_ROWS,
            hard_row_cap: DEFAULT_HARD_CAP,
            connection_scope_all: true,
            connections: Vec::new(),
        }
    }
}

impl Config {
    /// 把明显不合法的值收回可用范围：端口 < 1024、行数 0、硬上限小于默认值。
    fn sanitized(mut self) -> Self {
        if self.port < 1024 {
            self.port = DEFAULT_PORT;
        }
        if self.hard_row_cap == 0 {
            self.hard_row_cap = DEFAULT_HARD_CAP;
        }
        if self.max_rows == 0 || self.max_rows > self.hard_row_cap {
            self.max_rows = self.hard_row_cap;
        }
        self
    }

    pub fn clamp_rows(&self, wanted: Option<usize>) -> usize {
        let n = wanted.unwrap_or(self.max_rows);
        if n == 0 {
            self.max_rows.min(self.hard_row_cap)
        } else {
            n.min(self.hard_row_cap)
        }
    }

    pub fn visible_connection(&self, name: &str) -> bool {
        self.connection_scope_all || self.connections.iter().any(|n| n == name)
    }
}

fn config_path() -> Result<PathBuf, String> {
    Ok(store::data_dir()?.join("mcp.json"))
}

fn heartbeat_path() -> Result<PathBuf, String> {
    Ok(store::data_dir()?.join("mcpd.json"))
}

/// 读配置。文件缺失或坏掉（含 token 为空）都退回默认值并覆盖写回，
/// 免得一个残缺文件把服务永久卡在没有可用 token 的状态。
fn load_config(path: &Path) -> Result<Config, String> {
    if path.exists() {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        if let Ok(cfg) = serde_json::from_str::<Config>(&text) {
            if !cfg.token.is_empty() {
                return Ok(cfg.sanitized());
            }
        }
    }
    let cfg = Config::default();
    save_config(&cfg, path)?;
    Ok(cfg)
}

fn save_config(cfg: &Config, path: &Path) -> Result<(), String> {
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    daemon::write_atomic(path, &text)
}

/// 当前配置；每次工具调用都现读，改档 / 改令牌不必重启客户端。
pub fn current_config() -> Result<Config, String> {
    load_config(&config_path()?)
}

fn random_token() -> String {
    let mut buf = [0u8; 24];
    getrandom::fill(&mut buf).expect("系统随机源不可用");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// 后台进程每秒写一次的心跳。不带令牌——界面要令牌直接读 mcp.json。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Heartbeat {
    pub pid: u32,
    /// 实际绑上的端口
    pub port: u16,
    pub listening: bool,
    pub error: Option<String>,
    /// 写文件的 Unix 秒
    pub at: u64,
    /// 正在处理的工具调用数
    #[serde(default)]
    pub active: usize,
}

impl Heartbeat {
    fn is_live(&self, now: u64) -> bool {
        daemon::is_live(self.at, now)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// 开关状态与「有没有在监听」是两件事
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    pub token: String,
    pub url: String,
    pub config_file: String,
    pub error: Option<String>,
    pub active: usize,
}

// ------------------------------------------------------------------ 界面侧

pub fn client_status() -> Status {
    let cfg_file = config_path().unwrap_or_else(|_| PathBuf::from("mcp.json"));
    status_at(&cfg_file, &cfg_file.with_file_name("mcpd.json"))
}

fn status_at(cfg_file: &Path, hb_file: &Path) -> Status {
    let (cfg, config_err) = match load_config(cfg_file) {
        Ok(cfg) => (cfg, None),
        Err(e) => (
            Config::default(),
            Some(format!("mcp.json 读不了，本次按默认配置处理: {e}")),
        ),
    };
    // 只采信还活着的心跳：残留文件会把界面钉在早已没人听的旧端口或旧错误上
    let beat = read_heartbeat_at(hb_file).filter(|b| b.is_live(store::now_secs()));
    let port = beat.as_ref().map(|b| b.port).unwrap_or(cfg.port);
    Status {
        enabled: cfg.enabled,
        running: beat.as_ref().is_some_and(|b| b.listening),
        port,
        token: cfg.token.clone(),
        url: format!("http://127.0.0.1:{port}/mcp"),
        config_file: cfg_file.display().to_string(),
        error: config_err.or_else(|| beat.as_ref().and_then(|b| b.error.clone())),
        active: beat.map(|b| b.active).unwrap_or(0),
    }
}

fn update_config(f: impl FnOnce(&mut Config)) -> Result<Config, String> {
    let path = config_path()?;
    let mut cfg = load_config(&path)?;
    f(&mut cfg);
    cfg = cfg.sanitized();
    save_config(&cfg, &path)?;
    Ok(cfg)
}

/// 有活着的心跳就算有人在跑，哪怕它正卡在端口被占的重试上。
fn daemon_alive() -> bool {
    heartbeat_path()
        .ok()
        .and_then(|p| read_heartbeat_at(&p))
        .is_some_and(|b| b.is_live(store::now_secs()))
}

fn ensure_running() -> Result<(), String> {
    if daemon_alive() {
        return Ok(());
    }
    daemon::spawn()
}

/// 开：落盘 + 登记登录自启 + 没人在跑就立刻拉起后台进程。
/// 关：落盘 + 撤掉自启；正在跑的后台进程下一秒读到开关关掉会自己收工。
pub fn set_enabled(enabled: bool) -> Result<Status, String> {
    update_config(|cfg| cfg.enabled = enabled)?;
    let mut note = None;
    if enabled {
        if let Err(e) = daemon::enable_autostart() {
            note = Some(e);
        }
        if let Err(e) = ensure_running() {
            note = note.or(Some(e));
        }
    } else {
        let _ = daemon::disable_autostart();
    }
    let mut status = client_status();
    if note.is_some() {
        status.error = note;
    }
    Ok(status)
}

/// 改端口只落盘：在跑的后台进程下一秒重绑新端口。
pub fn set_port(port: u16) -> Result<Status, String> {
    if port < 1024 {
        return Err("端口要大于等于 1024".into());
    }
    let cfg = update_config(|cfg| cfg.port = port)?;
    if cfg.enabled {
        ensure_running()?;
    }
    Ok(client_status())
}

/// 改执行档只落盘：后台进程逐请求现读配置，下一句查询立刻按新档判定。
pub fn set_mode(mode: policy::Mode) -> Result<Status, String> {
    update_config(|cfg| cfg.default_mode = mode)?;
    Ok(client_status())
}

/// 换令牌只落盘：鉴权逐请求取最新值，不用重绑端口。
pub fn rotate_token() -> Result<Status, String> {
    update_config(|cfg| cfg.token = random_token())?;
    Ok(client_status())
}

/// 主程序启动时补齐后台服务的状态：开关是开的就得有自启登记和一个在跑的进程。
/// 自启登记只在发布构建里做，免得开发期 `cargo run` 把 target/debug 的路径写进系统启动项。
pub fn boot() -> Result<(), String> {
    if !current_config()?.enabled {
        return Ok(());
    }
    if !cfg!(debug_assertions) {
        let _ = daemon::enable_autostart();
    }
    ensure_running()
}

/// 卸载前的收尾：把开关落成关闭，让在跑的后台进程自己退；宽限期过了还活着，
/// 就按心跳里的 PID 结束它。安装包在删文件前调用，否则正在运行的
/// `daro-mcpd.exe` 删不掉，用户会看到服务还在跑。
pub fn shutdown_for_uninstall() -> Result<(), String> {
    let path = config_path()?;
    // 配置坏了也要继续停进程，别让一条坏 JSON 卡住卸载
    if let Ok(mut cfg) = load_config(&path) {
        if cfg.enabled {
            cfg.enabled = false;
            save_config(&cfg, &path)?;
        }
    }
    let Some(pid) = heartbeat_path()
        .ok()
        .and_then(|p| read_heartbeat_at(&p))
        .filter(|b| b.is_live(store::now_secs()))
        .map(|b| b.pid)
    else {
        return Ok(());
    };
    if pid == std::process::id() || wait_gone(pid, Duration::from_secs(3)) {
        return Ok(());
    }
    terminate_peer(pid)
}

/// 安装更新前让后台进程退场：安装包要覆盖 `daro-mcpd.exe`，而它不归 Restart
/// Manager 管，被占用时安装器会停在「无法关闭应用程序」——`/SUPPRESSMSGBOXES`
/// 默认选「中止」，安装直接失败，用户看到的是「下载完、重启后还是旧版」。
///
/// 先落成关闭让进程自己退，宽限期过了按心跳 PID 结束；装完主程序启动时
/// `boot()` 会按开关重新拉起，所以收尾要把开关恢复原值。
pub fn pause_for_update() -> Result<(), String> {
    let path = config_path()?;
    let enabled = load_config(&path).map(|cfg| cfg.enabled).unwrap_or(false);
    let pid = heartbeat_path()
        .ok()
        .and_then(|p| read_heartbeat_at(&p))
        .filter(|b| b.is_live(store::now_secs()))
        .map(|b| b.pid);
    if enabled {
        let mut cfg = load_config(&path)?;
        cfg.enabled = false;
        save_config(&cfg, &path)?;
    }
    if let Some(pid) = pid
        && pid != std::process::id()
        && !wait_gone(pid, Duration::from_secs(3))
    {
        terminate_peer(pid)?;
    }
    if enabled {
        let mut cfg = load_config(&path)?;
        cfg.enabled = true;
        save_config(&cfg, &path)?;
    }
    Ok(())
}

fn wait_gone(pid: u32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !process_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !process_alive(pid)
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let code = WaitForSingleObject(handle, 0);
        CloseHandle(handle);
        code == WAIT_TIMEOUT
    }
}

/// 只结束心跳里报出来的那个 PID，并先核对镜像名。PID 被复用时不至于误伤别的程序。
#[cfg(windows)]
fn terminate_peer(pid: u32) -> Result<(), String> {
    use std::ffi::OsStr;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
    };
    unsafe {
        let probe = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if probe.is_null() {
            return Err(format!("打不开后台进程 {pid}"));
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let named = QueryFullProcessImageNameW(probe, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(probe);
        let is_daemon = named != 0
            && Path::new(&String::from_utf16_lossy(&buf[..len as usize]))
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case(OsStr::new(daemon::BIN)));
        if !is_daemon {
            return Err(format!("心跳里的 PID {pid} 不是 {}，不结束它", daemon::BIN));
        }
        let handle = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return Err(format!("打不开后台进程 {pid}"));
        }
        let killed = TerminateProcess(handle, 1);
        if killed != 0 {
            WaitForSingleObject(handle, 3000);
        }
        CloseHandle(handle);
        if killed == 0 {
            return Err(format!("结束后台进程 {pid} 失败"));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn process_alive(pid: u32) -> bool {
    // 不能用 `kill -0`：僵尸进程也回成功。后台进程是自己退出的，父进程（界面或
    // 测试进程）没来得及收尸时它会一直挂着 Z 状态，卸载收尾就会误判「还在跑」。
    let out = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output();
    match out {
        Ok(out) if out.status.success() => {
            let stat = String::from_utf8_lossy(&out.stdout);
            let stat = stat.trim();
            !stat.is_empty() && !stat.starts_with('Z')
        }
        _ => false,
    }
}

#[cfg(not(windows))]
fn terminate_peer(pid: u32) -> Result<(), String> {
    for signal in ["-TERM", "-KILL"] {
        let ok = std::process::Command::new("kill")
            .args([signal, &pid.to_string()])
            .status()
            .is_ok_and(|status| status.success());
        if ok && wait_gone(pid, Duration::from_secs(1)) {
            return Ok(());
        }
    }
    Err(format!("结束后台进程 {pid} 失败"))
}

// ------------------------------------------------------------------ 后台进程侧

const TICK: Duration = Duration::from_secs(1);

struct Runtime {
    cfg: Config,
    cfg_file: PathBuf,
    server: Option<(CancellationToken, tokio::task::JoinHandle<()>)>,
    /// 监听任务自己报的退出原因（serve 出错或任务 panic）
    serve_fail: Arc<Mutex<Option<String>>>,
    /// 鉴权中间件每次请求现读的令牌：换令牌不重启监听（重启会在同一端口撞 10048）
    token: Arc<RwLock<String>>,
    /// 心跳里的 active：正在处理的工具调用数
    active: Arc<AtomicUsize>,
    error: Option<String>,
}

type Shared = Arc<Mutex<Runtime>>;

fn new_runtime(path: PathBuf) -> Shared {
    let (cfg, error) = match load_config(&path) {
        Ok(cfg) => (cfg, None),
        Err(e) => (
            Config::default(),
            Some(format!("mcp.json 读不了，本次按默认配置处理: {e}")),
        ),
    };
    let token = Arc::new(RwLock::new(cfg.token.clone()));
    Arc::new(Mutex::new(Runtime {
        cfg,
        cfg_file: path,
        server: None,
        serve_fail: Arc::new(Mutex::new(None)),
        token,
        active: Arc::new(AtomicUsize::new(0)),
        error,
    }))
}

/// 后台进程主循环：按 mcp.json 监听，每秒跟一次配置改动，开关关掉就自己收工。
pub async fn run() -> Result<(), String> {
    let state = new_runtime(config_path()?);
    let hb_file = heartbeat_path()?;
    if let Some(beat) = read_heartbeat_at(&hb_file) {
        // 开机自启已经拉起过一个了，界面再点「启动」不该开出第二个
        if beat.is_live(store::now_secs()) && beat.pid != std::process::id() {
            return Ok(());
        }
    }
    if !state.lock().unwrap().cfg.enabled {
        // 开关关掉之后残留的自启项不该继续监听
        return Ok(());
    }

    serve(&state).await;
    write_heartbeat(&state);
    loop {
        tokio::time::sleep(TICK).await;
        if !reload(&state).await || peer_listens(&state) {
            break;
        }
        write_heartbeat(&state);
    }
    // 收工前再写一次，界面立刻显示「未监听」，不用等心跳过期
    write_heartbeat(&state);
    Ok(())
}

/// 该监听却没有活着的任务时补一个。端口被占时不退出：原因写进 error，下一秒再来。
async fn serve(state: &Shared) {
    reap(state);
    if state.lock().unwrap().cfg.enabled {
        let _ = start(state).await;
    }
}

fn reap(state: &Shared) {
    let mut rt = state.lock().unwrap();
    let dead = matches!(&rt.server, Some((_, join)) if join.is_finished());
    if !dead {
        return;
    }
    rt.server = None;
    let fail = rt.serve_fail.lock().unwrap().take();
    let old = rt.error.take();
    rt.error = Some(fail.or(old).unwrap_or_else(|| "监听任务已退出".to_string()));
}

/// 自己没在监听、却有另一个进程报「我在监听」时安静退出。
fn peer_listens(state: &Shared) -> bool {
    if matches!(&state.lock().unwrap().server, Some((_, join)) if !join.is_finished()) {
        return false;
    }
    heartbeat_path()
        .ok()
        .and_then(|p| read_heartbeat_at(&p))
        .is_some_and(|b| {
            b.pid != std::process::id() && b.is_live(store::now_secs()) && b.listening
        })
}

/// 把 mcp.json 的改动落到运行态：令牌直接换、端口重绑、开关关掉就整体退出。
async fn reload(state: &Shared) -> bool {
    let (path, prev) = {
        let rt = state.lock().unwrap();
        (rt.cfg_file.clone(), rt.cfg.clone())
    };
    // 读不到就当这一秒什么都没变：一个写坏的配置文件不该把在跑的服务带倒
    let next = match load_config(&path) {
        Ok(cfg) => cfg,
        Err(_) => return true,
    };
    if next.token != prev.token {
        *state.lock().unwrap().token.write().unwrap() = next.token.clone();
    }
    let port_changed = next.port != prev.port;
    let enabled = next.enabled;
    state.lock().unwrap().cfg = next;
    if !enabled {
        stop(state).await;
        return false;
    }
    if port_changed {
        stop(state).await;
    }
    serve(state).await;
    true
}

fn write_heartbeat(state: &Shared) {
    let beat = heartbeat_of(&state.lock().unwrap());
    if let (Ok(path), Ok(text)) = (heartbeat_path(), serde_json::to_string(&beat)) {
        let _ = daemon::write_atomic(&path, &text);
    }
}

fn heartbeat_of(rt: &Runtime) -> Heartbeat {
    let task_err = match &rt.server {
        Some((_, join)) if join.is_finished() => Some(
            rt.serve_fail
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| "监听任务已退出".to_string()),
        ),
        _ => None,
    };
    Heartbeat {
        pid: std::process::id(),
        port: rt.cfg.port,
        listening: matches!(&rt.server, Some((_, join)) if !join.is_finished()),
        error: rt.error.clone().or(task_err),
        at: store::now_secs(),
        active: rt.active.load(Ordering::Relaxed),
    }
}

fn read_heartbeat_at(path: &Path) -> Option<Heartbeat> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Heartbeat>(&text).ok()
}

async fn start(state: &Shared) -> Result<(), String> {
    let (port, token, active, serve_fail) = {
        let rt = state.lock().unwrap();
        if rt.server.is_some() {
            return Ok(());
        }
        (
            rt.cfg.port,
            rt.token.clone(),
            rt.active.clone(),
            rt.serve_fail.clone(),
        )
    };

    let addr = format!("127.0.0.1:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            let msg = format!("监听 {addr} 失败: {e}");
            state.lock().unwrap().error = Some(msg.clone());
            return Err(msg);
        }
    };
    let actual_port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let ct = CancellationToken::new();
    let service = StreamableHttpService::new(
        move || Ok(DaroMcp::new(active.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default()
            .with_cancellation_token(ct.child_token())
            .with_json_response(true),
    );
    let app = Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(token, guard));

    let join = tokio::spawn(async move {
        // 不做 graceful shutdown：stop() abort 这个任务，正在跑的 SSE 随连接一起断
        if let Err(e) = axum::serve(listener, app).await {
            *serve_fail.lock().unwrap() = Some(format!("监听退出: {e}"));
        }
    });

    let mut rt = state.lock().unwrap();
    rt.server = Some((ct, join));
    *rt.serve_fail.lock().unwrap() = None;
    rt.cfg.port = actual_port;
    rt.error = None;
    Ok(())
}

async fn stop(state: &Shared) {
    let old = state.lock().unwrap().server.take();
    if let Some((ct, join)) = old {
        ct.cancel();
        join.abort();
        // 非等到任务真退出不可：listener 就在那个任务手里，没释放就重绑同一个端口，
        // 实测会撞上 "通常每个套接字地址只允许使用一次 (os error 10048)"。
        let _ = join.await;
    }
}

/// 只认 `Authorization: Bearer <token>`。绑在回环上，本机其它进程照样连得上，
/// 这一层不能省。
async fn guard(
    State(cell): State<Arc<RwLock<String>>>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = cell.read().unwrap().clone();
    if bearer_ok(headers.get(axum::http::header::AUTHORIZATION), &token) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// 各家客户端都发 `Bearer <token>`，容忍裸 token；空值一律拒。
pub fn bearer_ok(header: Option<&axum::http::HeaderValue>, token: &str) -> bool {
    let raw = header.and_then(|v| v.to_str().ok()).unwrap_or("");
    let given = raw.strip_prefix("Bearer ").unwrap_or(raw);
    !given.is_empty() && !token.is_empty() && given == token
}
