# 第三方商标声明 (NOTICE)

daro 的连接树、对话框等处会显示各数据库引擎的**厂商官方 logo**，
「导入连接」向导里还会显示可导入其导出文件的**第三方工具的官方标识**。
这些商标仅用于**指示该连接所指向的数据库产品 / 该导出文件来自哪个工具**
（指示性使用 / compatibility use），
与 DataGrip、DBeaver 等同类工具的做法一致；不用于暗示厂商对 daro 的背书或授权。

## 使用方式

* 官方原件抓取到 `assets/icons/brands/`，由 `tool/compose_engine_icons.py` 等比放进统一圆角瓦片底，
  输出 `assets/icons/engines/<id>.svg`。**未改动 logo 的几何与配色**（仅整体缩放、居中、裁剪到瓦片）。
* 第三方工具（Navicat / Another Redis Desktop Manager）的官方标识只有位图，
  由 `tool/compose_tool_icons.py` 把同一块瓦片底套在裁好的图形上，
  输出 `assets/icons/tools/<id>.svg`（图形按 base64 内嵌，随分发的只有这个 SVG）。
* `assets/icons/brands/` 未登记进 `pubspec.yaml` 的 assets，因此**原始文件不随应用分发**，
  随分发的只有合成后的 `assets/icons/engines/`、`assets/icons/tools/`，
  以及「关于」弹窗直接引用的 `assets/icons/brands/github.svg` / `gitee.svg`。
* 素材来源与权利人记录在 `assets/icons/brands/SOURCES.json`，本文件由其自动生成。

## 素材清单：数据库引擎

| 引擎 | 商标 | 权利人 | 来源 |
|---|---|---|---|
| `access` | Office Access 应用图标 | Microsoft Corporation | https://upload.wikimedia.org/wikipedia/commons/f/f8/Microsoft_Access_2013-2019_logo.svg?utm_source=commons.wikimedia.org&utm_campaign=imageinfo&utm_content=original |
| `mariadb` | 海狮 | MariaDB plc | https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/mariadb/mariadb-original.svg |
| `mongodb` | 绿叶 | MongoDB, Inc. | https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/mongodb/mongodb-original.svg |
| `mysql` | sakila 海豚 | Oracle America, Inc. | https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/mysql/mysql-original.svg |
| `oracle` | 红色字标 | Oracle Corporation | https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/oracle/oracle-original.svg |
| `postgresql` | Slonik 象头 | PostgreSQL Global Development Group | https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/postgresql/postgresql-original.svg |
| `redis` | 堆叠砖层 | Redis Ltd. | https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/redis/redis-original.svg |
| `snowflake` | 六角雪花 | Snowflake Inc. | https://cdn.simpleicons.org/snowflake |
| `sqlite` | 蓝色瓦片 + 羽毛笔 | D. Richard Hipp / SQLite project | https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/sqlite/sqlite-original.svg |
| `sqlserver` | 2025 蓝色缎带 S | Microsoft Corporation | https://upload.wikimedia.org/wikipedia/commons/4/41/Microsoft_SQL_Server_2025_icon.svg?utm_source=commons.wikimedia.org&utm_campaign=imageinfo&utm_content=original |

## 素材清单：导入来源工具

| 工具 | 商标 | 权利人 | 来源 |
|---|---|---|---|
| `another-redis-desktop-manager` | 红色圆角瓦片 + 白色五角星轮廓与圆点(应用图标) | qishibo / Another Redis Desktop Manager(仓库以 MIT 授权,含该图标) | https://raw.githubusercontent.com/qishibo/AnotherRedisDesktopManager/master/pack/electron/icons/icon.png |
| `navicat` | 金色三瓣图形 + Navicat 字标(横版 lockup) | PremiumSoft CyberTech Limited(Navicat) | https://upload.wikimedia.org/wikipedia/commons/d/dc/Navicat_logo.png?utm_source=commons.wikimedia.org&utm_campaign=imageinfo&utm_content=original |

## 素材清单：项目主页图标（「关于」弹窗）

| 站点 | 商标 | 权利人 | 来源 |
|---|---|---|---|
| `gitee` | 红色圆弧 G 字标 | 深圳奥思网络科技有限公司 (OSChina) | https://cdn.jsdelivr.net/npm/simple-icons@13/icons/gitee.svg |
| `github` | Invertocat 猫头 | GitHub, Inc. (Microsoft Corporation) | https://cdn.jsdelivr.net/npm/simple-icons@13/icons/github.svg |

## 权利归属

上述商标及其 logo 权利归属各自厂商：D. Richard Hipp / SQLite project、GitHub, Inc. (Microsoft Corporation)、MariaDB plc、Microsoft Corporation、MongoDB, Inc.、Oracle America, Inc.、Oracle Corporation、PostgreSQL Global Development Group、PremiumSoft CyberTech Limited(Navicat)、Redis Ltd.、Snowflake Inc.、qishibo / Another Redis Desktop Manager(仓库以 MIT 授权,含该图标)、深圳奥思网络科技有限公司 (OSChina)。

daro 与上述厂商之间不存在隶属、赞助、背书或授权关系。

如任一权利人希望以其他方式署名或要求移除其标识，请提 issue，我们会在下个版本处理。

> 本文件由 `tool/gen_brand_notice.py` 生成，请勿手工编辑清单部分。
