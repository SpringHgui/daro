import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show Clipboard, ClipboardData;
import 'package:provider/provider.dart';
import 'package:re_editor/re_editor.dart';
import 'package:re_highlight/languages/sql.dart';

import '../app/app_state.dart';
import '../data/db_data.dart';
import '../data/routine_sql.dart';
import '../theme/app_theme.dart';
import '../theme/sql_highlight.dart';
import 'query_page.dart' show formatSql;
import 'sql_find_bar.dart';
import 'view_rule_dialog.dart';

/// 工具栏度量:与「设计表」同一档 —— 纯图标点击块 25 宽(14 图标 + 5.5 × 2)、
/// 栏高 29、分隔块 17(1px 线 + 两侧 8 留白)。
const double _kToolbarH = 29;
const double _kIconPadX = (25 - 14) / 2;
const double _kToolSepW = 17;

/// 添加 / 删除图标色(与「设计表」工具栏同一对)
const Color _iconAdd = Color(0xFF45C300);
const Color _iconDanger = Color(0xFFFF4A37);

/// 视图设计器(按 Navicat 视图设计器 1:1 还原)。
///
/// 工具栏:保存 / 预览 / 解释 / 视图创建工具 / 美化 SQL(纯图标 + tooltip);
/// 「规则」页签追加 添加规则 / 删除规则;右缘全屏按钮收起 / 恢复两侧面板。
/// 页签:定义(仅 SELECT 查询)/ 规则(PG pg_rewrite 视图规则,仅 PG 出现)/
/// 高级(所有者 / 检查选项 / 安全屏障)/ 注释 / SQL 预览(更改 / DDL 子页)。
/// 「预览 / 解释」结果输出到定义页底部「消息 / 解释」面板。
///
/// 定义模型与 Navicat 一致:编辑区只存 SELECT,保存时由
/// CREATE VIEW + 高级选项拼装完整 DDL(见 [_changeStatements])。
/// 「更改」页与「保存」共用同一份语句清单,DDL 页则拿 [_baseline]
/// (打开时库里读回的快照)与界面当前值做逐行对比。
class ViewDesignPage extends StatefulWidget {
  const ViewDesignPage({
    super.key,
    required this.name,
    required this.connection,
    required this.database,
    required this.category,
    this.schema,
    this.isNew = false,
    this.comment = '',
    this.dialectOverride,
  });

  final String name;
  final String connection;
  final String database;
  final String? schema;
  final ObjectCategory category;
  final bool isNew;
  final String comment;

  /// 只给方言、不给连接的测试入口(与 [TableDesignerPage.dialectOverride]
  /// 同一条理由):单测里一旦注册真连接,`_load` 就真去建驱动连库,
  /// 断言永远等不到静止帧。方言门控的分支必须能脱离连接被测。
  final String? dialectOverride;

  @override
  State<ViewDesignPage> createState() => _ViewDesignPageState();
}

/// 一条视图规则(pg_rewrite;非 PG 类型列表为空)
class _ViewRule {
  const _ViewRule({
    required this.name,
    required this.oid,
    required this.event,
    required this.instead,
    required this.comment,
    required this.definition,
  });
  final String name;
  final String oid;
  final String event;
  final String instead;
  final String comment;
  final String definition;
}

/// 生成语句所需的全部输入:定义主体 + 高级页选项 + 注释。
///
/// 「更改」页、DDL 对比与「保存」三条路径都读它,谁都不许自己拼一份,
/// 否则预览和实际执行的语句会分叉(预览说一套、库里落一套)。
class _ViewState {
  const _ViewState({
    this.select = '',
    this.securityBarrier = false,
    this.checkOption = '',
    this.owner,
    this.comment = '',
  });

  final String select;
  final bool securityBarrier;

  /// '' = 不加;否则 LOCAL / CASCADED
  final String checkOption;
  final String? owner;
  final String comment;
}

class _ViewDesignPageState extends State<ViewDesignPage> {
  final CodeLineEditingController _controller =
      CodeLineEditingController.fromText('');
  final FocusNode _focusNode = FocusNode();
  late final CodeFindController _findController =
      CodeFindController(_controller);
  final TextEditingController _commentController = TextEditingController();
  final CodeLineEditingController _changeController =
      CodeLineEditingController.fromText('');
  /// 「更改」预览区的查找控制器:每个编辑区要各配一个(见 `_sqlEditor`)
  late final CodeFindController _changeFindController =
      CodeFindController(_changeController);

  /// 主页签下标(0 定义 / 1 规则 / 2 高级 / 3 注释 / 4 SQL 预览;
  /// 非 PG 没有规则页,下标整体前移一位)
  int _tabIndex = 0;

  /// SQL 预览子页(0 更改 / 1 DDL)
  int _previewSub = 0;

  bool _saving = false;
  bool _busy = false;

  /// 库里现有定义已读回(未读回之前 DDL 对比的「之前」栏只能留空)
  bool _loaded = false;

  // ── 底部 消息 / 解释 面板 ──
  bool _panelVisible = false;
  int _panelTab = 0;
  String _messages = '';
  String _explainText = '';

  // ── 规则页签 ──
  List<_ViewRule> _rules = const [];
  int? _selectedRule;
  final TextEditingController _ruleLocation = TextEditingController();
  final TextEditingController _ruleDefinition = TextEditingController();

