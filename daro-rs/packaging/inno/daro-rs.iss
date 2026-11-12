; daro-rs 的 Windows 安装包（Inno Setup 7）。
;
; 编译（推荐直接跑 packaging/build-installer.sh，它会从 Cargo.toml 取版本号）：
;   ISCC //DAppVersion=0.0.1 packaging\inno\daro-rs.iss
; Git Bash 里 /D 会被 MSYS 改写成本地路径，所以参数要写成双斜杠 //D。
;
; 安装模式：PrivilegesRequired=lowest —— 装进 %LOCALAPPDATA%\Programs\daro-rs，
; 全程不弹 UAC；卸载信息写 HKCU，控制面板「程序和功能」可见。

#define AppName "daro-rs"
#define AppDisplayName "daro-rs 数据库管理工具"
#define AppPublisher "daro"

#ifndef AppVersion
  #error 必须通过 //DAppVersion=<x.y.z> 传入版本号，见 packaging/build-installer.sh
#endif

; VersionInfoVersion 只接受纯数字段，带 -alpha 之类后缀会直接编译失败
#define ProjectDir AddBackslash(SourcePath) + "..\..\"
#define BinDir ProjectDir + "target\release"
#define ExePath BinDir + "\" + AppName + ".exe"
; MCP 后台进程：主程序按需拉起，安装时跟着放同一目录
#define DaemonExePath BinDir + "\daro-mcpd.exe"
#define LogoIco ProjectDir + "assets\icons\ui\logo.ico"

#ifndef OutputRoot
  #define OutputRoot AddBackslash(SourcePath) + "..\dist"
#endif

#if !FileExists(ExePath)
  #error 找不到 target\release\daro-rs.exe，请先执行 cargo build --release --bin daro-rs --bin daro-mcpd
#endif
#if !FileExists(DaemonExePath)
  #error 找不到 target\release\daro-mcpd.exe，请先执行 cargo build --release --bin daro-rs --bin daro-mcpd
#endif
#if !FileExists(LogoIco)
  #error 找不到 assets\icons\ui\logo.ico
#endif

[Setup]
AppId={{8E2B1C7A-54D0-4BD3-9E6F-31A7C20D5B48}
AppName={#AppDisplayName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=no
DisableWelcomePage=no
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
SetupIconFile={#LogoIco}
UninstallDisplayName={#AppDisplayName}
UninstallDisplayIcon={app}\{#AppName}.exe
Compression=lzma2/max
SolidCompression=yes
; 覆盖安装时若旧的 daro-rs.exe 正在运行，先弹出「正在使用的文件」页并结束进程，
; 否则 Windows 不允许覆盖正在运行的 exe（os error 5）
CloseApplications=yes
RestartApplications=no
VersionInfoVersion={#AppVersion}
VersionInfoProductName={#AppDisplayName}
VersionInfoProductVersion={#AppVersion}
VersionInfoCompany={#AppPublisher}
VersionInfoDescription={#AppDisplayName} 安装程序
OutputDir={#OutputRoot}
OutputBaseFilename={#AppName}-setup-{#AppVersion}-x64
LanguageDetectionMethod=uilanguage
ShowLanguageDialog=no

[Languages]
; 官方 Inno Setup 不带简体中文，只有用户贡献的 isl；这里随仓库带一份，
; 免得 CI/新机器没往 Languages 目录装语言包就编译失败。
Name: "zh"; MessagesFile: "ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DaemonExePath}"; DestDir: "{app}"; Flags: ignoreversion

[Registry]
; MCP 开关打开时主程序会登记这个登录自启项；卸载时一并撤掉，别留下指向
; 已删除 exe 的死项。ValueType: none 表示安装时不创建，只在卸载时删值。
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Daro MCP"; Flags: uninsdeletevalue

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppName}.exe"; WorkingDir: "{app}"; Comment: "{#AppDisplayName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppName}.exe"; WorkingDir: "{app}"; Comment: "{#AppDisplayName}"; Tasks: desktopicon

; 卸载第一步先让 MCP 后台进程退场。它不归 Restart Manager 管，正在运行的
; daro-mcpd.exe 会占住文件、让卸载删不掉，用户看到的就是「服务还在跑」。
; --uninstall 读心跳里的 PID 收工；skipifdoesntexist 兼容从没装过后台进程的旧版。
[UninstallRun]
Filename: "{app}\daro-mcpd.exe"; Parameters: "--uninstall"; Flags: runhidden waituntilterminated skipifdoesntexist

[Run]
Filename: "{app}\{#AppName}.exe"; Description: "立即运行 {#AppDisplayName}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent
