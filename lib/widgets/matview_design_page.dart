import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show Clipboard, ClipboardData;
import 'package:provider/provider.dart';
import 'package:re_editor/re_editor.dart';
import 'package:re_highlight/languages/sql.dart';

import '../app/app_state.dart';
import '../data/db_data.dart';
import '../data/matview_sql.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';
import '../theme/sql_highlight.dart';
import 'query_page.dart' show formatSql;
import 'sql_find_bar.dart';

/// 实体化视图设计器(按 Navicat 的实体化视图编辑器还原)。
///
/// 工具栏:保存 / 刷新 / 并发刷新 | 预览 解释 美化 SQL | [索引页:添加 / 删除索引]
/// … 右缘全屏按钮收起 / 恢复两侧面板。
/// 页签:定义(仅 SELECT)/ 索引(pg_index,含并发刷新所需的唯一索引)/
/// 高级(所有者 / 表空间 / 访问方法 / 存储参数 / 填充数据)/ 注释 /
/// SQL 预览(更改 / DDL 子页)。
///
/// 与视图设计器同构:编辑区只存 SELECT,保存时由 [MatViewSql.createSql] 拼
/// 完整 DDL。PG 没有 `CREATE OR REPLACE MATERIALIZED VIEW`,编辑只能
/// **DROP + CREATE 重建**;重建会连带丢掉索引,故 DROP 前抓一份
/// `pg_get_indexdef` 原文、CREATE 后逐条回放。
class MatViewDesignPage extends StatefulWidget {
  const MatViewDesignPage({
    super.key,
    required this.name,
    required this.connection,
    required this.database,
    this.schema,
    this.isNew = false,
    this.comment = '',
  });

  final String name;
  final String connection;
  final String database;
  final String? schema;
  final bool isNew;
  final String comment;

  @override
  State<MatViewDesignPage> createState() => _MatViewDesignPageState();
}

class _MatViewDesignPageState extends State<MatViewDesignPage> {
  final CodeLineEditingController _controller =
      CodeLineEditingController.fromText('');
  final TextEditingController _commentController = TextEditingController();
  final CodeLineEditingController _changeController =
      CodeLineEditingController.fromText('');
  final CodeLineEditingController _ddlController =
      CodeLineEditingController.fromText('');
  // 每个编辑器一个查找控制器：CodeFindController 绑定其编辑控制器，须随本页 dispose
  late final CodeFindController _findController =
      CodeFindController(_controller);
  late final CodeFindController _changeFindController =
      CodeFindController(_changeController);
  late final CodeFindController _ddlFindController =
      CodeFindController(_ddlController);
  final TextEditingController _indexName = TextEditingController();
  final TextEditingController _indexColumns = TextEditingController();
  final TextEditingController _storageOptions = TextEditingController();

  /// 主页签下标(0 定义 / 1 索引 / 2 高级 / 3 注释 / 4 SQL 预览)
  int _tabIndex = 0;

  /// SQL 预览子页(0 更改 / 1 DDL)
  int _previewSub = 0;

  bool _saving = false;
  bool _busy = false;

  // ── 底部 消息 / 解释 面板 ──
  bool _panelVisible = false;
  int _panelTab = 0;
  String _messages = '';
  String _explainText = '';

  // ── 索引页签 ──
  List<MatViewIndexRow> _indexes = const [];
  int? _selectedIndex;
  List<String> _columns = const [];
  bool _indexUnique = false;

  // ── 高级页签 ──
  List<String> _owners = const [];
  String? _owner;
  String? _origOwner;
  List<String> _tablespaces = const [];
  String _tablespace = '';
  List<String> _accessMethods = const [];
  String _accessMethod = '';
  bool _withData = true;

  /// 目录里是否已存在「无 WHERE 且覆盖全部列的唯一索引」;null = 读不出(未知)
  bool? _concurrentlyReady;

  /// 全屏(收起左右侧栏)状态与恢复快照
  bool _fullscreen = false;
  bool _wasLeftVisible = true;
  bool _wasRightVisible = true;

  AppState? _appState;

  /// 与其它设计页同格式的持久化 key(切换 tab 不丢编辑内容)
  String get _tabKey =>
      'design|${widget.connection}|${widget.database}|${widget.schema}|'
      '${widget.isNew ? '${widget.name} (新建)' : '${widget.name} (设计)'}'
      '|${ObjectCategory.materializedView.name}';

  ConnectionInfo? get _conn => _appState?.connectionByName(widget.connection);

