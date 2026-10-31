# daro-rs ← Flutter 版迁移台账

基准改为 **Flutter 版 daro（`lib/`）**：功能、交互、文案一律以它为准，
表格式的「Navicat 实测」记录（[DETAILS.md](DETAILS.md)）停止更新，只作历史参考。

状态：⬜ 未开始 / 🔶 部分 / ✅ 已迁移（有测试或实机截图）

---

## 总览（按 Flutter 目录清点）

### 数据层 `lib/data/`

| Flutter 模块 | Rust 现状 | 状态 |
|---|---|---|
| `drivers/*`（mysql / mariadb / pgsql / sqlserver / sqlite / access / mongo / redis / odbc） | 无驱动，全部 mock | ⬜ |
| `db_metadata.dart` / `table_view.dart` | 无 | ⬜ |
| `navicat_import.dart`（.ncx + .ano） | `.ncx` 解析 + 密码解密已迁移；`.ano`（Redis 的 base64 JSON）未做 | 🔶 |
| `navicat_export.dart` | ✅ 本轮迁移（生成 + 导出向导 + 保存对话框） | ✅ |
| `csv_codec.dart` / `db_export.dart` / `db_import.dart` | 结果导出五种文本格式已迁移（txt/csv/json/xml/html）；导入向导与 SQL 导出未做 | 🔶 |
| `sql_split.dart` | 已迁移（`split_sql` 口径，缺 DELIMITER/GO） | 🔶 |
| `sql_completions.dart` | ✅ 本轮迁移（词表 + 前缀匹配 + 弹层实机可用） | ✅ |
| `lib/theme/sql_highlight.dart` | ✅ 本轮迁移（词法 + 固定配色 + 编辑器高亮适配） | ✅ |
| `table_design.dart` / `view_rule_dialog` / `function_wizard` | 表设计器 10 个子页全部迁完（字段/索引/外键/唯一键/检查/排除/规则/触发器/选项/注释）+ SQL 预览；视图/实体化视图「定义」页已迁；ALTER 差异未做 | 🔶 |
| `schema_sync.dart` | 无 | ⬜ |
| `connection_store.dart` / `theme_store.dart` / `locale_store.dart` | `connections.json` 已有；主题随连接一起存；无语言存储 | 🔶 |
| `result_grid_layout.dart` / `column_clipboard.dart` / `cell_value_view.dart` | 无（网格是简化版） | ⬜ |
| `saved_query_store.dart` / `sql_file_run.dart` | 无 | ⬜ |

### 应用层 `lib/app/`

| Flutter 模块 | Rust 现状 | 状态 |
|---|---|---|
| `app_state.dart`（3142 行） | 主状态在 `main.rs`（I0k 行）里，语义只覆盖子集 | 🔶 |
| `connection_manager.dart` | mock 化的开关连接 | 🔶 |
| `mcp_service.dart` + `lib/mcp/*` | 无 | ⬜ |
| `kv_manager.dart` + `kv_browser_page` | 无 | ⬜ |
| `cli_console.dart` / `command_line_page` | 无 | ⬜ |
| `sub_window.dart` / `ime_suppressor.dart` | 对话框已是独立窗口；IME 未接 | 🔶 |

### 界面 `lib/widgets/` `lib/pages/`

