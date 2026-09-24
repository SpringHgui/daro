import 'dart:async';

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:re_editor/re_editor.dart';
import 'package:re_highlight/languages/sql.dart';
import '../app/app_state.dart';
import '../app/connection_manager.dart';
import '../data/db_data.dart';
import '../data/db_types.dart';
import '../data/drivers/db_driver.dart';
import '../data/result_grid_layout.dart';
import '../data/sql_completions.dart';
import '../data/sql_split.dart';
import '../theme/app_theme.dart';
import 'result_export_dialog.dart';

/// 结果网格行高(与表数据页一致)
const double _rowHeight = 22.0;

/// 结果网格数据单元格的字号与左右内边距
/// (自适应列宽要按同一套数值测量,否则量出来的宽度和实际渲染对不上)
/// 内边距取 6 而非 base-ui 默认的 12:密集网格下 12 会白吃掉窄列的近半宽度。
const double _cellFontSize = 12.5;
const double _cellPaddingX = 6.0;

/// 行号列宽
const double _numberColWidth = 44.0;

/// 查询结果行数上限(超出截断并在结果面板提示)
const int _resultLimit = 1000;

/// 「加载更多」累积上限:超过此数不再续取,防止用户在千万行表上
/// 反复点「加载更多」把客户端内存吃光(Navicat 亦有类似机制)。
/// 达到上限后引导用户走「导出结果」流式落盘,不再驻留 UI 内存
const int _maxAccumulatedRows = 50000;

/// 错误提示色(功能强调色,主题无关,同工具栏「停止」)
const Color _errorColor = Color(0xffd93025);

/// 单条语句的执行结果(结果面板每条语句一个 tab,同 pgAdmin)。
/// [result] 与 [error] 互斥:成功取结果(写操作结果无列),失败取错误文本
class _StatementOutcome {
  _StatementOutcome({
    required this.sql,
    this.result,
    this.error,
    required this.elapsedMs,
  });

  /// 语句文本(消息 tab 回显用)
  final String sql;

  /// 成功时的执行结果(「加载更多」续取后替换为累积更多行的新结果)
  QueryResult? result;

  /// 失败时的错误信息
  final String? error;

  /// 本条语句耗时(ms)
  final int elapsedMs;

  bool get isError => error != null;
}

/// 查询编辑页:工具栏 + 运行上下文栏(连接/库下拉 + 运行/停止/解释)+
/// 带行号的 SQL 编辑区 + 结果面板。
/// 运行/解释通过 ConnectionManager 走真实驱动,SQL 作用于运行
/// 上下文所选的库(新建时自动关联当前对象浏览上下文,见 AppState.newQuery)
class QueryPage extends StatefulWidget {
  const QueryPage({
    super.key,
    required this.title,
    this.connection,
    this.database,
    this.schema,
  });

  final String title;

  /// 关联的连接名(新建查询时取当时的活动连接;可为空)
  final String? connection;

  /// 关联的数据库(可为空)
  final String? database;

  /// 关联的模式(PostgreSQL 等有模式层的类型;可为空)
  final String? schema;

  @override
  State<QueryPage> createState() => _QueryPageState();
}

class _QueryPageState extends State<QueryPage> {
  final CodeLineEditingController _controller =
      CodeLineEditingController.fromText('');
  final FocusNode _focusNode = FocusNode();

  /// SQL 补全构建器(schema 感知,跟随运行上下文刷新数据源)
  final SqlPromptsBuilder _promptsBuilder = SqlPromptsBuilder();

  /// 正在执行查询
  bool _running = false;

  /// 最近一次执行是否被「停止」中断(状态栏文案区分用)
  bool _stopped = false;

  /// 最近一次执行的脚本语句总数
  int _totalStatements = 0;

  /// 最近一次执行各语句的结果(逐条一个结果 tab)
  List<_StatementOutcome> _outcomes = [];

  /// 结果面板当前活动 tab(0 = 消息,i+1 = 结果 i)
  int _panelTabIndex = 0;

  /// 正在「加载更多」续取的结果 tab 索引(null = 无在途续取)
  int? _loadingMoreIndex;

  /// 提示信息(运行前提示,如「请输入 SQL 语句」)
  String? _message;

  /// 最近一次执行总耗时(ms)
  int _elapsedMs = 0;

  /// 执行计时器(运行循环结束与「停止」时读取)
  final Stopwatch _runStopwatch = Stopwatch();

  /// 执行代次:标签切换/停止时作废进行中的请求,防止旧结果覆盖新 tab
  int _runGeneration = 0;

  /// 本次运行占用的服务端会话 id(取不到为 null);「停止」据此带外取消
  int? _activeSessionId;

  /// 编辑区是否有选中文本(运行按钮据此切换「运行/运行已选择的」)
  bool _hasSelection = false;

  /// 运行上下文:当前连接/库/模式(可在上下文栏下拉切换,初始取 tab 关联值)
  String? _connection;
  String? _database;
  String? _schema;

  /// 上下文栏下拉的弹层控制器
  final OverlayController _connMenu = OverlayController();
  final OverlayController _dbMenu = OverlayController();
  final OverlayController _schemaMenu = OverlayController();

  /// ConnectionManager 监听:库列表加载状态变化时刷新数据库下拉弹层
  ConnectionManager? _listenedManager;

  /// didChangeDependencies 中缓存的 AppState,供 dispose / 文本监听器安全引用
  AppState? _appState;

  /// 同类 tab 切换会复用本 State,SQL 文本按 tab key 存入 AppState
  String get _tabKey => _tabKeyOf(widget);

  static String _tabKeyOf(QueryPage w) =>
      AppState.queryTabKey(w.connection, w.database, w.title);

  @override
  void initState() {
    super.initState();
    _connection = widget.connection;
    _database = widget.database;
    _schema = widget.schema;
    _controller.text = context.read<AppState>().queryTextFor(_tabKey);
    _controller.addListener(_onEditorChanged);
    // 补全构建器读取整篇 SQL:解析 `FROM ... AS 别名`(别名可能在其他行)
    _promptsBuilder.sqlTextOf = () => _controller.text;
    _syncPromptContext();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _appState = context.read<AppState>();
    final manager = _appState!.connectionManager;
    if (_listenedManager != manager) {
      _listenedManager?.removeListener(_onManagerChanged);
      _listenedManager = manager;
      manager.addListener(_onManagerChanged);
    }
    _syncPromptContext();
  }

  /// 运行上下文(连接/库/模式)变化时刷新补全数据源
  void _syncPromptContext() {
    final app = _appState ?? context.read<AppState>();
    _promptsBuilder.updateContext(
      app.connectionManager,
      app.connectionByName(_connection),
      _database,
      schema: _schema,
    );
  }

  /// 库列表加载状态变化时重建(数据库下拉弹层跟随刷新)
  void _onManagerChanged() {
    if (mounted) setState(() {});
  }

  /// 输入即持久化;选区出现/消失时重建(运行按钮切换「运行/运行已选择的」)
  void _onEditorChanged() {
    _appState?.updateQueryText(_tabKey, _controller.text);
    final hasSelection = _selectedSql != null;
    if (hasSelection != _hasSelection) {
      _hasSelection = hasSelection;
      if (mounted) setState(() {});
    }
  }

  @override
  void didUpdateWidget(QueryPage oldWidget) {
    super.didUpdateWidget(oldWidget);
    final changed = oldWidget.title != widget.title ||
        oldWidget.connection != widget.connection ||
        oldWidget.database != widget.database ||
        oldWidget.schema != widget.schema;
    if (changed) {
      final app = _appState ?? context.read<AppState>();
      // 先保存旧 tab 文本再载入新 tab(同类型 State 复用,不会重跑 initState)
      app.updateQueryText(_tabKeyOf(oldWidget), _controller.text);
      _controller.text = app.queryTextFor(_tabKey);
      _runGeneration++;
      setState(() {
        _running = false;
        _stopped = false;
        _outcomes = [];
        _panelTabIndex = 0;
        _message = null;
        // 运行上下文重置为新 tab 的关联连接/库/模式
        _connection = widget.connection;
        _database = widget.database;
        _schema = widget.schema;
      });
      _syncPromptContext();
    }
  }

  @override
  void dispose() {
    // 先解绑文本监听,避免 controller 后续通知触发 _onEditorChanged(deactivated 态无法 context.read)
    _controller.removeListener(_onEditorChanged);
    _listenedManager?.removeListener(_onManagerChanged);
    _connMenu.dispose();
    _dbMenu.dispose();
    _schemaMenu.dispose();
    // controller 销毁前再持久化一次当前文本(用 didChangeDependencies 缓存的引用,不可走 context.read)
    _appState?.updateQueryText(_tabKey, _controller.text);
    _controller.dispose();
    _focusNode.dispose();
    super.dispose();
  }

