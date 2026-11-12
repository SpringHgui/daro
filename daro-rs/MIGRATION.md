# daro-rs ← Flutter 版迁移台账

基准改为 **Flutter 版 daro（`lib/`）**：功能、交互、文案一律以它为准，
表格式的「参考界面实测」记录（[DETAILS.md](DETAILS.md)）停止更新，只作历史参考。

状态：⬜ 未开始 / 🔶 部分 / ✅ 已迁移（有测试或实机截图）

> 本文按时间顺序记录，各章末尾的「遗留」是**当时**状态；最终状态一律以
> 「总览」表和最后一章「本轮完成：迁移收尾（#1–#5）」为准。
> 例：`.ano` 导入导出、数据导入向导已于 #4 结清，函数 / 视图设计器补页已于 #3 结清。

---

## 总览（按 Flutter 目录清点）

### 数据层 `lib/data/`

| Flutter 模块 | Rust 现状 | 状态 |
|---|---|---|
| `drivers/*`（mysql / mariadb / pgsql / sqlserver / sqlite / access / mongo / redis / odbc） | MySQL 真驱动 + 演示连接机制已迁；其余引擎仍 mock | 🔶 |
| `db_metadata.dart` / `table_view.dart` | 无 | ⬜ |
| 连接文件导入（.ncx + .ano） | `.ncx` 解析 + 密码解密、`.ano`（Redis base64 JSON）导入导出均已迁移 | ✅ |
| 连接文件导出 | ✅（生成 + 导出向导 + 保存对话框，含 `.ano`） | ✅ |
| `csv_codec.dart` / `db_export.dart` / `db_import.dart` | 结果导出五种文本格式（txt/csv/json/xml/html）+ 数据导入向导已迁移；表数据导出向导、SQL 格式导出未做 | 🔶 |
| `sql_split.dart` | 已迁移（`split_sql` 口径，缺 DELIMITER/GO） | 🔶 |
| `sql_completions.dart` | ✅ 本轮迁移（词表 + 前缀匹配 + 弹层实机可用） | ✅ |
| `lib/theme/sql_highlight.dart` | ✅ 本轮迁移（词法 + 固定配色 + 编辑器高亮适配） | ✅ |
| `table_design.dart` / `view_rule_dialog` / `function_wizard` | 表设计器 10 个子页全部迁完（字段/索引/外键/唯一键/检查/排除/规则/触发器/选项/注释）+ SQL 预览；真库表打开设计页会反查真实结构（SQLite pragma / MySQL information_schema / PG pg_catalog）；视图/实体化视图「定义」页已迁；ALTER 差异未做 | 🔶 |
| `schema_sync.dart` | 无 | ⬜ |
| `connection_store.dart` / `theme_store.dart` / `locale_store.dart` | `connections.json` 已有；主题随连接一起存；无语言存储 | 🔶 |
| `result_grid_layout.dart` / `column_clipboard.dart` | 无（网格是简化版） | ⬜ |
| `cell_value_view.dart` | ✅ 本轮迁移：底部「单元格编辑器」抽屉五页签（文本 / JSON / 十六进制 / 图像 / 网页）+ 折叠 / 关闭；纯逻辑在 `cell_value.rs` | ✅ |
| `saved_query_store.dart` / `sql_file_run.dart` | ✅ 本轮迁移（`queries.json` 落盘 + 运行 SQL 文件对话框） | ✅ |

### 应用层 `lib/app/`

| Flutter 模块 | Rust 现状 | 状态 |
|---|---|---|
| `app_state.dart`（3142 行） | 主状态在 `main.rs`（I0k 行）里，语义只覆盖子集 | 🔶 |
| `connection_manager.dart` | mock 化的开关连接 | 🔶 |
| `mcp_service.dart` + `lib/mcp/*` | 2026-11-10：`src/mcp/*`（策略/分类/工具）+ `src/bin/daro-mcpd.rs` 独立后台进程，Streamable HTTP 只听 127.0.0.1、每请求 Bearer 鉴权；7 个工具（连接/库/模式/表/字段/执行/预览），语句分类沿用 Flutter 版白名单口径；`get_schema_context` / 例程 3 个工具未做 | 🔶 |
| `kv_manager.dart` + `kv_browser_page` | 无 | ⬜ |
| `cli_console.dart` / `command_line_page` | 无 | ⬜ |
| `sub_window.dart` / `ime_suppressor.dart` | 对话框已是独立窗口；IME 未接 | 🔶 |

### 界面 `lib/widgets/` `lib/pages/`

| Flutter 模块 | Rust 现状 | 状态 |
|---|---|---|
| `database_tree.dart` | 有；搜索/分组/右键/改名；「我的连接」根节点已移除；无拖拽、无键盘导航 | 🔶 |
| `object_panel.dart` | 列表态单列列表；网格态已改成多列流式（先从上到下填满一列、再从左到右开新列，列宽 250、列高 = 视口高，横向滚动 + 列级虚拟化）；鼠标框选（橡皮筋多选）两态都已支持 | 🔶 |
| `database_info.dart`（右侧详情面板） | 连接/库/模式/对象四类卡 + 表 DDL 页（异步反查 → 设计器同款拼装器，语法高亮/复制/最大化）+ PG 表「使用 / 被使用」依赖树（`pg_depend` 双向反查，外键下挂内部触发器，可折叠） | 🔶 |
| `table_data_page.dart` | mock 网格：分页/排序/单元格编辑/增删行/底部单元格编辑器抽屉 | 🔶 |
| `query_page.dart` | 高亮 / 补全 / 查找替换栏 / 运行选区 / 已保存查询 / 运行 SQL 文件均已接线；真库执行仅 MySQL | 🔶 |
| `table_designer` 系 | 10 个子页全部迁完（见上）；真库表的结构取自目录反查，不再用种子数据 | 🔶 |
| `view_designer` / `matview` / `function_wizard` | 视图「定义」+「刷新选项」、函数「参数 / 返回值 / 函数体」已迁 | 🔶 |
| `data_export_wizard.dart` / `data_import_wizard.dart` | 数据导入向导已迁；表数据导出向导未做 | 🔶 |
| `ncx_import_dialog.dart` | ✅ 本轮迁移（见下） | ✅ |
| `ncx_export_dialog.dart` | ✅ 本轮迁移 | ✅ |
| `result_export_dialog.dart` | ✅ 结果导出对话框（数据页签右键「导出结果…」） | ✅ |
| `options_dialog.dart` / `theme_customize_dialog.dart` | 2026-11-10：`src/options_dialog.rs` 按 Zed 设置版式实现（左分类 + 右侧分组行，常规 / 界面 / 查询 / 存储），只放真正落盘的开关；主题自定义未做 | 🔶 |
| `mcp_settings_dialog.dart` | 2026-11-10：MCP 设置并入「工具 ▸ 选项…」第 5 个分类（`src/mcp_page.rs`）：开关 / 运行状态 / 三档模式 / 端口 / 令牌轮换 / 客户端配置复制，改动即时生效；按连接收紧范围与工具白名单未做 | 🔶 |
| `about_dialog.dart` / `sql_find_bar.dart` / `kv_*` | 关于对话框、SQL 查找替换栏已迁；KV 浏览器相关未做 | 🔶 |
| `l10n/*`（中/英/日） | 全中文硬编码 | ⬜ |

---

## 本轮完成：对象面板鼠标框选

- 首页对象面板（`ObjectsPane`）支持按住左键拖矩形多选：列表态按行命中，
  网格态按「列优先」的行位置命中；鼠标坐标先换算成滚动内容坐标，滚动后仍对得上。
- 多选集存在根视图 `DaroApp.obj_multi`（路径 + 对象下标）；快照只在「路径不变
  且当前项仍在集合里」时下发，切库 / 点树后自动退回单选，不必每个选中入口去清。
- 松手提交：命中为空回分类选中，一个命中按单选，多个命中才记多选。框选松手那次
  补发的 `click` 会被吞掉，不会把多选打回单选。
- 空框只把「分类路径上的选中（分类 / 库 / 对象）」回落到分类节点；选中还停在连接层时
  面板本来就是空的，空框不改变选中，不会凭空造出分类上下文把动作条点亮。
- 起拖阈值 4px；矩形画在面板视口的绝对定位层上、限制在视口内，不随内容滚动。
  视口右缘 / 底缘的滚动条占位不参与框选（拖滚动条不会被顺手拉起矩形）。
- 测试：`obj_rubber_tests::object_panel_rubber_band_selects_rows`（列表态框三行 →
  多选 → 单击回单选）、`object_panel_rubber_band_works_in_grid`（网格态同列三行）与
  `object_panel_rubber_band_skips_scrollbar_gutter`（右缘拖动不框选、内侧照常框）。

---

## 本轮完成：连接导入（`.ncx`）

- 新增 `src/ncx_import_dialog.rs`（820×560 独立窗口）：
  路径输入 + 「浏览…」（原生 `rfd` 文件对话框）→ 解析 → 勾选列表 → 导入。
- 口径照 Flutter `NcxImportDialog`：
  - 重名默认不勾（勾了导入时按 ` (2)`、` (3)` 顺延改名，同 `AppState._uniqueNamed`）；
  - 不受支持的类型（SQLite / Oracle …）灰显、不给勾选框；
  - 分组列对「本地还没有的分组」标 `·新建`；
  - 密码状态分四档显示：空串（已解密）/ 无密码 / 密码未保存 / 密码无法解密。
- 主窗口新增 `import_connections()`：登记缺失分组、顺延重名、落盘 `connections.json`。
- 顺手修了 `NcxFile::parse` 不还原 XML 实体的问题（`a&amp;b` 现在解析成 `a&b`），
  并加了守护用例 `parse_unescapes_attribute_entities`。
- 验证：`cargo test --bins` 33 个用例全绿；实机连续导入两次截图确认
  （第一次 6 条入库 + 新建分组「导入演示」；第二次同名全部默认不勾）。

**遗留**：`.ano`（Another Redis Desktop Manager 的 base64 JSON）未支持；
大文件（379 条）目前一次性铺满列表，未用虚拟化；路径输入框未做「失焦即解析」。

---

## 本轮完成：连接导出（`.ncx`）

- 新增 `src/ncx_export_dialog.rs`（780×560 独立窗口）：勾选连接 →
  选路径（默认桌面 `connections.ncx`，可走原生保存对话框）→ 「导出密码」开关 → 写出。
- 口径照 Flutter `NcxExportDialog`：默认全选可导出的连接；「全选 / 取消全选」
  语义固定；Redis 灰显并标「不支持 .ncx」；`SettingsSavePath` 按
  `<文档>\参考界面\<类型>\Servers\<连接名>` 生成；写出失败把原因留在对话框里。
- 主窗口新增 `export_snapshot()`；菜单项「文件 → 导出连接…」已接线。
- 新增往返用例 `export_round_trips_name_group_and_password`：导出 → 解析回来，
  名称里的 `&`、中文、分组私有属性、密码密文全部原样还原。
- 验证：实机导出 11 条到桌面并检查文件头（`<Connections Ver="1.5">`、中文连接名、
  `SettingsSavePath` 落 `<文档>\参考界面\MySQL\Servers\...`），确认后删除了测试文件；
  `cargo test --bins` **34 个用例全绿**。

**遗留**：`.ano` 导出（Redis）未做——与导入侧一起排期；大列表未虚拟化。

---

## 本轮完成：导出结果（CSV / JSON / XML / HTML / TXT）

- 新增 `src/export_format.rs`：从 Flutter `csv_codec.dart` + `db_export.dart` 迁来的
  纯函数编码器 —— `DelimitedStyle`（分隔符/引号/换行/NULL 文本）、CSV 行编码、
  带表头与 BOM 的整表编码、JSON 数组（紧凑/格式化）、XML（`<row>/<field>`）、
  HTML（带样式表格 + `td.null`）。6 个纯函数用例守护引号/NULL/BOM/转义/JSON 形状。
- 新增 `src/export_result_dialog.rs`（620×520 独立窗口）：格式五选一 + CSV 选项
  （分隔符 / 含列名 / BOM / 全部加引号）+ JSON 格式化 + 路径（默认桌面
  `<表名>.<ext>`，换格式自动换扩展名，可走原生保存对话框）。
- 接线：数据页签网格右键菜单新增「导出结果…」（在「复制为 …」四项之后）。
- 验证：实机从 `bx_cart` 数据页导出 CSV（984 字节，BOM + 11 列表头 + 14 行 CRLF）
  与 JSON（2906 字节，14 个对象、字段名/值正确），文件校验后已删除；
  `cargo test --bins` **40 个用例全绿**。

**遗留**：结果导出暂无 SQL 格式（与 Flutter 一致）；表数据「导出向导」（服务端分页 +
进度/取消）未做；`.ano` 导入导出仍缺。

---

## 本轮完成：控件尺寸统一

- 新增 [src/ui.rs](src/ui.rs)：标准控件 24 高 / 紧凑 22 高 / 标签宽 64 /
  标题栏 24 / 底栏 46 / 列表行 28，外加 `primary_button`、`secondary_button`、
  `compact_button`、`text_input`、`form_label` 五个构造函数。
- 改造范围：导入连接、导出连接、导出结果、文本输入框、确认框、筛选值弹窗
  —— 原先按钮高度在 22/26/30 之间参差，输入框 22/24/26 混用。
- 底栏顺序统一成「主按钮 → 次按钮 → 弹性空白 → 状态文字」。
- 验证：两张截图对比（导入 / 导出连接）控件高度与底栏布局一致；
  `cargo test --bins` **40 个用例全绿**。

---

## 本轮完成：SQL 语法高亮

- 新增 [src/sql_highlight.rs](src/sql_highlight.rs)：
  - 词法 `lex_sql`：关键字 / 字面量 / 类型 / 内置函数 / 字符串（`''` 与反斜杠转义）/
    数字（十六进制、指数）/ 行注释（`--`、`#`）/ 可嵌套块注释 / 反引号与方括号标识符；
  - 配色与 Flutter `sqlLightTheme` / `sqlDarkTheme` 逐条对齐（固定中调色，明暗由全局
    标志切换，不参与主题定制）；
  - 实现 gpui 编辑器 `InputHighlighter`：`styles()` 返回**完整覆盖请求区间**的样式段
    （字节偏移，与编辑器 `element.rs` 的 `byte_start..byte_end` 口径一致）。
- 接线：新建查询页签装上 highlighter 工厂并 `set_highlighter("sql")`；
  `apply_palette` 同步明暗标志，切主题后下一帧换色，不用重建页签。
- 验证：实机在查询页键入 SQL，关键字蓝、字符串红、数字绿、注释绿色斜体、
  内置函数黄褐（截图 `q_sql.png`）；`cargo test --bins` **44 个用例全绿**
  （新增 4 个词法/覆盖用例）。

**遗留**：只做高亮，没有补全；词法未覆盖各方言特有语法（PG `$$`、MySQL `DELIMITER`），
不认识的符号按正文色处理。

---

## 本轮完成：SQL 补全

- 新增 [src/sql_completion.rs](src/sql_completion.rs)：
  - 关键字表（与 Flutter `kSqlKeywords` 同一份）与 45 个常用函数（带分类说明）；
  - 匹配规则照 `_matchWord`：大小写不敏感前缀、输入等于整词时不再提示；
  - `word_prefix` 取光标前词前缀（字节偏移），`filtered` 保序限流；
  - 候选 = 关键字 + 函数 + 当前库的表 + 列，经 `CompletionProvider` 交给编辑器自带的补全弹层。
- 接线：查询页签装上 provider；应用层补 `ctrl-space → ShowCompletions` 键位
  （编辑器只挂了 on_action，键位得宿主绑）。
- **踩过的坑（已修）**：`is_completion_trigger(offset, text, cx)` 里的 `text` 是
  **本次插入的文本**、`offset` 才是文档绝对位置。第一版把它当整篇文档去
  `text[..offset]` 截断，长度不够直接 return false —— 补全弹层永远不出现。
  现在抽成纯函数 `is_trigger_text` 并按「插入内容末字符」判定，加了回归用例。
- 验证：实机键入 `sel` 弹出 `SELECT 关键字`，回车接受；再键入 `from b` 弹出
  `BY / BETWEEN`（关键字）与 `bx_cart / bx_cart_item / …`（表）两组候选，
  截图 `sql_complete.png` / `sql_complete_accept.png`；`cargo test --bins`
  **49 个用例全绿**。

**遗留**：Flutter 的「表别名解析」（`FROM t AS a` 后按别名提示列）未迁；
候选里的列取自 mock 公共列集，真驱动接入后要换成当前表的真实列。

---

## 本轮完成：设计表「索引」子页

- 模型：新增 [DesignIndex](src/main.rs)（名称 / 字段逗号列表 / 方法 / 唯一），随设计页签存亡；
  校验补三条：索引名不能为空、字段不能为空、索引名不能重复。
- 生成：``design_index_sql`` 按方言出 ``CREATE [UNIQUE] INDEX <name> ON <table> [USING m] (cols);``，
  追加在 CREATE TABLE（及 PG 的 COMMENT ON）之后；``design_create_sql`` 预览里能直接看到。
- 界面：子标签「索引」不再是占位页 —— 一行一个索引（名称 / 字段 / 方法 / 唯一 / ×），
  底部「+ 添加索引」；输入框沿用字段网格的常驻编辑器做法（[spawn_index_editors](src/main.rs)），
  键入即写回模型并清掉「无变更」提示。
- 验证：实机新建索引 ``idx_org / org_id, id / 唯一``，SQL 预览生成
  ``CREATE UNIQUE INDEX `idx_org` ON `` (`org_id`, `id`);``（截图 ``d_idx_filled.png``、``d_idx_sql.png``）；
  ``cargo test --bins`` **50 个用例全绿**（新增 1 个索引生成/校验用例）。

**遗留**：ALTER 场景还没有索引差异（设计器本来就只有 CREATE 预览，没有 ALTER 生成）；
外键 / 唯一键 / 检查 / 排除 / 规则 / 触发器 / 选项 / 注释 8 个子页仍是占位。

---

## 本轮完成：设计表「外键」子页

