//! daro 的 MCP 后台进程：不开界面，只按 `mcp.json` 监听回环端口。
//!
//! 由主程序按需拉起（登录自启 / 设置里打开开关），开关关掉后自己退出。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 安装包卸载前用这个开关让服务收工：先关配置开关等它自己退，超时才按心跳 PID 结束
    if std::env::args().nth(1).as_deref() == Some("--uninstall") {
        if let Err(e) = daro_rs::mcp::shutdown_for_uninstall() {
            eprintln!("daro-mcpd 卸载收尾失败: {e}");
            std::process::exit(1);
        }
        return;
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("daro-mcpd")
        .build()
        .expect("初始化 MCP 后台运行时失败");
    if let Err(e) = rt.block_on(daro_rs::mcp::run()) {
        eprintln!("daro-mcpd 退出: {e}");
        std::process::exit(1);
    }
}