  // ── 工具栏动作 ────────────────────────────────────────────

  /// 选中的 SQL 文本(无选中时为 null)
  String? get _selectedSql {
    final selection = _controller.selection;
    if (selection.isCollapsed) return null;
    final text = _controller.selectedText.trim();
    return text.isEmpty ? null : text;
  }

  /// 待执行 SQL:优先取选中文本,无选中时取全部
  String get _effectiveSql => _selectedSql ?? _controller.text.trim();

  /// 保存:将编辑区 SQL 存入查询管理面板(归属当前运行上下文的 连接|库,
  /// 同名覆盖;「无标题」查询先弹命名对话框,保存后 tab 重命名为查询名)。
  /// 之后可在对象面板「查询」分类中双击重新打开。
  Future<void> _onSave() async {
    final app = _appState ?? context.read<AppState>();
    var name = widget.title;
    if (name.startsWith('无标题-查询')) {
      final input = await _promptQueryName();
      if (input == null || input.isEmpty) return; // 用户取消 / 未输入
      name = input;
    }
    app.saveQuery(
      name: name,
      sql: _controller.text,
      connection: _connection,
      database: _database,
      tabTitle: widget.title,
    );
    if (!mounted) return;
    // 保存可能触发 tab 重命名(didUpdateWidget 会重置结果面板提示),
    // 用模态弹窗确认更可靠
    await MessageBox.show(
      context,
      title: '保存查询',
      message: '已保存查询「$name」,可在对象面板「查询」分类中管理。',
      type: MessageBoxType.info,
      okText: '知道了',
    );
  }

  /// 命名对话框:为「无标题」查询输入保存名称(取消返回 null)。
  /// 同名查询(同 连接|库)将被覆盖。
  Future<String?> _promptQueryName() async {
    final t = Tokens.read(context);
    final controller = TextEditingController();
    final focusNode = FocusNode();
    // 弹层打开后自动聚焦输入框
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (focusNode.context != null) focusNode.requestFocus();
    });
    final result = await MessageBox.show(
      context,
      title: '保存查询',
      content: Material(
        // 透明 Material 宿主:root overlay 弹层无 Material 祖先,
        // 而 base-ui [Input] 内嵌的 TextField 需要它
        type: MaterialType.transparency,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(2, 8, 2, 8),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                '输入查询名称(同名查询将被覆盖):',
                style: TextStyle(
                  fontSize: 12.5,
                  color: t.mutedForeground,
                  decoration: TextDecoration.none,
                ),
              ),
              const SizedBox(height: 8),
              Input(
                controller: controller,
                focusNode: focusNode,
                hint: '查询名称',
              ),
            ],
          ),
        ),
      ),
      buttons: MessageBoxButtons.okCancel,
      okText: '保存',
    );
    final name = controller.text.trim();
    controller.dispose();
    focusNode.dispose();
    return result == MessageBoxResult.ok ? name : null;
  }

  /// 美化 SQL:关键字大写,主要子句前换行,多余空白折叠
  void _onFormat() {
    final text = _controller.text;
    if (text.trim().isEmpty) return;
    _controller.text = formatSql(text);
  }

  /// 运行:对运行上下文的连接执行 SQL(选中文本优先),结果写入结果面板
  Future<void> _onRun() => _runSql(_effectiveSql);

  /// 解释:按连接类型拼 EXPLAIN 语句执行,展示执行计划
  Future<void> _onExplain() async {
    final sql = _effectiveSql;
    if (sql.isEmpty) {
      setState(() => _message = '请输入 SQL 语句');
      return;
    }
    final conn = _lookupConnection();
    if (conn == null) return;
    final explained = switch (conn.typeId) {
      'mysql' || 'postgresql' => 'EXPLAIN $sql',
      'sqlite' => 'EXPLAIN QUERY PLAN $sql',
      _ => null,
    };
    if (explained == null) {
      setState(() => _message = '${conn.typeId} 暂不支持解释执行计划');
      return;
    }
    await _runSql(explained);
  }

  /// 停止:作废后续语句,并向服务端带外下发取消(否则当前语句仍在库上跑完)
  void _onStop() {
    if (!_running) return;
    _runGeneration++;
    _elapsedMs = _runStopwatch.elapsedMilliseconds;
    // 带外取消:主连接正被 executeQuery 占住,用第二条连接发 KILL / cancel;
    // 权限不足或驱动不支持时静默,退化为「仅客户端停止」
    final sid = _activeSessionId;
    final conn = context.read<AppState>().connectionByName(_connection);
    if (sid != null && conn != null) {
      unawaited(
        context.read<AppState>().connectionManager.killSession(conn, sid),
      );
    }
    setState(() {
      _running = false;
      _stopped = true;
      if (_outcomes.isEmpty) {
        _message = '已停止(已请求取消服务端查询)';
      }
    });
  }

  /// 未实现功能提示
  void _notImplemented(String feature) {
    MessageBox.show(
      context,
      title: feature,
      message: '「$feature」功能尚未开发,敬请期待。',
      type: MessageBoxType.info,
    );
  }

  /// 解析运行上下文的连接;未选择/无驱动时弹窗提示并返回 null
  ConnectionInfo? _lookupConnection() {
    final app = context.read<AppState>();
    final conn = app.connectionByName(_connection);
    if (conn == null) {
      MessageBox.show(
        context,
        title: '运行查询',
        message: '当前查询未选择数据库连接。\n'
            '请点击编辑区上方的连接下拉框选择一个连接。',
        type: MessageBoxType.warning,
      );
      return null;
    }
    if (!hasDriver(conn)) {
      MessageBox.show(
        context,
        title: '运行查询',
        message: '连接「${conn.name}」的数据库类型(${conn.typeId})暂不支持执行查询。',
        type: MessageBoxType.warning,
      );
      return null;
    }
    return conn;
  }

  Future<void> _runSql(String sql) async {
    final trimmed = sql.trim();
    if (trimmed.isEmpty) {
      setState(() => _message = '请输入 SQL 语句');
      return;
    }
    final app = context.read<AppState>();
    final conn = _lookupConnection();
    if (conn == null) return;

    // 按顶层分号拆分脚本,逐条执行、逐条出结果(每条语句一个结果 tab)
    final statements = splitSqlStatements(trimmed);
    if (statements.isEmpty) {
      setState(() => _message = '请输入 SQL 语句');
      return;
    }

    app.recordSql(_tabKey, trimmed);
    _runGeneration++;
    final generation = _runGeneration;
    setState(() {
      _running = true;
      _stopped = false;
      _totalStatements = statements.length;
      _outcomes = [];
      _panelTabIndex = 0;
      _message = null;
    });
    // 取当前会话的服务端 id,供「停止」带外取消(取不到则退化为仅客户端停止)
    _activeSessionId = null;
    try {
      _activeSessionId = await app.connectionManager.serverSessionId(
        conn,
        database: _database,
        schema: _schema,
      );
    } catch (_) {
      _activeSessionId = null;
    }
    if (!mounted || generation != _runGeneration) return;
    _runStopwatch
      ..reset()
      ..start();
    for (final stmt in statements) {
      if (!mounted || generation != _runGeneration) break;
      final stmtSw = Stopwatch()..start();
      try {
        final result = await app.connectionManager.runQuery(
          conn,
          stmt,
          database: _database,
          schema: _schema,
          limit: _resultLimit,
        );
        if (!mounted || generation != _runGeneration) break;
        setState(() {
          _outcomes.add(
            _StatementOutcome(
                sql: stmt,
                result: result,
                elapsedMs: stmtSw.elapsedMilliseconds),
          );
          // 执行过程中自动切到最新结果 tab
          _panelTabIndex = _outcomes.length;
        });
      } catch (e) {
        if (!mounted || generation != _runGeneration) break;
        // 语句出错:记录错误并停止后续语句(同 pgAdmin)
        setState(() {
          _outcomes.add(
            _StatementOutcome(
                sql: stmt,
                error: e.toString(),
                elapsedMs: stmtSw.elapsedMilliseconds),
          );
          _panelTabIndex = _outcomes.length;
          _running = false;
          _elapsedMs = _runStopwatch.elapsedMilliseconds;
        });
        return;
      }
    }
    if (!mounted || generation != _runGeneration) return;
    _elapsedMs = _runStopwatch.elapsedMilliseconds;
    setState(() => _running = false);
  }

  // ── 构建 ──────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final panelVisible = _running || _outcomes.isNotEmpty || _message != null;
    return Container(
      color: t.background,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _toolbar(t),
          _connectionBar(t),
          Expanded(child: _editor(t)),
          if (panelVisible) _resultPanel(t),
        ],
      ),
    );
  }

  // 顶部工具栏:保存 / 查询创建工具 / 美化SQL / 代码段 / 创建图表
  // (运行 / 停止 / 解释在下方运行上下文栏右侧,同查询页布局)
  //
  // 底色取 secondary(栏底灰)而不是 control(控件底):本行紧贴文档标签条,
  // 而选中的标签就是 secondary 且**底边开放**(不画下边框),两者同色才能连成
  // 一整块、不留接缝 —— Navicat 里标签条上/下那两行同样是 #F0F0F0。
  // 用 control(#F8F8F8,本就是输入框/按钮的底)会与标签差一档灰,露出"色差"。
  Widget _toolbar(AppPalette t) {
    return Container(
      height: 34,
      color: t.secondary,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        children: [
          ToolbarButton(
            icon: Icons.save_outlined,
            text: '保存',
            onTap: _onSave,
          ),
          _divider(t),
          ToolbarButton(
            icon: Icons.construction_outlined,
            text: '查询创建工具',
            onTap: () => _notImplemented('查询创建工具'),
          ),
          ToolbarButton(
            icon: Icons.auto_fix_high_outlined,
            text: '美化 SQL',
            onTap: _onFormat,
          ),
          ToolbarButton(
            icon: Icons.bookmark_border,
            text: '代码段',
            onTap: () => _notImplemented('代码段'),
          ),
          ToolbarButton(
            icon: Icons.insert_chart_outlined,
            text: '创建图表',
            onTap: () => _notImplemented('创建图表'),
          ),
        ],
      ),
    );
  }

  // ── 运行上下文栏 ──────────────────────────────────────────

  // 运行上下文栏:左侧连接/库下拉选择器,右侧运行/停止/解释
  Widget _connectionBar(AppPalette t) {
    final c = AppColors.of(context);
    final conn = context.read<AppState>().connectionByName(_connection);
    return Container(
      height: 30,
      color: t.secondary,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        children: [
          // 连接选择器:类型品牌图标 + 连接名
          Popover(
            controller: _connMenu,
            width: 230,
            padding: const EdgeInsets.symmetric(vertical: 4),
            gap: 3,
            // 连接可能多达数十个:超高时面板内滚动,而不是溢出屏幕
            scrollable: true,
            maxHeight: 360,
            // 开合由外层 Popover 的 Listener 处理,按钮仅提供视觉态
            trigger: ToolbarButton(
              iconWidget: _typeIcon(conn?.typeId, t),
              text: _connection ?? '选择连接',
              showCaret: true,
              outlined: true,
              height: 22,
              textMaxWidth: 150,
              onTap: () {},
            ),
            content: _connectionMenu(t),
          ),
          const SizedBox(width: 8),
          // 数据库选择器:圆柱图标 + 库名
          Popover(
            controller: _dbMenu,
            width: 200,
            padding: const EdgeInsets.symmetric(vertical: 4),
            gap: 3,
            // 库列表同样可能很长(SQL Server 实例):超高时面板内滚动
            scrollable: true,
            maxHeight: 320,
            onOpenChanged: _onDbMenuOpen,
            trigger: ToolbarButton(
              iconWidget: Icon(Icons.storage, size: 14, color: c.iconSuccess),
              text: _database ?? '选择数据库',
              showCaret: true,
              outlined: true,
              height: 22,
              textMaxWidth: 150,
              onTap: () {},
            ),
            content: _databaseMenu(t),
          ),
          // 模式选择器:仅支持会话级模式切换的类型(PostgreSQL 家族)显示;
          // 选中模式后运行查询前会先 SET search_path
          if (kUseSchemaTypes.contains(conn?.typeId)) ...[
            const SizedBox(width: 8),
            Popover(
              controller: _schemaMenu,
              width: 200,
              padding: const EdgeInsets.symmetric(vertical: 4),
              gap: 3,
              scrollable: true,
              maxHeight: 320,
              onOpenChanged: _onSchemaMenuOpen,
              trigger: ToolbarButton(
                iconWidget: Icon(Icons.account_tree_outlined,
                    size: 14, color: c.iconPrimary),
                text: _schema ?? '默认模式',
                showCaret: true,
                outlined: true,
                height: 22,
                textMaxWidth: 150,
                onTap: () {},
              ),
              content: _schemaMenuList(t),
            ),
          ],
          const Spacer(),
          // 无下拉:编辑区有选中文本时按钮自动切换为「运行已选择的」(执行时选中文本优先)
          ToolbarButton(
            backgroundColor: t.secondary,
            icon: Icons.play_arrow,
            text: _hasSelection ? '运行已选择的' : '运行',
            iconColor: const Color(0xff1f6feb),
            enabled: !_running,
            onTap: _onRun,
          ),
          const SizedBox(width: 4),
          ToolbarButton(
            backgroundColor: t.secondary,
            icon: Icons.stop,
            text: '停止',
            iconColor: _errorColor,
            enabled: _running,
            onTap: _onStop,
          ),
          ToolbarButton(
            backgroundColor: t.secondary,
            icon: Icons.manage_search_outlined,
            text: _hasSelection ? '解释已选择的' : '解释',
            iconColor: const Color(0xff1f6feb),
            enabled: !_running,
            onTap: _onExplain,
          ),
        ],
      ),
    );
  }

  /// 数据库下拉打开时:懒加载所选连接的库列表(expandConnection 幂等)
  void _onDbMenuOpen(bool open) {
    if (!open) return;
    final app = context.read<AppState>();
    final conn = app.connectionByName(_connection);
    if (conn != null && hasDriver(conn)) {
      app.connectionManager.expandConnection(conn);
    }
  }

  /// 模式下拉打开时:懒加载所选库的模式列表(ensureSchemas 幂等)
  void _onSchemaMenuOpen(bool open) {
    if (!open) return;
    final app = context.read<AppState>();
    final conn = app.connectionByName(_connection);
    final db = _database;
    if (conn != null && db != null && hasDriver(conn)) {
      app.connectionManager.ensureSchemas(conn, db);
    }
  }

  /// 连接类型品牌图标(未匹配类型时回退连接节点图标)
  Widget _typeIcon(String? typeId, AppPalette t) {
    final c = AppColors.of(context);
    if (typeId != null) {
      for (final type in kAllDbTypes) {
        if (type.id == typeId) return DbTypeIcon(type: type, size: 14);
      }
    }
    return Icon(Icons.dns, size: 14, color: c.iconInfo);
  }

  /// 连接下拉列表:全部连接,当前项高亮
  Widget _connectionMenu(AppPalette t) {
    final conns = context.read<AppState>().connections;
    if (conns.isEmpty) {
      return _menuHint(t, '暂无连接,请先在左侧连接树新建连接');
    }
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final c in conns)
          ListItem(
            height: 24,
            leading: SizedBox(
              width: 16,
              height: 16,
              child: Center(child: _typeIcon(c.typeId, t)),
            ),
            title: c.name,
            selected: c.name == _connection,
            hoverBase: t.popover,
            onSelect: () {
              _connMenu.close();
              setState(() {
                _connection = c.name;
                // 换连接后原库不一定存在:回退到该连接的默认库
                _database = c.database.isEmpty ? null : c.database;
                // 模式归属旧库,一并重置(默认 search_path)
                _schema = null;
              });
              _syncPromptContext();
            },
          ),
      ],
    );
  }

  /// 数据库下拉列表:所选连接的库列表(按加载状态渲染)
  Widget _databaseMenu(AppPalette t) {
    final c = AppColors.of(context);
    final app = context.read<AppState>();
    final conn = app.connectionByName(_connection);
    if (conn == null) return _menuHint(t, '请先选择连接');
    if (!hasDriver(conn)) return _menuHint(t, '${conn.typeId} 类型暂不支持');
    final state = app.connectionManager.databaseStateOf(conn.name);
    switch (state.status) {
      case LoadStatus.idle:
      case LoadStatus.loading:
        return _menuHint(t, '加载中 ...');
      case LoadStatus.error:
        return _menuHint(t, '加载失败: ${state.error}');
      case LoadStatus.loaded:
        break;
    }
    if (state.databases.isEmpty) return _menuHint(t, '无数据库');
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final db in state.databases)
          ListItem(
            height: 24,
            leading: SizedBox(
              width: 16,
              height: 16,
              child: Center(
                  child: Icon(Icons.storage, size: 14, color: c.iconSuccess)),
            ),
            title: db,
            selected: db == _database,
            hoverBase: t.popover,
            onSelect: () {
              _dbMenu.close();
              setState(() {
                _database = db;
                // 换库后模式列表不同,重置为默认 search_path
                _schema = null;
              });
              _syncPromptContext();
            },
          ),
      ],
    );
  }

  /// 模式下拉列表:首项「默认」清除模式覆盖(走连接默认 search_path),
  /// 其后为所选库的模式列表(按加载状态渲染)
  Widget _schemaMenuList(AppPalette t) {
    final c = AppColors.of(context);
    final app = context.read<AppState>();
    final conn = app.connectionByName(_connection);
    if (conn == null) return _menuHint(t, '请先选择连接');
    final db = _database;
    if (db == null) return _menuHint(t, '请先选择数据库');
    if (!hasDriver(conn)) return _menuHint(t, '${conn.typeId} 类型暂不支持');
    final state = app.connectionManager.schemaStateOf(conn.name, db);
    switch (state.status) {
      case LoadStatus.idle:
      case LoadStatus.loading:
        return _menuHint(t, '加载中 ...');
      case LoadStatus.error:
        return _menuHint(t, '加载失败: ${state.error}');
      case LoadStatus.loaded:
        break;
    }
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // 默认项:不覆盖 search_path(连接默认,通常为 "$user", public)
        ListItem(
          height: 24,
          leading: SizedBox(
            width: 16,
            height: 16,
            child: Center(
              child: Icon(Icons.settings_backup_restore,
                  size: 14, color: c.iconInfo),
            ),
          ),
          title: '默认(search_path)',
          selected: _schema == null,
          hoverBase: t.popover,
          onSelect: () {
            _schemaMenu.close();
            setState(() => _schema = null);
            _syncPromptContext();
          },
        ),
        if (state.schemas.isEmpty)
          _menuHint(t, '无模式')
        else
          for (final s in state.schemas)
            ListItem(
              height: 24,
              leading: SizedBox(
                width: 16,
                height: 16,
                child: Center(
                  child: Icon(Icons.account_tree_outlined,
                      size: 14, color: c.iconPrimary),
                ),
              ),
              title: s,
              selected: s == _schema,
              hoverBase: t.popover,
              onSelect: () {
                _schemaMenu.close();
                setState(() => _schema = s);
                _syncPromptContext();
              },
            ),
      ],
    );
  }

  /// 菜单占位提示(加载中 / 错误 / 空列表)
  Widget _menuHint(AppPalette t, String text) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
      child: Text(
        text,
        style: TextStyle(
          fontSize: 12,
          color: t.mutedForeground,
          decoration: TextDecoration.none,
        ),
      ),
    );
  }

  // SQL 编辑区:re_editor 自绘编辑器(语法高亮 + 行号 + schema 感知补全)
  Widget _editor(AppPalette t) {
    final isDark = t.background.computeLuminance() < 0.5;
    return CodeAutocomplete(
      viewBuilder: (context, notifier, onSelected) => _SqlPromptPanel(
        notifier: notifier,
        onSelected: onSelected,
      ),
      promptsBuilder: _promptsBuilder,
      child: CodeEditor(
        controller: _controller,
        focusNode: _focusNode,
        hint: '-- 在此输入 SQL 语句',
        wordWrap: false,
        // SQL 无 {} 折叠语义,禁用折叠分析
        chunkAnalyzer: const NonCodeChunkAnalyzer(),
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
        style: CodeEditorStyle(
          fontFamily: 'monospace',
          fontSize: 13.5,
          fontHeight: 20 / 13.5,
          textColor: t.foreground,
          backgroundColor: t.background,
          hintTextColor: t.disabledForeground,
          selectionColor: Color.alphaBlend(
            t.accent.withValues(alpha: 0.30),
            t.background,
          ),
          cursorColor: t.foreground,
          codeTheme: CodeHighlightTheme(
            languages: {
              'sql': CodeHighlightThemeMode(mode: langSql),
            },
            theme: {
              ...(isDark ? _sqlDarkTheme : _sqlLightTheme),
              'root': TextStyle(color: t.foreground),
            },
          ),
        ),
        indicatorBuilder:
            (context, editingController, chunkController, notifier) =>
                DefaultCodeLineNumber(
          controller: editingController,
          notifier: notifier,
          textStyle: TextStyle(fontSize: 12.5, color: t.disabledForeground),
          focusedTextStyle: TextStyle(fontSize: 12.5, color: t.mutedForeground),
          minNumberCount: 4,
        ),
      ),
    );
  }

  // ── 结果面板 ──────────────────────────────────────────────

  /// 结果面板:头部状态栏 + tab 条(消息 / 结果 1..N)+ 内容区。
  /// 每条语句一个 tab(同 pgAdmin):SELECT 结果网格,写操作显示受影响行数,
  /// 失败显示错误文本;消息 tab 汇总各语句执行日志。
  /// 内容区用 IndexedStack 保活,各网格的滚动 / 选中状态切换 tab 不丢失
  Widget _resultPanel(AppPalette t) {
    return Container(
      height: 300,
      decoration: BoxDecoration(
        color: t.background,
        border: Border(top: BorderSide(color: t.border)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _resultHeader(t),
          if (_outcomes.isNotEmpty) ...[
            // 头-only 模式 tab 条(内容区为下方 IndexedStack,保活各 tab 状态)
            // 高度 / 宽度均由 TabControl 自适应(消息、结果 1..N 长短不一)
            TabControl(
              initialIndex: _panelTabIndex.clamp(0, _outcomes.length),
              contentPadding: EdgeInsets.zero,
              tabBarColor: t.secondary,
              selectedTabColor: t.background,
              onChanged: (i) => setState(() => _panelTabIndex = i),
              tabs: [
                const TabItem(label: '消息'),
                for (var i = 0; i < _outcomes.length; i++)
                  TabItem(label: '结果 ${i + 1}'),
              ],
            ),
            Expanded(
              child: IndexedStack(
                index: _panelTabIndex.clamp(0, _outcomes.length),
                children: [
                  _messagesTab(t),
                  for (var i = 0; i < _outcomes.length; i++)
                    _outcomeTab(t, _outcomes[i], i),
                ],
              ),
            ),
          ] else
            Expanded(child: _emptyPanelBody(t)),
        ],
      ),
    );
  }

  // 结果面板头:状态文案(执行中 / 停止 / 出错 / 语句统计)
  Widget _resultHeader(AppPalette t) {
    final hasError = _outcomes.any((o) => o.isError);
    return Container(
      height: 26,
      color: t.secondary,
      padding: const EdgeInsets.only(left: 12, right: 8),
      child: Row(
        children: [
          if (_running) ...[
            const Spinner(size: 13),
            const SizedBox(width: 6),
          ] else if (hasError) ...[
            const Icon(Icons.error_outline, size: 14, color: _errorColor),
            const SizedBox(width: 6),
          ],
          Flexible(
            child: Text(
              _statusText,
              style: TextStyle(
                fontSize: 12,
                color: hasError ? _errorColor : t.mutedForeground,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      ),
    );
  }

  /// 状态栏文案:参考 Navicat / DBeaver / DataGrip 的通行做法——
  /// 主信息是「执行结果 + 耗时」,行数用 `+` 后缀表示"还有更多"
  /// (由 LIMIT N+1 探测得来,不跑 COUNT 浪费服务端性能);
  /// DML 用「受影响 X 行」(与 MySQL/PG 服务端返回一致)。
  String get _statusText {
    if (_running) return '正在执行查询 ...';
    final n = _outcomes.length;
    if (n > 0) {
      if (_outcomes.any((o) => o.isError)) {
        return '执行出错 · $_elapsedMs ms';
      }
      if (_stopped) {
        return '已停止 · $n/$_totalStatements 条语句 · $_elapsedMs ms';
      }
      // 单条语句显示明细文案
      if (n == 1) {
        final r = _outcomes.single.result;
        if (r != null && r.isSelect) {
          final suffix = r.truncated ? '+' : '';
          return '执行成功 · ${r.rows.length}$suffix 行 · $_elapsedMs ms';
        }
        if (r != null) {
          return r.affectedRows > 0
              ? '执行成功 · 受影响 ${r.affectedRows} 行 · $_elapsedMs ms'
              : '执行成功 · $_elapsedMs ms';
        }
      }
      return '执行成功 · $n 条语句 · $_elapsedMs ms';
    }
    return _message ?? '';
  }

  /// 无结果时的面板体(运行前提示,如「请输入 SQL 语句」/ 停止提示)
  Widget _emptyPanelBody(AppPalette t) {
    final message = _message;
    if (message != null) {
      return Padding(
        padding: const EdgeInsets.all(12),
        child: Text(
          message,
          style: TextStyle(fontSize: 12.5, color: t.mutedForeground),
        ),
      );
    }
    return const SizedBox.shrink();
  }

  /// 消息 tab:各语句执行结果汇总日志(语句回显 + 结果 / 错误 + 耗时)
  Widget _messagesTab(AppPalette t) {
    return ListView(
      padding: const EdgeInsets.all(12),
      children: [
        for (var i = 0; i < _outcomes.length; i++)
          _messageEntry(t, i + 1, _outcomes[i]),
        if (_running)
          Row(
            children: [
              const Spinner(size: 13),
              const SizedBox(width: 6),
              Text(
                '正在执行第 ${_outcomes.length + 1}/$_totalStatements 条语句 ...',
                style: TextStyle(fontSize: 12.5, color: t.mutedForeground),
              ),
            ],
          ),
      ],
    );
  }

  Widget _messageEntry(AppPalette t, int index, _StatementOutcome o) {
    final isError = o.isError;
    final r = o.result;
    String summary;
    if (isError) {
      summary = o.error!;
    } else if (r != null && r.isSelect) {
      // `+` 后缀表示服务端还有更多行(LIMIT N+1 探测,不跑 COUNT)
      final suffix = r.truncated ? '+' : '';
      summary = '${r.rows.length}$suffix 行';
    } else if (r != null && r.affectedRows > 0) {
      summary = '受影响 ${r.affectedRows} 行';
    } else {
      summary = '执行成功';
    }
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // 语句回显(折叠为单行)
          Text(
            _oneLine(o.sql),
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(
              fontSize: 12,
              fontFamily: 'monospace',
              color: t.mutedForeground,
            ),
          ),
          const SizedBox(height: 2),
          Text(
            isError
                ? '语句 $index 出错:$summary · ${o.elapsedMs} ms'
                : '语句 $index:$summary · ${o.elapsedMs} ms',
            style: TextStyle(
              fontSize: 12.5,
              color: isError ? _errorColor : t.foreground,
            ),
          ),
        ],
      ),
    );
  }

  /// 语句文本折叠为单行(消息 tab 回显用)
  String _oneLine(String sql) => sql.replaceAll(RegExp(r'\s+'), ' ').trim();

  /// 单条语句结果 tab:SELECT 网格(截断时带「加载更多」底栏)/ 写操作执行信息 / 错误文本
  Widget _outcomeTab(AppPalette t, _StatementOutcome o, int index) {
    final error = o.error;
    if (error != null) {
      return Padding(
        padding: const EdgeInsets.all(12),
        child: SingleChildScrollView(
          child: Text(
            error,
            style: const TextStyle(
              fontSize: 12.5,
              fontFamily: 'monospace',
              color: _errorColor,
            ),
          ),
        ),
      );
    }
    final result = o.result;
    if (result == null || !result.isSelect) {
      final affected = result?.affectedRows ?? 0;
      return Center(
        child: Text(
          affected > 0 ? '执行成功 · 受影响 $affected 行' : '执行成功',
          style: TextStyle(fontSize: 12.5, color: t.mutedForeground),
        ),
      );
    }
    return Column(
      children: [
        Expanded(
          child: _ResultGrid(result: result, onExport: _exportResultRows),
        ),
        if (result.truncated) _loadMoreBar(t, index, result),
      ],
    );
  }

  /// 「导出结果」:把结果网格当前显示的数据写为 CSV / JSON 文件
  Future<void> _exportResultRows(TablePreview data) async {
    final conn = _lookupConnection();
    if (conn == null || !mounted) return;
    await showExportResultDialog(
      context,
      data: data,
      typeId: conn.typeId,
      database: _database ?? '',
      label: widget.title,
      schema: _schema,
    );
  }

  /// 「加载更多」:对某条被截断的 SELECT 结果,用 offset 续取下一页并累加进结果。
  /// 期间以 [_loadingMoreIndex] 标记;切 tab / 停止会 bump _runGeneration,
  /// 使在途续取结果作废(不追加到已失效的 outcome)。
  /// 累积到 [_maxAccumulatedRows] 后不再续取,引导用户走「导出结果」流式落盘。
  Future<void> _loadMore(int index) async {
    if (index >= _outcomes.length) return;
    final outcome = _outcomes[index];
    final cur = outcome.result;
    if (cur == null || !cur.isSelect) return;
    // 累积上限已到:不再往内存里堆行,导出走流式路径
    if (cur.rows.length >= _maxAccumulatedRows) return;
    final conn = _lookupConnection();
    if (conn == null) return;
    final generation = _runGeneration;
    setState(() => _loadingMoreIndex = index);
    try {
      final more = await context.read<AppState>().connectionManager.runQuery(
            conn,
            outcome.sql,
            database: _database,
            schema: _schema,
            limit: _resultLimit,
            offset: cur.rows.length,
          );
      if (!mounted || generation != _runGeneration) return;
      outcome.result = QueryResult(
        columns: cur.columns,
        rows: [...cur.rows, ...more.rows],
        limit: cur.limit,
        offset: cur.rows.length + more.rows.length,
        moreRows: more.moreRows,
      );
      setState(() => _loadingMoreIndex = null);
    } catch (e) {
      if (!mounted) return;
      setState(() => _loadingMoreIndex = null);
      await MessageBox.show(
        context,
        title: '加载更多',
        message: e.toString(),
        type: MessageBoxType.error,
        okText: '知道了',
      );
    }
  }

  /// 结果被截断时的「加载更多」底栏:简洁展示 `已加载 X+ 行`,
  /// 达到累积上限后隐藏「加载更多」按钮并提示走导出
  Widget _loadMoreBar(AppPalette t, int index, QueryResult result) {
    final loading = _loadingMoreIndex == index;
    final atCap = result.rows.length >= _maxAccumulatedRows;
    return Container(
      height: 30,
      decoration: BoxDecoration(
        color: t.secondary,
        border: Border(top: BorderSide(color: t.border)),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        children: [
          Text(
            atCap
                ? '已加载 ${result.rows.length} 行(达客户端上限,更多数据请导出)'
                : '已加载 ${result.rows.length}+ 行',
            style: TextStyle(fontSize: 12, color: t.mutedForeground),
          ),
          const Spacer(),
          if (!atCap)
            ToolbarButton(
              icon: Icons.expand_more,
              text: loading ? '加载中…' : '加载更多',
              enabled: !loading && !_running,
              onTap: () => _loadMore(index),
            ),
        ],
      ),
    );
  }

  Widget _divider(AppPalette t) => SizedBox(
        width: 6,
        height: 16,
        child: Center(
          child: Separator(
            orientation: Axis.vertical,
            thickness: 1,
            color: t.divider,
          ),
        ),
      );
}

// ────────────────────────────────────────────────────────────
// 结果网格(单结果集)
// ────────────────────────────────────────────────────────────

/// 单结果集网格:列头 + 行号列 + 数据行,横向滚动 + 纵向懒加载,
/// 双向滚动条 + 列头拖拽变宽 + 单元格多选(Ctrl+C 复制 / Ctrl+A 全选)。
/// 每个结果 tab 一个实例,持有自己的滚动 / 列宽 / 选中状态;
/// 由结果面板 IndexedStack 保活,切换 tab 状态不丢失
class _ResultGrid extends StatefulWidget {
  const _ResultGrid({required this.result, required this.onExport});

  final QueryResult result;

  /// 「导出结果...」:入参为当前显示顺序下的结果集
  final Future<void> Function(TablePreview data) onExport;

  @override
  State<_ResultGrid> createState() => _ResultGridState();
}

class _ResultGridState extends State<_ResultGrid> {
  /// 结果网格双向滚动控制器
  final ScrollController _hScrollController = ScrollController();
  final ScrollController _vScrollController = ScrollController();

  /// 结果网格焦点:Ctrl+C/Ctrl+A 快捷键据此判断焦点是否在结果区域
  final FocusNode _focusNode = FocusNode();

  /// 结果网格列宽(拖拽列头边框调整后持久化;新结果集时重置)
  List<double>? _columnWidths;

