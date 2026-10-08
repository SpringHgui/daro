# AGENTS.md

> 本文件为 AI 智能体（及人类开发者）提供项目导航与编码规范。
> 遵循本文档可确保代码风格一致、主题正确、组件归属清晰。

---

## 项目概览

**daro** 是一个用 Flutter 构建的 桌面数据库管理工具。
目标是高信息密度、桌面级交互体验，支持明 / 暗双主题。

| 维度 | 说明 |
|---|---|
| 框架 | Flutter (Dart SDK ≥3.0) |
| 状态管理 | `provider` + `ChangeNotifier` (AppState) |
| 主题 | 自建 `AppPalette` / `Tokens` 语义色板 + `base_ui_flutter` 的 `DesktopTokens` / `TokenScope` |
| UI 组件库 | `base_ui_flutter`（本地 path 依赖，git submodule） |
| 字体 | 中文回退 `Microsoft YaHei` / `PingFang SC`（见 `lib/theme/app_theme.dart` 的 `chineseFontFamilyFallback`） |

---

## 多语言(i18n)

支持 **简体中文 / English / 日本語**,机制为官方 `flutter gen-l10n` + ARB + 类型化 getter。

| 位置 | 职责 |
|---|---|
| `l10n.yaml` | codegen 配置(输入输出目录、`AppLocalizations` 类名、缺译报告) |
| `lib/l10n/app_zh.arb` | **模板文件**(中文即源文案),新增 key 先写这里 |
| `lib/l10n/app_en.arb` / `app_ja.arb` | 英/日译文;key 集合与占位符声明须与模板完全一致 |
| `lib/l10n/app_localizations*.dart` | codegen 产物,**已入库**(见下) |
| `lib/l10n/locale_config.dart` | 唯一配置点:支持语言表 / 兜底 / delegate / `context.l10n` 扩展 / 字体回退 / `splitTabTitle` |
| `AppState.languageCode` | 语言状态:`null` = 跟随系统,非空 = 用户手选并**永久覆盖**系统检测(与 `themeMode` 同构) |

取用方式:

```dart
final l = context.l10n;                 // 方法开头取一次,后续用 l.xxx
Text(context.l10n.someKey);             // 只用一次时内联
MenuItem(text: l.actionOpen(label));    // 带占位符
```

### 改文案的步骤(顺序不能错)

1. `app_zh.arb` 加 key(含 `@key` 的 `placeholders` 声明)→ 再补 `app_en.arb` / `app_ja.arb`
2. **`flutter gen-l10n`**
3. 代码里引用 getter
4. `flutter test test/l10n_parity_test.dart`

> **生成的 `app_localizations*.dart` 必须提交**:`flutter analyze` 与 `flutter test`
> 都**不会**触发 codegen,不入库别人拉下来直接编译失败。
> 占位符一律声明为 `String`(数字传 `'${count}'`);生成的方法参数按占位符名
> **字母序**排列,与 ARB 正文里的语序无关。
> `test/l10n_parity_test.dart` 守护三件事:key 集合一致、占位符声明与正文 `{x}` 对应、
> 每个 key 都已生成成员(即漏跑 codegen 会红)。

### 不进翻译范围

数据库返回的原始错误信息、连接名 / 库名等用户数据、MCP 工具描述与错误码文本。
语言自称(简体中文 / English / 日本語)固定不译。

### 已知坑

- **浮层里的文案**:`Popover.content` 的 builder 可能挂在 Localizations 作用域之外
  (同 `TokenScope.maybeOf` 的兜底原因)。在外层 `final l = context.l10n;` 捕获后闭包使用,
  不要在弹层内部现取。参考 `database_tree.dart` 的 `_buildBottomBar`。
- **多占位符按声明顺序传参**:`gen-l10n` 生成的是**位置参数**,顺序 = ARB 里
  `placeholders` 的**声明顺序**(不是字母序)。参数全是 `String`,写反了编译期不报错,
  只在界面上渲染成「约万1.2」。约定:**按中文正文里出现的先后声明**,调用方即可按语序传参;
  迁移老 key 时若声明顺序与语序不一致,要么整批改(ARB + 所有调用点),要么不动,别只改一侧。
  该不变量由 `test/l10n_parity_test.dart` 守护(三语声明顺序一致 + 生成签名与声明一致)。
- **本地化字符串不能当 ID**:固定标签页用哨兵 `AppState.objectsTabKey == '@@objects'`;
  标签后缀 `kTabDesignTitleSuffix = ' (设计)'` / `kTabNewTitleSuffix` 是语言无关的内部标记,
  渲染时经 `splitTabTitle` 拆解再翻译。
- **数字缩写**:万/亿 与 K/M 的进位基数按 `l.localeName` 分支,见 `object_panel.dart`
  的 `_formatRowEstimate`。
- **widget 测试必须钉语言**,否则"跟随系统"会让本机与 CI 结果不一致:
  直接 pump `MaterialApp` 的用例加 `locale: const Locale('zh')` +
  `kAppLocalizationsDelegates` / `kSupportedLocales`;
  pump 真实 `DbApp` 的用例改调 `test/pin_system_locale.dart` 的 `pinSystemChineseLocale(tester)`。

