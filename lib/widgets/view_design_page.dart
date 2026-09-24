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
import 'query_page.dart' show formatSql;
import 'routine_design_page.dart' show sqlLightTheme, sqlDarkTheme;

/// 视图设计器(按 Navicat 视图设计器 1:1 还原)。
///
/// 工具栏:保存 / 预览 / 解释 / 视图创建工具 / 美化 SQL;
/// 「规则」页签追加 添加规则 / 删除规则;右缘全屏按钮收起 / 恢复两侧面板。
/// 页签:定义(仅 SELECT 查询)/ 规则(PG pg_rewrite 视图规则)/
/// 高级(所有者 / 检查选项 / 安全屏障)/ 注释 / SQL 预览(更改 / DDL 子页)。
/// 「预览 / 解释」结果输出到定义页底部「消息 / 解释」面板。
///
/// 定义模型与 Navicat 一致:编辑区只存 SELECT,保存时由
/// CREATE VIEW + 高级选项拼装完整 DDL(见 [_createViewSql])。
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
  });

  final String name;
  final String connection;
  final String database;
  final String? schema;
  final ObjectCategory category;
  final bool isNew;
  final String comment;

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

class _ViewDesignPageState extends State<ViewDesignPage> {
  final CodeLineEditingController _controller =
      CodeLineEditingController.fromText('');
  final TextEditingController _commentController = TextEditingController();
  final CodeLineEditingController _changeController =
      CodeLineEditingController.fromText('');
  final CodeLineEditingController _ddlController =
      CodeLineEditingController.fromText('');

  /// 主页签下标(0 定义 / 1 规则 / 2 高级 / 3 注释 / 4 SQL 预览)
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