- 模型：``DesignForeignKey``（名称 / 字段 / 引用表 / 引用字段 / 删除时 / 更新时），随设计页签存亡；
  校验补四条必填（名称 / 字段 / 引用表 / 引用字段）与名称去重。
- 生成：``design_fk_sql`` 出 ``CONSTRAINT name FOREIGN KEY (cols) REFERENCES ref (refcols)``，
  ``ON DELETE`` / ``ON UPDATE`` 仅在不是 ``NO ACTION`` 时输出；作为表内约束子句排在 PRIMARY KEY 之后。
- 界面：子标签「外键」不再是占位页 —— 六列一行（名称 / 字段 / 引用表 / 引用字段 / 删除时 / 更新时 / ×），
  底部「+ 添加外键」，输入同样走常驻编辑器。
- 验证：实机填 ``fk_org / org_id / org / id / CASCADE / NO ACTION``，SQL 预览生成
  ``CONSTRAINT `fk_org` FOREIGN KEY (`org_id`) REFERENCES `org` (`id`) ON DELETE CASCADE``，
  ``ON UPDATE`` 按规则省略（截图 ``fk_filled.png``、``fk_sql.png``）；
  ``cargo test --bins`` **51 个用例全绿**（新增外键生成/校验用例，含 MySQL 与 PG 方言）。

**遗留**：``matchAll`` / 可延迟 / 约束注释这几个 PG 专属字段未迁；ALTER 差异仍未做；
唯一键 / 检查 / 排除 / 规则 / 触发器 / 选项 / 注释 7 个子页仍是占位。

---

## 本轮完成：设计表「唯一键」「检查」子页

- 模型：两页共用 `DesignPair`（名称 + 内容：唯一键存字段列表、检查存表达式）；
  校验统一处理「名称/内容不能为空」与「名称重复」，错误文案带页名。
- 生成：唯一键出 `CONSTRAINT name UNIQUE (cols)`（按方言引号），
  检查出 `CONSTRAINT name CHECK (expr)`（非标识符名自动加双引号）；
  都作为表内约束排在主键、外键之后。
- 界面：子标签「唯一键」「检查」不再是占位页 —— 一行「名称 + 字段/表达式 + ×」，
  底部添加按钮，共用一套 `pairs_tab` 渲染与编辑器装配。
- 验证：实机分别填 `uk_name / name, org_id` 与 `ck_age / age > 0`，SQL 预览同时出现
  `CONSTRAINT `uk_name` UNIQUE (`name`, `org_id`)` 与 `CONSTRAINT ck_age CHECK (age > 0)`
  （截图 `uk_filled.png`、`ck_sql.png`）；`cargo test --bins` **52 个用例全绿**
  （新增唯一键/检查的两种方言与校验用例）。

**遗留**：唯一键的 tablespace / fillFactor / 可延迟（SQL Server、PG 专属）未迁；
检查的 NO INHERIT（PG）未迁；ALTER 差异仍未做；排除 / 规则 / 触发器 / 选项 / 注释 5 页仍是占位。

---

## 本轮完成：设计表「选项」「注释」子页

- 模型：`DesignOptions`（表注释 / 填充因子 / 表空间），校验填充因子必须是 0..100 的整数。
- 生成（按方言分叉）：
  - MySQL：`CREATE TABLE … ) COMMENT='…';`（注释里的单引号双写）；不生成表选项；
  - PostgreSQL：`) WITH (fillfactor = n) TABLESPACE ts;` + `COMMENT ON TABLE … IS '…';`；
  - SQL Server：`) WITH (FILLFACTOR = n);`，注释走扩展属性、本版不生成（界面已注明）。
- 界面：「选项」页两行输入（填充因子 / 表空间）+ 方言提示；「注释」页一行表注释 + 三方言说明。
- 验证：实机（MySQL 连接）填表注释「演示表」，SQL 预览生成 `) COMMENT='演示表';`
  （截图 `opt_tab.png`、`cmt_try.png`、`opt_sql2.png`）；
  `cargo test --bins` **53 个用例全绿**（新增三方言表选项/注释与填充因子校验用例）。

**遗留**：PG 其余 reloption（autovacuum_* 等）未迁；SQL Server 表注释未生成；
排除 / 规则 / 触发器 3 个子页仍是占位；ALTER 差异仍未做。

---

## 本轮完成：设计表「排除」子页（仅 PostgreSQL）

- 模型：`DesignExclude`（名称 / 元素 / 方法 / 谓词），方法默认 `gist`，谓词可空。
- 生成：`CONSTRAINT "name" EXCLUDE USING <method> (<elements>) [WHERE (<predicate>)]`，
  作为表内约束排在检查约束之后；**只有 PostgreSQL 生成**。
- 方言门控：非 PG 连接里一旦有排除约束，保存被拦住并提示
  「排除约束仅 PostgreSQL 支持，当前连接不能保存」（不是悄悄不生成）。
- 界面：「排除」子页一行 名称 / 元素 / 方法 / 谓词 / ×，底部「+ 添加排除」+ 仅 PG 说明。
- 验证：实机在 MySQL 设计页填 `ex_room / room WITH = / gist`（截图 `ex_tab2.png`、`ex_filled.png`）；
  PG DDL、MySQL 不生成、门控与必填/去重校验由单测覆盖；
  `cargo test --bins` **54 个用例全绿**。

**遗留**：排除的可延迟/约束注释未迁；规则、触发器 2 页仍是占位；ALTER 差异仍未做。

---

## 本轮完成：设计表「规则」子页（仅 PostgreSQL）

- 模型：`DesignRule`（名称 / 事件 / 条件 / 动作 / 语句），动作默认 `ALSO`，事件默认 `INSERT`。
- 生成：`CREATE RULE "name" AS ON <event> [WHERE (<qual>)] DO <action> [<stmt>];`
  - `ALSO` / `INSTEAD` 必须带语句，`NOTHING` 不带；作为独立语句排在 CREATE TABLE 与索引之后。
  - 只有 PostgreSQL 生成，其它方言由校验拦住保存（「规则仅 PostgreSQL 支持，当前连接不能保存」）。
- 界面：「规则」子页一行 名称 / 事件 / 条件 / 动作 / 语句 / ×，底部「+ 添加规则」+ 仅 PG 说明。
- 验证：实机（MySQL 设计页）确认页面结构、可增删行、字段可编辑（截图 `rule_clean.png`）；
  PG DDL（含 WHERE / ALSO / INSTEAD / NOTHING）、非 PG 不生成、门控与校验由单测覆盖；
  `cargo test --bins` **55 个用例全绿**。

**遗留**：规则的启用开关（ALTER TABLE … DISABLE RULE）未迁；触发器子页仍是占位；
ALTER 差异仍未做。

---

## 本轮完成：设计表「触发器」子页（三方言）

- 模型：`DesignTrigger`（名称 / 时机 / 事件 / 级 / 函数 / 主体）；事件用逗号列表。
- 生成（三种方言各自成函数，不共享拼装分支）：
  - **MySQL**：一个触发器只能挂一个事件 → 多事件按事件后缀拆成多条
    `CREATE TRIGGER `name_event` timing EVENT ON `t` FOR EACH ROW body;`
  - **PostgreSQL**：`CREATE TRIGGER "name" timing EV1 OR EV2 ON "t" FOR EACH ROW|STATEMENT EXECUTE FUNCTION fn();`
  - **SQL Server**：`CREATE TRIGGER "name" ON "t" AFTER|INSTEAD OF EV1, EV2 AS body;`（语句级）
- 方言门控：MySQL 拒绝语句级与 INSTEAD OF；SQL Server 拒绝 BEFORE（提示改用 AFTER / INSTEAD OF）。
- 校验：名称/事件必填；PG 必须有函数名、MySQL/SQL Server 必须有主体；名称去重。
- 验证：实机（MySQL 设计页）确认页面结构、可增删行（截图 `trg_tab2.png`）；
  三方言 DDL、门控与校验由单测覆盖；`cargo test --bins` **56 个用例全绿**。

**遗留**：WHEN 条件、UPDATE OF 列、启用开关、可延迟（PG）未迁；ALTER 差异仍未做。

---

## 本轮完成：视图 / 实体化视图「定义」页

- 视图类设计页签的第一个子页从「字段（占位）」改成「定义」：**代码编辑器**（复用查询页那套
  SQL 高亮 + 补全），正文写回 `DesignerState.view_body`，并计入「已修改」。
- 最后一页 SQL 预览改成由正文生成：`design_view_sql(kind, name, body)` 纯函数，
  空正文给提示注释、非空出 `CREATE <kind> <name> AS\n<body>;`（单测覆盖）。
- 编辑器在打开视图设计页签时创建（[ensure_view_editor](src/main.rs)），只对「视图 / 实体化视图」生效。
- 验证：`cargo build` 通过、`cargo test --bins` **57 个用例全绿**（新增视图 DDL 用例）；
  **实机截图仍未完成**——本轮排到一半发现更根本的问题：工具栏「设计表」原本只对 cat 0（表）开放
  （`design_enabled`），视图 / 实体化视图 / 函数的设计页签**根本打不开**；已放宽到 cat 0..=3。
  放宽后自动化点击对象行 + 工具栏动作仍未触发设计页签，截图留待下一轮用真实点击补齐。

**遗留**：函数设计器的参数/返回值/函数体仍是占位；视图「刷新选项」页未做；
视图字段列（由 SELECT 推导）未展示；ALTER 差异仍未做。

---

## 本轮完成：迁移收尾（#1–#5）

五个提交把 Flutter 版剩余的「不依赖驱动」功能一次收口：

- **#1 `67db688` 连接树去掉「我的连接」根节点**：分组与连接同级挂在根下，
  缩进按树深度递进；`Act::Root` 退为无选中的哨兵态，各处 match 仍认得它。
- **#2 `fbe5d00` MySQL 真驱动 + 演示连接机制**：新增 `src/mysql_driver.rs`
  （1331 行），`backend::Target` 接通真实执行与列名回填；连接对话框区分
  「演示连接」（不落库）与真连接，目录/取数/写回按目标分叉。其余引擎仍 mock。
- **#3 `dcb1694` 函数设计器与视图设计器补页**：函数设计器补齐参数 /
  返回值 / 函数体，视图补刷新选项；生成侧方言门、`createUnsupported` /
  `alterUnsupported` 拦口、差异签名、驱动反查四处同步。
- **#4 `c867a83` `.ano` 导入导出 + 数据导入向导**：新增 `src/data_import.rs`
  与 `src/data_import_dialog.rs`（1268 行）；`ncx.rs` 补 `.ano`
  （Redis base64 JSON）解析与写出，导入/导出双向可用；向导带进度与取消。
- **#5 `8aac43f` SQL 查找栏 / 关于对话框 / 已保存查询 / 运行 SQL 文件**：
  - `about_dialog.rs`：帮助 → 关于 / 问题反馈接线，链接走 `open` crate。
  - `saved_queries.rs`：`queries.json` 落盘（`DARO_DATA_DIR` 可覆盖），
    业务键 `连接|库|模式|名称` 覆盖保存；工具条「保存」真存盘、
    「已保存」下拉回填新页签并支持删除。
  - `sql_file_run_dialog.rs`：选 `.sql` → `split_sql_statements` 拆语句 →
    逐条 `Target::execute`，遇错即停开关、进度回显、可取消、失败明细不静默吞。
  - **SQL 查找栏的关键判断**：gpui-component 自带完整查找/替换浮层
    （命中高亮、上/下一个、计数、区分大小写、替换/全部替换），但
    `EditorState` 的 `searchable` **默认 false**，快捷键会向祖先冒泡 ——
    之前「无查找栏」的根因就是没开这个开关。故查询页编辑器显式
    `.searchable(true)`，再用 `open_search(replace, cx)` 把工具条
    「查找」「替换」与编辑菜单两项接到同一个面板；`ctrl-f` / `ctrl-h`
    同时生效。不自建文本查找逻辑，避免与内置命中高亮/选区两套口径打架。

验证：`cargo check` 0 error；`cargo test -- --test-threads=1`
**154 passed / 13 ignored / 0 failed**。本批新增 4 个守护用例：
查找栏按钮打开内置会话并命中 2 处、编辑菜单作用到当前查询页、
`queries.json` 落盘往返、保存覆盖与删除同时落到文件。
另有两个纯函数用例守护 `SavedQuery::key()` 与 JSON 往返。

**遗留**：表数据导出向导、SQL 格式结果导出、KV 浏览器、i18n 仍未做；
MCP 核心 7 个工具已落地（`daro-mcpd` 独立进程），例程 / 模式上下文 / 打开表
3 个工具待驱动层补齐 routine 元数据后再做。

---

## 本轮完成：设计表打开真库表带出真实结构

- 根因：`open_designer` 不论连接真假都用 `mock_design_cols(obj)` 等种子数据铺满 10 个子页，
  真库表的字段/索引/外键等全成了演示内容。
- 现在：真连接（`backend::is_real_conn`）打开**表**设计页时先摆空模型并标 `loading`
  （工具栏出「载入表结构…」），后台线程跑 `read_design_meta` 反查目录；回执按 `did`
  认领页签（期间关页签则丢弃），把工作集与基线一起填回，所以打开即「无变更」，
  编辑器再按子页建齐。演示连接与视图 / 函数仍走种子数据。
- 反查来源：SQLite `pragma_table_info/index_list/index_info/foreign_key_list` +
  `sqlite_master`（默认值 / 表注释）；MySQL `information_schema`（COLUMNS /
  TABLE_CONSTRAINTS / KEY_COLUMN_USAGE / REFERENTIAL_CONSTRAINTS / CHECK_CONSTRAINTS /
  STATISTICS / TABLES）；PostgreSQL `pg_attribute / pg_attrdef / pg_constraint /
  pg_index / pg_inherits / pg_class / pg_tablespace / pg_collation`。
- 顺带修复：`split_column_type` 拼回修饰符时丢空格（`decimal(10,2) unsigned` →
  `decimalunsigned`）；MySQL 函数索引（列名 NULL）整条跳过；PG 表达式索引不反查；
  MySQL FULLTEXT / SPATIAL 的 DDL 改成 `CREATE FULLTEXT INDEX …`（不跟 `USING`）。
- 验证：纯函数用例（类型拆分 / 默认值括号 / PG CHECK 表达式）、SQLite 端到端 UI 用例
  （打开设计页 → 字段/主键/自增/默认值/唯一键/索引/FK 动作全对上）、PG 真机用例
  （datawalk 开发库的 `daro_pg_test`：identity、默认值、FK 动作、CHECK + NO INHERIT、
  DESC 索引、表注释与 owner）。`cargo test -- --test-threads=1`
  **158 passed / 13 ignored / 0 failed**。
- **未做**：保存仍只建基线快照、不执行 ALTER / CREATE（提示「演示数据，不落库」口径未改）。

---

## 本轮完成：表右键菜单（对象面板并入树菜单 + 删除/清空/截断/重命名接线）

- 结构对齐：对象面板（中栏「列表 / 图标」视图）原先挂着一套只有 5 项的独立菜单，
  现删掉 `object_context_menu`，两个视图容器统一走树的 `node_context_menu`；行和
  格子加 `on_mouse_down(Right)` → `select_for_context`，右键先把光标下的对象选上
  再弹菜单（照参考界面：右键即选中），与树同一个菜单实体，不会同屏弹两个。
- 新接线（表 k==0）：删除表 / 清空表 / 截断表 / 重命名 → `table_ops::statement`
  按方言拼 DDL → 独立窗口确认框（重命名是文本输入框）→ 落成 `TableOpJob` 排队 →
  帧尾 `pump_table_ops` 投后台线程执行 → 回执 `table_op_done`：失败弹错误窗，成功
  只失效该库的对象清单（`catalog::invalidate_objects`）+ 该表的列缓存，并立刻重查
  一次，所以树和面板当场就能看到表没了 / 改名了。
- 为什么要排队：确认框 / 输入框的回调只拿到 `&mut DaroApp`，起不了后台任务，
  所以动作先落 `table_ops: Vec<TableOpJob>`，和 `pump_catalog` 同口径在帧尾派发。
- 已复用既有入口：导入向导、导出向导、复制（名字）、刷新、打开表、设计表、
  新建表▸常规。
- 仍灰（无真驱动/无功能，按房子规矩不放假入口）：通过配置文件打开表(Ent)、
  通过 AI 新建表(Ent)、复制表▸、设置权限、数据字典/数据生成/数据脱敏(Ent)、
  转储 SQL 文件▸、维护▸、逆向表到模型(Ent)、创建 BI 工作区(Ent)、管理组▸、
  共享…、创建打开表快捷方式…。
- 口径偏差：① SQLite 没有 `TRUNCATE`，截断表落 `DELETE FROM`；② 「复制」复制的是
  对象**名**到剪贴板（参考界面复制对象本身，对象级粘贴还没有）；③ 「导出向导」
  复用 `open_table_export`，它优先当前数据页签、其次面板选中行。
- 演示连接不假装成功：`demo` 连接的 DDL 项直接置灰，`table_op_job` 里再拦一道。
- 验证：`table_ops` 3 个方言纯函数用例（限定名 / 引号双写 / 改名留在原库）+
  2 个 headless 用例（真 SQLite 建 gone/keep → 删除后目录只剩 keep → 改名后目录
  变 renamed；演示 MySQL 的 DDL 项不可用也给不出任务，真库上改名空值/同名被拒），
  5 项全绿。**整仓 `cargo test -- --test-threads=1` 本轮没跑完**：另一个会话正在改
  网格 / catalog 重试（`Pending::run` 补一轮、列拖拽），全量跑在
  `designer_tests::designer_add_del_pk_notnull_autoinc` 处原生中断（单独跑该用例通过，
  中断点随对方提交在 ui_flows 与 designer_tests 之间移动），总数以对方收敛后为准。
- 顺带：菜单测试里的夹具连接换成用例专有名（`sqlite_fixture` 统一叫「本地 SQLite」，
  目录缓存按连接名建键，会和其他测试串数据）。

---

## 本轮完成：窗格空白处的右键菜单（对象面板 = 分类那一套，树空白按推断）