  /// 各列「放下最长内容」所需宽度:按结果集测量一次后复用,
  /// 窗口宽度变化只重算「空余宽度补给最后一列」,不重复测量
  List<double>? _contentWidths;

  /// 用户拖过列头边框:本次结果集内尊重用户设的列宽,不再自动适配
  bool _manualColumnWidths = false;

  /// 各列是否「值全是数字」:结果集不携带列类型元数据,只能按值判定,
  /// 用于数值列右对齐。与 [_contentWidths] 同一时机算一次。
  List<bool>? _numericCols;

  /// 结果网格多选单元格集合
  Set<(int, int)> _selectedCells = {};

  /// 多选锚点(Shift+click 范围选择的起点)
  (int, int)? _anchorCell;

  /// 本地排序状态(拖列头标题设定):只对已取回的结果行重排,不重跑 SQL
  int? _sortCol;
  bool _sortAsc = true;

  /// 当前排序下的显示行(视图行号 → 行数据);未排序时即原结果行
  List<List<String>> _displayRows = [];

  @override
  void initState() {
    super.initState();
    _displayRows = widget.result.rows;
  }

  @override
  void didUpdateWidget(covariant _ResultGrid oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.result != widget.result) {
      // 新结果集:上一份的本地排序与选中都不再适用;列宽重新按内容适配
      _sortCol = null;
      _sortAsc = true;
      _selectedCells = {};
      _anchorCell = null;
      _displayRows = widget.result.rows;
      _columnWidths = null;
      _contentWidths = null;
      _manualColumnWidths = false;
    }
  }

  /// 拖列头标题排序:向右=升序、向左=降序
  void _sortBy(int col, bool asc) {
    if (_sortCol == col && _sortAsc == asc) return;
    setState(() {
      _sortCol = col;
      _sortAsc = asc;
      _displayRows = _sortedRows();
      // 行序已变,旧选中单元格不再指向同一数据
      _selectedCells = {};
      _anchorCell = null;
    });
  }

  List<List<String>> _sortedRows() {
    final col = _sortCol;
    final rows = List<List<String>>.of(widget.result.rows);
    if (col == null) return rows;
    final asc = _sortAsc;
    rows.sort((a, b) {
      final cmp = _compareCell(
          col < a.length ? a[col] : '', col < b.length ? b[col] : '');
      return asc ? cmp : -cmp;
    });
    return rows;
  }

  /// 两侧都能解析为数值时按数值比(避免 '9' > '10' 的字典序),否则按字符串比
  static int _compareCell(String x, String y) {
    final nx = double.tryParse(x);
    final ny = double.tryParse(y);
    if (nx != null && ny != null) return nx.compareTo(ny);
    return x.compareTo(y);
  }

  @override
  void dispose() {
    _hScrollController.dispose();
    _vScrollController.dispose();
    _focusNode.dispose();
    super.dispose();
  }

  /// 复制选中单元格:按行排序、同行内按列排序,制表符分隔列,换行分隔行
  void _copySelectedCells() {
    // 选中存的是视图行号,须按当前排序后的显示行取值
    final rows = _displayRows;
    if (_selectedCells.isEmpty) return;
    // 按行分组,同行内按列排序
    final Map<int, List<int>> byRow = {};
    for (final (r, c) in _selectedCells) {
      byRow.putIfAbsent(r, () => []).add(c);
    }
    final sortedRows = byRow.keys.toList()..sort();
    final buffer = StringBuffer();
    for (final r in sortedRows) {
      final cols = byRow[r]!..sort();
      for (var i = 0; i < cols.length; i++) {
        if (i > 0) buffer.write('\t');
        if (r < rows.length && cols[i] < rows[r].length) {
          buffer.write(rows[r][cols[i]]);
        }
      }
      if (r != sortedRows.last) buffer.write('\n');
    }
    Clipboard.setData(ClipboardData(text: buffer.toString()));
  }

  /// 当前显示顺序下的结果集:本地拖列头排序只改视图行序,导出跟随所见
  TablePreview _displayPreview() {
    final result = widget.result;
    return TablePreview(
      columns: result.columns,
      rows: _displayRows,
      limit: result.limit,
    );
  }

  /// 右键单元格菜单(与表数据页同一交互口径)
  void _showCellMenu(int row, int col, Offset position) {
    final rows = _displayRows;
    final cell =
        (row < rows.length && col < rows[row].length) ? rows[row][col] : '';
    showContextMenu(
      context,
      position: position,
      items: [
        MenuItem(
            text: '复制单元格',
            shortcut: 'Ctrl+C',
            onPressed: () => Clipboard.setData(ClipboardData(text: cell))),
        const MenuSeparator(),
        MenuItem(
            text: '导出结果...',
            onPressed: () => widget.onExport(_displayPreview())),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    return Focus(
      focusNode: _focusNode,
      child: _grid(t),
      onKeyEvent: (node, event) {
        if (event is! KeyDownEvent ||
            !HardwareKeyboard.instance.isControlPressed) {
          return KeyEventResult.ignored;
        }
        // Ctrl+C:复制选中单元格
        if (event.logicalKey == LogicalKeyboardKey.keyC &&
            _selectedCells.isNotEmpty) {
          _copySelectedCells();
          return KeyEventResult.handled;
        }
        // Ctrl+A:全选结果网格所有单元格
        if (event.logicalKey == LogicalKeyboardKey.keyA) {
          final result = widget.result;
          if (result.rows.isNotEmpty) {
            final cells = <(int, int)>{};
            for (var r = 0; r < result.rows.length; r++) {
              for (var c = 0; c < result.columns.length; c++) {
                cells.add((r, c));
              }
            }
            setState(() {
              _selectedCells = cells;
              _anchorCell = (0, 0);
            });
            return KeyEventResult.handled;
          }
        }
        return KeyEventResult.ignored;
      },
    );
  }

  // 结果网格:列头 + 行号列 + 数据行,横向滚动 + 纵向懒加载,
  // 双向滚动条 + 列宽按内容自适应 + 列头拖边框变宽 + 拖列头标题本地排序
  Widget _grid(AppPalette t) {
    final columns = widget.result.columns;
    final rows = _displayRows;
    if (columns.isEmpty) {
      // 无列元数据(非 SELECT):内容区留白,不展示空态提示
      return Container(color: t.background);
    }
    // 0 行结果照常渲染网格:列头必须可见(仅数据区留白),
    // 与表数据页「空表仅渲染表头」保持一致
    return LayoutBuilder(
      builder: (context, constraints) {
        // 列宽要按面板宽度决定「是否补满」,故在拿到约束后再算
        _syncColumnWidths(context, columns, rows, constraints.maxWidth);
        final widths = _columnWidths!;
        final totalWidth =
            _numberColWidth + widths.fold<double>(0, (a, b) => a + b);
        // 纵向条必须挂在横向滚动区**之外**(与表数据页 _buildDataGrid 同理):
        // 挂在内部时它画在内容右缘(=所有列宽之和),宽表一横向滚动整条就滑出视口。
        // scrollbars:false 关掉桌面端为每个 Scrollable 自动补的隐式条,只留这两条。
        return ScrollConfiguration(
          behavior: ScrollConfiguration.of(context).copyWith(scrollbars: false),
          child: ScrollBar(
            controller: _vScrollController,
            thumbVisibility: true,
            // 纵向条在横向滚动区外面,内层 ListView 的通知到这儿 depth==1,
            // Material 默认的 depth==0 过滤会把它整条拒掉(纵向条消失的根因),改按轴过滤。
            notificationPredicate: (n) => n.metrics.axis == Axis.vertical,
            child: ScrollBar(
              controller: _hScrollController,
              orientation: ScrollBarOrientation.horizontal,
              thumbVisibility: true,
              child: SingleChildScrollView(
                controller: _hScrollController,
                scrollDirection: Axis.horizontal,
                child: SizedBox(
                  width: totalWidth,
                  child: DataGridView(
                    columns: [
                      for (var i = 0; i < columns.length; i++)
                        DataGridViewColumn(
                          title: columns[i],
                          alignment: _numericCols != null &&
                                  i < _numericCols!.length &&
                                  _numericCols![i]
                              ? Alignment.centerRight
                              : Alignment.centerLeft,
                        ),
                    ],
                    columnWidths: widths,
                    onColumnResize: (index, newWidth) {
                      setState(() {
                        // 用户手动定过列宽:本次结果集内不再自动适配
                        _manualColumnWidths = true;
                        widths[index] = newWidth;
                      });
                    },
                    sortColumn: _sortCol,
                    sortAscending: _sortAsc,
                    onHeaderSort: _sortBy,
                    selectedCells: _selectedCells,
                    anchorCell: _anchorCell,
                    onCellContext: _showCellMenu,
                    onCellsSelected: (cells) {
                      setState(() {
                        _selectedCells = cells;
                        // 更新锚点:单选或 Ctrl+click 时取最后点击的单元格
                        if (cells.length == 1) {
                          _anchorCell = cells.first;
                        }
                      });
                      // 选中后焦点移入结果面板,使 Ctrl+C/Ctrl+A 生效;
                      // 点击 SQL 编辑器时焦点自然切走,快捷键回归编辑器
                      _focusNode.requestFocus();
                    },
                    rowCount: rows.length,
                    cellBuilder: (row, col) => Text(
                      rows[row][col],
                      style: const TextStyle(fontSize: _cellFontSize),
                      overflow: TextOverflow.ellipsis,
                      maxLines: 1,
                    ),
                    rowHeight: _rowHeight,
                    showRowNumbers: true,
                    rowNumberWidth: _numberColWidth,
                    headerColor: t.secondary,
                    gridLineColor: t.gridLine,
                    cellPaddingX: _cellPaddingX,
                    rowHoverColor: Color.alphaBlend(
                      t.foreground.withValues(alpha: 0.06),
                      t.background,
                    ),
                    verticalScrollController: _vScrollController,
                  ),
                ),
              ),
            ),
          ),
        );
      },
    );
  }

  /// 结果网格列宽自适应(构建期赋值,同旧版初始化列宽的做法,不额外 setState)。
  ///
  /// 起因:PostgreSQL 的 `EXPLAIN` 每行都是一条计划文本(列名 QUERY PLAN),
  /// 固定 150px 列宽会把计划截成「Index Scan using p...」「Index Cond: ((id > ...」,
  /// 而右侧还空着一大片 —— 计划完全读不出来。这里改成按内容测宽,
  /// 并把面板空余宽度补给最后一列,长文本能整行显示、面板也不留缺口。
  void _syncColumnWidths(
    BuildContext context,
    List<String> columns,
    List<List<String>> rows,
    double viewportWidth,
  ) {
    var content = _contentWidths;
    if (content == null || content.length != columns.length) {
      content = _contentWidths = _measureColumnWidths(context, columns, rows);
      _numericCols = _detectNumericColumns(columns.length, rows);
    }
    // 用户手动拖过列宽:只保证长度对齐,不再覆盖用户的选择
    if (_manualColumnWidths) {
      final current = _columnWidths;
      if (current == null || current.length != content.length) {
        _columnWidths = List<double>.of(content);
      }
      return;
    }
    final fitted = fillPanelWidth(
      content,
      viewportWidth: viewportWidth,
      leadingWidth: _numberColWidth,
    );
    final current = _columnWidths;
    if (current == null || !_sameWidths(current, fitted)) {
      _columnWidths = fitted;
    }
  }

  /// 按内容测量列宽:数据单元格跟随实际继承的文本样式(只有字号由结果网格指定),
  /// 列头是 base-ui 自绘、不继承应用文本样式,字体来自 DesktopTokens
  List<double> _measureColumnWidths(
    BuildContext context,
    List<String> columns,
    List<List<String>> rows,
  ) {
    final textDirection = Directionality.of(context);
    // 系统文本缩放会同时放大单元格与列头,测量必须跟着放大,否则量出来的宽度偏窄
    final textScaler = MediaQuery.textScalerOf(context);
    final cellStyle =
        DefaultTextStyle.of(context).style.copyWith(fontSize: _cellFontSize);
    final tokens = TokenScope.maybeOf(context);
    final headerStyle = TextStyle(
      fontFamily: tokens?.fontFamily,
      fontSize: tokens?.fontSize,
      fontWeight: FontWeight.w600,
      decoration: TextDecoration.none,
    );
    double measure(String text, bool isHeader) {
      final painter = TextPainter(
        text: TextSpan(text: text, style: isHeader ? headerStyle : cellStyle),
        maxLines: 1,
        textDirection: textDirection,
        textScaler: textScaler,
      )..layout();
      return painter.width;
    }

    return autoColumnWidths(
      columns: columns,
      rows: rows,
      measureText: measure,
      paddingX: _cellPaddingX,
    );
  }

  /// 数值列判定(按值):某列所有非空值都能按数字解析才右对齐。
  /// 空串(驱动把 NULL 渲染成空)不参与判定,且要求至少出现一个数字值,
  /// 否则整列皆空 / 全空的列会被误判成数值列。
  static List<bool> _detectNumericColumns(
      int columnCount, List<List<String>> rows) {
    final result = List<bool>.filled(columnCount, false);
    for (var c = 0; c < columnCount; c++) {
      var numeric = true;
      var seen = false;
      for (final r in rows) {
        if (c >= r.length) continue;
        final v = r[c].trim();
        if (v.isEmpty) continue;
        if (num.tryParse(v) == null) {
          numeric = false;
          break;
        }
        seen = true;
      }
      result[c] = numeric && seen;
    }
    return result;
  }

  static bool _sameWidths(List<double> a, List<double> b) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) return false;
    }
    return true;
  }
}

