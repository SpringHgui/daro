//! 版本检查与自动更新（移植自 OneCRT `update.rs`）：查 GitHub 上版本号最高的
//! 发布，下载 Windows 安装包并静默装上。
//!
//! 与 OneCRT 的差异：daro 的发布标签是 `daro-rs-v*`（同仓还有 Flutter 版的 `v*`
//! 标签，必须排除）；发布走 pre-release，所以不过滤 prerelease；只认带本平台
//! 安装包的发布（早期只发过 zip，那种发布没法自动装，直接跳过）。

use std::cmp::Ordering;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// 不能用 `/releases/latest`：它按「最近一次公开」挑，补发一个旧版本的 draft 就会顶掉最新版，
/// 界面于是看到的版本比当前还旧。这里拉列表自己取版本号最大的。
const ENDPOINT: &str = "https://api.github.com/repos/SpringHgui/daro/releases?per_page=100";
const USER_AGENT: &str = concat!("daro/", env!("CARGO_PKG_VERSION"));

#[derive(Clone)]
pub struct Latest {
    pub version: String,
    pub url: String,
    /// 当前平台安装包的直链
    pub asset: String,
    /// 发布说明（Markdown，GitHub 发布正文）
    pub notes: String,
    /// 发布时间，ISO 8601；老发布可能没有
    pub published_at: Option<String>,
}

/// 更新日志里的一条版本记录（含当前已装版本，不只看有没有更新）。
#[derive(Clone)]
pub struct ReleaseInfo {
    pub version: String,
    pub url: String,
    pub notes: String,
    pub published_at: Option<String>,
}

/// 一次检查的结果：有新版本时 `latest` 是它，`releases` 供更新日志列出历史版本。
#[derive(Default)]
pub struct Catalog {
    pub latest: Option<Latest>,
    pub releases: Vec<ReleaseInfo>,
}

/// 下载中的一次进度上报。
pub struct DownloadProgress {
    pub downloaded: u64,
    /// 服务端给的总字节数；没给就是 None，此时进度条只能显示已下载量
    pub total: Option<u64>,
    /// 最近一段时间的平均速度，字节/秒
    pub bytes_per_sec: f64,
    /// 本次从残留半截文件的这个位置继续（`Range` 起点）；0 表示从头下
    pub resumed_from: u64,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    published_at: Option<String>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// 拉发布列表：有比 `current` 更新的发布时 `catalog.latest` 是它，`catalog.releases`
/// 总是带上可安装的历史版本（供更新日志展示，最新在前）。出错交给调用方决定怎么提示。
pub fn check(current: &str) -> Result<Catalog, String> {
    // 目前只出 Windows x64 安装包，其它平台没有可自动安装的资产
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Ok(Catalog::default());
    }
    let mut response = agent()
        .get(ENDPOINT)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let releases: Vec<Release> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let mut list = candidates(&releases);
    list.sort_by(|a, b| compare(&b.info.version, &a.info.version));
    let latest = list
        .first()
        .filter(|candidate| newer(&candidate.info.version, current))
        .map(|candidate| Latest {
            version: candidate.info.version.clone(),
            url: candidate.info.url.clone(),
            asset: candidate.asset.clone(),
            notes: candidate.info.notes.clone(),
            published_at: candidate.info.published_at.clone(),
        });
    Ok(Catalog {
        latest,
        releases: list.into_iter().map(|candidate| candidate.info).collect(),
    })
}

