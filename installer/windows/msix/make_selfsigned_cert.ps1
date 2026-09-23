<#
.SYNOPSIS
    生成一张用于本地安装测试 MSIX 的自签名证书，导出 .pfx，并把公钥装进受信任存储。

.DESCRIPTION
    MSIX 必须签名才能本地双击/Add-AppxPackage 安装；装的那张证书的 Subject 必须和
    AppxManifest.xml 里的 Publisher 完全一致。本脚本一步到位：
      1. New-SelfSignedCertificate 造一张代码签名证书（Subject = -Publisher）
      2. 导出 .pfx 到 dist\（给 build_msix.ps1 的 -SignPfx 用）
      3. 把公钥证书装进 本地计算机\受信任人 + 受信任根（让系统接受该签名包）

    仅用于本机自测！上架到 Microsoft Store 不需要这张证书——商店入库时会用
    微软的证书重新签名（Store signing），用户机器上根本不认这张自签证书。

    需要以【管理员身份】运行 PowerShell（要写 LocalMachine 证书存储）。

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File make_selfsigned_cert.ps1 -Publisher "CN=daro"
#>
[CmdletBinding()]
param(
    [string]$Publisher = 'CN=daro',
    [string]$PfxPassword = 'daro-test',
    [string]$OutDir = ''
)

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = (Resolve-Path (Join-Path $ScriptDir '..\..\..')).Path
if ([string]::IsNullOrWhiteSpace($OutDir)) { $OutDir = Join-Path $ProjectRoot 'dist' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir | Out-Null }

# 检查管理员权限（写 LocalMachine 存储需要）
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()
           ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Warning "当前非管理员，无法写入 本地计算机 证书存储；导出的 .pfx 仍可用，但本地安装可能仍被挡。请用【管理员 PowerShell】重跑本脚本。"
}

# 从 Subject 里取 CN 值当作证书 FriendlyName / 文件名（CN=daro -> daro）
$cn = ($Publisher -split 'CN=')[-1].Trim().Split(',')[0].Trim()
if (-not $cn) { $cn = 'daro' }
$pfxPath = Join-Path $OutDir ("msix-test-{0}.pfx" -f $cn)

Write-Host "==^> 生成自签名代码签名证书：Subject=$Publisher" -ForegroundColor Cyan

# 先删同名旧测试证书，避免重复
Get-ChildItem Cert:\CurrentUser\My -ErrorAction SilentlyContinue |
    Where-Object { $_.Subject -eq $Publisher -and $_.FriendlyName -eq "daro-msix-test" } |
    Remove-Item -Force -ErrorAction SilentlyContinue

if (Test-Path $pfxPath) { Remove-Item $pfxPath -Force }

$cert = New-SelfSignedCertificate `
    -Type Custom `
    -Subject $Publisher `
    -KeyUsage DigitalSignature `
    -FriendlyName "daro-msix-test" `
    -CertStoreLocation 'Cert:\CurrentUser\My' `
    -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')

$secPwd = ConvertTo-SecureString -String $PfxPassword -Force -AsPlainText
Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $secPwd | Out-Null
Write-Host "   已导出 pfx：$pfxPath" -ForegroundColor Green

# 导出公钥 cer 用于装受信任存储
$cerPath = Join-Path $OutDir ("msix-test-{0}.cer" -f $cn)
Export-Certificate -Cert $cert -FilePath $cerPath | Out-Null

if ($isAdmin) {
    Import-Certificate -FilePath $cerPath -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople' | Out-Null
    Import-Certificate -FilePath $cerPath -CertStoreLocation 'Cert:\LocalMachine\Root' | Out-Null
    Write-Host "   公钥已装入 本地计算机\受信任人 + 受信任根（本机可安装该签名包）" -ForegroundColor Green
}

Write-Host ""
Write-Host "==^> 下一步：用它签名打包" -ForegroundColor Cyan
Write-Host "   build_msix.bat -Publisher `"$Publisher`" -SignPfx `"$pfxPath`" -SignPfxPassword `"$PfxPassword`""
Write-Host "   （清单里的 -Publisher 必须与这里一致：$Publisher）"
