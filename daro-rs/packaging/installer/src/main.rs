#![windows_subsystem = "windows"]

//! daro-rs 的自包含 Windows 安装器。
//! 把 daro-rs.exe 编译进自身（见 build.rs / DARO_EMBED），运行时：
//!   - 默认安装到 %LOCALAPPDATA%\daro-rs（无需管理员权限 / UAC）
//!   - 在开始菜单创建快捷方式
//!   - 在 HKCU 写入卸载信息（控制面板“程序和功能”可卸载）
//!   - 复制自身为 uninstall.exe 作为卸载程序
//!
//! 用法：
//!   daro-installer.exe            安装
//!   daro-installer.exe --uninstall 卸载
//!   daro-installer.exe --dir X    安装到指定目录（替代默认 LocalAppData）
//!   daro-installer.exe --extract D 仅把内置的 daro-rs.exe 解压到 D（调试/校验用）

use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const APP_NAME: &str = "daro-rs";
const DISPLAY_NAME: &str = "daro-rs 数据库管理工具";
const PUBLISHER: &str = "daro";
const VERSION: &str = env!("CARGO_PKG_VERSION");

// 由 build.rs 生成的 embed.rs 提供：static DARO_EXE: &[u8]
include!(env!("DARO_EMBED"));

// --silent：CI / 测试用，不弹窗，仅以退出码表达成败。
static SILENT: AtomicBool = AtomicBool::new(false);

fn to_wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

/// 诊断日志：追加写入安装器同目录的 daro-installer.log（写不进则退回 %TEMP%）。
/// 出问题时把该文件内容发给维护者即可定位根因。
fn diag_log(msg: &str) {
    let path = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("daro-installer.log")))
        .unwrap_or_else(|| env::temp_dir().join("daro-installer.log"));
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{secs}s] {msg}");
    }
}

/// 返回诊断日志文件的路径（用于在错误提示里告知用户）。
fn diag_log_path() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("daro-installer.log")))
        .unwrap_or_else(|| env::temp_dir().join("daro-installer.log"))
}

fn msgbox(title: &str, text: &str, error: bool) {
    if SILENT.load(Ordering::SeqCst) {
        return;
    }
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
    };
    let flags = if error {
        MB_OK | MB_ICONERROR
    } else {
        MB_OK | MB_ICONINFORMATION
    };
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            to_wide(text).as_ptr(),
            to_wide(title).as_ptr(),
            flags,
        );
    }
}

fn install_dir() -> Result<PathBuf, String> {
    let args: Vec<String> = env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--dir") {
        // 取 --dir 之后第一个非 flag 参数作为目标目录，与 flag 顺序无关
        if let Some(p) = args[pos + 1..].iter().find(|a| !a.starts_with('-')).cloned() {
            return Ok(PathBuf::from(p));
        }
    }
    let local = dirs::data_local_dir().ok_or("无法确定 LocalAppData 目录")?;
    Ok(local.join(APP_NAME))
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();

    // 调试/校验：仅解压内置 exe（取 --extract 之后第一个非 flag 参数作为目录）
    if let Some(pos) = args.iter().position(|a| a == "--extract") {
        if let Some(dir) = args[pos + 1..].iter().find(|a| !a.starts_with('-')).cloned() {
            let d = PathBuf::from(dir);
            fs::create_dir_all(&d).map_err(|e| e.to_string())?;
            let out = d.join(format!("{APP_NAME}.exe"));
            fs::write(&out, DARO_EXE).map_err(|e| e.to_string())?;
            msgbox(
                "提取完成",
                &format!("已提取内置 daro-rs.exe 到：\n{out}", out = out.display()),
                false,
            );
            return Ok(());
        } else {
            return Err("--extract 需要一个目标目录参数".into());
        }
    }

    let is_uninstall = args.iter().any(|a| a == "--uninstall" || a == "/uninstall");
    if is_uninstall {
        uninstall()
    } else {
        install()
    }
}

