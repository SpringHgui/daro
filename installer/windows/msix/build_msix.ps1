<#
.SYNOPSIS
    把 flutter build windows 的 Release 产物打包成 MSIX（Microsoft Store 上架格式）。

.DESCRIPTION
    流程：
      1. 校验 -Source 下有 daro.exe（Release 构建产物目录）
      2. 建临时布局目录，拷入全部产物
      3. 用 build\ico\256.png（缺则回退 app_icon.ico）生成商店所需方块图标
      4. 读 msix\AppxManifest.xml 模板，替换 #{Token} 占位，写出布局根的 AppxManifest.xml
      5. makeappx pack 打成 .msix
      6. 可选：给了 -SignPfx 就用 signtool 签（本地自测安装用；上架时商店会重签）

    依赖 Windows 10/11 SDK 里的 makeappx.exe / signtool.exe（脚本会在 Windows Kits 目录里自动找）。
    仅做打包，不做 flutter build —— 先跑 build_msix.bat 或手动 flutter build windows --release。

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File build_msix.ps1 `
      -Source "..\..\build\windows\x64\runner\Release" -Version "0.8.0" `
      -Publisher "CN=daro" -Out "..\..\dist\daro-0.8.0-windows-x64.msix"
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Version,
    [ValidateSet('x64', 'arm64')][string]$Arch = 'x64',
    # 发布者主体：须与签名证书 Subject 一致。商店上架时会被商店证书覆盖，本地测才严格匹配。
    [string]$Publisher = 'CN=daro',
    # 商店里展示的发布者名（个人账号 = 注册时的 Publisher displayName）
    [string]$PublisherDisplayName = 'daro',
    [string]$PackageName = 'com.example.daro',
    [string]$DisplayName = 'daro',
    [string]$Description = '高信息密度的桌面数据库管理工具',
    [string]$Out = '',
    # 可选自签：本地安装测试用
    [string]$SignPfx = '',
    [string]$SignPfxPassword = '',
    # 打包后保留临时布局目录（排查用）
    [switch]$KeepLayout
)

$ErrorActionPreference = 'Stop'

# ---------- 路径解析 ----------
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
# ScriptDir = <root>\installer\windows\msix
$ProjectRoot = (Resolve-Path (Join-Path $ScriptDir '..\..\..')).Path
$TemplatePath = Join-Path $ScriptDir 'AppxManifest.xml'
$MasterPng = Join-Path $ProjectRoot 'build\ico\256.png'
$FallbackIco = Join-Path $ProjectRoot 'windows\runner\resources\app_icon.ico'

$Source = (Resolve-Path $Source).Path
$ExePath = Join-Path $Source 'daro.exe'
if (-not (Test-Path $ExePath)) {
    throw "未找到构建产物：$ExePath`n请先执行  flutter build windows --release  或用 build_msix.bat"
}
if (-not (Test-Path $TemplatePath)) {
    throw "未找到清单模板：$TemplatePath"
}

# ---------- 版本号：x.y.z -> x.y.z.r（makeappx 要四段）----------
$CleanVer = ($Version -split '\+')[0].Trim()
$verParts = $CleanVer -split '\.'
if ($verParts.Count -lt 3) { throw "版本号格式不对（需 x.y.z）：$Version" }
$PackageVersion = '{0}.{1}.{2}.0' -f [int]$verParts[0], [int]$verParts[1], [int]$verParts[2]

# ---------- 输出路径 ----------
$DistDir = Join-Path $ProjectRoot 'dist'
if (-not (Test-Path $DistDir)) { New-Item -ItemType Directory -Path $DistDir | Out-Null }
if ([string]::IsNullOrWhiteSpace($Out)) {
    $Out = Join-Path $DistDir ("daro-{0}-windows-{1}.msix" -f $CleanVer, $Arch)
} else {
    # 允许只给文件名
    if (-not [System.IO.Path]::IsPathRooted($Out)) { $Out = Join-Path $DistDir $Out }
}

Write-Host "==^> 打包 daro MSIX" -ForegroundColor Cyan
Write-Host "   Source    : $Source"
Write-Host "   Version   : $PackageVersion (arch $Arch)"
Write-Host "   Publisher : $Publisher"
Write-Host "   PackageId : $PackageName"
Write-Host "   Output    : $Out"

