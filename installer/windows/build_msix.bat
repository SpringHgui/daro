@echo off
setlocal EnableDelayedExpansion

rem daro Windows MSIX packer (Microsoft Store 上架格式)
rem 用法:   installer\windows\build_msix.bat [options]
rem 选项:
rem   -Arch x64^|arm64      目标架构，默认 x64（arm64 只能在 arm64 主机上 flutter build）
rem   -SkipBuild           跳过 flutter build，直接打包 build\windows\<arch>\runner\Release 现有产物
rem   -Publisher "CN=xxx"  清单发布者主体，默认 CN=daro（须与签名证书 Subject 一致；上架时商店会覆盖）
rem   -PackageId "com.xxx" 包唯一标识 Name，默认 io.github.springhgui.daro（商店内不可与已有包冲突）
rem   -SignPfx "<路径>"    用该 pfx 签名（本地自测用；不填则产出未签名 msix）
rem   -SignPfxPwd "<口令>" 上述 pfx 的口令
rem   -NoPause             结束不等待按键（CI 用）
rem 产物:   dist\daro-<version>-windows-<arch>.msix
rem 依赖:   Windows 10/11 SDK 的 makeappx.exe（脚本会在 Windows Kits 目录里自动找）
rem 提示:   首次本地自测先跑（管理员 PowerShell）:
rem           powershell -ExecutionPolicy Bypass -File installer\windows\msix\make_selfsigned_cert.ps1
rem         它会生成 pfx 并把证书装进本机受信任存储，再带着 -SignPfx 回来打包。

set "SCRIPT_DIR=%~dp0"
for %%I in ("%SCRIPT_DIR%..") do set "PROJECT_ROOT=%%~fI"
set "MSIX_DIR=%SCRIPT_DIR%msix"
set "PACKER=%MSIX_DIR%\build_msix.ps1"

rem ---------- args ----------
set "SKIP_BUILD="
set "NO_PAUSE="
set "ARCH=x64"
set "PUBLISHER=CN=daro"
set "PACKAGE_ID=io.github.springhgui.daro"
set "SIGN_PFX="
set "SIGN_PWD="
:parse_args
if "%~1"=="" goto :parse_args_done
if /i "%~1"=="-SkipBuild"     ( set "SKIP_BUILD=1" & shift & goto :parse_args )
if /i "%~1"=="-NoPause"       ( set "NO_PAUSE=1"   & shift & goto :parse_args )
if /i "%~1"=="-Arch"          ( set "ARCH=%~2" & shift & shift & goto :parse_args )
if /i "%~1"=="-Publisher"     ( set "PUBLISHER=%~2" & shift & shift & goto :parse_args )
if /i "%~1"=="-PackageId"     ( set "PACKAGE_ID=%~2" & shift & shift & goto :parse_args )
if /i "%~1"=="-SignPfx"       ( set "SIGN_PFX=%~2" & shift & shift & goto :parse_args )
if /i "%~1"=="-SignPfxPwd"    ( set "SIGN_PWD=%~2" & shift & shift & goto :parse_args )
echo ERROR: unknown option "%~1"
echo Usage: build_msix.bat [-Arch x64^|arm64] [-SkipBuild] [-Publisher "CN=..."] [-PackageId "..."] [-SignPfx "..." -SignPfxPwd "..."] [-NoPause]
goto :die
:parse_args_done

if /i not "%ARCH%"=="x64" if /i not "%ARCH%"=="arm64" (
    echo ERROR: -Arch 只支持 x64 或 arm64（Flutter 桌面没有 32 位 x86 目标）。
    goto :die
)

rem ---------- 0. 版本号：pubspec.yaml 是唯一数据源（与 build_installer.bat 一致）----------
set "APP_VERSION="
for /f "usebackq tokens=1,2 delims=: " %%a in ("%PROJECT_ROOT%\pubspec.yaml") do (
    if "%%a"=="version" for /f "delims=+" %%v in ("%%b") do set "APP_VERSION=%%v"
)
if not defined APP_VERSION (
    echo ERROR: pubspec.yaml 里没找到 "version: x.y.z+n"。
    goto :die
)
echo App version (from pubspec.yaml): %APP_VERSION%

rem ---------- 1. 可选 flutter build ----------
set "PBD=%PROJECT_ROOT%\build\windows\%ARCH%\runner\Release"
if defined SKIP_BUILD goto :check_out
echo ==^> flutter build windows --release  (arch: %ARCH%) ...
pushd "%PROJECT_ROOT%"
call flutter build windows --release
if errorlevel 1 (
    popd
    echo ERROR: flutter build 失败。若链接器无法覆盖 exe^(LNK1168^)，请先关掉正在运行的 daro 再重试。
    goto :die
)
popd

:check_out
if not exist "%PBD%\daro.exe" (
    echo ERROR: 未找到构建产物 %PBD%\daro.exe
    echo        先跑 flutter build windows --release，或用 -SkipBuild 前确认产物已就位。
    goto :die
)

rem ---------- 2. 调打包脚本 ----------
if not exist "%PACKER%" (
    echo ERROR: 缺少打包脚本 %PACKER%
    goto :die
)

set "PS_ARGS=-Source "%PBD%" -Version "%APP_VERSION%" -Arch "%ARCH%" -Publisher "%PUBLISHER%" -PackageName "%PACKAGE_ID%""
if defined SIGN_PFX set "PS_ARGS=%PS_ARGS% -SignPfx "%SIGN_PFX%" -SignPfxPassword "%SIGN_PWD%""

echo ==^> powershell build_msix.ps1 ...
powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%PACKER%" %PS_ARGS%
if errorlevel 1 (
    echo ERROR: MSIX 打包失败。
    goto :die
)

echo.
echo Done. 产物在 %PROJECT_ROOT%\dist
call :maybe_pause
endlocal
exit /b 0

:die
echo.
Build aborted.
call :maybe_pause
endlocal
exit /b 1

:maybe_pause
if defined NO_PAUSE goto :eof
pause
goto :eof