---

## MCP 服务(默认关闭)

daro 内置一个本地 Model Context Protocol (MCP) HTTP 服务器,允许 AI Agent 通过标准协议调用数据库工具。安全策略为**默认关闭、按需开启**。

### 核心组件

| 文件 | 职责 |
|---|---|
| `lib/app/mcp_service.dart` | 主服务:策略管理/HTTP主机启动停止/驱动池协调/活动调用计数 |
| `lib/mcp/mcp_policy.dart` | 授权策略:连接级授权列表 + 单连接规则模式/库范围/库级别模式 |
| `lib/mcp/mcp_pool.dart` | 专用驱动实例池:`Lease` borrow/release 生命周期 |
| `lib/mcp/mcp_errors.dart` | 错误码契约 |
| `lib/mcp/mcp_tools.dart` | 工具注册表 + 执行分发(按 mode 路由到 agent/sandbox/read_only) |
| `lib/mcp/mcp_protocol.dart` | JSON-RPC 2.0 信封构造 + 超时取消 |
| `lib/mcp/mcp_http_host.dart` | Streamable HTTP 端点,Bearer auth,POST 收请求 GET 探健康 |
| `lib/data/mcp_policy_store.dart` | `mcp_settings.json` 持久化,三种加载状态(ok/missing/corrupted) |
| `lib/widgets/mcp_settings_dialog.dart` | 设置对话框:开关/模式矩阵/token管理/端口配置/客户端配置 |
| `lib/widgets/status_bar.dart` → `_McpIndicator` | 状态栏指示器:禁用/未监听/运行中(#N)四种态,点击打开设置 |

### 三种模式矩阵

每条连接可设 `agent` / `sandbox` / `read_only`:

| 模式 | 权限 | 典型用途 |
|---|---|---|
| `read_only` | 只读查询 | 数据审计、报表生成 |
| `sandbox` | DDL 执行,无跨库操作 | schema 变更实验 |
| `agent` | 完全访问(含写操作) | AI 辅助运维 |

连接未出现在授权列表 = **拒绝所有工具调用**(返回 `CONNECTION_NOT_AUTHORIZED`)。

### API 形状

```dart
class McpService {
  bool authorizes(String connectionName);
  Future<void> renameConnection(String oldName, String newName); // 改名迁移策略+池驱动
  Future<bool> requestPassword(ConnectionInfo conn, {Duration timeout}); // W14 密码补录
  bool get isRunning;
  int get activeCalls;
  McpPolicy get policy;
  McpPolicyLoadStatus get policyStatus;
}
```

### UI 交互规则

- **默认关闭**:安装后 MCP 未启用,不绑定端口,不占用系统资源
- **状态栏指示器**:点击弹出设置对话框;运行中带活动调用数显示
- **连接改名守卫(P6/D10)**:名称即标识,改名自动迁移 MCP 授权条目与池驱动实例(`McpService.renameConnection`)
- **密码补录(W14)**:连接缺少密码且需要认证时,通过 `askPassword` 回调或桌面多窗口插件弹出密码输入框,120 秒超时

### 安全原则

- 宿主在 loopback 接口监听(非回环需 token)
- 每次工具调用重读策略文件(不缓存),修改即时生效无需重启
- 禁止并发 bind/start,内部用 Completer 串行链保护

---

## 目录结构

```
daro/
├── lib/
│   ├── main.dart                 # 应用入口；TokenScope 包裹；主题切换
│   ├── app/
│   │   ├── app_state.dart        # AppState: 主题模式、标签管理、表选择
│   │   ├── mcp_service.dart      # MCP 主服务:策略/HTTP主机/驱动池/活动计数
│   │   └── connection_manager.dart # 连接生命周期管理(打开/断开/扩展)
│   ├── data/
│   │   ├── db_data.dart          # 静态模拟数据（连接 / 数据库 / 表列表）
│   │   └── mcp_policy_store.dart # MCP 策略文件持久化(mcp_settings.json)
│   ├── mcp/                      # MCP 子模块
│   │   ├── mcp_policy.dart       # 授权策略模型(连接规则/库范围/模式矩阵)
│   │   ├── mcp_pool.dart         # 专用驱动实例池(Lease borrow/release)
│   │   ├── mcp_errors.dart       # JSON-RPC 错误码契约
│   │   ├── mcp_tools.dart        # 工具注册表 + 执行分发(mode 路由)
│   │   ├── mcp_protocol.dart     # JSON-RPC 2.0 信封构造
│   │   ├── mcp_http_host.dart    # Streamable HTTP 端点 + Bearer auth
│   │   └── mcp_client_configs.dart # 客户端配置生成(免鉴权/带Token两种格式)
│   ├── l10n/                     # 多语言(见「多语言(i18n)」章节)
│   │   ├── app_zh.arb            # 模板 = 中文源文案
│   │   ├── app_en.arb            # 英文译文
│   │   ├── app_ja.arb            # 日文译文
│   │   ├── app_localizations*.dart # gen-l10n 产物，已入库
│   │   └── locale_config.dart    # 支持语言表 / delegate / context 扩展 / 字体回退
│   ├── pages/
│   │   └── main_page.dart     # 主页面布局：顶栏 + Ribbon + 三栏 + 状态栏
│   ├── theme/
│   │   └── app_theme.dart        # AppPalette 明暗色板 + Tokens.of() + toDesktopTokens() 桥接
│   └── widgets/                  # 应用级 widget（业务耦合，不可复用）
│       ├── top_menu.dart         # 顶部菜单栏(复用 base-ui-flutter MenuStrip) + 主题切换按钮
│       ├── ribbon.dart           # 工具栏
│       ├── database_tree.dart    # 左侧连接树
│       ├── object_panel.dart    # 中部对象面板（108 表网格）
│       ├── object_tabs.dart      # 对象路径标签
│       ├── view_tabs.dart        # 视图标签栏
│       ├── database_info.dart    # 右侧详情面板
│       ├── status_bar.dart       # 底部状态栏 + _McpIndicator(MCP 指示器)
│       ├── mcp_settings_dialog.dart # MCP 设置对话框(开关/模式/token/端口/客户端配置)
│       ├── table_icon.dart       # 表图标（应用级，不归入 base-ui-flutter）
│       ├── table_data_page.dart  # 表数据浏览页
│       └── query_page.dart       # SQL 查询编辑页
│
├── base-ui-flutter/             # 独立 UI 组件库（git submodule）
│   └── lib/
│       ├── base_ui_flutter.dart  # barrel export（单一入口）
│       └── src/
│           ├── foundation/       # DesktopTokens、TokenScope、Control 基类
│           ├── common/            # Button、Input、Label、CheckBox、ComboBox…
│           ├── lists/             # ListBox、TreeView、DataGridView…
│           ├── containers/         # GroupBox、TabControl、SplitContainer…
│           ├── menus/             # MenuStrip、ToolStrip、StatusStrip…
│           ├── overlay/           # Popover、MessageBox、Toast…
│           ├── dialogs/           # ColorDialog、DateTimePicker…
│           ├── data/              # Chart、Pagination…
│           ├── scroll/            # ScrollBar、TrackBar
│           └── misc/              # ProgressBar、Skeleton、Spinner…
│
├── pubspec.yaml                  # base_ui_flutter 通过 path 依赖引入
└── AGENTS.md                     # 本文件
```

---

## 双层主题系统

### 第一层：AppPalette / Tokens（应用语义色板）

定义在 `lib/theme/app_theme.dart`。这是应用的**主**主题系统，共 16 个通用语义色，
按抽象层级命名（底色 / 前景 / 交互 / 线条），**不绑定具体组件**；
业务定制色（图标 / 头像等）在 `AppColors` 里独立管理，不参与主题定制弹窗。

```dart
// 取色方式
final t = Tokens.of(context);
Container(color: t.surface);
Text('hello', style: TextStyle(color: t.foreground));
```

> 明亮主题是**纸白**配色：内容纯白，铬件（`surface` / `control`）与次级底（`secondary`）
> 只保留两级极浅灰，状态栏 `statusBar` 不得比 `secondary` 更深。
> 历史坑：0.5 及更早版本同一屏叠了 5 档互不相同的浅灰（`#F3F3F3` 铬件 /
> `#E7E7E7` 状态栏 / `#E0E0E0` 分割线），整窗观感发灰发脏。
> 需要加深某块区域时**先问是不是层级问题**，不要为单个组件单独加深一档。
> 约束由 `test/theme_palette_test.dart` 守护。

### 第二层：DesktopTokens / TokenScope（base_ui_flutter 令牌）

`base_ui_flutter` 组件库使用自己的 `DesktopTokens` 系统。通过 `AppPalette.toDesktopTokens()` 桥接方法，将应用色板映射到 `DesktopTokens` 字段，使 `base_ui_flutter` 组件自动跟随应用主题。

```dart
// 在 main.dart 中（TokenScope 挂在 MaterialApp 之上，覆盖 Dialog / Overlay）
return TokenScope(
  tokens: palette.toDesktopTokens(),   // palette = app.effectiveLight / effectiveDark
  child: MaterialApp(theme: buildAppTheme(Brightness.light, app.effectiveLight), ...),
);
```

### 桥接映射

| AppPalette 字段 | DesktopTokens 字段 | 用途 |
|---|---|---|
| `accent` | `primaryColor` / `ringColor` / `accentColor` | 选中 / 焦点 / 强调 |
| `background` | `backgroundColor` / `surfaceColor` / `cardColor` | 窗口与可编辑底（纯白） |
| `control` | `controlColor` | ribbon / 按钮面 |
| `surface` | `controlDisabledColor` | 禁用控件面 |
| `secondary` | `secondaryColor` | 标签条 / 内嵌带 |
| `popover` | `popoverColor` | 弹出菜单 |
| `foreground` | `foregroundColor` / `secondaryForegroundColor` / `cardForegroundColor` / `popoverForegroundColor` | 主文字 |
| `mutedForeground` | `mutedForegroundColor` | 次要文字 |
| `disabledForeground` | `disabledForegroundColor` | 禁用文字 |
| `accentForeground` | `accentForegroundColor` | 选中态文字 |
| `muted` | `mutedColor` | 柔和底 / 行号槽 |
| `border` | `borderColor` | 控件边框 |

> `controlHoverColor` / `controlPressedColor` 由 `control` 按 8% / 13% 黑叠加派生；
> `hoverOverlayColor` / `pressedOverlayColor` 按 `background` 亮度自适应（亮色加深、暗色提亮）。
>
> **新增 DesktopTokens 字段时**：在 `toDesktopTokens()` 中补充映射。

### 定制色板持久化与迁移

- 用户定制落盘在 `theme_custom.json`（`lib/data/theme_store.dart`）。
- **加载时会丢弃"等于任一版内置默认值"的色板**（`AppTheme.isBuiltInDefault`，
  依赖 `AppPalette` 的深比较）：老版本只要在主题弹窗里点过「应用」就会把当时的
  默认值写成"定制"，不识别出来就会把新版内置配色一直盖住。
- 因此**改内置默认颜色后，必须把旧值保留为 `AppTheme.lightLegacy` 之类的常量**，
  否则老用户的"伪定制"会继续生效（表现为改了 `AppTheme.light` 却看不到变化）。

---

## 组件归属规则

### ⚠️ 强制规则：禁止在 daro 中创建自定义 UI 组件

**任何 UI 组件需求，必须遵循以下流程：**

1. **优先检查 `base-ui-flutter`**：查看是否已有该组件（见底部组件速查表）
2. **存在 → 直接使用**：从 `base_ui_flutter` barrel 导入使用
3. **不存在 → 在 `base-ui-flutter` 中创建通用组件**：按下方步骤新增，然后在 daro 中引入
4. **绝对禁止**：在 `lib/widgets/` 或任何 daro 业务代码中自造 UI 组件（如 `_FormField`、`_CloseButton`、`_ApplyButton` 等）

> **违规示例**：主题定制弹窗中曾出现 `_ApplyButton` 自定义组件，应直接使用 base-ui 的 `Button`。

### ⚠️ 强制规则：禁止直接使用 Material 组件

**所有 UI 控件必须使用 `base_ui_flutter` 封装组件，禁止直接使用 Flutter Material 控件**（`ElevatedButton`、`TextField`、`Checkbox`、`Dialog`、`SnackBar`、`Tooltip`、`InkWell`、`Material` 等）。

| 场景 | ✅ 使用 base-ui 组件 | ❌ 禁止的 Material 组件 |
|---|---|---|
| 按钮 | `Button` | `ElevatedButton` / `TextButton` / `FilledButton` / `IconButton` |
| 输入 | `Input` | `TextField` / `TextFormField` |
| 勾选 | `CheckBox` | `Checkbox` / `Switch` / `CheckboxListTile` |
| 下拉 | `ComboBox` | `DropdownButton` / `DropdownMenu` |
| 弹窗 / 提示 | `MessageBox` / `Popover` / `Toast` | `Dialog` / `AlertDialog` / `SnackBar` |
| 分组 | `GroupBox` | `Card` / `ExpansionTile` |
| 标签页 | `TabControl` | `TabBar` / `TabBarView` |
| 点击反馈 | `GestureDetector` / `Listener` / `MouseRegion` | `InkWell` / `InkResponse` |

> **base-ui 未提供的组件**：一律按上述流程在 `base-ui-flutter` 中创建通用组件，禁止直接用 Material 组件替代。
> **布局基础组件**（`Row`、`Column`、`Stack`、`Container`、`ListView`、`GridView`、`CustomPaint` 等）不属于 Material 控件，可正常使用。
> **例外：查询页 SQL 编辑器**采用第三方包 `re_editor`（用户决策豁免 base-ui 组件禁令，见 `query_page.dart`）。其自带的 `CodeEditor` / 行号 / 语法高亮 / 补全框架不重复造轮子；但补全弹层视图（`_SqlPromptPanel`）仍遵循零延迟交互与 token 取色规范。

### `lib/widgets/` 仅允许页面级组合

`lib/widgets/` 中的文件只能是**页面级组合 widget**，满足以下全部条件：
- **业务耦合**：直接依赖 `AppState`、`DbData` 或应用数据模型
- **布局特定**：为 daro 的特定布局而设计，不可独立复用
- **组合性质**：由多个 base-ui 子组件组合而成，自身不包含独立 UI 原语

当前示例：`TopMenu`、`Ribbon`、`DatabaseTree`、`ObjectPanel`、`ViewTabs`、`StatusBar`、`QueryPage`、`TableDataPage`、`DatabaseInfo`、`ObjectTabs`、`TableIcon`

> **注意**：`TableIcon` 是特例（应用主题专用，绑定 `Tokens.of(context).tableIcon` 与 108 表项性能优化），不迁移到 base-ui。

### 放入 `base-ui-flutter`（可复用组件库）

满足以下全部条件：
- **无业务依赖**：不引用 `AppState`、`DbData` 或任何应用层代码
- **Token 驱动**：所有视觉值通过 `DesktopTokens` 或构造参数传入，零硬编码颜色 / 字体 / 间距
- **独立可用**：可在任意 Flutter 项目中直接使用，无需修改
- **有明确语义**：代表一个通用的 UI 控件或原语

> 注意：`TableIcon` 是 daro 应用级组件（绑定 `Tokens.of(context).tableIcon` 主题色与 108 表项性能优化），**不迁移到** `base-ui-flutter`。

#### 新增 base-ui-flutter 组件的步骤

1. 在 `base-ui-flutter/lib/src/<命名空间>/` 下创建 `.dart` 文件
2. 实现 widget，遵循 headless + token 驱动约定：
   - 接受可选 `tokens` 参数（`DesktopTokens?`）
   - 取色链：`tokens ?? TokenScope.maybeOf(context) ?? DesktopTokens.winForm`
   - 不硬编码任何颜色 / 字体 / 间距
3. 在 `base_ui_flutter.dart` barrel 中添加 `export`
4. 在 `CHANGELOG.md` 中记录
5. 在 `example/lib/pages/` 中添加演示页（可选但推荐）

---

## 视觉延迟 / 特效禁令（用户强烈要求，必须遵守）

**背景**：多次反馈"首次点击/选中慢半拍、点击特效、动画慢、菜单项黄线/加粗"。以下模式一律禁止：

1. **禁止 `onTap` 与 `onDoubleTap` 同时注册在同一手势目标**（InkWell / GestureDetector 都不行）：单击会被双击判定窗口 hold 约 300ms → 视觉延迟。
   ✅ 正确模式：**选中/常用操作 → `Listener.onPointerDown`（按下瞬间触发，零延迟）**，双击动作单独放 `GestureDetector.onDoubleTap`。
   参考：`object_panel.dart` 表项、`connection_dialog_page.dart` `_GridCard`/`_ListRow`、`database_tree.dart` `_node`、base-ui `list_view.dart` `_buildRow`。

2. **禁止用 `window_manager.DragToMoveArea` 包裹可点击区域**：其自带 `onDoubleTap`（双击最大化）→ 内部控件单击被 hold ~300ms（顶部菜单首次展开延迟的根因）。
   ✅ 改用 `GestureDetector(onPanStart: () => windowManager.startDragging())`（无双击判定）。

3. **禁止 `InkWell` / Material 水波纹特效**（用户明确反感"点击特效"）：用 `GestureDetector` / `Listener` / `MouseRegion` 手绘 hover / pressed。
   （残留待统一替换：`status_bar.dart` `_PanelToggleIcon`、`view_tabs.dart`、`table_data_page.dart`、`database_page.dart` 表卡片、`database_tree.dart` 表节点）

4. **禁止带勾选/点击动画的控件**：CheckBox 已重写为自绘无动画（WinForms 快节奏）。新增/重写组件默认无动画。

5. **菜单/浮层性能**：
   - MenuStrip 下拉面板**禁止 `Material(elevation)`**（阴影首次计算 / shader 编译 → "首次展开慢、之后快"）。
   - **禁止依赖 Material 的 `DefaultTextStyle` 兜底**：去掉 Material 后 Text 会继承应用级带 `decoration: underline/double/yellow` + `fontWeight: bold` 的样式（导致菜单项双黄线、加粗）。凡脱离 Material 的文本必须显式 `decoration: TextDecoration.none` + `fontWeight: FontWeight.w400`。
   - MenuStrip 顶层项高亮用 `Row(crossAxisAlignment: stretch)` 填满整行，避免 hover 窄带像"横线"。

6. **hover / 选中色必须明暗自适应**：基于目标底色派生（暗色提亮、亮色加深），禁止直接用 winForm 亮色默认值（暗色下突兀/不可见）。

---

## 主题切换

`AppState` 提供 `themeMode`（默认 `ThemeMode.system`）和 `cycleThemeMode()`。
顶部菜单栏右侧有主题切换按钮，在 **系统 → 明亮 → 暗黑** 之间循环。

顶部菜单栏（`lib/widgets/top_menu.dart`）本身由 base-ui-flutter 的 `MenuStrip` 提供，
原生支持「点击展开 + 菜单打开时鼠标移到其它菜单自动切换」的桌面菜单行为，
并通过 `AppPalette.toDesktopTokens()` 桥接跟随明/暗主题；本文件只负责装配假项目
菜单数据与右侧主题切换按钮。

```dart
// 代码中切换主题
context.read<AppState>().cycleThemeMode();

// 或直接设置
context.read<AppState>().setThemeMode(ThemeMode.dark);
```

---

## 连接分组（单层）

左侧连接树顶层可按分组折叠管理连接。约定与不变量：

| 位置 | 事实 |
|---|---|
| 模型 | `ConnectionInfo.group`（空串 = 未分组）+ `ConnGroup(name)` 独立条目，允许**空分组存在**（否则无法预建 / 重命名空分组） |
| 持久化 | `connections.json` 的 `groups` 键 + 每条连接的 `group`；老文件缺 `groups` 时按连接上的 `group` 现场补齐 |
| 状态 | `AppState` 的 `groups` / `groupNames` / `addGroup` / `renameGroup` / `deleteGroup` / `moveConnectionToGroup` / `ensureGroups`；分组变更**不改连接名**，因此不触发标签迁移与驱动重连 |
| 树 | 分组节点 `NodeKind.connGroup`（depth 0）→ 组内连接 depth 1；分组**默认展开**，折叠态记在 `_collapsedGroups`（不落盘）；一条分组都没有时保持平铺，不插入包装层；搜索 / 类型筛选只按连接名命中，空分组头随子项显隐 |
| 命名 | 树内**无弹窗**:新建分组直接落地占位名节点并就地编辑（`InlineEditor`，Esc 撤销新建并回移连接）；选中分组/连接/表节点按 **F2** 进入同一内联改名；仅连接向导下拉的「新建分组…」仍复用 base-ui `InputDialog`（`connection_group_prompt.dart`） |
| Navicat 导出 | `.ncx` 里 Navicat **原生没有分组字段**（实测本机 378 条连接的导出文件无任何 group/folder 属性，`SettingsSavePath` 恒为 `<根>\<类型>\Servers\<连接名>`），故 daro 用**私有属性** `Group="分组名"` 追加在实测属性表末尾；未分组不写该属性（文件与不带分组时逐字节一致） |
| Navicat 导入 | `NavicatConnection.group` 读私有属性 → `addConnections` 顺带登记缺失分组（即「分组不存在则重建」，幂等）；导入列表用分组列标出「·新建」 |

> 已知边界：`Group` 是 daro 私有属性，Navicat 打开该文件时忽略它（静态推断，未实机验证），分组只在 daro ↔ daro 之间往返。

---

## 编码规范

### 表格密度与单元格对齐
- 数据网格行高 **22**、单元格横向留白 **6**（`table_data_page.dart` / `query_page.dart` 的文件级常量），不用 base-ui 控件默认的 28 / `controlPaddingX`(12)——那是按钮的量。
- 列内对齐由 `DataGridViewColumn.alignment` 决定：**数值列 `Alignment.centerRight`、其余 `centerLeft`**，组件负责把内容垂直居中（表数据页按列类型判，`columnIsNumeric`；结果网格无类型元数据，按值判）。
- 编辑器同理：`InlineEditor` 传 `height: 行高` + 与显示态同向的 `textAlign`，进编辑时文字不跳位。

### 设计器密度档（表 / 视图 / 函数等「设计 X」页）
- 设计器网格与属性面板走 base-ui 的 **`gridCellTokens`**（字号 ≤12 / 控件 ≤20 高 / 水平留白 ≤5），
  整页包一层 `TokenScope` 下发，**不逐个控件传 `height`**（`ComboBox` 也没有 `height` 形参）。
- 具体度量集中在 `table_designer_page.dart` 的文件级 `_k*` 常量，每个值都注明了参考工具的实测来源，
  改之前先复量截图，别凭手感调。
- 单元格静止态是**扁平**的（`Input.showBorder` / `ComboBox.showBorder` = false）：只有 1px 网格线，
  没有每格一个框。当前格（锚点行 × 当前列）整格铺 `accent` + 反白文字，行首槽铺淡蓝档
  （`AppPalette.accentSoftBg`）+ 2px `highlight` 条，焦点列的表头同铺淡蓝档 + 2px 下划线。
- 勾选框传 `CheckBox.size`（实测 13）：方框是命中目标，不跟行高一起缩。
- 网格线宽一律用 `DesktopTokens.borderWidth`（宿主已收成 1 **设备**像素），不要写死 `1.0`——
  参考工具是原生窗口，1 物理像素在 200% 缩放下是 0.5 逻辑像素，写死 1.0 会粗一倍。
- 工具栏是**纯图标 + tooltip**（`ToolbarButton.paddingX` 收到 5.5 得到实测的 25 宽点击块），
  文字按钮与说明文字都不留；表名不在页面上填，新建时点「保存」弹窗要，已有表改名走对象树 F2。

### 设计器方言门控（表 / 视图设计器共用原则）

**界面能改、DDL 不生成**是设计器最容易长出来的缺陷类：用户在 MySQL 连接上写了三条规则，
点保存，库里一条都没有，而界面回「保存成功」。三条硬规矩：

1. **生成侧每个 `if (isPgLike)` 一类的方言门，都要有对应的拦口。**
   新建走 `DdlBuilder.createUnsupported(design, typeId)`，编辑走 `DdlBuilder.alterUnsupported(...)`。
   两者都并进 `TableDesignerPage._validation`，于是状态栏直接说清原因、保存键自动灰。
   只在 `_save()` 里拦是不够的：`_canSave` 依赖 `_dirty`，而 `_dirty` 只看 ALTER 清单，
   「改了触发器主体」清单为空 → 界面显示「无变更」，用户永远看不到拒绝原因。
2. **方言用不上的控件一律 `enabled: false`，不给「改了不生效」的入口。**
   门控条件必须与生成侧**逐字相同**（`isPgLike` / `isMysqlLike` / `isSqlServerLike`），
   并在 UI 上留注释指明是哪条子句把它挡在外（如「CONCURRENTLY 仅 PG」）。
   连各方言都不支持的项（触发器注释：没有 `COMMENT ON TRIGGER`）也要灰掉，别留空位。
3. **差异签名必须覆盖所有参与生成的字段。**
   漏一个（典型是后加的 `DesignTrigger.body`）就等于「改了但显示无变更」。
   不参与生成的字段（触发器注释）反过来**不要**进签名，否则会报假变更。

各方言的触发器语法互不相容，各自成函数（`_pgTriggerSql` / `_mysqlTriggerSql` /
`_sqlServerTriggerSql`），不共享拼装分支：MySQL 一个触发器只能挂一个事件（勾多个 →
各出一条 `CREATE` 并按事件加后缀，同名会 1359）、只有行级、无 `WHEN`、无 `TRUNCATE`；
SQL Server 是语句级、无 `BEFORE`（用 `AFTER` / `INSTEAD OF`）、事件用逗号列表、
触发器是 schema 级对象（名字要带模式前缀）；两者主体都内联（`body`），不像 PG 走独立函数。
MySQL 的 `BEGIN … END` 里的分号由服务端解析，**不需要也不应该发 `DELIMITER`**（那是 CLI 指令）。

### 触发器编辑（编辑模式）

触发器**支持在「设计表」里增删改**（不再被 `alterUnsupported` 阻断；规则 / 排除约束仍阻断）。
它是「整条重建」而不是 ALTER：`ALTER TABLE` 表达不了触发器，三种方言也都没有
`CREATE OR REPLACE TRIGGER`，所以差异清单固定是**先 `DROP` 后 `CREATE`**（见
`buildAlterStatements` 的 3b 段，复用与索引同一套 `diffByName`，按触发器名配对）。

**前提是驱动必须反查得到触发器 —— 这是这条链路最容易断的地方。**
若 `readTableDesign` 不回填 `triggers`，基线恒为空，模型会把库里**已有的**触发器
全当成「用户新增」而重复建（PG 报 42710 已存在，MySQL 直接撞名）。三驱动各一句：

| 驱动 | 取数 | 注意 |
|---|---|---|
| PostgreSQL | `pg_trigger` + `pg_get_triggerdef(oid, true)`，`WHERE NOT tgisinternal` | 反解析走 `parsePgTriggerDef`（见下） |
| MySQL / MariaDB | `information_schema.TRIGGERS` | 主体要过 `stripMysqlTriggerBeginEnd` |
| SQL Server | `sys.triggers` + `sys.trigger_events` + `OBJECT_DEFINITION()` | 主体要过 `stripSqlServerTriggerBody` |

三条必须记住的约束：

1. **外键的内部触发器不能进设计器**。PG 给每个外键偷偷建 4 个
   `RI_ConstraintTrigger_*`，靠 `NOT tgisinternal` 滤掉；放进来会变成一堆用户没写过的行，
   且删约束时它们会跟着消失，反查口径必须与依赖页一致。
2. **反查的「主体」要还原成设计器存的裸语句**，不能把外层包裹存回去。MySQL 库里存的是
   `BEGIN … END` 整段、SQL Server 的 `OBJECT_DEFINITION()` 返回整条 `CREATE TRIGGER … AS …`，
   而 `DesignTrigger.body` 只存裸语句（生成时自己包）。不做这层还原，
   「反查 → 原样保存」会套成 `BEGIN BEGIN … END; END;` 并逐次叠加。
   两个剥壳函数都是顶层导出（`stripMysqlTriggerBeginEnd` / `stripSqlServerTriggerBody`）便于离线回归。
3. **剥壳必须往返幂等**：`strip(gen(x)) == strip(x)`。不幂等的话，打开一张没动过的表
   `_dirty` 就会是 true，界面凭空显示「已修改」。改动这两个函数后要按这个不变量补用例。

PG 反解析为什么读 `pg_get_triggerdef` 而不是逐列拼 `tgtype` 位掩码：位掩码要自己组合
`BEFORE/AFTER/INSTEAD OF` 与七个事件位，拼错了不报错、只是静默生成错 DDL；函数给的是
服务端认定的真实定义。解析失败返回 `null`（宁可少一行让人看见，也不塞半条数据）。

`DROP TRIGGER` 的名字作用域各方言不同，**写错目标会悄悄删不掉或删错对象**：

- PG：`DROP TRIGGER x ON table` —— 表局部作用域，**必须带 `ON`**；
- SQL Server：`DROP TRIGGER schema.x` —— schema 级，**不带 `ON`**；
- MySQL：`DROP TRIGGER x` —— 库级，**不带 `ON`**，也没有模式层。

PG 的「禁用」不是建表子句，是建好后的独立动作（`ALTER TABLE … DISABLE TRIGGER`），
新建与编辑两条路径都要补，别只补一边。

### 设计器测试

- **绝不在 widget 测试里注册真连接**：`initState` 的 `_loadRefTables` / `_ensureCandidates`
  会真去建驱动连库，`pumpAndSettle` 等不到静止帧；假时钟下它**不会触发测试超时**，
  实测一个用例独占 10 分钟并带崩同文件其余用例。改走 `TableDesignerPage.dialectOverride`。
- 同一用例内换方言重开页面，**必须给页面 `ValueKey(typeId)`**：`_typeId` 只在 `initState`
  里定，缺 key 时走 `didUpdateWidget`，第二次打开仍是上一个方言。
- 属性面板整体挂在 `SingleChildScrollView` 里 → 子项拿到的是**无界高度**约束。
  里面的控件不能用 `expands: true` / `SizedBox.expand`（布局自激，同上会挂死 `pumpAndSettle`），
  多行框走 `Textarea` 固定 `minLines` + `maxLines`，行高由 `_propField(height:)` 预留。

### 颜色
- **应用 widget**：通过 `Tokens.of(context)` 取色，不硬编码 `Color(0xff…)`
- **base-ui-flutter 组件**：通过 `DesktopTokens` 取色，不接受 `Color` 参数
- 例外：Ribbon / QueryPage 的功能强调色（运行=绿、停止=红等）是主题无关的中调色，可直接定义

### 性能
- `ObjectPanel` 的 108 表项使用 `ValueListenableBuilder` + 按项通知器，避免全量重建
- `TableIcon` 按颜色缓存 painter，支持 100+ 图标共享同一 `CustomPainter` 实例
- 列表使用 `ListView.builder` + `itemExtent` 懒加载
- **设计器网格禁止「改一个格就整页 `setState`」**（`table_designer_page.dart`）：一屏有 160+ 个常开编辑器，
  整页重建实测每按一个键 ≈ 110–150ms（约 8fps，肉眼就是卡）。页面按依赖面分四档信号：
  `_rowRevs[i]`（所在行）/ `_chromeRev`（保存键、工具栏可用性、状态栏）/
  `_propsRev`（底部属性面板，只有换选中行或改到面板要显示的那个属性才刷）/
  `_curColRev`（表头当前列）。行与面板一律挂在对应 `ValueListenableBuilder` 上，
  且**选中态、当前列必须用取值闭包现读**（`anchor: () => _selColumn`），传快照值会显示成上一次的样子。
  新增单元格时给 `_modelInput` 传 `row:`，自定义控件的 `onChanged` 调 `_touchCell(row)`。
- 同理，「把整套 DDL 生成出来再看空不空」这类判定只能算一次：`_dirty` / 「SQL 预览」/ 保存确认
  共用缓存好的 `_alterStatements`，作废口是 `setState` 覆写 + `_touchCell` / `_touchChrome`。

### 命名
- 应用 widget：PascalCase，按功能命名（`TopMenu`、`DatabaseTree`）
- base-ui-flutter 组件：PascalCase，按 WinForm 语义命名（`Button`、`MenuStrip`、`ToolStrip`）

---

## 构建与验证

```bash
# 改过 lib/l10n/*.arb 后必须手动跑(analyze / test 都不触发 codegen)
flutter gen-l10n

# 本地化一致性守护
flutter test test/l10n_parity_test.dart

# 静态分析
flutter analyze

# 运行
flutter run -d windows

# base-ui-flutter 单独分析
cd base-ui-flutter && flutter analyze
```

---

## base-ui-flutter 组件速查

| 需求 | 组件 | 命名空间 |
|---|---|---|
| 菜单栏 | `MenuStrip` | menus |
| 右键菜单 | `ContextMenuStrip` | menus |
| 工具栏 | `ToolStrip` | menus |
| 状态栏 | `StatusStrip` | menus |
| 按钮 | `Button` | common |
| 输入框 | `Input` | common |
| 标签 | `Label` | common |
| 复选框 | `CheckBox` | common |
| 下拉框 | `ComboBox` | common |
| 表单字段 | `Field` | common |
| 标签页 | `TabControl` | containers |
| 分割容器 | `SplitContainer` | containers |
| 拖动改宽(独立分隔条) | `Splitter`（三栏/停靠面板：只上报像素增量，宽度由宿主状态掌控） | containers |
| 分组框 | `GroupBox` | containers |
| 树形视图 | `TreeView` | lists |
| 数据网格 | `DataGridView` | lists |
| 列表框 | `ListBox` | lists |
| 进度条 | `ProgressBar` | misc |
| 弹出层 | `Popover` | overlay |
| 对话框 | `MessageBox` | overlay |
| 多选候选弹窗 | `ListPickerDialog` | dialogs |
| 提示 | `Toast` | overlay |
| 头像 | `Avatar` | misc |
| 面包屑 | `Breadcrumb` | misc |

> 完整列表见 `base-ui-flutter/README.md`。
