# daro

轻量级桌面数据库管理工具，Rust + gpui 重写版（`daro-rs`）。

界面与交互对齐 Flutter 版 daro，功能按 [MIGRATION.md](daro-rs/MIGRATION.md) 的台账逐步迁移。
当前处于 **pre-release** 阶段：SQLite / PostgreSQL 已接真实驱动；MySQL 等其余引擎只有界面，
数据仍是 mock。

## 交流

QQ 交流群：1132083886

## 功能

- 连接树：分组、搜索过滤与高亮、右键菜单、F2 就地改名、颜色标签 / 星标
- 表数据网格：分页、列头排序、单元格编辑、增删行、应用 / 放弃变更
- SQL 查询页：语法高亮与补全、执行结果网格、多页签
- 表设计器、连接导入导出（`.ncx`）、结果导出（txt / csv / json / xml / html）
- 明暗主题切换，窗口与面板状态持久化

## 构建与运行

要求 Rust stable（edition 2024，≥ 1.85）与桌面构建工具链。

```bash
cd daro-rs
cargo run --release
```

数据目录默认为 `%LOCALAPPDATA%\daro-rs`（连接配置、主题等），可用 `DARO_DATA_DIR` 覆盖。

## 测试

```bash
cd daro-rs
cargo test --bin daro-rs -- --test-threads=1
```

包含 headless UI 流程测试（真实建窗口并派发点击 / 键入）。用例串行执行，因为共享 `DARO_DATA_DIR`。
标注 `#[ignore]` 的用例需要可访问的 PostgreSQL 服务。

## 发布

版本号唯一来源是 `daro-rs/Cargo.toml`。推送 `daro-rs-v<version>` 标签即触发
[.github/workflows/release-daro-rs.yml](.github/workflows/release-daro-rs.yml)：
校验版本一致 → 四平台（Windows / macOS x64 / macOS arm64 / Linux x64）各自跑测试与构建
→ 汇总产物并发布 pre-release。

产物：

- Windows：`daro-rs-setup-<version>-x64.exe`（安装版）、`daro-rs-<version>-windows-x64-portable.zip`（绿色版，解压后带 `daro-rs.portable`，数据写在 exe 旁的 `daro-rs-data`）
- macOS：`daro-rs-<version>-macos-x64.zip` / `-macos-arm64.zip`（内含 `daro-rs.app`）
- Linux：`daro-rs-<version>-linux-x64.tar.gz`

macOS / Linux 的包没有签名：macOS 解压后先 `xattr -dr com.apple.quarantine daro-rs.app`。
SQL Server 连接走纯 Rust 的 TDS 驱动（tiberius），三个平台都不需要装 ODBC 驱动或额外运行库；
Linux 只需要系统里有 x11/wayland 与 fontconfig。Access（.mdb/.accdb）只有 Windows 版驱动，
后续按 Windows 专属能力接，macOS/Linux 不受影响。

```bash
git tag daro-rs-v0.0.1
git push origin daro-rs-v0.0.1
```

## 文档

- [MIGRATION.md](daro-rs/MIGRATION.md)：Flutter → Rust 迁移台账
- [FEATURES.md](daro-rs/FEATURES.md)：功能对照进度
- [DETAILS.md](daro-rs/DETAILS.md)：交互细节实测记录