  // ── 高级页签 ──
  List<String> _owners = const [];
  String? _owner;
  String? _origOwner;
  String _checkOption = '';
  bool _securityBarrier = false;

  /// 库里现有的注释(与 [_origOwner] 同性质:改动与否的基线)
  String _origComment = '';

  /// 打开时读到的状态快照 —— DDL 对比的「之前」栏;保存成功后换成新基线
  _ViewState _baseline = const _ViewState();

  /// 全屏(收起左右侧栏)状态与恢复快照
  bool _fullscreen = false;
  bool _wasLeftVisible = true;
  bool _wasRightVisible = true;

  AppState? _appState;

  /// 与 RoutineDesignPage 同格式的持久化 key(切换 tab 不丢编辑内容)
  String get _tabKey =>
      'design|${widget.connection}|${widget.database}|${widget.schema}|'
      '${widget.isNew ? '${widget.name} (新建)' : '${widget.name} (设计)'}'
      '|${widget.category.name}';

  ConnectionInfo? get _conn =>
      _appState?.connectionByName(widget.connection);

  String get _typeId => widget.dialectOverride ?? _conn?.typeId ?? '';
  bool get _isPg => _typeId == 'postgresql';

  /// WITH [LOCAL|CASCADED] CHECK OPTION 只有 MySQL / MariaDB 支持:
  /// PG 没有该子句,SQL Server 只认不带关键字的 `WITH CHECK OPTION`,
  /// SQLite 完全没有。界面与生成侧用同一个条件,不给「改了不生效」的入口。
  bool get _checkOptionSupported =>
      _typeId == 'mysql' || _typeId == 'mariadb';

  /// 规则(pg_rewrite)是 PG 独有对象:其它方言连「规则」页签都不出现
  bool get _hasRules => _isPg;

  /// 界面当前状态(预览与保存都从这一份生成)
  _ViewState get _current {
    var select = _controller.text.trim();
    while (select.endsWith(';')) {
      select = select.substring(0, select.length - 1).trimRight();
    }
    return _ViewState(
      select: select,
      securityBarrier: _securityBarrier,
      checkOption: _checkOption,
      owner: _owner,
      comment: _commentController.text.trim(),
    );
  }