  @override
  void initState() {
    super.initState();
    _appState = context.read<AppState>();
    _commentController.text = widget.comment.isNotEmpty
        ? widget.comment
        : _appState!.routineCommentFor(_tabKey);
    _controller.text = _appState!.routineTextFor(_tabKey);
    _controller.addListener(_onEditorChanged);
    _commentController.addListener(_onCommentChanged);
    if (!widget.isNew && _controller.text.trim().isEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _load();
      });
    }
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _appState = context.read<AppState>();
  }

  @override
  void dispose() {
    _controller.removeListener(_onEditorChanged);
    _commentController.removeListener(_onCommentChanged);
    _appState?.updateRoutineText(_tabKey, _controller.text);
    _appState?.updateRoutineComment(_tabKey, _commentController.text);
    // findController 监听编辑控制器,必须先于它销毁
    _findController.dispose();
    _controller.dispose();
    _commentController.dispose();
    _changeFindController.dispose();
    _changeController.dispose();
    _ddlFindController.dispose();
    _ddlController.dispose();
    _indexName.dispose();
    _indexColumns.dispose();
    _storageOptions.dispose();
    super.dispose();
  }

  void _onEditorChanged() {
    _appState?.updateRoutineText(_tabKey, _controller.text);
  }

  void _onCommentChanged() {
    _appState?.updateRoutineComment(_tabKey, _commentController.text);
  }

  // ── 加载:属性 / 定义 / 索引 / 候选 ──────────────────────

  Future<void> _load() async {
    final conn = _conn;
    if (conn == null) return;
    try {
      final props = await _rows(MatViewSql.propsSql(widget.name,
          schema: widget.schema));
      final def = await _scalar(
          MatViewSql.viewDefinitionSql(widget.name, schema: widget.schema));
      if (!mounted) return;
      if (def != null && def.trim().isNotEmpty && _controller.text.trim().isEmpty) {
        setState(() => _controller.text = def.trim());
      }
      if (props.isNotEmpty) {
        final p = MatViewSql.parseProps(props.first);
        setState(() {
          _owner = _origOwner = p.owner;
          _tablespace = p.tablespace;
          _accessMethod = p.accessMethod;
          _withData = p.populated;
          _storageOptions.text = p.storageOptions.join(', ');
          // 注释框可能已被「上次的编辑」填过,只有真空着才回填目录值
          if (_commentController.text.isEmpty) {
            _commentController.text = p.comment;
          }
        });
        if (!p.populated) {
          setState(() {
            _panelVisible = true;
            _panelTab = 0;
            _messages = context.l10n.matvNotPopulated(widget.name);
          });
        }
      }
      await _loadIndexes();
      await _loadColumns();
      await _loadCandidates();
    } catch (e) {
      if (!mounted) return;
      MessageBox.show(
        context,
        title: context.l10n.actionDesign(context.l10n.catMaterializedView),
        message: context.l10n.matvDefinitionReadFailed(widget.name, '$e'),
        type: MessageBoxType.error,
        okText: context.l10n.btnGotIt,
      );
    }
  }

  Future<void> _loadIndexes() async {
    final rows = await _rows(
        MatViewSql.indexesSql(widget.name, schema: widget.schema));
    if (!mounted) return;
    setState(() {
      _indexes = [for (final r in rows) MatViewSql.parseIndexRow(r)];
      _selectedIndex = null;
    });
    // 索引变了,「能否并发刷新」的答案也可能变
    await _loadConcurrentlyReady();
  }

  Future<void> _loadColumns() async {
    final rows = await _rows(
        MatViewSql.columnsSql(widget.name, schema: widget.schema));
    if (!mounted) return;
    setState(() => _columns = [for (final r in rows) r.first]);
  }

  /// 「能否并发刷新」由 PG 自己判定:它要求无 WHERE 且覆盖全部列的唯一索引。
  /// 探测语句读不出时留 null(未知),界面据此放行、由服务端报错兜底。
  Future<void> _loadConcurrentlyReady() async {
    final rows = await _rows(
        MatViewSql.concurrentlyReadySql(widget.name, schema: widget.schema));
    final ready = MatViewSql.parseExists(rows);
    if (ready == null || !mounted) return;
    setState(() => _concurrentlyReady = ready);
  }

  Future<void> _loadCandidates() async {
    final owners = await _rows(MatViewSql.ownersSql);
    final tablespaces = await _rows(MatViewSql.tablespacesSql);
    final ams = await _rows(MatViewSql.accessMethodsSql);
    if (!mounted) return;
    setState(() {
      _owners = [for (final r in owners) r.first];
      _tablespaces = [for (final r in tablespaces) r.first];
      _accessMethods = [for (final r in ams) r.first];
    });
  }

  /// 执行查询并返回行(失败静默为空集——元数据读取不阻断设计器主流程)
  Future<List<List<String>>> _rows(String sql) async {
    final app = _appState ?? context.read<AppState>();
    final conn = _conn;
    if (conn == null) return const [];
    try {
      final r = await app.connectionManager.runQuery(conn, sql,
          database: widget.database, limit: 1000);
      return r.rows;
    } catch (_) {
      return const [];
    }
  }

  Future<String?> _scalar(String sql) async =>
      MatViewSql.firstCell(await _rows(sql));

  // ── SQL 拼装 ────────────────────────────────────────────

  /// 高级页当前值组成的创建期规格(存储参数 / 表空间 / 访问方法随 CREATE 落库)
  MatViewSpec _spec() => MatViewSpec(
        name: widget.name,
        schema: widget.schema,
        select: MatViewSql.stripTrailingSemicolon(_controller.text),
        tablespace: _tablespace,
        accessMethod: _accessMethod,
        storageOptions: MatViewSql.parseOptionsText(_storageOptions.text),
        withData: _withData,
      );

  /// 「更改」子页:编辑场景把 DROP 与 CREATE 都摊开(重建是破坏性的,不能只给一半)
  String _changeSql() {
    final buf = StringBuffer();
    if (!widget.isNew) {
      buf.writeln(MatViewSql.dropSql(widget.name, schema: widget.schema));
      buf.writeln();
    }
    buf.write(MatViewSql.createSql(_spec()));
    return buf.toString();
  }

  /// 「DDL」子页:CREATE + 现有索引 + 归属 + 注释,与 [_onSave] 的执行集合同形
  String _ddlSql() => MatViewSql.fullScript(
        spec: _spec(),
        dropFirst: widget.isNew ? null : MatViewSql.dropSql(widget.name, schema: widget.schema),
        indexes: [for (final i in _indexes) i.definition],
        owner: (_owner ?? '').isEmpty || _owner == _origOwner ? '' : _owner!,
        comment: _commentController.text.trim(),
      );

  // ── 工具栏动作 ──────────────────────────────────────────

  Future<void> _onSave() async {
    final l = context.l10n;
    if (_controller.text.trim().isEmpty) {
      await MessageBox.show(
        context,
        title: l.matvSaveTitle,
        message: l.matvDefinitionEmpty,
        type: MessageBoxType.warning,
        okText: l.btnGotIt,
      );
      return;
    }
    final app = _appState ?? context.read<AppState>();
    final conn = _conn;
    if (conn == null) return;
    if (!widget.isNew) {
      final go = await MessageBox.show(
        context,
        title: l.matvSaveTitle,
        message: l.matvRebuildConfirm(widget.name),
        type: MessageBoxType.question,
        buttons: MessageBoxButtons.okCancel,
        okText: l.btnSave,
        cancelText: l.btnCancel,
      );
      if (go != MessageBoxResult.ok) return;
      // 重建是破坏性的:先用 EXPLAIN 验定义(解析 + 规划,不执行),
      // 免得 DROP 成功、CREATE 失败把对象删成残局。
      final probe = await _runQuiet(MatViewSql.explainSql(_controller.text));
      if (!mounted) return;
      if (probe.isNotEmpty) {
        MessageBox.show(
          context,
          title: l.matvSaveTitle,
          message: l.matvSaveFailed(probe.trim()),
          type: MessageBoxType.error,
          okText: l.btnGotIt,
        );
        return;
      }
    }
    setState(() => _saving = true);
    // DROP 前重新抓一次索引原文:页面上的清单可能不是最新的(别处建过索引)
    if (!widget.isNew) await _loadIndexes();
    final indexDefs = [for (final i in _indexes) i.definition];
    final spec = _spec();
    final extraErrs = <String>[];
    var fail = '';
    if (!widget.isNew) {
      fail = await _runQuiet(
          MatViewSql.dropSql(widget.name, schema: widget.schema));
    }
    if (fail.isEmpty) {
      final outcome = await app.runDdl(MatViewSql.createSql(spec),
          connection: widget.connection,
          database: widget.database,
          schema: widget.schema);
      if (!outcome.ok) fail = outcome.error ?? '';
    }
    if (!mounted) return;
    if (fail.isNotEmpty) {
      setState(() => _saving = false);
      MessageBox.show(
        context,
        title: l.matvSaveTitle,
        // DROP 就失败时对象还是原样;CREATE 失败才说明视图已被重建流程影响,
        // 两种情况都把服务端原因原样交出,不粉饰
        message: l.matvSaveFailed(fail),
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
      return;
    }
    // 重建后回放索引,再补归属与注释;这些附属语句失败不推翻主保存
    for (final def in indexDefs) {
      final err = await _runQuiet(def.trimRight().endsWith(';') ? def : '$def;');
      if (err.isNotEmpty) extraErrs.add(err);
    }
    final ownerSql = (_owner ?? '').isEmpty || _owner == _origOwner
        ? ''
        : MatViewSql.ownerSql(widget.name, _owner!, schema: widget.schema);
    if (ownerSql.isNotEmpty) {
      final err = await _runQuiet(ownerSql);
      if (err.isNotEmpty) extraErrs.add(err);
    }
    final comment = _commentController.text.trim();
    if (comment.isNotEmpty) {
      final err = await _runQuiet(
          MatViewSql.commentSql(widget.name, comment, schema: widget.schema));
      if (err.isNotEmpty) extraErrs.add(err);
    }
    if (!mounted) return;
    await _loadIndexes();
    if (!mounted) return;
    setState(() {
      _saving = false;
      _origOwner = _owner;
      _panelVisible = true;
      _panelTab = 0;
      _messages = '${l.matvSaved(widget.name)}'
          '${extraErrs.map((e) => '\n${e.trim()}').join()}';
    });
  }

  /// 执行一条语句,返回空串表示成功;失败返回原因(不弹界面,由调用方汇总)
  Future<String> _runQuiet(String sql) async {
    final app = _appState ?? context.read<AppState>();
    final conn = _conn;
    if (conn == null || sql.trim().isEmpty) return '';
    try {
      await app.connectionManager.runQuery(conn, sql,
          database: widget.database, limit: 1);
      return '';
    } catch (e) {
      return '$e';
    }
  }

  /// 刷新 / 并发刷新。走 [AppState.runDdl]:执行成功后顺带刷新对象列表,
  /// 估算行数(reltuples)才会跟着变。
  Future<void> _onRefresh({bool concurrently = false}) async {
    final l = context.l10n;
    final app = _appState ?? context.read<AppState>();
    final conn = _conn;
    if (conn == null) return;
    if (concurrently) {
      await _loadConcurrentlyReady();
      if (_concurrentlyReady == false) {
        final go = await MessageBox.show(
          context,
          title: l.actionRefreshConcurrently(l.catMaterializedView),
          message: '${l.matvConcurrentlyNeedUnique}\n${l.matvConcurrentlyAsk}',
          type: MessageBoxType.warning,
          buttons: MessageBoxButtons.okCancel,
          okText: l.btnOk,
          cancelText: l.btnCancel,
        );
        if (go != MessageBoxResult.ok || !mounted) return;
      }
    }
    setState(() {
      _busy = true;
      _panelVisible = true;
      _panelTab = 0;
      _messages = l.matvRefreshRunning;
    });
    final sw = Stopwatch()..start();
    final outcome = await app.runDdl(
        MatViewSql.refreshSql(widget.name,
            schema: widget.schema, concurrently: concurrently),
        connection: widget.connection,
        database: widget.database,
        schema: widget.schema);
    if (!mounted) return;
    final ms = '${sw.elapsedMilliseconds}';
    if (!outcome.ok) {
      setState(() {
        _busy = false;
        _messages = l.matvRefreshFailed('${outcome.error}');
      });
      MessageBox.show(
        context,
        title: concurrently
            ? l.actionRefreshConcurrently(l.catMaterializedView)
            : l.actionRefresh(l.catMaterializedView),
        message: l.matvRefreshFailed('${outcome.error}'),
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
      return;
    }
    setState(() {
      _busy = false;
      _withData = true;
      _messages = l.matvRefreshed(widget.name, ms);
    });
  }

  /// 预览:执行定义 SELECT,行数 / 错误写入「消息」页
  Future<void> _onPreview() async {
    final l = context.l10n;
    final select = MatViewSql.stripTrailingSemicolon(_controller.text);
    if (select.isEmpty) {
      setState(() {
        _panelVisible = true;
        _panelTab = 0;
        _messages = l.matvPreviewEmpty;
      });
      return;
    }
    final app = _appState ?? context.read<AppState>();
    final conn = _conn;
    if (conn == null) return;
    setState(() {
      _busy = true;
      _panelVisible = true;
      _panelTab = 0;
      _messages = '${l.matvPreview} ...';
    });
    final sw = Stopwatch()..start();
    try {
      final r = await app.connectionManager.runQuery(conn, select,
          database: widget.database, schema: widget.schema, limit: 200);
      if (!mounted) return;
      setState(() {
        _busy = false;
        _messages = l.matvPreviewResult('${r.rows.length}',
            '${sw.elapsedMilliseconds}');
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _messages = l.matvPreviewFailed('$e');
      });
    }
  }

  /// 解释:EXPLAIN 定义 SELECT,执行计划写入「解释」页
  Future<void> _onExplain() async {
    final l = context.l10n;
    final select = MatViewSql.stripTrailingSemicolon(_controller.text);
    if (select.isEmpty) {
      setState(() {
        _panelVisible = true;
        _panelTab = 1;
        _explainText = l.matvExplainEmpty;
      });
      return;
    }
    final app = _appState ?? context.read<AppState>();
    final conn = _conn;
    if (conn == null) return;
    setState(() {
      _busy = true;
      _panelVisible = true;
      _panelTab = 1;
      _explainText = '${l.matvExplain} ...';
    });
    try {
      final r = await app.connectionManager.runQuery(
          conn, MatViewSql.explainSql(select),
          database: widget.database, schema: widget.schema, limit: 500);
      if (!mounted) return;
      setState(() {
        _busy = false;
        _explainText = r.rows.map((row) => row.join('  ')).join('\n');
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _explainText = l.matvExplainFailed('$e');
      });
    }
  }

  /// 美化 SQL:与查询页共用 formatSql
  void _onFormat() {
    if (_controller.text.trim().isEmpty) return;
    _controller.text = formatSql(_controller.text);
  }

  /// 添加索引:立即执行 CREATE INDEX(与视图设计器「删除规则」同口径)
  Future<void> _onAddIndex() async {
    final l = context.l10n;
    final sql = MatViewSql.createIndexSql(
      widget.name,
      _indexName.text,
      _indexColumns.text,
      schema: widget.schema,
      unique: _indexUnique,
    );
    if (sql.isEmpty) {
      _toastSelection(l.matvIndexIncomplete);
      return;
    }
    final err = await _runQuiet(sql);
    if (!mounted) return;
    if (err.isNotEmpty) {
      MessageBox.show(
        context,
        title: l.matvAddIndex,
        message: l.matvSaveFailed(err.trim()),
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
      return;
    }
    _indexName.clear();
    _indexColumns.clear();
    setState(() => _indexUnique = false);
    await _loadIndexes();
  }

  Future<void> _onDropIndex() async {
    final l = context.l10n;
    final row = (_selectedIndex == null || _selectedIndex! >= _indexes.length)
        ? null
        : _indexes[_selectedIndex!];
    if (row == null) {
      _toastSelection(l.matvIndexNoSelection);
      return;
    }
    final result = await MessageBox.show(
      context,
      title: l.matvDropIndex,
      message: l.matvIndexDropConfirm(row.name),
      type: MessageBoxType.warning,
      buttons: MessageBoxButtons.okCancel,
      okText: l.btnDelete,
      cancelText: l.btnCancel,
    );
    if (result != MessageBoxResult.ok || !mounted) return;
    final err = await _runQuiet(
        MatViewSql.dropIndexSql(row.name, schema: widget.schema));
    if (!mounted) return;
    if (err.isNotEmpty) {
      MessageBox.show(
        context,
        title: l.matvDropIndex,
        message: l.deleteFailedDetail(err.trim()),
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
      return;
    }
    await _loadIndexes();
  }

  /// 未选中 / 未填写时的提示:走消息面板,不打断操作(与 Navicat 一致的快节奏)
  void _toastSelection(String message) {
    setState(() {
      _panelVisible = true;
      _panelTab = 0;
      _messages = message;
    });
  }

  /// 全屏:收起 / 恢复左右侧栏,设计器占满中间区域
  void _onFullscreen() {
    final app = _appState!;
    setState(() {
      if (!_fullscreen) {
        _wasLeftVisible = app.leftPanelVisible;
        _wasRightVisible = app.rightPanelVisible;
        if (app.leftPanelVisible) app.toggleLeftPanel();
        if (app.rightPanelVisible) app.toggleRightPanel();
      } else {
        if (_wasLeftVisible && !app.leftPanelVisible) app.toggleLeftPanel();
        if (_wasRightVisible && !app.rightPanelVisible) app.toggleRightPanel();
      }
      _fullscreen = !_fullscreen;
    });
  }

  void _selectIndex(int row) {
    setState(() => _selectedIndex = row);
  }

  // ── 构建 ────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final l = context.l10n;
    return ColoredBox(
      color: t.background,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _toolbar(t, l),
          Expanded(
            child: TabControl(
              contentPadding: EdgeInsets.zero,
              tabBarColor: t.secondary,
              selectedTabColor: t.surface,
              onChanged: (i) => setState(() => _tabIndex = i),
              tabs: [
                TabItem(label: l.matvTabDefinition, child: _definitionTab(t)),
                TabItem(label: l.matvTabIndexes, child: _indexesTab(t, l)),
                TabItem(label: l.matvTabAdvanced, child: _advancedTab(t, l)),
                TabItem(label: l.matvTabComment, child: _commentTab(t, l)),
                TabItem(label: l.matvTabSqlPreview, child: _previewTab(t)),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _toolbar(AppPalette t, AppLocalizations l) {
    // 图标色走业务色板而不是写死十六进制:写死的是亮色档,暗色下几乎看不见
    final c = AppColors.of(context);
    final blue = c.iconPrimary;
    return Container(
      height: 34,
      color: t.secondary,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        children: [
          ToolbarButton(
            icon: Icons.save_outlined,
            iconColor: blue,
            text: l.btnSave,
            enabled: !_saving && !_busy,
            onTap: _onSave,
          ),
          _divider(t),
          ToolbarButton(
            icon: Icons.refresh,
            iconColor: blue,
            text: l.ctxRefresh,
            enabled: !widget.isNew && !_busy && !_saving,
            onTap: () => _onRefresh(),
          ),
          ToolbarButton(
            icon: Icons.sync_alt,
            iconColor: blue,
            text: l.matvToolbarRefreshConcurrently,
            enabled: !widget.isNew && !_busy && !_saving,
            onTap: () => _onRefresh(concurrently: true),
          ),
          _divider(t),
          ToolbarButton(
            icon: Icons.visibility_outlined,
            iconColor: blue,
            text: l.matvPreview,
            enabled: !_busy && !_saving,
            onTap: _onPreview,
          ),
          ToolbarButton(
            icon: Icons.account_tree_outlined,
            iconColor: blue,
            text: l.matvExplain,
            enabled: !_busy && !_saving,
            onTap: _onExplain,
          ),
          ToolbarButton(
            icon: Icons.auto_fix_high_outlined,
            text: l.matvBeautifySql,
            onTap: _onFormat,
          ),
          if (_tabIndex == 1) ...[
            const SizedBox(width: 18),
            ToolbarButton(
              icon: Icons.add_circle_outline,
              iconColor: c.iconSuccess,
              text: l.matvAddIndex,
              enabled: !_busy && !_saving,
              onTap: _onAddIndex,
            ),
            ToolbarButton(
              icon: Icons.remove_circle_outline,
              // 业务色板没有「危险」档,沿用视图设计器的同一个红
              iconColor: const Color(0xFFFF4A37),
              text: l.matvDropIndex,
              enabled: _selectedIndex != null,
              onTap: _onDropIndex,
            ),
          ],
          const Spacer(),
          IconBtn(
            icon: _fullscreen ? Icons.fullscreen_exit : Icons.fullscreen,
            iconSize: 16,
            tooltip: _fullscreen ? l.matvExitFullscreen : l.matvFullscreen,
            onTap: _onFullscreen,
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

  // ── 定义 ──

  Widget _definitionTab(AppPalette t) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Expanded(
          child: _sqlEditor(
            t,
            controller: _controller,
            readOnly: false,
            findController: _findController,
          ),
        ),
        if (_panelVisible) _bottomPanel(t),
      ],
    );
  }

  /// 底部「消息 / 解释」面板(预览 / 解释 / 刷新 / 保存结果输出)
  Widget _bottomPanel(AppPalette t) {
    final l = context.l10n;
    return Container(
      height: 200,
      decoration: BoxDecoration(
        border: Border(top: BorderSide(color: t.border)),
        color: t.surface,
      ),
      child: TabControl(
        initialIndex: _panelTab,
        contentPadding: EdgeInsets.zero,
        tabBarColor: t.secondary,
        selectedTabColor: t.surface,
        onChanged: (i) => setState(() => _panelTab = i),
        tabs: [
          TabItem(
            label: l.matvPanelMessages,
            child: Padding(
              padding: const EdgeInsets.all(8),
              child: SingleChildScrollView(
                child: SelectableText(
                  _messages,
                  style: TextStyle(
                    fontSize: 12.5,
                    color: t.foreground,
                    decoration: TextDecoration.none,
                  ),
                ),
              ),
            ),
          ),
          TabItem(
            label: l.matvExplain,
            child: Padding(
              padding: const EdgeInsets.all(8),
              child: SingleChildScrollView(
                child: SelectableText(
                  _explainText,
                  style: TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 12.5,
                    height: 20 / 12.5,
                    color: t.foreground,
                    decoration: TextDecoration.none,
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  // ── 索引 ──

  Widget _indexesTab(AppPalette t, AppLocalizations l) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Expanded(
          child: Padding(
            padding: const EdgeInsets.fromLTRB(8, 8, 8, 0),
            child: DataGridView(
              columns: [
                DataGridViewColumn(title: l.colName, width: 220, flex: 0),
                DataGridViewColumn(title: l.colUnique, width: 70, flex: 0),
                DataGridViewColumn(title: l.matvColDefinition),
              ],
              rowCount: _indexes.length,
              selectedRow: _selectedIndex,
              onRowSelected: _selectIndex,
              cellBuilder: (row, col) {
                final idx = _indexes[row];
                final text = switch (col) {
                  0 => idx.name,
                  1 => idx.unique ? '✓' : '',
                  _ => idx.definition,
                };
                return Text(
                  text,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12.5,
                    color: t.foreground,
                    decoration: TextDecoration.none,
                  ),
                );
              },
            ),
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(8, 10, 8, 12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              FieldRow(
                label: l.matvFieldIndexName,
                labelWidth: 120,
                child: SizedBox(
                  width: 300,
                  child: Input(
                    controller: _indexName,
                    onChanged: (_) => setState(() {}),
                  ),
                ),
              ),
              const SizedBox(height: 8),
              FieldRow(
                label: l.matvFieldIndexColumns,
                labelWidth: 120,
                child: SizedBox(
                  width: 300,
                  child: Input(
                    controller: _indexColumns,
                    hint: _columns.isEmpty
                        ? null
                        : l.matvColumnsHint(_columns.join(', ')),
                    onChanged: (_) => setState(() {}),
                  ),
                ),
              ),
              const SizedBox(height: 8),
              FieldRow(
                label: '',
                child: Row(
                  children: [
                    CheckBox(
                      value: _indexUnique,
                      onChanged: (v) => setState(() => _indexUnique = v ?? false),
                      label: l.colUnique,
                    ),
                    const SizedBox(width: 16),
                    // 并发刷新的硬前提,直接说人话,省得用户撞上限时报错
                    Text(
                      switch (_concurrentlyReady) {
                        null => '',
                        true => '✓ ${l.matvConcurrentlyOk}',
                        false => l.matvConcurrentlyNeedUnique,
                      },
                      style: TextStyle(fontSize: 12, color: t.mutedForeground),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  // ── 高级 ──

  Widget _advancedTab(AppPalette t, AppLocalizations l) {
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(8, 18, 8, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          FieldRow(
            label: l.matvFieldOwner,
            labelWidth: 140,
            child: SizedBox(
              width: 240,
              child: ComboBox<String>(
                items: _owners,
                value: _owner,
                enabled: _owners.isNotEmpty,
                itemToString: (v) => v,
                onChanged: (v) => setState(() => _owner = v),
              ),
            ),
          ),
          const SizedBox(height: 14),
          FieldRow(
            label: l.matvFieldTablespace,
            labelWidth: 140,
            child: SizedBox(
              width: 240,
              child: ComboBox<String>(
                items: ['', ..._tablespaces],
                value: _tablespace,
                itemToString: (v) => v.isEmpty ? l.matvDefault : v,
                onChanged: (v) => setState(() => _tablespace = v ?? ''),
              ),
            ),
          ),
          const SizedBox(height: 14),
          FieldRow(
            label: l.matvFieldAccessMethod,
            labelWidth: 140,
            child: SizedBox(
              width: 240,
              child: ComboBox<String>(
                items: ['', ..._accessMethods],
                value: _accessMethod,
                itemToString: (v) => v.isEmpty ? l.matvDefault : v,
                onChanged: (v) => setState(() => _accessMethod = v ?? ''),
              ),
            ),
          ),
          const SizedBox(height: 14),
          FieldRow(
            label: l.matvFieldStorageParams,
            labelWidth: 140,
            child: SizedBox(
              width: 420,
              child: Input(
                controller: _storageOptions,
                hint: 'fillfactor = 90, autovacuum_enabled = false',
                onChanged: (_) => setState(() {}),
              ),
            ),
          ),
          const SizedBox(height: 14),
          FieldRow(
            label: '',
            child: CheckBox(
              value: _withData,
              onChanged: (v) => setState(() => _withData = v ?? true),
              label: l.matvWithData,
            ),
          ),
        ],
      ),
    );
  }

  // ── 注释 ──

  Widget _commentTab(AppPalette t, AppLocalizations l) {
    return Padding(
      padding: const EdgeInsets.all(8),
      child: Textarea(
        controller: _commentController,
        expands: true,
        showBorder: false,
        hint: l.matvCommentHint,
        style: TextStyle(
          fontSize: 13.5,
          height: 20 / 13.5,
          color: t.foreground,
        ),
      ),
    );
  }

  // ── SQL 预览 ──

  Widget _previewTab(AppPalette t) {
    final l = context.l10n;
    return ListenableBuilder(
      listenable: Listenable.merge(
          [_controller, _commentController, _indexName, _indexColumns]),
      builder: (context, _) {
        final change = _changeSql();
        final ddl = _ddlSql();
        if (_changeController.text != change) _changeController.text = change;
        if (_ddlController.text != ddl) _ddlController.text = ddl;
        final text = _previewSub == 0 ? change : ddl;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TabStrip(
              items: [l.matvTabChange, l.infoPageDdl],
              index: _previewSub,
              height: 28,
              background: t.secondary,
              onChanged: (i) => setState(() => _previewSub = i),
            ),
            Expanded(
              child: Stack(
                children: [
                  Positioned.fill(
                    child: _sqlEditor(
                      t,
                      controller: _previewSub == 0
                          ? _changeController
                          : _ddlController,
                      readOnly: true,
                      findController: _previewSub == 0
                          ? _changeFindController
                          : _ddlFindController,
                      showLineNumbers: false,
                    ),
                  ),
                  Positioned(
                    top: 6,
                    right: 10,
                    child: IconBtn(
                      icon: Icons.copy_outlined,
                      iconSize: 15,
                      tooltip: l.matvCopy,
                      onTap: () =>
                          Clipboard.setData(ClipboardData(text: text)),
                    ),
                  ),
                ],
              ),
            ),
          ],
        );
      },
    );
  }

  // ── 代码编辑器(定义 / 预览共用)──

  Widget _sqlEditor(
    AppPalette t, {
    required CodeLineEditingController controller,
    required bool readOnly,
    required CodeFindController findController,
    bool showLineNumbers = true,
  }) {
    final isDark = t.isDark;
    return CodeEditor(
      controller: controller,
      readOnly: readOnly,
      findController: findController,
      findBuilder: (context, controller, readOnly) =>
          SqlFindBar(controller: controller, readOnly: readOnly),
      shortcutsActivatorsBuilder: const SqlFindShortcuts(),
      chunkAnalyzer: const NonCodeChunkAnalyzer(),
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
      style: CodeEditorStyle(
        fontFamily: 'monospace',
        fontSize: 13.5,
        fontHeight: 20 / 13.5,
        textColor: t.foreground,
        backgroundColor: t.surface,
        hintTextColor: t.disabledForeground,
        selectionColor: Color.alphaBlend(
          t.accent.withValues(alpha: 0.30),
          t.surface,
        ),
        cursorColor: t.foreground,
        codeTheme: CodeHighlightTheme(
          languages: {
            'sql': CodeHighlightThemeMode(mode: langSql),
          },
          theme: sqlEditorTheme(isDark: isDark, foreground: t.foreground),
        ),
      ),
      indicatorBuilder: showLineNumbers
          ? (context, editingController, chunkController, notifier) =>
              DefaultCodeLineNumber(
            controller: editingController,
            notifier: notifier,
            textStyle: TextStyle(
                fontSize: 12.5, color: t.disabledForeground),
            focusedTextStyle: TextStyle(
                fontSize: 12.5, color: t.mutedForeground),
            minNumberCount: 2,
          )
          : null,
    );
  }
}
