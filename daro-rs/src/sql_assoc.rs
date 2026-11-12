//! `.sql` 文件的系统关联：双击 / 「打开方式」/ 拖入窗口共用的入口。
//!
//! Windows 从 10 起不允许应用静默改默认程序（UserChoice 有校验），所以这里
//! 只把当前 exe 登记成 `.sql` 的「打开方式」候选，再打开系统的选择对话框；
//! 用户勾「始终」之后由系统自己记住。只写 HKCU，不需要管理员。

use std::path::{Path, PathBuf};

/// 是否 `.sql`（扩展名大小写不敏感）。
pub fn is_sql_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("sql"))
}

/// 命令行里带进来的 `.sql`：双击关联和「打开方式」都走这里。
/// 只留真实存在的文件，其他参数（安装器残留 / 开关）一律忽略。
pub fn sql_paths_from_args() -> Vec<PathBuf> {
    sql_files(std::env::args_os().skip(1).map(PathBuf::from))
}

/// 拖放与命令行共用的过滤：存在 + `.sql`。目录、快捷方式、别的类型都丢掉。
pub fn sql_files(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter(|p| p.is_file() && is_sql_file(p))
        .collect()
}

/// 剪贴板里由资源管理器「复制文件」带进来的 `.sql` 清单。
/// 剪贴板只有文字（复制 SQL 正文）时返回空，调用方照旧走文本粘贴。
pub fn clipboard_sql_files() -> Vec<PathBuf> {
    sql_files(clipboard_paths())
}

/// 读 SQL 文件正文：UTF-8 优先，读不出来按 GB18030 解（中文 Windows 的
/// ANSI 存盘很常见），开头的 BOM 去掉，免得贴在编辑器第一行里看不见。
pub fn read_sql_text(path: &Path) -> Result<String, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("无法读取 {}：{e}", path.display()))?;
    let text = match std::str::from_utf8(&bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GB18030.decode(&bytes).0.into_owned(),
    };
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

#[cfg(windows)]
fn clipboard_paths() -> Vec<PathBuf> {
    win::clipboard_paths()
}