// ────────────────────────────────────────────────────────────
// SQL 美化(关键字表见 data/sql_completions.dart 的 kSqlKeywords)
// ────────────────────────────────────────────────────────────

/// 这些关键字前换行(LEFT/RIGHT 等放行前,即使不跟 JOIN 也断行,简化处理)
const _breakBefore = {
  'FROM',
  'WHERE',
  'GROUP',
  'HAVING',
  'ORDER',
  'LIMIT',
  'OFFSET',
  'UNION',
  'VALUES',
  'SET',
  'JOIN',
  'LEFT',
  'RIGHT',
  'INNER',
  'OUTER',
  'CROSS',
  'FULL',
  'RETURNING',
};

/// 简单 SQL 美化:关键字大写、主要子句前换行、多余空白折叠、
/// 逗号紧跟前一词。按引号(单/双/反引号)外的空白切词,
/// 不破坏字符串字面量
String formatSql(String input) {
  if (input.trim().isEmpty) return input;

  // 1. 切词:引号内整体保留
  final tokens = <String>[];
  final buf = StringBuffer();
  String? quote;
  for (var i = 0; i < input.length; i++) {
    final ch = input[i];
    if (quote != null) {
      buf.write(ch);
      if (ch == quote) quote = null;
    } else if (ch == '\'' || ch == '"' || ch == '`') {
      if (buf.isNotEmpty) {
        tokens.add(buf.toString());
        buf.clear();
      }
      buf.write(ch);
      quote = ch;
    } else if (ch.trim().isEmpty) {
      if (buf.isNotEmpty) {
        tokens.add(buf.toString());
        buf.clear();
      }
    } else {
      buf.write(ch);
    }
  }
  if (buf.isNotEmpty) tokens.add(buf.toString());

  // 2. 逗号紧跟前一个词
  final merged = <String>[];
  for (final token in tokens) {
    if (token == ',' && merged.isNotEmpty) {
      merged[merged.length - 1] = '${merged.last},';
    } else if (token.startsWith(',') && merged.isNotEmpty) {
      merged[merged.length - 1] = '${merged.last}${token[0]}';
      final rest = token.substring(1);
      if (rest.isNotEmpty) merged.add(rest);
    } else {
      merged.add(token);
    }
  }

  // 3. 关键字大写 + 子句前换行
  final lines = <String>[];
  final current = StringBuffer();
  for (final raw in merged) {
    final upper = raw.toUpperCase();
    final token = kSqlKeywords.contains(upper) ? upper : raw;
    if (_breakBefore.contains(token) && current.isNotEmpty) {
      lines.add(current.toString());
      current.clear();
    }
    if (current.isNotEmpty) current.write(' ');
    current.write(token);
  }
  if (current.isNotEmpty) lines.add(current.toString());
  return lines.join('\n');
}