- 实测结论：参考界面在中栏对象列表的**空白处**右键，弹的就是当前分类节点那一套
  （新建表▸ / AI 新建表(Ent) / 导入·导出向导 / 数据字典·生成·脱敏(Ent) / 运行 SQL 文件 /
  在模式中查找(Ent) / 新建组 / 粘贴(灰) / 刷新）。所以没有新写一套骨架，
  直接让 `node_context_menu` 按 `ctx_blank` 分派到 `cat_menu`，目标取
  `pane_path()`＝面板正在看的 (连接, 库, 模式, 分类)，和树上分类节点同一份实现。
- 菜单实体挪位：原来挂在 `object_list` / `object_grid` 的内容层上，内容只有行高，
  行数不满一屏时下方空白右键不到。改挂在 `flex_1` 的滚动容器上（一个窗格仍只有
  一个菜单实体）。gpui-component 的 `ContextMenu` 在装饰元素**之后**才画内层，
  冒泡顺序因此是「行 → 容器 → ContextMenu」，`on_mouse_down` 必须写在
  `.context_menu(...)` 之前（`ContextMenu<E>` 不暴露该 builder）。
- 归属判定：右键点行时行先收到事件，`select_for_context` 除记目标外把 `ctx_row_hit`
  置真认领本次按下；随后容器上的 `blank_for_context` 见到已认领就只清标记、
  不改写目标。只有没被认领的按下才把 `ctx` 清空、记 `ctx_blank = Some(窗格)`。
- 树空白是另一套，且**没有可只读实测的样本**（参考界面这块读不到），按标准行为推断：
  新建连接 / 新建组 / 粘贴(灰，对象级剪贴板还没做) / 刷新，代码注释已标推断。
- 新接线：`run_sql_file_here(c, d, s)`（目标＝窗格正在看的库 / 模式，打开的对话框与
  查询页那条入口同一个）；`table_wizard_available()` 决定导入 / 导出向导能不能点——
  向导要一张具体的表（当前数据页签或选中行），空白右键时两者皆无就没有落点，置灰。
  为此加了 `wired_item(me, label, ok, run)`：有落点接回调、没落点转 `todo_item`。
- 保持灰的理由写进注释：连接 / 库层的「运行 SQL 文件…」不能接线——对话框没有选库
  的地方（`RunSqlDialog` 只显示固定目标），这两层没有确定落点，接了就是拿第一个库
  瞎跑语句。
- 不越界：数据 / 查询 / 设计页签所在的滚动壳不套分类菜单（`self_scrolls` 与 `has_tab`
  分成两条路），页签内容自带各自的右键菜单。
- 验证：新增 2 个 headless 用例（`blank_press_defers_to_a_row_that_claimed_it` 守
  行/空白按下归属与 `ctx_blank` 转移；`blank_menu_wizards_need_a_table_to_act_on` 守
  向导落点门禁）。原生菜单在无头测试里弹不出来，菜单内容层只能实机点。
  （这一句写于本轮：当时「弹不出来」是菜单实体挂错了渲染路径 + 断言放在了同一次
  `update_window` 里，见下一章；挂对路径后无头就能断言 `popup-menu`。）
  `tree_menu_tests` 9 项全绿。整仓 `cargo test --bin daro-rs -- --test-threads=1`
  **179 passed / 2 failed / 15 ignored**（上一轮的整轮原生中断本轮没有再出现）。
  两个失败都不在本轮改的区域，也不由本轮改动引起：
  ① `save_query_dialog::tests::save_as_menu_scrolls_instead_of_stretching_window`
  是另一会话刚加的在制文件；
  ② `ui_flows::sqlite_edit_cell_and_apply_through_ui` 单独跑通过、和同模块别的用例
  一起跑就挂（前面是 `record_limit_panel_wires_page_size`），是按执行顺序串味的老问题，
  本轮改动之前整仓跑同样挂它（那一轮该文件还编译不过，故本轮复跑取的是收敛后的数字）。

---

## 本轮完成：视图右键菜单（对象 + 分类/面板空白，DDL 串上对象类型）

- 结构对齐：`obj_menu` 原来只有 `k == 0` 一支是逐项实测的，视图和实体化视图 / 函数 /
  查询挤在同一个 `else` 骨架里、除「打开视图」外全灰。本轮把视图单独拆成一支，
  按参考界面实测的两张截图（视图对象菜单 15+3 项、视图分类 / 面板空白菜单）
  逐项铺；实体化视图 / 函数 / 查询继续走推断骨架，全灰不动。
- 接线（视图对象 k==1）：打开视图（原有）+ 设计视图 → `open_designer`（真库能带出
  视图的列名，只是「定义」页的 SELECT 正文不从驱动回填，属既有缺口）、新建视图 →
  `open_new_designer`、删除视图 → `table_ddl_item(Drop)`、导出向导 → 点名导这张视图、
  复制 / 刷新（共用尾巴已接）。
- 接线（视图分类 + 面板空白 k==1）：「新建视图」开未命名设计器页签；「导出向导」沿用
  表那层的门禁——有数据页签或选中行才可点，两者皆无＝没有落点，置灰。
- DDL 层按对象类型分叉（`table_ops::ObjKind`）：`Drop` 出 `DROP TABLE` / `DROP VIEW`，
  菜单文案、确认框正文、失败弹窗标题三处都跟着类型走（视图的确认框写明
  「视图定义会被移除，基表数据不受影响」）。可用性门禁 `applies_to(kind)` 一处收口：
  清空 / 截断根本不进视图菜单，`table_op_job` 再拦一道，两者都给不出任务。
- 为什么视图的「重命名」留灰：本轮实测 SQLite 直接回 `view vw may not be altered`
  （`ALTER TABLE … RENAME TO` 不认视图），改名得走「读 sqlite_master 的 SELECT 体 →
  CREATE VIEW 新名 → DROP 旧视图」这条重建链路；PG 要 `ALTER VIEW`、MySQL 能不能用
  `RENAME TABLE` 改名都没有实测样本。按房子规矩不放假入口，菜单项保留但置灰，
  重建视图体归后续排期。
- 设计器放开分类 1：`open_new_designer` 的门禁从只认 `cat==0` 改成 `k @ (0 | 1)` 并把
  `cat` 带进页签，于是标题（`tab_label` 的 `新建{分类}`）、保存时的提问文案
  （`design_save` 弹「视图名」而不是「表名」）自动跟着走；`obj` 仍是 `usize::MAX` 哨兵，
  不挂目录。保存本身仍是演示口径（提示「已创建（演示数据，不落库）」，不发 DDL）。
- 导出向导改为点名目标：新增 `open_table_export_for(act, cx)`，列名取目录、真连接把
  整张表 / 视图交给导出对话框后台取数、演示连接本地取一页。对象菜单（表 + 视图）改用
  它，右键点的是哪张就导哪张，不再跟着当前数据页签跑——上一轮记的口径偏差 ③ 就地消掉；
  分类 / 空白菜单那一层没有点名对象，仍走「当前页签优先、其次选中行」。
- 演示连接照旧不假装成功：`table_op_available` 仍然只认真连接，视图侧同样受限。
- 验证：`table_ops` 4 项（新增 `view_drop_uses_the_view_keyword_in_every_dialect`：
  三方言 `DROP VIEW` 限定名 + 清空/截断/改名在视图上不可达 + 文案跟类型）；
  `tree_menu_tests` 12 项，含两个本轮新增 headless 用例
  （`view_menu_drop_runs_on_real_db_and_the_rest_stay_grey`：真 SQLite 夹具建
  `base` 表 + `vw` 视图 → 删除任务拼 `DROP VIEW "main"."vw"` → 帧尾执行后视图清单空、
  基表清单仍是 `["base"]`，同时断言清空/截断/改名既不可点也给不出任务；
  `view_menu_new_item_opens_an_unnamed_view_tab`：新建视图页签 `cat==1` +
  `obj==usize::MAX` + `new_name==Some("")` + 标题「新建视图」）。
  整仓 `cargo test --bin daro-rs -- --test-threads=1`
  **183 passed / 1 failed / 15 ignored**；唯一失败仍是
  `ui_flows::sqlite_edit_cell_and_apply_through_ui`——单独跑通过、和同模块用例按顺序
  一起跑就挂，是另一会话网格 / 分页那块按执行顺序串味的老问题，本轮改动之前整仓同样挂它。
  原生菜单在无头测试里弹不出来，菜单条目本身（视图那 15+3 项的排布与灰态）只能实机点。
  （这一句写于本轮：当时「弹不出来」其实是菜单实体挂错了渲染路径，见下一章。）

---

## 本轮完成：对象面板右键菜单挂回实时渲染路径（实机「右键表没反应」）

**现象**：实机右键中栏对象面板里的表，菜单一次都不出；同一棵树右键连接 / 库节点正常。

**取证**（临时在四个环节插 `eprintln!`，跑私有实例把 stderr 落盘）：

```
PROBE panel row claim act=Obj(1, 0, NO_SCHEMA, 0, 0)   ← 行认领了按下
（没有 PROBE blank / PROBE builder called）              ← 容器与菜单实体一次都没收到
```

同一份二进制里，树上连接行的按下、容器空白判定、建菜单三步全打出来了，说明
ContextMenu 机制本身没问题，缺的是**面板这一侧根本没有菜单实体**。

**根因**：渲染树早先从单栏 `DaroApp::center()` 换成了 `dock::DaroApp::pane_view`
（[src/dock.rs:419](src/dock.rs)），`center()` 只留作参考实现并标了
`#[allow(dead_code)]`。前几轮把「对象菜单 + 面板空白菜单」的载体
（`.on_mouse_down(Right, blank_for_context)` + `.context_menu(node_context_menu)`）
挂在了 `center()` 的滚动容器上——那条路一帧都不渲染，所以面板右键永远只有
`select_for_context` 记下目标，祖先链上没有任何 ContextMenu 去开弹层。
树那侧一直挂在实时路径（`tree()` 的 `tree-scroll`），所以连接 / 库节点正常。

**修法**：把同一对载体挂到 `pane_view` 里 `has_tab == false`（首页对象列表 / 图标视图）
那条 `pane-scroll/{pane_id}` 滚动容器上；有页签时仍不挂（表格 / 查询 / 设计器各自带菜单）。
`center()` 里那份保持原样，不再改动。

**顺带纠正一条旧结论**：「原生菜单在无头测试里弹不出来」是错的——挂对路径后
`popup-menu` 元素在无头里就找得到。但有两个坑，前几轮踩的就是第二个：

1. `open_menu` 把建菜单推到 `window.defer`，而 gpui-kit 的 `render_frame` 只做
   `refresh + draw`，**不 flush App 的 effects**；deferred 回调要等退出
   `cx.update_window`（`App::update` 收尾的 `flush_effects`）才跑。所以「同一次
   闭包里右键 → 连渲染两帧 → 断言 `popup-menu`」必然看不到菜单，看着像机制坏了。
   正确写法是拆成两次 `cx.update_window`：第一次按下，第二次再渲染并断言。
2. 载体挂错路径（就是本轮的死 `center()`），一次都不会建菜单。

之前那个假象是这两条叠出来的，所以前几轮的空白菜单只守了状态转移。

**验证**：

- 新增回归用例 `panel_row_right_click_opens_the_object_menu`：hover 到面板表行 →
  右键 → **退出这次 `update_window` 再渲染两帧** → `popup-menu` 必须出现，且
  `ctx == Some(Act::Obj(0, 0, NO_SCHEMA, 0, 0))`、`ctx_blank == None`（目标是被
  右键的那一行，不是上一次选中的）。
- 实机（私有实例，PID 35004 / hwnd 3279548，`.scratch/p5-*.png` 五张）：
  面板卡片右键出表菜单（打开表 / 设计表 / 新建表▸ / 删除表 / 清空表 / 截断表 /
  复制表▸ / 导入·导出向导…，Ent 角标与灰态齐）；面板空白右键出分类那套
  （新建表▸ / 导入向导… / 导出向导… / 刷新，AI 新建表·数据字典·数据生成灰）；
  树里的表行右键同样出对象菜单；点「打开表」真的开出 `dev_notes @main (本地 SQLite)`
  数据页签并渲染出三行数据。
- 临时探针已全部撤除（`grep PROBE src/*.rs` 无残留）。
- 整仓 `cargo test --bin daro-rs -- --test-threads=1`
  **193 passed / 3 failed / 15 ignored**，本轮新用例在内。3 个失败都不是本轮改动：
  `tool_view_tests::ribbon_other_dropdown_opens_events_panel`（单独跑也挂）与
  `ribbon_user_opens_readonly_users_panel`（单独跑通过，按顺序串味）是另一会话正在
  改的 ribbon 工具视图；`designer_tests::cell_edit_right_click_shows_app_text_menu`
  单独跑挂在「`popup-menu` 必须出现」那条断言上，而它的工作区里还留着
  `[probe] after right click` 的 eprintln（`git diff` 是未提交的 + 行），也就是同一
  会话正在查这条——挂的原因就是我上面第 1 条：它在同一次 `update_window` 闭包里
  右键完直接断言，deferred 建菜单还没 flush。拆成两次 `update_window` 就能看见菜单。
- 交付实例复验（`.scratch/daro-rs-fixed-1105.exe`，PID 37408 / hwnd 332684，
  `.scratch/h-*.png`）：树里展开 本地 SQLite ▸ main ▸ 表 → 面板列出 `dev_notes` →
  右键该行出完整对象菜单（`.scratch/h-6.png`）→ 点「打开表」开出
  `dev_notes @main (本地 SQLite)` 数据页签并渲染出三行（`.scratch/h-7.png`）。
  注：面板要先把树选到连接 / 库节点才列出对象，只展开分类节点时右侧是空的。

---

## 本轮完成：对象面板工具条随分类变

**现象**：中栏对象面板顶部的工具条原来是一套写死的「打开表 / 设计表 / 新建表 /
删除表 / 导入 / 导出」，切到视图 / 实体化视图 / 函数 / 查询也不改文案和动作。

**修法**（[src/main.rs](src/main.rs) `DaroApp::action_bar`）：

- `打开 / 设计 / 新建 / 删除` 一律拼当前分类名：`打开{kind}` 等，`kind` 取
  `CATEGORIES[cat]`。动作按分类分派——查询分类的「打开 / 删除」落在已保存查询
  （`open_saved_query` / `delete_saved_query`），其余落对象（`open_object` /
  `ask_table_op(Drop)`）。
- 「设计」对表 / 视图 / 实体化视图 / 函数开放；「新建」对表 / 视图 / 查询
  （函数的新建在下一轮补上；实体化视图还没接，按既有口径置灰 / 不放）。
- 「通过 AI 新建表」只在表分类出现；「导入向导」只对表成立；
  「导出向导」表 / 视图 / 实体化视图都有（导的是数据，`open_table_export_for`
  按 `cat` 取目录项）。三者都要选中对象才可点，未接的动作不放可点按钮。
- 工具条只在对象列表首页（没有活动页签）出现，文案与动作都不依赖上一次页签。

**验证**：新增无头用例 `object_action_bar_follows_category`：视图分类出
`打开视图 / 设计视图 / 新建视图 / 删除视图 / 导出向导`、没有 `导入向导` 与
AI 新建表；表分类出全套导入导出；实体化视图有导出无导入；视图分类点「新建」
开的是视图设计器（`tab.design == true`、`tab.cat == 1`、`tab.obj == usize::MAX`）。
同模块 `designer_entry_via_action_bar_dedups`、`object_panel_search_filters_list`
一并回归通过。

---

## 本轮完成：函数分类动作条与「新建函数」

参考界面的函数分类只有四项：**设计函数 / 新建函数 / 删除函数 / 运行函数**（没有
单独的「打开函数」——打开就是设计页）。

- 动作条对 `cat == 3` 去掉「打开函数」，补上「新建函数」「运行函数」。
  「新建函数」走 [open_new_designer](src/main.rs)（放开到 cat 3）：开一张空
  函数设计页（参数 / 返回值 / 函数体全空，`obj = usize::MAX` 哨兵，页签标题
  先显示「新建函数」），保存时弹「函数名」输入框再落名。
- SQL 预览的名字优先取保存时输入的新名字（新建视图 / 实体化视图同样受益），
  没命名时如实留空。
- 分类右键菜单的「新建函数」同步接线（原来是无动作的灰项）。
- 「删除函数」「运行函数」先占位灰：删要区分 `DROP FUNCTION` / `DROP PROCEDURE`
  的例程签名，运行要参数录入与回执面板，两样都还没有；按既有口径不放
  「点了没反应」的假入口。

**验证**：`object_action_bar_follows_category` 增加函数分类断言（四项齐、
无「打开函数」、无导出）；新增 `new_function_designer_from_action_bar` 断言
点「新建函数」开的是 `cat == 3`、`obj == usize::MAX`、`new_name == Some("")`
的设计页签。`function_designer_params_returns_and_sql`、`designer_click_type_save`、
`new_table_designer_flow`、`view_menu_new_item_opens_an_unnamed_view_tab` 回归通过。

---

## 本轮完成：Ent 入口全部接通（点击弹「敬请期待」）

前几轮的口径是「Ent 工具不要 / Ent 项不放 / 置灰占位」，本轮按用户要求翻成
**入口一律接通，点开弹「敬请期待」说明窗**：功能本体还没做，但角标项不再是死灰块——
点得到，也当场看懂「这是企业版功能，正在开发」。

- 统一落点 [ent_coming_soon](src/main.rs)：文案「「X」是参考界面企业版功能，daro-rs
  正在开发中，敬请期待。」，走 [open_notice](src/prompt_dialog.rs) 的独立窗口（房子规矩：
  这类框都是顶层窗口，不在主窗叠遮罩），确定 / Enter / Esc 关窗。
- 菜单侧 `ent_item(&me, "…")` 取代全部 `todo_item(_, true)`（26 处）：数据字典... /
  逆向▸到模型... / 通过 AI 新建表 / 通过配置文件打开表 / 数据生成... / 数据脱敏... /
  在模式中查找 / 创建 BI 工作区... 以及菜单栏「查看 ▸ 详细信息」。自绘菜单行本来没有 id，
  无头用例点不到，所以 `mi_with_id` 给这些项加 `ent-menu/<标签>` 锚点。
