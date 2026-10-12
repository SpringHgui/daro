import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../data/connection_store.dart';
import '../data/create_database_catalog.dart';
import '../data/database_edit_catalog.dart';
import '../data/db_create_options.dart';
import '../data/db_data.dart';
import '../data/db_edit_options.dart';
import '../data/db_export.dart';
import '../data/db_import.dart';
import '../data/drivers/db_driver.dart';
import '../data/locale_store.dart';
import '../data/matview_sql.dart';
import '../data/routine_sql.dart';
import '../data/saved_query_store.dart';
import '../data/sql_file_run.dart';
import '../data/table_design.dart';
import '../data/theme_store.dart';
import '../data/user_sql.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';
import 'cli_console.dart';
import 'connection_manager.dart';
import 'mcp_service.dart';

/// 中部视图标签的类型:对象浏览页 / 表数据页 / 查询编辑页 / 表设计器 / 命令列界面
enum TabType { object, table, query, design, createTable, commandLine }

/// 中部对象页当前浏览的对象分类(对应连接树的分组节点)
enum ObjectCategory { table, view, materializedView, function, procedure, user, query, backup }

extension ObjectCategoryX on ObjectCategory {
  /// 分类显示名(唯一数据源:Ribbon 按钮、连接树分组、对象面板均取此处,
  /// user 分类显示「角色」)。
  ///
  /// 显示名随界面语言变化,而本类拿不到 `BuildContext`,故由调用方把
  /// [AppLocalizations] 传进来;枚举的 `.name` 仍是稳定的英文标识,
  /// 用于能力匹配与持久化,不受语言影响。
  String labelOf(AppLocalizations l) => switch (this) {
        ObjectCategory.table => l.catTable,
        ObjectCategory.view => l.catView,
        ObjectCategory.materializedView => l.catMaterializedView,
        ObjectCategory.function => l.catFunction,
        ObjectCategory.procedure => l.catProcedure,
        ObjectCategory.user => l.catRole,
        ObjectCategory.query => l.catQuery,
        ObjectCategory.backup => l.catBackup,
      };

  /// 复数形态:英语的「删除 3 个 Tables」这类句子需要它,中文与日语没有单复数
  /// 变形、与 [labelOf] 同形。带数量的提示一律用它。
  String pluralOf(AppLocalizations l) => switch (this) {
        ObjectCategory.table => l.catTablePlural,
        ObjectCategory.view => l.catViewPlural,
        ObjectCategory.materializedView => l.catMaterializedViewPlural,
        ObjectCategory.function => l.catFunctionPlural,
        ObjectCategory.procedure => l.catProcedurePlural,
        ObjectCategory.user => l.catRole,
        ObjectCategory.query => l.catQuery,
        ObjectCategory.backup => l.catBackup,
      };
}

class OpenTab {
  const OpenTab(
    this.type,
    this.title, [
    this.typeId,
    this.connection,
    this.database,
    this.schema,
    this.routineCategory,
    this.routineParams,
    this.routineComment,
    this.routineOptions,
    this.routineReturnType,
  ]);

  final TabType type;
  final String title;

  /// 数据库/表等业务类型标识(如数据库类型 id: postgresql / mysql)
  final String? typeId;

  /// 表数据页所属连接(真实连接时非空,数据页据此走真实查询)
  final String? connection;

  /// 表数据页所属数据库(真实连接时非空)
  final String? database;

  /// 表数据页所属模式(PostgreSQL 等有模式层的类型;无模式层为 null)
  final String? schema;

  /// 视图 / 函数设计标签的分类(仅 routine 设计页使用,表设计为空)
  final ObjectCategory? routineCategory;

  /// 例程设计页初始参数签名(向导第 2 步采集,如 `IN "a" pg_catalog.bool`;
  /// 仅新建模式携带)
  final String? routineParams;

  /// 例程设计页初始注释(向导 / 设计页「注释」标签内容)
  final String? routineComment;

  /// 向导第 3 步的选项(窗口 / 特性 / 防漏 / STRICT / 安全性;仅新建模式携带)
  final RoutineOptions? routineOptions;

  /// 向导第 2 步采集的返回类型(函数专用,可带类型模式,如
  /// `pg_catalog."char"`;仅新建模式携带)
  final String? routineReturnType;

  /// 同类 tab 的业务唯一键(同名表在不同库/模式可共存)
  String get key =>
      '$type|$connection|$database|$schema|$title|$routineCategory';
}

/// 新建表时的列定义(由表设计对话框采集)
class TableColumnSpec {
  TableColumnSpec({
    this.name = '',
    this.type = '',
    this.nullable = true,
    this.primaryKey = false,
    this.defaultValue = '',
  });

  String name;
  String type;
  bool nullable;
  bool primaryKey;
  String defaultValue;
}

/// DDL 操作结果:ok 为是否成功,error 携带失败原因(成功时为空)
class DdlOutcome {
  const DdlOutcome(this.ok, [this.error, this.failedAt = 0, this.rolledBack = false]);

  final bool ok;
  final String? error;

  /// 批量语句中失败发生在第几条(1 基;0 = 非批量或未执行到语句)
  final int failedAt;

  /// 失败时是否已事务回滚(仅事务化执行的方言为 true;
  /// 否则之前的语句已生效)
  final bool rolledBack;
}

/// 对象面板 Ctrl+C 记下的表剪贴板:[tables] 为源表名,来源上下文一并记录,
/// 粘贴时据此定位(跨 连接/库/模式 的粘贴不支持)
class TableClipboard {
  const TableClipboard({
    required this.tables,
    required this.connection,
    required this.database,
    this.schema,
  });

  final List<String> tables;
  final String connection;
  final String database;
  final String? schema;
}

/// 连接树 / 对象面板中可选中的节点种类
enum NodeKind {
  /// 连接分组(左侧树顶层文件夹,仅视图层,不对应任何服务端对象)
  connGroup,
  connection,
  database,
  schema,
  tableGroup,
  table,
}

/// 表数据页的分页 / 记录位置状态,供状态栏显示
/// "第 X 条记录（共 N 条）于第 P 页"。总数未知(未统计)时 totalRows 为 null。
class TablePageStatus {
  const TablePageStatus({
    required this.totalRows,
    required this.currentRecord,
    required this.page,
    required this.pageSize,
    this.selectedRowCount = 0,
  });

  /// 记录总数(null 表示尚未统计,如表数据页按需 COUNT 模式)
  final int? totalRows;

  /// 当前记录号(1-based;选中行优先,否则为当前页末条)
  final int currentRecord;

  /// 当前页码(1-based)
  final int page;

  /// 每页行数
  final int pageSize;

  /// 多行选中数(>1 时状态栏改显示「已选 N 行」替代当前记录号;
  /// 0 / 1 与旧行为一致,展示 currentRecord)
  final int selectedRowCount;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is TablePageStatus &&
          other.totalRows == totalRows &&
          other.currentRecord == currentRecord &&
          other.page == page &&
          other.pageSize == pageSize &&
          other.selectedRowCount == selectedRowCount;

  @override
  int get hashCode =>
      Object.hash(totalRows, currentRecord, page, pageSize, selectedRowCount);
}

/// 详情面板当前跟随的选中节点(值相等即视为同一节点)。
/// connection/database/schema 携带节点所属上下文,
/// 不同库 / 模式的同名表视为不同节点
class SelectedNode {
  const SelectedNode(this.kind, this.name,
      {this.connection, this.database, this.schema});

  final NodeKind kind;
  final String name;

  /// 所属连接名(树/对象面板选中时携带)
  final String? connection;

  /// 所属数据库(表/库/模式节点携带)
  final String? database;

  /// 所属模式(模式/表节点携带,PostgreSQL 等)
  final String? schema;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is SelectedNode &&
          other.kind == kind &&
          other.name == name &&
          other.connection == connection &&
          other.database == database &&
          other.schema == schema;

  @override
  int get hashCode =>
      kind.hashCode ^ name.hashCode ^ Object.hash(connection, database, schema);
}

class AppState extends ChangeNotifier {
  AppState() {
    // 启动即从本地加载已保存的连接、查询与定制主题(并行,完成后通知重建)
    _initialLoad = Future.wait([
      _loadPersisted(),
      _loadSavedQueries(),
      loadCustomTheme(),
      loadLanguage(),
    ]);
    // MCP 起宿主放在连接加载之后:否则 agent 抢在首帧前连上只会看到空连接列表。
    unawaited(_initialLoad.then((_) => mcp.bootstrap()));
  }

  /// 主题模式:默认跟随系统,可在顶部菜单手动切换
  ThemeMode themeMode = ThemeMode.system;

  /// 语言偏好: null = 跟随系统,可在「工具 → 选项… → 常规」里手动切换。
  /// 生效语言由 [resolveLocale] 结合系统语言算出。
  String? languageCode;

  /// 真实连接管理:驱动连接池 + 树元数据懒加载状态
  final ConnectionManager connectionManager = ConnectionManager();

  /// 连接配置持久化存储(应用支持目录 connections.json)
  final ConnectionStore _store = ConnectionStore();

  /// 定制主题持久化存储(应用支持目录 theme_custom.json)
  final ThemeStore _themeStore = ThemeStore();

  /// 已保存查询持久化存储(应用支持目录 queries.json)
  final SavedQueryStore _queryStore = SavedQueryStore();

  /// 密码补录(W14)桥接:将密码存入内存连接列表后返回 true,
  /// 驱动重试时会自动使用新密码;取消或未输入则返回 false.
  late final McpPasswordAsker mcpAskPassword = (ConnectionInfo conn) async {
    // 从当前连接列表查找同名连接,注入密码。
    try {
      final index = _connections.indexWhere((c) => c.name == conn.name);
      if (index >= 0) {
        // 直接修改连接对象上的 password 字段(池会复用同一 conn 引用)。
        final updated = _connections[index].copyWith(password: '');
        _connections[index] = updated;
      }
    } catch (_) {
      // 静默忽略:即使更新失败也不应阻断 UI。
    }
    // 实际密码收集通过 openSubWindow 完成(已在 McpService._askForPassword).
    // 此处仅做状态同步:子窗口完成后工具层会重新调用 _loadConnections().
    return true;
  };

  /// MCP 服务:内嵌 HTTP 宿主 + 专用驱动池 + 审计日志(默认关闭,设置页里开)。
  ///
  /// 用 `late final` 而非字段初始化器:它要引用本实例的连接列表与 [openTable]。
  /// 密码补录(W14)桥接在设置页接线时补上 —— 未注入时工具层直接回 `PASSWORD_REQUIRED`,
  /// 不会静默挂住 agent。
  late final McpService mcp = McpService(
    loadConnections: () async => connections,
    askPassword: mcpAskPassword,
    openTableBridge: (connection, database, table) async =>
        openTable(table, connection: connection, database: database),
  );

  /// 已保存查询(对象面板「查询」分类的数据源,启动时从本地加载)
  final List<SavedQuery> _savedQueries = [];

  /// 启动时的首次加载 future;后续落盘等待它完成,避免覆盖
  late final Future<void> _initialLoad;

  @override
  void dispose() {
    // 关掉 MCP 监听 + 丢弃专用驱动实例:热重启与测试收尾都不该留端口占用。
    mcp.dispose();
    objectSearchController.dispose();
    super.dispose();
  }

  // ── 主题定制 ──────────────────────────────────────────────

  /// 用户定制的明 / 暗色板覆盖(null = 用 [AppTheme.light] / [AppTheme.dark] 默认值)
  AppPalette? _customLight;
  AppPalette? _customDark;

  /// 当前生效的明亮色板:有定制用定制,否则默认
  AppPalette get effectiveLight => _customLight ?? AppTheme.light;

  /// 当前生效的暗黑色板:有定制用定制,否则默认
  AppPalette get effectiveDark => _customDark ?? AppTheme.dark;

  /// 启动时从本地加载定制色板;文件不存在 / 损坏时保持默认
  Future<void> loadCustomTheme() async {
    final stored = await _themeStore.load();
    _customLight = stored.light;
    _customDark = stored.dark;
    notifyListeners();
  }

  /// 应用定制色板(立即生效并落盘);传入 null 表示该亮度不变
  void setCustomPalette({AppPalette? light, AppPalette? dark}) {
    if (light != null) _customLight = light;
    if (dark != null) _customDark = dark;
    notifyListeners();
    _themeStore.save(_customLight, _customDark);
  }

  /// 重置定制色板:[b] 指定要清的亮度,all=true 清两套
  void resetCustomPalette(Brightness b, {bool all = false}) {
    if (all || b == Brightness.light) _customLight = null;
    if (all || b == Brightness.dark) _customDark = null;
    notifyListeners();
    _themeStore.save(_customLight, _customDark);
  }

  /// 左侧连接树数据源:启动时从本地加载,新建的连接追加进来并落盘
  final List<ConnectionInfo> _connections = [];

  /// 连接列表的不可变视图缓存:连接树用 `context.select` 订阅它,
  /// 只有 `addConnection` 更新该引用时树才重建,其它 AppState 变化不触发
  late List<ConnectionInfo> _connectionsView = List.unmodifiable(_connections);

  /// 连接树条目(不可变视图)
  List<ConnectionInfo> get connections => _connectionsView;

  /// 连接分组(顶层文件夹)。与连接一样维护不可变视图,供连接树 `select` 订阅。
  ///
  /// 允许空分组存在:删除组内最后一个连接、或刚新建还没放东西,分组都保留,
  /// 要清掉得显式「删除分组」。
  final List<ConnGroup> _groups = [];
  late List<ConnGroup> _groupsView = List.unmodifiable(_groups);

  /// 分组列表(不可变视图)
  List<ConnGroup> get groups => _groupsView;

  /// 分组名的快捷视图(表单下拉候选、右键菜单用)
  List<String> get groupNames => [for (final g in _groups) g.name];

  /// 把 [name] 登记为分组(已存在或为空名则忽略);批量导入与新建连接共用。
  ///
  /// 返回是否真的新增,调用方据此汇报「新建分组 N 个」。
  bool ensureGroup(String name) {
    final trimmed = name.trim();
    if (trimmed.isEmpty ||
        _groups.any((g) => g.name.toLowerCase() == trimmed.toLowerCase())) {
      return false;
    }
    _groups.add(ConnGroup(name: trimmed));
    _groupsView = List.unmodifiable(_groups);
    return true;
  }

  /// 登记一批分组,只在真有新增时通知 + 落盘一次(Navicat 导入一次带来上百条时用)
  int ensureGroups(Iterable<String> names) {
    final added = [for (final n in names) if (ensureGroup(n)) n];
    if (added.isEmpty) return 0;
    notifyListeners();
    _persist();
    return added.length;
  }

  /// 新建分组。名称空、或只空白时返回 null 表示未建立;重名直接返回原名。
  ///
  /// 大小写不敏感判重:同名的两种写法在树里几乎无法区分,不如拒掉。
  String? addGroup(String name) {
    final trimmed = name.trim();
    if (trimmed.isEmpty) return null;
    if (_groups.any((g) => g.name.toLowerCase() == trimmed.toLowerCase())) {
      return trimmed;
    }
    ensureGroup(trimmed);
    notifyListeners();
    _persist();
    return trimmed;
  }

  /// 重命名分组:同步改写组内每条连接的 `group`。
  ///
  /// 连接名不变,所以不涉及标签迁移与驱动重连(那两个动作按连接名索引)。
  /// 返回 false 表示目标名非法(空 / 与既有分组重名)。
  bool renameGroup(String from, String to) {
    final trimmed = to.trim();
    if (trimmed.isEmpty || trimmed == from) return trimmed == from;
    final index = _groups.indexWhere((g) => g.name == from);
    if (index < 0) return false;
    if (_groups.any((g) => g.name.toLowerCase() == trimmed.toLowerCase())) {
      return false;
    }
    _groups[index] = ConnGroup(name: trimmed);
    _groupsView = List.unmodifiable(_groups);
    for (var i = 0; i < _connections.length; i++) {
      if (_connections[i].group == from) {
        _connections[i] = _connections[i].copyWith(group: trimmed);
      }
    }
    _connectionsView = List.unmodifiable(_connections);
    notifyListeners();
    _persist();
    return true;
  }

  /// 删除分组:组内连接回落到「未分组」,连接本身与驱动都不动。
  void deleteGroup(String name) {
    final before = _groups.length;
    _groups.removeWhere((g) => g.name == name);
    if (_groups.length == before) return;
    _groupsView = List.unmodifiable(_groups);
    for (var i = 0; i < _connections.length; i++) {
      if (_connections[i].group == name) {
        _connections[i] = _connections[i].copyWith(group: '');
      }
    }
    _connectionsView = List.unmodifiable(_connections);
    notifyListeners();
    _persist();
  }

  /// 把一条连接移动到分组([group] 传空串 = 移出分组)。分组不存在则顺手建。
  void moveConnectionToGroup(ConnectionInfo conn, String group) {
    final index = _connections.indexWhere((c) => c.name == conn.name);
    if (index < 0) return;
    final target = group.trim();
    if (_connections[index].group == target) return;
    if (target.isNotEmpty) ensureGroup(target);
    _connections[index] = _connections[index].copyWith(group: target);
    _connectionsView = List.unmodifiable(_connections);
    _groupsView = List.unmodifiable(_groups);
    notifyListeners();
    _persist();
  }

  /// 连接树「定位新增连接」信号:连接向导确定 / 复制连接后,树选中该连接
  /// 并滚动到其所在位置。载荷为 (单调序号, 连接名):序号保证同名连续新增
  /// 也能触发(ValueNotifier 值相等不通知,与 [treeNavigate] 的计数约定同理)。
  /// 批量导入 [addConnections] 刻意不触发——一次涌入上百条不该逐条滚动定位。
  final ValueNotifier<(int, String)> connectionAdded =
      ValueNotifier<(int, String)>((0, ''));

  /// 连接向导「确定」时添加一条新连接、落盘并刷新树。
  /// 同名连接会在名称后追加序号(ConnectionManager 按名称索引驱动,不允许重名)
  void addConnection(ConnectionInfo conn) {
    final added = _uniqueNamed(conn);
    _connections.add(added);
    ensureGroup(conn.group);
    _connectionsView = List.unmodifiable(_connections);
    _groupsView = List.unmodifiable(_groups);
    notifyListeners();
    _persist();
    // 通知树选中并滚动定位到新连接(以重名避让后的最终名为准)
    connectionAdded.value = (connectionAdded.value.$1 + 1, added.name);
  }

  /// 批量添加连接(Navicat 导入这类一次带来上百条的场景)。
  /// 逐条沿用同名自动改名规则,但只落盘一次:connections.json 是全量覆写,
  /// 按 [addConnection] 逐条调用会写出上百份同样的文件。
  void addConnections(List<ConnectionInfo> conns) {
    if (conns.isEmpty) return;
    for (final conn in conns) {
      _connections.add(_uniqueNamed(conn));
      ensureGroup(conn.group);
    }
    _connectionsView = List.unmodifiable(_connections);
    _groupsView = List.unmodifiable(_groups);
    notifyListeners();
    _persist();
  }

  /// 删除一条连接并断开其驱动;
  /// 若该连接正被中部对象页浏览,同步清空对象上下文
  Future<void> removeConnection(ConnectionInfo conn) async {
    _connections.removeWhere((c) => c.name == conn.name);
    _connectionsView = List.unmodifiable(_connections);
    notifyListeners();
    _persist();
    await connectionManager.disconnect(conn.name);
    if (objectConnection == conn.name) {
      objectConnection = null;
      objectDatabase = null;
      objectSchema = null;
      _clearTableSelection();
      notifyListeners();
    }
  }

  /// 更新一条连接(编辑连接后调用)。
  /// 名称与其他连接冲突时自动追加「 (2)」等序号;
  /// 返回最终生效的连接(未找到原连接时返回 null)。
  Future<ConnectionInfo?> updateConnection(
    ConnectionInfo oldConn,
    ConnectionInfo newConn,
  ) async {
    final index = _connections.indexWhere((c) => c.name == oldConn.name);
    if (index < 0) return null;
    final updated = _uniqueNamed(newConn, exclude: oldConn.name);
    _connections[index] = updated;
    ensureGroup(updated.group);
    _connectionsView = List.unmodifiable(_connections);
    _groupsView = List.unmodifiable(_groups);
    notifyListeners();
    _persist();
    // 改名后:迁移打开标签的引用与按 key 缓存的编辑状态,断开旧名驱动
    // (新名的驱动与元数据状态由树展开时懒加载重建)
    if (updated.name != oldConn.name) {
      _migrateTabsConnection(oldConn.name, updated.name);
      await connectionManager.disconnect(oldConn.name);
    }
    return updated;
  }

  /// 把「打开连接」时补录的密码写入内存中的连接。
  /// 未勾选「保存密码」的连接密码为空,弹窗补录后即可正常连接与断线重连;
  /// [save] 为 false 时密码只保留在本次会话,重启后需重新输入。
  ConnectionInfo? setConnectionPassword(ConnectionInfo conn, String password,
      {bool save = false}) {
    final index = _connections.indexWhere((c) => c.name == conn.name);
    if (index < 0) return null;
    final updated = _connections[index].copyWith(password: password);
    _connections[index] = updated;
    _connectionsView = List.unmodifiable(_connections);
    notifyListeners();
    if (save) _persist();
    return updated;
  }

  /// 复制一条连接:名称追加「 (2)」等序号,其余配置原样复制
  void copyConnection(ConnectionInfo conn) {
    final copy = _uniqueNamed(conn.copyWith());
    _connections.add(copy);
    _connectionsView = List.unmodifiable(_connections);
    notifyListeners();
    _persist();
    // 复制同样产出一条新连接实例:树选中并滚动定位(与 addConnection 同口径)
    connectionAdded.value = (connectionAdded.value.$1 + 1, copy.name);
  }

  /// 连接改名后:把打开标签的 connection 引用切到新名,
  /// 并迁移按 key(OpenTab.key / queryTabKey)缓存的查询文本、SQL 历史与分页状态
  void _migrateTabsConnection(String oldConn, String newConn) {
    for (var i = 0; i < tabs.length; i++) {
      final tab = tabs[i];
      if (tab.connection != oldConn) continue;
      final oldKey = tab.key;
      final oldQk = queryTabKey(oldConn, tab.database, tab.title);
      final migrated = OpenTab(
        tab.type,
        tab.title,
        tab.typeId,
        newConn,
        tab.database,
        tab.schema,
        tab.routineCategory,
        tab.routineParams,
        tab.routineComment,
        tab.routineOptions,
        tab.routineReturnType,
      );
      tabs[i] = migrated;
      _moveKeyedState(oldKey, migrated.key);
      _moveKeyedState(oldQk, queryTabKey(newConn, tab.database, tab.title));
    }
  }

  /// 把某个 key 下的表分页状态 / 查询文本 / 例程文本与注释 / SQL 历史
  /// 迁移到新 key(存在才迁移)
  void _moveKeyedState(String oldKey, String newKey) {
    if (oldKey == newKey) return;
    final status = _tableStatus.remove(oldKey);
    if (status != null) _tableStatus[newKey] = status;
    final text = _queryTexts.remove(oldKey);
    if (text != null) _queryTexts[newKey] = text;
    final routine = _routineTexts.remove(oldKey);
    if (routine != null) _routineTexts[newKey] = routine;
    final comment = _routineComments.remove(oldKey);
    if (comment != null) _routineComments[newKey] = comment;
    final history = _tabSqlHistory.remove(oldKey);
    if (history != null) _tabSqlHistory[newKey] = history;
  }

  /// 若名称已存在则追加 " (2)"、" (3)" … 直到不重名;
  /// [exclude] 为允许占用但不参与判重的连接名(编辑连接改名时用)
  ConnectionInfo _uniqueNamed(ConnectionInfo conn, {String? exclude}) {
    final names = _connections
        .where((c) => c.name != exclude)
        .map((c) => c.name)
        .toSet();
    if (!names.contains(conn.name)) return conn;
    var i = 2;
    while (names.contains('${conn.name} ($i)')) {
      i++;
    }
    return conn.copyWith(name: '${conn.name} ($i)');
  }

  /// 重新从本地加载连接(菜单「查看 → 刷新」);
  /// addConnection 均已落盘,重载后与内存状态一致
  Future<void> reloadConnections() => _loadPersisted();

  /// 启动时从本地加载已保存的连接。
  ///
  /// 不能简单 `clear()` 后整体替换:加载是异步的,期间可能已经有连接加进来
  /// (刚启动就完成一次导入 / 自动化用例),整体替换会把这些尚未落盘的改动抹掉,
  /// 随后 [_persist] 又会把空列表写回磁盘。因此只补齐「磁盘有、内存没有」的条目,
  /// 常规启动路径下内存为空,结果与整体替换完全一致。
  Future<void> _loadPersisted() async {
    final (saved, savedGroups) = await _store.load();
    final pending =
        _connections.where((m) => saved.every((s) => s.name != m.name)).toList();
    final pendingGroups = [
      for (final g in _groups)
        if (savedGroups.every((s) => s.name != g.name)) g,
    ];
    _connections
      ..clear()
      ..addAll(saved)
      ..addAll(pending);
    _groups
      ..clear()
      ..addAll(savedGroups)
      ..addAll(pendingGroups);
    // 老配置只有连接上的 group、没有 groups 条目:现场补齐,分组才不会在树里消失
    for (final conn in _connections) {
      ensureGroup(conn.group);
    }
    _connectionsView = List.unmodifiable(_connections);
    _groupsView = List.unmodifiable(_groups);
    notifyListeners();
  }

  /// 全量落盘(等待首次加载完成后再写,防止启动竞态覆盖旧配置)
  Future<void> _persist() async {
    await _initialLoad;
    await _store.save(_connections, _groups);
  }

  /// 启动时从本地加载已保存查询(查询管理面板数据源)
  Future<void> _loadSavedQueries() async {
    _savedQueries
      ..clear()
      ..addAll(await _queryStore.load());
    notifyListeners();
  }

  /// 已保存查询全量落盘(等待首次加载完成后再写)
  Future<void> _persistQueries() async {
    try {
      await _initialLoad;
      await _queryStore.save(_savedQueries);
    } catch (_) {
      // 落盘失败不阻断功能(下次保存会再尝试全量覆写)
    }
  }

  /// 左侧面板(连接树)可见性
  bool leftPanelVisible = true;

  /// 右侧面板(详情信息)可见性
  bool rightPanelVisible = true;

  /// 侧栏宽度(拖动分隔条调整)。用 [ValueNotifier] 而非普通字段:拖动每帧都会
  /// 变更宽度,走 notifyListeners 会连带重建所有 AppState 监听者
  final ValueNotifier<double> leftPanelWidth = ValueNotifier<double>(240);
  final ValueNotifier<double> rightPanelWidth = ValueNotifier<double>(300);

  /// 按分隔条位移调整左栏宽度([delta] 为像素,向右为正)
  void resizeLeftPanel(double delta) =>
      leftPanelWidth.value = (leftPanelWidth.value + delta).clamp(180.0, 600.0);

  /// 按分隔条位移调整右栏宽度([delta] 为像素,向右为正故取反)
  void resizeRightPanel(double delta) =>
      rightPanelWidth.value = (rightPanelWidth.value - delta).clamp(200.0, 640.0);

  /// 连接树搜索文本(右下角搜索框)
  String treeSearchText = '';

  /// 连接树筛选:选中的数据库类型 id 集合(空=全部)
  Set<String> selectedDbTypes = {};

  /// 对象面板布局:true = 多列网格(详细布局), false = 单列列表
  bool objectGridLayout = true;

  /// 对象面板工具栏搜索框的输入控制器。状态放在 AppState 而不是面板里:
  /// 打开一张表会让面板整棵子树销毁,回到「对象」标签时是全新实例;
  /// 且搜索是全局口径——换连接 / 库 / 分类都沿用同一份过滤词。
  final TextEditingController objectSearchController = TextEditingController();

  /// [objectSearchController] 内容的规范化副本(小写,供子串匹配)。
  /// 由面板在 onChanged 里回写并 setState 重画列表,故不再另设通知。
  String objectSearchText = '';

  // ── 对象浏览上下文 ─────────────────────────────

  /// 中部对象页当前浏览的连接名(null = 未选择,显示空态)
  String? objectConnection;

  /// 中部对象页当前浏览的数据库
  String? objectDatabase;

  /// 中部对象页当前浏览的模式(PostgreSQL 等;null = 未选/默认模式)
  String? objectSchema;

  /// 中部对象页当前浏览的对象分类(表 / 视图 / 函数等,默认表)
  ObjectCategory objectCategory = ObjectCategory.table;

  /// 当前对象页展示的表列表(由对象面板加载后回写,供 Shift 范围选择用)
  List<String> objectTables = const [];

  /// 连接树单击库 / 模式节点:设置对象页浏览上下文并触发对象列表加载。
  /// [schema] 为 null 表示库级(默认模式)上下文
  void setObjectContext(
    String connection,
    String database, {
    ObjectCategory category = ObjectCategory.table,
    String? schema,
  }) {
    if (objectConnection == connection &&
        objectDatabase == database &&
        objectSchema == schema &&
        objectCategory == category) {
      return;
    }
    objectConnection = connection;
    objectDatabase = database;
    objectSchema = schema;
    objectCategory = category;
    // 切换上下文时清空对象面板残留的选中
    _clearTableSelection();
    notifyListeners();
  }

  /// 清空对象浏览上下文(右键关闭连接 / 库 / 模式且该节点正被浏览时调用):
  /// 面板回到「未选择数据库」空态,操作栏整体禁用,不残留任何数据。
  void clearObjectContext() {
    if (objectConnection == null &&
        objectDatabase == null &&
        objectSchema == null) {
      return;
    }
    objectConnection = null;
    objectDatabase = null;
    objectSchema = null;
    _clearTableSelection();
    notifyListeners();
  }

  /// 连接树导航请求信号:Ribbon 按钮点击后通知树展开祖先并选中分组节点。
  /// int 为单调递增序号,保证连续相同参数也能触发 ValueListenableBuilder 重建。
  final ValueNotifier<int> treeNavigate = ValueNotifier<int>(0);

  /// Ribbon 快速切换:激活"对象"页并浏览当前上下文库的指定分类。
  /// 未选择数据库时仅切换分类(联动 Ribbon 高亮),不自动选库、不触发任何加载;
  /// 已选库时同样只切分类并通知连接树高亮分组节点——数据加载一律由
  /// 树中展开(打开)库/模式节点驱动,未打开时对象面板保持空态、操作栏禁用。
  void showObjectCategory(ObjectCategory category) {
    final conn = objectConnection;
    final db = objectDatabase;

    // 未选择数据库:仅切换分类,不展示任何数据
    if (conn == null || db == null) {
      if (objectCategory != category) {
        objectCategory = category;
        _clearTableSelection();
      }
      activeTab = objectsTabKey;
      notifyListeners();
      return;
    }

    if (objectCategory != category) {
      objectCategory = category;
      _clearTableSelection();
    }
    activeTab = objectsTabKey;
    notifyListeners();
    // 通知连接树选中分组节点(仅高亮联动,不触发数据加载)
    treeNavigate.value++;
  }

  /// 对象面板加载出表列表后回写(Shift 范围选择依赖顺序列表)
  void setObjectTables(List<String> tables) {
    objectTables = tables;
  }

  /// 清空对象面板选中(不通知,由调用方决定)
  void _clearTableSelection() {
    for (final old in selectedTables) {
      itemNotifierFor(old).value = false;
    }
    selectedTables.clear();
    selectionNotifier.value = const <String>{};
  }

  /// 清空对象面板选中(删除对象后调用;按项通知器自带精确重建,
  /// 无需 Provider 全量通知)
  void clearObjectSelection() => _clearTableSelection();

  /// 切换对象面板布局(网格 / 列表)
  void setObjectLayout(bool grid) {
    if (objectGridLayout == grid) return;
    objectGridLayout = grid;
    notifyListeners();
  }

  // ── 连接树筛选 ──────────────────────────────────────────────

  void setTreeSearchText(String text) {
    treeSearchText = text;
    notifyListeners();
  }

  void toggleDbTypeFilter(String typeId) {
    final next = Set<String>.from(selectedDbTypes);
    if (next.contains(typeId)) {
      next.remove(typeId);
    } else {
      next.add(typeId);
    }
    selectedDbTypes = next;
    notifyListeners();
  }

  void clearTreeFilter() {
    treeSearchText = '';
    selectedDbTypes = {};
    notifyListeners();
  }

  /// 筛选后的连接列表:按搜索文本 + 类型过滤
  List<ConnectionInfo> get filteredConnections {
    final text = treeSearchText.toLowerCase().trim();
    return _connections.where((conn) {
      if (text.isNotEmpty && !conn.name.toLowerCase().contains(text)) {
        return false;
      }
      if (selectedDbTypes.isNotEmpty && !selectedDbTypes.contains(conn.typeId)) {
        return false;
      }
      return true;
    }).toList();
  }

  void setThemeMode(ThemeMode mode) {
    if (themeMode == mode) return;
    themeMode = mode;
    notifyListeners();
  }

  /// 在 跟随系统 -> 明亮 -> 暗黑 之间循环切换
  void cycleThemeMode() {
    themeMode = switch (themeMode) {
      ThemeMode.system => ThemeMode.light,
      ThemeMode.light => ThemeMode.dark,
      ThemeMode.dark => ThemeMode.system,
    };
    notifyListeners();
  }

  // ── 语言 ──────────────────────────────────────────────────

  /// 语言偏好持久化存储(应用支持目录 locale.json)
  final LocaleStore _localeStore = LocaleStore();

  /// 启动时读取语言偏好;文件缺失 / 损坏时保持「跟随系统」
  Future<void> loadLanguage() async {
    final stored = await _localeStore.load();
    if (stored == languageCode) return;
    languageCode = stored;
    notifyListeners();
  }

  /// 切换界面语言:[code] 传 null 表示改回跟随系统。立即生效并落盘。
  ///
  /// 落盘失败(只读目录等)不影响本次会话,故不向上抛。
  void setLanguageCode(String? code) {
    if (languageCode == code) return;
    languageCode = code;
    notifyListeners();
    unawaited(_localeStore.save(code).catchError((_) {}));
  }

  /// 当前生效的 locale:[languageCode] 为空时按系统语言解析。
  ///
  /// [system] 仅供测试注入;运行时读平台语言。
  Locale resolveLocale([Locale? system]) => resolveAppLocale(
        override: languageCode,
        system: system ?? WidgetsBinding.instance.platformDispatcher.locale,
      );

  /// 已打开的标签(不含固定的"对象"首页)
  final List<OpenTab> tabs = [];

  /// 「对象」首页标签的**身份标识**。
  ///
  /// 标签身份一直是标题字符串([activeTab] 与 [OpenTab.title] 比对),但固定首页
  /// 的标题要随界面语言显示成「对象 / Objects / オブジェクト」—— 拿译文当身份,
  /// 切一次语言活动标签就找不到自己了。故这里存一个不可能与表名撞车的哨兵,
  /// 显示文案由 [OpenTab] 之外按词条取。
  static const String objectsTabKey = '@@objects';

  /// 当前活动标签标题,[objectsTabKey] 表示对象浏览页
  String activeTab = objectsTabKey;

  String selectedTable = '';

  /// 对象面板中当前被选中的表(支持 Ctrl 多选)
  final Set<String> selectedTables = {};

  /// 选中状态独立通知器,避免触发 Provider 全量级联
  final selectionNotifier = ValueNotifier<Set<String>>(<String>{});

  /// 详情面板跟随的选中节点(连接/库/模式/表分组/表)。
  /// 用独立 ValueNotifier 驱动,仅重建右侧详情面板,不影响表项性能优化。
  final detailSelection = ValueNotifier<SelectedNode?>(null);

  /// 树操作日志:最近一条打开/关闭节点操作,供底部状态栏展示
  final treeLog = ValueNotifier<String?>(null);

  /// 记录树操作日志(打开/关闭连接、库、模式、分组)
  void logTreeAction(String message) => treeLog.value = message;

  /// 按项独立通知器:每个表名对应一个 ValueNotifier<bool>,
  /// 选中操作仅重建实际变化的 1~2 个项,避免全表项重建
  final Map<String, ValueNotifier<bool>> _itemNotifiers = {};

  ValueNotifier<bool> itemNotifierFor(String name) =>
      _itemNotifiers[name] ??= ValueNotifier<bool>(false);

  int _queryCount = 0;

  // ── 查询编辑页 SQL 文本(按 tab key 保存,切换标签不丢失) ──

  final Map<String, String> _queryTexts = {};

  /// 获取指定查询 tab 已输入的 SQL 文本(无记录返回空串)
  String queryTextFor(String tabKey) => _queryTexts[tabKey] ?? '';

  /// 写入查询 tab 的 SQL 文本(不通知:输入高频,无 widget 订阅它重建)
  void updateQueryText(String tabKey, String text) {
    _queryTexts[tabKey] = text;
  }

  // ── 例程设计页 SQL 文本 / 注释(按 OpenTab.key 保存,切换标签不丢失) ──

  final Map<String, String> _routineTexts = {};
  final Map<String, String> _routineComments = {};

  /// 获取指定例程设计 tab 已编辑的 SQL 文本(无记录返回空串)
  String routineTextFor(String tabKey) => _routineTexts[tabKey] ?? '';

  /// 写入例程设计 tab 的 SQL 文本(不通知:输入高频)
  void updateRoutineText(String tabKey, String text) {
    _routineTexts[tabKey] = text;
  }

  /// 获取指定例程设计 tab 的注释(无记录返回空串)
  String routineCommentFor(String tabKey) => _routineComments[tabKey] ?? '';

  /// 写入例程设计 tab 的注释(不通知)
  void updateRoutineComment(String tabKey, String text) {
    _routineComments[tabKey] = text;
  }

  // ── 每 tab SQL 执行历史 ──────────────────────────────────────

  /// tab key → 该 tab 最近执行的 SQL 列表(最多 100 条,最新在末尾)
  final Map<String, List<String>> _tabSqlHistory = {};

  /// 为指定 tab 记录一条 SQL 执行语句,超过 100 条时移除最早的
  void recordSql(String tabKey, String sql) {
    final list = _tabSqlHistory.putIfAbsent(tabKey, () => []);
    list.add(sql);
    if (list.length > 100) {
      list.removeRange(0, list.length - 100);
    }
    notifyListeners();
  }

  /// 获取指定 tab 的 SQL 历史(最新在末尾),无记录返回空列表
  List<String> sqlHistoryFor(String tabKey) =>
      _tabSqlHistory[tabKey] ?? const [];

  /// 清除某 tab 的 SQL 历史(关闭标签时调用)
  void clearSqlHistory(String tabKey) => _tabSqlHistory.remove(tabKey);

  // ── 表数据页分页状态 ─────────────────────────────────

  /// tab key → 该表数据页当前的分页 / 记录位置状态
  final Map<String, TablePageStatus> _tableStatus = {};

  /// 表数据页上报当前分页状态(状态栏据此显示记录位置)
  void updateTableStatus(String tabKey, TablePageStatus status) {
    if (_tableStatus[tabKey] == status) return;
    _tableStatus[tabKey] = status;
    notifyListeners();
  }

  /// 获取指定表 tab 的分页状态(未上报过返回 null)
  TablePageStatus? tableStatusFor(String tabKey) => _tableStatus[tabKey];

  bool get ctrlPressed => HardwareKeyboard.instance.isControlPressed;

  bool get shiftPressed => HardwareKeyboard.instance.isShiftPressed;

  OpenTab? get activeTabModel {
    for (final tab in tabs) {
      if (tab.title == activeTab) return tab;
    }
    return null;
  }

  /// 按连接名查连接配置(查询编辑页执行 SQL 时用)
  ConnectionInfo? connectionByName(String? name) =>
      name == null ? null : _connections.where((c) => c.name == name).firstOrNull;

  /// 双击表:打开该表的数据浏览页(前 100 行)。
  /// [connection] + [database] + [schema] 指明表所属上下文,
  /// 据此走真实驱动查询([schema] 仅 PostgreSQL 等有模式层的类型使用)
  void openTable(
    String name, {
    required String connection,
    required String database,
    String? schema,
    bool select = true,
  }) {
    if (select) selectTable(name);

    final tab = OpenTab(
      TabType.table,
      name,
      null,
      connection,
      database,
      schema,
    );

    // 按业务键去重:不同库/模式的同名表各自成 tab,但激活标题相同
    final exists =
        tabs.any((t) => t.type == TabType.table && t.key == tab.key);
    activeTab = name;
    if (!exists) {
      tabs.add(tab);
    }

    notifyListeners();
  }

  /// 设计表:打开该表的结构设计页(以编辑模式回填已有结构)。
  /// [connection] + [database] + [schema] 指明表所属上下文,据此走真实驱动查询。
  void designTable(
    String name, {
    required String connection,
    required String database,
    String? schema,
  }) {
    selectTable(name);

    final tab = OpenTab(
      TabType.design,
      '$name$kTabDesignTitleSuffix',
      null,
      connection,
      database,
      schema,
    );

    final exists =
        tabs.any((t) => t.type == TabType.design && t.key == tab.key);
    activeTab = '$name$kTabDesignTitleSuffix';
    if (!exists) {
      tabs.add(tab);
    }

    notifyListeners();
  }

  /// 新建查询:打开查询编辑页,自动关联当前对象浏览上下文的连接与库
  /// (未选择数据库时为无关联查询,运行前会提示先选库)
  void newQuery() {
    _queryCount++;
    final title = '无标题-查询 $_queryCount';
    tabs.add(OpenTab(
      TabType.query,
      title,
      null,
      objectConnection,
      objectDatabase,
      objectSchema,
    ));
    activeTab = title;
    notifyListeners();
  }

  // ── 命令列界面 ──────────────────────────────────────────────

  /// 每个命令列会话一次取回的最大行数(交给驱动下推 LIMIT,不拉整表)
  static const int cliRowLimit = 1000;

  /// 标签标题(= 会话键)→ 命令列会话。输出与历史存在这里而不是页面 State:
  /// 切换标签会销毁页面(见 main_page 的按类型分发),存这里才不会丢日志。
  final Map<String, CliConsole> _cliConsoles = {};

  /// 命令列标签的标题(内部身份:连接名|库名 + 不参与翻译的令牌后缀)
  static String cliTabTitle(String connection, String database) =>
      '$connection|$database$kTabCliTitleSuffix';

  /// 取(或首次创建)某连接 + 库的命令列会话
  CliConsole cliConsoleFor(String connection, String database) {
    return _cliConsoles[cliTabTitle(connection, database)] ??= CliConsole(
      database: database,
      typeId: connectionByName(connection)?.typeId ?? '',
      rowLimit: cliRowLimit,
      executor: (sql) async {
        final conn = connectionByName(connection);
        if (conn == null) throw StateError('连接「$connection」已不存在');
        if (!hasDriver(conn)) {
          throw UnsupportedError('暂不支持 ${conn.typeId} 类型的连接');
        }
        return connectionManager.runQuery(conn, sql,
            database: database, limit: cliRowLimit);
      },
    );
  }

  /// 打开命令列界面:每个「连接 + 库」一个标签,已存在则直接激活。
  /// 会话上下文即该库,提示符与切库都由 [CliConsole] 负责。
  void openCommandLine({
    required String connection,
    required String database,
  }) {
    final title = cliTabTitle(connection, database);
    if (!tabs.any((tab) => tab.title == title)) {
      tabs.add(OpenTab(TabType.commandLine, title, null, connection, database));
    }
    setObjectContext(connection, database);
    activeTab = title;
    notifyListeners();
  }

  /// 新建表设计器标签计数(保证多个设计器标签标题唯一)
  int _tableDesignCount = 0;

  /// 新建表:在当前库 / 模式上下文打开表设计器标签页。
  /// 设计器负责采集 字段 / 索引 / 外键 / 唯一键 / 检查 / 触发器 / 注释 等,
  /// 点「保存」调用 [createTableDesign] 执行生成的全套 DDL。
  void newTableDesigner({
    required String connection,
    required String database,
    String? schema,
  }) {
    _tableDesignCount++;
    final title = '新建表' + (_tableDesignCount > 1 ? ' $_tableDesignCount' : '');
    tabs.add(OpenTab(
      TabType.createTable,
      title,
      null,
      connection,
      database,
      schema,
    ));
    activeTab = title;
    notifyListeners();
  }

  // ── 查询管理面板(已保存查询) ──────────────────────

  /// 查询 tab 的编辑文本 / SQL 历史 key(与 QueryPage._tabKeyOf 同一
  /// 格式;注意不同于 [OpenTab.key] —— 后者末尾多一个 routineCategory 段)
  static String queryTabKey(
          String? connection, String? database, String title) =>
      'query|$connection|$database|$title';

  /// 某连接 / 库下的已保存查询(对象面板「查询」分类的数据源)
  List<SavedQuery> savedQueriesOf(String? connection, String? database) => [
        for (final q in _savedQueries)
          if (q.connection == connection && q.database == database) q,
      ];

  /// 按名称取某连接 / 库下的已保存查询(无则 null)
  SavedQuery? savedQueryOf(String name,
      {String? connection, String? database}) {
    for (final q in _savedQueries) {
      if (q.key == '$connection|$database|$name') return q;
    }
    return null;
  }

  /// 保存 / 更新一条查询(同 连接|库|名称 覆盖)并落盘。
  /// [tabTitle] 为发起保存的查询 tab 标题,与 [name] 不同时同步重命名
  /// 该 tab(迁移编辑文本与 SQL 历史,标题变为查询名)。
  void saveQuery({
    required String name,
    required String sql,
    required String? connection,
    required String? database,
    String? tabTitle,
  }) {
    final query = SavedQuery(
      name: name,
      connection: connection,
      database: database,
      sql: sql,
    );
    final index = _savedQueries.indexWhere((q) => q.key == query.key);
    if (index >= 0) {
      _savedQueries[index] = query;
    } else {
      _savedQueries.add(query);
    }
    if (tabTitle != null && tabTitle != name) {
      _renameQueryTab(tabTitle, name, connection, database);
    }
    notifyListeners();
    _persistQueries();
  }

  /// 删除已保存查询(打开着的查询 tab 不受影响)
  void deleteSavedQuery(String name, {String? connection, String? database}) {
    _savedQueries.removeWhere((q) => q.key == '$connection|$database|$name');
    notifyListeners();
    _persistQueries();
  }

  /// 打开已保存查询:已有同名同上下文的查询 tab 则直接激活,
  /// 否则新建 tab 并预填该查询的 SQL 文本
  void openSavedQuery(SavedQuery query) {
    final exists = tabs.any((t) =>
        t.type == TabType.query &&
        t.title == query.name &&
        t.connection == query.connection &&
        t.database == query.database);
    if (!exists) {
      tabs.add(OpenTab(
        TabType.query,
        query.name,
        null,
        query.connection,
        query.database,
      ));
      _queryTexts[
          queryTabKey(query.connection, query.database, query.name)] = query.sql;
    }
    activeTab = query.name;
    notifyListeners();
  }

  /// 查询 tab 重命名(保存命名后调用):迁移编辑文本与 SQL 历史
  void _renameQueryTab(
      String oldTitle, String newTitle, String? connection, String? database) {
    for (var i = 0; i < tabs.length; i++) {
      final tab = tabs[i];
      if (tab.type == TabType.query &&
          tab.title == oldTitle &&
          tab.connection == connection &&
          tab.database == database) {
        final oldKey = queryTabKey(connection, database, oldTitle);
        final newKey = queryTabKey(connection, database, newTitle);
        final text = _queryTexts.remove(oldKey);
        if (text != null) _queryTexts[newKey] = text;
        final history = _tabSqlHistory.remove(oldKey);
        if (history != null) _tabSqlHistory[newKey] = history;
        tabs[i] = OpenTab(TabType.query, newTitle, null, connection, database);
        if (activeTab == oldTitle) activeTab = newTitle;
        break;
      }
    }
  }

  /// 标签重命名(「设计表」保存后表被重命名时同步标题)。
  /// 标题在 [OpenTab] 中是 final,故原位重建实例;激活项同步。
  void renameTab(String oldTitle, String newTitle) {
    if (oldTitle == newTitle) return;
    for (var i = 0; i < tabs.length; i++) {
      final tab = tabs[i];
      if (tab.title != oldTitle) continue;
      tabs[i] = OpenTab(
        tab.type,
        newTitle,
        tab.typeId,
        tab.connection,
        tab.database,
        tab.schema,
        tab.routineCategory,
        tab.routineParams,
        tab.routineComment,
        tab.routineOptions,
        tab.routineReturnType,
      );
      if (activeTab == oldTitle) activeTab = newTitle;
      notifyListeners();
      return;
    }
  }

  void closeTab(String title) {
    final closing = tabs.where((tab) => tab.title == title).toList();
    tabs.removeWhere((tab) => tab.title == title);
    // 清理被关闭标签的 SQL 历史、分页状态与查询/例程文本、注释。
    // 查询编辑页的文本 / 历史按 [queryTabKey] 存(不含 [OpenTab.key]
    // 末尾的 routineCategory 段),需换算后再清,否则会一直残留
    for (final tab in closing) {
      if (tab.type == TabType.query) {
        final qk = queryTabKey(tab.connection, tab.database, tab.title);
        clearSqlHistory(qk);
        _queryTexts.remove(qk);
      } else {
        clearSqlHistory(tab.key);
        _queryTexts.remove(tab.key);
        _routineTexts.remove(tab.key);
        _routineComments.remove(tab.key);
      }
      _tableStatus.remove(tab.key);
    }
    _releaseCliConsoles(closing);
    if (activeTab == title) activeTab = objectsTabKey;
    notifyListeners();
  }

  /// 释放被关闭标签的命令列会话(输出缓冲 + 历史随标签一起销毁)
  void _releaseCliConsoles(Iterable<OpenTab> closing) {
    for (final tab in closing) {
      if (tab.type == TabType.commandLine) {
        _cliConsoles.remove(tab.title)?.dispose();
      }
    }
  }

  /// 关闭除 [title] 外的所有标签,并激活该标签
  void closeOtherTabs(String title) {
    final closing = tabs.where((tab) => tab.title != title).toList();
    tabs.removeWhere((tab) => tab.title != title);
    _releaseCliConsoles(closing);
    activeTab = title;
    notifyListeners();
  }

  /// 关闭 [title] 右侧的所有标签;活动标签被关闭时改为激活 [title]
  void closeTabsToRight(String title) {
    final index = tabs.indexWhere((tab) => tab.title == title);
    if (index < 0) return;
    final closingTabs = tabs.sublist(index + 1);
    final closing = closingTabs.map((tab) => tab.title).toSet();
    tabs.removeRange(index + 1, tabs.length);
    _releaseCliConsoles(closingTabs);
    if (closing.contains(activeTab)) activeTab = title;
    notifyListeners();
  }

  /// 关闭全部标签,回到"对象"页
  void closeAllTabs() {
    final closing = List<OpenTab>.of(tabs);
    tabs.clear();
    _releaseCliConsoles(closing);
    activeTab = objectsTabKey;
    notifyListeners();
  }

  void activateTab(String title) {
    activeTab = title;
    notifyListeners();
  }

  /// 切换左侧面板显示/隐藏
  void toggleLeftPanel() {
    leftPanelVisible = !leftPanelVisible;
    notifyListeners();
  }

  /// 切换右侧面板显示/隐藏
  void toggleRightPanel() {
    rightPanelVisible = !rightPanelVisible;
    notifyListeners();
  }

  /// 单击选中表;按住 Ctrl 时切换多选
  /// 通过按项通知器精确重建,仅触发实际变化的表项
  void selectTable(String name) {
    if (ctrlPressed) {
      final selected = selectedTables.contains(name);

      if (selected) {
        selectedTables.remove(name);

        itemNotifierFor(name).value = false;
      } else {
        selectedTables.add(name);

        itemNotifierFor(name).value = true;
        // 详情面板跟随当前选中的表
        detailSelection.value = SelectedNode(
          NodeKind.table,
          name,
          connection: objectConnection,
          database: objectDatabase,
          schema: objectSchema,
        );
      }
    } else {
      for (final old in selectedTables) {
        itemNotifierFor(old).value = false;
      }

      selectedTables
        ..clear()
        ..add(name);

      itemNotifierFor(name).value = true;
      // 详情面板跟随当前选中的表
      detailSelection.value = SelectedNode(
        NodeKind.table,
        name,
        connection: objectConnection,
        database: objectDatabase,
        schema: objectSchema,
      );
    }
    // 同步选中集合,供对象面板工具栏(打开/设计按钮可用性)与状态栏订阅
    selectionNotifier.value = Set<String>.from(selectedTables);
  }

  /// Shift 范围多选:选中 [from, to] 在当前对象页表列表顺序区间内的所有表。
  /// additive 为 true(Ctrl+Shift)时保留原有选中,否则先清空再选区间。
  /// 通过按项通知器精确重建,仅触发实际变化的表项。
  void selectRange(String from, String to, {bool additive = false}) {
    final list = objectTables;
    final i = list.indexOf(from);
    final j = list.indexOf(to);
    if (i < 0 || j < 0) return;
    final a = i < j ? i : j;
    final b = i < j ? j : i;

    if (!additive) {
      for (final old in selectedTables) {
        itemNotifierFor(old).value = false;
      }
      selectedTables.clear();
    }

    for (var k = a; k <= b; k++) {
      final name = list[k];
      selectedTables.add(name);
      itemNotifierFor(name).value = true;
    }

    detailSelection.value = SelectedNode(NodeKind.table, to);
    selectionNotifier.value = Set<String>.from(selectedTables);
  }

  /// 批量选中(框选):把选中集合整体替换为 [names];[additive] 为 true 时并入
  /// 原有选中(Ctrl 框选)。不改动详情面板跟随对象——多选本身没有"当前项"语义。
  /// 通过按项通知器精确重建,仅触发实际变化的表项。
  void selectMany(Iterable<String> names, {bool additive = false}) {
    final next = additive ? {...selectedTables, ...names} : names.toSet();
    if (next.length == selectedTables.length &&
        next.containsAll(selectedTables)) {
      // 拖动过程中选框反复覆盖同一批表项:命中集合未变则整体跳过
      return;
    }

    for (final old in selectedTables) {
      if (!next.contains(old)) itemNotifierFor(old).value = false;
    }
    for (final name in next) {
      itemNotifierFor(name).value = true;
    }

    selectedTables
      ..clear()
      ..addAll(next);
    selectionNotifier.value = Set<String>.from(selectedTables);
  }

  // ── 对象 DDL 操作(工具栏「新建 / 删除 / 设计」使用) ──

  /// 按数据库类型选择标识符引用方式(避免表名与关键字冲突)
  String _ident(String typeId, String name) {
    switch (typeId) {
      case 'postgresql':
      case 'sqlite':
        return '"${name.replaceAll('"', '""')}"';
      case 'sqlserver':
      case 'access':
        return '[${name.replaceAll(']', ']]')}]';
      default: // mysql / mariadb 等
        return '`${name.replaceAll('`', '``')}`';
    }
  }

  /// 带模式限定的标识符:"schema"."object" / [schema].[object]。
  /// 仅 PostgreSQL / SQL Server 等有独立模式层的类型生效;
  /// [schema] 为空或该类型无模式层时退化为普通标识符
  String _qualifiedIdent(String typeId, String? schema, String name) {
    final ident = _ident(typeId, name);
    if (schema == null || schema.isEmpty) return ident;
    switch (typeId) {
      case 'postgresql':
      case 'sqlite':
      case 'sqlserver':
      case 'access':
        return '${_ident(typeId, schema)}.$ident';
      default: // mysql / mariadb:Database 即 Schema,不做二级限定
        return ident;
    }
  }

  /// 删除语句关键字:表 / 视图 / 实体化视图 / 函数 / 过程
  static String _dropKeyword(ObjectCategory category) => switch (category) {
        ObjectCategory.table => 'TABLE',
        ObjectCategory.view => 'VIEW',
        ObjectCategory.materializedView => 'MATERIALIZED VIEW',
        ObjectCategory.function => 'FUNCTION',
        ObjectCategory.procedure => 'PROCEDURE',
        _ => 'TABLE',
      };

  /// 删除指定分类下的多个对象(表 / 视图 / 函数 / 过程)。
  /// [schema] 非空时按模式限定对象名(PostgreSQL / SQL Server 等)。
  /// 返回删除失败的对象名列表(空列表表示全部成功)。
  Future<List<String>> dropObjects(
    ObjectCategory category,
    List<String> names, {
    required String connection,
    required String database,
    String? schema,
  }) async {
    final conn = connectionByName(connection);
    if (conn == null) return List<String>.from(names);
    final kw = _dropKeyword(category);
    final failed = <String>[];
    for (final name in names) {
      final ident = _qualifiedIdent(conn.typeId, schema, name);
      final sql = 'DROP $kw IF EXISTS $ident';
      try {
        await connectionManager.runQuery(
          conn,
          sql,
          database: database,
          limit: 1,
        );
      } catch (e) {
        failed.add(name);
      }
    }
    if (failed.length < names.length) {
      await connectionManager.refreshDatabase(conn, database, schema: schema);
    }
    return failed;
  }

  /// 新建数据库:按连接类型生成 CREATE DATABASE 语句(字符集 / 编码 /
  /// 模板 / 排序规则 / 表空间 / 连接限制等选项见 [CreateDatabaseOptions])并执行,
  /// 成功后刷新连接下的库列表。名称空 / 重名 / 无权限由服务端报错。
  ///
  /// 建库后的附加语句(PostgreSQL 的扩展 / 库注释,见
  /// [buildCreateDatabasePostSql])在**新库**上下文中逐条执行。
  Future<DdlOutcome> createDatabase(
    ConnectionInfo conn,
    CreateDatabaseOptions options,
  ) async {
    final dbName = options.name.trim();
    if (dbName.isEmpty) return DdlOutcome(false, '数据库名不能为空');
    final sql = buildCreateDatabaseSql(conn.typeId, options);
    final post = buildCreateDatabasePostSql(conn.typeId, options);
    try {
      // PostgreSQL 的 CREATE DATABASE 不允许在事务块中执行,所以每条语句
      // 单独 runQuery(驱动不会把它们并成一次多语句调用)。
      await connectionManager.runQuery(conn, sql, limit: 1);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
    for (final extra in post) {
      try {
        await connectionManager.runQuery(conn, extra,
            database: dbName, limit: 1);
      } catch (e) {
        // 库已经建出来了,不能整体报「创建失败」——说明清楚并刷新列表
        await connectionManager.refreshDatabases(conn);
        return DdlOutcome(
          false,
          '数据库「$dbName」已创建,但附加语句执行失败:\n$e',
        );
      }
    }
    await connectionManager.refreshDatabases(conn);
    return DdlOutcome(true);
  }

  /// 加载「新建数据库」对话框的下拉候选(PostgreSQL 的所有者 / 模板 /
  /// 表空间 / 扩展)。
  ///
  /// 连接未打开、非 PostgreSQL 或任一查询失败时回退到内置兜底常量
  /// (见 [DatabaseCreateCatalog.fallback]),对话框始终可用。
  Future<DatabaseCreateCatalog> loadCreateDatabaseCatalog(
    ConnectionInfo conn,
  ) async {
    final fallback = DatabaseCreateCatalog.fallback(username: conn.username);
    if (conn.typeId != 'postgresql') return fallback;
    if (!connectionManager.isConnected(conn.name)) return fallback;
    try {
      final ownersRow =
          await connectionManager.runQuery(conn, kPgOwnerCatalogSql, limit: 500);
      final templatesRow = await connectionManager.runQuery(
          conn, kPgTemplateCatalogSql,
          limit: 500);
      final spacesRow = await connectionManager.runQuery(
          conn, kPgTablespaceCatalogSql,
          limit: 500);
      final extsRow = await connectionManager.runQuery(
          conn, kPgExtensionCatalogSql,
          limit: 2000);
      final owners = firstColumnOf(ownersRow.rows);
      final templates = firstColumnOf(templatesRow.rows);
      final tablespaces = firstColumnOf(spacesRow.rows);
      final extensions = parseExtensionRows(extsRow.rows);
      return DatabaseCreateCatalog(
        owners: owners.isEmpty ? fallback.owners : owners,
        templates: templates.isEmpty ? fallback.templates : templates,
        tablespaces: tablespaces.isEmpty ? fallback.tablespaces : tablespaces,
        extensions: extensions.isEmpty ? fallback.extensions : extensions,
        loadedFromServer: true,
      );
    } catch (_) {
      // 权限不足 / 连接中断:静默回退,不打断用户建库
      return fallback;
    }
  }

  /// 读取「编辑数据库」对话框的一次性快照:库属性 + 注释、所有者 / 表空间候选,
  /// 以及该库的可用 / 已安装扩展清单。
  ///
  /// 三段查询分开兜底,因为失败的影响面不同:
  /// - 属性读不到 → [DatabaseEditSnapshot.loadedFromServer] = false,表单基准不可信,
  ///   对话框据此禁用「确定」(否则会拿错现状生成出一堆无谓的 `ALTER DATABASE`);
  /// - 候选读不到 → 用现状值 + 内置常量补齐下拉,不阻塞编辑;
  /// - 扩展读不到 → [DatabaseEditSnapshot.extensionsLoaded] = false,扩展页显示
  ///   说明而非空列表(空列表会被误读成「该库没装任何扩展」)。
  ///
  /// 属性查的是集群共享目录(`pg_database`),不带 `database:`;扩展视图是每库
  /// 一份,必须落在目标库上下文里查。
  Future<DatabaseEditSnapshot> loadDatabaseEditSnapshot(
    ConnectionInfo conn,
    String database,
  ) async {
    var props =
        PgDatabaseProps.unknown(database, owner: conn.username.trim());
    var loaded = false;
    try {
      // PG 18 把 pg_database.datencoding 改名为 encoding,先问版本再拼查询
      final version = parseServerVersion((await connectionManager
              .runQuery(conn, kPgServerVersionSql, limit: 1))
          .rows);
      final rows = (await connectionManager.runQuery(
              conn,
              pgDatabasePropsSql(database,
                  pg18Plus: (version ?? 0) >=
                      kPgEncodingColumnRenamedVersion),
              limit: 2))
          .rows;
      final parsed = parseDatabasePropsRow(rows, database);
      if (parsed != null) {
        props = parsed;
        loaded = true;
      }
    } catch (_) {
      // 库不存在 / 无权限:保持 unknown,由对话框禁用保存
    }

    List<String> owners = const [];
    List<String> tablespaces = const [];
    try {
      owners = firstColumnOf((await connectionManager
              .runQuery(conn, kPgOwnerCatalogSql, limit: 500))
          .rows);
      tablespaces = firstColumnOf((await connectionManager
              .runQuery(conn, kPgTablespaceCatalogSql, limit: 500))
          .rows);
    } catch (_) {
      // 候选读不到时下面按现状值 + 内置常量补齐
    }
    // 当前值一定得能在下拉里选回来(权限受限的实例上候选可能不含它)
    List<String> withCurrent(List<String> items, String current,
            List<String> fallback) =>
        [
          if (current.isNotEmpty) current,
          ...items.isEmpty ? fallback : items,
        ].toSet().toList()
          ..sort();

    final ext = await _loadDatabaseExtensions(conn, database);

    return DatabaseEditSnapshot(
      props: props,
      owners: withCurrent(owners, props.owner, const []),
      tablespaces:
          withCurrent(tablespaces, props.tablespace, kPgTablespaceFallback),
      availableExtensions: ext.$1,
      installedExtensions: ext.$2,
      loadedFromServer: loaded,
      extensionsLoaded: ext.$3,
    );
  }

  /// 目标库的扩展清单:返回 (未安装的可用扩展, 已安装扩展, 是否读取成功)。
  ///
  /// 两条查询都带 `database:`,把会话定位到目标库(PG 家族的 `useDatabase`
  /// 实现为断开重连,与 [createDatabase] 建库后语句同一套路)。
  Future<(List<PgExtensionInfo>, List<PgExtensionInfo>, bool)>
      _loadDatabaseExtensions(
    ConnectionInfo conn,
    String database,
  ) async {
    try {
      final available = parseExtensionVersionRows((await connectionManager
              .runQuery(conn, kPgAvailableExtensionsSql,
                  database: database, limit: 2000))
          .rows);
      final installed = parseExtensionVersionRows((await connectionManager
              .runQuery(conn, kPgInstalledExtensionsSql,
                  database: database, limit: 2000))
          .rows);
      return (available, installed, true);
    } catch (_) {
      // 低版本服务端 / 权限不足:列表留空,由对话框给出说明
      return (
        const <PgExtensionInfo>[],
        const <PgExtensionInfo>[],
        false,
      );
    }
  }

  /// 执行「编辑数据库」的差异语句,语句由
  /// [buildEditDatabaseStatements] 依据快照现状与表单值算出。
  ///
  /// 按执行上下文分流:
  /// - 库级语句(`ALTER DATABASE` / `COMMENT ON DATABASE`)按库名寻址,**不带**
  ///   `database:` —— PG 家族切换上下文等于断开重连,而 `SET TABLESPACE` 恰恰
  ///   要求目标库没有活动连接,切过去只会自己挡住自己;
  /// - 扩展语句必须落在目标库,带 `database:`。
  ///
  /// 逐条执行、遇错即停:DDL 之间无事务保证,回滚不在职责范围内,把失败的那条
  /// 原文与服务端错误一起返回,由对话框展示。
  Future<DdlOutcome> applyDatabaseEdits(
    ConnectionInfo conn,
    DatabaseEditSnapshot snapshot,
    DatabaseEditForm form,
  ) async {
    final database = snapshot.props.name;
    final typeId = conn.typeId;
    final batches = <(String, String?)>[
      for (final s in buildDatabaseLevelEditStatements(
          typeId: typeId, current: snapshot.props, form: form))
        (s, null),
      for (final s in buildExtensionEditStatements(typeId, form)) (s, database),
    ];
    for (final (sql, db) in batches) {
      try {
        await connectionManager.runQuery(conn, sql, database: db, limit: 1);
      } catch (e) {
        return DdlOutcome(false, '$sql;\n\n$e');
      }
    }
    return DdlOutcome(true);
  }

  /// 新建模式(仅支持模式层的类型:PostgreSQL / SQL Server):
  /// 在指定库下执行 CREATE SCHEMA 并刷新模式列表。
  Future<DdlOutcome> createSchema(
    ConnectionInfo conn,
    String database,
    String schema,
  ) async {
    final name = schema.trim();
    if (name.isEmpty) return DdlOutcome(false, '模式名不能为空');
    final sql = 'CREATE SCHEMA ${_ident(conn.typeId, name)}';
    try {
      // 带 database 上下文执行:PG / SQL Server 的 CREATE SCHEMA 作用于当前库
      await connectionManager.runQuery(conn, sql,
          database: database, limit: 1);
      await connectionManager.refreshSchemas(conn, database);
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  /// 删除模式(仅支持模式层的类型:PostgreSQL / SQL Server):
  /// 在指定库下执行 DROP SCHEMA 并刷新模式列表。
  /// 成功后清理该模式的对象元数据缓存;若该模式正被中部对象页浏览,重置到库级。
  Future<DdlOutcome> dropSchema(
    ConnectionInfo conn,
    String database,
    String schema,
  ) async {
    final sql = 'DROP SCHEMA ${_ident(conn.typeId, schema)}';
    try {
      await connectionManager.runQuery(conn, sql,
          database: database, limit: 1);
      connectionManager.clearSchemaState(conn.name, database, schema);
      await connectionManager.refreshSchemas(conn, database);
      // 正浏览该模式:重置到库级(默认模式)上下文并确保对象列表加载
      if (objectConnection == conn.name &&
          objectDatabase == database &&
          objectSchema == schema) {
        setObjectContext(conn.name, database);
        await connectionManager.expandDatabase(conn, database);
      }
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  /// 重命名模式(仅 PostgreSQL 家族支持 ALTER SCHEMA ... RENAME TO;
  /// SQL Server 无该语法,「编辑模式」菜单项按类型隐藏)。
  /// 成功后清理旧模式缓存并刷新模式列表;若正浏览旧模式,上下文迁移到新模式。
  Future<DdlOutcome> renameSchema(
    ConnectionInfo conn,
    String database,
    String oldName,
    String newName,
  ) async {
    final from = _ident(conn.typeId, oldName);
    final to = _ident(conn.typeId, newName);
    final sql = 'ALTER SCHEMA $from RENAME TO $to';
    try {
      await connectionManager.runQuery(conn, sql,
          database: database, limit: 1);
      connectionManager.clearSchemaState(conn.name, database, oldName);
      await connectionManager.refreshSchemas(conn, database);
      // 正浏览旧模式:上下文迁移到新模式名并触发对象列表加载
      if (objectConnection == conn.name &&
          objectDatabase == database &&
          objectSchema == oldName) {
        objectSchema = newName;
        _clearTableSelection();
        notifyListeners();
        await connectionManager.expandSchema(conn, database, newName);
      }
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  /// 删除数据库:执行 DROP DATABASE 并刷新库列表。
  /// 成功后清理该库的元数据缓存;若该库正被中部对象页浏览,同步清空上下文。
  Future<DdlOutcome> dropDatabase(ConnectionInfo conn, String database) async {
    final sql = 'DROP DATABASE ${_ident(conn.typeId, database)}';
    try {
      await connectionManager.runQuery(conn, sql, limit: 1);
      connectionManager.clearDatabaseState(conn.name, database);
      await connectionManager.refreshDatabases(conn);
      if (objectConnection == conn.name && objectDatabase == database) {
        objectConnection = null;
        objectDatabase = null;
        objectSchema = null;
        _clearTableSelection();
      }
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  /// 转储数据库结构(仅结构):生成 表 / 视图 / 函数 的 CREATE DDL 文本。
  /// [schema] 为空转储库级(默认模式)对象;对象级失败跳过,不中断整体。
  /// 返回 null 表示连库列表都加载失败(如连接已断开)。
  Future<String?> dumpDatabaseStructure(
    ConnectionInfo conn,
    String database, {
    String? schema,
  }) async {
    // 先确保库列表就绪 / 会话切到目标库
    try {
      await connectionManager.expandDatabase(conn, database);
    } catch (e) {
      return null;
    }
    final state =
        connectionManager.tableStateOf(conn.name, database, schema: schema);

    final buf = StringBuffer();
    buf.writeln('-- ============================================');
    buf.writeln('-- daro 结构转储(仅结构,不含数据)');
    buf.writeln('-- 连接: ${conn.name}');
    buf.writeln('-- 数据库: $database');
    buf.writeln('-- 生成时间: ${DateTime.now().toIso8601String()}');
    buf.writeln('-- ============================================');
    buf.writeln();

    // 表
    for (final table in state.tables ?? const <String>[]) {
      try {
        final cols = await connectionManager.describeTable(
            conn, database, table,
            schema: schema);
        if (cols.isEmpty) continue;
        buf.writeln(_createTableDdl(conn.typeId, table, cols, schema: schema));
        buf.writeln();
      } catch (_) {
        // 单表结构读取失败:跳过,不中断整库转储
      }
    }
    // 视图 / 函数 / 过程:直接落驱动返回的 CREATE 定义文本
    for (final kind in [
      (category: ObjectCategory.view, items: state.views),
      (category: ObjectCategory.function, items: state.functions),
      (category: ObjectCategory.procedure, items: state.procedures),
    ]) {
      for (final name in kind.items ?? const <String>[]) {
        try {
          final def = await connectionManager.getDefinition(
              conn, database, name,
              _definitionKind(kind.category),
              schema: schema);
          if (def == null || def.trim().isEmpty) continue;
          buf.writeln(def.trim().endsWith(';') ? def.trim() : '${def.trim()};');
          buf.writeln();
        } catch (_) {
          // 单对象定义读取失败:跳过
        }
      }
    }
    return buf.toString();
  }

  /// 按列定义生成 CREATE TABLE 语句(标识符引用 / 默认值规则与建表一致)。
  /// 单列主键内联 PRIMARY KEY;多列主键改为表级约束。
  String _createTableDdl(
    String typeId,
    String table,
    List<ColumnDef> cols, {
    String? schema,
  }) {
    final pkCols = cols
        .where((c) => c.primaryKey)
        .map((c) => _ident(typeId, c.name))
        .toList();
    final lines = <String>[];
    for (final c in cols) {
      final buf = StringBuffer('  ${_ident(typeId, c.name)} ${c.type}');
      if (!c.nullable) buf.write(' NOT NULL');
      if (c.primaryKey && pkCols.length == 1) buf.write(' PRIMARY KEY');
      if (c.defaultValue != null &&
          c.defaultValue!.isNotEmpty &&
          c.defaultValue != 'NULL') {
        buf.write(' DEFAULT ${c.defaultValue}');
      }
      if (c.comment.isNotEmpty) {
        switch (typeId) {
          case 'mysql':
          case 'mariadb':
            buf.write(" COMMENT '${c.comment.replaceAll("'", "''")}'");
        }
      }
      lines.add(buf.toString());
    }
    if (pkCols.length > 1) {
      lines.add('  PRIMARY KEY (${pkCols.join(', ')})');
    }
    final ident = _qualifiedIdent(typeId, schema, table);
    return 'CREATE TABLE $ident (\n${lines.join(',\n')}\n);';
  }

  /// 新建表:根据列定义生成 CREATE TABLE 并执行;成功返回 DdlOutcome(true)。
  /// [schema] 非空时在指定模式下建表(PostgreSQL / SQL Server 等)。
  Future<DdlOutcome> createTable({
    required String name,
    required List<TableColumnSpec> columns,
    required String connection,
    required String database,
    String? schema,
  }) async {
    final conn = connectionByName(connection);
    if (conn == null) return DdlOutcome(false, '连接 "$connection" 不存在');
    final tableName = name.trim();
    if (tableName.isEmpty) return DdlOutcome(false, '表名不能为空');
    if (columns.isEmpty) return DdlOutcome(false, '至少需要一列');

    final pkColumns = columns
        .where((c) => c.primaryKey && c.name.trim().isNotEmpty)
        .toList();
    final lines = <String>[];
    for (final c in columns) {
      final n = c.name.trim();
      if (n.isEmpty) return DdlOutcome(false, '存在未命名的列');
      final type = c.type.trim().isEmpty ? 'TEXT' : c.type.trim();
      var def = '  ${_ident(conn.typeId, n)} $type';
      if (!c.nullable) def += ' NOT NULL';
      // 多列主键改为表级约束,避免与内联 PRIMARY KEY 冲突
      if (c.primaryKey && pkColumns.length == 1) def += ' PRIMARY KEY';
      if (c.defaultValue.trim().isNotEmpty) {
        def += " DEFAULT ${c.defaultValue.trim()}";
      }
      lines.add(def);
    }
    if (pkColumns.length > 1) {
      final pk = pkColumns
          .map((c) => _ident(conn.typeId, c.name.trim()))
          .join(', ');
      lines.add('  PRIMARY KEY ($pk)');
    }
    final ident = _qualifiedIdent(conn.typeId, schema, tableName);
    final sql = 'CREATE TABLE $ident (\n${lines.join(',\n')}\n)';
    try {
      await connectionManager.runQuery(
        conn,
        sql,
        database: database,
        limit: 1,
      );
      await connectionManager.refreshDatabase(conn, database, schema: schema);
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  /// 顺序执行设计器生成的 DDL 语句集(新建表与「设计表」保存共用)。
  ///
  /// [useTx] 为 true 时整体包在 BEGIN / COMMIT 中(仅 PostgreSQL 家族支持
  /// 事务化 DDL),失败自动 ROLLBACK 并在结果里标记已回滚;其余类型 DDL
  /// 自带提交或不可回滚,失败时之前的语句已生效。成功后刷新对象列表。
  Future<DdlOutcome> _runDesignStatements(
    List<String> stmts, {
    required ConnectionInfo conn,
    required String database,
    required bool useTx,
    String? schema,
  }) async {
    var attempted = 0;
    try {
      if (useTx) {
        await connectionManager.runQuery(conn, 'BEGIN', database: database, limit: 1);
      }
      for (final sql in stmts) {
        attempted++;
        await connectionManager.runQuery(conn, sql, database: database, limit: 1);
      }
      if (useTx) {
        await connectionManager.runQuery(conn, 'COMMIT', database: database, limit: 1);
      }
    } catch (e) {
      var rolledBack = false;
      if (useTx) {
        // 回滚失败不覆盖原始错误
        try {
          await connectionManager.runQuery(conn, 'ROLLBACK',
              database: database, limit: 1);
          rolledBack = true;
        } catch (_) {}
      }
      return DdlOutcome(false, e.toString(), attempted, rolledBack);
    }
    await connectionManager.refreshDatabase(conn, database, schema: schema);
    return DdlOutcome(true);
  }

  /// 新建表设计器「保存」:将 [design] 生成的目标方言 DDL 逐条执行。
  ///
  /// PostgreSQL 家族在事务中执行(失败回滚,不留下半成品表);
  /// 含 CONCURRENTLY 索引时放弃事务(该语法不能在事务内执行),顺序执行。
  /// 其余类型(MySQL / SQLite / SQL Server 等)DDL 自带提交或不可回滚,
  /// 同样顺序执行。成功后刷新对象列表。
  Future<DdlOutcome> createTableDesign(
    DesignTable design, {
    required String connection,
    required String database,
    String? schema,
  }) async {
    final conn = connectionByName(connection);
    if (conn == null) return DdlOutcome(false, '连接 "$connection" 不存在');
    design.schema = schema;

    final check = DdlBuilder.validate(design, typeId: conn.typeId);
    if (!check.ok) return DdlOutcome(false, check.error ?? '设计数据不完整');
    // 当前方言表达不了的设计项(非 PG 连接上的规则 / 排除 / 索引并发等):
    // 执行前拦住并说明,不能让它们被 buildStatements 的方言门静默跳过
    final blocked = DdlBuilder.createUnsupported(design, conn.typeId);
    if (blocked != null) return DdlOutcome(false, blocked);

    final isPg = DdlBuilder.isPgLike(conn.typeId);
    final stmts = DdlBuilder.buildStatements(design, conn.typeId);
    final useTx =
        isPg && stmts.length > 1 && !design.indexes.any((i) => i.concurrent);
    return _runDesignStatements(
      stmts,
      conn: conn,
      database: database,
      useTx: useTx,
      schema: schema,
    );
  }

  /// 「设计表」保存:比较 [target](界面当前值)与 [original](打开时的反查快照),
  /// 只执行差异生成的 ALTER 语句;无变更时直接成功返回(不碰库)。
  ///
  /// 不能由 ALTER 表达的变更(触发器 / 规则 / 排除约束 / 存储参数 /
  /// PG 与 SQL Server 的列序调整等)由 [DdlBuilder.alterUnsupported] 阻断并
  /// 说明原因——宁可拒于执行前,也不静默丢弃用户的修改。
  Future<DdlOutcome> saveTableDesignEdit(
    DesignTable target,
    DesignTable original, {
    required String connection,
    required String database,
    String? schema,
  }) async {
    final conn = connectionByName(connection);
    if (conn == null) return DdlOutcome(false, '连接 "$connection" 不存在');
    target.schema = schema;

    final check = DdlBuilder.validate(target, typeId: conn.typeId);
    if (!check.ok) return DdlOutcome(false, check.error ?? '设计数据不完整');
    final blocked = DdlBuilder.alterUnsupported(target, original, conn.typeId);
    if (blocked != null) return DdlOutcome(false, blocked);

    final stmts = DdlBuilder.buildAlterStatements(target, original, conn.typeId);
    if (stmts.isEmpty) return DdlOutcome(true);
    return _runDesignStatements(
      stmts,
      conn: conn,
      database: database,
      // ALTER 集合不含 CONCURRENTLY 索引(索引新增走普通 CREATE INDEX),
      // PostgreSQL 可整体事务化
      useTx: DdlBuilder.isPgLike(conn.typeId),
      schema: schema,
    );
  }

  /// 执行任意 DDL(新建视图 / 函数等),成功后刷新对象列表。
  /// [schema] 指明 DDL 作用的目标模式(刷新对应模式的对象列表)
  Future<DdlOutcome> runDdl(
    String sql, {
    required String connection,
    required String database,
    String? schema,
  }) async {
    final conn = connectionByName(connection);
    if (conn == null) return DdlOutcome(false, '连接 "$connection" 不存在');
    try {
      await connectionManager.runQuery(
        conn,
        sql,
        database: database,
        limit: 1,
      );
      await connectionManager.refreshDatabase(conn, database, schema: schema);
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  /// 保存用户 / 角色(`UserSql.buildScript` 生成的语句逐条执行)。
  ///
  /// 与其它设计页一致:语句由 [UserSql] 统一拼装,"SQL 预览"看到的
  /// 就是这里执行的。任一条失败即中止并返回失败语句的下标(1 基),
  /// 让界面能指出"第几条挂了"——账号类 DDL 常有多条且互相依赖
  /// (先建号再授权),静默继续会留下半成品。
  Future<DdlOutcome> saveUser(
    UserSpec spec, {
    required String connection,
    required String database,
    String? schema,
  }) async {
    final conn = connectionByName(connection);
    if (conn == null) return DdlOutcome(false, '连接 "$connection" 不存在');
    final stmts = UserSql.buildScript(conn.typeId, spec);
    if (stmts.isEmpty) {
      return DdlOutcome(false, '没有可执行的语句(请先填写用户名)');
    }
    for (var i = 0; i < stmts.length; i++) {
      try {
        await connectionManager.runQuery(
          conn,
          stmts[i],
          database: database,
          limit: 1,
        );
      } catch (e) {
        return DdlOutcome(false, e.toString(), i + 1);
      }
    }
    // 账号列表来自 mysql.user / pg_roles 等系统表,刷新对象列表即可
    await connectionManager.refreshDatabase(conn, database, schema: schema);
    return DdlOutcome(true);
  }

  /// 删除用户 / 角色
  Future<DdlOutcome> dropUser(
    UserAccount account, {
    required String connection,
    required String database,
    String? schema,
    bool isRole = false,
  }) async {
    final conn = connectionByName(connection);
    if (conn == null) return DdlOutcome(false, '连接 "$connection" 不存在');
    if (!account.isValid) return DdlOutcome(false, '账号名不能为空');
    final stmts =
        UserSql.buildDrop(conn.typeId, account, isRole: isRole);
    if (stmts.isEmpty) {
      return DdlOutcome(false, '当前数据库类型不支持账号管理');
    }
    try {
      for (final sql in stmts) {
        await connectionManager.runQuery(conn, sql,
            database: database, limit: 1);
      }
      await connectionManager.refreshDatabase(conn, database, schema: schema);
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  /// 读取账号详情 / 权限(供设计页初始化)。
  /// 读不到时返回 null(界面退化为"按新建处理"或显示只读提示)。
  Future<UserSpec?> readUser(
    String account, {
    required String connection,
    required String database,
  }) async {
    final conn = connectionByName(connection);
    if (conn == null) return null;
    final driver = await connectionManager.driverForUser(conn);
    if (driver == null) return null;
    try {
      final spec = await driver.readUser(database, account);
      if (spec == null) return null;
      // 权限 / 成员关系与常规字段分次拉取,单项失败不影响主体
      spec.serverPrivileges.addAll(UserSql.aggregate(
        await driver.readUserPrivileges(database, account, serverLevel: true),
        serverLevel: true,
      ));
      spec.privileges.addAll(UserSql.aggregate(
        await driver.readUserPrivileges(database, account),
      ));
      try {
        spec.memberOf.addAll(await driver.readUserRoles(database, account));
        spec.members.addAll(await driver.readRoleMembers(database, account));
      } catch (_) {
        // 角色关系读取失败(权限不足 / 版本无此表)不影响主体信息
      }
      return spec;
    } catch (_) {
      return null;
    }
  }

  /// 角色候选(「成员属于」页的候选列表)
  Future<List<String>> listGrantableRoles({
    required String connection,
    required String database,
  }) async {
    final conn = connectionByName(connection);
    if (conn == null) return const [];
    final driver = await connectionManager.driverForUser(conn);
    if (driver == null) return const [];
    try {
      return await driver.listGrantableRoles(database);
    } catch (_) {
      return const [];
    }
  }

  /// 重写视图 / 函数:先 DROP(IF EXISTS)再执行新的 CREATE 语句。
  /// [schema] 非空时按模式限定对象名(PostgreSQL / SQL Server 等)
  Future<DdlOutcome> replaceRoutine(
    ObjectCategory category,
    String name, {
    required String createSql,
    required String connection,
    required String database,
    String? schema,
  }) async {
    final conn = connectionByName(connection);
    if (conn == null) return DdlOutcome(false, '连接 "$connection" 不存在');
    final kw = _dropKeyword(category);
    final ident = _qualifiedIdent(conn.typeId, schema, name);
    try {
      await connectionManager.runQuery(
        conn,
        'DROP $kw IF EXISTS $ident',
        database: database,
        limit: 1,
      );
      await connectionManager.runQuery(
        conn,
        createSql,
        database: database,
        limit: 1,
      );
      await connectionManager.refreshDatabase(conn, database, schema: schema);
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  /// 例程分类 → 驱动 getDefinition 的 kind 参数('view' / 'function' / 'procedure')
  static String _definitionKind(ObjectCategory category) => switch (category) {
        ObjectCategory.view => 'view',
        ObjectCategory.procedure => 'procedure',
        _ => 'function',
      };

  /// 获取视图 / 函数 / 过程的定义文本(供设计页展示),失败返回 null。
  /// [schema] 非空时限定该模式下的对象
  Future<String?> getObjectDefinition(
    ObjectCategory category,
    String name, {
    required String connection,
    required String database,
    String? schema,
  }) async {
    final conn = connectionByName(connection);
    if (conn == null) return null;
    // 实体化视图不走驱动 getDefinition(它只认 view/function/procedure):
    // 定义与存储属性直接由 PG 目录读出,再拼成完整的 CREATE 语句
    if (category == ObjectCategory.materializedView) {
      return _matViewDefinition(conn, database, name, schema: schema);
    }
    final kind = _definitionKind(category);
    try {
      return await connectionManager.getDefinition(
        conn,
        database,
        name,
        kind,
        schema: schema,
      );
    } catch (e) {
      return null;
    }
  }

  /// 实体化视图的完整 CREATE 文本(转储与设计页共用)。任一查询读不到即 null。
  Future<String?> _matViewDefinition(
    ConnectionInfo conn,
    String database,
    String name, {
    String? schema,
  }) async {
    try {
      final def = await connectionManager.runQuery(
        conn,
        MatViewSql.viewDefinitionSql(name, schema: schema),
        database: database,
        limit: 1,
      );
      final select = MatViewSql.firstCell(def.rows);
      if (select == null) return null;
      final props = await connectionManager.runQuery(
        conn,
        MatViewSql.propsSql(name, schema: schema),
        database: database,
        limit: 1,
      );
      final p = props.rows.isEmpty
          ? const MatViewProps()
          : MatViewSql.parseProps(props.rows.first);
      return MatViewSql.createSql(MatViewSpec(
        name: name,
        schema: schema,
        select: select,
        tablespace: p.tablespace,
        accessMethod: p.accessMethod,
        storageOptions: p.storageOptions,
      ));
    } catch (e) {
      return null;
    }
  }

  /// 强刷新对象列表(新建 / 删除对象后)。
  /// [schema] 为空刷新库级(默认模式),非空刷新该模式
  Future<void> refreshObjects(String connection, String database,
      {String? schema}) async {
    final conn = connectionByName(connection);
    if (conn != null) {
      await connectionManager.refreshDatabase(conn, database, schema: schema);
    }
  }

  /// 删除表:执行 DROP TABLE IF EXISTS 并刷新对象列表。
  /// 成功后关闭该表已打开的数据 / 设计标签,并清空命中详情面板的选中状态。
  Future<DdlOutcome> dropTable(
    ConnectionInfo conn,
    String database,
    String table, {
    String? schema,
  }) async {
    final ident = _qualifiedIdent(conn.typeId, schema, table);
    try {
      await connectionManager.runQuery(
        conn,
        'DROP TABLE IF EXISTS $ident',
        database: database,
        limit: 1,
      );
      await connectionManager.refreshDatabase(conn, database, schema: schema);
      _closeTableTabs(conn.name, database, schema, table);
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  /// 清空表:删除全部行(保留结构)。
  /// MySQL / MariaDB / PostgreSQL / SQL Server 用 TRUNCATE(更快、重置自增);
  /// SQLite / Access 不支持 TRUNCATE,退化为 DELETE FROM(逐行删除)。
  Future<DdlOutcome> truncateTable(
    ConnectionInfo conn,
    String database,
    String table, {
    String? schema,
  }) async {
    final ident = _qualifiedIdent(conn.typeId, schema, table);
    final useTruncate = const {
      'mysql',
      'mariadb',
      'postgresql',
      'sqlserver',
    }.contains(conn.typeId);
    final sql = useTruncate ? 'TRUNCATE TABLE $ident' : 'DELETE FROM $ident';
    try {
      await connectionManager.runQuery(conn, sql, database: database, limit: 1);
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  /// 复制表:将结构 + 数据克隆到新表名。
  /// MySQL / MariaDB / PostgreSQL / SQLite 用 CREATE TABLE ... AS SELECT;
  /// SQL Server / Access 用 SELECT * INTO(等价语义,但仅复制列与数据,不复制约束)。
  Future<DdlOutcome> copyTable(
    ConnectionInfo conn,
    String database,
    String oldName,
    String newName, {
    String? schema,
    bool refresh = true,
  }) async {
    final newName2 = newName.trim();
    if (newName2.isEmpty) return DdlOutcome(false, '表名不能为空');
    final oldIdent = _qualifiedIdent(conn.typeId, schema, oldName);
    final newIdent = _qualifiedIdent(conn.typeId, schema, newName2);
    final sql = (conn.typeId == 'sqlserver' || conn.typeId == 'access')
        ? 'SELECT * INTO $newIdent FROM $oldIdent'
        : 'CREATE TABLE $newIdent AS SELECT * FROM $oldIdent';
    try {
      await connectionManager.runQuery(conn, sql, database: database, limit: 1);
      if (refresh) {
        await connectionManager.refreshDatabase(conn, database, schema: schema);
      }
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  /// 重命名表:原地更名。
  /// MySQL / MariaDB / PostgreSQL / SQLite 用 ALTER TABLE ... RENAME TO;
  /// SQL Server 用 sp_rename;
  /// Access 不支持 RENAME,退化为「复制为新名 + 删除旧表」实现等价语义。
  Future<DdlOutcome> renameTable(
    ConnectionInfo conn,
    String database,
    String oldName,
    String newName, {
    String? schema,
  }) async {
    final newName2 = newName.trim();
    if (newName2.isEmpty) return DdlOutcome(false, '表名不能为空');
    if (newName2 == oldName) return DdlOutcome(false, '新表名与当前表名相同');
    if (conn.typeId == 'access') {
      // Access 无 RENAME:先复制再删旧,等价重命名
      final cp = await copyTable(conn, database, oldName, newName2, schema: schema);
      if (!cp.ok) return cp;
      return dropTable(conn, database, oldName, schema: schema);
    }
    final oldIdent = _qualifiedIdent(conn.typeId, schema, oldName);
    final newIdent = _ident(conn.typeId, newName2);
    final sql = conn.typeId == 'sqlserver'
        ? "EXEC sp_rename '${schema == null ? oldName : '$schema.$oldName'}', '$newName2'"
        : 'ALTER TABLE $oldIdent RENAME TO $newIdent';
    try {
      await connectionManager.runQuery(conn, sql, database: database, limit: 1);
      await connectionManager.refreshDatabase(conn, database, schema: schema);
      return DdlOutcome(true);
    } catch (e) {
      return DdlOutcome(false, e.toString());
    }
  }

  // ── 表剪贴板(对象面板 Ctrl+C / Ctrl+V) ────────────────────────

  /// 应用内表剪贴板:null = 尚未复制过。粘贴动作是「建表」而非贴文本,
  /// 故不写系统剪贴板
  TableClipboard? tableClipboard;

  /// 记录 Ctrl+C 复制的表([schema] 为有模式层类型的路径)
  void copyTablesToClipboard(
    List<String> tables, {
    required String connection,
    required String database,
    String? schema,
  }) {
    tableClipboard = TableClipboard(
      tables: tables,
      connection: connection,
      database: database,
      schema: schema,
    );
  }

  /// 规划粘贴:给剪贴板里每张源表排定不冲突的新表名(`x_copy`,被占用则
  /// `x_copy_2`…递增)。[taken] 为目标上下文当前已占用的表名;批次内互相
  /// 避让。无剪贴板时返回空列表
  List<(String src, String dst)> tablePastePlan(Set<String> taken) {
    final cb = tableClipboard;
    if (cb == null) return const [];
    final used = {...taken};
    final plan = <(String, String)>[];
    for (final src in cb.tables) {
      var dst = '${src}_copy';
      for (var i = 2; used.contains(dst); i++) {
        dst = '${src}_copy_$i';
      }
      used.add(dst);
      plan.add((src, dst));
    }
    return plan;
  }

  /// 执行 [plan]:把剪贴板里的表按结构 + 数据克隆到其来源 连接/库/模式,
  /// 返回失败项(`新表名: 原因`),全部成功时为空列表
  Future<List<String>> pasteTables(List<(String src, String dst)> plan) async {
    final cb = tableClipboard;
    if (cb == null || plan.isEmpty) return const [];
    final conn = connectionByName(cb.connection);
    if (conn == null) {
      return [for (final (_, dst) in plan) '$dst: 连接「${cb.connection}」不存在'];
    }
    final failed = <String>[];
    var created = 0;
    for (final (src, dst) in plan) {
      final outcome = await copyTable(conn, cb.database, src, dst,
          schema: cb.schema, refresh: false);
      if (outcome.ok) {
        created++;
      } else {
        failed.add('$dst: ${outcome.error}');
      }
    }
    // 整批只重载一次对象列表
    if (created > 0) {
      await connectionManager
          .refreshDatabase(conn, cb.database, schema: cb.schema);
    }
    return failed;
  }

  /// 转储单表结构:返回 CREATE TABLE DDL 文本(失败返回 null)。
  /// 与「转储数据库结构」一致:仅结构、不含数据。
  Future<String?> dumpTableSql(
    ConnectionInfo conn,
    String database,
    String table, {
    String? schema,
  }) async {
    try {
      final cols = await connectionManager.describeTable(
        conn,
        database,
        table,
        schema: schema,
      );
      if (cols.isEmpty) return null;
      final buf = StringBuffer();
      buf.writeln('-- ============================================');
      buf.writeln('-- daro 表结构转储(仅结构,不含数据)');
      buf.writeln('-- 连接: ${conn.name}');
      buf.writeln('-- 数据库: $database');
      if (schema != null && schema.isNotEmpty) buf.writeln('-- 模式: $schema');
      buf.writeln('-- 表: $table');
      buf.writeln('-- 生成时间: ${DateTime.now().toIso8601String()}');
      buf.writeln('-- ============================================');
      buf.writeln();
      buf.writeln(_createTableDdl(conn.typeId, table, cols, schema: schema));
      buf.writeln();
      return buf.toString();
    } catch (e) {
      return null;
    }
  }

  /// 转储视图 / 实体化视图的结构:返回其 CREATE 定义文本(失败返回 null)。
  ///
  /// 与表不同:视图没有列清单可读(读 information_schema.columns 得到的是
  /// 展开后的输出列,重建不出视图),唯一可靠来源是驱动侧的 [getDefinition]。
  /// 因此 Access 这类 getDefinition 恒为 null 的引擎会返回 null,
  /// 由调用方给出「结构读取失败」而不是落一个空文件。
  Future<String?> dumpViewSql(
    ConnectionInfo conn,
    String database,
    String name, {
    ObjectCategory category = ObjectCategory.view,
    String? schema,
  }) async {
    final def = await getObjectDefinition(
      category,
      name,
      connection: conn.name,
      database: database,
      schema: schema,
    );
    if (def == null || def.trim().isEmpty) return null;
    final buf = StringBuffer();
    buf.writeln('-- ============================================');
    // 注释头的分类名跟随 [dumpTableSql] 的既有做法写死中文:落盘文件要能脱离
    // 应用被读,不随界面语言变(AppState 也拿不到 BuildContext)
    buf.writeln('-- daro ${_dumpCategoryName(category)}结构转储(仅结构,不含数据)');
    buf.writeln('-- 连接: ${conn.name}');
    buf.writeln('-- 数据库: $database');
    if (schema != null && schema.isNotEmpty) buf.writeln('-- 模式: $schema');
    buf.writeln('-- 对象: $name');
    buf.writeln('-- 生成时间: ${DateTime.now().toIso8601String()}');
    buf.writeln('-- ============================================');
    buf.writeln();
    // 定义文本末尾可能带分号(MySQL 的 SHOW CREATE VIEW 不带,
    // SQL Server 的 OBJECT_DEFINITION 带),统一补一个且不重复
    final body = def.trimRight();
    buf.writeln(body.endsWith(';') ? body : '$body;');
    buf.writeln();
    return buf.toString();
  }

  /// 转储注释头里的对象分类名(写死,与 [dumpTableSql] 的「表」同源)
  static String _dumpCategoryName(ObjectCategory category) =>
      category == ObjectCategory.materializedView ? '实体化视图' : '视图';

  // ── 导入 / 导出向导 ────────────────────────────────────────

  /// 导出用的分页排序键:优先主键,无主键时按全部列。
  /// OFFSET/LIMIT 分页在无序结果集上会重复或漏行,必须有确定性顺序。
  /// 表无列信息(空表 / 不支持)时返回 null,退化为无序分页。
  ///
  /// [sortColumn] 非空时(来自表数据页当前的排序列)排在最前并带方向,
  /// 其余确定性键跟在后面做 tie-breaker,保证同值行的分页仍然稳定。
  Future<String?> _exportOrderBy(
    ConnectionInfo conn,
    String database,
    String table,
    String? schema, {
    String? sortColumn,
    bool sortAscending = true,
  }) async {
    final cols = await connectionManager.describeTable(conn, database, table,
        schema: schema);
    if (cols.isEmpty) return null;
    final pk = cols.where((c) => c.primaryKey).map((c) => c.name).toList();
    final keys = (pk.isNotEmpty ? pk : cols.map((c) => c.name).toList())
        // 用户排序列已在最前,从 tie-breaker 列表里剔除
        .where((c) => c != sortColumn)
        .map((c) => DdlBuilder.ident(conn.typeId, c))
        .toList();
    if (sortColumn == null || sortColumn.isEmpty) {
      return keys.isEmpty ? null : keys.join(', ');
    }
    final head =
        '${DdlBuilder.ident(conn.typeId, sortColumn)} ${sortAscending ? 'ASC' : 'DESC'}';
    return keys.isEmpty ? head : '$head, ${keys.join(', ')}';
  }

  /// 把一张表导出到 [filePath](格式与选项见 [request])。
  ///
  /// [onProgress] 回调(已导出行数, 总行数或 -1 表示未取到总数);
  /// [isCancelled] 每页检查一次,取消后已写出的内容保留在文件里。
  /// [where] / [sortColumn] 由表数据页「保存数据为」传入当前视图的筛选与排序,
  /// 使导出结果与屏幕所见一致;为 null 时导出整表。
  Future<ExportResult> exportTableData({
    required ConnectionInfo conn,
    required String database,
    required String table,
    String? schema,
    required String filePath,
    required DbExportRequest request,
    List<String>? columns,
    String? where,
    String? sortColumn,
    bool sortAscending = true,
    void Function(int done, int total)? onProgress,
    bool Function()? isCancelled,
  }) async {
    if (columns != null && columns.isNotEmpty) {
      request = request.copyWith(columns: columns);
    }
    String? orderBy;
    var total = -1;
    try {
      orderBy = await _exportOrderBy(conn, database, table, schema,
          sortColumn: sortColumn, sortAscending: sortAscending);
      total = await connectionManager.countTable(conn, database, table,
          schema: schema, where: where);
    } catch (e) {
      return ExportResult(rowsWritten: 0, error: e.toString());
    }
    String? createDdl;
    if (request.format == DbExportFormat.sql &&
        request.sql.createStatement) {
      createDdl = await dumpTableSql(conn, database, table, schema: schema);
    }
    final target = ExportTarget(
      typeId: conn.typeId,
      database: database,
      table: table,
      schema: schema,
    );
    return exportTable(
      target: target,
      request: request.copyWith(createDdl: createDdl),
      filePath: filePath,
      load: (limit, offset) => connectionManager.previewTable(
        conn,
        database,
        table,
        limit: limit,
        offset: offset,
        schema: schema,
        where: where,
        orderBy: orderBy,
      ),
      onProgress:
          onProgress == null ? null : (done, _) => onProgress(done, total),
      isCancelled: isCancelled,
    );
  }

  /// 按 [mapping] 把 [filePath] 导入已存在的表 [table]。
  ///
  /// 每批一条多值 INSERT,经 [ConnectionManager.runQuery] 执行;
  /// 失败批次记录错误但不中断整体(与导出一样可中途取消)。
  Future<ImportResult> importTableData({
    required ConnectionInfo conn,
    required String database,
    required String table,
    String? schema,
    required String filePath,
    required DbImportRequest request,
    required List<ImportColumn> mapping,
    void Function(int rowsDone, int bytesDone, int totalBytes)? onProgress,
    bool Function()? isCancelled,
  }) {
    return importTable(
      filePath: filePath,
      request: request,
      mapping: mapping,
      typeId: conn.typeId,
      database: database,
      table: table,
      schema: schema,
      execute: (sql) async {
        await connectionManager.runQuery(conn, sql,
            database: database, schema: schema, limit: 1);
      },
      onProgress: onProgress,
      isCancelled: isCancelled,
    );
  }

  /// 库(或模式下)的全部表名:必要时触发一次对象列表加载。
  /// 加载失败抛出异常文本,由向导转成错误提示。
  Future<List<String>> tablesInDatabase(
    ConnectionInfo conn,
    String database, {
    String? schema,
  }) async {
    final cm = connectionManager;
    final hasSchema = schema != null && schema.isNotEmpty;
    var state = cm.tableStateOf(conn.name, database, schema: schema);
    if (state.status != LoadStatus.loaded || state.tables == null) {
      if (hasSchema) {
        await cm.expandSchema(conn, database, schema);
      } else {
        await cm.expandDatabase(conn, database);
      }
      state = cm.tableStateOf(conn.name, database, schema: schema);
    }
    if (state.error != null) throw StateError(state.error!);
    return [...?state.tables];
  }

  /// 批量导出多张表(导出向导第 5 步的执行体)。
  ///
  /// 逐表调用 [exportTableData]:每表开始前统计总行数并回调 [onTableStart],
  /// 进度按**全部表**的累计行数上报(进度条跨表不回退)。
  /// [where] / [sortColumn] 只作用于 [whereTable](表数据页带入的当前视图),
  /// 其余表整表导出。单表失败时 [continueOnError] 决定是否继续下一张。
  Future<BatchExportResult> exportTablesBatch({
    required ConnectionInfo conn,
    required String database,
    String? schema,
    required List<ExportJob> jobs,
    required DbExportRequest request,
    String? where,
    String? sortColumn,
    bool sortAscending = true,
    String? whereTable,
    bool continueOnError = true,
    void Function(ExportJob job, int totalRows)? onTableStart,
    void Function(int rowsDone, int rowsTotal)? onProgress,
    void Function(String line)? onLog,
    bool Function()? isCancelled,
  }) async {
    var rowsDone = 0;
    var rowsTotal = 0;
    var failed = 0;
    var cancelled = false;
    String? firstError;

    for (final job in jobs) {
      if (isCancelled?.call() ?? false) {
        cancelled = true;
        onLog?.call('[EXP] Cancelled by user');
        break;
      }
      int total;
      try {
        total = await connectionManager.countTable(conn, database, job.table,
            schema: schema, where: job.table == whereTable ? where : null);
      } catch (e) {
        total = -1;
      }
      rowsTotal += total > 0 ? total : 0;
      onTableStart?.call(job, total);
      onLog?.call('[EXP] Export table [${job.table}]');

      final result = await exportTableData(
        conn: conn,
        database: database,
        table: job.table,
        schema: schema,
        filePath: job.filePath,
        request: request,
        columns: job.columns,
        where: job.table == whereTable ? where : null,
        sortColumn: job.table == whereTable ? sortColumn : null,
        sortAscending: sortAscending,
        onProgress: (done, _) {
          onProgress?.call(rowsDone + done, rowsTotal);
        },
        isCancelled: isCancelled,
      );
      if (result.cancelled) {
        cancelled = true;
        onLog?.call('[EXP] Cancelled by user');
        break;
      }
      rowsDone += result.rowsWritten;
      onProgress?.call(rowsDone, rowsTotal);

      if (result.error != null) {
        failed++;
        firstError ??= '${job.table}: ${result.error}';
        onLog?.call('[ERR] ${job.table} 失败:${result.error}');
        if (!continueOnError) {
          onLog?.call('[EXP] Stopped on error');
          break;
        }
        continue;
      }
      onLog?.call('[EXP] Export to - ${job.filePath}');
    }

    if (failed == 0 && !cancelled) onLog?.call('[EXP] Finished successfully');
    return BatchExportResult(
      rowsWritten: rowsDone,
      tablesDone: jobs.length - failed,
      tablesFailed: failed,
      cancelled: cancelled,
      error: firstError,
    );
  }

  /// 运行 / 还原 SQL 转储文件 [filePath](切分与进度口径见数据层 `runSqlFile`)。
  ///
  /// 整个文件共用**同一个已定位会话**:逐条语句重新切库会让 PostgreSQL 家族
  /// 反复断开重连(其 `useDatabase` 即重连),转储依赖的临时表、会话变量与
  /// `SET` 选项也就无法跨语句存活。
  /// 结束后(无论成败)刷新该库节点,转储新建的表 / 视图即时可见。
  ///
  /// 方法名与顶层引擎函数 [runSqlFile] 刻意不同:实例方法会遮蔽同名导入函数。
  Future<SqlRunResult> executeSqlFile({
    required ConnectionInfo conn,
    required String database,
    String? schema,
    required String filePath,
    bool stopOnError = false,
    void Function(SqlRunProgress progress)? onProgress,
    bool Function()? isCancelled,
  }) async {
    final SqlExecutor execute;
    try {
      final driver = await connectionManager
          .sessionFor(conn, database: database, schema: schema);
      execute = (sql) async {
        // 转储里的语句大多无结果集;limit 1 让驱动不必为个别 SELECT 拉全表
        await driver.executeQuery(sql, limit: 1);
      };
    } catch (e) {
      return SqlRunResult(fatalError: '无法连接到 $database:$e');
    }
    final result = await runSqlFile(
      filePath: filePath,
      execute: execute,
      onProgress: onProgress,
      isCancelled: isCancelled,
      stopOnError: stopOnError,
    );
    try {
      await connectionManager.refreshDatabase(conn, database, schema: schema);
    } catch (_) {
      // 刷新失败不影响执行结果,树节点下次展开自会重试
    }
    return result;
  }

  /// 关闭引用指定表的数据 / 设计标签,并清空命中详情面板的选中状态。
  /// 用于删除 / 重命名表后清理 UI 残留。
  void _closeTableTabs(
    String connection,
    String database,
    String? schema,
    String name,
  ) {
    tabs.removeWhere((t) =>
        t.connection == connection &&
        t.database == database &&
        t.schema == schema &&
        (t.title == name || t.title == '$name$kTabDesignTitleSuffix'));
    final sel = detailSelection.value;
    if (sel != null &&
        sel.kind == NodeKind.table &&
        sel.connection == connection &&
        sel.database == database &&
        sel.schema == schema &&
        sel.name == name) {
      detailSelection.value = null;
    }
    notifyListeners();
  }

  /// 打开用户 / 角色的设计标签(新建或编辑)。
  ///
  /// [name] 为 [listUsers] 返回的对象标识(MySQL 系是 `user@host`)。
  /// 与 [designRoutine] 同构,只是分类固定为 [ObjectCategory.user] ——
  /// 路由时据此进 `UserDesignPage`(见 main_page 的 TabType.design 分支)。
  void designUser(
    String name, {
    required String connection,
    required String database,
    String? schema,
    bool isNew = false,
  }) {
    selectTable(name);
    final tab = OpenTab(
      TabType.design,
      isNew ? '$name$kTabNewTitleSuffix' : '$name$kTabDesignTitleSuffix',
      null,
      connection,
      database,
      schema,
      ObjectCategory.user,
    );
    final exists =
        tabs.any((t) => t.type == TabType.design && t.key == tab.key);
    activeTab = tab.title;
    if (!exists) tabs.add(tab);
    notifyListeners();
  }

  /// 打开视图 / 函数 / 过程的设计标签(新建或编辑已有定义)。
  /// [schema] 非空时对象位于该模式下(PostgreSQL / SQL Server 等);
  /// [params] / [comment] / [options] / [returnType] 为新建模式时向导采集的
  /// 初始定义,透传给设计页用于生成模板(编辑模式忽略)
  void designRoutine(
    String name, {
    required String connection,
    required String database,
    required ObjectCategory category,
    String? schema,
    bool isNew = false,
    String? params,
    String? comment,
    RoutineOptions options = const RoutineOptions(),
    String? returnType,
  }) {
    selectTable(name);
    final tab = OpenTab(
      TabType.design,
      isNew ? '$name$kTabNewTitleSuffix' : '$name$kTabDesignTitleSuffix',
      null,
      connection,
      database,
      schema,
      category,
      isNew ? params : null,
      isNew ? comment : null,
      isNew ? options : null,
      isNew ? returnType : null,
    );
    final exists =
        tabs.any((t) => t.type == TabType.design && t.key == tab.key);
    activeTab = tab.title;
    if (!exists) tabs.add(tab);
    notifyListeners();
  }
}