  String get _typeId => _conn?.typeId ?? '';
  bool get _isPg => _typeId == 'postgresql';

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
    if (!widget.isNew && saved.isEmpty) {
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
    _controller.dispose();
    _commentController.dispose();
    _changeController.dispose();
    _ddlController.dispose();
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
      setState(() {
        if (def != null && def.trim().isNotEmpty) {
          _securityBarrier =
              RegExp(r'WITH\s*\([^)]*security_barrier', caseSensitive: false)
                  .hasMatch(def);
          _controller.text = _extractSelect(def);
        }
      });
      if (_isPg) {
        await _loadOwner();
        await _loadRules();
        await _loadOwners();
      }
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

  Future<void> _loadOwner() async {
    final owner = await _scalar(
        "SELECT pg_get_userbyid(c.relowner) FROM pg_class c "
        "JOIN pg_namespace n ON n.oid = c.relnamespace "
        "WHERE n.nspname = ${_lit(widget.schema ?? 'public')} "
        "AND c.relname = ${_lit(widget.name)}");
    if (!mounted) return;
    setState(() => _owner = _origOwner = owner);
  }

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

  Future<String?> _scalar(String sql) async {
    final rows = await _rows(sql);
    if (rows.isEmpty || rows.first.isEmpty) return null;
    return rows.first.first;
  }

  static String _lit(String v) => "'${v.replaceAll("'", "''")}'";

  /// 从 CREATE VIEW 语句中取出顶层 AS 之后的 SELECT 主体。
  /// 非 CREATE VIEW 开头(用户直接写 SELECT)时原样返回。
  /// 逐字符扫描(跳过引号 / 括号内的关键字)——MySQL 定义头部可含
  /// ``DEFINER=`root`@`localhost` SQL SECURITY DEFINER`` 等正则难覆盖的形态。
  /// 尾部 WITH [CASCADED|LOCAL] CHECK OPTION 与分号属于视图选项,归「高级」页。
  static String _extractSelect(String sql) {
    final s = sql.trim();
    final start = RegExp(r'^CREATE(\s+OR\s+REPLACE)?\s+', caseSensitive: false)
        .matchAsPrefix(s);
    if (start == null) return s;
    final viewEnd = _scanKeyword(s, start.end, 'VIEW');
    if (viewEnd == null) return s;
    final asEnd = _scanKeyword(s, viewEnd, 'AS');
    if (asEnd == null) return s;
    var body = s.substring(asEnd).trim();
    final check = RegExp(
            r'\bWITH\s+(CASCADED\s+|LOCAL\s+)?CHECK\s+OPTION\s*;?\s*$',
            caseSensitive: false)
        .firstMatch(body);
    if (check != null) {
      body = body.substring(0, check.start).trimRight();
    }
    if (body.endsWith(';')) body = body.substring(0, body.length - 1);
    return body.trim();
  }

  /// 从 [from] 起在引号 / 括号之外找到关键字 [word](完整单词,大小写不敏感),
  /// 返回该单词结束后的下标;未找到返回 null
  static int? _scanKeyword(String s, int from, String word) {
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
  static int? _wordAt(String s, int i, String word) {
    final end = i + word.length;
    if (end > s.length) return null;
    if (s.substring(i, end).toUpperCase() != word) return null;
    if (i > 0 && _isIdentChar(s.codeUnitAt(i - 1))) return null;
    if (end < s.length && _isIdentChar(s.codeUnitAt(end))) return null;
    return end;
  }

  static bool _isIdentChar(int r) =>
      (r >= 0x61 && r <= 0x7a) || // a-z
      (r >= 0x41 && r <= 0x5a) || // A-Z
      (r >= 0x30 && r <= 0x39) || // 0-9
      r == 0x5f; // _

  // ── SQL 拼装 ────────────────────────────────────────────

  /// CREATE VIEW 语句;[withOptions] 时并入高级页的
  /// 安全屏障(PG WITH 选项)与检查选项(WITH CHECK OPTION)
  String _createViewSql({bool withOptions = true}) {
    final q =
        RoutineSql.qualifiedName(_typeId, widget.name, schema: widget.schema);
    var select = _controller.text.trim();
    while (select.endsWith(';')) {
      select = select.substring(0, select.length - 1).trimRight();
    }
    final buf = StringBuffer('CREATE VIEW $q');
    if (withOptions && _isPg && _securityBarrier) {
      buf.write(' WITH (security_barrier)');
    }
    buf.write(' AS $select');
    if (withOptions && _checkOption.isNotEmpty) {
      buf.write(' WITH $_checkOption CHECK OPTION');
    }
    buf.write(';');
    return buf.toString();
  }

  String _ddlSql() {
    final buf = StringBuffer(_createViewSql());
    final q =
        RoutineSql.qualifiedName(_typeId, widget.name, schema: widget.schema);
    if (_isPg && _owner != null && _owner!.isNotEmpty && _owner != _origOwner) {
      buf.writeln();
      buf.writeln('ALTER VIEW $q OWNER TO "${_owner!}"');
    }
    final comment = _commentController.text.trim();
    if (_isPg && comment.isNotEmpty) {
      buf.writeln();
      buf.writeln(
          "COMMENT ON VIEW $q IS '${comment.replaceAll("'", "''")}';");
    }
    return buf.toString().trimRight();
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
    final sql = _createViewSql();
    final outcome = widget.isNew
        ? await app.runDdl(sql,
            connection: widget.connection,
            database: widget.database,
            schema: widget.schema)
        : await app.replaceRoutine(
            widget.category,
            widget.name,
            createSql: sql,
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
    // 高级页的归属变更与注释:PG 以独立语句补执行
    final extraErrs = <String>[];
    final extras = <String>[];
    if (_isPg && _owner != null && _owner!.isNotEmpty && _owner != _origOwner) {
      final q = RoutineSql.qualifiedName(
          _typeId, widget.name, schema: widget.schema);
      extras.add(
          'ALTER VIEW $q OWNER TO "${_owner!.replaceAll('"', '""')}";');
    }
    final comment = _commentController.text.trim();
    if (_isPg && comment.isNotEmpty) {
      final q = RoutineSql.qualifiedName(
          _typeId, widget.name, schema: widget.schema);
      extras.add(
          "COMMENT ON VIEW $q IS '${comment.replaceAll("'", "''")}';");
    }
    for (final stmt in extras) {
      final err = await _runDdlQuiet(stmt);
      if (err.isNotEmpty) extraErrs.add(err);
      if (!mounted) return;
    }
    if (!mounted) return;
    setState(() {
      _saving = false;
      _origOwner = _owner;
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
    final select = _previewSelect();
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
    final select = _previewSelect();
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

  /// 预览 / 解释用主体:去掉尾部分号(驱动按单条语句执行)
  String _previewSelect() {
    var s = _controller.text.trim();
    while (s.endsWith(';')) {
      s = s.substring(0, s.length - 1).trimRight();
    }
    return s;
  }

  /// 美化 SQL:与查询页共用 formatSql
  void _onFormat() {
    if (_controller.text.trim().isEmpty) return;
    _controller.text = formatSql(_controller.text);
  }

  void _onAddRule() {
    MessageBox.show(
      context,
      title: '添加规则',
      message: '「添加规则」功能尚未开发,敬请期待。',
      type: MessageBoxType.info,
      okText: '知道了',
    );
  }

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
                TabItem(label: '规则', child: _rulesTab(t)),
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

  /// 工具栏:保存 | 预览 解释 视图创建工具 美化 SQL [规则页:添加/删除规则] … 全屏
  ///
  /// 底色取 secondary(栏底灰),与文档标签条里**选中标签**的底色一致 ——
  /// 本行紧贴标签条、选中的标签底边开放,同色才能连成一整块,不留接缝。
  Widget _toolbar(AppPalette t) {
    const blue = Color(0xff1f6feb);
    return Container(
      height: 34,
      color: t.secondary,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        children: [
          ToolbarButton(
            icon: Icons.save_outlined,
            iconColor: blue,
            text: '保存',
            enabled: !_saving && !_busy,
            onTap: _onSave,
          ),
          _divider(t),
          ToolbarButton(
            icon: Icons.visibility_outlined,
            iconColor: blue,
            text: '预览',
            enabled: !_busy && !_saving,
            onTap: _onPreview,
          ),
          ToolbarButton(
            icon: Icons.account_tree_outlined,
            iconColor: blue,
            text: '解释',
            enabled: !_busy && !_saving,
            onTap: _onExplain,
          ),
          ToolbarButton(
            icon: Icons.construction_outlined,
            iconColor: blue,
            text: '视图创建工具',
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
            text: '美化 SQL',
            onTap: _onFormat,
          ),
          if (_tabIndex == 1) ...[
            const SizedBox(width: 18),
            ToolbarButton(
              icon: Icons.add_circle_outline,
              iconColor: const Color(0xff188038),
              text: '添加规则',
              onTap: _onAddRule,
            ),
            ToolbarButton(
              icon: Icons.remove_circle_outline,
              iconColor: const Color(0xffd93025),
              text: '删除规则',
              enabled: _selectedRule != null,
              onTap: _onDeleteRule,
            ),
          ],
          const Spacer(),
          IconBtn(
            icon: _fullscreen ? Icons.fullscreen_exit : Icons.fullscreen,
            iconSize: 16,
            tooltip: _fullscreen ? '退出全屏' : '全屏',
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
            hint: null,
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

  Widget _advancedTab(AppPalette t) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 18, 8, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          FieldRow(
            label: '所有者:',
            labelWidth: 120,
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
          FieldRow(
            label: '检查选项:',
            labelWidth: 120,
            child: SizedBox(
              width: 240,
              child: ComboBox<String>(
                items: const ['LOCAL', 'CASCADED'],
                value: _checkOption.isEmpty ? null : _checkOption,
                itemToString: (v) => v,
                onChanged: (v) =>
                    setState(() => _checkOption = v ?? ''),
              ),
            ),
          ),
          const SizedBox(height: 14),
          FieldRow(
            label: '',
            child: CheckBox(
              value: _securityBarrier,
              onChanged: (v) =>
                  setState(() => _securityBarrier = v ?? false),
              label: '安全屏障',
            ),
          ),
        ],
      ),
    );
  }

  // ── 注释 ──

  Widget _commentTab(AppPalette t) {
    return Padding(
      padding: const EdgeInsets.all(8),
      child: Textarea(
        controller: _commentController,
        expands: true,
        showBorder: false,
        hint: '输入视图注释${_isPg ? '(保存时写入 COMMENT ON)' : ''}',
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
    return ListenableBuilder(
      listenable: Listenable.merge([_controller, _commentController]),
      builder: (context, _) {
        final change = _createViewSql(withOptions: false);
        final ddl = _ddlSql();
        if (_changeController.text != change) _changeController.text = change;
        if (_ddlController.text != ddl) _ddlController.text = ddl;
        final text = _previewSub == 0 ? change : ddl;
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
              child: Stack(
                children: [
                  Positioned.fill(
                    child: _sqlEditor(
                      t,
                      controller: _previewSub == 0
                          ? _changeController
                          : _ddlController,
                      readOnly: true,
                      showLineNumbers: false,
                      hint: null,
                    ),
                  ),
                  Positioned(
                    top: 6,
                    right: 10,
                    child: IconBtn(
                      icon: Icons.copy_outlined,
                      iconSize: 15,
                      tooltip: '复制',
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
    String? hint,
    bool showLineNumbers = true,
  }) {
    final isDark = t.background.computeLuminance() < 0.5;
    return CodeEditor(
      controller: controller,
      readOnly: readOnly,
      hint: hint,
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
          theme: {
            ...(isDark ? sqlDarkTheme : sqlLightTheme),
            'root': TextStyle(color: t.foreground),
          },
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