- 功能区：备份 / 自动运行 / 模型 / BI 四枚磁贴可点。
- 数据页工具条补回带角标的 6 项：表配置文件、AI 自动填充、数据分析、数据生成、数据脱敏、
  创建 BI 工作区（后三项只在非视图出现，照参考界面）。
- 查询页：查询创建工具、询问 AI（下拉里的解释 / 优化 / 生成 SQL 三项同样走这条提示）；
  结果面板内条「数据分析」；设计器 11 个子页工具条尾部「询问 AI」；对象面板动作条
  「通过 AI 新建表」；状态栏「详细信息」视图钮。
- 导出向导第一步的 5 个 Ent 格式（Excel .xls/.xlsx、Access、DBase、Parquet）：整行可点
  弹说明窗，单选位仍不给选（真导出落地后才放开）。
- 仍然置灰的只剩**非 Ent** 的驱动欠账项：设置权限、管理组▸、共享、复制表▸、
  转储 SQL 文件▸、维护▸、创建快捷方式，以及函数「删除函数 / 运行函数」。
  `todo_item(label, false)` 一条没动。

**验证**：新增 `ent_tests` 4 个无头用例——功能区 4 枚磁贴、对象右键菜单「数据字典...」、
数据页 6 个 Ent 按钮、导出向导第一个 Ent 格式行，逐个断言「点得到 → 弹出说明窗 →
文案点名该功能 → 确定后回到原窗口」。

---

## 下一步顺序

1. **导出向导（表数据 / 多表批量）** —— 带进度与取消；`db_export.dart` 的
   `ExportJob` / `BatchExportResult` 口径。
2. **视图 / 函数设计器**（现为简化版）。视图侧两处欠账：「定义」页的 SELECT 正文
   不从驱动回填（打开已有视图是空正文）；对象菜单的「重命名」置灰，补它要建
   「读旧视图体 → CREATE VIEW 新名 → DROP 旧视图」这条重建链路（各家语法需实测）。
3. **驱动层**（单独排期）：pgsql → sqlserver → sqlite → mongo/redis；
   MySQL 已落地，落地前先做「连接测试」，再把 mock 数据源逐块换成真实查询。
4. **i18n**（中/英/日，照 ARB key 表迁移）。
5. ~~**MCP 服务**~~（✅ 2026-11-10：`daro-mcpd` 独立进程 + 选项窗 MCP 页 + 15 条测试；
   例程 3 个工具等驱动层 routine 元数据补齐后再加）。
6. ~~补 `.ano` 导入导出与数据导入向导~~（✅ #4 已完成）。

> 顺序原则：先把「纯 Rust 能自己做、不需要驱动」的功能迁完（导入导出、编辑器、设计器），
> 再进驱动层这个大件。

---

## 控件尺寸规范（所有新对话框必须遵守）

尺寸只有一处定义：[src/ui.rs](src/ui.rs)。各文件里**不要再手写** `.h(px(24.))`
这类数字，用 `ui::` 的构造函数与常量：

| 用途 | 规格 | 用法 |
|---|---|---|
| 标准按钮 / 单行输入 | 高 24，水平留白 12 | `ui::primary_button` / `ui::secondary_button` / `ui::text_input` |
| 列表工具条小按钮（全选、分隔符选择） | 高 22，水平留白 8 | `ui::compact_button` |
| 表单左侧标签 | 宽 64，12px、muted 色 | `ui::form_label` |
| 对话框标题栏 / 底栏 | 24 / 46 | `ui::TITLE_H` / `ui::FOOTER_H` |
| 对话框内列表行 | 28 | `ui::DIALOG_ROW_H` |

底栏布局统一为：**主按钮 → 次按钮 → 弹性空白 → 右侧状态文字**（例：
`导入 | 取消 | …… | 已选 6 / 共 8 条`）。主按钮用 `primary_button`，
取消 / 关闭 / 浏览… 一律 `secondary_button`。

已按此规范改造：导入连接、导出连接、导出结果、文本输入框、确认框、筛选值弹窗。
连接向导的输入框也已统一为 24 高。


## 本轮完成：对象面板网格态改多列流式（先竖后横）

Flutter 版 `object_panel.dart` 是多列流式（参考截图：条目先从上到下填满一列、
再从左到右开新列），原网格态是行优先的大瓦片，这次整块换掉：

- **条目形态**：网格态条目与列表态同款（`obj_row`：16px 图标 + 名字一行，
  行高 17），不再是 96×68 大瓦片；列宽 250（`OBJ_COL_W`），列高 = 滚动容器
  实测高（首帧回落窗口高），每列条数 = `(视口高 − 16 + 4) / 21`。
- **滚动方向**：网格态改横向滚动（`overflow_x_scroll` + `horizontal_scrollbar`），
  列表态仍竖向。两态都改用裸 `overflow_scroll + track_scroll`（数据页网格同款）
  ——原来套的 `overflow_y_scrollbar`（Scrollable 包装）滚动发生在包装内部那层
  keyed handle 上，调用方 handle 收不到偏移，虚拟化窗口读不到真实滚动位置。
- **虚拟化**：`grid_row_window` 重写成列口径（按横向偏移取可见列，左右各留
  2 列缓冲），两侧用等宽空白撑住滚动范围；纯函数用例
  `object_grid_window_counts_columns_and_clamps` 按新口径重写。
- **布局坑**：gpui 的 `div()` 默认 `Display::Block`，滚动容器里的块级子元素
  宽度会被拉满视口、量不到内容宽（横向 content_size = 0），网格态的滚动
  容器必须显式 `.flex()`，内容层 `flex_none` 才按内容撑宽。
- **测试**：新增 headless 用例 `object_grid_fills_top_to_bottom_then_left_to_right`
  ——真 SQLite 建 200 张表，断言横向滚动有量（`max_offset().x > 0`）且相邻条目
  「同列向下或换列回顶」；注意目录缓存按连接名建键，fixture 连接要换名防串。
  全量 `cargo test -- --test-threads=1` 221 passed / 0 failed。
- **遗留**：列表态的竖向虚拟化此前因同一 Scrollable 问题收不到偏移，本轮一并修好；
  框选（橡皮筋多选）已在后续一轮补齐，列表态与网格态都可用。


## 本轮完成：所有子窗口默认模态

要求：子窗口开着时必须先关掉才能继续操作主窗口（参考界面的对话框行为）。

- **统一入口**：`src/ui.rs` 新增 `dialog_options()` = `TitleBar::window_options()`
  再设 `kind = WindowKind::Dialog`。所有对话框开窗改走它：关于、新建/编辑连接、
  数据导入、导出结果、筛选值、`.anc` 导入导出、运行 SQL 文件、表导出、
  文本输入/确认/提示、保存查询。
- **模态怎么来的**：gpui-pre 在 Windows 上创建 `WindowKind::Dialog` 时对
  `GetActiveWindow()` 调 `EnableWindow(parent, false)` 并设 owner，窗口销毁时
  恢复并 `SetForegroundWindow(parent)`。嵌套弹窗（如向导里再弹确认框）锁住的
  是当前活动窗，逐层关闭、逐层解锁。
- **不模态的部分**：下拉浮层（`ComboPopup`，如保存查询的名称输入）仍用
  `WindowKind::PopUp`；右键菜单、菜单栏下拉是窗口内浮层，不涉及锁主窗。
- **验证**：实机（非 headless）用窗口枚举断言——新建连接对话框的
  `Owner = 主窗 HWND`，此时主窗 `IsWindowEnabled = False`；`WM_CLOSE` 关掉
  对话框后主窗恢复 `Enabled = True`。headless 测试拿不到真实 HWND
  （`window_handle()` 返回 `NotSupported`），故 Win32 层断言不放进测试套件。

---

## 本轮完成：左右侧栏拖分栏缝改宽

需求：主窗口三栏里左连接树、右信息卡的宽度都不再写死，鼠标拖边缘即可调整。

- **状态与落盘**：`LEFT_W` / `RIGHT_W` 降为默认档（250 / 315），`DaroApp` 新增
  `left_w` / `right_w` / `side_drag` / `viewport_w`，`Store` 加两个
  `#[serde(default = …)]` 字段（旧 connections.json 缺字段按默认档起步），松手时
  跟主题 / 视图模式一起写回 `connections.json`。
- **三栏渲染**：`body()` 的两侧宽度、状态栏第一段宽度、树底筛选弹层的 `left`
  偏移全部改用动态宽度；中栏仍是 `flex_1 + min_w(CENTER_MIN_W)`。
- **把手**：分栏缝用**绝对定位**的 5px 透明热区压在左右栏边框上（不做 flex
  子元素，省得多占一栏宽度、破坏对齐），`cursor_col_resize()`，hover / 拖动中
  亮一条淡主色；按下时 `stop_propagation()`，否则同一次按下会漏给下面的树行
  改选中项。右栏拖的是**左边缘**，位移要取反。
- **事件**：拖动期间正文里盖一层全尺寸透明 `occlude()` 板接住 move / up，
  避免鼠标划过树 / 网格误点；根视图另挂全局 `on_mouse_move` / `on_mouse_up`
  兜底——指针划到标题栏 / ribbon / 状态栏仍跟手，在窗口任何位置松手都能
  收状态并落盘（同 `grid_header_release` 的口径）。
- **夹紧与性能**：左 180..560、右 220..560，且按本帧 `viewport_w` 给中栏留
  `CENTER_MIN_W + 16`；`clamp` 的上限先 `max(min_w)` 兜底，窄窗口也不会
  min > max 触发 panic。宽度取整后没变就不 `cx.notify()`——拖侧栏要重建整帧，
  跟表头列宽拖动一样少画一帧是一帧。
- **验证**：新增 headless 用例 `side_panels_resize_by_dragging_edges`：左右把手
  各拖一次（250→310 / 315→355）、断言中栏仍有余量、上下限夹紧、松手收状态、
  两个宽度落盘；`cargo check --bin daro-rs` 通过，全量
  `cargo test --bin daro-rs -- --test-threads=1` 230 passed / 0 failed / 15 ignored。
- **环境坑**：本轮机器上有另一路 cargo 在同时编 / 测同一个 `target/debug`，
  直接 `cargo test` 会撞 `LNK1104 无法打开输出 exe`、跑到一半的测试二进制也
  会被覆盖导致输出截断。改跑已编译好的 `target/debug/deps/daro_rs-*.exe`
  （先复制一份到临时目录再跑）即可绕开，不影响结论。


## 本轮完成：没有打开数据库时 Ribbon 分类按钮仍能切对象面板

**口径**：参考界面里即使一个数据库都没打开，顶部 Ribbon 的「表 / 视图 /
实体化视图 / 函数 / 查询」仍可点击并切换中栏对象面板显示的分类；区别只在
于没有库上下文时清单为空、动作条上的对象操作按钮全部不可用。

**原实现**：`DaroApp::select_category` 里 `anchor_db()` 取不到（没选中连接、
连接未打开、或目录清单还没回来）就直接 `return`，按钮点了没有任何反馈；
`pane_cat()` 为 `None` 时 Ribbon 高亮、动作条文案、面板图标一律回落成「表」。

**修法**（[src/main.rs](src/main.rs)）：

- 新增 `idle_cat: usize`：没有库上下文时对象面板正在看的分类（纯界面瞬时
  状态，不落盘）；`display_cat()` = `pane_cat().unwrap_or(idle_cat)`，只供显示层
  取用（Ribbon 高亮 / `pane_label` / `pane_icon` / 动作条文案 / 列表与网格图标）。
- `select_category` 先把 `idle_cat` 记成新分类再判 `anchor_db()`：取不到就只
  切分类返回，不伪造 `Act::Cat` 路径、不把假节点写进 `expanded` / `visited_dbs`；
  有库上下文时仍走原来「自动落到初始库并展开到该分类」那条路。
- **可操作判定没有放宽**：`action_bar` 的 `has_ctx` 仍只看 `pane_cat().is_some()`，
  `can_new / can_open / can_design / can_delete / can_import / can_export` 照旧全灰；
  `pane_objects()` 仍返回空清单。`pane_path()` 的 cat 不动，空白右键菜单的
  目标口径不受影响（搜索与 Ent「敬请期待」入口属界面级入口，保持可点）。

**验证**：新增 `idle_category_tests` 两条 headless 用例——无库时点「视图」后
分类、Ribbon 高亮、动作条文案跟着变，选中路径 / 展开态 / 对象清单 / 页签
都不动，点禁用的「新建视图」不开页签；有库上下文时仍以 `Act::Cat` 为准，
关库回落后保持最后点过的分类。
`cargo test --bin daro-rs -- --test-threads=1`：230 passed / 0 failed / 15 ignored。

---

## 本轮完成：主标题栏不显示 logo / 应用名

- `title_bar()` 中间拖动带里的 `db_icons::logo(15.)` 与 `daro-rs` 文本整体删除，
  连带清掉只为这两个元素服务的 `items_center / justify_center / gap /
  overflow_hidden`；`title-bar-drag` 这块本身保留，`window_control_area(Drag)`
  与「窗口失活时补发一次非客户区按下」的逻辑原样不动，中间变成一条可拖的空白带。
- 关于对话框里的 52px logo、菜单项「关于 daro-rs」、系统任务栏标题都不在本轮范围。
- **验证**：纯 UI 删除，`rustfmt --edition 2024` 解析整个 `main.rs` 通过。本轮做
  编译时工作树里另有在途改动（视图设计器预览 / 美化 SQL、解释计划）尚未收口，
  且数据盘写满，未单独跑出全量回归；改动与那些编译错误无关，收口后统一编即可。

---

## 本轮完成：查询页「解释」（执行计划）

- `backend::Target::explain_sql`：SQLite 用 `EXPLAIN QUERY PLAN`（`EXPLAIN`
  只出字节码），MySQL / PG 用 `EXPLAIN`；用户已写 EXPLAIN 时原样执行，
  不叠第二层。
- 解释目标跟编辑器走：有选区解释选区（多条逐条），否则取光标所在语句；
  光标落在语句间空白时取最近一条。演示连接 / 未接驱动的引擎只弹说明窗，
  不生成假计划，也不冲掉已有结果。
- 计划复用结果面板：列头 / 网格 / 导出沿用同一套渲染，页签与状态栏文案
  改为「执行计划 N / 执行计划 · X 行」，消息页写「> 执行计划」而不是
  「> OK」；真库文件仍走后台任务，界面不冻结。
- **验证**：新增 `explain_tests` 四条 headless 用例——目标语句选择、三方言
  EXPLAIN 前缀 / 防重复、真 SQLite `EXPLAIN QUERY PLAN` 出 4 列计划并自动
  切页、演示连接拒绝假计划。`cargo test explain_ -- --test-threads=1`：
  4 passed / 0 failed。

---

## 本轮完成：视图 / 实体化视图设计器「预览」「解释」

**口径**：参考界面的视图设计器工具条是「保存 ┃ 预览 解释 视图创建工具 美化 SQL」
（图标 + 文字，不是纯图标）。点「预览」执行的是「定义」正文里那条 SELECT 本身，
不是 `CREATE VIEW` 的 DDL；底部随即弹出「消息 / 预览」双页签面板，预览页自动激活
并出只读结果网格。点「解释」把第二个页签换成「解释」，正文是执行计划。

**修法**（[src/main.rs](src/main.rs)）：

- `DesignerState.preview: Option<DesignPreview>` 装回执（`QueryOutcome`）、消息页
  日志、当前页签与滚动句柄；没点过就不占底部空间，函数设计器（cat 3）不出现这两枚
  按钮也不出面板。
- 工具条由纯图标改成图标 + 文字：`dsave` / `dpreview` / `dexplain` / `dbuilder` /
  `dpretty`。「视图创建工具」按既有口径出「尚未接入」说明窗。
- `design_preview(did, explain)`：页签坐标 → `backend::Target`，`explain` 为真时
  下发方言的 EXPLAIN 前缀语句（复用查询页那套 `explain_sql`），同步取数、行数按
  驱动上限。正文为空 / 演示连接 / 正文不是查询这三种情况只在消息页写说明，
  不铺空网格。
- 面板高 `D_PREVIEW_PANEL_H = 300`，与查询页结果面板同一默认高（同样暂不可拖）；
  消息页沿用查询页回执口径（语句原文 + `> OK` + `> 查询时间`）；预览页是只读网格：
  单行列头、斑马纹、当前行浅灰铺满且行首槽压一块深灰、数值列右对齐、底条「共 N 行」，
  行虚拟化与结果网格共用 `visible_row_window`。
- 执行计划按列数分流：单列（PostgreSQL 的 `QUERY PLAN` 文本行）铺成纯文本 `dpplan`
  并带 Consolas，对齐参考界面截图；多列（SQLite `EXPLAIN QUERY PLAN` 的 4 列、
  MySQL 的表格）仍走网格。
- 「美化 SQL」`design_prettify` 对编辑器正文跑查询页同款 `sql_format::pretty`，
  写回经变更订阅同步 `view_body`，「已修改」照常亮。

**验证**：新增 `explain_tests` 两条 headless 用例——`view_preview_runs_body_and_fills_grid`
（真 SQLite 夹具：预览跑通、自动切预览页签、`dpcell/0/0` 出网格、切回消息页网格让位给
回执、点「解释」出计划且消息页记下真正下发的 `EXPLAIN QUERY PLAN`）与
`view_explain_single_column_plan_renders_as_text`（单列计划铺 `dpplan` 文本、不套网格）。
`cargo test --bin daro-rs explain_tests`：6 passed / 0 failed。

**环境坑**：另一路会话同仓同 `target/` 在途改 `main.rs`，本轮多次撞到
`LNK1104 无法打开输出 exe`（那是对方正在跑全量测试二进制）与在途编译错误；
等对方跑完再编即可，结论不受影响。另外预览要落真库，夹具连接名独占
（「视图预览测试」）并在收尾 `sqlite_driver::close`，避免目录缓存串味。