| Flutter 模块 | Rust 现状 | 状态 |
|---|---|---|
| `database_tree.dart` | 有；搜索/分组/右键/改名；无拖拽、无键盘导航 | 🔶 |
| `object_panel.dart` | 有单列列表；Flutter 是多列流式 + 框选 | 🔶 |
| `table_data_page.dart` | mock 网格：分页/排序/单元格编辑/增删行 | 🔶 |
| `query_page.dart` | mock 运行；无高亮、无补全、无查找栏、无「运行选区」按钮态 | 🔶 |
| `table_designer` 系 | 字段页闭环；子页占位 | 🔶 |
| `view_designer` / `matview` / `function_wizard` | 无 | ⬜ |
| `data_export_wizard.dart` / `data_import_wizard.dart` | 无 | ⬜ |
| `navicat_import_dialog.dart` | ✅ 本轮迁移（见下） | ✅ |
| `navicat_export_dialog.dart` | ✅ 本轮迁移 | ✅ |
| `result_export_dialog.dart` | ✅ 结果导出对话框（数据页签右键「导出结果…」） | ✅ |
| `options_dialog.dart` / `theme_customize_dialog.dart` | 无 | ⬜ |
| `mcp_settings_dialog.dart` | 无 | ⬜ |
| `about_dialog.dart` / `sql_find_bar.dart` / `kv_*` | 无 | ⬜ |
| `l10n/*`（中/英/日） | 全中文硬编码 | ⬜ |

---

## 本轮完成：连接导入（`.ncx`）

- 新增 `src/navicat_import_dialog.rs`（820×560 独立窗口）：
  路径输入 + 「浏览…」（原生 `rfd` 文件对话框）→ 解析 → 勾选列表 → 导入。
- 口径照 Flutter `NavicatImportDialog`：
  - 重名默认不勾（勾了导入时按 ` (2)`、` (3)` 顺延改名，同 `AppState._uniqueNamed`）；
  - 不受支持的类型（SQLite / Oracle …）灰显、不给勾选框；
  - 分组列对「本地还没有的分组」标 `·新建`；
  - 密码状态分四档显示：空串（已解密）/ 无密码 / 密码未保存 / 密码无法解密。
- 主窗口新增 `import_connections()`：登记缺失分组、顺延重名、落盘 `connections.json`。
- 顺手修了 `NavicatNcx::parse` 不还原 XML 实体的问题（`a&amp;b` 现在解析成 `a&b`），
  并加了守护用例 `parse_unescapes_attribute_entities`。
- 验证：`cargo test --bins` 33 个用例全绿；实机连续导入两次截图确认
  （第一次 6 条入库 + 新建分组「导入演示」；第二次同名全部默认不勾）。

**遗留**：`.ano`（Another Redis Desktop Manager 的 base64 JSON）未支持；
大文件（379 条）目前一次性铺满列表，未用虚拟化；路径输入框未做「失焦即解析」。

---

## 本轮完成：连接导出（`.ncx`）

- 新增 `src/navicat_export_dialog.rs`（780×560 独立窗口）：勾选连接 →
  选路径（默认桌面 `connections.ncx`，可走原生保存对话框）→ 「导出密码」开关 → 写出。
- 口径照 Flutter `NavicatExportDialog`：默认全选可导出的连接；「全选 / 取消全选」
  语义固定；Redis 灰显并标「不支持 .ncx」；`SettingsSavePath` 按
  `<文档>\Navicat\<类型>\Servers\<连接名>` 生成；写出失败把原因留在对话框里。
- 主窗口新增 `export_snapshot()`；菜单项「文件 → 导出连接…」已接线。
- 新增往返用例 `export_round_trips_name_group_and_password`：导出 → 解析回来，
  名称里的 `&`、中文、分组私有属性、密码密文全部原样还原。
- 验证：实机导出 11 条到桌面并检查文件头（`<Connections Ver="1.5">`、中文连接名、
  `SettingsSavePath` 落 `<文档>\Navicat\MySQL\Servers\...`），确认后删除了测试文件；
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

## 下一步顺序

1. **导出向导（表数据 / 多表批量）** —— 带进度与取消；`db_export.dart` 的
   `ExportJob` / `BatchExportResult` 口径。
2. **视图 / 函数设计器**（现为简化版）。
3. **驱动层**（单独排期）：mysql → pgsql → sqlserver → sqlite → mongo/redis；
   落地前先做「连接测试」，再把 mock 数据源逐块换成真实查询。
4. **i18n**（中/英/日，照 ARB key 表迁移）。
5. **MCP 服务**（最后，依赖驱动层）。
6. **补 `.ano` 导入导出**（Redis 的 base64 JSON）与数据导入向导。

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