// ────────────────────────────────────────────────────────────
// SQL 语法高亮配色(re_highlight 主题映射,中调色主题无关)
// ────────────────────────────────────────────────────────────

/// 明亮主题:近似 SSMS / VS Code Light+(关键字蓝、字符串红、注释绿)
const Map<String, TextStyle> _sqlLightTheme = {
  'keyword': TextStyle(color: Color(0xff0000ff)),
  'literal': TextStyle(color: Color(0xff001080)),
  'name': TextStyle(color: Color(0xff001080)),
  'attr': TextStyle(color: Color(0xff001080)),
  'variable': TextStyle(color: Color(0xff001080)),
  'meta': TextStyle(color: Color(0xff001080)),
  'string': TextStyle(color: Color(0xffa31515)),
  'quote': TextStyle(color: Color(0xffa31515)),
  'regexp': TextStyle(color: Color(0xffa31515)),
  'comment': TextStyle(color: Color(0xff008000), fontStyle: FontStyle.italic),
  'doctag': TextStyle(color: Color(0xff800000)),
  'section': TextStyle(color: Color(0xff800000)),
  'number': TextStyle(color: Color(0xff098658)),
  'symbol': TextStyle(color: Color(0xff098658)),
  'type': TextStyle(color: Color(0xff267f99)),
  'class-title': TextStyle(color: Color(0xff267f99)),
  'built_in': TextStyle(color: Color(0xff795e26)),
  'title': TextStyle(color: Color(0xff795e26)),
};

