import 'dart:async';
import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
// RenderAbstractViewport(新增连接的滚动定位用)不在 material 导出面里
import 'package:flutter/rendering.dart' show RenderAbstractViewport;
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:base_ui_flutter/base_ui_flutter.dart';
import '../app/app_state.dart';
import '../app/connection_manager.dart';
import '../app/kv_manager.dart';
import '../data/db_data.dart';
import '../data/db_types.dart';
import '../data/drivers/db_driver.dart';
import '../data/drivers/kv_driver.dart';
import '../data/table_design.dart';
import '../data/user_sql.dart';
import '../pages/connection_dialog_page.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';
import 'connection_password_window.dart';
import 'create_database_dialog.dart';
import 'create_schema_dialog.dart';
import 'database_edit_dialog.dart';
import 'function_wizard_dialog.dart';
import 'object_category_icon.dart';
import 'rename_schema_dialog.dart';
import 'sql_file_run_dialog.dart';
import 'table_context_menu.dart';

class DatabaseTree extends StatefulWidget {
  const DatabaseTree({super.key});

  @override
  State<DatabaseTree> createState() => _DatabaseTreeState();
}

class _DatabaseTreeState extends State<DatabaseTree> {
  /// 树节点图标尺寸(连接/库/模式/分组):正文字号 12px,图标取 16px。
  static const double _treeIconSize = 16;

  /// 对象行图标尺寸(表/视图/函数):与树节点图标同尺寸。
  static const double _objectIconSize = 16;

  /// 节点行高(与 [_node] 的 Container height 一致)。仅用于新增连接的
  /// 滚动定位估算:提示行为 24px 造成的少量偏差由第二段精确定位校正。
  static const double _kNodeRowHeight = 26;

  /// 展开节点 key 集合:连接名 / "连接名|库" / "连接名|库|模式"。
  /// 分组(表 / 视图 / 函数 / 用户 / 查询)不可展开,不入此集合。
  final Set<String> _expanded = {};

  /// 已打开节点 key 集合:记录曾通过 SQL 获取过下级数据的节点。
  /// 折叠不清除;右键「关闭」库 / 模式或关闭连接时清除(节点回到灰色未打开态,
  /// 再次打开需重新拉取)。用于控制箭头显隐与灰色态。
  final Set<String> _opened = {};

  /// 被折叠的**连接分组**名集合。语义与 [_expanded] 相反:分组默认展开
  /// (新建分组不该看起来"空的"),所以只记被收起的那些;也与 _expanded 分开,
  /// 避免分组名与连接名同名时互相干扰。
  final Set<String> _collapsedGroups = {};

  final ValueNotifier<String?> _selectedNode = ValueNotifier<String?>(null);

  /// 树列表键盘焦点:点行即取得,F2 就地重命名由此触发。
  final FocusNode _treeFocus = FocusNode();

  /// 树纵向滚动:新增连接的滚动定位由它执行
  final ScrollController _treeScroll = ScrollController();

  /// 新增连接滚动定位:值 = 待定位的连接名(null = 无)。目标行在 build 时
  /// 携带 [_revealRowKey] 并登记行号 [_revealRowIndex],post-frame 由
  /// [_scrollToReveal] 消费后清空。
  String? _revealConnName;
  final GlobalKey _revealRowKey = GlobalKey();

  /// 本次 build 中定位目标行的行号(rows 列表下标,估算滚动偏移用)
  int? _revealRowIndex;

  /// 正在内联改名的节点 key(null = 无编辑)。支持三类:
  /// 分组 `g:名` / 连接 `名` / 表 `连接|库[|模式]|table|名`(与行 key 一致)。
  String? _editingKey;

  /// 手动双击判定用:上一次左键按下的行 key 与时刻(毫秒)。
  /// 行体双击展开不走 GestureDetector.onDoubleTap——那会让同目标上的
  /// 单击(箭头切换)被双击判定窗口 hold 约 300ms。
  String? _lastTapKey;
  int _lastTapMs = 0;
  static const int _doubleTapWindowMs = 500;

  /// 正在被拖动的连接(null = 无拖动)。拖动分组内连接时,
  /// 树底部显示独立的「移到未分组」放置条(与分组行 DragTarget 不嵌套)。
  ConnectionInfo? _draggingConn;

  /// 编辑被 Esc 取消时的撤销动作(仅"新建分组"用:删掉临时分组并回移连接)。
  VoidCallback? _onEditCancel;

  /// 每次 build 重建:表节点 key → 改名所需的连接/库/模式上下文。
  final Map<String, ({ConnectionInfo conn, String database, String? schema})>
      _tableNodes = {};

  final _searchController = TextEditingController();

  /// 底部搜索框的焦点节点:Ctrl+F 把光标送进来、Esc 再交还给树,
  /// [_onPanelKey] 也用它判断「焦点此刻是否就在搜索框里」。
  final FocusNode _searchFocus = FocusNode();

  /// 订阅 ConnectionManager:真实连接的库/表列表加载完成后重建树
  ConnectionManager? _listenedManager;

  /// 订阅 KvConnectionManager:缓存型连接(Redis 等)的命名空间加载完成后重建树
  KvConnectionManager? _listenedKvManager;

  /// 订阅 AppState:Ribbon 按钮点击后展开祖先并选中分组节点
  AppState? _listenedApp;

  void _onManagerChanged() {
    // 关键:进入 loading 的时刻记在这里而不是 build 里。加载比一帧还短时,
    // loading 期间根本不会有任何 build,只有通知是无条件到达的。
    _recordLoadingEntries();
    if (mounted) setState(() {});
  }

  // ── 「打开中」指示器的最小可见时长 ──────────────────────────────
  // 本地库(或已建连的会话)「打开」常在 1~2 帧内跑完:状态 loading→loaded
  // 的整个窗口比一帧(16ms)还短,转圈指示器一次都来不及绘制,用户看到的
  // 就是子级凭空冒出来。给指示器一个最小停留时长,让「打开中」真正可感知;
  // 只延长视觉呈现,不拖慢真实数据。

  /// 转圈指示器至少停留这么久才会收起
  static const Duration kMinSpinnerVisible = Duration(milliseconds: 320);

  /// 节点 key → 该节点进入 loading 的时刻(只记首次,重复刷新不重置)
  final Map<String, DateTime> _loadingSince = {};

  /// 已触发「打开动作」但还没进入 loading 的节点(典型:正在等密码补录弹窗)。
  /// 「打开中」从触发动作那一刻就转起来,而不是等密码填完才开始。
  final Set<String> _pendingOpening = {};

  /// 标记已触发打开动作:指示器从这一刻开始转(密码弹窗等待期也算打开中)
  void _markOpening(String key) {
    if (!mounted) return;
    setState(() => _pendingOpening.add(key));
  }

  /// 打开动作结束(成功转由 loading 接管 / 用户取消 / 失败):撤下挂起态
  void _clearOpening(String key) {
    if (!mounted) return;
    setState(() => _pendingOpening.remove(key));
  }

  /// 标记「打开中」并**等这一帧真正画完**再返回。
  ///
  /// 紧跟其后的动作是密码补录:桌面端要拉起独立的系统窗口(新的 Flutter
  /// engine),在 Windows 上会占用主线程数百毫秒。若不等这一帧上屏就开窗口,
  /// 转圈会被这个重活挤到后面,和弹窗一起「卡」一下才冒出来——看起来就是
  /// 点了没反应。先让指示器画出来,后续再重也不影响它已经可见。
  Future<void> _beginOpening(String key) async {
    if (!mounted) return;
    _markOpening(key);
    await WidgetsBinding.instance.endOfFrame;
  }

  /// 节点 key → 到点收起指示器的定时器
  final Map<String, Timer> _minSpinnerTimers = {};

  /// 扫描当前处于 loading 的节点,补记它们的进入时刻。
  /// 由 [_onManagerChanged] 调用——通知独立于帧,快于一帧的加载也能被记到。
  void _recordLoadingEntries() {
    final app = _listenedApp;
    if (app == null) return;
    final manager = app.connectionManager;
    final now = DateTime.now();
    for (final conn in app.connections) {
      final dbState = manager.databaseStateOf(conn.name);
      if (dbState.status == LoadStatus.loading) {
        _loadingSince[conn.name] ??= now;
      }
      for (final db in dbState.databases) {
        final dbKey = '${conn.name}|$db';
        if (manager.tableStateOf(conn.name, db).status == LoadStatus.loading) {
          _loadingSince[dbKey] ??= now;
        }
        for (final schema in manager.schemaStateOf(conn.name, db).schemas) {
          final sKey = '$dbKey|$schema';
          if (manager.tableStateOf(conn.name, db, schema: schema).status ==
              LoadStatus.loading) {
            _loadingSince[sKey] ??= now;
          }
        }
      }
    }
  }

  /// 是否显示转圈指示器:
  /// - [loading] 为真 → 立即显示(顺带补记进入时刻,兜底未走通知的路径);
  /// - 加载已结束但停留不足 [kMinSpinnerVisible] → 继续显示,到点自动重建收起。
  bool _showSpinner(String key, bool loading) {
    if (loading) {
      _minSpinnerTimers.remove(key)?.cancel();
      _loadingSince[key] ??= DateTime.now();
      return true;
    }
    final since = _loadingSince[key];
    if (since == null) return false;
    final elapsed = DateTime.now().difference(since);
    if (elapsed >= kMinSpinnerVisible) {
      _loadingSince.remove(key);
      return false;
    }
    // 还没停留够:补一个到点收起的定时器(只排一次,避免重复重建时被顶掉)
    _minSpinnerTimers[key] ??= Timer(kMinSpinnerVisible - elapsed, () {
      _minSpinnerTimers.remove(key);
      _loadingSince.remove(key);
      if (mounted) setState(() {});
    });
    return true;
  }

  /// Ribbon 快速切换时:仅联动选中当前上下文库对应的分组节点,
  /// 不改动树的展开状态——连接/库/模式/分组保持用户原有的展开/折叠,
  /// 分组子树(对象列表)也不强制展开。数据加载仍触发,展开后即可见。
  void _onTreeNavigate() {
    final app = _listenedApp;
    if (app == null) return;
    final conn = app.objectConnection;
    final db = app.objectDatabase;
    if (conn == null || db == null) return;
    final schema = app.objectSchema;
    final category = app.objectCategory;
    // 分组节点 key 随层级变化:有模式层时为 连接|库|模式|分类,否则 连接|库|分类
    _selectedNode.value = schema == null
        ? '$conn|$db|${category.name}'
        : '$conn|$db|$schema|${category.name}';
    // 仅联动高亮分组节点,不触发任何加载——数据加载由展开(打开)树节点驱动;
    // 库/模式未打开时分组节点本就不可见,选中无视觉效果也不报错
  }

  /// 新增连接(向导确定 / 复制):树选中该连接并滚动到其所在位置。
  /// 所在分组处于折叠态时先展开,否则节点根本不在树里;
  /// 被搜索 / 类型筛选滤除时只选中、不滚动(节点未渲染,无从定位)。
  void _onConnectionAdded() {
    final app = _listenedApp;
    if (app == null) return;
    final name = app.connectionAdded.value.$2;
    if (name.isEmpty) return;
    ConnectionInfo? found;
    for (final c in app.connections) {
      if (c.name == name) {
        found = c;
        break;
      }
    }
    final conn = found;
    if (conn == null) return;
    setState(() {
      if (conn.group.isNotEmpty) _collapsedGroups.remove(conn.group);
      _revealConnName = conn.name;
      _revealRowIndex = null;
    });
    _selectedNode.value = conn.name;
    // 与单击连接节点同口径:右侧详情面板跟随显示该连接
    app.detailSelection.value =
        SelectedNode(NodeKind.connection, conn.name, connection: conn.name);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _scrollToReveal();
    });
  }

  /// 把新增连接行滚入视口。两段式:
  /// 行已构建(在视口或 cacheExtent 内)→ 直接最小滚动露出整行;
  /// 行未构建(长列表远端,Element 未挂载)→ 先按行号 × 行高估算跳转,
  /// 行挂载后下一帧精确校正。提示行(24px)造成的估算偏差由第二段兜底。
  void _scrollToReveal() {
    if (_revealConnName == null) return;
    _revealConnName = null;
    final ctx = _revealRowKey.currentContext;
    if (ctx != null) {
      _ensureRowVisible(ctx);
      return;
    }
    if (!_treeScroll.hasClients) return;
    final index = _revealRowIndex;
    if (index == null) return; // 行未渲染(被筛选滤除):只选中,不滚动
    final position = _treeScroll.position;
    // 目标行以上全是 26px 节点行;先让行进入视口(居中偏上),下一帧精确校正
    final estimated = index * _kNodeRowHeight - position.viewportDimension / 2;
    position.jumpTo(estimated.clamp(0.0, position.maxScrollExtent));
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final refined = _revealRowKey.currentContext;
      if (refined != null) _ensureRowVisible(refined);
    });
  }

  /// 最小滚动量把 [rowCtx] 的行露出:已在视口内是空操作;在视口上方滚到
  /// 行顶对齐视口顶,在下方滚到行底对齐视口底(EnsureVisible 语义)。
  /// 即时跳转不走动画,与全应用的零延迟节奏一致。
  void _ensureRowVisible(BuildContext rowCtx) {
    final obj = rowCtx.findRenderObject();
    final viewport = obj == null ? null : RenderAbstractViewport.maybeOf(obj);
    final scrollable = Scrollable.maybeOf(rowCtx);
    if (obj == null || viewport == null || scrollable == null) return;
    final position = scrollable.position;
    // 行顶对齐视口顶 / 行底对齐视口底所需的偏移;行可见 ⇔ offset ∈ [bottom, top]
    final top = viewport.getOffsetToReveal(obj, 0.0).offset;
    final bottom = viewport.getOffsetToReveal(obj, 1.0).offset;
    if (position.pixels > top) {
      position.jumpTo(top.clamp(0.0, position.maxScrollExtent));
    } else if (position.pixels < bottom) {
      position.jumpTo(bottom.clamp(0.0, position.maxScrollExtent));
    }
  }

  /// 关闭筛选(搜索框 × / 退格删空 / 「清除全部筛选」)后:滚回当前操作的
  /// 连接位置。筛选一关,全量列表从顶部重新出现,刚操作的连接容易淹没在
  /// 长列表里。选中的是其下的库 / 模式 / 表节点时同样映射到所属连接行;
  /// 分组节点(g:)、空选中、或该连接行未渲染(所在分组折叠)时不处理。
  void _revealSelectedConnection(AppState app) {
    final key = _selectedNode.value;
    if (key == null || key.startsWith('g:')) return;
    final name = key.split('|').first;
    if (!app.connections.any((c) => c.name == name)) return;
    setState(() {
      _revealConnName = name;
      _revealRowIndex = null;
    });
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _scrollToReveal();
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final app = context.read<AppState>();
    final manager = app.connectionManager;
    if (_listenedManager != manager) {
      _listenedManager?.removeListener(_onManagerChanged);
      _listenedManager = manager;
      manager.addListener(_onManagerChanged);
    }
    if (_listenedKvManager != app.kvManager) {
      _listenedKvManager?.removeListener(_onManagerChanged);
      _listenedKvManager = app.kvManager;
      app.kvManager.addListener(_onManagerChanged);
    }
    if (_listenedApp != app) {
      _listenedApp?.treeNavigate.removeListener(_onTreeNavigate);
      _listenedApp?.connectionAdded.removeListener(_onConnectionAdded);
      _listenedApp = app;
      app.treeNavigate.addListener(_onTreeNavigate);
      app.connectionAdded.addListener(_onConnectionAdded);
    }
  }

  void _toggle(String key) => setState(() {
        _expanded.contains(key) ? _expanded.remove(key) : _expanded.add(key);
      });

  /// 折叠全部节点(底栏「折叠全部」按钮,无展开节点时按钮禁用)。
  /// 连接分组默认展开,折叠全部 = 把当前所有分组名收进折叠集。
  void _collapseAll(AppState app) => setState(() {
        _expanded.clear();
        _collapsedGroups.addAll([for (final g in app.groups) g.name]);
      });

  /// 统一展开 / 收起:连接 / 库 / 模式节点,单击箭头或双击整行都走此逻辑,
  /// 切换其子树展开状态,并在「首次展开」时懒加载下一级数据。
  /// 分组节点(表 / 视图 / 函数 / 用户 / 查询)不需要「打开」功能——
  /// 父级(库 / 模式)展开后其下对象行始终可见(见 _groupRows),不走这里。
  /// 叶子节点(表 / 视图 / 函数等)不在此列,各自另有双击行为。
  void _toggleNode({
    required AppState app,
    required ConnectionInfo conn,
    required NodeKind kind,
    required String key,
    String? database,
    String? schema,
  }) {
    final l = context.l10n;
    final willExpand = !_expanded.contains(key);
    if (!willExpand) {
      _toggle(key); // 收起时无需加载
      return;
    }
    // 连接节点可能需要补录密码(弹窗可被取消):「展开 + 标记已打开 + 记日志 +
    // 拉库列表」整体延后到密码确认之后(见 [_lazyExpandConnection])。否则弹窗
    // 一取消,节点就停在展开态且状态仍是 idle,树上留下永久「加载中...」。
    if (kind == NodeKind.connection && hasAnyDriver(conn)) {
      _lazyExpandConnection(app, conn);
      return;
    }
    _toggle(key);
    // 标记为已打开(曾通过 SQL 获取过下级数据)
    _opened.add(key);
    // 状态栏记录打开操作
    app.logTreeAction(switch (kind) {
      NodeKind.connection => l.openedConnection(conn.name),
      NodeKind.database => l.openedDatabase('$database'),
      NodeKind.schema => l.openedSchema('$schema'),
      _ => l.openedBare,
    });
    switch (kind) {
      case NodeKind.connection:
        // 有驱动的连接已在上面提前返回;此处只剩未实现驱动的类型,
        // 展开后由树渲染「暂不支持该类型」提示,无需发请求
        break;
      case NodeKind.database:
        if (database != null) {
          app.connectionManager.expandDatabase(conn, database);
        }
      case NodeKind.schema:
        if (database != null && schema != null) {
          app.connectionManager.expandSchema(conn, database, schema);
        }
      case NodeKind.tableGroup:
      case NodeKind.table:
      case NodeKind.connGroup:
      // 命名空间是叶子:键不进树(Redis 单库几千个键,塞进树等于把
      // 浏览页的活搬到侧栏),键列表在点开该节点时的键浏览标签里
      case NodeKind.kvNamespace:
        // 分组 / 叶子不可展开(分组对象行随父级展开始终可见),仅保留 case 满足穷尽
        break;
    }
  }

  void _selectNode(String key) => _selectedNode.value = key;

  /// 切换分组节点展开/折叠并在状态栏记录日志(分组打开/关闭=展开/折叠)
  void _toggleGroup(AppState app, String groupKey, String label, bool wasExpanded) {
    _toggle(groupKey);
    final l = context.l10n;
    app.logTreeAction(wasExpanded ? l.closedNamed(label) : l.openedNamed(label));
  }

  // ── 内联改名:F2 / 菜单进入编辑,无弹窗 ─────────────────────────

  /// 树键盘交互:F2 对当前选中的分组 / 连接 / 表节点进入就地重命名。
  /// 编辑器持有焦点时不参与(Enter / Esc 归编辑器)。
  KeyEventResult _onTreeKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent || _editingKey != null) {
      return KeyEventResult.ignored;
    }
    if (event.logicalKey != LogicalKeyboardKey.f2) {
      return KeyEventResult.ignored;
    }
    final key = _selectedNode.value;
    if (key == null) return KeyEventResult.ignored;
    final app = context.read<AppState>();
    if (!_canRenameKey(key, app)) return KeyEventResult.ignored;
    setState(() {
      _editingKey = key;
      _onEditCancel = null;
    });
    return KeyEventResult.handled;
  }

  /// 区域级快捷键:Ctrl+F 把光标送进底部搜索框,Esc 再退出回到树。
  ///
  /// 挂在 [build] 返回的最外层 Focus 上而不是 [_treeFocus] 上:`_treeFocus` 只包住
  /// ListView,底部搜索栏与它是**兄弟节点**——光标一旦进了搜索框,树这条分支再也
  /// 收不到任何按键(按 Esc 退出时正是如此);祖先 Focus 则会收到后代冒泡上来的事件。
  /// `canRequestFocus = false` 让这层只做转发,不参与焦点竞争、也不吞掉 Tab 键。
  KeyEventResult _onPanelKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    // 内联改名编辑器持有焦点时不抢键(与 [_onTreeKey] 同一口径)
    if (_editingKey != null) return KeyEventResult.ignored;

    if (event.logicalKey == LogicalKeyboardKey.keyF &&
        HardwareKeyboard.instance.isControlPressed) {
      _searchFocus.requestFocus();
      // 整段选中:再按一次 Ctrl+F 是直接改写现有过滤词,而不是把光标挪到末尾
      final text = _searchController.text;
      _searchController.selection =
          TextSelection(baseOffset: 0, extentOffset: text.length);
      return KeyEventResult.handled;
    }

    if (event.logicalKey == LogicalKeyboardKey.escape && _searchFocus.hasFocus) {
      final app = context.read<AppState>();
      if (_searchController.text.isNotEmpty) {
        // 与搜索框内 × 按钮同一动作(SearchInput._clear 会补发一次空串):
        // controller.clear() 不走 onChanged,所以落盘与滚回选中连接都得手动补
        _searchController.clear();
        app.setTreeSearchText('');
        _revealSelectedConnection(app);
      }
      // 交还焦点:Esc 之后方向键 / F2 仍然作用在树里选中的节点上
      _treeFocus.requestFocus();
      return KeyEventResult.handled;
    }

    return KeyEventResult.ignored;
  }

  /// key 是否可就地改名:分组(存在)/ 连接(存在)/ 表(节点可见且已连接)。
  bool _canRenameKey(String key, AppState app) {
    if (key.startsWith('g:')) {
      final group = key.substring(2);
      return app.groupNames.any((g) => g == group);
    }
    if (app.connections.any((c) => c.name == key)) return true;
    final table = _tableNodes[key];
    if (table != null) {
      return app.connectionManager.isConnected(table.conn.name);
    }
    return false;
  }

  /// 结束编辑并把键盘焦点收回树列表(编辑器随重建卸载)。
  void _endEditing() {
    if (_editingKey == null) return;
    setState(() {
      _editingKey = null;
      _onEditCancel = null;
    });
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _treeFocus.requestFocus();
    });
  }

  /// Esc 取消:先执行登记的撤销动作(新建分组 = 删除临时分组并回移连接)
  void _cancelEditing() {
    final cancel = _onEditCancel;
    _endEditing();
    cancel?.call();
  }

  /// 新建分组用的默认名:「未命名分组」,重名递增序号
  String _uniqueGroupName(AppState app) {
    final l = context.l10n;
    final taken = {for (final g in app.groupNames) g.toLowerCase()};
    for (var i = 0;; i++) {
      final name = i == 0 ? l.unnamedGroup : l.unnamedGroupNumbered('${i + 1}');
      if (!taken.contains(name.toLowerCase())) return name;
    }
  }

  /// 就地新建分组:[moveConn] 非空时把该连接移入。
  /// 新分组节点直接展开编辑(全选占位名),Esc 撤销整个操作
  void _createGroupInline(AppState app, {ConnectionInfo? moveConn}) {
    final name = _uniqueGroupName(app);
    app.addGroup(name);
    final originGroup = moveConn?.group ?? '';
    if (moveConn != null) app.moveConnectionToGroup(moveConn, name);
    _selectedNode.value = 'g:$name';
    app.detailSelection.value = SelectedNode(NodeKind.connGroup, name);
    setState(() {
      _editingKey = 'g:$name';
      _onEditCancel = moveConn == null
          ? () => app.deleteGroup(name)
          : () {
              app.moveConnectionToGroup(moveConn, originGroup);
              app.deleteGroup(name);
            };
    });
    _treeFocus.requestFocus();
  }

  /// 分组改名提交:重名(不区分大小写)弹错并保持原名
  void _commitGroupRename(String old, String input) {
    _endEditing();
    final name = input.trim();
    if (name.isEmpty || name == old) return;
    final app = context.read<AppState>();
    if (!app.renameGroup(old, name)) {
      if (!mounted) return;
      final l = context.l10n;
      MessageBox.show(
        context,
        title: l.renameGroupTitle,
        message: l.groupAlreadyExists(name),
        type: MessageBoxType.error,
        okText: l.btnGotIt,
        tokens: Tokens.read(context).desktopTokensFor(context),
      );
      return;
    }
    // 折叠状态跟着改名走,否则重命名后原本收起的分组会突然展开
    if (_collapsedGroups.remove(old)) _collapsedGroups.add(name);
    if (_selectedNode.value == 'g:$old') _selectedNode.value = 'g:$name';
  }

  /// 连接改名提交:走 [AppState.updateConnection](自动避重 + 迁移标签与驱动),
  /// 与「编辑连接」改名的后续处理一致
  Future<void> _commitConnectionRename(String old, String input) async {
    _endEditing();
    final name = input.trim();
    if (name.isEmpty || name == old) return;
    final app = context.read<AppState>();
    ConnectionInfo? conn;
    for (final c in app.connections) {
      if (c.name == old) {
        conn = c;
        break;
      }
    }
    if (conn == null) return;
    final updated = await app.updateConnection(conn, conn.copyWith(name: name));
    if (updated == null || !mounted) return;
    if (updated.name != old) {
      _migrateConnectionKeys(old, updated.name);
      final detail = app.detailSelection.value;
      if (detail != null && detail.connection == old) {
        app.detailSelection.value = SelectedNode(
          detail.kind,
          detail.name,
          connection: updated.name,
          database: detail.database,
          schema: detail.schema,
        );
      }
      if (_expanded.contains(updated.name) && hasAnyDriver(updated)) {
        expandKvOrSql(app, updated);
      }
      // 连接改名守卫:旧名若已被 MCP 授权,自动迁移策略与池驱动实例
      try {
        await app.mcp.renameConnection(old, updated.name);
      } catch (_) {
        // 不影响主流程:改名已生效,仅记录日志不弹窗
      }
      final l = context.l10n;
      app.logTreeAction(l.renamedConnection(updated.name, old));
    }
  }

  /// 表改名提交:ALTER TABLE ... RENAME(renameTable 内部已刷新对象列表);
  /// 失败弹窗,成功后新名保持选中
  Future<void> _commitTableRename(
    ConnectionInfo conn,
    String database,
    String? schema,
    String old,
    String input,
  ) async {
    _endEditing();
    final name = input.trim();
    if (name.isEmpty || name == old) return;
    final app = context.read<AppState>();
    final outcome =
        await app.renameTable(conn, database, old, name, schema: schema);
    if (outcome.ok) {
      final newKey = schema == null
          ? '${conn.name}|$database|${ObjectCategory.table.name}|$name'
          : '${conn.name}|$database|$schema|${ObjectCategory.table.name}|$name';
      if (_selectedNode.value != null) _selectedNode.value = newKey;
      app.logTreeAction(context.l10n.renamedTable(name, old));
      return;
    }
    if (!mounted) return;
    final l = context.l10n;
    MessageBox.show(
      context,
      title: l.renameTableTitle,
      message: l.renameFailedDetail('${outcome.error}'),
      type: MessageBoxType.error,
      okText: l.btnGotIt,
      tokens: Tokens.read(context).desktopTokensFor(context),
    );
  }

  // ── 连接节点右键菜单:打开 / 关闭 / 新建库 / 编辑 / 复制 / 删除 ──

  /// 右键连接节点:弹出上下文菜单。
  /// 菜单项按需构建(连接配置在右键时才能确定),用 showContextMenu 程序化弹出
  void _showConnectionMenu(
    BuildContext context,
    AppState app,
    ConnectionInfo conn,
    Offset position,
  ) {
    // 状态一律按驱动链分流取(与连接行的图标 / 转圈判定同口径):
    // Redis 等缓存型引擎的驱动与库列表在 KvConnectionManager 里,
    // 只读 SQL 侧会让它恒判为「未连接」——菜单永远只显示「打开连接」,关不掉。
    final isKv = isKvStoreConnection(conn);
    final connected = isKv
        ? app.kvManager.isConnected(conn.name)
        : app.connectionManager.isConnected(conn.name);
    final l = context.l10n;
    // 刷新仅对已加载出子节点(库 / 命名空间列表)的连接可用
    final hasDbChildren = isKv
        ? app.kvManager.namespacesOf(conn.name).namespaces.isNotEmpty
        : app.connectionManager.databaseStateOf(conn.name).databases.isNotEmpty;
    showContextMenu(
      context,
      position: position,
      items: [
        if (!connected)
          MenuItem(
            text: l.ctxOpenConnection,
            onPressed: () => _openConnectionNode(app, conn),
          )
        else
          MenuItem(
            text: l.ctxCloseConnection,
            onPressed: () => _closeConnectionNode(app, conn),
          ),
        MenuItem(
          text: l.ctxRefresh,
          enabled: hasDbChildren,
          onPressed: () => _openConnectionNode(app, conn),
        ),
        MenuItem(
          text: l.ctxNewDatabase,
          // 需连接处于打开状态(驱动已建立)才可建库;
          // 文件型数据库(SQLite / Access)无独立库概念,不支持建库;
          // 缓存型引擎的库数由服务端配置决定(databases),客户端建不出新库
          enabled: connected && !isKv && !_isFileBasedType(conn),
          onPressed: () => _createDatabaseNode(app, conn),
        ),
        MenuSeparator(),
        MenuItem(text: l.ctxEditConnection, onPressed: () => _editConnectionNode(app, conn)),
        MenuItem(text: l.ctxCopyConnection, onPressed: () => app.copyConnection(conn)),
        MenuItem(
          // 移动到分组:纯视图变化,不断驱动、不动已打开的标签(两者都按连接名索引)
          text: l.ctxMoveToGroup,
          children: [
            MenuItem(
              text: l.ctxUngrouped,
              enabled: conn.group.isNotEmpty,
              onPressed: () => app.moveConnectionToGroup(conn, ''),
            ),
            for (final g in app.groups)
              MenuItem(
                text: g.name,
                enabled: g.name != conn.group,
                onPressed: () => app.moveConnectionToGroup(conn, g.name),
              ),
            const MenuSeparator(),
            MenuItem(
              text: l.ctxNewGroup,
              onPressed: () => _createGroupInline(app, moveConn: conn),
            ),
          ],
        ),
        MenuSeparator(),
        MenuItem(text: l.ctxDeleteConnection, onPressed: () => _deleteConnectionNode(app, conn)),
      ],
    );
  }

  // ── 连接分组节点右键菜单:新建连接 / 重命名 / 删除 ──

  /// 右键分组节点。分组只是本地视图层容器,菜单里不放任何「打开/连接」类动作。
  void _showConnGroupMenu(
    BuildContext context,
    AppState app,
    String group,
    Offset position,
  ) {
    final members = app.connections.where((c) => c.group == group).length;
    final l = context.l10n;
    showContextMenu(
      context,
      position: position,
      items: [
        MenuItem(
          text: l.ctxNewConnectionEllipsis,
          onPressed: () => _newConnectionInGroup(context, app, group),
        ),
        MenuSeparator(),
        MenuItem(
          text: l.ctxRefresh,
          enabled: members > 0,
          onPressed: () => app.reloadConnections(),
        ),
        MenuSeparator(),
        MenuItem(
          text: l.ctxRenameGroup,
          onPressed: () {
            _selectedNode.value = 'g:$group';
            setState(() {
              _editingKey = 'g:$group';
              _onEditCancel = null;
            });
            _treeFocus.requestFocus();
          },
        ),
        MenuItem(
          text: members == 0 ? l.ctxDeleteGroup : l.ctxDeleteGroupWith('$members'),
          onPressed: () => _deleteGroupNode(context, app, group, members),
        ),
      ],
    );
  }

  /// 在指定分组下新建连接:走同一个连接向导,确定后落到该分组
  Future<void> _newConnectionInGroup(
    BuildContext context,
    AppState app,
    String group,
  ) async {
    final result = await showDialog<ConnectionInfo>(
      context: context,
      builder: (_) => const ConnectionDialogPage(),
    );
    if (result != null) app.addConnection(result.copyWith(group: group));
  }

  /// 删除分组:连接不删,回落到未分组(故仅在有连接时二次确认)
  Future<void> _deleteGroupNode(
    BuildContext context,
    AppState app,
    String group,
    int memberCount,
  ) async {
    if (memberCount > 0) {
      final l = context.l10n;
      final confirm = await MessageBox.show(
        context,
        title: l.ctxDeleteGroup,
        message: l.deleteGroupConfirm('$memberCount', group),
        type: MessageBoxType.warning,
        buttons: MessageBoxButtons.yesNo,
        yesText: l.ctxDeleteGroup,
        noText: l.btnCancel,
        tokens: Tokens.read(context).desktopTokensFor(context),
      );
      if (confirm != MessageBoxResult.yes || !mounted) return;
    }
    app.deleteGroup(group);
  }

  /// 文件型数据库(SQLite / Access):单文件即一个库,无「新建数据库」概念
  static bool _isFileBasedType(ConnectionInfo conn) =>
      conn.typeId == 'sqlite' || conn.typeId == 'access';

  /// 「关闭连接」:断开服务器连接并清空该连接在树中的元数据
  /// (连接配置保留);再次展开节点时自动重连并懒加载。
  /// 同时清理树内选中 / 详情选中;若该连接正被中部对象页浏览,
  /// 同步清空对象上下文(面板回到禁用空态)。
  Future<void> _closeConnectionNode(AppState app, ConnectionInfo conn) async {
    setState(() {
      _expanded.removeWhere(
          (k) => k == conn.name || k.startsWith('${conn.name}|'));
      // 关闭连接时清除已打开状态,再次打开需要重新获取数据
      _opened.removeWhere(
          (k) => k == conn.name || k.startsWith('${conn.name}|'));
      final sel = _selectedNode.value;
      if (sel == conn.name || (sel != null && sel.startsWith('${conn.name}|'))) {
        _selectedNode.value = null;
      }
      final detail = app.detailSelection.value;
      if (detail != null && detail.connection == conn.name) {
        app.detailSelection.value = null;
      }
    });
    if (app.objectConnection == conn.name) {
      app.clearObjectContext();
    }
    app.logTreeAction(context.l10n.closedConnection(conn.name));
    await app.connectionManager.disconnect(conn.name);
    await app.kvManager.disconnect(conn.name);
  }

  /// 「新建数据库」:弹出名称输入对话框,成功后库列表已刷新(树无需额外动作)
  Future<void> _createDatabaseNode(AppState app, ConnectionInfo conn) async {
    await showDialog<bool>(
      context: context,
      builder: (_) => CreateDatabaseDialog(connection: conn),
    );
  }

  // ── 右键菜单「刷新」:仅对已加载出子节点的节点可用 ──

  /// 库 / 模式下的对象列表是否已加载完成(分组行会随之渲染)
  bool _objectsLoaded(
      AppState app, ConnectionInfo conn, String database, String? schema) {
    final state = schema == null
        ? app.connectionManager.tableStateOf(conn.name, database)
        : app.connectionManager.tableStateOf(conn.name, database,
            schema: schema);
    return state.status == LoadStatus.loaded;
  }

  /// 库节点在树中是否有子行:模式层节点,或已加载出的对象分组
  bool _databaseHasChildren(
      AppState app, ConnectionInfo conn, String database) {
    final schemas =
        app.connectionManager.schemaStateOf(conn.name, database).schemas;
    if (schemas.isNotEmpty) return true;
    return _objectsLoaded(app, conn, database, null);
  }

  /// 刷新库节点:重拉模式列表 + 库级对象列表
  Future<void> _refreshDatabaseNode(
      AppState app, ConnectionInfo conn, String database) async {
    if (kSchemaLayerTypes.contains(conn.typeId)) {
      await app.connectionManager.refreshSchemas(conn, database);
    }
    await app.connectionManager.refreshDatabase(conn, database);
  }

  /// 刷新某库 / 模式下的对象列表
  Future<void> _refreshObjectsNode(
      AppState app, ConnectionInfo conn, String database, String? schema) {
    return app.connectionManager
        .refreshDatabase(conn, database, schema: schema);
  }

  // ── 数据库节点右键菜单:打开 / 新建模式 / 删除 / 新建查询 / 转储SQL ──

  /// 右键数据库节点:弹出上下文菜单。
  /// 服务器操作(新建模式 / 删除 / 转储)需连接处于打开状态;
  /// 打开与新建查询仅依赖本地上下文。
  /// 注意:建表入口不在库节点——表属于具体模式(无模式层类型为库本身),
  /// 「新建表」在「表」分组节点右键菜单中提供。
  void _showDatabaseMenu(
    BuildContext context,
    AppState app,
    ConnectionInfo conn,
    String database,
    Offset position,
  ) {
    final isOpen = app.connectionManager.isConnected(conn.name);
    final dbKey = '${conn.name}|$database';
    final isExpanded = _expanded.contains(dbKey);
    // 「打开/关闭」菜单项按打开状态(_opened)显隐,与图标状态保持一致:
    // 打开过 = 显示「关闭」,未打开 = 显示「打开」,与是否折叠无关
    final isOpened = _opened.contains(dbKey);
    final l = context.l10n;
    showContextMenu(
      context,
      position: position,
      items: [
        if (!isOpened)
          MenuItem(
            text: l.ctxOpen,
            onPressed: () => _openDatabaseNode(app, conn, database),
          )
        else
          MenuItem(
            text: l.ctxClose,
            onPressed: () => _closeDatabaseNode(app, conn, dbKey, database),
          ),
        MenuItem(
          text: l.ctxRefresh,
          enabled: isOpen && _databaseHasChildren(app, conn, database),
          onPressed: () => _refreshDatabaseNode(app, conn, database),
        ),
        // 仅支持模式层的类型(PostgreSQL / SQL Server)可新建模式;
        // 需连接存活且数据库已打开(展开),否则模式列表未加载
        if (kSchemaLayerTypes.contains(conn.typeId))
          MenuItem(
            text: l.ctxNewSchema,
            enabled: isOpen && isExpanded,
            onPressed: () => _createSchemaNode(app, conn, database),
          ),
        MenuItem(
          text: l.ctxDelete,
          // 文件型数据库(SQLite / Access)的库节点即文件本身,不可删除
          enabled: isOpen && !_isFileBasedType(conn),
          onPressed: () => _deleteDatabaseNode(app, conn, database),
        ),
        // 编辑数据库:目前仅 PostgreSQL 家族有对应能力(库属性 / 注释 / 扩展)
        if (DdlBuilder.isPgLike(conn.typeId))
          MenuItem(
            text: l.ctxEditDatabase,
            enabled: isOpen,
            onPressed: () => _editDatabaseNode(app, conn, database),
          ),
        MenuSeparator(),
        MenuItem(text: l.ctxNewQuery, onPressed: () => _newQueryNode(app, conn, database)),
        // 命令列界面:打开绑定到该库的文本式控制台(会话按需连接,
        // 连不上或类型不支持时由命令行以 ERROR 行回显,不另弹对话框)
        MenuItem(
          text: l.menuCommandLine,
          onPressed: () => app.openCommandLine(
            connection: conn.name,
            database: database,
          ),
        ),
        // 转储SQL:仅无模式层类型(MySQL 等,database 即 schema)在数据库节点提供;
        // 有模式层的类型(PG / SQL Server)在模式节点右键提供
        if (!kSchemaLayerTypes.contains(conn.typeId)) ...[
          MenuSeparator(),
          MenuItem(
            text: l.ctxDumpSql,
            enabled: isOpen,
            children: [
              MenuItem(text: l.ctxStructureOnly, onPressed: () => _dumpDatabaseNode(app, conn, database)),
            ],
          ),
          // 还原侧入口:与转储配对,把 .sql 脚本逐条执行到该库
          MenuItem(
            text: l.ctxRunSql,
            enabled: isOpen,
            onPressed: () => showRunSqlFileDialog(
              context,
              app: app,
              conn: conn,
              database: database,
            ),
          ),
        ],
      ],
    );
  }

  /// 「新建模式」:仅模式层类型(PostgreSQL / SQL Server)可用,
  /// 成功后模式列表已刷新(树通过 manager 监听重建)
  Future<void> _createSchemaNode(
      AppState app, ConnectionInfo conn, String database) async {
    await showDialog<bool>(
      context: context,
      builder: (_) => CreateSchemaDialog(
        connection: conn,
        database: database,
      ),
    );
  }

  /// 「编辑数据库」:仅 PostgreSQL 家族提供。库名不变(对话框不做 RENAME),
  /// 因此关闭后无需刷新树;注释等变化由详情面板在下次选中时重读。
  Future<void> _editDatabaseNode(
      AppState app, ConnectionInfo conn, String database) async {
    await showDialog<bool>(
      context: context,
      builder: (_) => DatabaseEditDialog(
        connection: conn,
        database: database,
      ),
    );
  }

  // ── 模式节点右键菜单:打开模式 / 关闭模式 / 编辑模式 / 删除模式 ──

  /// 右键模式节点:弹出上下文菜单。
  /// 「打开模式」仅在当前折叠时显示,「关闭模式」仅在当前展开时显示
  /// (按 _expanded 当前展开状态判断,保证菜单操作始终可用);
  /// 编辑模式(仅 PostgreSQL 家族支持重命名)与删除模式需连接处于打开状态
  void _showSchemaMenu(
    BuildContext context,
    AppState app,
    ConnectionInfo conn,
    String database,
    String schema,
    Offset position,
  ) {
    final key = '${conn.name}|$database|$schema';
    final connected = app.connectionManager.isConnected(conn.name);
    // 「打开/关闭」菜单项按打开状态(_opened)显隐,与图标状态保持一致:
    // 打开过 = 显示「关闭模式」,未打开 = 显示「打开模式」,与是否折叠无关
    final isOpened = _opened.contains(key);
    final l = context.l10n;
    showContextMenu(
      context,
      position: position,
      items: [
        if (isOpened)
          MenuItem(
            text: l.ctxCloseSchema,
            onPressed: () =>
                _closeSchemaNode(app, conn, database, key, schema),
          )
        else
          MenuItem(
            text: l.ctxOpenSchema,
            onPressed: () => _openSchemaNode(app, conn, database, schema),
          ),
        MenuItem(
          text: l.ctxRefresh,
          enabled: connected && _objectsLoaded(app, conn, database, schema),
          onPressed: () =>
              _refreshObjectsNode(app, conn, database, schema),
        ),
        MenuSeparator(),
        // SQL Server 无 ALTER SCHEMA RENAME 语法,编辑模式仅 PostgreSQL 家族展示
        if (kRenameSchemaTypes.contains(conn.typeId))
          MenuItem(
            text: l.ctxEditSchema,
            enabled: connected,
            onPressed: () => _renameSchemaNode(app, conn, database, schema),
          ),
        MenuItem(
          text: l.ctxDeleteSchema,
          enabled: connected,
          onPressed: () => _deleteSchemaNode(app, conn, database, schema),
        ),
        MenuSeparator(),
        MenuItem(
          text: l.ctxDumpSql,
          enabled: connected,
          children: [
            MenuItem(text: l.ctxStructureOnly, onPressed: () => _dumpSchemaNode(app, conn, database, schema)),
          ],
        ),
        // 还原侧入口:与转储配对,执行上下文定位到该模式
        MenuItem(
          text: l.ctxRunSql,
          enabled: connected,
          onPressed: () => showRunSqlFileDialog(
            context,
            app: app,
            conn: conn,
            database: database,
            schema: schema,
          ),
        ),
      ],
    );
  }

  /// 「打开模式」:选中模式节点、同步对象页上下文并展开加载对象
  void _openSchemaNode(
      AppState app, ConnectionInfo conn, String database, String schema) {
    final key = '${conn.name}|$database|$schema';
    _selectNode(key);
    app.detailSelection.value = SelectedNode(
      NodeKind.schema,
      schema,
      connection: conn.name,
      database: database,
    );
    app.setObjectContext(conn.name, database, schema: schema);
    if (!_expanded.contains(key)) {
      setState(() {
        _expanded.add(key);
        _opened.add(key);
      });
      app.logTreeAction(context.l10n.openedSchema(schema));
      app.connectionManager.expandSchema(conn, database, schema);
    }
  }

  /// 「关闭模式」:折叠模式节点及其子树,并清理该模式的对象数据——
  /// 清除已打开状态(节点回到灰色未打开态)、管理器对象缓存与树内选中 /
  /// 详情选中,再次打开时重新从数据库拉取。
  /// 若该模式正被中部对象页浏览,同步清空对象上下文(面板回到禁用空态)。
  void _closeSchemaNode(
      AppState app, ConnectionInfo conn, String database, String key, String schema) {
    setState(() {
      _expanded.removeWhere((k) => k == key || k.startsWith('$key|'));
      _opened.removeWhere((k) => k == key || k.startsWith('$key|'));
      final sel = _selectedNode.value;
      if (sel == key || (sel != null && sel.startsWith('$key|'))) {
        _selectedNode.value = null;
      }
      final detail = app.detailSelection.value;
      if (detail != null &&
          detail.connection == conn.name &&
          detail.database == database &&
          detail.schema == schema) {
        app.detailSelection.value = null;
      }
    });
    app.connectionManager.clearSchemaState(conn.name, database, schema);
    if (app.objectConnection == conn.name &&
        app.objectDatabase == database &&
        app.objectSchema == schema) {
      app.clearObjectContext();
    }
    app.logTreeAction(context.l10n.closedSchema(schema));
  }

  /// 「编辑模式」:打开重命名对话框(ALTER SCHEMA ... RENAME TO)
  Future<void> _renameSchemaNode(
      AppState app, ConnectionInfo conn, String database, String schema) async {
    await showDialog<bool>(
      context: context,
      builder: (_) => RenameSchemaDialog(
        connection: conn,
        database: database,
        schema: schema,
      ),
    );
  }

  /// 「删除模式」:确认后 DROP SCHEMA;顺带清理树中该模式子树的展开 / 选中状态
  Future<void> _deleteSchemaNode(
      AppState app, ConnectionInfo conn, String database, String schema) async {
    final l = context.l10n;
    final result = await MessageBox.show(
      context,
      title: l.ctxDeleteSchema,
      message: l.deleteSchemaConfirm(schema),
      type: MessageBoxType.warning,
      buttons: MessageBoxButtons.okCancel,
      okText: l.btnDelete,
    );
    if (result != MessageBoxResult.ok || !mounted) return;
    final key = '${conn.name}|$database|$schema';
    setState(() {
      _expanded.removeWhere((k) => k == key || k.startsWith('$key|'));
      final sel = _selectedNode.value;
      if (sel == key || (sel != null && sel.startsWith('$key|'))) {
        _selectedNode.value = null;
      }
    });
    final outcome = await app.dropSchema(conn, database, schema);
    if (!mounted) return;
    if (!outcome.ok) {
      MessageBox.show(
        context,
        title: l.ctxDeleteSchema,
        message: l.deleteFailedDetail('${outcome.error}'),
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
    }
  }

  /// 「打开」:展开库节点、加载对象列表并同步中部对象页上下文
  void _openDatabaseNode(AppState app, ConnectionInfo conn, String database) {
    final key = '${conn.name}|$database';
    _selectNode(key);
    app.detailSelection.value =
        SelectedNode(NodeKind.database, database, connection: conn.name, database: database);
    app.setObjectContext(conn.name, database);
    if (!_expanded.contains(key)) {
      setState(() {
        _expanded.add(key);
        _opened.add(key);
      });
      app.logTreeAction(context.l10n.openedDatabase(database));
      app.connectionManager.expandDatabase(conn, database);
    }
  }

  /// 「关闭」:折叠库节点及其子树,并清理该库的子树数据——
  /// 清除已打开状态(节点回到灰色未打开态)、管理器缓存(模式 / 对象列表)
  /// 与树内选中 / 详情选中,再次打开时重新从数据库拉取。
  /// 若该库正被中部对象页浏览,同步清空对象上下文(面板回到禁用空态)。
  void _closeDatabaseNode(
      AppState app, ConnectionInfo conn, String key, String database) {
    setState(() {
      _expanded.removeWhere((k) => k == key || k.startsWith('$key|'));
      _opened.removeWhere((k) => k == key || k.startsWith('$key|'));
      final sel = _selectedNode.value;
      if (sel == key || (sel != null && sel.startsWith('$key|'))) {
        _selectedNode.value = null;
      }
      final detail = app.detailSelection.value;
      if (detail != null &&
          detail.connection == conn.name &&
          detail.database == database) {
        app.detailSelection.value = null;
      }
    });
    app.connectionManager.clearDatabaseState(conn.name, database);
    if (app.objectConnection == conn.name && app.objectDatabase == database) {
      app.clearObjectContext();
    }
    app.logTreeAction(context.l10n.closedDatabase(database));
  }

  // ── 对象分组节点右键菜单:刷新 + 按分类的新建入口 ──

  /// 右键对象分组(表 / 视图 / 函数 / 过程 / 用户…)节点。
  /// 「刷新」仅在该分组下有对象时可用;新建表 / 函数 / 过程按分类追加。
  /// 打开/关闭由单击分组完成,不占菜单项。
  void _showObjectGroupMenu(
    BuildContext context,
    AppState app,
    ConnectionInfo conn,
    String database,
    String? schema,
    ObjectCategory category,
    Offset position,
  ) {
    final isOpen = app.connectionManager.isConnected(conn.name);
    final state = schema == null
        ? app.connectionManager.tableStateOf(conn.name, database)
        : app.connectionManager.tableStateOf(conn.name, database,
            schema: schema);
    final hasObjects = _itemsOf(state, category).isNotEmpty;
    final l = context.l10n;
    showContextMenu(
      context,
      position: position,
      items: [
        MenuItem(
          text: l.ctxRefresh,
          enabled: isOpen && hasObjects,
          onPressed: () => _refreshObjectsNode(app, conn, database, schema),
        ),
        if (category == ObjectCategory.table) ...[
          MenuSeparator(),
          MenuItem(
            text: l.ctxNewTable,
            enabled: isOpen,
            onPressed: () =>
                _createTableNode(app, conn, database, schema: schema),
          ),
        ] else if (category == ObjectCategory.function ||
            category == ObjectCategory.procedure) ...[
          MenuSeparator(),
          MenuItem(
            text: l.ctxNewFunction,
            enabled: isOpen,
            onPressed: () => showFunctionWizard(
              context,
              app: app,
              connection: conn.name,
              database: database,
              schema: schema,
              initialCategory: ObjectCategory.function,
            ),
          ),
          MenuItem(
            text: l.ctxNewProcedure,
            enabled: isOpen,
            onPressed: () => showFunctionWizard(
              context,
              app: app,
              connection: conn.name,
              database: database,
              schema: schema,
              initialCategory: ObjectCategory.procedure,
            ),
          ),
        ] else if (category == ObjectCategory.materializedView) ...[
          MenuSeparator(),
          MenuItem(
            text: l.ctxNewMatView,
            enabled: isOpen,
            onPressed: () =>
                _createMatViewNode(app, conn, database, schema: schema),
          ),
        ] else if (category == ObjectCategory.user) ...[
          MenuSeparator(),
          MenuItem(
            text: l.ctxNewRole,
            enabled: isOpen && UserSql.supportsUsers(conn.typeId),
            onPressed: () => _createRoleNode(app, conn, database, schema: schema),
          ),
        ],
      ],
    );
  }

  /// 「新建角色」:在当前库 / 模式上下文打开一个空白的账号设计页。
  /// 名称先在界面上给出自增默认名(账号名必须在打开页面前定下来 ——
  /// 设计页靠它拼 CREATE USER,不能留到保存时再问),保存时才落 DDL。
  void _createRoleNode(
      AppState app, ConnectionInfo conn, String database,
      {String? schema}) {
    app.setObjectContext(conn.name, database, schema: schema);
    app.designUser(
      _defaultRoleName(app, conn.name, database, schema: schema),
      connection: conn.name,
      database: database,
      schema: schema,
      isNew: true,
    );
  }

  /// 自增默认角色名:`role_N`(N 取到不与现有对象重号为止)
  String _defaultRoleName(
      AppState app, String connection, String database,
      {String? schema}) {
    final existing = schema == null
        ? app.connectionManager.tableStateOf(connection, database)
        : app.connectionManager
            .tableStateOf(connection, database, schema: schema);
    final names = existing.users ?? const <String>[];
    for (var i = 1; i <= names.length + 1; i++) {
      final candidate = 'role_$i';
      if (!names.contains(candidate)) return candidate;
    }
    return 'role_${names.length + 1}';
  }

  /// 「新建实体化视图」:在当前库 / 模式上下文打开空白设计页。
  /// 名称同样要在打开页面前定下来(设计页靠它拼 CREATE MATERIALIZED VIEW)。
  void _createMatViewNode(
      AppState app, ConnectionInfo conn, String database,
      {String? schema}) {
    app.setObjectContext(conn.name, database, schema: schema);
    app.designRoutine(
      _defaultMatViewName(app, conn.name, database, schema: schema),
      connection: conn.name,
      database: database,
      category: ObjectCategory.materializedView,
      schema: schema,
      isNew: true,
    );
  }

  /// 自增默认实体化视图名:`matview_N`(N 取到不与现有对象重号为止)
  String _defaultMatViewName(
      AppState app, String connection, String database,
      {String? schema}) {
    final existing = schema == null
        ? app.connectionManager.tableStateOf(connection, database)
        : app.connectionManager
            .tableStateOf(connection, database, schema: schema);
    final names = existing.materializedViews ?? const <String>[];
    for (var i = 1; i <= names.length + 1; i++) {
      final candidate = 'matview_$i';
      if (!names.contains(candidate)) return candidate;
    }
    return 'matview_${names.length + 1}';
  }

  /// 「新建表」:在当前库 / 模式上下文打开表设计器标签页
  void _createTableNode(
      AppState app, ConnectionInfo conn, String database,
      {String? schema}) {
    app.setObjectContext(conn.name, database, schema: schema);
    app.newTableDesigner(connection: conn.name, database: database, schema: schema);
  }

  /// 「删除」:确认后 DROP DATABASE;顺带清理树中该库子树的展开 / 选中状态
  Future<void> _deleteDatabaseNode(
      AppState app, ConnectionInfo conn, String database) async {
    final l = context.l10n;
    final result = await MessageBox.show(
      context,
      title: l.deleteDatabaseTitle,
      message: l.deleteDatabaseConfirm(database),
      type: MessageBoxType.warning,
      buttons: MessageBoxButtons.okCancel,
      okText: l.btnDelete,
    );
    if (result != MessageBoxResult.ok || !mounted) return;
    final key = '${conn.name}|$database';
    setState(() {
      _expanded.removeWhere((k) => k == key || k.startsWith('$key|'));
      final sel = _selectedNode.value;
      if (sel == key || (sel != null && sel.startsWith('$key|'))) {
        _selectedNode.value = null;
      }
    });
    final outcome = await app.dropDatabase(conn, database);
    if (!mounted) return;
    if (!outcome.ok) {
      MessageBox.show(
        context,
        title: l.deleteDatabaseTitle,
        message: l.deleteFailedDetail('${outcome.error}'),
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
    }
  }

  /// 「新建查询」:把对象浏览上下文切到该库并打开查询编辑页
  void _newQueryNode(AppState app, ConnectionInfo conn, String database) {
    app.setObjectContext(conn.name, database);
    app.newQuery();
  }

  /// 「转储SQL文件 → 仅结构」:选择保存位置,生成结构 DDL 并落盘
  Future<void> _dumpDatabaseNode(
      AppState app, ConnectionInfo conn, String database) async {
    final l = context.l10n;
    final location = await getSaveLocation(
      acceptedTypeGroups: [XTypeGroup(label: l.sqlFileTypeLabel, extensions: ['sql'])],
      suggestedName: '${database}_structure.sql',
      confirmButtonText: l.btnSave,
    );
    if (location == null || !mounted) return;

    final sql = await app.dumpDatabaseStructure(conn, database);
    if (sql == null || !mounted) {
      MessageBox.show(
        context,
        title: l.ctxDumpSql,
        message: l.dumpStructureReadFailed,
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
      return;
    }

    try {
      final file = File(location.path);
      await file.writeAsString(sql);
    } catch (e) {
      if (!mounted) return;
      MessageBox.show(
        context,
        title: l.ctxDumpSql,
        message: l.dumpWriteFailed('$e'),
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
      return;
    }
    if (!mounted) return;
    MessageBox.show(
      context,
      title: l.ctxDumpSql,
      message: l.dumpDatabaseDone(database, location.path),
      type: MessageBoxType.info,
      okText: l.btnGotIt,
    );
  }

  /// 「转储SQL文件 → 仅结构」(模式级):选择保存位置,生成该模式下结构 DDL 并落盘
  Future<void> _dumpSchemaNode(
      AppState app, ConnectionInfo conn, String database, String schema) async {
    final l = context.l10n;
    final location = await getSaveLocation(
      acceptedTypeGroups: [XTypeGroup(label: l.sqlFileTypeLabel, extensions: ['sql'])],
      suggestedName: '${schema}_structure.sql',
      confirmButtonText: l.btnSave,
    );
    if (location == null || !mounted) return;

    final sql = await app.dumpDatabaseStructure(conn, database, schema: schema);
    if (sql == null || !mounted) {
      MessageBox.show(
        context,
        title: l.ctxDumpSql,
        message: l.dumpStructureReadFailed,
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
      return;
    }

    try {
      final file = File(location.path);
      await file.writeAsString(sql);
    } catch (e) {
      if (!mounted) return;
      MessageBox.show(
        context,
        title: l.ctxDumpSql,
        message: l.dumpWriteFailed('$e'),
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
      return;
    }
    if (!mounted) return;
    MessageBox.show(
      context,
      title: l.ctxDumpSql,
      message: l.dumpSchemaDone(schema, location.path),
      type: MessageBoxType.info,
      okText: l.btnGotIt,
    );
  }


  /// 打开连接前的密码预检:类型需要密码且未保存时弹窗补录。
  /// 密码写入内存连接;弹窗勾选「保存密码」时同时落盘,重启后无需再输。
  /// 返回携带密码的连接;无需密码或用户取消时返回调用方应中止的信号
  /// (返回值与入参相同对象 = 无需密码;null = 用户取消)。
  Future<ConnectionInfo?> _ensurePassword(
      AppState app, ConnectionInfo conn) async {
    if (!connectionNeedsPassword(conn)) return conn;
    if (conn.password.isNotEmpty) return conn;
    final result =
        await showConnectionPasswordDialog(context, conn: conn);
    if (result == null || !mounted) return null;
    return app.setConnectionPassword(conn, result.password, save: result.save) ??
        conn.copyWith(password: result.password);
  }

  /// 展开 / 刷新连接时按驱动链分流。SQL 与 KV 各持一套管理器与状态字典,
  /// 判据是 [isKvStoreConnection](两侧类型集合互斥,见 kv_driver.dart)。
  static void expandKvOrSql(AppState app, ConnectionInfo conn) {
    if (isKvStoreConnection(conn)) {
      app.kvManager.expandConnection(conn);
    } else {
      app.connectionManager.expandConnection(conn);
    }
  }

  /// 展开连接节点:类型需要密码且未保存时先弹窗补录,确认后再展开节点并
  /// 懒加载库列表。用户取消密码输入(或环境已卸载)时整次展开作废——不展开、
  /// 不标记已打开、不记日志,转圈指示器也随之撤下。
  ///
  /// 「打开中」从触发动作那一刻就开始转(见 [_markOpening]):等密码弹窗的
  /// 时间同样属于打开过程,不该出现「点了没反应」的空档。
  Future<void> _lazyExpandConnection(
      AppState app, ConnectionInfo conn) async {
    _markOpening(conn.name);
    final target = await _ensurePassword(app, conn);
    if (target == null || !mounted) {
      _clearOpening(conn.name);
      return;
    }
    setState(() {
      _pendingOpening.remove(conn.name);
      _expanded.add(conn.name);
      _opened.add(conn.name);
    });
    app.logTreeAction(context.l10n.openedConnection(conn.name));
    expandKvOrSql(app, target);
  }

  /// 「打开连接」:真实连接服务器并获取数据库信息(非仅展开树)。
  /// 即使节点已展开 / 已有库列表缓存,也强制重新通信刷新;
  /// 驱动不存活自动重连。成功树中显示最新库列表,失败弹窗明确报错。
  Future<void> _openConnectionNode(AppState app, ConnectionInfo conn) async {
    _selectNode(conn.name);
    final l = context.l10n;
    if (!hasAnyDriver(conn)) {
      _expandNode(conn.name);
      MessageBox.show(
        context,
        title: l.ctxOpenConnection,
        message: l.openConnectionUnsupported(conn.name, conn.typeId),
        type: MessageBoxType.warning,
        okText: l.btnGotIt,
      );
      return;
    }
    // 需要密码但未保存时先弹窗补录,确认后再展开节点并连接。
    // 用户取消则整次打开作废——不展开、不标记已打开,转圈指示器撤下。
    // 指示器从本动作触发起就开始转(等密码弹窗的时间也算打开中)。
    await _beginOpening(conn.name);
    if (!mounted) return;
    final target = await _ensurePassword(app, conn);
    if (target == null || !mounted) {
      _clearOpening(conn.name);
      return;
    }
    _expandNode(conn.name);
    final (ok, message) = isKvStoreConnection(target)
        ? await app.kvManager.forceExpandConnection(target)
        : await app.connectionManager.forceExpandConnection(target);
    _clearOpening(conn.name);
    if (!mounted) return;
    if (ok) {
      // 连接成功后标记为已打开并记录日志
      setState(() => _opened.add(conn.name));
      app.logTreeAction(l.openedConnection(conn.name));
    } else {
      MessageBox.show(
        context,
        title: l.ctxOpenConnection,
        message: l.openConnectionFailed(message, conn.name),
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
    }
  }

  /// 展开指定节点(幂等);连接节点的展开一律在密码确认之后调用
  void _expandNode(String key) {
    if (!mounted || _expanded.contains(key)) return;
    setState(() => _expanded.add(key));
  }

  /// 「编辑连接」:以现有配置预填连接向导弹窗,确认后更新连接。
  /// 名称变化时迁移展开 / 选中状态,并重新加载元数据(参数可能已变)
  Future<void> _editConnectionNode(AppState app, ConnectionInfo conn) async {
    final result = await showDialog<ConnectionInfo>(
      context: context,
      builder: (_) => ConnectionDialogPage(initial: conn),
    );
    if (result == null || !mounted) return;
    final updated = await app.updateConnection(conn, result);
    if (updated == null || !mounted) return;
    if (updated.name != conn.name) {
      _migrateConnectionKeys(conn.name, updated.name);
      // 右侧详情面板跟随的选中节点同步迁移
      final detail = app.detailSelection.value;
      if (detail != null && detail.connection == conn.name) {
        app.detailSelection.value = SelectedNode(
          detail.kind,
          detail.name,
          connection: updated.name,
          database: detail.database,
          schema: detail.schema,
        );
      }
    }
    if (_expanded.contains(updated.name) && hasAnyDriver(updated)) {
      expandKvOrSql(app, updated);
    }
  }

  /// 连接改名后:迁移该连接子树的展开状态与选中状态到新名前缀
  void _migrateConnectionKeys(String oldName, String newName) {
    final prefix = '$oldName|';
    setState(() {
      final renamed = <String>{
        for (final k in _expanded)
          if (k.startsWith(prefix)) newName + k.substring(oldName.length),
      };
      _expanded
        ..removeWhere((k) => k == oldName || k.startsWith(prefix))
        ..addAll(renamed);
      final sel = _selectedNode.value;
      if (sel == oldName) {
        _selectedNode.value = newName;
      } else if (sel != null && sel.startsWith(prefix)) {
        _selectedNode.value = newName + sel.substring(oldName.length);
      }
    });
  }

  /// 「删除连接」:确认后删除并断开驱动;顺带清理树中该连接的展开状态
  Future<void> _deleteConnectionNode(AppState app, ConnectionInfo conn) async {
    final l = context.l10n;
    final result = await MessageBox.show(
      context,
      title: l.ctxDeleteConnection,
      message: l.deleteConnectionConfirm(conn.name),
      type: MessageBoxType.warning,
      buttons: MessageBoxButtons.okCancel,
      okText: l.btnDelete,
    );
    if (result != MessageBoxResult.ok || !mounted) return;
    setState(() {
      _expanded.removeWhere((k) => k == conn.name || k.startsWith('${conn.name}|'));
      if (_selectedNode.value == conn.name) _selectedNode.value = null;
    });
    await app.removeConnection(conn);
  }

  @override
  void dispose() {
    _listenedManager?.removeListener(_onManagerChanged);
    _listenedKvManager?.removeListener(_onManagerChanged);
    _listenedApp?.treeNavigate.removeListener(_onTreeNavigate);
    _listenedApp?.connectionAdded.removeListener(_onConnectionAdded);
    for (final timer in _minSpinnerTimers.values) {
      timer.cancel();
    }
    _minSpinnerTimers.clear();
    _selectedNode.dispose();
    _treeFocus.dispose();
    _treeScroll.dispose();
    _searchController.dispose();
    _searchFocus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final app = context.read<AppState>();

    final connections =
        context.select<AppState, List<ConnectionInfo>>((a) => a.filteredConnections);
    final groups =
        context.select<AppState, List<ConnGroup>>((a) => a.groups);
    final hasFilter =
        context.select<AppState, bool>((a) =>
            a.selectedDbTypes.isNotEmpty ||
            a.treeSearchText.isNotEmpty);

    final rows = <Widget>[];
    // 表节点改名上下文每次重建时重新登记(只登记当前可见的行)
    _tableNodes.clear();
    // 新增连接定位:行号只对本次 build 有效,先清掉再由装配过程登记
    _revealRowIndex = null;
    // 顶层分段:一条分组都没有时保持原来的平铺(不给既有配置凭空插入一层),
    // 一旦有分组就按分组渲染,未分组的连接沉底平铺在最后一层
    final sections = _connectionSections(connections, groups, hideEmpty: hasFilter);
    if (sections.isEmpty) {
      for (final conn in connections) {
        final start = rows.length;
        rows.addAll(_liveConnectionRows(context, app, conn));
        if (conn.name == _revealConnName) _revealRowIndex = start;
      }
    } else {
      for (final section in sections) {
        final group = section.group;
        if (group == null) {
          // 未分组段也是放置目标:把分组内连接拖到未分组兄弟上 = 移出分组
          for (final conn in section.connections) {
            final start = rows.length;
            for (final row in _liveConnectionRows(context, app, conn)) {
              rows.add(_memberDropTarget(context, app, '', row));
            }
            if (conn.name == _revealConnName) _revealRowIndex = start;
          }
          continue;
        }
        rows.add(_connGroupDropTarget(context, app, group));
        if (_collapsedGroups.contains(group)) continue;
        for (final conn in section.connections) {
          // 组内成员(含其展开的库/表子树)也是放置目标:
          // 拖到兄弟节点上等同拖到分组头上
          final start = rows.length;
          for (final row in _liveConnectionRows(context, app, conn, base: 1)) {
            rows.add(_memberDropTarget(context, app, group, row));
          }
          if (conn.name == _revealConnName) _revealRowIndex = start;
        }
      }
    }

    // 空态:无任何连接 / 筛选无结果
    if (rows.isEmpty) {
      rows.add(
        SizedBox(
          height: 200,
          child: Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(Icons.dns_outlined, size: 28, color: t.disabledForeground),
                const SizedBox(height: 10),
                Text(
                  hasFilter
                      ? context.l10n.noMatchingConnections
                      : context.l10n.noConnectionsYet,
                  style: TextStyle(color: t.mutedForeground, fontSize: 13),
                ),
                if (!hasFilter) ...[
                  const SizedBox(height: 4),
                  Text(
                    context.l10n.clickToolbarNewConnection,
                    style: TextStyle(color: t.disabledForeground, fontSize: 12),
                  ),
                ],
              ],
            ),
          ),
        ),
      );
    }

    final panel = Container(
      color: t.background,
      child: Column(
        children: [
          Expanded(
            // Focus 承载树键盘焦点:点行取得焦点后 F2 就地重命名选中节点
            child: Focus(
              focusNode: _treeFocus,
              onKeyEvent: _onTreeKey,
              child: ListView(controller: _treeScroll, children: rows),
            ),
          ),
          // 拖动分组内连接时:底部出现「移到未分组」放置条
          if (_draggingConn != null && _draggingConn!.group.isNotEmpty)
            _ungroupedDropZone(context, app),
          _buildBottomBar(context, t, hasFilter),
        ],
      ),
    );

    // Ctrl+F / Esc 走这层(见 [_onPanelKey]):它是树与底部搜索栏的共同祖先,
    // 焦点落在哪一侧都能收到冒泡上来的按键
    return Focus(
      canRequestFocus: false,
      onKeyEvent: _onPanelKey,
      child: panel,
    );
  }

  // ── 真实连接树:连接 → 库 → 表,展开时懒加载 ──────────────────

  /// 顶层分段:已登记分组按登记顺序在前(空分组也保留,否则「新建分组」看着像没生效),
  /// 只出现在连接上、未登记的分组随后补上,最后一段是未分组连接(`group: null`)。
  ///
  /// [hideEmpty] 为 true(搜索 / 类型筛选中)时丢掉没有可见连接的分组:
  /// 命中口径仍是连接名,分组头只随子项显隐。
  /// 一条分组都没有时返回空列表,调用方据此保持原有的平铺布局。
  List<({String? group, List<ConnectionInfo> connections})> _connectionSections(
    List<ConnectionInfo> connections,
    List<ConnGroup> groups, {
    required bool hideEmpty,
  }) {
    final sections = <({String? group, List<ConnectionInfo> connections})>[];
    void add(String name) {
      if (sections.any((s) => s.group == name)) return;
      sections.add((
        group: name,
        connections: [for (final c in connections) if (c.group == name) c],
      ));
    }

    for (final g in groups) {
      add(g.name);
    }
    // 连接指向了没有条目的分组(手改配置 / 更早版本遗留):现场补一段,别藏起连接
    for (final c in connections) {
      if (c.group.isNotEmpty) add(c.group);
    }
    if (sections.isEmpty) return const [];
    if (hideEmpty) sections.removeWhere((s) => s.connections.isEmpty);
    final loose = [for (final c in connections) if (c.group.isEmpty) c];
    if (loose.isNotEmpty) sections.add((group: null, connections: loose));
    return sections;
  }

  /// 拖动连接时的浮层预览:品牌图标 + 连接名。
  /// 不用 Material(避免阴影/shader 首次计算延迟),文本显式去下划线。
  Widget _dragFeedback(BuildContext context, ConnectionInfo conn) {
    final t = Tokens.of(context);
    DbType? dbType;
    for (final e in kAllDbTypes) {
      if (e.id == conn.typeId) { dbType = e; break; }
    }
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: t.surface,
        border: Border.all(color: t.accent, width: 1),
        borderRadius: BorderRadius.circular(3),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (dbType != null) DbTypeIcon(type: dbType, size: _treeIconSize),
          const SizedBox(width: 6),
          Text(
            conn.name,
            style: TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w400,
              color: bodyTextColor(context),
              decoration: TextDecoration.none,
            ),
          ),
        ],
      ),
    );
  }

  /// 放置到分组([group] 为空串 = 未分组):移动 + 状态栏日志。
  /// 分组头、组内成员行、未分组兄弟行共用
  void _dropIntoGroup(AppState app, ConnectionInfo conn, String group) {
    if (conn.group == group) return;
    app.moveConnectionToGroup(conn, group);
    final l = context.l10n;
    app.logTreeAction(group.isEmpty
        ? l.movedConnectionToUngrouped(conn.name)
        : l.movedConnectionToGroup(group, conn.name));
  }

  /// 连接分组的放置目标:接受被拖动的连接并移入本分组。
  /// 拖入时高亮分组行(candidateData 非空);拖到已在组内的连接不接收(无效果)。
  Widget _connGroupDropTarget(
    BuildContext context,
    AppState app,
    String group,
  ) {
    return DragTarget<ConnectionInfo>(
      onWillAcceptWithDetails: (details) => details.data.group != group,
      onAcceptWithDetails: (details) =>
          _dropIntoGroup(app, details.data, group),
      builder: (context, candidate, _) =>
          _connGroupRow(context, app, group, dropHighlight: candidate.isNotEmpty),
    );
  }

  /// 成员行的放置目标:拖到分组展开后的任一成员行(连接 / 库 / 表)上,
  /// 等同拖到分组头;[group] 为空串时是未分组段——拖到未分组兄弟上 = 移出分组。
  /// 悬停时该行加强调色边框(DecoratedBox 不改变布局尺寸)。
  Widget _memberDropTarget(
    BuildContext context,
    AppState app,
    String group,
    Widget row,
  ) {
    return DragTarget<ConnectionInfo>(
      onWillAcceptWithDetails: (details) => details.data.group != group,
      onAcceptWithDetails: (details) =>
          _dropIntoGroup(app, details.data, group),
      builder: (context, candidate, _) => candidate.isEmpty
          ? row
          : DecoratedBox(
              decoration: BoxDecoration(
                border: Border.all(color: Tokens.of(context).accent, width: 1),
              ),
              child: row,
            ),
    );
  }

  /// 「移到未分组」放置条:拖动分组内连接时出现在树底部,释放即移出分组。
  /// 与分组行的 DragTarget 平级(不嵌套),避免嵌套目标的落点判定歧义。
  Widget _ungroupedDropZone(BuildContext context, AppState app) {
    final t = Tokens.of(context);
    return DragTarget<ConnectionInfo>(
      onWillAcceptWithDetails: (details) => details.data.group.isNotEmpty,
      onAcceptWithDetails: (details) => _dropIntoGroup(app, details.data, ''),
      builder: (context, candidate, _) {
        final hot = candidate.isNotEmpty;
        return Container(
          height: 30,
          margin: const EdgeInsets.fromLTRB(6, 0, 6, 4),
          padding: const EdgeInsets.symmetric(horizontal: 8),
          decoration: BoxDecoration(
            color: hot ? t.treeSelectedBg : null,
            border: Border.all(color: hot ? t.accent : t.border, width: 1),
            borderRadius: BorderRadius.circular(3),
          ),
          child: Row(
            children: [
              Icon(
                Icons.folder_outlined,
                size: _treeIconSize,
                color: hot ? t.accent : t.mutedForeground,
              ),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  context.l10n.dropToUngroup,
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w400,
                    color: hot ? t.accent : t.mutedForeground,
                    decoration: TextDecoration.none,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ),
        );
      },
    );
  }

  /// 连接分组节点:箭头 = 折叠 / 展开(纯视图状态,不触发任何加载);
  /// 单击 = 选中并在右侧详情显示组内连接数;右键 = 分组菜单。
  /// [dropHighlight] 为 true(有连接正拖到此分组上)时高亮整行作为放置反馈。
  Widget _connGroupRow(
    BuildContext context,
    AppState app,
    String group, {
    bool dropHighlight = false,
  }) {
    final c = AppColors.of(context);
    final key = 'g:$group';
    final expanded = !_collapsedGroups.contains(group);
    return ValueListenableBuilder<String?>(
      valueListenable: _selectedNode,
      builder: (context, sel, _) => _node(
        context,
        key: key,
        depth: 0,
        text: group,
        icon: Icons.folder,
        color: c.iconWarning,
        // 分组图标不随展开/折叠切换,始终用同一个实心文件夹 SVG
        leading: UiIcon(kConnGroupIcon, size: _treeIconSize),
        expanded: expanded,
        selected: sel == key,
        dropHighlight: dropHighlight,
        kind: NodeKind.connGroup,
        onToggle: () => setState(() {
          expanded ? _collapsedGroups.add(group) : _collapsedGroups.remove(group);
        }),
        onSelect: () {
          _selectNode(key);
          app.detailSelection.value = SelectedNode(NodeKind.connGroup, group);
        },
        onContextMenu: (position) =>
            _showConnGroupMenu(context, app, group, position),
        editing: _editingKey == key,
        onCommitRename: (input) => _commitGroupRename(group, input),
      ),
    );
  }

  /// 渲染一个真实连接的整棵子树。
  /// [base] 为该连接节点的层级(顶层 0 / 分组内 1),其下各级依次 +1。
  List<Widget> _liveConnectionRows(
    BuildContext context,
    AppState app,
    ConnectionInfo conn, {
    int base = 0,
  }) {
    final c = AppColors.of(context);
    final manager = app.connectionManager;
    final supported = hasAnyDriver(conn);
    // 缓存型引擎(Redis 等)走 KvConnectionManager 那条独立驱动链;
    // 两侧类型集合互斥,所以这是一个二选一的开关而不是叠加状态
    final isKv = isKvStoreConnection(conn);
    final rows = <Widget>[];

    // 查找连接类型对应的品牌图标定义
    DbType? dbType;
    for (final e in kAllDbTypes) {
      if (e.id == conn.typeId) { dbType = e; break; }
    }

    // 连接图标:品牌图标 + 右下角状态点;离线时品牌色去饱和
    final connected = isKv
        ? app.kvManager.isConnected(conn.name)
        : manager.isConnected(conn.name);
    final connLeading = dbType == null
        ? null
        : DbTypeIcon(type: dbType, size: _treeIconSize, connected: connected);
    // 打开中:库列表正在拉取(展开或右键「打开连接」都会把状态置为 loading),
    // 或已触发打开动作但还在等密码补录(见 [_pendingOpening])。
    // 行首品牌图标整体换成转圈指示器。不看展开态——收起也照常反映进度
    final connLoading = supported &&
        ((isKv
                ? app.kvManager.namespacesOf(conn.name).status
                : manager.databaseStateOf(conn.name).status) ==
            LoadStatus.loading ||
            _pendingOpening.contains(conn.name));

    // 连接节点:展开时触发连接 + 拉取库列表
    final connEditing = _editingKey == conn.name;
    final connRow = ValueListenableBuilder<String?>(
      valueListenable: _selectedNode,
      builder: (context, sel, _) => _node(
        context,
        key: conn.name,
        depth: base,
        text: conn.name,
        icon: Icons.dns,
        color: c.iconInfo,
        expanded: _expanded.contains(conn.name),
        selected: sel == conn.name,
        kind: NodeKind.connection,
        leading: connLeading,
        loading: _showSpinner(conn.name, connLoading),
        // 箭头 / 灰色态跟随真实连接状态:查询页等外部入口打开连接后
        // (驱动已建立,isConnected=true)即使本树未展开过也显示展开箭头;
        // 与右键菜单「已连接=显示关闭连接」的判定口径保持一致
        opened: _opened.contains(conn.name) || connected,
        onToggle: () => _toggleNode(
          app: app,
          conn: conn,
          kind: NodeKind.connection,
          key: conn.name,
        ),
        onSelect: () {
          _selectNode(conn.name);
          app.detailSelection.value =
              SelectedNode(NodeKind.connection, conn.name, connection: conn.name);
        },
        onContextMenu: (position) =>
            _showConnectionMenu(context, app, conn, position),
        editing: connEditing,
        onCommitRename: (input) => _commitConnectionRename(conn.name, input),
      ),
    );
    // 新增连接定位:目标行携带 GlobalKey(post-frame 由 _scrollToReveal 消费)。
    // key 挂在拖拽层**内侧**:拖动开始的重建(child ↔ childWhenDragging 切换)
    // 与定位态清除都会换掉这层包裹,包在外侧会连拖拽手势子树一起拆掉,
    // onDragEnd 不再回访;包在内侧只重建行体,ThresholdDraggable 自身稳定。
    // key 同一时刻只挂在一行上,新增另一条时旧行先失 key 再上新,同帧内不重号
    final Widget connRowRevealed = conn.name == _revealConnName
        ? KeyedSubtree(key: _revealRowKey, child: connRow)
        : connRow;
    // 按住连接行拖动到分组节点即完成分组;拖到底部「移到未分组」条可移出分组。
    // 改名态不参与拖动。用 ThresholdDraggable 而非 Draggable:后者在本行是竞技场
    // 唯一识别器时,pointer down 就被判接受,onDragStarted 立刻触发——选中 /
    // 双击打开连接都会闪一下拖拽浮层与底部放置条(桌面端鼠标拖拽源基本都是这种
    // 唯一成员情形;且框架对鼠标的起手容差只有 1px,手抖也算拖)。
    // ThresholdDraggable 要求位移超过 4px 才起手,选中仍走 _node 内
    // Listener.onPointerDown(按下即选,零延迟),互不干扰。
    rows.add(connEditing
        ? connRowRevealed
        : ThresholdDraggable<ConnectionInfo>(
            data: conn,
            feedback: _dragFeedback(context, conn),
            childWhenDragging: Opacity(opacity: 0.4, child: connRowRevealed),
            onDragStarted: () => setState(() => _draggingConn = conn),
            // onDragEnd 覆盖放置与取消两种结局;onDraggableCanceled 兜底
            onDragEnd: (_) => setState(() => _draggingConn = null),
            onDraggableCanceled: (_, __) => setState(() => _draggingConn = null),
            child: connRowRevealed,
          ));
    if (!_expanded.contains(conn.name)) return rows;

    // 尚未实现驱动的类型:展开仅提示,不发请求
    if (!supported) {
      rows.add(_hintNode(
          context, depth: base + 1, text: context.l10n.driverNotImplemented));
      return rows;
    }

    // 缓存型引擎(Redis 等):连接下直接挂命名空间,没有「库 → 模式 → 对象
    // 分组」那三层关系型结构,键本身也不进树(见 _kvNamespaceRows)
    if (isKv) {
      rows.addAll(_kvNamespaceRows(context, app, conn, base + 1));
      return rows;
    }

    // 库列表:按加载状态渲染
    final dbState = manager.databaseStateOf(conn.name);
    switch (dbState.status) {
      case LoadStatus.idle:
      case LoadStatus.loading:
        // 加载中不挂提示行:进度由连接行的行首图标转圈表达(见 _node 的 loading)
        return rows;
      case LoadStatus.error:
        rows.add(_hintNode(
          context,
          depth: base + 1,
          text: context.l10n.loadFailedDetail('${dbState.error}'),
          isAction: true,
          onTap: () => manager.retryExpandConnection(conn),
        ));
        return rows;
      case LoadStatus.loaded:
        break;
    }

    for (final database in dbState.databases) {
      final dbKey = '${conn.name}|$database';
      // 打开中:该库的模式列表与对象列表正在拉取(expandDatabase 二者一并加载),
      // 行首库图标换成转圈指示器,取代原来挂在库节点下的「加载中...」提示行
      final dbLoading =
          manager.tableStateOf(conn.name, database).status == LoadStatus.loading;
      rows.add(
        ValueListenableBuilder<String?>(
          valueListenable: _selectedNode,
          builder: (context, sel, _) => _node(
            context,
            key: dbKey,
            depth: base + 1,
            text: database,
            icon: Icons.storage,
            color: c.iconSuccess,
            expanded: _expanded.contains(dbKey),
            selected: sel == dbKey,
            kind: NodeKind.database,
            opened: _opened.contains(dbKey),
            // 图标按节点「打开过」状态切换(曾加载数据 / 右键打开 = 彩色,
            // 未打开 / 右键关闭 = 灰阶);「打开」≠ 树节点展开——折叠 / 折叠全部
            // 不清打开状态,图标不变;连接在线与否不影响库图标
            leading: UiIcon(
              _opened.contains(dbKey) ? kDatabaseIcon : kDatabaseClosedIcon,
              size: _treeIconSize,
            ),
            loading: _showSpinner(dbKey, dbLoading),
            onToggle: () => _toggleNode(
              app: app,
              conn: conn,
              kind: NodeKind.database,
              key: dbKey,
              database: database,
            ),
            onSelect: () {
              _selectNode(dbKey);
              app.detailSelection.value = SelectedNode(
                NodeKind.database,
                database,
                connection: conn.name,
                database: database,
              );
              // 单击库节点:仅同步中部对象页浏览上下文(重置到默认模式)。
              // 单击 ≠ 打开——不触发对象列表加载;数据在展开(打开)库/模式节点时才加载,
              // 未打开时对象面板保持空态、操作栏整体禁用
              app.setObjectContext(conn.name, database);
            },
            onContextMenu: (position) =>
                _showDatabaseMenu(context, app, conn, database, position),
          ),
        ),
      );
      if (!_expanded.contains(dbKey)) continue;

      // 模式层:PostgreSQL / SQL Server 等有独立模式层的类型
      // (listSchemas 返回非空)在库与对象分组之间渲染模式节点;
      // MySQL 等 Database 即 Schema 的类型返回空,保持原有三级布局
      final schemaState = manager.schemaStateOf(conn.name, database);
      if (schemaState.status == LoadStatus.loaded &&
          schemaState.schemas.isNotEmpty) {
        for (final schema in schemaState.schemas) {
          final schemaKey = '$dbKey|$schema';
          // 打开中:该模式的对象列表正在拉取,行首模式图标换成转圈指示器
          final schemaLoading = manager
                  .tableStateOf(conn.name, database, schema: schema)
                  .status ==
              LoadStatus.loading;
          rows.add(
            ValueListenableBuilder<String?>(
              valueListenable: _selectedNode,
              builder: (context, sel, _) => _node(
                context,
                key: schemaKey,
                depth: base + 2,
                text: schema,
                icon: Icons.folder_outlined,
                color: c.iconWarning,
                expanded: _expanded.contains(schemaKey),
                selected: sel == schemaKey,
                kind: NodeKind.schema,
                opened: _opened.contains(schemaKey),
                // 图标按节点「打开过」状态切换(曾加载数据 / 右键打开 = 彩色,
                // 未打开 / 右键关闭 = 灰阶);「打开」≠ 树节点展开——折叠 / 折叠全部
                // 不清打开状态,图标不变;连接在线与否不影响模式图标
                leading: UiIcon(
                  _opened.contains(schemaKey) ? kSchemaIcon : kSchemaClosedIcon,
                  size: _treeIconSize,
                ),
                loading: _showSpinner(schemaKey, schemaLoading),
                onToggle: () => _toggleNode(
                  app: app,
                  conn: conn,
                  kind: NodeKind.schema,
                  key: schemaKey,
                  database: database,
                  schema: schema,
                ),
                onSelect: () {
                  _selectNode(schemaKey);
                  app.detailSelection.value = SelectedNode(
                    NodeKind.schema,
                    schema,
                    connection: conn.name,
                    database: database,
                  );
                  // 单击模式节点:仅同步对象页上下文,不触发加载(单击 ≠ 打开)
                  app.setObjectContext(conn.name, database, schema: schema);
                },
                onContextMenu: (position) => _showSchemaMenu(
                    context, app, conn, database, schema, position),
              ),
            ),
          );
          if (!_expanded.contains(schemaKey)) continue;
          rows.addAll(
            _groupRows(context, app, conn, database, schema, schemaKey, base + 3),
          );
        }
      } else {
        // 无模式层:分组直挂库节点下(原布局)
        rows.addAll(_groupRows(context, app, conn, database, null, dbKey, base + 2));
      }
    }
    return rows;
  }

  /// KV 连接的子级:命名空间节点(Redis 的 db0..db15)。
  ///
  /// 节点是**叶子** —— 键不进树:单库几千上万个键,而 SCAN 是游标式的,
  /// 树要「展开时才知有没有子级」就得替浏览页把整轮扫描跑完。键列表与值
  /// 网格都在双击节点后打开的键浏览标签里(那里才有过滤、分页与续扫)。
  /// 行尾的计数是驱动从 DBSIZE / INFO keyspace 一次拿到的现成值,
  /// 取不到就不显示 —— 不为了凑一个数字去扫键空间。
  List<Widget> _kvNamespaceRows(
    BuildContext context,
    AppState app,
    ConnectionInfo conn,
    int depth,
  ) {
    final kv = app.kvManager;
    final state = kv.namespacesOf(conn.name);
    final rows = <Widget>[];
    switch (state.status) {
      case LoadStatus.idle:
      case LoadStatus.loading:
        // 加载中不挂提示行:进度由连接行的行首图标转圈表达(见 _node 的 loading)
        return rows;
      case LoadStatus.error:
        rows.add(_hintNode(
          context,
          depth: depth,
          text: context.l10n.loadFailedDetail('${state.error}'),
          isAction: true,
          onTap: () => kv.retryExpandConnection(conn),
        ));
        return rows;
      case LoadStatus.loaded:
        break;
    }
    final c = AppColors.of(context);
    for (final ns in state.namespaces) {
      final key = '${conn.name}|${ns.id}';
      rows.add(
        ValueListenableBuilder<String?>(
          valueListenable: _selectedNode,
          builder: (context, sel, _) => _node(
            context,
            key: key,
            depth: depth,
            text: ns.keyCount == null ? ns.label : '${ns.label} · ${ns.keyCount}',
            icon: Icons.storage,
            color: c.iconSuccess,
            noArrow: true,
            selected: sel == key,
            kind: NodeKind.kvNamespace,
            onDoubleTapActivate: () => app.openKvKeys(
              connection: conn.name,
              namespaceId: ns.id,
              namespaceLabel: ns.label,
            ),
            onSelect: () {
              _selectNode(key);
              app.detailSelection.value = SelectedNode(
                NodeKind.kvNamespace,
                ns.label,
                connection: conn.name,
                database: ns.id,
              );
            },
          ),
        ),
      );
    }
    return rows;
  }

  /// 对象分组 + 对象行(库直挂分组或模式挂分组共用)。
  /// [schema] 为 null 表示库级(默认模式)上下文;
  /// [parentKey] 为分组 key 的父前缀(库 key 或模式 key),
  /// [groupDepth] 为分组节点深度(无模式层 2 / 有模式层 3)。
  /// 分组(表 / 视图 / 函数 / 用户 / 查询)不是懒加载层级:无箭头、不可展开,
  /// 父级(库 / 模式)展开后对象行始终紧随分组渲染;单击分组仅同步对象面板分类。
  List<Widget> _groupRows(
    BuildContext context,
    AppState app,
    ConnectionInfo conn,
    String database,
    String? schema,
    String parentKey,
    int groupDepth,
  ) {
    final manager = app.connectionManager;
    final rows = <Widget>[];

    // 对象列表(表 / 视图 / 函数):按加载状态渲染
    final objState = schema == null
        ? manager.tableStateOf(conn.name, database)
        : manager.tableStateOf(conn.name, database, schema: schema);
    switch (objState.status) {
      case LoadStatus.idle:
      case LoadStatus.loading:
        // 加载中不挂提示行:对象列表(表 / 视图 / 函数)属父级(库 / 模式)的加载,
        // 进度由父节点行首图标转圈表达(见 _node 的 loading);此处留空
        return rows;
      case LoadStatus.error:
        rows.add(_hintNode(
          context,
          depth: groupDepth,
          text: context.l10n.loadFailedDetail('${objState.error}'),
          isAction: true,
          onTap: () => schema == null
              ? manager.retryExpandDatabase(conn, database)
              : manager.retryExpandSchema(conn, database, schema),
        ));
        return rows;
      case LoadStatus.loaded:
        break;
    }

    // 分组节点:按当前连接类型过滤不支持的分类(如 SQLite 不显示函数/用户)
    for (final group in _groupsForType(conn.typeId)) {
      final groupKey = '$parentKey|${group.category.name}';
      final isExpanded = _expanded.contains(groupKey);
      final groupItems = _itemsOf(objState, group.category);
      rows.add(
        ValueListenableBuilder<String?>(
          valueListenable: _selectedNode,
          builder: (context, sel, _) => _node(
            context,
            key: groupKey,
            depth: groupDepth,
            text: group.category.labelOf(context.l10n),
            icon: group.icon,
            color: _groupColor(context, group.category),
            // 与 Ribbon 分类按钮同源的自绘 SVG
            leading: ObjectCategoryIcon(
              category: group.category,
              size: _treeIconSize,
            ),
            expanded: isExpanded,
            selected: sel == groupKey,
            kind: NodeKind.tableGroup,
            onToggle: () =>
                _toggleGroup(app, groupKey, group.category.labelOf(context.l10n), isExpanded),
            // 空分组(打开后无数据)不显示折叠按钮
            noArrow: groupItems.isEmpty,
            onSelect: () {
              _selectNode(groupKey);
              // 单击分组:仅选中并同步对象面板分类,不展开/折叠
              app.setObjectContext(conn.name, database,
                  category: group.category, schema: schema);
              app.activateTab(AppState.objectsTabKey);
              // 详情面板展示父节点信息(模式层存在时为模式,否则为数据库)
              app.detailSelection.value = schema == null
                  ? SelectedNode(
                      NodeKind.database,
                      database,
                      connection: conn.name,
                      database: database,
                    )
                  : SelectedNode(
                      NodeKind.schema,
                      schema,
                      connection: conn.name,
                      database: database,
                    );
            },
            // 右键菜单:刷新 + 按分类的新建入口
            // (打开/关闭由单击完成,不占菜单)
            onContextMenu: (position) => _showObjectGroupMenu(context, app, conn,
                database, schema, group.category, position),
          ),
        ),
      );
      // 分类级降级:该分组单独读取失败(如「角色」无 mysql.user / pg_roles
      // 读取权限),在分组下给出可点击重试的提示行,而不是显示成空分组
      if (objState.categoryErrorOf(group.category) != null) {
        rows.add(_hintNode(
          context,
          depth: groupDepth + 1,
          text: context.l10n.readFailed,
          isAction: true,
          onTap: () => schema == null
              ? manager.retryExpandDatabase(conn, database)
              : manager.retryExpandSchema(conn, database, schema),
        ));
      }
      // 仅展开时渲染子项
      if (isExpanded) {
        for (final name in groupItems) {
          rows.add(
            _liveObjectNode(
              context,
              app,
              conn,
              database,
              name,
              category: group.category,
              schema: schema,
              depth: groupDepth + 1,
              // 表实例右键:打开 / 删除 / 清空 / 设计 / 转储 / 复制重命名;
              // 视图 / 实体化视图实例右键:打开 / 设计 / 删除 / 转储(按引擎能力裁剪);
              // 函数 / 过程实例右键:设计 / 删除
              onContextMenu: group.category == ObjectCategory.table
                  ? (position) => showTableContextMenu(
                        context: context,
                        app: app,
                        conn: conn,
                        database: database,
                        table: name,
                        schema: schema,
                        position: position,
                      )
                  : group.category == ObjectCategory.view ||
                          group.category == ObjectCategory.materializedView
                      ? (position) => showViewContextMenu(
                          context: context,
                          app: app,
                          category: group.category,
                          conn: conn,
                          database: database,
                          name: name,
                          schema: schema,
                          position: position,
                        )
                      : group.category == ObjectCategory.function ||
                              group.category == ObjectCategory.procedure
                          ? (position) => showRoutineContextMenu(
                              context: context,
                              app: app,
                              category: group.category,
                              conn: conn,
                              database: database,
                              name: name,
                              schema: schema,
                              position: position,
                            )
                          : group.category == ObjectCategory.user
                              ? (position) => showUserContextMenu(
                                  context: context,
                                  app: app,
                                  conn: conn,
                                  database: database,
                                  name: name,
                                  schema: schema,
                                  position: position,
                                )
                              : null,
            ),
          );
        }
      }
    }
    return rows;
  }

  /// 库下的对象分组;按数据库类型过滤不支持的分类。
  /// 图标与 Ribbon 分类按钮一致:自绘 SVG(assets/icons/ui/*)。
  /// 备份分组暂无数据源,暂时隐藏(后续支持备份功能时加回)
  static const _allGroups = <({ObjectCategory category, IconData icon})>[
    (category: ObjectCategory.table, icon: Icons.table_chart_outlined),
    (category: ObjectCategory.view, icon: Icons.visibility_outlined),
    (category: ObjectCategory.materializedView, icon: Icons.auto_awesome_motion),
    (category: ObjectCategory.function, icon: Icons.functions),
    (category: ObjectCategory.procedure, icon: Icons.settings_suggest),
    (category: ObjectCategory.user, icon: Icons.person_outline),
    (category: ObjectCategory.query, icon: Icons.description_outlined),
    // (category: ObjectCategory.backup, icon: Icons.restore),
  ];

  /// 按数据库类型过滤后的分组列表:仅返回该类型支持的分类。
  /// 例如 SQLite / Access 不显示「函数」「用户」分组。
  static List<({ObjectCategory category, IconData icon})> _groupsForType(
      String typeId) {
    return _allGroups
        .where((g) => isCategorySupportedForType(typeId, g.category.name))
        .toList();
  }

  /// 分组节点图标颜色:表 / 视图 / 实体化视图 / 函数 / 过程用表色,查询用文件夹色,备份用次要色
  Color _groupColor(BuildContext context, ObjectCategory category) {
    final c = AppColors.of(context);
    final t = Tokens.of(context);
    return switch (category) {
      ObjectCategory.table ||
      ObjectCategory.view ||
      ObjectCategory.materializedView ||
      ObjectCategory.function ||
      ObjectCategory.procedure => c.iconPrimary,
      ObjectCategory.user => c.iconSecondary,
      ObjectCategory.query => c.iconWarning,
      ObjectCategory.backup => t.mutedForeground,
    };
  }

  /// 分组下的子项列表(查询 / 备份尚未实现,暂无子项);
  /// 列表字段空安全兑底:热重载后旧实例的 views/functions 可能为 null
  List<String> _itemsOf(TableListState state, ObjectCategory category) {
    return switch (category) {
      ObjectCategory.table => state.tables ?? const [],
      ObjectCategory.view => state.views ?? const [],
      ObjectCategory.materializedView => state.materializedViews ?? const [],
      ObjectCategory.function => state.functions ?? const [],
      ObjectCategory.procedure => state.procedures ?? const [],
      ObjectCategory.user => state.users ?? const [],
      ObjectCategory.query => const [],
      ObjectCategory.backup => const [],
    };
  }

  /// 真实对象节点(表 / 视图 / 函数):单击选中(零延迟);
  /// 双击表 / 视图打开前 100 行数据页。
  /// [schema] 为该对象所属模式(无模式层类型为 null);
  /// [depth] 为节点深度(无模式层 3 / 有模式层 4),决定缩进
  Widget _liveObjectNode(
    BuildContext context,
    AppState app,
    ConnectionInfo conn,
    String database,
    String name, {
    required ObjectCategory category,
    String? schema,
    required int depth,
    void Function(Offset position)? onContextMenu,
  }) {
    final t = Tokens.of(context);
    final key = schema == null
        ? '${conn.name}|$database|${category.name}|$name'
        : '${conn.name}|$database|$schema|${category.name}|$name';
    // 表节点登记改名上下文(F2 需要 连接/库/模式 才能落 DDL);
    // 视图 / 函数等无改名 API,不登记
    if (category == ObjectCategory.table) {
      _tableNodes[key] = (conn: conn, database: database, schema: schema);
    }
    final editing = _editingKey == key;
    // Listener.onPointerDown 立即选中(零延迟);
    // 双击单独走 GestureDetector,避免单击被双击判定窗口 hold;
    // 右键触发上下文菜单(表实例支持 打开 / 删除 / 清空 / 设计 / 转储 / 复制重命名)
    return ValueListenableBuilder<String?>(
      valueListenable: _selectedNode,
      builder: (context, sel, _) => Listener(
        behavior: HitTestBehavior.opaque,
        onPointerDown: (event) {
          if (editing) return;
          if (_editingKey != null) {
            // 点了别的行:让编辑器失焦提交(本行照常选中)
            FocusManager.instance.primaryFocus?.unfocus();
          }
          _selectNode(key);
          if (!_treeFocus.hasFocus) _treeFocus.requestFocus();
          if (category == ObjectCategory.table ||
              category == ObjectCategory.view ||
              category == ObjectCategory.materializedView) {
            app.detailSelection.value = SelectedNode(
              NodeKind.table,
              name,
              connection: conn.name,
              database: database,
              schema: schema,
            );
          }
          if (event.buttons == kSecondaryMouseButton) {
            onContextMenu?.call(event.position);
          }
        },
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onDoubleTap: editing
              ? null
              : switch (category) {
            ObjectCategory.table ||
            ObjectCategory.view ||
            ObjectCategory.materializedView => () => app.openTable(
                  name,
                  connection: conn.name,
                  database: database,
                  schema: schema,
                ),
            ObjectCategory.function ||
            ObjectCategory.procedure => () => app.designRoutine(
                  name,
                  connection: conn.name,
                  database: database,
                  category: category,
                  schema: schema,
                ),
            ObjectCategory.user => () => app.designUser(
                  name,
                  connection: conn.name,
                  database: database,
                  schema: schema,
                ),
            _ => null,
          },
          child: Container(
            height: 24,
            color: sel == key ? t.treeSelectedBg : null,
            // 无箭头占位:深度缩进 + 箭头宽(20)+ 间隙(4)对齐父级行文本
            padding: EdgeInsets.only(left: 4 + depth * 14.0 + 20 + 4),
            child: Row(
              children: [
                _objectIcon(category),
                const SizedBox(width: 6),
                Expanded(
                  child: editing
                      ? InlineEditor(
                          initialValue: name,
                          height: 24,
                          contentPadding: const EdgeInsets.symmetric(horizontal: 2),
                          selectAll: true,
                          onCommit: (input) =>
                              _commitTableRename(conn, database, schema, name, input),
                          onCancel: _cancelEditing,
                        )
                      : Text(
                          name,
                          style: TextStyle(
                            fontSize: 12,
                            color: bodyTextColor(context),
                          ),
                          overflow: TextOverflow.ellipsis,
                        ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// 对象项图标(表 / 视图 / 实体化视图 / 函数 / 角色 / 查询):
  /// 与对应分组节点同源的自绘 SVG(ObjectCategoryIcon)
  Widget _objectIcon(ObjectCategory category) =>
      ObjectCategoryIcon(category: category, size: _objectIconSize);

  /// 错误 / 不可用提示节点(加载中已改由节点行首图标转圈表达,不再走这里);
  /// isAction 为 true 时点击可重试
  Widget _hintNode(
    BuildContext context, {
    required int depth,
    required String text,
    bool isAction = false,
    VoidCallback? onTap,
  }) {
    final t = Tokens.of(context);
    Widget row = Row(
      children: [
        if (isAction)
          Icon(Icons.error_outline, size: 14, color: t.mutedForeground)
        else
          const SizedBox(width: 14),
        const SizedBox(width: 6),
        Expanded(
          child: Text(
            text,
            style: TextStyle(fontSize: 12, color: t.mutedForeground),
            overflow: TextOverflow.ellipsis,
          ),
        ),
        if (isAction)
          Text(
            context.l10n.clickToRetry,
            style: TextStyle(fontSize: 12, color: t.accent),
          ),
      ],
    );

    if (isAction && onTap != null) {
      row = GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onTap,
        child: row,
      );
    }

    return Container(
      height: 24,
      padding: EdgeInsets.only(left: 4.0 + depth * 14 + 20 + 4 + 6),
      child: row,
    );
  }

  Widget _buildBottomBar(
    BuildContext context,
    AppPalette t,
    bool hasFilter,
  ) {
    final app = context.read<AppState>();
    final l = context.l10n;
    final selectedDbTypes =
        context.select<AppState, Set<String>>((a) => a.selectedDbTypes);

    // 与「新建连接」对话框一致:未实现驱动的类型禁用并标注「未实现」
    final options = [
      for (final e in kAllDbTypes)
        (
          option: CheckOption(
            id: e.id,
            label: e.label,
            leading: Opacity(
              opacity: isEngineTypeSupported(e.id) ? 1.0 : 0.35,
              child: DbTypeIcon(type: e, size: 16),
            ),
            selected: selectedDbTypes.contains(e.id),
            onToggle: isEngineTypeSupported(e.id)
                ? () => app.toggleDbTypeFilter(e.id)
                : null,
          ),
          supported: isEngineTypeSupported(e.id),
        ),
    ];

    return Container(
      height: 32,
      decoration: BoxDecoration(
        border: Border(top: BorderSide(color: t.border, width: 1)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Padding(
              padding: const EdgeInsets.only(left: 6, top: 3, bottom: 3),
              child: SearchInput(
                controller: _searchController,
                // 焦点节点交给 State:Ctrl+F 进来、Esc 出去都由 [_onPanelKey] 调度
                focusNode: _searchFocus,
                hintText: l.searchConnectionsHint,
                // 清空搜索(× 内部会补发一次 onChanged('') / 退格删空)视作
                // 「关闭筛选」:滚回当前操作的连接位置
                onChanged: (text) {
                  app.setTreeSearchText(text);
                  if (text.isEmpty) _revealSelectedConnection(app);
                },
                onCleared: () => app.setTreeSearchText(''),
              ),
            ),
          ),
          const SizedBox(width: 4),
          Padding(
            padding: const EdgeInsets.only(right: 4),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Popover(
                  width: 240,
                  padding: const EdgeInsets.all(10),
                  // 开合由外层 Popover 的 Listener 处理,按钮仅提供视觉态
                  trigger: IconBtn(
                    icon: Icons.filter_list,
                    iconSize: 15,
                    color: t.disabledForeground,
                    selected: hasFilter,
                    selectedColor: t.accent,
                    // 幽灵风格:无边框,仅 hover / 选中态着色
                    size: const Size(26, 26),
                    onTap: () {},
                  ),
                  content: Builder(
                    builder: (context) {
                      final dt = TokenScope.maybeOf(context) ?? DesktopTokens.winForm;
                      return Column(
                        mainAxisSize: MainAxisSize.min,
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Row(
                            children: [
                              Icon(Icons.tune, size: 15, color: t.disabledForeground),
                              const SizedBox(width: 6),
                              Text(
                                l.dbTypeFilter,
                                style: TextStyle(
                                  fontFamily: dt.fontFamily,
                                  fontSize: dt.fontSize,
                                  fontWeight: FontWeight.w600,
                                  color: t.foreground,
                                  decoration: TextDecoration.none,
                                ),
                              ),
                            ],
                          ),
                          const SizedBox(height: 8),
                          SizedBox(
                            height: 220,
                            child: SingleChildScrollView(
                              child: Column(
                                children: [
                                  for (final o in options)
                                    CheckRow(
                                      option: o.option,
                                      enabled: o.supported,
                                      trailing: o.supported
                                          ? null
                                          : Text(
                                              l.notImplemented,
                                              style: TextStyle(
                                                fontFamily: dt.fontFamily,
                                                fontSize: 11,
                                                color: t.disabledForeground,
                                                decoration: TextDecoration.none,
                                              ),
                                            ),
                                    ),
                                ],
                              ),
                            ),
                          ),
                          const SizedBox(height: 6),
                          GestureDetector(
                            behavior: HitTestBehavior.opaque,
                            onTap: () {
                              _searchController.clear();
                              app.clearTreeFilter();
                              // controller.clear() 不走 onChanged,这里单独定位
                              _revealSelectedConnection(app);
                            },
                            child: Text(
                              l.clearAllFilters,
                              style: TextStyle(
                                fontFamily: dt.fontFamily,
                                fontSize: dt.fontSize,
                                color: t.accent,
                                decoration: TextDecoration.none,
                              ),
                            ),
                          ),
                        ],
                      );
                    },
                  ),
                ),
                const SizedBox(width: 4),
                IconBtn(
                  icon: Icons.unfold_less,
                  iconSize: 15,
                  color: t.mutedForeground,
                  tooltip: l.collapseAll,
                  size: const Size(26, 26),
                  // 无展开节点时禁用(onTap 为 null → 灰显不可点)。分组默认展开,
                  // 所以「还有可折叠的东西」= 有展开的连接层级 或 还有未折叠的分组
                  onTap: (_expanded.isEmpty &&
                          _collapsedGroups.length >= app.groups.length)
                      ? null
                      : () => _collapseAll(app),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _node(
    BuildContext context, {
    required String key,
    required int depth,
    required String text,
    required IconData icon,
    required Color color,
    required VoidCallback onSelect,
    required NodeKind kind,
    bool expanded = false,
    VoidCallback? onToggle,
    // 叶子节点的「双击进入」动作(如 KV 命名空间 → 打开键浏览页)。
    // 传了它,行体双击就取代「切换子树」—— 叶子没有子树可切,
    // 但仍要能双击进入(noArrow 只关掉箭头热区,不该连进入一起没了)
    VoidCallback? onDoubleTapActivate,
    // 无展开功能的节点(分组):隐藏箭头(保留箭头宽占位对齐文本)、双击无动作
    bool noArrow = false,
    // 节点是否已被打开过:未打开的节点隐藏箭头并以灰色显示,
    // 打开后恢复箭头和正常颜色。分组节点(noArrow)不受此参数影响
    bool opened = true,
    bool selected = false,
    // 拖动放置反馈:有连接正拖到本行(分组)上方时,用强调色边框高亮整行
    bool dropHighlight = false,
    Widget? leading,
    // 打开中(正在拉取子级):行首图标整体换成转圈指示器(Navicat 风格)。
    // 取代过去挂在节点下的「加载中...」提示行——加载进度体现在节点自身上,
    // 不额外占一行,也不会在「展开了但加载没发起」时留下永久提示
    bool loading = false,
    void Function(Offset position)? onContextMenu,
    // 就地改名态:文本换成 base-ui InlineEditor(Enter / 失焦提交,Esc 取消)
    bool editing = false,
    ValueChanged<String>? onCommitRename,
  }) {
    final t = Tokens.of(context);
    // 行左缩进 + 箭头 20px 热区(与下方占位宽度一致)
    final arrowLeft = 4.0 + depth * 14;
    // leading 槽:加载中优先换成转圈指示器;否则用调用方按打开/关闭状态传好的
    // 图标(连接 ON/OFF、库 DATABASE/CLOSE、模式 SCHEMA_ON/CLOSE);再否则回退到
    // icon + color。槽宽固定为 _treeIconSize,换图标不影响文本对齐
    final Widget leadingSlot = loading
        ? const SizedBox(
            width: _treeIconSize,
            height: _treeIconSize,
            child: Spinner(size: _treeIconSize),
          )
        : (leading ??
            Icon(
              icon,
              size: _treeIconSize,
              color: (!noArrow && !opened) ? t.mutedForeground : color,
            ));
    return Listener(
      behavior: HitTestBehavior.opaque,
      onPointerDown: (event) {
        // 编辑中的行只响应文本框本身(光标定位归输入框)
        if (editing) return;
        if (_editingKey != null) {
          // 点了别的行:让编辑器失焦提交(本行照常选中)
          FocusManager.instance.primaryFocus?.unfocus();
        }
        onSelect();
        if (!_treeFocus.hasFocus) _treeFocus.requestFocus();
        // 右键:选中节点并弹出上下文菜单(不参与双击 / 展开判定)
        if (event.buttons == kSecondaryMouseButton) {
          onContextMenu?.call(event.position);
          return;
        }
        final now = event.timeStamp.inMilliseconds;
        final dx = event.localPosition.dx;
        // 箭头:按下瞬间即展开 / 收起(零延迟,不走 tap-up)
        final inArrow = onToggle != null &&
            !noArrow &&
            opened &&
            dx >= arrowLeft &&
            dx < arrowLeft + 20;
        // 行体双击整行:叶子节点(传了 [onDoubleTapActivate])进入该对象,
        // 其余走「切换子树」(与箭头同一逻辑),手动判定不拖累单击
        final bodyDouble = key == _lastTapKey &&
            now - _lastTapMs <= _doubleTapWindowMs &&
            (onDoubleTapActivate != null || (onToggle != null && !noArrow));
        if (bodyDouble && onDoubleTapActivate != null) {
          onDoubleTapActivate();
        } else if (inArrow || bodyDouble) {
          onToggle?.call();
        }
        _lastTapKey = key;
        _lastTapMs = now;
      },
      child: Container(
        height: 26,
        decoration: BoxDecoration(
          color: selected ? t.treeSelectedBg : null,
          border: dropHighlight ? Border.all(color: t.accent, width: 1) : null,
        ),
        padding: EdgeInsets.only(left: arrowLeft),
        child: Row(
          children: [
            if (noArrow)
              // 无箭头占位:与展开节点箭头宽(20)一致,保证同级文本对齐
              const SizedBox(width: 20, height: 26)
            else if (!opened)
              // 未打开:隐藏箭头,保留占位宽度对齐文本
              const SizedBox(width: 20, height: 26)
            else
              // 箭头本体不挂任何手势:热区判定在行级 Listener 完成,
              // tap 即切、无 ~300ms 双击竞技延迟
              SizedBox(
                width: 20,
                height: 26,
                child: Icon(
                  expanded ? Icons.expand_more : Icons.chevron_right,
                  size: 14,
                  color: t.disabledForeground,
                ),
              ),
            const SizedBox(width: 4),
            // leadingSlot 已按打开/关闭状态与加载中状态定好:
            // 加载中 → 转圈指示器;打开/关闭 → 调用方传入的 ON/OFF 图标
            // (如连接 ON/OFF、库 DATABASE/CLOSE、模式 SCHEMA_ON/CLOSE),
            // 不再叠加半透明灰显
            leadingSlot,
            const SizedBox(width: 6),
            Expanded(
              child: editing
                  ? InlineEditor(
                      initialValue: text,
                      height: 26,
                      contentPadding: const EdgeInsets.symmetric(horizontal: 2),
                      selectAll: true,
                      onCommit: (input) => onCommitRename?.call(input),
                      onCancel: _cancelEditing,
                    )
                  : Text(
                      text,
                      style: TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.w400,
                        // 正文一律纯黑(明亮主题);未打开节点仅隐藏箭头 / 图标灰显,
                        // 文字不再降灰
                        color: bodyTextColor(context),
                      ),
                      overflow: TextOverflow.ellipsis,
                    ),
            ),
          ],
        ),
      ),
    );
  }
}