/// 下载安装包到临时目录，返回落地路径。
///
/// 半截文件一直留在临时目录里（`.part` 后缀），中途失败、关掉窗口甚至重启应用后
/// 再来都会带 `Range` 续传；只有整份下完才改名成正式文件名。
pub fn download(
    latest: &Latest,
    cancel: &AtomicBool,
    report: impl Fn(DownloadProgress),
) -> Result<PathBuf, String> {
    let url = latest.asset.as_str();
    let name = url
        .rsplit('/')
        .next()
        .unwrap_or("daro-rs-setup")
        .to_string();
    let dir = std::env::temp_dir().join("daro-update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建临时目录失败：{e}"))?;
    let dest = dir.join(name);
    // 同名安装包之前已经整份下完（可能只是装到一半失败），直接复用。
    // 进度仍要上报，否则界面会从「正在连接」直接跳到安装。
    if let Ok(meta) = std::fs::metadata(&dest)
        && meta.len() > 0
    {
        report(DownloadProgress {
            downloaded: meta.len(),
            total: Some(meta.len()),
            bytes_per_sec: 0.0,
            resumed_from: 0,
        });
        return Ok(dest);
    }
    let partial = dest.with_extension("part");
    let mut offset = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);

    let agent = download_agent();
    let mut response = fetch(&agent, url, offset).map_err(|e| format!("下载失败：{e}"))?;
    let mut status = response.status().as_u16();
    if status == 416 {
        // 服务端说这个起点已经到头了，说明残留的半截文件对不上，只能整份重下
        offset = 0;
        response = fetch(&agent, url, 0).map_err(|e| format!("下载失败：{e}"))?;
        status = response.status().as_u16();
    }
    if status != 200 && status != 206 {
        return Err(format!("下载失败：HTTP {status}"));
    }
    // 200 表示服务端没按 Range 给，只能从头写
    if status == 200 {
        offset = 0;
    }
    let total = if status == 206 {
        content_range_total(&response)
    } else {
        header_u64(&response, "content-length")
    };

    if offset == 0 {
        // append 模式不会截断，重下前先把旧文件清掉
        let _ = std::fs::remove_file(&partial);
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&partial)
        .map_err(|e| format!("写临时文件失败：{e}"))?;

    // 安装包几十 MB，默认 10MB 上限不够
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(256 * 1024 * 1024)
        .reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut downloaded = offset;
    let mut speed = 0.0;
    let mut window_start = Instant::now();
    let mut window_bytes = downloaded;
    let resumed_from = offset;
    report(DownloadProgress {
        downloaded,
        total,
        bytes_per_sec: speed,
        resumed_from,
    });
    loop {
        if cancel.load(AtomicOrdering::Relaxed) {
            return Err("下载已取消".to_string());
        }
        let read = reader
            .read(&mut buf)
            .map_err(|e| format!("下载失败：{e}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buf[..read])
            .map_err(|e| format!("写临时文件失败：{e}"))?;
        downloaded += read as u64;
        let elapsed = window_start.elapsed();
        if elapsed >= Duration::from_millis(250) {
            let instant = (downloaded - window_bytes) as f64 / elapsed.as_secs_f64();
            // 短窗口的瞬时值抖得厉害，拿旧值压一压
            speed = if speed == 0.0 {
                instant
            } else {
                speed * 0.5 + instant * 0.5
            };
            report(DownloadProgress {
                downloaded,
                total,
                bytes_per_sec: speed,
                resumed_from,
            });
            window_start = Instant::now();
            window_bytes = downloaded;
        }
    }
    let _ = file.flush();
    drop(file);

    // 少给字节就别拿去装：续传接错位置时这是唯一能发现的判据
    if let Some(total) = total {
        if downloaded != total {
            return Err(format!("下载不完整：{downloaded} / {total} 字节"));
        }
    }

    if dest.exists() {
        let _ = std::fs::remove_file(&dest);
    }
    std::fs::rename(&partial, &dest).map_err(|e| format!("保存安装包失败：{e}"))?;
    report(DownloadProgress {
        downloaded,
        total,
        bytes_per_sec: speed,
        resumed_from,
    });
    Ok(dest)
}

/// 拿响应头而不是让 ureq 把 4xx 变成错误：416 要靠状态码判断续传起点是否失效。
fn fetch(
    agent: &ureq::Agent,
    url: &str,
    offset: u64,
) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    let mut request = agent.get(url).header("User-Agent", USER_AGENT);
    if offset > 0 {
        request = request.header("Range", format!("bytes={offset}-"));
    }
    request.config().http_status_as_error(false).build().call()
}