/// 暗黑主题:近似 VS Code Dark+
const Map<String, TextStyle> _sqlDarkTheme = {
  'keyword': TextStyle(color: Color(0xff569cd6)),
  'literal': TextStyle(color: Color(0xff569cd6)),
  'name': TextStyle(color: Color(0xff9cdcfe)),
  'attr': TextStyle(color: Color(0xff9cdcfe)),
  'variable': TextStyle(color: Color(0xff9cdcfe)),
  'meta': TextStyle(color: Color(0xff9cdcfe)),
  'string': TextStyle(color: Color(0xffce9178)),
  'quote': TextStyle(color: Color(0xffce9178)),
  'regexp': TextStyle(color: Color(0xffd16969)),
  'comment': TextStyle(color: Color(0xff6a9955), fontStyle: FontStyle.italic),
  'doctag': TextStyle(color: Color(0xff6a9955)),
  'section': TextStyle(color: Color(0xff6a9955)),
  'number': TextStyle(color: Color(0xffb5cea8)),
  'symbol': TextStyle(color: Color(0xffb5cea8)),
  'type': TextStyle(color: Color(0xff4ec9b0)),
  'class-title': TextStyle(color: Color(0xff4ec9b0)),
  'built_in': TextStyle(color: Color(0xffdcdcaa)),
  'title': TextStyle(color: Color(0xffdcdcaa)),
};

// ────────────────────────────────────────────────────────────
// SQL 补全提示面板
// ────────────────────────────────────────────────────────────

/// 补全提示面板(re_editor CodeAutocomplete 的 viewBuilder)。
///
/// 每项 = 类型小图标 + 匹配词(输入前缀段加粗)+ 右侧灰色类型标注;
/// 列项额外追加中文注释列(来自 ColumnDef.comment)。
/// 选中 / hover 手绘派生色(明暗自适应),Listener.onPointerDown
/// 零延迟选择,无 InkWell / 水波纹。
class _SqlPromptPanel extends StatefulWidget implements PreferredSizeWidget {
  const _SqlPromptPanel({
    required this.notifier,
    required this.onSelected,
  });

  final ValueNotifier<CodeAutocompleteEditingValue> notifier;
  final ValueChanged<CodeAutocompleteResult> onSelected;

  static const double itemHeight = 26;
  static const double panelWidth = 400;
  static const int maxVisibleItems = 8;

  @override
  Size get preferredSize {
    final count = notifier.value.prompts.length.clamp(1, maxVisibleItems);
    return Size(panelWidth, itemHeight * count + 2);
  }

  @override
  State<_SqlPromptPanel> createState() => _SqlPromptPanelState();
}

class _SqlPromptPanelState extends State<_SqlPromptPanel> {
  final ScrollController _scroll = ScrollController();
  int _hoverIndex = -1;

  @override
  void initState() {
    super.initState();
    widget.notifier.addListener(_onValueChanged);
  }

  @override
  void dispose() {
    widget.notifier.removeListener(_onValueChanged);
    _scroll.dispose();
    super.dispose();
  }

  void _onValueChanged() {
    if (!mounted) return;
    setState(() {});
    // ↑↓ 换选中项后保持可见(jumpTo 无动画,零延迟)
    if (_scroll.hasClients) {
      final top = widget.notifier.value.index * _SqlPromptPanel.itemHeight;
      final bottom = top + _SqlPromptPanel.itemHeight;
      final offset = _scroll.offset;
      final viewport = _scroll.position.viewportDimension;
      if (top < offset) {
        _scroll.jumpTo(top);
      } else if (bottom > offset + viewport) {
        _scroll.jumpTo(bottom - viewport);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final c = AppColors.of(context);
    final value = widget.notifier.value;
    final prompts = value.prompts;
    final visible = prompts.length.clamp(1, _SqlPromptPanel.maxVisibleItems);
    return Container(
      width: _SqlPromptPanel.panelWidth,
      height: _SqlPromptPanel.itemHeight * visible + 2,
      decoration: BoxDecoration(
        color: t.popover,
        border: Border.all(color: t.border),
        borderRadius: BorderRadius.circular(4),
        boxShadow: [
          BoxShadow(
            color: t.foreground.withValues(alpha: 0.15),
            blurRadius: 8,
            offset: const Offset(0, 4),
          ),
        ],
      ),
      child: ListView.builder(
        controller: _scroll,
        itemExtent: _SqlPromptPanel.itemHeight,
        itemCount: prompts.length,
        itemBuilder: (context, index) =>
            _buildItem(t, c, value, index, prompts[index] as SqlPrompt),
      ),
    );
  }

  Widget _buildItem(
    AppPalette t,
    AppColors c,
    CodeAutocompleteEditingValue value,
    int index,
    SqlPrompt prompt,
  ) {
    final selected = index == value.index;
    Color? background;
    if (selected) {
      background = Color.alphaBlend(
        t.accent.withValues(alpha: 0.25),
        t.popover,
      );
    } else if (index == _hoverIndex) {
      // hover 明暗自适应:前景色低透明混合,暗色提亮 / 亮色加深
      background = Color.alphaBlend(
        t.foreground.withValues(alpha: 0.06),
        t.popover,
      );
    }
    return Listener(
      // 按下瞬间触发,零延迟(避开 onTap 的双击判定窗口)
      onPointerDown: (_) {
        widget.onSelected(value.copyWith(index: index).autocomplete);
      },
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hoverIndex = index),
        onExit: (_) {
          if (_hoverIndex == index) setState(() => _hoverIndex = -1);
        },
        child: Container(
          height: _SqlPromptPanel.itemHeight,
          color: background,
          padding: const EdgeInsets.symmetric(horizontal: 8),
          child: Row(
            children: [
              _kindIcon(t, c, prompt.kind),
              const SizedBox(width: 6),
              Expanded(child: _buildWord(t, value.input, prompt)),
              if (prompt.detail.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(left: 8),
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 90),
                    child: Text(
                      prompt.detail,
                      textAlign: TextAlign.right,
                      overflow: TextOverflow.ellipsis,
                      maxLines: 1,
                      style: TextStyle(
                        fontSize: 11,
                        color: t.mutedForeground,
                        decoration: TextDecoration.none,
                      ),
                    ),
                  ),
                ),
              // 对象注释(列 / 表 / 视图 / 函数):后端已拉回 comment 时展示,
              // Expanded 吃满剩余宽度、ellipsis 截断
              if (prompt.comment.isNotEmpty)
                Expanded(
                  child: Padding(
                    padding: const EdgeInsets.only(left: 10),
                    child: Text(
                      prompt.comment,
                      textAlign: TextAlign.right,
                      overflow: TextOverflow.ellipsis,
                      maxLines: 1,
                      style: TextStyle(
                        fontSize: 11,
                        color: t.mutedForeground.withValues(alpha: 0.85),
                        decoration: TextDecoration.none,
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }

  /// 匹配词:输入前缀段加粗,其余正常(显式清除装饰,不依赖 Material 兑底)
  Widget _buildWord(AppPalette t, String input, SqlPrompt prompt) {
    final word = prompt.word;
    final style = TextStyle(
      fontSize: 12.5,
      fontFamily: 'monospace',
      color: t.foreground,
      decoration: TextDecoration.none,
    );
    var prefixLength = 0;
    if (input.isNotEmpty &&
        word.toLowerCase().startsWith(input.toLowerCase())) {
      prefixLength = input.length;
    }
    return Text.rich(
      TextSpan(
        children: [
          if (prefixLength > 0)
            TextSpan(
              text: word.substring(0, prefixLength),
              style: style.copyWith(fontWeight: FontWeight.w600),
            ),
          TextSpan(text: word.substring(prefixLength), style: style),
        ],
      ),
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
    );
  }

  /// 类型小图标(功能中调色,主题无关)
  Widget _kindIcon(AppPalette t, AppColors c, SqlPromptKind kind) {
    final (icon, color) = switch (kind) {
      SqlPromptKind.keyword => (Icons.tag, c.iconInfo),
      SqlPromptKind.function => (Icons.functions, const Color(0xff9b6bde)),
      SqlPromptKind.table => (Icons.table_chart_outlined, c.iconPrimary),
      SqlPromptKind.view => (Icons.visibility_outlined, c.iconSuccess),
      SqlPromptKind.column => (Icons.view_column_outlined, t.mutedForeground),
    };
    return Icon(icon, size: 13, color: color);
  }
}