  @override
  void initState() {
    super.initState();
    _appState = context.read<AppState>();
    _commentController.text = widget.comment.isNotEmpty
        ? widget.comment
        : _appState!.routineCommentFor(_tabKey);
    final saved = _appState!.routineTextFor(_tabKey);
    _controller.text = saved;
    _controller.addListener(_onEditorChanged);
    _commentController.addListener(_onCommentChanged);
    if (widget.isNew) {
      _loaded = true;
    } else {
      // 即使编辑区带着未保存的文本也要读库:所有者 / 规则 / 注释以及 DDL 对比
      // 的「之前」栏都出自这次加载(定义文本不会被它盖掉)
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
    _focusNode.dispose();
    _commentController.dispose();
    _changeFindController.dispose();
    _changeController.dispose();
    _ruleLocation.dispose();
    _ruleDefinition.dispose();
    super.dispose();
  }

  void _onEditorChanged() {
    _appState?.updateRoutineText(_tabKey, _controller.text);
  }

  void _onCommentChanged() {
    _appState?.updateRoutineComment(_tabKey, _commentController.text);
  }

  // ── 加载:定义 / 所有者 / 规则 ──────────────────────────

  Future<void> _load() async {
    final app = _appState ?? context.read<AppState>();
    final conn = _conn;
    if (conn == null) return;
    try {
      final def = await app.getObjectDefinition(
        widget.category,
        widget.name,
        connection: widget.connection,
        database: widget.database,
        schema: widget.schema,
      );
      if (!mounted) return;
      final split = (def == null || def.trim().isEmpty)
          ? null
          : splitViewDefinition(def);
      setState(() {
        if (split == null) return;
        _checkOption = split.checkOption;
        _securityBarrier = split.securityBarrier;
        // 标签页重开时编辑区里可能已有用户改过但没保存的文本,
        // 不能被库里的旧定义盖掉
        if (_controller.text.trim().isEmpty) _controller.text = split.select;
      });
      if (_isPg) {
        await _loadPgMeta();
        await _loadRules();
        await _loadOwners();
      }
      if (!mounted) return;
      // 基线取**库里读回的值**而不是界面当前值:界面可能带着未保存的改动,
      // 用它当基线就等于把用户改的东西说成「库里本来就是这样」
      setState(() {
        _loaded = true;
        _baseline = _ViewState(
          select: split?.select ?? '',
          securityBarrier: _securityBarrier,
          checkOption: _checkOption,
          owner: _origOwner,
          comment: _origComment,
        );
      });
    } catch (e) {
      if (!mounted) return;
      MessageBox.show(
        context,
        title: '设计视图',
        message: '读取视图「${widget.name}」定义失败:\n$e',
        type: MessageBoxType.error,
        okText: '知道了',
      );
    }
  }

  /// PG 一次查回视图的所有者 / 安全屏障 / 注释(三者都在 pg_class +
  /// pg_description 上,拆成三条语句就是三次驱动往返)。
  Future<void> _loadPgMeta() async {
    final rows = await _rows(
        "SELECT pg_get_userbyid(c.relowner), "
        "'security_barrier' = ANY(c.reloptions), "
        "COALESCE(d.description, '') "
        "FROM pg_class c "
        "JOIN pg_namespace n ON n.oid = c.relnamespace "
        "LEFT JOIN pg_description d ON d.objoid = c.oid AND d.objsubid = 0 "
        "WHERE n.nspname = ${_lit(widget.schema ?? 'public')} "
        "AND c.relname = ${_lit(widget.name)}");
    if (rows.isEmpty || !mounted) return;
    final r = rows.first;
    setState(() {
      _owner = _origOwner = r[0];
      // pg_get_viewdef 只给 SELECT 主体,CREATE 头部的 WITH 选项不在其中,
      // 所以 reloptions 才是安全屏障的权威来源(覆盖定义文本里的兜底判定)
      _securityBarrier = r.length > 1 && _truthy(r[1]);
      if (r.length > 2) {
        _origComment = r[2];
        if (_commentController.text.trim().isEmpty) {
          _commentController.text = r[2];
        }
      }
    });
  }

  static bool _truthy(String v) =>
      v == 'true' || v == 't' || v == '1' || v == 'yes';

  Future<void> _loadOwners() async {
    final rows = await _rows(
        'SELECT rolname FROM pg_roles ORDER BY rolname');
    if (!mounted) return;
    setState(() => _owners = [for (final r in rows) r.first]);
  }

  Future<void> _loadRules() async {
    // 隐式 _RETURN 规则(Navicat 同样不展示)排除在外
    final rows = await _rows(
        "SELECT r.rulname, r.oid::text, "
        "CASE r.evtype WHEN 'n' THEN 'NONE' WHEN 'i' THEN 'INSERT' "
        "WHEN 'u' THEN 'UPDATE' WHEN 'd' THEN 'DELETE' ELSE '' END, "
        "CASE WHEN r.evinstead THEN 'true' ELSE 'false' END, "
        "COALESCE(d.description, ''), pg_get_ruledef(r.oid) "
        "FROM pg_rewrite r "
        "JOIN pg_class c ON c.oid = r.ev_class "
        "JOIN pg_namespace n ON n.oid = c.relnamespace "
        "LEFT JOIN pg_description d ON d.objoid = r.oid AND d.objsubid = 0 "
        "WHERE n.nspname = ${_lit(widget.schema ?? 'public')} "
        "AND c.relname = ${_lit(widget.name)} "
        "AND r.rulname <> '_RETURN' ORDER BY r.rulname");
    if (!mounted) return;
    setState(() {
      _rules = [
        for (final r in rows)
          _ViewRule(
            name: r[0],
            oid: r[1],
            event: r[2],
            instead: r[3],
            comment: r[4],
            definition: r.length > 5 ? r[5] : '',
          ),
      ];
      _selectedRule = null;
      _ruleLocation.clear();
      _ruleDefinition.clear();
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

  static String _lit(String v) => "'${v.replaceAll("'", "''")}'";
  static String _dq(String v) => '"${v.replaceAll('"', '""')}"';

  // ── SQL 拼装 ────────────────────────────────────────────

  /// CREATE VIEW 语句:安全屏障(PG 的 WITH 选项)与检查选项并入同一句
  String _createViewSql(_ViewState s) {
    final q =
        RoutineSql.qualifiedName(_typeId, widget.name, schema: widget.schema);
    final buf = StringBuffer('CREATE VIEW $q');
    if (_isPg && s.securityBarrier) buf.write(' WITH (security_barrier)');
    buf.write(' AS ${s.select}');
    if (_checkOptionSupported && s.checkOption.isNotEmpty) {
      buf.write(' WITH ${s.checkOption} CHECK OPTION');
    }
    buf.write(';');
    return buf.toString();
  }

  /// 「保存」将执行的语句清单 —— 「更改」页与 [_onSave] 共用这一份,
  /// 预览即所见:界面说什么,库里就落什么。
  ///
  /// 编辑模式先 DROP 再 CREATE(视图没有可改的列定义,只能整条替换);
  /// 所有者 / 注释只在相对基线变了才补,且**清空注释要显式写 IS NULL**,
  /// 否则「删掉注释」这件事永远传不到库里。
  List<String> _changeStatements(_ViewState s) {
    final q =
        RoutineSql.qualifiedName(_typeId, widget.name, schema: widget.schema);
    return [
      if (!widget.isNew) 'DROP VIEW IF EXISTS $q;',
      _createViewSql(s),
      if (_isPg && s.owner != null && s.owner!.isNotEmpty && s.owner != _origOwner)
        'ALTER VIEW $q OWNER TO ${_dq(s.owner!)};',
      if (_isPg && s.comment != _origComment)
        'COMMENT ON VIEW $q IS ${s.comment.isEmpty ? 'NULL' : _lit(s.comment)};',
    ];
  }

  /// 对象自身的完整定义(DDL 对比用):不带「相对基线是否有变」的门,
  /// 所有者与注释按该状态有什么就写什么 —— 「之前」栏要照实反映库里的现状,
  /// 否则两边都不输出 ALTER/COMMENT,差异就无从对比。
  String _fullDdl(_ViewState s) {
    final q =
        RoutineSql.qualifiedName(_typeId, widget.name, schema: widget.schema);
    return [
      _createViewSql(s),
      if (_isPg && s.owner != null && s.owner!.isNotEmpty)
        'ALTER VIEW $q OWNER TO ${_dq(s.owner!)};',
      if (_isPg && s.comment.isNotEmpty)
        'COMMENT ON VIEW $q IS ${_lit(s.comment)};',
    ].join('\n\n');
  }

  // ── 工具栏动作 ──────────────────────────────────────────

  Future<void> _onSave() async {
    if (_controller.text.trim().isEmpty) {
      await MessageBox.show(
        context,
        title: '保存视图',
        message: '视图定义不能为空,请在「定义」页输入查询 SQL。',
        type: MessageBoxType.warning,
        okText: '知道了',
      );
      return;
    }
    final app = _appState ?? context.read<AppState>();
    final conn = _conn;
    if (conn == null) return;
    setState(() => _saving = true);
    // 与「更改」页同一份清单:首条 CREATE(编辑模式前面还有一条 DROP)走
    // 原有的替换路径,其后的所有者 / 注释按语句顺序补执行
    final stmts = _changeStatements(_current);
    final createIdx = widget.isNew ? 0 : 1;
    final extras = stmts.sublist(createIdx + 1);
    final outcome = widget.isNew
        ? await app.runDdl(stmts[createIdx],
            connection: widget.connection,
            database: widget.database,
            schema: widget.schema)
        : await app.replaceRoutine(
            widget.category,
            widget.name,
            createSql: stmts[createIdx],
            connection: widget.connection,
            database: widget.database,
            schema: widget.schema,
          );
    if (!mounted) return;
    if (!outcome.ok) {
      setState(() => _saving = false);
      MessageBox.show(
        context,
        title: '保存视图',
        message: '保存失败:\n${outcome.error}',
        type: MessageBoxType.error,
        okText: '知道了',
      );
      return;
    }
    final extraErrs = <String>[];
    for (final stmt in extras) {
      final err = await _runDdlQuiet(stmt);
      if (err.isNotEmpty) extraErrs.add(err);
      if (!mounted) return;
    }
    if (!mounted) return;
    setState(() {
      _saving = false;
      // 基线跟着前移:否则保存后 DDL 对比仍显示刚才那批变更,
      // 且再点一次保存会把同一批 ALTER 重复执行
      _origOwner = _owner;
      _origComment = _current.comment;
      _baseline = _current;
      _panelVisible = true;
      _panelTab = 0;
      _messages = '已保存视图「${widget.name}」'
          '${extraErrs.join()}';
    });
  }

  /// 保存后的附属语句(所有者 / 注释):失败不推翻主保存,仅回报原因
  Future<String> _runDdlQuiet(String sql) async {
    final app = _appState ?? context.read<AppState>();
    final conn = _conn;
    if (conn == null) return '';
    try {
      await app.connectionManager.runQuery(conn, sql,
          database: widget.database, limit: 1);
      return '';
    } catch (e) {
      return '\n$e';
    }
  }

  /// 预览:执行定义 SELECT,行数 / 错误写入「消息」页
  Future<void> _onPreview() async {
    final select = _current.select;
    if (select.isEmpty) {
      setState(() {
        _panelVisible = true;
        _panelTab = 0;
        _messages = '视图定义为空,无可预览的数据。';
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
      _messages = '正在预览 ...';
    });
    final sw = Stopwatch()..start();
    try {
      final r = await app.connectionManager.runQuery(conn, select,
          database: widget.database, schema: _isPg ? widget.schema : null, limit: 200);
      if (!mounted) return;
      setState(() {
        _busy = false;
        _messages = '已返回 ${r.rows.length} 行'
            '${r.truncated ? '(已达 200 行上限,仅预览前 200 行)' : ''}'
            ',耗时 ${sw.elapsedMilliseconds} ms';
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _messages = '预览失败:\n$e';
      });
    }
  }

  /// 解释:EXPLAIN 定义 SELECT,执行计划写入「解释」页
  Future<void> _onExplain() async {
    final select = _current.select;
    if (select.isEmpty) {
      setState(() {
        _panelVisible = true;
        _panelTab = 1;
        _explainText = '视图定义为空,无法解释。';
      });
      return;
    }
    final explained = switch (_typeId) {
      'mysql' || 'mariadb' || 'postgresql' => 'EXPLAIN $select',
      'sqlite' => 'EXPLAIN QUERY PLAN $select',
      _ => null,
    };
    if (explained == null) {
      setState(() {
        _panelVisible = true;
        _panelTab = 1;
        _explainText = '$_typeId 暂不支持解释执行计划';
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
      _explainText = '正在解释 ...';
    });
    try {
      final r = await app.connectionManager.runQuery(conn, explained,
          database: widget.database, limit: 500);
      if (!mounted) return;
      setState(() {
        _busy = false;
        _explainText = r.rows.map((row) => row.join('  ')).join('\n');
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _explainText = '解释失败:\n$e';
      });
    }
  }

  /// 美化 SQL:与查询页共用 formatSql
  void _onFormat() {
    if (_controller.text.trim().isEmpty) return;
    _controller.text = formatSql(_controller.text);
  }

  /// 添加规则:对话框只回语句,执行与刷新在这里(报错能带着上下文落到底部面板)
  Future<void> _onAddRule() async {
    final q =
        RoutineSql.qualifiedName(_typeId, widget.name, schema: widget.schema);
    final sql = await showDialog<String>(
      context: context,
      builder: (_) => ViewRuleDialog(target: q),
    );
    if (sql == null || !mounted) return;
    final app = _appState ?? context.read<AppState>();
    final conn = _conn;
    if (conn == null) {
      await _ruleFailed('连接「${widget.connection}」当前不可用,规则未创建。');
      return;
    }
    try {
      await app.connectionManager.runQuery(conn, sql,
          database: widget.database, limit: 1);
    } catch (e) {
      await _ruleFailed('创建失败:\n$e');
      return;
    }
    if (!mounted) return;
    await _loadRules();
  }

  Future<void> _ruleFailed(String message) => MessageBox.show(
        context,
        title: '添加规则',
        message: message,
        type: MessageBoxType.error,
        okText: '知道了',
      );

  Future<void> _onDeleteRule() async {
    final rule =
        (_selectedRule == null || _selectedRule! >= _rules.length)
            ? null
            : _rules[_selectedRule!];
    if (rule == null) {
      MessageBox.show(
        context,
        title: '删除规则',
        message: '请先在规则列表中选择要删除的规则。',
        type: MessageBoxType.warning,
        okText: '知道了',
      );
      return;
    }
    final result = await MessageBox.show(
      context,
      title: '删除规则',
      message: '确定删除视图「${widget.name}」的规则「${rule.name}」吗?\n'
          '将执行 DROP RULE,不可恢复。',
      type: MessageBoxType.question,
      buttons: MessageBoxButtons.okCancel,
      okText: '删除',
      cancelText: '取消',
    );
    if (result != MessageBoxResult.ok || !mounted) return;
    final app = _appState ?? context.read<AppState>();
    final conn = _conn;
    if (conn == null) return;
    final q =
        RoutineSql.qualifiedName(_typeId, widget.name, schema: widget.schema);
    try {
      await app.connectionManager.runQuery(
          conn, 'DROP RULE "${rule.name.replaceAll('"', '""')}" ON $q;',
          database: widget.database, limit: 1);
    } catch (e) {
      if (!mounted) return;
      MessageBox.show(
        context,
        title: '删除规则',
        message: '删除失败:\n$e',
        type: MessageBoxType.error,
        okText: '知道了',
      );
      return;
    }
    if (!mounted) return;
    await _loadRules();
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

  void _selectRule(int row) {
    setState(() {
      _selectedRule = row;
      final rule = _rules[row];
      _ruleLocation.text =
          '${widget.schema ?? 'public'}.${widget.name}';
      _ruleDefinition.text = rule.definition;
    });
  }

  // ── 构建 ────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    return ColoredBox(
      color: t.background,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _toolbar(t),
          Expanded(
            child: TabControl(
              contentPadding: EdgeInsets.zero,
              tabBarColor: t.secondary,
              selectedTabColor: t.surface,
              onChanged: (i) => setState(() => _tabIndex = i),
              tabs: [
                TabItem(label: '定义', child: _definitionTab(t)),
                // 规则是 PG 独有对象(pg_rewrite):其它方言不出现这一页,
                // 免得在一个永远没有内容的表格里空转
                if (_hasRules) TabItem(label: '规则', child: _rulesTab(t)),
                TabItem(label: '高级', child: _advancedTab(t)),
                TabItem(label: '注释', child: _commentTab(t)),
                TabItem(label: 'SQL 预览', child: _previewTab(t)),
              ],
            ),
          ),
        ],
      ),
    );
  }

  /// 规则页在主页签里的下标(非 PG 没有这一页,返回 -1)
  int get _rulesIdx => _hasRules ? 1 : -1;

  /// 工具栏:保存 | 预览 解释 视图创建工具 美化 SQL [规则页:添加/删除规则] … 全屏
  ///
  /// 纯图标 + tooltip,度量与「设计表」同一档(点击块 25 宽、栏高 29):
  /// 参考工具的图标栏上没有一个文字按钮。
  ///
  /// 底色取 secondary(栏底灰),与文档标签条里**选中标签**的底色一致 ——
  /// 本行紧贴标签条、选中的标签底边开放,同色才能连成一整块,不留接缝。
  Widget _toolbar(AppPalette t) {
    final blue = AppColors.of(context).iconPrimary;
    final onRules = _tabIndex == _rulesIdx;
    return Container(
      height: _kToolbarH,
      color: t.secondary,
      child: Row(
        children: [
          ToolbarButton(
            icon: Icons.save_outlined,
            iconColor: blue,
            tooltip: widget.isNew ? '保存并创建视图' : '保存变更',
            enabled: !_saving && !_busy,
            paddingX: _kIconPadX,
            onTap: _onSave,
          ),
          _toolSep(t),
          // 窄窗口下按钮可能多于可用宽:横向滚动而非 RenderFlex 溢出
          Expanded(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: Row(
                children: [
                  ToolbarButton(
                    icon: Icons.visibility_outlined,
                    iconColor: blue,
                    tooltip: '预览数据(执行定义查询)',
                    enabled: !_busy && !_saving,
                    paddingX: _kIconPadX,
                    onTap: _onPreview,
                  ),
                  ToolbarButton(
                    icon: Icons.account_tree_outlined,
                    iconColor: blue,
                    tooltip: '解释执行计划',
                    enabled: !_busy && !_saving,
                    paddingX: _kIconPadX,
                    onTap: _onExplain,
                  ),
                  ToolbarButton(
                    icon: Icons.construction_outlined,
                    iconColor: blue,
                    tooltip: '视图创建工具',
                    paddingX: _kIconPadX,
                    onTap: () => MessageBox.show(
                      context,
                      title: '视图创建工具',
                      message: '「视图创建工具」功能尚未开发,敬请期待。',
                      type: MessageBoxType.info,
                      okText: '知道了',
                    ),
                  ),
                  ToolbarButton(
                    icon: Icons.auto_fix_high_outlined,
                    iconColor: blue,
                    tooltip: '美化 SQL',
                    paddingX: _kIconPadX,
                    onTap: _onFormat,
                  ),
                  if (onRules) ...[
                    SizedBox(width: _kToolSepW),
                    ToolbarButton(
                      icon: Icons.add_circle_outline,
                      iconColor: _iconAdd,
                      // 新建模式下视图还不存在,CREATE RULE 必然报错,不给入口
                      tooltip: widget.isNew ? '保存视图后才能添加规则' : '添加规则',
                      enabled: !widget.isNew,
                      paddingX: _kIconPadX,
                      onTap: _onAddRule,
                    ),
                    ToolbarButton(
                      icon: Icons.remove_circle_outline,
                      iconColor: _iconDanger,
                      tooltip: '删除规则',
                      enabled: _selectedRule != null,
                      paddingX: _kIconPadX,
                      onTap: _onDeleteRule,
                    ),
                  ],
                ],
              ),
            ),
          ),
          IconBtn(
            icon: _fullscreen ? Icons.fullscreen_exit : Icons.fullscreen,
            iconSize: 16,
            tooltip: _fullscreen ? '退出全屏' : '全屏',
            onTap: _onFullscreen,
          ),
          const SizedBox(width: 4),
        ],
      ),
    );
  }

  /// 工具栏分隔块:1px 竖线 + 两侧留白(与「设计表」同档)
  Widget _toolSep(AppPalette t) => SizedBox(
        width: _kToolSepW,
        child: Center(
          child: Container(width: 1, height: 23, color: t.border),
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
            hint: null,
            findController: _findController,
          ),
        ),
        if (_panelVisible) _bottomPanel(t),
      ],
    );
  }

  /// 底部「消息 / 解释」面板(预览 / 解释 / 保存结果输出)
  Widget _bottomPanel(AppPalette t) {
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
            label: '消息',
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
            label: '解释',
            child: Padding(
              padding: const EdgeInsets.all(8),
              child: SingleChildScrollView(
                child: SelectableText(
                  _explainText,
                  style: TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 12.5,
                    height: 1.5,
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

  // ── 规则 ──

  Widget _rulesTab(AppPalette t) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Expanded(
          child: Padding(
            padding: const EdgeInsets.fromLTRB(8, 8, 8, 0),
            child: DataGridView(
              columns: const [
                DataGridViewColumn(title: '名称', width: 300, flex: 0),
                DataGridViewColumn(title: 'OID', width: 115, flex: 0),
                DataGridViewColumn(title: '事件', width: 115, flex: 0),
                DataGridViewColumn(title: '代替运行', width: 115, flex: 0),
                DataGridViewColumn(title: '注释'),
              ],
              rowCount: _rules.length,
              selectedRow: _selectedRule,
              onRowSelected: _selectRule,
              cellBuilder: (row, col) {
                final rule = _rules[row];
                final text = switch (col) {
                  0 => rule.name,
                  1 => rule.oid,
                  2 => rule.event,
                  3 => rule.instead,
                  _ => rule.comment,
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
        Container(
          padding: const EdgeInsets.fromLTRB(8, 10, 8, 12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _ruleField(t, '位置:', _ruleLocation, showPicker: false),
              const SizedBox(height: 8),
              _ruleField(t, '定义:', _ruleDefinition, showPicker: true),
            ],
          ),
        ),
      ],
    );
  }

  /// 规则页底部只读字段:输入 + 「...」按钮(与 Navicat 布局一致)
  Widget _ruleField(AppPalette t, String label, TextEditingController c,
      {required bool showPicker}) {
    return FieldRow(
      label: label,
      labelWidth: 120,
      child: Row(
        children: [
          SizedBox(
            width: 430,
            // 值随选中规则回填,编辑无意义 → 视觉保持可输入态但屏蔽输入
            child: IgnorePointer(child: Input(controller: c)),
          ),
          const SizedBox(width: 6),
          SizedBox(
            width: 30,
            child: Button(
              text: '...',
              onPressed: showPicker && _ruleDefinition.text.isNotEmpty
                  ? () => MessageBox.show(
                        context,
                        title: '规则定义',
                        message: _ruleDefinition.text,
                        type: MessageBoxType.info,
                        okText: '知道了',
                      )
                  : null,
            ),
          ),
        ],
      ),
    );
  }

  // ── 高级 ──

  /// 三项各自按方言门控:**生成侧不产出的选项就不给改**。
  /// 界面能改、DDL 不生成是最坏的一种「保存成功」—— 用户点了保存,库里一条没变。
  /// 禁用时挂 tooltip 说清是哪条子句把它挡在外。
  Widget _advancedTab(AppPalette t) {
    Widget field(String label, {required bool ok, String why = '', required Widget child}) {
      final w = FieldRow(label: label, labelWidth: 120, child: child);
      return ok ? w : WinToolTip(message: why, child: w);
    }

    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 18, 8, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          field(
            '所有者:',
            ok: _isPg,
            why: '仅 PostgreSQL 支持改视图所有者(ALTER VIEW … OWNER TO)',
            child: SizedBox(
              width: 240,
              child: ComboBox<String>(
                items: _owners,
                value: _owner,
                enabled: _isPg && _owners.isNotEmpty,
                itemToString: (v) => v,
                onChanged: (v) => setState(() => _owner = v),
              ),
            ),
          ),
          const SizedBox(height: 14),
          field(
            '检查选项:',
            ok: _checkOptionSupported,
            why: '仅 MySQL / MariaDB 支持 WITH [CASCADED|LOCAL] CHECK OPTION'
                '(PostgreSQL 没有该子句)',
            child: SizedBox(
              width: 240,
              child: ComboBox<String>(
                items: const ['LOCAL', 'CASCADED'],
                value: _checkOption.isEmpty ? null : _checkOption,
                enabled: _checkOptionSupported,
                itemToString: (v) => v,
                onChanged: (v) => setState(() => _checkOption = v ?? ''),
              ),
            ),
          ),
          const SizedBox(height: 14),
          field(
            '',
            ok: _isPg,
            why: '仅 PostgreSQL 支持 WITH (security_barrier)',
            child: CheckBox(
              value: _securityBarrier,
              enabled: _isPg,
              onChanged: (v) => setState(() => _securityBarrier = v ?? false),
              label: '安全屏障',
            ),
          ),
        ],
      ),
    );
  }

  // ── 注释 ──

  /// 只有 PG 有存放视图注释的地方(`COMMENT ON VIEW`);MySQL / SQLite /
  /// SQL Server 上这一页整块禁用 —— 能打字却存不下去的输入框比没有更糟。
  Widget _commentTab(AppPalette t) {
    return Padding(
      padding: const EdgeInsets.all(8),
      child: Textarea(
        controller: _commentController,
        expands: true,
        enabled: _isPg,
        showBorder: false,
        hint: _isPg ? '输入视图注释(保存时写入 COMMENT ON VIEW)' : '当前数据库类型不支持视图注释',
        style: TextStyle(
          fontSize: 13.5,
          height: 20 / 13.5,
          color: t.foreground,
        ),
      ),
    );
  }

  // ── SQL 预览 ──

  /// 「更改」= 保存要执行的完整语句清单(与 [_onSave] 同一份);
  /// 「DDL」= 对象定义的之前 / 之后逐行对比。
  Widget _previewTab(AppPalette t) {
    return ListenableBuilder(
      listenable: Listenable.merge([_controller, _commentController]),
      builder: (context, _) {
        final stmts = _changeStatements(_current);
        final change = stmts.join('\n\n');
        if (_changeController.text != change) _changeController.text = change;
        final before =
            widget.isNew || !_loaded ? '' : _fullDdl(_baseline);
        final after = _fullDdl(_current);
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TabStrip(
              items: const ['更改', 'DDL'],
              index: _previewSub,
              height: 28,
              background: t.secondary,
              onChanged: (i) => setState(() => _previewSub = i),
            ),
            Expanded(
              child: _previewSub == 0
                  ? Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        _previewHint(
                          t,
                          widget.isNew
                              ? '「保存」将按以下顺序执行这些语句'
                              : '「保存」将按以下顺序执行这些语句(先删后建,依赖该视图的对象会一并失效)',
                        ),
                        Expanded(
                          child: Stack(
                            children: [
                              Positioned.fill(
                                child: _sqlEditor(
                                  t,
                                  controller: _changeController,
                                  readOnly: true,
                                  showLineNumbers: false,
                                  hint: null,
                                  findController: _changeFindController,
                                ),
                              ),
                              Positioned(
                                top: 6,
                                right: 10,
                                child: IconBtn(
                                  icon: Icons.copy_outlined,
                                  iconSize: 15,
                                  tooltip: '复制',
                                  onTap: () => Clipboard.setData(
                                      ClipboardData(text: change)),
                                ),
                              ),
                            ],
                          ),
                        ),
                      ],
                    )
                  : Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        _previewHint(t, '左:库里当前的定义;右:保存后的定义。'),
                        Expanded(
                          child: TextDiffView(
                            before: before,
                            after: after,
                            beforeLabel: '之前:',
                            afterLabel: '之后:',
                            fontSize: 13,
                            lineHeight: 20,
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

  /// 预览页顶部的灰底说明条(与「设计表」的 SQL 预览同款式)
  Widget _previewHint(AppPalette t, String text) => Container(
        height: 26,
        padding: const EdgeInsets.symmetric(horizontal: 12),
        alignment: Alignment.centerLeft,
        color: t.secondary,
        child: Text(
          text,
          style: TextStyle(fontSize: 11.5, color: t.mutedForeground),
        ),
      );

  // ── 代码编辑器(定义 / 预览共用)──

  Widget _sqlEditor(
    AppPalette t, {
    required CodeLineEditingController controller,
    required bool readOnly,
    String? hint,
    bool showLineNumbers = true,
    CodeFindController? findController,
  }) {
    final isDark = t.background.computeLuminance() < 0.5;
    return CodeEditor(
      controller: controller,
      focusNode: readOnly ? null : _focusNode,
      readOnly: readOnly,
      hint: hint,
      chunkAnalyzer: const NonCodeChunkAnalyzer(),
      // 两者要成对:只给 findController 时 Ctrl+F 的默认动作没有落点(不渲染栏),
      // 只给 findBuilder 时栏会去用编辑器自建的控制器 —— 所以控制器由调用方按
      // 编辑区各传一个,别在这里共用。
      findBuilder: findController == null ? null : _buildFind,
      findController: findController,
      shortcutsActivatorsBuilder: const SqlFindShortcuts(),
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

  /// 查找栏(CodeEditor.findBuilder):未进入查找模式时零高度
  PreferredSizeWidget _buildFind(
      BuildContext context, CodeFindController controller, bool readOnly) {
    return SqlFindBar(
      controller: controller,
      readOnly: readOnly,
      // 只读区没把 `_focusNode` 交给编辑器(它不接焦点),关闭时无处可还
      onClosed: readOnly ? null : () => _focusNode.requestFocus(),
    );
  }
}

/// 从驱动读回的 CREATE VIEW 文本里拆出「定义页要显示的 SELECT」与
/// 属于「高级」页的三个选项。
///
/// 拆走的一律要记下来:旧实现只剥 `WITH … CHECK OPTION` 却不回填界面,
/// 于是打开一个带检查选项的视图 → 界面显示空 → 保存 → 选项静默消失。
///
/// 逐字符扫描(跳过引号 / 括号内的关键字)找顶层 `AS`:MySQL 的定义头部可含
/// ``DEFINER=`root`@`localhost` SQL SECURITY DEFINER`` 等正则难覆盖的形态。
({String select, String checkOption, bool securityBarrier})
    splitViewDefinition(String sql) {
  final s = sql.trim();
  final start =
      RegExp(r'^CREATE(\s+OR\s+REPLACE)?\s+', caseSensitive: false)
          .matchAsPrefix(s);
  final barrier = RegExp(r'WITH\s*\([^)]*security_barrier', caseSensitive: false)
      .hasMatch(s);
  if (start == null) {
    // 用户直接写 SELECT 存进来的(非 CREATE VIEW 开头):原样回显
    return (select: _stripTail(s), checkOption: '', securityBarrier: barrier);
  }
  final viewEnd = _scanKeyword(s, start.end, 'VIEW');
  final asEnd = viewEnd == null ? null : _scanKeyword(s, viewEnd, 'AS');
  if (asEnd == null) {
    return (select: _stripTail(s), checkOption: '', securityBarrier: barrier);
  }
  var body = s.substring(asEnd).trim();
  var checkOption = '';
  final check = RegExp(
          r'\bWITH\s+(CASCADED\s+|LOCAL\s+)?CHECK\s+OPTION\s*;?\s*$',
          caseSensitive: false)
      .firstMatch(body);
  if (check != null) {
    // 不写关键字时 MySQL 按 CASCADED 处理,回填成显式的才能原样往返
    final kw = check.group(1)?.trim().toUpperCase();
    checkOption = (kw == null || kw.isEmpty) ? 'CASCADED' : kw;
    body = body.substring(0, check.start).trimRight();
  }
  return (
    select: _stripTail(body),
    checkOption: checkOption,
    securityBarrier: barrier,
  );
}

/// 去掉语句尾的分号(与 [_ViewState] 里 SELECT 主体的形态一致)
String _stripTail(String sql) =>
    sql.endsWith(';') ? sql.substring(0, sql.length - 1).trimRight() : sql;

/// 从 [from] 起在引号 / 括号之外找到关键字 [word](完整单词,大小写不敏感),
/// 返回该单词结束后的下标;未找到返回 null
int? _scanKeyword(String s, int from, String word) {
  var depth = 0;
  String? quote;
  var i = from;
  while (i < s.length) {
    final ch = s[i];
    if (quote != null) {
      if (ch == quote) {
        // SQL 单引号翻倍转义不退出转义态
        if (quote == "'" && i + 1 < s.length && s[i + 1] == "'") {
          i += 2;
          continue;
        }
        quote = null;
      }
    } else if (ch == '"' || ch == "'" || ch == '`') {
      quote = ch;
    } else if (ch == '(' || ch == '[') {
      depth++;
    } else if (ch == ')' || ch == ']') {
      depth--;
    } else if (depth == 0) {
      final hit = _wordAt(s, i, word);
      if (hit != null) return hit;
    }
    i++;
  }
  return null;
}

/// 若 [s] 在 [i] 处正好是完整单词 [word](大小写不敏感,前后非字母数字下划线),
/// 返回单词结束后的下标,否则返回 null
int? _wordAt(String s, int i, String word) {
  final end = i + word.length;
  if (end > s.length) return null;
  if (s.substring(i, end).toUpperCase() != word) return null;
  if (i > 0 && _isIdentChar(s.codeUnitAt(i - 1))) return null;
  if (end < s.length && _isIdentChar(s.codeUnitAt(end))) return null;
  return end;
}

bool _isIdentChar(int r) =>
    (r >= 0x61 && r <= 0x7a) || // a-z
    (r >= 0x41 && r <= 0x5a) || // A-Z
    (r >= 0x30 && r <= 0x39) || // 0-9
    r == 0x5f; // _