fn header_u64(response: &ureq::http::Response<ureq::Body>, name: &str) -> Option<u64> {
    response
        .headers()
        .get(name)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// `Content-Range: bytes 100-999/1000` 里的总长度。
fn content_range_total(response: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    let value = response.headers().get("content-range")?.to_str().ok()?;
    parse_content_range_total(value)
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.trim().parse().ok()
}

/// 安装器失败时把退出码写在这个文件里，下次开更新中心时用来解释「为什么还是旧版」。
const STATUS_FILE: &str = "update-status.txt";

/// 拉起安装程序：先等本进程退出（安装包要覆盖正在运行的文件），再静默安装，最后启动新版本。
#[cfg(target_os = "windows")]
pub fn install(setup: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    // daro-mcpd 是独立后台进程，装主程序时它不会跟着退；它占着 daro-mcpd.exe
    // 又不在 Restart Manager 的管辖里，安装器会停在「无法关闭应用程序」并按
    // /SUPPRESSMSGBOXES 默认中止（退出码 5）。先让它退场，装完主程序启动时
    // boot() 会按原开关重新拉起。
    if let Err(e) = daro_rs::mcp::pause_for_update() {
        eprintln!("[update] 更新前停 MCP 后台进程失败：{e}");
    }

    let log = setup.with_extension("log");
    let status = setup.with_file_name(STATUS_FILE);
    let script = install_script(setup, &install_exe(), &status, &log);
    let bat = setup.with_file_name("daro-update.cmd");
    std::fs::write(&bat, script.as_bytes()).map_err(|e| format!("写更新脚本失败：{e}"))?;
    std::process::Command::new("cmd")
        .args(["/c", &bat.to_string_lossy()])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .map_err(|e| format!("启动安装程序失败：{e}"))?;
    Ok(())
}

/// 更新脚本。整段命令不能直接塞进 `cmd /c`：Rust 会把内嵌引号转义成 `\"`，
/// cmd 不认这种转义，start 报“文件名、目录名或卷标语法不正确”。另外脚本里的
/// 中文路径要靠 chcp 65001 才能按 UTF-8 解出来。
///
/// `/FORCECLOSEAPPLICATIONS` 兜住其它还占着文件的进程；失败把安装器退出码
/// 落盘，重启后的界面才能说明「上次没装上」。重定向写在 echo 前面否则
/// `echo 5>"文件"` 会被当成句柄 5 的重定向。
#[cfg(target_os = "windows")]
fn install_script(setup: &Path, exe: &Path, status: &Path, log: &Path) -> String {
    format!(
        "@echo off\r\nchcp 65001 >nul\r\nping -n 3 127.0.0.1 >nul\r\n\
         del \"{status}\" 2>nul\r\n\
         start \"\" /wait \"{setup}\" /SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /FORCECLOSEAPPLICATIONS /LOG=\"{log}\"\r\n\
         if errorlevel 1 (>\"{status}\" echo %ERRORLEVEL%)\r\n\
         start \"\" \"{exe}\"\r\n",
        status = status.display(),
        setup = setup.display(),
        log = log.display(),
        exe = exe.display()
    )
}

/// 上次自动安装的退出码：只有安装包还在（说明这次更新没装上）且版本对得上才报。
#[cfg(target_os = "windows")]
pub fn install_failure(version: &str) -> Option<u32> {
    let dir = std::env::temp_dir().join("daro-update");
    let code = failure_code_from(&std::fs::read_to_string(dir.join(STATUS_FILE)).ok()?)?;
    dir.join(asset_name(version)).exists().then_some(code)
}

#[cfg(not(target_os = "windows"))]
pub fn install_failure(_version: &str) -> Option<u32> {
    None
}

fn failure_code_from(text: &str) -> Option<u32> {
    text.trim().parse().ok().filter(|code| *code != 0)
}

#[cfg(not(target_os = "windows"))]
pub fn install(_setup: &Path) -> Result<(), String> {
    Err("自动安装目前只做在 Windows 上".to_string())
}

fn agent() -> ureq::Agent {
    build_agent(Some(Duration::from_secs(120)))
}

/// 下载不设总时长：几十 MB 在慢代理下可能要跑几分钟，总时长会把正常下载拦腰截断。
/// 改成按“多久没收到数据”判挂，连接和单次读取各留上限。
fn download_agent() -> ureq::Agent {
    build_agent(None)
}

fn build_agent(global: Option<Duration>) -> ureq::Agent {
    // 代理沿用 ureq 默认取法（环境变量，Windows 上还有 Internet 选项）
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .timeout_global(global)
        .build()
        .into()
}

/// 发布页里安装包的文件名（Inno Setup per-user，免 UAC）。目前只出
/// Windows x64 安装包，平台门槛在 [check] 里把。
fn asset_name(version: &str) -> String {
    format!("daro-rs-setup-{version}-x64.exe")
}

/// 装完要启动的 exe：优先用卸载项里记的安装目录（用户装到别处也认），否则用 per-user 默认目录。
#[cfg(target_os = "windows")]
fn install_exe() -> PathBuf {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    // AppId 与 packaging/inno/daro-rs.iss 保持一致
    let subkey = wide(
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{8E2B1C7A-54D0-4BD3-9E6F-31A7C20D5B48}_is1",
    );
    let value = wide("InstallLocation");
    let mut buf = [0u16; 520];
    let mut size = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut _,
            &mut size,
        ) == ERROR_SUCCESS
    };
    if ok {
        let len = (size as usize / 2).saturating_sub(1);
        let dir = String::from_utf16_lossy(&buf[..len]);
        let dir = dir.trim_end_matches('\\');
        if !dir.is_empty() {
            return PathBuf::from(dir).join("daro-rs.exe");
        }
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Programs")
        .join("daro-rs")
        .join("daro-rs.exe")
}