---

## 本轮完成：运行 / 解释按钮随选区换文案

- 编辑器有选区时，主按钮「运行」与「解释」自动变成「运行已选择的 /
  解释已选择的」；选区清空后回到原名（参考界面口径）。点下去本来就只作用于
  选区，按钮文案现在与行为一致。
- 实现：`QueryState::has_selection` 由 `cx.observe(&editor)` 写回，只在
  「空 ↔ 非空」翻转时 `cx.notify()`，光标闪烁 / 普通击键不重绘整页；工具条
  文案与无障碍标签共用同一状态（`aria_label` 随之切换，便于无头测试断言）。
- **验证**：`query_run_selected_only` 扩展断言——无选区出「运行 / 解释」，
  shift-end 选中文首行后出「运行已选择的 / 解释已选择的」，并照旧只执行
  选中的一条；`cargo test query_`：17 passed / 0 failed，
  `explain_tests::`：5 passed / 0 failed（含并行会话新增的视图解释用例）。

---

## 本轮完成：右侧信息面板 DDL 页

**口径**：Flutter `lib/widgets/database_info.dart` 的对象详情面板顶部是
「ⓘ 信息 / DDL」两枚页签 + 依赖图图标 + 最大化；DDL 页显示语法高亮的
`CREATE TABLE …`（PG 还带 `ALTER TABLE … OWNER TO`），底部横向滚动、右上角复制。

**做法**（[src/main.rs](src/main.rs)）：

- 状态收在 `DaroApp`：`info_page`（0 信息 / 1 DDL）、`info_ddl`
  （`Loading / Ready(sql) / Error`，带缓存 key）、`info_restore_w`
  （最大化前的右栏宽度，`None` = 未展开）。都是瞬态，不落 `connections.json`。
- DDL 页签只在「真库 + 表对象（含字段行）」出现（`info_ddl_capable`）；
  选中项换成库/模式/视图/函数时，`render` 把 `info_page` 归零，页签随之消失。
- 取数复用设计器链路：`read_table_ddl` = `read_design_meta`（SQLite pragma /
  MySQL information_schema / PG pg_catalog）+ `design_create_sql_at`，与
  「设计表 → SQL 预览 → DDL」同一份口径；缓存 key 用
  `连接\u{1}库\u{1}模式\u{1}表` 而不是目录下标，连接改名 / 重排不会串 DDL。
- 首次切到 DDL 页由 `cx.defer_in` 的 `pump_info_ddl` 投后台反查，同一 key
  在飞不重复发；回执 `info_ddl_loaded` 先比对 key，选中项已经换掉就丢弃。
- 正文三态：载入中 / 读取失败（带原因）/ 就绪。就绪时用
  `sql_preview::plain_row` 逐行铺高亮，外层 `overflow_scrollbar` 双向滚动，
  右上角复制按钮把整段 SQL 写入剪贴板。演示连接 / 未连接只出说明，
  不伪造 DDL，也不发查询。
- 顶部最大化按钮与参考界面一致：展开到右栏上限 `RIGHT_MAX_W`，再点回到
  展开前的宽度。

**验证**：`table_ddl_matches_designer_create_statement`（真 SQLite 夹具：
列 / 主键 / 外键 / 唯一索引 / 普通索引全部进语句，且不夹带 MySQL 的
AUTO_INCREMENT / DEFAULT）与 `info_panel_ddl_page_reads_real_table`
（headless：页签出现 → 点开 → 异步就绪且 `info-ddl` 落地 → 复制按钮 →
最大化往返 → 切回信息页 → 演示连接只剩说明且不发起反查）。
`cargo test --bin daro-rs -- --test-threads=1`：240 passed / 0 failed / 15 ignored。

---

## 本轮完成：视图设计器「规则」子页（PostgreSQL）

**口径**：参考界面的 PG 视图设计器在「定义」右边还有一枚「规则」，表头五列
名称 / OID / 事件 / 代替运行 / 注释，只播种一条空白行、当前格（名称那格）
整块铺实蓝白字；工具条在「美化 SQL」后再多出「添加规则 / 删除规则」两枚；
底部两行属性「位置:」「定义:」各是只读字段 + `...` 按钮。非 PG 引擎的视图
没有规则，子标签仍是「定义 / SQL 预览」两枚。

**做法**（[src/main.rs](src/main.rs)）：

- 子标签按引擎给：`view_subs_pg = 定义 / 规则 / SQL 预览`，只有
  `conn.kind == PostgreSql` 才用（`view_or_function_designer`），内容分发
  `cat == 1 && sub == 1` 落到新的 `view_rules_tab`。
- 新写 `view_rules_tab` 而不是改表页的 `rules_tab`：并发会话在改后者，且两页
  列口径不同（表页 名称/事件/代替运行/启用/注释，视图页多一枚只读 OID、去掉
  启用），列宽常量另立 `D_VIEW_RULE_WIDTHS`（截图 501/201/161/161/601 物理
  像素 ÷ 2 缩放 = 250/100/80/80/300，加行首槽 12 正好铺满 822 的窗格）。
- 当前格取实心态：`D_RULE_CELL_CUR = #0078D7` 整格铺满、字色转白，表头对应列
  用 74% 混白的强调底 + 2px `#4DA0E1` 底边；这与表页规则格的「白底 + 强调边」
  是两套取样口径，各自照各自截图。
- 复用表页的机制而不复制逻辑：`design_pick_rule_cell` / `design_set_rule_action`
  / `design_add_rule` / `design_remove_rule` / `spawn_rule_editors` /
  `drop_cell` + `RULE_ACTIONS` / `rule_props`（位置 = 条件、定义 = 语句，`...`
  开独立小窗）全部照用；列宽拖动用 `ds.widths` 的 `(sub=1, col)` 键，与表页的
  `(sub=6, col)` 不串。
- `design_rule_sql` 补上 PG 语法要求的 `TO 目标`：签名加 `to: &str`，视图页传
  `sql_preview::qualify(kind, schema, 视图名)`，SQL 预览在 `CREATE VIEW` 之后逐条
  追加 `CREATE RULE … ON event TO "public"."v_x" DO …`；表页显式传空串，输出与
  改动前逐字节一致（`design_rules_generate_pg_only` 仍绿）。
- 保存态：视图 dirty 判定加上 `ds.rules != ds.baseline_rules`，规则改动会点亮
  「保存」并走同一条 DDL 下发链路。
- 默认当前格：视图页 OID 插在名称之后，`rule_cur` 的种子按 `cat` 分流
  （`k == 1` 给 0，其余仍是 1），首屏高亮落在名称格，与截图一致。

**没做的两处（有意留白）**：截图里视图还有「高级」「注释」两枚子标签，本轮没补
——它们与并发会话正在改的属性区重叠；表页 `CREATE RULE` 至今不带 `TO`（PG 真机
会报语法错），本轮只把视图侧补对，没顺手改表页。

**验证**：`explain_tests` 新增两条 headless 用例——
`view_rules_subtab_grid_and_props`（PG 夹具：三枚子标签且无第四枚 → 五列表格 +
五枚列宽把手 → 工具条添加/删除 → 底部位置/定义两个 `...` → 首屏当前格在名称列
→ 点事件格 `rule_cur` 跟到 2 → 末行空白时添行不叠行 → 填名后添/删各生效，删的
是选中行 → 切 SQL 预览规则格退场）与 `view_rules_subtab_absent_for_other_engines`
（MySQL 视图只有两枚子标签，第二枚出 SQL 而不是规则格）；另有单元用例
`rule_sql_targets_object_when_to_supplied` 钉住带 `TO` 的完整语句、表页不带 `TO`
与空白行不出语句。变异抽查：把 `rule_cur` 种子改回 1、把非 PG 的子标签加上「规则」
都能把对应用例打红。
`cargo test --bin daro-rs -- --test-threads=1`：243 passed / 0 failed / 15 ignored。

**环境坑**：`view_rules_tab` 表头列宽把手最初在 `move` 闭包里调 `cw(i)`（借用
`ds`）撞到 `E0521 borrowed data escapes`，照表页做法把宽度提到闭包外；同仓并发
会话仍在改 `main.rs`，本轮链接期未再撞 `LNK1104`。

## 本轮完成：新建实体化视图设计器（1:1 截图）

参照截图（`无标题 @daowei_dev.public (shucon…` 页签）补齐「新建实体化视图」：

- 入口全接：分类动作条「新建实体化视图」（`act-new`，`can_new` 已含 cat 2）、
  实体化视图分类右键菜单、对象右键菜单三处都走
  `open_new_designer(Act::Obj(c, d, s, 2, 0))`；新页签标题按参考界面显示
  「无标题 @库.模式（连接）」。
- 页签图标：新增 `art::Glyph::DesignMatView`（红卡片 + 圆眼镜 + 铅笔），
  Dock 页签与顶部页签栏同款；素材模块不收录，任意尺寸都手绘。
- 工具条：`保存 | 预览(蓝眼睛 PreviewEye) 解释 | 视图创建工具(蓝面板 + 灰腿，
  ViewBuilder) 美化 SQL`，Ent 角标挂在图标左下；尾部「设计器 · 名字」标签删掉。
- 子标签：按截图改为 `定义 / 高级 / 注释 / SQL 预览`（原「刷新选项」页退场，
  `matview_concurrent` 状态与 `design_matview_refresh_sql` 保留给 SQL 预览）。
- 高级页：所有者 / 表空间 / 填充因子三只 372×24 下拉 + 「配备数据」勾选
  （PG `WITH DATA`），标签列 250 宽、行距 36；注释页：注释输入框。
- 编辑器：新建实体化视图不再塞占位串（截图正文为空，只有行号 1）。
- SQL 预览：`design_view_sql(kind_label, keyword, name, body)` 拆成中文叙述词与
  SQL 关键字，实体化视图出 `CREATE MATERIALIZED VIEW`（原先误拼成 `CREATE 视图`）。
- 验证：`designer_tests::new_matview_designer_from_action_bar`（动作条开页签 →
  四枚子标签 → 高级页三行 + 配备数据 → 注释行 → SQL 预览出 MATERIALIZED VIEW）；
  `designer_tests::` 全组 64 passed / 0 failed / 2 ignored。
- 截图取证：真库 `datawalk 开发库 / daowei_dev / public` 按截图路径实际点开，
  页签图标、工具条、子标签、空编辑器与参考界面截图逐项对照。

## 本轮完成：右侧信息面板「使用 / 被使用」依赖页（仅 PostgreSQL）

**口径**（对着 Flutter `database_info.dart` / `pgsql_driver.dart` 移植）：右栏
页签条在 DDL 之后加两枚箭头图标——`ArrowUpRight` = 使用、`ArrowDownLeft` = 被使用，
只有 PG 的表对象出现（字段 / 其它引擎都不出）。方向只换 `pg_depend` 的两组列：
被使用取 `(classid,objid)` 为来源、`(refclassid,refobjid)` 指向本表；使用反过来。
本表对别的表的依赖不记在表上，所以「本表」一端还要收下 `pg_constraint` 里
`conrelid = 本表` 的约束 oid。来源对象经一张 `objs` 联合表翻译成
模式 / 名称 / 类型（表、约束、类型、函数、排序规则、转换、默认值、模式、角色），
`deptype` 映射 i→INTERNAL / a→AUTO / e,x→EXTENSION / 其余 NORMAL；系统模式
（`pg_catalog` / `information_schema` / `pg_toast`）与自依赖都滤掉。PG 给外键偷建
的 4 个 `RI_ConstraintTrigger_*` 不在对象级依赖里，按 `tgconstraint` 挂到父约束
下作为一级可折叠子行。

**界面**：外键等约束行默认展开，点击行首箭头折叠（按 `kind+schema+name` 记，
换表时清空）；行尾灰字 `类型 (性质)`；序列 / 索引画 `123` / `A-Z` 文字徽标，
其余按类型给线框图标（外键/唯一键蓝、主键/触发器黄褐、表/视图蓝、类型青）。
数据按表身份缓存两槽（`info_deps: [Option; 2]`，key 与 DDL 页同源），
Loading / Ready / Error 三态；演示连接或未打开只出说明、不发起反查，
选区离开表对象时页签退场并自动回到信息页。

**验证**：`info_panel_dependency_tabs_follow_engine_and_selection`（SQLite 真表不
出依赖页签 → 换演示 PG 连接后出现、点开只有说明且缓存为空 → 选区离开自动回信息页）；
真库（ignore）`pg_dependencies_both_directions`（独立测试库建父表 / 子表外键 /
枚举 / 索引 / identity：被使用侧见到外键 + 4 个内部触发器、索引、identity 序列；
使用侧见到父表与用户枚举，系统目录对象不出现；反方向不串）。
`cargo test --bin daro-rs -- --test-threads=1`：247 passed / 0 failed / 16 ignored。

**坑**：`SELECT DISTINCT` 配 `ORDER BY o.sch` 在 PG 上直接 42P10（排序表达式不在
选择列），照选择列里的 `COALESCE(o.sch, '')` 排序；SQLite 夹具的驱动注册表按
连接名建键，并行跑会互踩，UI 用例改用独占连接名（仓库口径本来就是串行跑）。

## 本轮完成：视图设计器「高级」子页（PostgreSQL）

**口径**：截图（PG 视图 `menu @daowei_dev.public`）的工具条没有多任何钮（
`保存 ┃ 预览 解释 ┃ 视图创建工具 美化 SQL`），子标签是 定义 / 规则 / 高级 / 注释 /
SQL 预览；高级页三行——`所有者:` 可编辑下拉（框里 postgres，焦点 + 全选）、
`检查选项:` 空下拉、`安全屏障` 不勾选 + 文字。这一族的量按物理像素 ÷ 2 取：
标签列 191、字段 241×20、行距 26、左边距 10、顶距 7（缩放系数用两处锚点交叉
确认：页签条带 40px ↔ `D_TABBAR_H` 21、Windows 复选框 26px ↔ 13 逻辑）。

**做法**（[src/main.rs](src/main.rs)）：

- 子标签 `view_subs_pg = 定义 / 规则 / 高级 / SQL 预览`（27555，仍缺「注释」，
  见下），`cat == 1 && sub == 2` 落到新的 `view_advanced_tab`（16115）；非 PG
  视图继续只有 定义 / SQL 预览 两枚。
- 所有者不新增状态：复用表页那套编辑器 `Input::new(&ds.opt_edits[0])`，右侧绝对
  定位一枚 9px `ChevronDown` 画成「可编辑下拉」；PG 连接的 `seed_options` 本来
  就播种 postgres。
- 检查选项 = ghost 按钮 + `dropdown_menu(pick_menu(（无）/ LOCAL / CASCADED))`，
  安全屏障 = `Checkbox` + 文字；两者写进新字段 `view_check_option` /
  `view_security_barrier`（各带 baseline），setter 在 10229 / 10249，沿用表页
  `design_set_*` 的写法（只清 `ds.note` + `cx.notify()`）。
- dirty：`cat == 1` 的判定在 PG 时加上 属主 / 检查选项 / 安全屏障 三项与 baseline
  的比较，改一动即点亮「保存」。
- DDL：新 `design_view_sql_pg`（3640）把安全屏障挂在视图名后
  （`CREATE VIEW 名 WITH (security_barrier=true) AS`）、检查选项挂在查询之后
  （`WITH LOCAL / CASCADED CHECK OPTION`）；SQL 预览的 cat 1 分支在 PG 且正文非空
  时补一条 `ALTER VIEW … OWNER TO "…"`，再逐条追加规则页的 `CREATE RULE … TO …`。
  非 PG 视图仍走老的 `design_view_sql`。

**没做的四处（有意留白）**：截图里视图子标签还有「注释」，本轮仍没补（与并发
会话正在改的属性区重叠）；表页 `CREATE RULE` 至今不带 PG 必需的 `TO`；打开已有
视图时属主 / 检查选项 / 安全屏障不从真库回填（只在建新视图时给默认值，表页属性
区本来也是这个口径）；本文件早前的几组量（`D_RULE_WIDTHS`、`D_RL_PROP_*`、
实体化视图高级页）是按 ÷1.75 除的，与这一族的 ÷2 差约 14%，只记录没顺手改。

**验证**：`explain_tests::view_advanced_subtab_rows_and_sql`（PG 夹具：高级子标签
在 → 三行 id + 三个 aria_label 齐 → 规则页的格子与钮不出 → 起始态 属主 postgres、
编辑器在、检查选项空、屏障不勾 → 两个 setter 写回 → 填正文切 SQL 预览，高级三行
退场）与 `view_advanced_subtab_absent_for_other_engines`（MySQL 视图无 `dsub/2`、
无 `vadv/owner`）；单元 `design_view_sql_pg_places_barrier_and_check_option`
（35415）钉住屏障在 `AS` 之前、检查选项在查询之后、大小写不敏感、空正文只出注释。
规则页那条用例同步改成四枚子标签（点 SQL 预览用 `dsub/3`）。
变异抽查：把 `{with}` 挪到 `AS` 之后打红单元用例；把 `vadv/barrier` 的 id 改名
打红 UI 用例的「缺行」断言。
`cargo test --bin daro-rs -- --test-threads=1`：247 passed / 0 failed / 16 ignored。

**环境坑**：链接器 `LNK1104`（打不开 `target\debug\deps\daro_rs-….exe`）是同仓
并发会话正在链同一份产物，`cargo check --bin daro-rs --tests` 绿之后重跑即过；
gpui 无头侧没有取文本的 API，渲染出来的 SQL 串改由单元用例钉住，UI 用例只断言
子标签与行的在场 / 退场（沿用本仓 SQL 预览的既有口径）。

---

## 本轮完成：实体化视图设计器两页照截图对齐

按用户给的两张参考界面截图复刻「新建实体化视图」的 **高级** 与
**SQL 预览** 两页，几何全部按截图物理像素量（截图与本机同为 2× DPI）。

- **高级页**：`所有者:` / `表空间` 用带 ▼ 的组合框；`填充因子` 是**无箭头**的
  普通边框输入（`plain` 闭包单拎一支，别让下拉箭头误入）；`☑ 配备数据`
  去掉左侧 14px 缩进，与标签列齐平。