fn install() -> Result<(), String> {
    let mut dir = install_dir()?;
    diag_log(&format!(
        "install begin, version={VERSION}, target dir = {}",
        dir.display()
    ));
    fs::create_dir_all(&dir).map_err(|e| {
        diag_log(&format!("create_dir_all({}) failed: {e}", dir.display()));
        format!("创建安装目录失败：{e}")
    })?;

    // 安装前先终止可能占用的旧进程，避免覆盖被锁定的可执行文件
    kill_existing();

    let mut exe_dest = dir.join(format!("{APP_NAME}.exe"));
    if let Err(first_err) = write_exe_with_retry(&exe_dest) {
        diag_log(&format!("default dir failed: {first_err}"));
        msgbox(
            "选择安装目录",
            &format!(
                "默认安装位置无法写入：\n{first_err}\n\n点击“确定”后请选择另一个安装目录；\n若想继续用默认位置，请先关闭正在运行的 daro-rs 再重试。",
            ),
            true,
        );
        match pick_folder(&format!("选择 {DISPLAY_NAME} 的安装目录")) {
            Some(chosen) => {
                diag_log(&format!("user picked dir = {}", chosen.display()));
                dir = chosen;
                fs::create_dir_all(&dir).map_err(|e| format!("创建安装目录失败：{e}"))?;
                exe_dest = dir.join(format!("{APP_NAME}.exe"));
                write_exe_with_retry(&exe_dest).map_err(|e2| {
                    format!("{e2}\n\n详细诊断日志：\n{}", diag_log_path().display())
                })?;
            }
            None => {
                diag_log("folder picker cancelled or failed");
                return Err(format!(
                    "已取消选择安装目录。\n原始错误：{first_err}\n\n详细诊断日志：\n{}",
                    diag_log_path().display()
                ));
            }
        }
    }

    finish_install(&dir, &exe_dest)
}

/// 写入内置 exe；先探测目录可写性（区分「目录无权限」与「exe 被占用/被拦截」），
/// 再杀占用进程、清只读属性并重试。失败时返回带上下文的错误描述。
fn write_exe_with_retry(dest: &Path) -> Result<(), String> {
    let dir = dest.parent().unwrap_or_else(|| Path::new("."));
    diag_log(&format!("write target = {}", dest.display()));

    // 用临时探针文件判断目录本身是否可写，便于区分两类失败原因
    let probe = dir.join("daro-write-probe.tmp");
    let probe_result = fs::write(&probe, b"t");
    let _ = fs::remove_file(&probe);
    if let Err(e) = &probe_result {
        diag_log(&format!("probe write failed: {e}"));
        return Err(format!("目录无写入权限：{}（{e}）", dir.display()));
    }
    diag_log("probe write ok: directory is writable");

    for attempt in 1..=6 {
        if attempt > 1 {
            kill_existing();
        }
        clear_readonly(dest);
        match fs::write(dest, DARO_EXE) {
            Ok(_) => {
                diag_log(&format!("write ok on attempt {attempt}"));
                return Ok(());
            }
            Err(e) => diag_log(&format!("attempt {attempt} failed: {e}")),
        }
        sleep(Duration::from_millis(400));
    }
    Err(format!(
        "目录可写，但写入 {} 被拒绝（已重试 6 次）。多半是杀毒软件拦截了未签名 exe 的写入，或目标文件被无法终止的进程锁定。",
        dest.display()
    ))
}

/// 终止可能正在运行的 daro-rs 进程，以便覆盖其文件（best-effort）。
fn kill_existing() {
    let st = Command::new("taskkill")
        .args(["/IM", &format!("{APP_NAME}.exe"), "/F"])
        .status();
    diag_log(&format!(
        "taskkill daro-rs.exe -> exit {:?}",
        st.ok().and_then(|s| s.code())
    ));
}

