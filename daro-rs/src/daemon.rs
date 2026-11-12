//! MCP 后台进程（daro-mcpd）与界面之间的握手：程序在哪、怎么拉起、开机自启怎么注册。
//!
//! 界面不监听 MCP 端口。`mcp.json` 是唯一的意图来源，后台进程每秒跟着改，并把
//! 「在不在监听」写进 `mcpd.json` 心跳；界面只看心跳新鲜度判断死活，因此不需要
//! 按 PID 杀进程——开关关掉后后台进程自己收工。

use std::path::{Path, PathBuf};
use std::process::Stdio;

/// 后台进程的文件名；安装包把它和主程序放在同一个目录里。
pub const BIN: &str = if cfg!(windows) {
    "daro-mcpd.exe"
} else {
    "daro-mcpd"
};

/// 自启项在系统启动项管理里显示的名字
pub const LABEL: &str = "Daro MCP";

/// 心跳超过这个秒数就算后台进程已经不在了。它每秒写一次，三倍余量留给调度抖动。
pub const STALE_SECS: u64 = 3;

/// Windows 的 Run 项：登录时启动。不做真正的开机启动——数据库口令是按用户存的，
/// 用户会话没起来之前后台进程也读不到自己的配置。
#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// 心跳还新鲜吗：`at` 是写文件的 Unix 秒
pub fn is_live(at: u64, now: u64) -> bool {
    now.saturating_sub(at) <= STALE_SECS
}

/// 写文件先落临时名再改名：读的一侧（界面读心跳、后台进程读配置）不会看到半截 JSON
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// 后台进程的可执行文件路径。与当前程序同目录，Windows/Linux 的安装目录和
/// macOS 的 `Daro.app/Contents/MacOS` 都满足这个假设。
pub fn exe_path() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = me
        .parent()
        .ok_or_else(|| "读不到当前程序所在目录".to_string())?;
    let path = dir.join(BIN);
    if path.exists() {
        Ok(path)
    } else {
        Err(format!("找不到后台服务程序：{}", path.display()))
    }
}

/// 拉起后台进程后不等它：主程序退出之后它还得继续服务。
pub fn spawn() -> Result<(), String> {
    let exe = exe_path()?;
    let mut cmd = std::process::Command::new(&exe);
    #[cfg(not(windows))]
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // 调试构建保留控制台，崩了看得见；发布构建加 CREATE_NO_WINDOW，免得从 GUI
    // 拉起来时闪一个黑窗（主程序自己是 GUI 子系统，没有控制台可继承）。
    #[cfg(windows)]
    if !cfg!(debug_assertions) {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("启动 {} 失败: {e}", exe.display()))
}

/// 登记开机自启：登录时把后台进程拉起来
pub fn enable_autostart() -> Result<(), String> {
    let exe = exe_path()?;
    autostart::enable(&exe)
}

/// 撤掉开机自启。本来就没登记过也算成功。
pub fn disable_autostart() -> Result<(), String> {
    autostart::disable()
}

/// Run 项里带空格的路径必须自己加引号，否则登录时被拆成「程序 + 参数」
#[cfg(windows)]
fn run_value(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

#[cfg(windows)]
mod autostart {
    use super::{RUN_KEY, LABEL, run_value};
    use std::path::Path;
    use windows_sys::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteKeyValueW,
        RegOpenKeyExW, RegSetKeyValueW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open_run_key() -> Result<HKEY, String> {
        let name = wide(RUN_KEY);
        let mut sub: HKEY = std::ptr::null_mut();
        let code = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, name.as_ptr(), 0, KEY_SET_VALUE, &mut sub) };
        if code != ERROR_SUCCESS {
            return Err(format!("打不开注册表 Run 项: {code}"));
        }
        Ok(sub)
    }

    pub fn enable(exe: &Path) -> Result<(), String> {
        let key = open_run_key()?;
        let value = wide(LABEL);
        let data = wide(&run_value(exe));
        let code = unsafe {
            RegSetKeyValueW(
                key,
                std::ptr::null(),
                value.as_ptr(),
                REG_SZ,
                data.as_ptr() as *const core::ffi::c_void,
                (data.len() * std::mem::size_of::<u16>()) as u32,
            )
        };
        unsafe { RegCloseKey(key) };
        if code != ERROR_SUCCESS {
            return Err(format!("写注册表 Run 项失败: {code}"));
        }
        Ok(())
    }

    pub fn disable() -> Result<(), String> {
        let key = open_run_key()?;
        let value = wide(LABEL);
        let code =
            unsafe { RegDeleteKeyValueW(key, std::ptr::null(), value.as_ptr()) };
        unsafe { RegCloseKey(key) };
        // 没登记过就没什么可删的
        if code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND {
            return Ok(());
        }
        if code != ERROR_SUCCESS {
            return Err(format!("删注册表 Run 项失败: {code}"));
        }
        Ok(())
    }
}

/// Linux：XDG autostart 目录里的 .desktop，登录时由桌面环境拉起
#[cfg(all(unix, not(target_os = "macos")))]
mod autostart {
    use super::{write_atomic, BIN, LABEL};
    use std::path::{Path, PathBuf};

    fn path() -> Result<PathBuf, String> {
        let base = dirs::config_dir().ok_or_else(|| "无法获取配置目录".to_string())?;
        Ok(base.join("autostart").join(format!("{BIN}.desktop")))
    }

    /// Exec 带引号：安装路径可能含空格
    pub fn content(exe: &Path) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName={LABEL}\nExec=\"{}\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        )
    }

    pub fn enable(exe: &Path) -> Result<(), String> {
        write_atomic(&path()?, &content(exe))
    }

    pub fn disable() -> Result<(), String> {
        let p = path()?;
        match std::fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("删 {} 失败: {e}", p.display())),
        }
    }
}

/// macOS：LaunchAgent，登录时由 launchd 拉起。
/// 不设 KeepAlive：关掉开关后后台进程是自己退出的，KeepAlive 会立刻把它拉回来。
#[cfg(target_os = "macos")]
mod autostart {
    use super::write_atomic;
    use std::path::{Path, PathBuf};

    const LABEL_ID: &str = "com.springh.daro.mcpd";

    fn path() -> Result<PathBuf, String> {
        let home = dirs::home_dir().ok_or_else(|| "无法获取用户目录".to_string())?;
        Ok(home
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{LABEL_ID}.plist")))
    }

    pub fn content(exe: &Path) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL_ID}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
            exe.display()
        )
    }

    /// 当场生效靠 launchctl；它失败也不报错——plist 已落盘，下次登录照样起来。
    fn launchctl(action: &str) {
        if let Ok(p) = path() {
            let _ = std::process::Command::new("launchctl")
                .args([action, "-w"])
                .arg(&p)
                .output();
        }
    }

    pub fn enable(exe: &Path) -> Result<(), String> {
        write_atomic(&path()?, &content(exe))?;
        launchctl("load");
        Ok(())
    }

    pub fn disable() -> Result<(), String> {
        launchctl("unload");
        let p = path()?;
        match std::fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("删 {} 失败: {e}", p.display())),
        }
    }
}