- **SQL 预览页**：
  - 子条「更改 | DDL」：活动页黑字 + 蓝粗下划线，非活动页灰字 + `#f4f4f4` 底、
    条底 `#f9f9f9`；点哪页切 `ds.sql_sub`。
  - 量（物理像素，截图 / 实现）：子页高 66 / 66（=33 逻辑）、条底再留 12 / 12
    （=6 逻辑）的灰底再接正文；下划线 64×4 / 64×4；文字底→下划线 8 / 8；
    下划线底→子页底 13 / 12；下划线与 DDL 格间距 17 / 17；DDL 格宽 94 / 98
    （≈2 逻辑像素差，不再追）；非活动字色 `#6d6d6d` / `#6d6d6d`。
  - 下划线色照参考界面的 `#00a2e8`（浅色主题；深色仍用主题 accent）。
  - 正文贴边：外层编辑器内边距从 `p(12)` 收到 `pt(0).px(1)`，首行墨迹距条底
    24 / 截图 21、距左边 1 / 3；正文仍是高亮的
    `CREATE MATERIALIZED VIEW "public"."Untitled"` / `AS;`（空正文，无 REFRESH）。
- 用例：`new_matview_designer_from_action_bar` 增加子条高度 = 33 的断言，并继续
  钉住未命名预览串；`design_view_sql_uses_body` 盯纯函数输出。
  两条均通过（`cargo test --bin daro-rs -- --test-threads=1`）。

**并发/环境**：另一会话正在跑同一 target，`cargo build` 收尾复制
`target\debug\daro-rs.exe` 时报 os error 5（文件被其运行实例占用）。未杀非本会话
进程，改为把新鲜产物 `target\debug\deps\daro_rs.exe` 拷成 `daro-rs-probe.exe`
启动探针实例（按 PID 只停自己起的那个）。几何值与配色在锁屏前用 PrintWindow(2)
截图逐行量过；最后一笔下划线改色只经编译 + 用例验证——用户随后锁屏（LogonUI
常驻），PrintWindow 全黑，无法再取实机图。两次启动的面板分隔宽度不同（树列宽
差 35 逻辑像素），像素量按各自内容左边缘对齐，不跨启动比绝对 x。

---

## 本轮完成：视图设计器「注释」子页照截图对齐（1:1 复刻第四页）

按用户给的参考界面截图（PG 视图 `menu @daowei_dev.public`，2036×1390，本仓口径
物理像素 ÷2 = 逻辑像素）复刻视图设计器第四枚子标签 **注释**。截图内容区只有一枚
铺满的多行正文：没有字段名、没有边框、没有底栏，光标离内容区左上角约 5~6 逻辑
像素，右侧 2001~2033 那条 16 逻辑像素宽的槽是正文自己的滚动条；工具条与前三页
一致（`保存 ┃ 预览 解释 ┃ 视图创建工具 美化 SQL`，注释页不加钮）。

**做法**
- 子标签表：`view_subs_pg`（27781）从四枚改成
  `定义 / 规则 / 高级 / 注释 / SQL 预览`，SQL 预览顺延成 `dsub/4`；非 PG 视图仍是
  `定义 / SQL 预览` 两枚。分发处新增 `3 if view_pg => self.view_comment_tab(…)`
  （28041）。
- 状态：`DesignerState` 加一枚 `view_comment_edit: Option<Entity<TextareaState>>`
  （4016）。值不另开一份，仍写回既有的 `options.comment` —— 表页、实体化视图注释
  页读的都是它，DDL、保存、dirty 全共用一条链。
- 编辑器：新 `ensure_view_comment_editor`（16539），照 `ensure_view_editor` 的写法
  懒建 + 按 `did` 定位页签订阅 `InputEvent::Change` 写回；两个建页签入口（已选对象
  7712、新建对象 8001）在 `k == 1` 时一并备好，打开截图那种空白视图时
  起始就是空正文。
- 渲染：新 `view_comment_tab`（16315）= `div().id(rid("dcomment")).size_full().bg(
  背景)` 套 `Textarea::new(…).appearance(false).bordered(false).size_full()
  .text_size(px(12.))`，即无边饰、铺满内容区。
- dirty：`cat == 1` 的判定（27820）加上 `options.comment != baseline_options.comment`。
  baseline 不用新增字段：`baseline_options` 整份克隆 `options`，注释天然跟着存。
- DDL：把原先内联在 SQL 预览 cat 1 分支里的三条尾注抽成 `design_view_pg_tail`
  （3668，调用处 29127），顺序固定为 注释页 `COMMENT ON VIEW 名 IS '…'` →
  高级页 `ALTER VIEW … OWNER TO` → 规则页 `CREATE RULE …`；语句间单换行，与既有
  `CREATE RULE` 追加口径一致。空白正文不出尾注。

**没做的四处（有意留白）**：打开已有视图时注释不从真库回填（与属主 / 检查选项 /
安全屏障同一口径，只在新建时给默认）；表页 `CREATE RULE` 至今不带 PG 必需的 `TO`；
注释正文不做 ` COMMENT ` 之外的多方言分支（MySQL / SQLite 视图本来就没有这枚子
标签）；本文件早前的几组量（`D_RULE_WIDTHS`、`D_RL_PROP_*`、实体化视图高级页）仍
是按 ÷1.75 除的，与这一族的 ÷2 差约 14%，只记录没顺手改。

**验证**：`explain_tests::view_comment_subtab_is_full_pane_textarea`（43029，PG 夹具
`视图注释测试`：注释在 `dsub/3`、SQL 预览顺延 `dsub/4`、`dsub/5` 不出 → 点进去
`dcomment` 在、`vrule-cell/0/0` 与 `vadv/owner` 退场 → 编辑器已备好且起始为空 →
真打字（`within("dcomment").click("input-state")` + `input`）另起一轮断言写回
`options.comment`）；单元 `design_view_pg_tail_orders_comment_owner_and_rules`
（35709）钉住注释 → 属主 → 规则三条的先后、空栏不出语句、空白规则行不算规则。
规则页与高级页两条老用例改成五枚子标签（点 SQL 预览用 `dsub/4`）。
变异抽查四笔，各打红该打的用例后逐笔还原：`dcomment` 改名 → 注释 UI 用例「注释
正文在」红；`design_view_pg_tail` 里删掉 `COMMENT ON VIEW` → 单元用例红；订阅回调
里把 `ds.options.comment = text` 换成 `let _ = text` → 注释写回断言红；
`view_subs_pg` 去掉「注释」→ 注释 / 高级 / 规则三条用例同时红。
`cargo test --bin daro-rs -- --test-threads=1`：249 passed / 0 failed / 16 ignored。

**环境坑**：gpui 的 `InputBaseState::set_value` 会临时关掉 `emit_events`，只
`cx.notify()`，**永远不发** `InputEvent::Change`（`gpui-base-0.8.1/src/input/base/
state.rs:1226`）；`replace_all` / `insert` / `replace` 走 `replace_text_in_range_silent`，
只压 `on_text_typed`，emit 仍在（同文件 4715）。所以凡是靠订阅写回模型的用例都不能
用 `set_value` 驱动，得走 `window.click` + `window.input` 的真输入路径。订阅回调排在
effect 队列里、冲刷发生在 update 边界，模型断言必须另起一轮 `app.update`（沿用本仓
「键入写回随 effect 冲刷落地」的既有写法）。链接器仍会因并发会话链同一产物报
`LNK1104`，`cargo check --bin daro-rs --tests` 绿之后重跑即过。桌面实机图本轮没取到：
本会话起的验证实例约 15 秒内自行退出（stderr 空、无 panic），而其它会话的实例正在
换 PID 连着真开发库，按规矩不去动别人的窗口，故 1:1 的几何只按截图量、未经桌面复核。
---

## 本轮完成：视图设计器「SQL 预览」两子页（更改 / DDL）照截图对齐

按用户给的两张参考界面截图复刻 SQL 预览子页（PG 视图 `menu @daowei_dev.public`，
一张停在「更改」、一张停在「DDL」；本族口径仍是物理像素 ÷2 = 逻辑像素）。

**截图口径**
- 「更改」页：`更改 | DDL` 子条 + 整片空白正文（页签标题带 `*`、定义确实改过，
 参考界面这页照样是空的）。
- 「DDL」页：同一条子条 + 白底高亮脚本 `CREATE VIEW "public"."menu" AS SELECT id,
  … FROM menus;`，**空一行**再出 `ALTER TABLE "public"."menu" OWNER TO "postgres";`
  —— 属主走的是 `ALTER TABLE`（表页那条才是两行式 `ALTER TABLE …\n  OWNER TO`），
  而且一行写完。
- 子条量（物理 / 逻辑）：下划线 64×4 / 32×2、字 12 逻辑、下划线左边离内容区
  22 / 11 逻辑、两下划线之间 36 / 18 逻辑、带底边线到首行墨迹 21 / 10.5。
  两张截图的条带高度分别是 74 / 82 物理（37 / 41 逻辑），现有实现取 39 居中，没动。

**做法**
- `designer_sql_preview`（29099）原来把两子页做成装饰：正文不分叉，`更改` 也铺同一
  份 DDL。现在按 `ds.sql_sub` 分叉——`DDL` 出高亮正文（`dsql-ddl`），`更改` 出空白
  白底（`dsql-changes`）。这条链是视图 / 实体化视图 / 函数共用的，三者的「更改」页
  一起从「假 DDL」改成留白。
- 子条几何：`strip` 加 `.px(px(3.))` + `.gap(px(2.))`（配合格子自身 `px(8.)`、内层
  `px(4.)` 正好落在截图的 11 / 18 逻辑），并给条本身挂 `dsql-strip` 的
  id + `test_support()`，让左边距能用 bounds 断言。
- DDL 尾注 `design_view_pg_tail`（3695）改成先攒语句再拼：注释 → 属主 → 规则，
  属主换成 `ALTER TABLE {qual} OWNER TO "{owner}";` 一行式，`stmts.join("\n\n")`
  配上前级 `CREATE …;\n` 自带的换行，正好每两条之间空一行。

**没做的四处（有意留白）**：视图 / 实体化视图 / 函数的「更改」页只做留白，没接
`design_change_sql` 那样的增量脚本流水线（表页有，视图页无参考图，不臆造）；打开
已有视图时属主 / 注释 / 检查选项仍不从真库回填；表页 `CREATE RULE` 仍缺 PG 必需的
`TO`；新建未命名对象的 DDL 出 `"Untitled"` 而不带模式（截图是已有对象的
`"public"."menu"` 形态），这条本轮只记录没改。

**验证**：`explain_tests::view_sql_preview_has_blank_changes_page_then_ddl_script`
（43138，PG 夹具 `视图SQL预览测试`：进 SQL 预览 → 两枚子页在、子条高 33、左边距 3、
标签间距 2 → 默认 `sql_sub == 0` 且 `dsql-changes` 在场、`dsql-ddl` 退场 → 点
`dsqlsub/1` 后互换 → 用 `dsql` 的 aria_label 钉住整串脚本，含 `ALTER TABLE … OWNER TO`
一行式与语句之间的空行）；单元 `design_view_pg_tail_orders_comment_owner_and_rules`
（35756）同步换成新串。变异抽查五笔，各打红该红的用例后逐笔还原：`ALTER TABLE`
改回 `ALTER VIEW` → 单元 + UI 两条红；`join("\n\n")` 改 `join("\n")` → 单元 + UI 两条红；
`if ds.sql_sub == 1` 改成恒真 → 「DDL 正文不出场」红；条 `px(3.)`→`px(0.)` → 左边距红；
`gap(2.)`→`gap(0.)` → 标签间距红。
`cargo test --bin daro-rs -- --test-threads=1`：250 passed / 0 failed / 16 ignored。

**环境坑**：`gpui_kit::Bounds` 的坐标是 `.origin.x`，不是 `.x`（写成 `.x` 报 E0609）。
带 `overflow_y_scrollbar` 的 div 会吃掉 `test_support` 的观察包装，所以 `dsql-ddl` /
`dsql-changes` 的 id 挂在外层、滚动 div 放里面（沿用表页 diff 两栏的既有写法）。
桌面实机图仍未取到：本会话起的验证实例约 15 秒自退，其它会话实例正连真开发库且
在换 PID，按规矩不去驱动别人的窗口，故本轮几何只按截图量。

---

## 本轮：SQL 预览实机取证 + 空行塌陷修正

**取证**：`cargo build --bin daro-rs` 后拉起 `target/debug/daro-rs.exe`（PID 18948，
MainWindowHandle 6425258），PrintWindow 抓到 2906×1813 物理像素整窗图
`.scratch/sql2-0.png`（口径仍是物理 ÷2 = 逻辑）。图里视图设计器停在 `SQL 预览`，
`更改 | DDL` 子条几何对得上：条底灰带实测正好 78 物理行 = `h(px(39.))`，活动页
黑字 + #00a2e8 下划线、非活动页灰字浅灰底；DDL 出
`CREATE VIEW "public"."menu" AS` / `SELECT … WITH LOCAL CHECK OPTION;` /
`ALTER TABLE "public"."menu" OWNER TO "postgres";`——关键字着色、一行式属主、
模式限定名三项全中。

**实机抓到的缺陷（本轮修）**：语句之间的空行在屏幕上不存在。逐行扫正文，三段墨迹
落在 465–486 / 506–524 / 543–564，间隔都是 18–19 物理像素（= 一行内部的行距，不是
空出一行；真空行该差 ~40）。原因：空行那格 `h_flex` 里一个 span 都没有，自然高度
塌成 0。修法：每行 `.h(px(20.))` 定高，与表页 SQL 预览 `diff_row` / `plain_row`
的 20px 同一口径（sql_preview.rs:449、470），并给每行挂 `dsql-row/{i}` 的
id + `test_support()`。

**验证**：`view_sql_preview_has_blank_changes_page_then_ddl_script` 补六行的行高 20
与相邻行 `origin.y` 差 20 两条断言。变异抽查一笔：去掉 `.h(px(20.))` 该用例即红
（`dsql-row/0 行高 left 19.5px right 20px`——顺带量出头文本行自然高 19.5，定高后
整页统一 20）。套件：`cargo test --bin daro-rs -- --test-threads=1`
262 passed / 0 failed / 17 ignored；并行跑会红 7–9 条，全在 SQLite / 真库一族
（`refresh_catalog_rereads_real_db` 单跑绿、与 `designer_loads_real_sqlite_structure`
同跑即红），是驱动句柄表按连接名全局共享的相互干扰，与本轮改动无关，本轮没动。

**留白更正**：上一轮写「打开已有视图时属主 / 注释不从真库回填」不准确——
`read_pg_design` 的目录查询里带 `pg_get_userbyid(relowner)` 和注释，`design_loaded`
把整份 `meta.options` 赋给 `ds.options`。但本轮查出更根本的一条：`load_real` 当时
只放表，视图压根没走反查，所以实机那句 `OWNER TO "postgres"` 其实是
`seed_options` 的种子；视图真正读回真库属主 / 注释是本轮放开 `load_real` 之后才成立的。

**未闭环**：定高改完没能再拍一张实机图——工作区锁屏（LogonUI 在跑、
`GetForegroundWindow()==0`、`OpenInputDesktop` 失败），新起的实例（PID 27184）
PrintWindow 只回黑帧，GPU 帧没提交。解锁后要补拍一张确认空行真出来了。

---

## 本轮：编辑视图要把库里的 SQL 显示出来（用户报的）

**根因**：`open_designer` 里 `load_real = k == 0 && is_real_conn(conn)` —— 真库反查
只放表。视图 / 实体化视图走的是演示档那条路：模型来自 `mock_design_cols` /
`seed_options` 的种子，正文 `view_body` 恒空，于是「定义」页是空编辑器、
SQL 预览只能出占位注释。上一轮实机图里那句 `OWNER TO "postgres"` 也是
`seed_options` 的种子，不是库里读的。

**做法**
- `DesignMeta` 加 `definition`（视图 / 实体化视图的 SELECT 正文，表留空），三族各自反查：
  PG `pg_get_viewdef(oid, true)`（`relkind in ('v','m')`）、MySQL
  `information_schema.views.VIEW_DEFINITION`、SQLite 取 `sqlite_master.sql` 再
  `strip_create_view` 剥掉 `CREATE 名 AS ` 头部（AS 要在引号外，视图名可带引号、
  正文里有 `x AS y` 别名）。取数统一走新的 `view_definition`，读不到返回空、并 trim
  掉 `pg_get_viewdef` 美化后的结尾换行。
- `load_real` 放开到 `k != 3`（表 / 视图 / 实体化视图都反查；函数仍是种子，见留白）。
- `design_loaded` 把 `meta.definition` 写进 `ds.view_body`，并补建视图类页签的
  「定义」编辑器与 PG 视图的「注释」编辑器——反查路径以前根本不建这两个编辑器，
  不补就是「编辑器尚未就绪」。
- 视图 / 实体化视图 / 函数的「已修改」原来是 `!view_body.trim().is_empty()`，
  回填后一打开就算改过。改成与基线比：新增 `DesignerState::baseline_view_body`，
  载入与两条保存出口（`design_save` / `finish_new_design_save`）都对齐。
- PG 的属主 / 注释那条目录查询原来过滤 `relkind in ('r','p')`，视图读不到；
  放开成 `('r','p','v','m')`。不改这条，视图一旦走真库反查，`options` 会被清成默认值、
  属主从种子 "postgres" 变成空串，DDL 里的 `ALTER TABLE … OWNER TO` 直接消失。

**验证**
- `designer_tests::designer_loads_real_sqlite_view_body`（新建，真 SQLite 夹具里有
  `CREATE VIEW menu AS SELECT id, name FROM menus WHERE id > 1`）：打开视图设计器 →
  `view_body` 等于剥头后的正文、`baseline_view_body` 与它相等（打开即无变更）、
  编辑器已就位且 `value()` 就是这段正文、字段是视图自己的 `id / name`、
  点 `dsub/1` 后 SQL 预览的 aria_label 以 `CREATE VIEW` 开头并含正文。