/// 若目标文件被标记为只读，先清除只读属性，否则 Windows 会拒绝覆盖。
fn clear_readonly(p: &Path) {
    if let Ok(meta) = fs::metadata(p) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = fs::set_permissions(p, perms);
        }
    }
}

/// 原生文件夹选择框（shell32 的 SHBrowseForFolderW），不依赖 PowerShell、
/// 不存在 STA/运行空间等脚本环境导致的「弹不出来」问题。
/// 返回用户选择的目录；用户取消则返回 None。
fn pick_folder(title: &str) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows_sys::Win32::UI::Shell::{
        SHBrowseForFolderW, SHGetPathFromIDListW, BROWSEINFOW, BIF_NEWDIALOGSTYLE,
        BIF_RETURNONLYFSDIRS,
    };

    // BIF_NEWDIALOGSTYLE 要求调用线程先初始化 COM
    // （注意：CoInitializeEx 的 dwcoinit 形参是 u32，而 COINIT_* 常量带 i32 类型标注）
    let hr = unsafe {
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32)
    };
    if hr != 0 && hr != 1 {
        diag_log(&format!("CoInitializeEx failed: hr={hr}"));
    }
    let com_inited = hr == 0 || hr == 1; // S_OK / S_FALSE

    let title_w = to_wide(title);
    let mut display_name = [0u16; 1024];
    let mut path_buf = [0u16; 1024];

    let mut bi: BROWSEINFOW = unsafe { std::mem::zeroed() };
    bi.hwndOwner = std::ptr::null_mut();
    bi.pidlRoot = std::ptr::null_mut();
    bi.pszDisplayName = display_name.as_mut_ptr();
    bi.lpszTitle = title_w.as_ptr();
    bi.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE;
    bi.lpfn = None;
    bi.lParam = 0;
    bi.iImage = 0;

    let pidl = unsafe { SHBrowseForFolderW(&bi) };
    let result = if pidl.is_null() {
        diag_log("folder picker: user cancelled");
        None
    } else {
        let ok = unsafe { SHGetPathFromIDListW(pidl, path_buf.as_mut_ptr()) };
        unsafe { CoTaskMemFree(pidl as *const core::ffi::c_void) };
        if ok != 0 {
            let len = path_buf.iter().position(|&c| c == 0).unwrap_or(0);
            if len > 0 {
                Some(PathBuf::from(String::from_utf16_lossy(&path_buf[..len])))
            } else {
                diag_log("picked path is empty");
                None
            }
        } else {
            diag_log("SHGetPathFromIDListW failed");
            None
        }
    };

    if com_inited {
        unsafe { CoUninitialize() };
    }
    result
}

/// 写入卸载程序、开始菜单快捷方式与卸载注册表，并提示安装完成。
fn finish_install(dir: &Path, exe_dest: &Path) -> Result<(), String> {
    // 复制自身作为卸载程序
    let self_exe = env::current_exe().map_err(|e| format!("获取自身路径失败：{e}"))?;
    let uninstaller = dir.join("uninstall.exe");
    fs::copy(&self_exe, &uninstaller).map_err(|e| format!("写入卸载程序失败：{e}"))?;

    // 开始菜单快捷方式（失败不阻断安装）
    if let Err(e) = create_shortcut(exe_dest, dir) {
        eprintln!("创建开始菜单快捷方式失败（已忽略）：{e}");
    }

    // 卸载注册表（失败不阻断安装）
    if let Err(e) = write_uninstall_registry(dir, &uninstaller) {
        eprintln!("写入卸载注册表失败（已忽略）：{e}");
    }

    msgbox(
        "安装完成",
        &format!(
            "{DISPLAY_NAME} 已安装到：\n{dir}\n开始菜单已添加快捷方式。",
            dir = dir.display()
        ),
        false,
    );
    Ok(())
}