struct Candidate {
    info: ReleaseInfo,
    asset: String,
}

/// 能自动安装的发布。只认 `daro-rs-v<数字…>` 标签（Flutter 版的 `v*` 标签、
/// 杂 tag 一律跳过），draft 跳过；daro 走 pre-release 发布，prerelease 不算排除项。
/// 没有本平台安装包的发布（早期只发 zip）也跳过——提示了也装不了。
fn candidates(releases: &[Release]) -> Vec<Candidate> {
    // 目前只有 Windows 出安装包；其他平台查到也装不了，干脆不提示更新。
    #[cfg(not(target_os = "windows"))]
    {
        let _ = releases;
        Vec::new()
    }
    #[cfg(target_os = "windows")]
    {
        releases
            .iter()
            .filter(|r| !r.draft)
            .filter_map(|r| {
                let tag = r.tag_name.strip_prefix("daro-rs-v")?;
                if !tag.starts_with(|c: char| c.is_ascii_digit()) {
                    return None;
                }
                let wanted = asset_name(tag);
                let asset = r.assets.iter().find(|a| a.name == wanted)?;
                Some(Candidate {
                    info: ReleaseInfo {
                        version: tag.to_string(),
                        url: r.html_url.clone(),
                        notes: r.body.clone(),
                        published_at: r.published_at.clone(),
                    },
                    asset: asset.browser_download_url.clone(),
                })
            })
            .collect()
    }
}

/// 列表里版本号最大的可安装发布（单测用）。
#[cfg(test)]
fn newest(releases: &[Release]) -> Option<Candidate> {
    candidates(releases)
        .into_iter()
        .max_by(|a, b| compare(&a.info.version, &b.info.version))
}

/// 只比较点分数字段；预发布后缀（`-beta.1` 等）按正式版对待。
fn newer(latest: &str, current: &str) -> bool {
    compare(latest, current) == Ordering::Greater
}

