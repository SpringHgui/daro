# daro → Microsoft Store（MSIX 打包）

把 `flutter build windows` 的 Release 产物打成 **MSIX**，用于上架 Microsoft Store。
上架后从商店安装的用户不会再遇到 Edge / SmartScreen「通常不会下载」拦截。

## 为什么走商店能免签名

商店只收 MSIX（不收裸 exe / Inno 的 setup.exe）。你上传的包用一个**自签测试证书**签一下即可，
微软在入库（ingest）时会用商店证书**重新签名（Store signing）**；用户机器上认的是微软的签名，
发布者名取自你账号里的 Publisher displayName。因此**上架不需要买代码签名证书**。
个人开发者注册现已免费（微软 2025-09 起取消 $19 一次性费）。

## 目录内容

| 文件 | 作用 |
|---|---|
| `AppxManifest.xml` | 包清单**模板**（含 `#{Token}` 占位，由打包脚本注入实际值，勿直接用） |
| `build_msix.ps1` | 打包器：建布局 → 生成方块图标 → 渲染清单 → `makeappx pack` → 可选签名 |
| `make_selfsigned_cert.ps1` | 生成**本地自测**用的自签 pfx + 装进本机受信任存储（上架用不到） |
| `../build_msix.bat` | 一键包装：读 pubspec 版本 → 可选 flutter build → 调 `build_msix.ps1` |

依赖 **Windows 10/11 SDK** 的 `makeappx.exe` / `signtool.exe`（脚本会在
`C:\Program Files (x86)\Windows Kits\10\bin\*\x64\` 里自动找；找不到就用 VS 开发者命令提示符运行）。

## 本地打包（三步）

```bat
:: 1) 首次：管理员 PowerShell 生成自签证书并装进本机（仅本地自测用）
powershell -ExecutionPolicy Bypass -File installer\windows\msix\make_selfsigned_cert.ps1 -Publisher "CN=daro"

:: 2) 打包（自动 flutter build + 用上面导出的 pfx 签名）
installer\windows\build_msix.bat -Publisher "CN=daro" ^
  -SignPfx "dist\msix-test-daro.pfx" -SignPfxPwd "<你设的口令>"

:: 只打现有产物、跳过 flutter build：加 -SkipBuild；arm64：-Arch arm64
```

产物：`dist\daro-<version>-windows-<arch>.msix`。双击即可本地安装测试。

> 不签名也能产出 `.msix`（`build_msix.bat` 不带 `-SignPfx`），但本机双击装会被挡——
> 未签名包只用于上传前的结构自检，或你打算直接上传商店（商店会重签）。

## 上架流程（Partner Center）

1. 用微软账号登录 <https://partner.microsoft.com/dashboard>，走**个人（Individual）**注册（免费），
   确认账户设置里的 **Publisher displayName**。
2. 创建应用 → 填产品名称、定价、类别、**年龄分级**、**隐私政策 URL**（必填）。
3. 「包和文件」上传 `daro-<version>-windows-<arch>.msix` → 等 **Package Validation** 通过。
4. 补商店图标 + 截图（≥1 张 720p/1920×1080）+ 描述 → 提交版本。
5. 认证通过（数小时~2 天）→ 发布。之后用户从商店安装，无 SmartScreen 拦截。

> `PackageName`（清单 Identity Name，默认 `io.github.springhgui.daro`）在商店内全局唯一，
> 若被占用会在 Package Validation 报错——改成你自己的反向域名（如 `io.github.<你>.daro`）重打。

## ⚠️ AppData 落点（已在清单里解决，务必实测确认）

daro 的连接/主题/MCP 配置都走 `getApplicationSupportDirectory()`（`lib/data/connection_store.dart`、
`theme_store.dart`、`mcp_service.dart`）。`path_provider_windows` 2.3.0 在 Windows 上把它解析为
`SHGetKnownFolderPath(RoamingAppData)\com.example\daro`（`CompanyName\ProductName` 取自 exe 的
VERSIONINFO，见 `windows/runner/Runner.rc`），即真实 `%APPDATA%\com.example\daro`。

**问题**：MSIX 默认会把打包应用（**即使声明了 runFullTrust**）对 `%APPDATA%` 的新建写入重定向进
per-app 私有容器、卸载即清除。后果：① 商店版与 GitHub/绿色版不是同一份配置，老用户迁商店版看不到
连接；② 商店版卸载会清掉连接配置。

**解决（清单级，无需改 Dart）**：`AppxManifest.xml` 已声明
`unvirtualizedResources` 受限能力 + `desktop6:FileSystemWriteVirtualization=disabled`，
让 AppData 写入直接落到真实 `%APPDATA%\com.example\daro`、对包外可见、且不随卸载清除——
两个渠道天然共用同一份配置，老用户零迁移。因该能力需 Windows 10 1903（build 18362）起，
清单 `MinVersion` 已相应抬高。

> 注：把代码从 `getApplicationSupportDirectory()` 改成直接读 `%APPDATA%` 环境变量**没用**——
> 虚拟化对 `SHGetKnownFolderPath` 和环境变量同样生效，落点仍是容器。真正的开关在清单。

上架前请实测确认（需先装 Windows SDK 打出并本地安装签名包）：

```powershell
# 装好并建一条连接、退出后：配置应落在真实 %APPDATA%\com.example\daro，而非 Packages 容器
Test-Path "$env:APPDATA\com.example\daro\connections.json"          # 期望 True
Get-ChildItem -Recurse "$env:LOCALAPPDATA\Packages\io.github.springhgui.daro_*" -Filter connections.json -ErrorAction SilentlyContinue  # 期望空
# 卸载后重装，确认连接仍在（unvirtualizedResources 生效则不会丢）
```


## 与现有发布链路的关系

`build_installer.bat`（Inno 安装包 + 绿色 zip）保持不变，GitHub Releases 继续发；
MSIX 是**并行的商店渠道**。要接 CI（`release.yml` 里加一个 `build-windows-msix` job，
pfx 走 GitHub Secrets），说一声我照现有 workflow 风格补。