fn create_shortcut(target: &Path, workdir: &Path) -> Result<(), String> {
    let appdata =
        env::var("APPDATA").map_err(|_| "未找到 APPDATA 环境变量".to_string())?;
    let programs = Path::new(&appdata)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    fs::create_dir_all(&programs).map_err(|e| e.to_string())?;
    let lnk = programs.join(format!("{APP_NAME}.lnk"));

    let script = format!(
        "$Wsh = New-Object -ComObject WScript.Shell\n\
         $lnk = $Wsh.CreateShortcut('{lnk}')\n\
         $lnk.TargetPath = '{target}'\n\
         $lnk.WorkingDirectory = '{workdir}'\n\
         $lnk.Description = '{DISPLAY_NAME}'\n\
         $lnk.IconLocation = '{target},0'\n\
         $lnk.Save()\n",
        lnk = lnk.display(),
        target = target.display(),
        workdir = workdir.display(),
    );

    let tmp = env::temp_dir().join(format!("{APP_NAME}-mklink.ps1"));
    let mut content: Vec<u8> = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
    content.extend_from_slice(script.as_bytes());
    fs::write(&tmp, content).map_err(|e| e.to_string())?;

    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            tmp.to_str().unwrap_or(""),
        ])
        .status()
        .map_err(|e| format!("启动 powershell 失败：{e}"))?;
    let _ = fs::remove_file(&tmp);

    if !status.success() {
        return Err("创建开始菜单快捷方式失败（powershell 返回非零）".into());
    }
    Ok(())
}

fn write_uninstall_registry(dir: &Path, uninstaller: &Path) -> Result<(), String> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\daro-rs";
    let exe = dir.join(format!("{APP_NAME}.exe"));
    let entries = [
        ("DisplayName", DISPLAY_NAME.to_string()),
        (
            "UninstallString",
            format!("\"{}\" --uninstall", uninstaller.display()),
        ),
        ("DisplayIcon", format!("\"{}\",0", exe.display())),
        ("InstallLocation", dir.display().to_string()),
        ("Publisher", PUBLISHER.to_string()),
        ("DisplayVersion", VERSION.to_string()),
        ("NoModify", "1".to_string()),
        ("NoRepair", "1".to_string()),
    ];
    for (name, val) in entries {
        let status = Command::new("reg")
            .args(["add", key, "/v", name, "/t", "REG_SZ", "/d", &val, "/f"])
            .status()
            .map_err(|e| format!("写入注册表失败：{e}"))?;
        if !status.success() {
            return Err(format!("写入注册表项 {name} 失败"));
        }
    }
    Ok(())
}

fn uninstall() -> Result<(), String> {
    let dir = install_dir()?;

    // 关闭可能运行中的程序
    let _ = Command::new("taskkill")
        .args(["/IM", &format!("{APP_NAME}.exe"), "/F"])
        .status();

    // 删除开始菜单快捷方式
    if let Ok(appdata) = env::var("APPDATA") {
        let lnk = Path::new(&appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join(format!("{APP_NAME}.lnk"));
        let _ = fs::remove_file(&lnk);
    }

    // 删除卸载注册表项
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\daro-rs";
    let _ = Command::new("reg").args(["delete", key, "/f"]).status();

    // 删除安装目录（含自身 uninstall.exe）
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }

    msgbox(
        "卸载完成",
        &format!("{DISPLAY_NAME} 已从以下位置移除：\n{dir}", dir = dir.display()),
        false,
    );
    Ok(())
}

fn main() {
    if env::args().any(|a| a == "--silent") {
        SILENT.store(true, Ordering::SeqCst);
    }
    if let Err(e) = run() {
        if SILENT.load(Ordering::SeqCst) {
            let _ = fs::write(
                env::temp_dir().join("daro-installer.err.log"),
                format!("daro-installer 失败：{e}"),
            );
        }
        msgbox("daro-rs 安装", &format!("操作失败：\n{e}"), true);
        std::process::exit(1);
    }
}