# ---------- 1. 建布局目录 ----------
$Layout = Join-Path $env:TEMP ("daro-msix-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Layout | Out-Null
try {
    Copy-Item -Path (Join-Path $Source '*') -Destination $Layout -Recurse -Force

    # ---------- 2. 生成方块图标 ----------
    $AssetsDir = Join-Path $Layout 'Assets'
    New-Item -ItemType Directory -Path $AssetsDir | Out-Null
    $master = $null
    if (Test-Path $MasterPng) { $master = $MasterPng }
    elseif (Test-Path $FallbackIco) { $master = $FallbackIco }
    else { throw "找不到图标母图：$MasterPng 或 $FallbackIco（先跑 flutter test test/app_icon_test.dart 生成 build\ico）" }

    Add-Type -AssemblyName System.Drawing
    function New-SquarePng {
        param([string]$Src, [string]$Dst, [int]$Size)
        $img = [System.Drawing.Image]::FromFile($Src)
        try {
            $bmp = New-Object System.Drawing.Bitmap $Size, $Size
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            try {
                $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                $g.DrawImage($img, 0, 0, $Size, $Size)
            } finally { $g.Dispose() }
            $bmp.Save($Dst, [System.Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $bmp.Dispose(); $img.Dispose()
        }
    }
    New-SquarePng -Src $master -Dst (Join-Path $AssetsDir 'StoreLogo.png')        -Size 50
    New-SquarePng -Src $master -Dst (Join-Path $AssetsDir 'Square44x44Logo.png')  -Size 44
    New-SquarePng -Src $master -Dst (Join-Path $AssetsDir 'Square150x150Logo.png') -Size 150

    # ---------- 3. 渲染清单 ----------
    $xml = Get-Content -Path $TemplatePath -Raw -Encoding UTF8
    $tokens = @{
        '#{PackageName}'        = $PackageName
        '#{Publisher}'          = $Publisher
        '#{PublisherDisplayName}' = $PublisherDisplayName
        '#{PackageVersion}'     = $PackageVersion
        '#{Architecture}'       = $Arch
        '#{DisplayName}'        = $DisplayName
        '#{Description}'        = $Description
    }
    foreach ($k in $tokens.Keys) { $xml = $xml.Replace($k, $tokens[$k]) }
    if ($xml -match '#\{') { throw "清单里还有未替换的占位符：`n$($Matches[0])" }
    # 无 BOM 的 UTF-8，makeappx 更稳
    [System.IO.File]::WriteAllText((Join-Path $Layout 'AppxManifest.xml'), $xml, (New-Object System.Text.UTF8Encoding $false))

    # ---------- 4. makeappx pack ----------
    function Find-Tool {
        param([string]$Name)
        $cmd = Get-Command $Name -ErrorAction SilentlyContinue
        if ($cmd) { return $cmd.Source }
        foreach ($root in @(
                'C:\Program Files (x86)\Windows Kits\10\bin',
                'C:\Program Files\Windows Kits\10\bin')) {
            if (Test-Path $root) {
                $hit = Get-ChildItem -Path $root -Recurse -Filter "$Name.exe" -ErrorAction SilentlyContinue |
                    Where-Object { $_.FullName -match '\\x64\\' } | Select-Object -First 1
                if ($hit) { return $hit.FullName }
            }
        }
        return $null
    }

    $makeappx = Find-Tool 'makeappx'
    if (-not $makeappx) {
        throw "未找到 makeappx.exe。请安装 Windows 10/11 SDK（Visual Studio Installer → 单个组件 → Windows SDK），或用 VS 开发者命令提示符运行本脚本。"
    }
    if (Test-Path $Out) { Remove-Item $Out -Force }
    Write-Host "==^> makeappx pack ..." -ForegroundColor Cyan
    & $makeappx pack /d "$Layout" /p "$Out" /o /v
    if ($LASTEXITCODE -ne 0) { throw "makeappx pack 失败（exit $LASTEXITCODE）" }

    # ---------- 5. 可选签名 ----------
    if (-not [string]::IsNullOrWhiteSpace($SignPfx)) {
        $signtool = Find-Tool 'signtool'
        if (-not $signtool) { throw "指定了 -SignPfx 但未找到 signtool.exe（同属 Windows SDK）。" }
        if (-not (Test-Path $SignPfx)) { throw "找不到证书文件：$SignPfx" }
        Write-Host "==^> signtool sign ..." -ForegroundColor Cyan
        & $signtool sign /fd SHA256 /f "$SignPfx" /p "$SignPfxPassword" /tr http://timestamp.digicert.com /td SHA256 "$Out"
        if ($LASTEXITCODE -ne 0) { throw "signtool 签名失败（exit $LASTEXITCODE）" }
    } else {
        Write-Host "   （未签名：本地双击安装会被挡；上架时商店会重签，自测请配 -SignPfx 或跑 make_selfsigned_cert.ps1）" -ForegroundColor Yellow
    }

    $mb = [math]::Round((Get-Item $Out).Length / 1MB, 1)
    Write-Host "==^> 完成：$Out ($mb MB)" -ForegroundColor Green
}
finally {
    if ($KeepLayout) {
        Write-Host "   布局目录保留：$Layout"
    } elseif (Test-Path $Layout) {
        Remove-Item -Path $Layout -Recurse -Force -ErrorAction SilentlyContinue
    }
}
