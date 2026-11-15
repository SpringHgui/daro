# daro

> **原生、轻量、跨平台的桌面数据库连接与管理工具。**
> 一个工具，连得上你手头所有数据库；不用装一堆驱动，不占内存，开箱即用。

---

## daro 是什么

**daro 是一款对标 Navicat 的轻量级桌面数据库管理工具。** 它把连接管理、表数据浏览/编辑、SQL 查询、表结构设计、数据导入导出这些日常 DBA 与开发工作，收进一个干净的原生桌面应用里。

与常见的 Electron 类工具不同，daro 用 纯 **Rust** 从零重写（代号 `daro-rs`），不依赖浏览器内核，因此启动快、内存占用低、安装包小，还提供免安装的绿色版。

界面与交互对齐参考产品，连接文件（`.ncx`）双向互通，老用户迁移零成本。

---

## 界面预览

**SQL 查询 · 结果网格**（PostgreSQL）

<p align="center">
  <img src="docs/images/postgresql-query.png" width="760" alt="daro PostgreSQL SQL 查询与结果网格">
</p>

**多页签查询 · 表数据浏览**

<p align="center">
  <img src="docs/images/postgresql-multi-tab-query.png" width="760" alt="daro 多页签查询与表数据浏览">
</p>

**Redis 键浏览 · 深色主题**

<p align="center">
  <img src="docs/images/redis-browser-dark.png" width="760" alt="daro Redis 键浏览深色主题">
</p>

**Redis 键浏览 · 浅色主题**

<p align="center">
  <img src="docs/images/redis-browser-light.png" width="760" alt="daro Redis 键浏览浅色主题">
</p>

---

## 为什么选择 daro

- **原生轻量，不为 GUI 付额外代价。** Rust + gpui 原生实现，没有 Electron 的运行时与内存开销；绿色版解压即用，不写注册表、不留垃圾。
- **一个工具管所有数据库。** SQLite / MySQL / PostgreSQL / SQL Server / Redis / MongoDB / Access 统一在一个界面里管理，不用为每种引擎装一套客户端。
- **真正跨平台。** Windows / macOS / Linux 同一套代码、同一套交互，换电脑不断档。
- **兼容 Navicat 和 another redis desktop manager 连接。** 支持导入 / 导出 `.ncx` 连接文件（含分组与私有属性、密码解密），老连接一键带走。
- **本地优先，数据不出本机。** 连接配置、主题、已保存查询全部存在本地，不上云、不追踪。
- **为 AI 时代预留接口。** 内置 `daro-mcpd` MCP 服务，让 Claude / Cursor 等 AI 工具在你的授权下直接查询数据库——把 daro 变成 AI 编程工作流里的数据库入口。

---

## 支持的数据库

| 引擎 | 平台 | 当前状态 |
|---|---|---|
| SQLite | Windows / macOS / Linux | ✅ |
| PostgreSQL | Windows / macOS / Linux | ✅ |
| MySQL / MariaDB | Windows / macOS / Linux | ✅ |
| SQL Server | Windows / macOS / Linux | ✅ |
| Redis | Windows / macOS / Linux | ✅ |
| MongoDB | Windows / macOS / Linux | ✅ |
| Access（`.mdb` / `.accdb`） | 仅 Windows | ✅ |

---
 
## 交流

QQ 交流群：1132083886