- 单元 `strip_create_view_keeps_body_and_ignores_inner_as`：普通形态、带引号视图名 +
  正文别名、名内转义双引号、非 CREATE 原样返回、空串。
- `cargo test --bin daro-rs -- --test-threads=1`：275 passed / 0 failed / 18 ignored。
- `pg_design_reads_view_definition`（`#[ignore]`，打 datawalk 独立测试库 `daro_pg_test`）
  已补跑通过：真库 `pg_get_viewdef` 回来的正文含 `SELECT` / `VIEW_BASE`、不带
  `CREATE VIEW` 头部、结尾无换行，字段取自视图本身，注释与属主也读到了。
  之前编不过的 `white is ambiguous`（并发会话在 `src/main.rs:61` 的 glob 导入撞名）
  与 `RoleBundle` 字段挪动都由那个会话自行落地，本轮没动它的代码。

**留白**：函数设计器仍不反查——PG 要 `pg_get_functiondef` 且得把
`CREATE OR REPLACE FUNCTION … AS $fn$ … $fn$ LANGUAGE plpgsql;` 拆成参数 / 返回类型 /
函数体三格，MySQL 要 `information_schema.ROUTINES`（只有正文没有参数签名，
与既有「函数删除 / 运行受驱动限制」同一坑），本轮没动。视图的规则页仍不读真库规则
（`pg_rewrite` / `ev_*`），检查选项与 security barrier 也不回填——
`pg_get_viewdef` 不给这两项，要另查 `pg_class.reloptions` 与定义文本尾部。

**环境坑**：并发会话共享 `target/`，我起的实例占着 `daro-rs.exe` 会让链接报
`拒绝访问 (os error 5)`，按 PID 停掉自己的实例才能重建；同一时段 `src/main.rs`
被对方连续改写，行号一直在漂，改完要用 `grep -c` 逐条核对自己的 hunk 还在不在。

---

## 本轮完成：PostgreSQL 角色设计器「常规 / 成员属于 / 成员」三页（1:1 截图）

**口径**：用户给的三张参考界面 PG 角色设计器截图（常规、成员属于、成员），
外加一条更正——同一套设计器族在 MySQL 那边叫「用户」、在 PG 这边叫「角色」，
本轮只做 PG，动作条入口是对象面板的「用户」分类。

**做法**
- 新建 `src/role_design.rs`（纯逻辑层，不碰 gpui）：`RoleModel` 十个属性位 +
  OID + 口令一次性字段；`role_meta_sql` 一条查询取 `pg_roles` + `pg_authid` +
  `pg_description`；`validate` / `alter_statements` / `create_statements` / `is_dirty`。
  目录里读不出口令，所以哈希永不回填，`ALTER ROLE` 只带用户这次新输的那条；
  「密码加密」下拉决定这条新口令用什么哈希存，它本身不改目录。
- 成员两页：`MemberRow{name, granted, admin}` + `MemberDir{MemberOf, Members}`，
  同一张 `pg_auth_members` 正着查反着查。`member_sql` 用「列全部角色 + 两条
  EXISTS」而不是 JOIN——PG16 起同一对角色可以按不同 grantor 有多条边，JOIN 会把
  一行摊成多行；并按截图口径排除当前角色自己（截图那 16 行 = 全集群减
  `pg_read_all_data`，PG 也不允许 `GRANT x TO x`）。`membership_statements` 先
  REVOKE 后 GRANT（加一条边可能要求先有别的边，反过来撤不牵连还没建的关系），
  只动管理选项时走 `REVOKE ADMIN OPTION FOR`。
- `pg_auth_members` 记「这一层能不能再授」的那一列，PG16 起叫 `admin_option`、
  之前叫 `admoption`；列名在 SQL 解析期就定死，写任何一个都会在另一台上报
  「列不存在」。所以打开设计器先用 `ADMIN_COL_PROBE` 问一次 `pg_attribute`，
  认不出的形态按 PG16 写法兜底（让报错说实话，而不是悄悄换一列去查）。
- 界面（`main.rs`）：子页签条按截图分隔线定宽 51/58/48/48/48/61.5（参考界面这几枚
  不是「文字 + 固定留白」，任何统一留白都对不上量出来的分隔线）；常规页十四行 +
  七个权限勾选 + 过期日期小窗（这里没有日历控件，「…」只给「手写
  `YYYY-MM-DD HH:MM:SS`」和「清成永不过期」两条实在的路）；成员两页共用
  `role_members_tab`，定宽四列 40.5/320.5/156/72、表头 29.5、行高 20.5，选中行铺
  `#0078D7` 文字转白，网格总宽 590 不铺满右侧（参考界面是定宽表格不是自适应列表）。
- 真连反查走后台线程：`request_role_load` → `read_role_meta` → `role_loaded`，
  一次拿 `RoleBundle`（属性 + 两页成员）。成员关系读不到就整次失败——宁可报错，
  也不给两页铺一排「没有」的勾。脏标 / 基线 / 保存三处都改成同时算属性差异与
  两页成员差异（`role_dirty` / `role_align_baselines`），保存一次把三页的账结掉。
- 演示连接不假装执行：保存后说明写「已保存（演示数据，不落库）」，SQL 预览尾部
  带一句「以上脚本不会执行」。`mock::PG_ROLES` 从 8 条扩到 17 条对齐截图那台集群。
- 「注释」与「SQL 预览」两页不出新布局，按用户口径与表 / 视图设计器同一套：
  注释页仍是铺满内容区的无边框多行正文（与 `view_comment_tab` 逐字同形，只换 id）；
  SQL 页改成「更改 | DDL」两子页——横条 39 / 子页 33 / 左 3 / 间隔 2 的几何直接取
  视图设计器那条对截图量出来的数，「更改」放差异脚本 + 右上角复制（无改动留白），
  「DDL」是 之前 / 之后 两栏整段重建的 diff。整段重建走新的 `ddl_script`
  （属性 + 注释 + 两页成员边，基线与工作集各出一份才有共同口径）。
  这段渲染与表 / 视图设计器是两份实现（结构一致、id 前缀不同）：设计器那条正被
  并发会话改，本轮不合并，去重留到它落地后。

**实测与截图不一致的一条（别当 bug 改）**：截图那台 `pg_read_all_data` 的
「继承权限」是勾，datawalk 这台目录给的是 NOINHERIT。演示档照截图，真连反查照
目录；`live_role_lookup_reads_predefined_role` 因此不把某台服务器的答案写死，改成
逐位对照 `pg_roles`——这样列序错位照样红（本项目真机踩过 `rolcaninherit`），
服务器差异不会误报。

**踩到的坑**：滚动容器把 `.id()` / `.test_support()` 和 `.overflow_scrollbar()`
写在同一个 div 上，`try_find` 取不到这个 id（成员页、SQL 预览页都中过）。要么外层
留 id + test_support、内层管滚动，只有一层。

**验证**
- `role_design` 一族 20 条单元：SQL 拼装、两方向 EXISTS 条数、`admoption` 老写法、
  探测列名兜底、GRANT/REVOKE 顺序与 ADMIN OPTION 成对、脏判定按名字不按顺序。
- `role_designer_tests` 5 条 headless UI：双击进常规页 14 行齐；取消勾选→脏标→
  演示保存如实说明；「成员属于」网格 16 行、不含自己、整行点击只选行、勾管理选项
  蕴含授予、取消授予连带清管理选项、改回原样就不脏、SQL 预览页可达、演示保存后
  两页基线一起对齐；「成员」页改的是 `members` 不污染 `member_of`，语句读作
  `GRANT 当前角色 TO 成员`；「SQL 预览」页两子页几何（横条 39 / 子页 33）、干净时
  更改页空且无复制按钮、改属性 + 勾成员后正文同时含 `ALTER ROLE` 与 `GRANT`、
  切 DDL 后 之前 / 之后 两栏互换出场。
- `live_role_lookup_reads_predefined_role`（`#[ignore]`，只读）已跑绿：属性逐位对照
  目录、两页成员非空且不含自己、`admin_option` 那列在这台上确实取得到。
- 套件：`cargo test --bin daro-rs -- --test-threads=1` 277 passed / 0 failed /
  18 ignored（本轮 +6 条新用例）。
- 布局数值来自对截图逐像素量取（成员网格分隔线 81/723/1035/1179、表头底 222、
  行距 41 物理像素 ÷2），headless 用例只断言元素存在与状态，没做像素比对。

**未闭环**
- 「权限」子页还没复刻：工具条要随子页换成 保存 / 添加权限 / 删除权限 / 权限筛选 /
  对象筛选，网格是 数据库|模式|名称 + Connect…Usage 十二列权限矩阵，反查要在选中的
  库上对 `pg_database.datacl` / `pg_namespace.nspacl` / `pg_class.relacl` /
  `pg_proc.proacl` 逐族 `aclexplode` 再透视；函数那条还得带
  `pg_get_function_identity_arguments` 才能拼出无歧义的 `GRANT EXECUTE ON FUNCTION`。
- 本轮没取到实机截图：工作区仍在锁屏（LogonUI），新实例 PrintWindow 只回黑帧。
  解锁后要补拍常规页与成员页各一张，核对勾选态与行高。
- MySQL 的「用户」设计器不在本轮范围（PG 与 MySQL 的属性位、成员语义不同源，
  要另出一套口径，不能套这十个位）。

## 本轮完成：设计态页签图标清点（表 / 视图 / 实体化视图 / 函数 / 角色）

- 资源清点：旧素材包（位图资源目录与 RT_RCDATA / RT_BITMAP）里，
  带铅笔的编辑态位图只有 `OV_Table_Design` / `OV_View_Design` /
  `OV_Materialized_View_Design` 三档；工具条动作铅笔是 `TB_Design`。对
  旧素材全量扫描按 `OV_*Design` 也只有这四个名字——函数、用户 / 角色、
  事件、存储过程都没有设计态位图，不自造。
- `tab_glyph`：表 / 视图 / 实体化视图设计页签各取一档 `*_Design`；函数设计页签
  保留 `OV_Function`；角色页签从功能区 `FB_User` 改为对象图标 `OV_User`
  （页签跟树节点一样走 OV_*，FB_* 只进功能区 / 信息面板）。
- 回归：`designer_tests::new_function_designer_from_action_bar`、
  `designer_tests::new_matview_designer_from_action_bar`、
  `role_designer_tests::double_click_role_row_opens_general_page` 各补页签图标与
  位图入包断言；表 / 视图两条旧断言同跑全绿。
- 未跑实时截图：另一会话的 `daro-rs.exe` 实例占着链接产物（LNK1104），本轮没有
  强杀别人的进程；等它退出后可补一张五种页签的截图核对。

## 本轮完成：顶栏菜单按「有无页签」分两套上下文（刚打开 / 开着查询页）

- 取证结论：之前几轮的对齐截图是参考界面**激活 SQL 查询窗口**时拍的，本轮补的
  三张是**一个页签都没开**那一态，两者条目不一样（截图里那态顶栏根本没有「格式」
  栏，「编辑」只剩复制/粘贴/全选，「查看」只剩窗格开关 + 视图模式那一组）。
  所以顶栏不能只有一套菜单。
- 实现：`enum BarCtx { Bare, Tab }`，`menu_bar()` 按 `opened.is_empty()` 定档，
  把 `ctx` 透给 `file_bar_menu` / `edit_bar_menu` / `view_bar_menu`；`Tab` 分支的
  条目与行序一律不动（旧用例的下标断言全部照绿），只在 `ix == 3` 槽位后按档追加
  `BarMenu::Static(格式)`——Bare 时整栏收起。
- Bare 文件：新建项目… / 新建连接… / 新建 ▸ / URI… / 打开文件 ▸ / 打开最近 ▸ ─
  导入连接… / 导出连接… ─ 管理云… ─ 关闭窗口 / 关闭选项卡（置灰）/ 退出；保存·
  另存为…·另存为外部文件…·导出结果… 与「创建 BI 工作区…」一起收回（都是编辑器级）。
- Bare 编辑：`menu.label()` 三条（复制 / 粘贴 / 全选）——没有编辑器，撤消·剪切·
  查找·替换·代码补全没有落点，留真入口就是假入口。
- Bare 查看：导航窗格 ▸ / 信息窗格 ▸ / •列表 / 详细信息(Ent) / ER 图表 / 隐藏对象组
  / 排序 ▸ / 选择列 / 显示隐藏的项目 / 显示表配置文件。导航窗格 / 信息窗格两层子菜单
  这次抽成 `nav_pane_submenu` / `info_pane_submenu`，两套上下文共用同一份实现与同一份
  持久化状态（`show_tree` / `show_info` / `info_page`），不做第二套开关。
- 口径偏差：①「使用 URI 打开…」截图可点，daro 没实现 URI 解析，保持死标签；
  ②bare 查看没有「刷新」这行（按截图），但 `F5` 键位与树 / 面板右键的刷新没动；
  ③「排序 ▸」正文没拍到，留占位；④「关闭选项卡」在 Tab 档走 `CloseTab` 动作，
  与 `Ctrl+W` 同一落点。
- 验证：`bar_menus_match_reference_without_any_tab`（「格式」不在栏上 + 文件 12 行 +
  编辑器级条目/BI 角标不在 + 点第 7 行「导入连接…」真开一个窗 + 编辑只剩三条 +
  查看 10 行且无查询级条目 + 点「列表」翻 `list_view`）、
  `bar_menus_follow_the_tab_count`（开查询页→「格式」和保存那组回来→点第 21 行
  「关闭选项卡」真关页签→退回刚打开那一套）。整仓
  `cargo test --bin daro-rs -- --test-threads=1` 310 passed / 18 ignored 全绿。
- 踩坑记录：`CloseTab` 处理器是 `cx.on_action` + `cx.defer`，defer 要到下一个
  `cx.update_window` 边界才落，在同一个闭包里点完就断言 `opened.len()` 必然读到旧值。

## 本轮完成：对象页「编辑」菜单的 全选 / 复制 接线（2026-11-08）

- 用户口径（参考界面「编辑(E)」截图：复制 `Ctrl+C` 可用、粘贴 `Ctrl+V` 灰、全选
  `Ctrl+A` 可用）：切到「对象」页时「全选」要真能用；选中了对象时「复制」要可用。
  此前 `BarCtx::Bare` 那三条是 `menu.label()` 死标签，而且开着别的页签再切回对象页
  时，这栏仍整栏显示查询窗口那一套。
- 上下文改判：编辑菜单不再只看「有没有页签」——`edit_bar_menu()` 用
  `me.read(cx).active_tab == 0` 判「激活的是 0 号对象页」，`ctx == Bare ||
  objects_active` 转 `objects_edit_bar_menu()`；`BarCtx` 枚举与文件 / 查看两栏的
  Bare/Tab 分档都没动，查询页激活时仍是原来那一整套（旧用例全绿）。
- 三条菜单用 `mi_with_id` 自绘行 + 固定 id（headless 能精确点到某一项，不用数
  分隔线下标），顺序照截图：复制 / 粘贴 ─ 全选。
  - 复制：挂 `GridCopy` 动作显示 `Ctrl+C`，`has_sel` 由 `selected_object_acts()` +
    `object_ref()` 判定；点击 `copy_selected_objects()` 把名字写进剪贴板，多选按
    框选顺序一行一个（与对象右键「复制」同为「复制名字」口径，对象级粘贴仍未做）。
  - 粘贴：恒灰，只挂 `GridPaste` 显示 `Ctrl+V`。
  - 全选：`has_objects` 由 `pane_objects_visible()` 判定；点击 `select_all_objects()`
    取可见对象的原始下标，复用 `apply_object_box_selection()`（与橡皮筋框选同一
    落点，搜索过滤后的清单也照此全选）。
- 快捷键不放假提示：`wire_global_keys` 的 `GridSelectAll` / `GridCopy` 处理器改派
  `edit_select_all()` / `edit_copy()`，`active_tab == 0` 走对象那套，其它页原样走
  `grid_select_all()` / `grid_copy()`；输入框焦点仍由 gpui 的输入动作先截获。
- 口径变化：上一轮台账写「Bare 编辑三条留灰，等有真落点再接线」，本轮按用户要求
  接线，FEATURES.md A 节那行同步改掉「三条死标签」的说法。仍留白：对象级粘贴，
  以及文件 / 查看两栏切回对象页时仍按「有无页签」分档（本轮只要求编辑栏）。
- 验证：新增 `objects_tab_edit_menu_select_all_and_copy`（开查询页→切回对象页仍是
  三条、全选全量进多选、多选 / 单选复制写剪贴板、`edit_select_all` 分派）、
  `objects_tab_edit_menu_disabled_without_targets`（空面板 + 无选中对象时两项点了
  不生效）。`cargo check --bin daro-rs` 通过；全量
  `cargo test --bin daro-rs -- --test-threads=1` 314 passed / 0 failed / 18 ignored。
- 环境坑：另一路会话并发编辑 `main.rs`、并发 cargo 编译同一 `target/debug`，
  `cargo test` 跑到角色设计器用例时进程异常退出（`0xffffffff`）；把
  `target/debug/deps/daro_rs-*.exe` 复制到 `.scratch/` 再跑反而 149 failed（相对
  资源路径找不到），**直接在 deps 目录跑原 exe 稳定全绿**。

## 本轮完成：分类按引擎可见（2026-11-08）

- 用户口径（指着 SQLite 库下面那排分类问「sqlite 库下面没有实体化视图对象吧」）：
  SQLite 没有 `CREATE MATERIALIZED VIEW`，也没有存储函数，那两行是永远点不开的假节点。
- 根因：驱动层早就按引擎裁剪（`catalog::query_objects` 里 SQLite 只回 cat 0/1、
  MySQL 只回 0/1/3），但界面把 `CATEGORIES` 五格无条件铺满，树 / 状态栏 / 功能区都不看引擎。
- 新增 `catalog::has_category(kind, cat)`：cat 2 只属 PG，cat 3 除 SQLite 都有；
  其余分类恒可见。分类下标不动，`objects_key` 那套缓存键不受影响。