#[cfg(not(windows))]
fn clipboard_paths() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(windows)]
mod win {
    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_NONE, REG_SZ, RegSetKeyValueW,
    };
    use windows_sys::Win32::UI::Shell::{
        OAIF_ALLOW_REGISTRATION, OAIF_REGISTER_EXT, OPENASINFO, SHCNE_ASSOCCHANGED, SHCNF_IDLIST,
        SHChangeNotify, SHOpenWithDialog,
    };

    const APP_KEY: &str = r"Software\Classes\Applications\daro-rs.exe";
    const PROGID: &str = "daro-rs.sql";
    const PROGID_KEY: &str = r"Software\Classes\daro-rs.sql";
    const TYPE_NAME: &str = "daro SQL 文件";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// `RegSetKeyValueW` 会顺手建出缺失的项，不用逐层 Open/Create。
    fn set_value(sub: &str, name: &str, ty: u32, data: &[u16]) -> Result<(), String> {
        let sub_w = wide(sub);
        let name_w = wide(name);
        let code = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                sub_w.as_ptr(),
                name_w.as_ptr(),
                ty,
                data.as_ptr() as *const core::ffi::c_void,
                (data.len() * std::mem::size_of::<u16>()) as u32,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(format!("写注册表 {sub} 失败: {code}"));
        }
        Ok(())
    }

    fn command_line(exe: &str) -> Vec<u16> {
        wide(&format!("\"{exe}\" \"%1\""))
    }

    pub fn register_open_with() -> Result<(), String> {
        let exe = std::env::current_exe()
            .map_err(|e| format!("取可执行文件路径失败: {e}"))?
            .display()
            .to_string();
        let cmd = command_line(&exe);
        let icon = wide(&format!("\"{exe}\",0"));

        // 「打开方式」里显示的名字与启动命令
        set_value(APP_KEY, "FriendlyAppName", REG_SZ, &wide("daro"))?;
        set_value(APP_KEY, "ApplicationName", REG_SZ, &wide("daro"))?;
        set_value(APP_KEY, "DefaultIcon", REG_SZ, &icon)?;
        set_value(&format!(r"{APP_KEY}\shell\open\command"), "", REG_SZ, &cmd)?;

        // 类型 ProgID：列表里显示成「daro SQL 文件」，而不是 exe 文件名
        set_value(PROGID_KEY, "", REG_SZ, &wide(TYPE_NAME))?;
        set_value(PROGID_KEY, "FriendlyTypeName", REG_SZ, &wide(TYPE_NAME))?;
        set_value(PROGID_KEY, "DefaultIcon", REG_SZ, &icon)?;
        set_value(&format!(r"{PROGID_KEY}\shell\open\command"), "", REG_SZ, &cmd)?;

        // 只挂候选，不写 .sql 的默认值：默认程序必须由用户在系统对话框里选
        set_value(r"Software\Classes\.sql\OpenWithProgids", PROGID, REG_NONE, &[])?;

        // 让资源管理器立刻刷新关联列表，不必重启
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
        Ok(())
    }

    /// 剪贴板里的文件清单（CF_HDROP）。资源管理器 Ctrl+C 复制文件时写的就是它；
    /// 拿不到（没复制文件 / 剪贴板被别的程序占着）就返回空。
    pub fn clipboard_paths() -> Vec<PathBuf> {
        use windows_sys::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        };
        use windows_sys::Win32::System::Ole::CF_HDROP;
        use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};

        let mut out = Vec::new();
        unsafe {
            if IsClipboardFormatAvailable(CF_HDROP as u32) == 0 {
                return out;
            }
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return out;
            }
            let handle = GetClipboardData(CF_HDROP as u32);
            if !handle.is_null() {
                let drop = handle as HDROP;
                let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0);
                for i in 0..count {
                    let len = DragQueryFileW(drop, i, std::ptr::null_mut(), 0);
                    if len == 0 {
                        continue;
                    }
                    let mut buf = vec![0u16; len as usize + 1];
                    let n = DragQueryFileW(drop, i, buf.as_mut_ptr(), buf.len() as u32);
                    if n > 0 {
                        out.push(PathBuf::from(String::from_utf16_lossy(
                            &buf[..n as usize],
                        )));
                    }
                }
            }
            CloseClipboard();
        }
        out
    }

    /// 打开系统的「打开方式」对话框，并把 daro 放进候选。
    /// 用户勾「始终使用此应用」后，以后双击 .sql 就归系统记住了。
    pub fn ask_default_app() -> Result<(), String> {
        register_open_with()?;
        // 对话框按扩展名列候选；给一个真实存在的占位文件最稳，且不执行它
        let placeholder =
            std::env::temp_dir().join(format!("daro-open-with-{}.sql", std::process::id()));
        std::fs::write(&placeholder, b"").map_err(|e| format!("创建临时 .sql 失败: {e}"))?;
        let file = wide(&placeholder.display().to_string());
        let class = wide(".sql");
        let info = OPENASINFO {
            pcszFile: file.as_ptr(),
            pcszClass: class.as_ptr(),
            oaifInFlags: OAIF_ALLOW_REGISTRATION | OAIF_REGISTER_EXT,
        };
        let hr = unsafe { SHOpenWithDialog(std::ptr::null_mut(), &info) };
        let _ = std::fs::remove_file(&placeholder);
        // 用户取消（ERROR_CANCELLED）不算失败
        const CANCELLED: i32 = 0x8007_04C7u32 as i32;
        if hr < 0 && hr != CANCELLED {
            return Err(format!("打开「打开方式」对话框失败: 0x{:08x}", hr as u32));
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use win::{ask_default_app, register_open_with};

/// 非 Windows：macOS 的关联写在 app bundle 的 Info.plist，Linux 走 MIME 包，
/// 都不是运行时能安全接管的，这里保持空操作。
#[cfg(not(windows))]
pub fn register_open_with() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_existing_sql_files() {
        let dir = std::env::temp_dir().join(format!("daro_sql_assoc_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sql = dir.join("a.SQL");
        let txt = dir.join("b.txt");
        let missing = dir.join("c.sql");
        std::fs::write(&sql, "-- hi").unwrap();
        std::fs::write(&txt, "x").unwrap();

        let got = sql_files(vec![sql.clone(), txt, missing, dir.clone()]);
        assert_eq!(got, vec![sql]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_sql_text_as_utf8_bom_or_gb18030() {
        let dir = std::env::temp_dir().join(format!("daro_sql_read_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // 带 BOM 的 UTF-8：BOM 不进正文
        let utf8 = dir.join("utf8.sql");
        std::fs::write(&utf8, b"\xEF\xBB\xBFselect 1;").unwrap();
        assert_eq!(read_sql_text(&utf8).unwrap(), "select 1;");

        // GB18030（「查询」= B2 E9 D1 AF）按 UTF-8 解不开，要回退
        let gbk = dir.join("gbk.sql");
        std::fs::write(&gbk, b"-- \xB2\xE9\xD1\xAF\nselect 1;").unwrap();
        assert_eq!(read_sql_text(&gbk).unwrap(), "-- 查询\nselect 1;");

        assert!(read_sql_text(&dir.join("missing.sql")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