fn compare(a: &str, b: &str) -> Ordering {
    fn parts(v: &str) -> Vec<u64> {
        let core = v.split(['-', '+']).next().unwrap_or(v);
        core.split('.').map(|p| p.parse().unwrap_or(0)).collect()
    }
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        match a
            .get(i)
            .copied()
            .unwrap_or(0)
            .cmp(&b.get(i).copied().unwrap_or(0))
        {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::{newer, newest, Asset, Release};
    #[cfg(target_os = "windows")]
    use std::path::Path;

    fn release(tag: &str, draft: bool, assets: &[&str]) -> Release {
        Release {
            tag_name: tag.to_string(),
            html_url: format!("https://example.invalid/{tag}"),
            draft,
            assets: assets
                .iter()
                .map(|name| Asset {
                    name: name.to_string(),
                    browser_download_url: format!("https://example.invalid/{name}"),
                })
                .collect(),
            body: format!("{tag} 的更新说明"),
            published_at: Some("2026-11-11T10:00:00Z".to_string()),
        }
    }

    fn setup(version: &str) -> String {
        format!("daro-rs-setup-{version}-x64.exe")
    }

    #[test]
    fn compares_numeric_fields() {
        assert!(newer("0.3.1", "0.3.0"));
        assert!(newer("0.4.0", "0.3.9"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(!newer("0.3.0", "0.3.0"));
        assert!(!newer("0.2.1", "0.3.0"));
        assert!(!newer("0.3.0-beta.1", "0.3.0"));
        assert!(newer("0.3", "0.2.7"));
    }

    #[test]
    fn picks_highest_version_not_most_recently_published() {
        // 补发旧版本的 draft 会排在列表最前，但它不是最高版本
        let list = [
            release("daro-rs-v0.3.0", false, &[&setup("0.3.0")]),
            release("daro-rs-v0.4.0", false, &[&setup("0.4.0")]),
            release("daro-rs-v0.3.2", false, &[&setup("0.3.2")]),
        ];
        assert_eq!(newest(&list).unwrap().info.version, "0.4.0");
    }

    #[test]
    fn skips_drafts_junk_and_flutter_tags_but_keeps_prereleases() {
        // daro 的发布全是 pre-release，不能按 prerelease 过滤；
        // Flutter 版的 v* 标签不是 rust 发布，必须排除
        let list = [
            release("daro-rs-v0.5.0", true, &[&setup("0.5.0")]),
            release("v9.9.9", false, &["daro-9.9.9.apk"]),
            release("nightly", false, &[&setup("9.9.9")]),
            release("daro-rs-v0.4.0", false, &[&setup("0.4.0")]),
        ];
        assert_eq!(newest(&list).unwrap().info.version, "0.4.0");
        assert!(newest(&[]).is_none());
    }

    #[test]
    fn skips_releases_without_this_platforms_installer() {
        // 早期发布只有 zip（daro-rs-v0.0.4），版本号再高也没法自动装；
        // 带安装包的较低版本才是可更新目标
        let list = [
            release("daro-rs-v0.0.4", false, &["daro-rs-0.0.4-windows-x64.zip"]),
            release("daro-rs-v0.0.5", false, &[&setup("0.0.5"), "SHA256SUMS.txt"]),
        ];
        let found = newest(&list).unwrap();
        assert_eq!(found.info.version, "0.0.5");
        assert!(found.asset.ends_with(&setup("0.0.5")), "{}", found.asset);
        // 全是 zip 发布时没有可更新目标
        let zips = [release("daro-rs-v0.0.4", false, &["daro-rs-0.0.4.zip"])];
        assert!(newest(&zips).is_none());
    }

    #[test]
    fn reads_total_length_from_content_range() {
        assert_eq!(
            super::parse_content_range_total("bytes 1000-1999/2000"),
            Some(2000)
        );
        assert_eq!(super::parse_content_range_total("bytes 0-0/123"), Some(123));
        assert_eq!(super::parse_content_range_total("bytes 0-9/*"), None);
        assert_eq!(super::parse_content_range_total("garbage"), None);
    }

    #[test]
    fn reads_installer_failure_code() {
        assert_eq!(super::failure_code_from("5\r\n"), Some(5));
        assert_eq!(super::failure_code_from(" 2 "), Some(2));
        // 0 表示装成功，不当作失败
        assert_eq!(super::failure_code_from("0"), None);
        assert_eq!(super::failure_code_from(""), None);
        assert_eq!(super::failure_code_from("oops"), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn install_script_force_closes_blockers_and_records_failure() {
        let script = super::install_script(
            Path::new(r"C:\t\setup.exe"),
            Path::new(r"C:\Program Files\daro-rs\daro-rs.exe"),
            Path::new(r"C:\t\update-status.txt"),
            Path::new(r"C:\t\setup.log"),
        );
        // 后台进程占用文件时 Restart Manager 关不掉，必须强关，否则安装器静默中止
        assert!(script.contains("/FORCECLOSEAPPLICATIONS"));
        assert!(script.contains("/LOG=\"C:\\t\\setup.log\""));
        // 重定向在 echo 前，写的是安装器退出码
        assert!(
            script.contains("if errorlevel 1 (>\"C:\\t\\update-status.txt\" echo %ERRORLEVEL%)"),
            "{script}"
        );
        assert!(script.contains("start \"\" \"C:\\Program Files\\daro-rs\\daro-rs.exe\""));
    }

    /// 起一个只回一次的本地 HTTP 服务，记录请求里的 Range 头，回 206 剩余字节。
    fn serve_once(
        body: Vec<u8>,
        honor_range: bool,
    ) -> (
        String,
        std::sync::mpsc::Receiver<Option<String>>,
        std::thread::JoinHandle<()>,
    ) {
        use std::io::{BufRead, BufReader, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut range = None;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap() == 0 || header == "\r\n" {
                    break;
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("range:") {
                    range = Some(value.trim().to_string());
                }
            }
            let start = range
                .as_deref()
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| r.strip_suffix('-'))
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0);
            let (head, rest) = if honor_range && start > 0 {
                (
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        body.len() - start,
                        start,
                        body.len() - 1,
                        body.len()
                    ),
                    &body[start..],
                )
            } else {
                (
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    ),
                    &body[..],
                )
            };
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(rest).unwrap();
            let _ = tx.send(range);
        });
        (format!("http://{addr}"), rx, server)
    }

    fn resume_fixture(name: &str) -> (super::Latest, std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join("daro-update");
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join(name);
        let partial = dest.with_extension("part");
        let _ = std::fs::remove_file(&dest);
        let _ = std::fs::remove_file(&partial);
        (
            super::Latest {
                version: "0.0.0".to_string(),
                url: String::new(),
                asset: format!("http://127.0.0.1:1/{name}"),
                notes: String::new(),
                published_at: None,
            },
            dest,
            partial,
        )
    }

    /// 断点续传：残留半截文件时带 Range 请求，只把剩下的字节接在后面。
    #[test]
    fn resumes_partial_file_with_range_request() {
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let (base, range_rx, server) = serve_once(body.clone(), true);
        let name = format!("daro-resume-{}.bin", std::process::id());
        let (mut latest, dest, partial) = resume_fixture(&name);
        let offset = 120_000;
        std::fs::write(&partial, &body[..offset]).unwrap();
        latest.asset = format!("{base}/{name}");

        let cancel = std::sync::atomic::AtomicBool::new(false);
        let reports = std::sync::Mutex::new(Vec::new());
        let got = super::download(&latest, &cancel, |p| {
            reports.lock().unwrap().push((p.downloaded, p.resumed_from));
        })
        .unwrap();
        assert_eq!(std::fs::read(&got).unwrap(), body, "续传后的文件内容完整");
        assert_eq!(
            range_rx.recv().unwrap().as_deref(),
            Some(format!("bytes={offset}-").as_str()),
            "半截文件必须换成 Range 请求"
        );
        let reports = reports.lock().unwrap();
        assert!(
            reports.iter().any(|(downloaded, resumed)| *downloaded >= offset as u64
                && *resumed == offset as u64),
            "进度里要带上续传起点：{reports:?}"
        );
        server.join().unwrap();
        let _ = std::fs::remove_file(&dest);
    }

    /// 服务端不认 Range（回 200 整包）时，续传起点作废，从头覆盖写，不能接出双份内容。
    #[test]
    fn restarts_from_zero_when_server_ignores_range() {
        let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let (base, range_rx, server) = serve_once(body.clone(), false);
        let name = format!("daro-resume-ignore-{}.bin", std::process::id());
        let (mut latest, dest, partial) = resume_fixture(&name);
        std::fs::write(&partial, &body[..90_000]).unwrap();
        latest.asset = format!("{base}/{name}");

        let cancel = std::sync::atomic::AtomicBool::new(false);
        let got = super::download(&latest, &cancel, |_| {}).unwrap();
        assert_eq!(std::fs::read(&got).unwrap(), body, "忽略 Range 时必须整份重下");
        assert_eq!(
            range_rx.recv().unwrap().as_deref(),
            Some("bytes=90000-"),
            "先按续传试一次"
        );
        server.join().unwrap();
        let _ = std::fs::remove_file(&dest);
    }
}