- 四处按这道门收放：`cat_levels`（普通态树）、`filtered_cats`（搜索态，直接返回
  「不显示」）、`any_match`（分类名命中也要看可见性，否则搜索态会为一类不存在的
  对象亮出「有命中」）、`sel_child_count`（库 / 模式节点的「N 个类别」按可见数计）。
- 功能区（Ribbon）按用户选择整格隐藏：`ribbon_tile_shown` 只看当前选中连接的引擎，
  没选中连接时 13 格照旧全摆。`ribbon_tile_left` 加了 `shown` 参数，「其它」下拉的
  对齐跟着可见格子累加，隐藏两格后不会错位 132px。
- 显示兜底：`display_cat()` 在选中的连接没有这一类时退回表分类，动作条不会再写着
  「新建实体化视图」而面板空着。`select_category` 没有加守卫——入口格子已经不在了。
- 口径变化：DETAILS.md 基线里「功能区 13 项」与「左树 表/视图/实体化视图/函数/查询」
  两条都是按 PG 样本量的，本轮起非 PG 引擎的格子数会少；「需确认是否 PG 专属入口」
  那条按「是」处理了。
- 验证：新增 `category_visibility_tests` 2 条——真 SQLite 夹具库（表 + 视图）树里只有
  cat 0/1/4、功能区那两格不在、`cat_count_for(SQLite)==3`；演示 PG 五格全在。
  两条旧用例（`new_matview_designer_from_action_bar`、`object_action_bar_follows_category`）
  原来拿 MySQL 演示连接测实体化视图，换成 PG 夹具，SQL 预览期望同步改成
  `"public"."Untitled"` 那一形。`cat_row_elem` 补 `.test_support()`，否则无头用例找不到分类行。
- 全量 `cargo test --bin daro-rs -- --test-threads=1` 316 passed / 0 failed / 18 ignored。

## 本轮完成：代码段面板 + SQL 代码折叠（2026-11-08）

- 用户口径两条：工具栏「代码段」点开要在右侧信息栏切到模板库，双击条目快速插入
  模板（参考图为参考界面代码段面板）；SQL 编辑窗口要能折叠代码段。
- 代码段库：新增 [src/snippet.rs](src/snippet.rs)，条目与说明照参考界面默认库
  11 条（CASE / COMMENTS / FOR / IF…ELSE / LOOP / WHILE / 三条 DML / 两条
 参考界面-only）。面板复用右栏（`info()` 早期返回 `snippet_panel`）：顶部 ⓘ / 代码段
  两个页签图标切回 / 展开，标签下拉 + 底部搜索实时过滤，条目行单击选中、双击插入。
 参考界面-only 两条（结果页签名、运行参数）插入的是写明「daro 不支持」的注释占位，
  不伪造语义；参考图里没有落点的网格图标与「+」按钮刻意没画。
- 插入与撤销：按当前行缩进铺模板后 `EditorState::insert`（Atomic 事务）并
  定位到模板末尾；最初用 `set_value` 会清空撤销栈导致 Ctrl+Z 无效，改 `insert` 后
  无头用例与实机 SendKeys `^z` 都能撤回。实机还修了一处：页签「代码段」再点一次
  回信息卡，隐藏信息窗格时自动退出代码段视图。
- 代码折叠：gpui 编辑器本来就带折叠骨架（行号槽 chevron、点击开合），缺的只是
  高亮器的候选区间。`sql_highlight::fold_ranges_sql` 整段扫描给出跨行候选：
  `BEGIN…END` / `CASE…END` / `IF…THEN` / `WHILE…DO` / `LOOP…END LOOP` /
  `REPEAT…END REPEAT`、跨行括号组、多行块注释；字符串（含 PG 美元引号）、行注释、
  `/* */` 内不认关键字，`BEGIN;`/`BEGIN TRANSACTION`（事务）、`IF(...)`/`IF EXISTS`、
  `REPEAT(str,n)` 函数不当块开始；同首行只留最宽区间（fold map 按 start_line 去重），
  未闭合块折到文末，方便写到一半先收起。
- 验证：`sql_highlight` 新增 6 条单测（嵌套块、CASE + 括号、LOOP/WHILE/REPEAT +
  多行注释、字符串与函数误判、未闭合、highlighter 适配），全量
  `cargo test --bin daro-rs -- --test-threads=1` 324 passed / 0 failed / 18 ignored。
  实机（Windows）：查询页键入 4 行 `BEGIN…END`，行号槽悬停出现 ⌄；
  点一下折成 `BEGIN ⌄` + `END;`（2、3 行收起），再点还原。

## 本轮完成：折叠外观对齐参考界面+ 代码段参数 Tab 跳转（2026-11-08）

- 用户口径（附参考界面截图）：折叠要像参考界面那样——行号槽里方形 `−` / `+`
  常显，折叠首行末尾钉一个方框「⋯」；代码段模板的参数要能用 `Tab` 快捷键快速跳。
- 折叠外观：gpui-base 的折叠图标只在悬停 / 当前行 / 已折叠时出现，且
  gpui-component 每帧在 `Input::render` 里重置 `fold_icon_renderer`，应用层改不了
  渲染器，所以改用与编辑器同原点的 `FoldOverlay` 浮层（`main.rs` 的 `FoldOverlay`）：
  - 行号槽按 `display_map` 的折叠候选逐行画 14×14 方形图标，展开 `−`、折叠 `+`；
    图标格子底下先用行号槽底色铺一块 20×line_height 的遮罩，盖掉引擎雪佛龙
    （否则圆角缝里会透出来）。
  - 浮层是纯装饰：不带 hitbox，点击穿透到引擎自己的折叠热区，开合逻辑不复制一份。
  - 坐标口径：`range_to_bounds` / `input_bounds` 报的是编辑器**内边距里**的坐标
    （gpui-component `Size::Medium`：上 8、代码编辑器左 6），而浮层铺在外框上，
    所以 y 要 +8、行号槽右缘 = 左差 + 6；图标左 = 槽右 − 20、遮罩左 = 槽右 − 24
    （与引擎 `FOLD_ICON_HITBOX_WIDTH=18` / `FOLD_ICON_WIDTH=14` 的算法对齐）。
    横向按 `scroll_offset().x` 扣掉滚动量，图标像引擎一样钉在槽里。
  - 折叠首行末尾「⋯」：取首行去掉行尾空白后的偏移，`range_to_bounds` 定位后加
    内边距偏移，方框 18×14。
  - 已知限制：引擎的折叠投影固定保留首末行，折叠 `IF…THEN` 后 `END IF;` 仍会
    单独显示，与参考界面的「整块收起」有差异；要改得 vendor gpui-base。
- 代码段参数：`snippet::render_template` 早就在剥 `{{}}` 时返回参数区间。本轮：
  - 插入后用 `RangeDecorationCollection`（Fill + Frame 双装饰）给参数铺淡绿底 +
    细框；集合挂在 `QueryState.ph_decos` 上，随编辑自动漂移。
  - `Tab` 跳转：gpui 的键位绑定（`IndentInline`）先于元素级 `on_key_down` 派发并
    吞掉事件，元素监听拦不到；改为每个查询页签注册
    `App::intercept_keystrokes`，只认无修饰键的 `tab` + 编辑器持有焦点，
    命中参数则 `stop_propagation()` 并选中下一个参数、把已跳过的区间从集合里删掉；
    没有下一个参数时清空集合、不消费事件，Tab 回落成编辑器缩进。
    拦截器包在 `Rc<Subscription>` 里（`QueryState` 要 Clone，`Subscription` 不实现
    Clone），页签关掉随状态释放。
  - 修了一个实机崩溃：`RangeDecorationCollection::set` 内部会再
    `update` 一次编辑器，放在 `editor.update(...)` 里就是重入更新 panic；插入路径
    改成先 `editor.update` 落正文 + 选区，再在外面 `set` 装饰。无头用例补了
    「第二次插入」回归（集合已存在时走 `set` 的分支）。
- 验证：全量 `cargo test --bin daro-rs -- --test-threads=1` 330 passed / 0 failed /
  18 ignored（新增「只显示在查询页签」用例之前的一轮）；实机（Windows）逐项核过：
  - 键入 `IF x = 1 THEN / SELECT 1; / END IF;`，方框 `−` 与第 1 行文字同高；
    点击后变 `+`、`IF x = 1 THEN ⋯` 且第 2 行收起；再点还原。
  - 工具条「代码段」→ 双击 CASE，`case_value` / `when_value` / 两处
    `statement_list` 四处淡绿；Tab 依次跳 `when_value` → `statement_list`，
    已跳过的标绿消失；再次双击插入不崩。


## 本轮完成：代码段库只在查询页签出现（2026-11-08）

- 用户口径：代码段只有激活的是查询窗口才显示。
- 入口只有一个：查询页工具条的「代码段」按钮，所以问题出在右栏是全局状态——
  在查询页签打开库后切到表 / 设计器 / 角色页签，右栏还挂着模板库。
- 修法：`DaroApp::snippet_visible = snip_open && active_query_id().is_some()`，
  `info()` 按它决定是否渲染 `snippet_panel`。`snip_open` 本身不清，库的开合状态
  保留，切回查询页签还能接着用同一个库；非查询页签右栏自然回到信息卡。
- 回归：新增无头用例 `ui_flows::snippet_panel_only_shows_for_query_tabs`——
  查询页签里开库 → 打开表页签后 `snip/row/0` 消失且 `snippet_visible()` 为假 →
  切回查询页签库重新出现。与既有的双击插入用例一起
  `cargo test --bin daro-rs snippet_panel` 2 passed；全量
  `cargo test --bin daro-rs -- --test-threads=1` 331 passed / 0 failed / 18 ignored。
- 实机（Windows）核到查询侧：新建查询页签点「代码段」右栏照常出现模板库。
  切到表页签之后的半边没继续点——前台被另一个进程（游戏）占住，按约定不抢焦点；
  这一段由上面的无头用例覆盖（真窗口渲染后查 `snip/row/0`，并读模型断言）。


## 本轮完成：选项设置窗（工具 → 选项...，2026-11-10）

- 用户口径：照 Zed 设置窗的版式（左分类 + 右侧分组行）做，但设置项换成 daro
  自己的规划，不照搬 Zed 的条目。
- 新增 `src/options_dialog.rs`：独立模态窗，内容区 760×540 + 24 自绘标题栏
  （`ZedIcon::Settings` + 「选项」），底栏「确定 / 取消」。左栏 190px 四页
  「常规 / 界面 / 查询 / 存储」，右栏「页标题 + 分组名 + 发丝线 + 设置行」，
  行是左标题 / 说明、右控件。
- 只摆真正落盘到 `connections.json` 的项，没实现的入口不摆：
  - 常规：界面主题（浅色 / 深色分段，与顶栏月亮 / 太阳同一开关）+ 当前版本；
  - 界面：显示导航窗格、显示信息窗格（`Switch`）、对象面板视图（图标 / 列表）；
  - 查询：执行后默认显示（消息 / 摘要 / 结果·解释，落 `run_pane`）；
  - 存储：数据目录只读路径 + 「打开目录」（`open::that`）。
- 草稿态：开窗时 `DaroApp::app_options()` 拍一份快照，「确定」走
  `apply_app_options` 整份写回并 `save()`，「取消」/ 关窗 / Esc 直接
  `remove_window()`，不改主窗状态也不写盘。主题在草稿里只切分段高亮，确定后
  才 `Theme::change` + `apply_palette`，避免半途把主窗与本窗一起闪一下。
- 入口：`工具 ▸ 选项...`、标题栏右上角应用菜单「设置...」、`Ctrl+,` 三处共用
  `OpenOptions` 动作（菜单行角标由键位系统补）。无头用例里动作项要点得动，
  必须先 `wire_global_keys` 装配监听，两个选项窗用例与 `app_menu` 用例都已补。
- 验证：
  - headless 新增 `options_dialog_applies_and_persists`（开窗 → 界面页关导航窗格
    → 查询页改默认页 → 确定 → 断言状态与存档 `show_tree=false` / `run_pane=0`）、
    `options_dialog_cancel_discards`（改开关后取消，状态不变）；连同
    `app_menu_wires_settings_and_panel_layout` 一起通过。
  - 全量 `cargo test --bin daro-rs -- --test-threads=1` **354 passed / 0 failed /
    18 ignored**。
  - 实机（Windows，release 构建 + 隔离 `DARO_DATA_DIR`，PrintWindow 截图）：四页
    版式核对通过；常规页切「深色」→ 确定后主窗转深色、`connections.json` 落
    `"dark": true`。
- 偏差：Flutter 版 `options_dialog.dart` 只规划了界面语言，Rust 端 i18n 未做，
  故不摆语言项；主题自定义（`theme_customize_dialog.dart`）未做。


## 本轮完成：对象级复制 / 粘贴表（Issue #12，2026-11-12）

- 用户口径：照参考界面的交互——在 A 库选中表 Ctrl+C，切到 B 库 Ctrl+V，
  确认后把表（结构 / 结构和数据）复制过去；库内右键「复制表」同样可用。
  之前对象页 Ctrl+C 只把表名写进系统剪贴板，Ctrl+V 恒灰。
- 新增 `src/table_copy.rs`：复制引擎。同一连接内优先走服务端原生语句，
  不把行搬进内存：
  - MySQL：`CREATE TABLE dst LIKE src` + `INSERT INTO dst SELECT * FROM src`；
  - PostgreSQL：同库跨模式 `CREATE TABLE dst (LIKE src INCLUDING ALL)`；
    跨库 / 跨服务器由客户端搬（服务端没有跨库通道）：从 `pg_catalog` 读列
    （类型 / 默认值 / NOT NULL / 排序规则 / 标识列 / 生成列）、主键 / 唯一 /
    检查 / 排他 / 外键、额外索引与表 / 列注释，在目标库重建结构后经
    `COPY … TO STDOUT` → `COPY … FROM STDIN` 文本流搬行。SERIAL 转标识列，
    搬完把序列推到「最大值 + 1」；自引用外键改写到新表名，被引用表不在
    目标库时跳过该外键；目标库上的 DDL 与数据在同一个事务里，失败整表回滚；
  - SQL Server：`SELECT [TOP 0] * INTO dst FROM src`（仅结构时带 `TOP 0`）；
  - SQLite：从 `sqlite_master` 读原始建表语句改写表名执行（默认值 /
    AUTOINCREMENT / WITHOUT ROWID 原样保留），索引按目标表名前缀重新起名；
    跨文件时把源文件 `ATTACH` 到目标连接（只支持源 `main` 库），完成后
    `DETACH`；
  - 跨引擎一律报「暂不支持在 X 与 Y 之间粘贴表」。
  - 失败回滚只删「本次建出来的目标表」：同名表本来就在时不能连锅端。
  - 撞名规划 `plan_names`：`_copy` / `_copy_2`…，大小写不敏感，按方言截断
    （MySQL 64 / PG 63 / SQL Server 128）。
- 新增 `src/table_paste_dialog.rs`：「粘贴表」确认框（独立小窗），逐张列出
  源表 → 目标表，「结构和数据 / 仅结构」单选（默认结构和数据），Enter 粘贴 /
  Esc 取消。
- 接线：
  - 对象页 `Ctrl+C`（`edit_copy`）先把真库表记进 `table_clip` 并提示
    「已复制 N 张表，Ctrl+V 粘贴为副本」；视图 / 演示库 / Mongo / Redis
    退回原来的「复制名字」。
  - 对象页 `Ctrl+V`（新增 `edit_paste`）先校验目标连接已连、库存在、引擎
    一致，再弹粘贴框；`GridPaste` 全局动作与顶栏「编辑 ▸ 粘贴」共用这条落点。
  - 表分类右键、对象面板空白、连接树空白、顶栏「编辑」四处的「粘贴」按
    `table_clip` 真状态定灰态；对象右键「复制表 ▸ 结构和数据 / 仅结构」在
    当前库另起一张，先弹新表名（预填 `_copy`）。
  - 批次在 `Render` 帧尾由 `pump_table_copies` 投后台（内存库同步跑），回执
    失效目标库目录并立刻重查；成功只改状态栏，失败合成一张清单弹一次。
- 验证：
  - `table_copy` 真库单测 11 条：命名规划 / DDL 改写 / 引号与转义、同文件
    复制（数据 + 索引 + AUTOINCREMENT）、跨文件 ATTACH、撞名失败不误删、
    `CREATE TABLE … AS SELECT` 不把数据抄两遍；
  - headless `object_copy_paste_creates_a_table_copy`：真 SQLite 建表 → 对象页
    Ctrl+C 断言剪贴板 → Ctrl+V 弹窗 → 点「粘贴」→ 目录出现 `member_copy`
    且行数 2；
  - PG 纯函数单测：建表语句组装（标识列 / SERIAL / 生成列 / 默认值 / 排序
    规则 / NOT NULL）、外键 `REFERENCES` 改写（引号模式名与尾部动作）、索引
    命名去重与 63 字符截断；另有 ignore 真库用例
    `pg_cross_database_copy_rebuilds_structure_data_and_constraints`
    （打 datawalk 开发库）：把 member / gen / copy_probe / copy_tree 复制到
    临时库，断言行数、生成列重算、标识列续号、默认值、唯一约束与额外索引、
    表 / 列注释、自引用外键可用；
  - 全量 `cargo test --bin daro-rs -- --test-threads=1` **396 passed /
    0 failed / 19 ignored**；PG 跨库 ignore 用例单独跑过（`--ignored`）并通过。
- 偏差：MySQL / SQL Server 跨连接仍没有服务端原生通路，按房子规矩明确报
  不支持，不悄悄退化成丢结构或丢数据的复制；PG 的跨库 / 跨服务器已补客户端
  搬运。PG 客户端路径只认普通表（分区表 / 外部表直接报错），默认值里引用
  「不属于本列的独立序列」暂不重建；`LIKE INCLUDING ALL` 与客户端路径都不
  带外键时另有取舍：同库捷径不下外键，客户端路径只在被引用表存在于目标库
  时重建外键。索引之外的约束随引擎而异：MySQL `LIKE` 不带外键，SQL Server
  `SELECT INTO` 只带列和数据。

