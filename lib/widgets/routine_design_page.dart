import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:re_editor/re_editor.dart';
import 'package:re_highlight/languages/sql.dart';
import '../app/app_state.dart';
import '../data/routine_sql.dart';
import '../theme/app_theme.dart';

/// 例程设计页:展示并编辑过程 / 函数 / 视图的 CREATE 定义。
/// - 编辑模式(打开 / 设计):加载现有定义,保存时 DROP + CREATE 重写;
/// - 新建模式:按向导采集的名称 / 参数生成模板,保存时直接 CREATE。
///
/// 布局:
/// - 工具栏:保存 / 运行 / 停止 / 调试 / 查找 / 自动换行;
/// - 标签页:定义(SQL 编辑器)/ 高级(返回类型 · 语言 · 安全定义者)/
///   注释 / SQL 预览(只读,含注释)。
/// 通过真实驱动获取 / 执行定义(SQL Server / MySQL / PostgreSQL / SQLite 等)。
class RoutineDesignPage extends StatefulWidget {
  const RoutineDesignPage({
    super.key,
    required this.name,
    required this.connection,
    required this.database,
    required this.category,
    this.schema,
    this.isNew = false,
    this.params = '',
    this.comment = '',
  });

  /// 对象名(过程 / 函数 / 视图名)
  final String name;

  /// 所属连接名
  final String connection;

  /// 所属数据库
  final String database;

  /// 所属模式(PostgreSQL / SQL Server 等;无模式层为 null)
  final String? schema;

  /// 对象分类(过程 / 函数 / 视图)
  final ObjectCategory category;

  /// 是否为新建(新建=向导采集的参数生成模板,无需加载定义)
  final bool isNew;

  /// 新建模式下的初始参数签名(向导第 2 步采集,如 "IN a INT, IN b VARCHAR(50)")
  final String params;

  /// 新建模式下的初始注释(向导 / 历史编辑)
  final String comment;

  @override
  State<RoutineDesignPage> createState() => _RoutineDesignPageState();
}

class _RoutineDesignPageState extends State<RoutineDesignPage> {
  final CodeLineEditingController _controller =
      CodeLineEditingController.fromText('');
  final FocusNode _focusNode = FocusNode();
  final TextEditingController _commentController = TextEditingController();
  final FocusNode _commentFocusNode = FocusNode();
  late final CodeFindController _findController = CodeFindController(_controller);

  /// 自动换行开关(工具栏高亮跟随)
  bool _wordWrap = false;

  /// 保存 / 执行进行中
  bool _saving = false;
  bool _running = false;

  /// 执行代次:停止 / 切换标签时作废进行中的请求
  int _runGeneration = 0;

  String? _error;
  String? _message;

  /// 预览标签的只读 controller(文本在 builder 中同步)
  final CodeLineEditingController _previewController =
      CodeLineEditingController.fromText('');

  /// 高级标签字段(解析自当前 SQL;无法解析时保持默认并禁用「应用」)
  String _returnType = '';
  String _language = '';
  bool _securityDefiner = false;

  /// 最近一次成功解析出的参数签名(用于 COMMENT ON / SQL 预览)
  String _parsedSignature = '';

  /// 当前 SQL 是否可被解析(决定高级标签「应用」可用性)
  bool _parsedOk = false;

  /// didChangeDependencies 中缓存的 AppState(dispose 时安全引用)
  AppState? _appState;

  /// 本 tab 的持久化 key(与 OpenTab.key 同格式,见 main_page 路由)
  String get _tabKey =>
      'design|${widget.connection}|${widget.database}|${widget.schema}|'
      '${widget.isNew ? '${widget.name} (新建)' : '${widget.name} (设计)'}'
      '|${widget.category.name}';

  ObjectCategory get _category => widget.category;

  String get _label => switch (_category) {
        ObjectCategory.view => '视图',
        ObjectCategory.function => '函数',
        ObjectCategory.procedure => '过程',
        _ => '例程',
      };

  @override
  void initState() {
    super.initState();
    _appState = context.read<AppState>();
    _commentController.text = widget.comment.isNotEmpty
        ? widget.comment
        : _appState!.routineCommentFor(_tabKey);
    final saved = _appState!.routineTextFor(_tabKey);
    if (widget.isNew) {
      // 新建:优先恢复未保存的编辑,否则生成模板
      _controller.text = saved.isNotEmpty ? saved : _template();
    } else {
      _controller.text = saved.isNotEmpty ? saved : '';
      if (saved.isEmpty) {
        // 首帧后加载定义(initState 期间不可 setState)
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) _load();
        });
      }
    }
    _controller.addListener(_onEditorChanged);
    _commentController.addListener(_onCommentChanged);
    _reparseAdvanced();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _appState = context.read<AppState>();
  }

  @override
  void didUpdateWidget(RoutineDesignPage oldWidget) {
    super.didUpdateWidget(oldWidget);
    final changed = oldWidget.name != widget.name ||
        oldWidget.connection != widget.connection ||
        oldWidget.database != widget.database ||
        oldWidget.schema != widget.schema ||
        oldWidget.category != widget.category ||
        oldWidget.isNew != widget.isNew;
    if (!changed) return;
    // 同类设计 tab 间切换:先保存旧 tab 内容,再载入新 tab
    _appState?.updateRoutineText(_tabKeyOf(oldWidget), _controller.text);
    _appState?.updateRoutineComment(
        _tabKeyOf(oldWidget), _commentController.text);
    final saved = _appState?.routineTextFor(_tabKey) ?? '';
    _controller.text = saved.isNotEmpty
        ? saved
        : (widget.isNew ? _template() : '');
    _commentController.text =
        _appState?.routineCommentFor(_tabKey) ?? widget.comment;
    _runGeneration++;
    setState(() {
      _running = false;
      _error = null;
      _message = null;
    });
    if (!widget.isNew && saved.isEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _load();
      });
    }
    _reparseAdvanced();
  }

  @override
  void dispose() {
    // 先解绑监听,再持久化,最后释放 controller
    _controller.removeListener(_onEditorChanged);
    _commentController.removeListener(_onCommentChanged);
    _appState?.updateRoutineText(_tabKey, _controller.text);
    _appState?.updateRoutineComment(_tabKey, _commentController.text);
    _commentController.dispose();
    _commentFocusNode.dispose();
    _previewController.dispose();
    _findController.dispose();
    _controller.dispose();
    _focusNode.dispose();
    super.dispose();
  }

  /// 同类型 tab 切换时,旧 widget 的持久化 key
  static String _tabKeyOf(RoutineDesignPage w) =>
      'design|${w.connection}|${w.database}|${w.schema}|'
      '${w.isNew ? '${w.name} (新建)' : '${w.name} (设计)'}'
      '|${w.category.name}';

  // ── 模板与解析 ──────────────────────────────────────────

  String get _typeId => _appState?.connectionByName(widget.connection)?.typeId ?? '';

  /// 新建模式下的 CREATE 模板(按数据库类型 / 分类生成)
  String _template() {
    if (_category == ObjectCategory.view) {
      return 'CREATE VIEW ${RoutineSql.qualifiedName(_typeId, widget.name, schema: widget.schema)} AS\n'
          'SELECT *\n'
          'FROM ${_placeholderTable()}\n'
          'WHERE 1 = 1;';
    }
    return RoutineSql.buildTemplate(
      typeId: _typeId,
      category: _category,
      name: widget.name,
      schema: widget.schema,
      params: _paramsFromSignature(widget.params),
    );
  }

  /// 把签名文本解析回参数列表(模板生成用;签名来自向导)。
  /// 简单按 ", " 分段,每段切 模式 / 名称 / 类型 三段
  List<RoutineParam> _paramsFromSignature(String signature) {
    if (signature.trim().isEmpty) return const [];
    final params = <RoutineParam>[];
    for (final seg in signature.split(RegExp(r',\s*'))) {
      final tokens = seg.trim().split(RegExp(r'\s+'));
      if (tokens.isEmpty || tokens.first.isEmpty) continue;
      if (tokens.length == 1) {
        params.add(RoutineParam(name: '', type: tokens[0]));
      } else if (tokens.length == 2) {
        // SQL Server 风格: @a INT 或 @a INT OUTPUT
        if (tokens.last.toUpperCase() == 'OUTPUT') {
          params.add(
              RoutineParam(mode: 'OUTPUT', name: tokens[0], type: tokens[1]));
        } else {
          params.add(RoutineParam(mode: '', name: tokens[0], type: tokens[1]));
        }
      } else {
        params.add(RoutineParam(
            mode: tokens[0].toUpperCase(),
            name: tokens[1],
            type: tokens.sublist(2).join(' ')));
      }
    }
    return params;
  }

  String _placeholderTable() {
    switch (_typeId) {
      case 'postgresql':
      case 'sqlite':
        return '"your_table"';
      case 'sqlserver':
      case 'access':
        return '[your_table]';
      default:
        return '`your_table`';
    }
  }

  /// 从当前 SQL 解析高级字段(返回类型 / 语言 / 安全定义者 / 参数签名)
  void _reparseAdvanced() {
    final parsed = parseRoutineSql(
      _controller.text,
      typeId: _typeId,
      category: _category,
    );
    if (parsed == null) {
      _parsedOk = false;
      _parsedSignature = '';
      return;
    }
    _parsedOk = true;
    _parsedSignature = parsed.signature;
    _returnType = parsed.returnType;
    _language = parsed.language;
    _securityDefiner = parsed.securityDefiner;
  }

  /// 高级标签「应用」:用表单字段 + 当前函数体重建完整 SQL
  void _applyAdvanced() {
    final parsed = parseRoutineSql(
      _controller.text,
      typeId: _typeId,
      category: _category,
    );
    if (parsed == null) {
      setState(() {
        _message = null;
        _error = 'SQL 与模板结构不一致,无法自动应用高级选项,请直接编辑 SQL';
      });
      return;
    }
    _controller.text = RoutineSql.buildWithBody(
      typeId: _typeId,
      category: _category,
      name: widget.name,
      schema: widget.schema,
      signature: parsed.signature,
      body: parsed.body,
      returnType: _returnType,
      language: _language,
      securityDefiner: _securityDefiner,
    );
    _reparseAdvanced();
    setState(() {
      _error = null;
      _message = '已应用高级选项';
    });
  }

  // ── 加载与保存 ──────────────────────────────────────────

  Future<void> _load() async {
    final app = _appState ?? context.read<AppState>();
    final conn = app.connectionByName(widget.connection);
    if (conn == null) {
      setState(() => _error = '连接 "${widget.connection}" 不存在');
      return;
    }
    try {
      final def = await app.getObjectDefinition(
        _category,
        widget.name,
        connection: widget.connection,
        database: widget.database,
        schema: widget.schema,
      );
      if (!mounted) return;
      setState(() {
        _error = null;
        if (def == null || def.trim().isEmpty) {
          // 驱动不支持获取定义:退化为模板,用户可直接编辑后保存重建
          _controller.text = _template();
          _message = '未能读取 $_label 定义,已载入模板,编辑后保存将重建该对象';
        } else {
          _controller.text = def;
          _message = null;
        }
        _reparseAdvanced();
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = e.toString());
    }
  }

  /// 保存:新建=直接 CREATE;编辑=DROP + CREATE 重写;
  /// 注释(PG)作为 COMMENT ON 语句追加执行
  Future<void> _onSave() async {
    final sql = _controller.text.trim();
    if (sql.isEmpty) {
      setState(() => _message = '请输入 $_label 定义 SQL');
      return;
    }
    final app = _appState ?? context.read<AppState>();
    setState(() {
      _saving = true;
      _error = null;
      _message = null;
    });
    final outcome = widget.isNew
        ? await app.runDdl(
            sql,
            connection: widget.connection,
            database: widget.database,
            schema: widget.schema,
          )
        : await app.replaceRoutine(
            _category,
            widget.name,
            createSql: sql,
            connection: widget.connection,
            database: widget.database,
            schema: widget.schema,
          );
    if (!mounted) return;
    if (!outcome.ok) {
      setState(() {
        _saving = false;
        _error = outcome.error ?? '保存失败';
      });
      return;
    }
    // 保存成功:处理注释(仅 PG 有 COMMENT ON 语法)
    String? commentFail;
    final comment = _commentController.text.trim();
    if (comment.isNotEmpty && RoutineSql.supportsCommentOn(_typeId)) {
      final commentSql = RoutineSql.commentOnSql(
        typeId: _typeId,
        category: _category,
        name: widget.name,
        schema: widget.schema,
        signature: _parsedSignature,
        comment: comment,
      );
      if (commentSql.isNotEmpty) {
        try {
          final conn = app.connectionByName(widget.connection);
          if (conn != null) {
            await app.connectionManager.runQuery(
                conn, commentSql,
                database: widget.database, limit: 1);
          }
        } catch (e) {
          commentFail = e.toString();
        }
      }
    }
    if (!mounted) return;
    setState(() {
      _saving = false;
      _message = commentFail == null
          ? '已保存$_label ${widget.name}'
          : '已保存$_label ${widget.name}(注释写入失败:$commentFail)';
    });
  }

  /// 运行:把编辑区 SQL 作为单条语句执行(结果 / 错误显示在状态栏)
  Future<void> _onRun() async {
    final sql = _controller.text.trim();
    if (sql.isEmpty) {
      setState(() => _message = '请输入 $_label 定义 SQL');
      return;
    }
    final app = _appState ?? context.read<AppState>();
    final conn = app.connectionByName(widget.connection);
    if (conn == null) {
      setState(() => _error = '连接 "${widget.connection}" 不存在');
      return;
    }
    _runGeneration++;
    final gen = _runGeneration;
    setState(() {
      _running = true;
      _error = null;
      _message = null;
    });
    final sw = Stopwatch()..start();
    try {
      final result = await app.connectionManager.runQuery(
        conn,
        sql,
        database: widget.database,
        limit: 100,
      );
      if (!mounted || gen != _runGeneration) return;
      setState(() {
        _running = false;
        _message = result.isSelect
            ? '已返回 ${result.rows.length} 行(耗时 ${sw.elapsedMilliseconds} ms)'
            : result.affectedRows > 0
                ? '已执行,受影响 ${result.affectedRows} 行(耗时 ${sw.elapsedMilliseconds} ms)'
                : '已执行(耗时 ${sw.elapsedMilliseconds} ms)';
      });
    } catch (e) {
      if (!mounted || gen != _runGeneration) return;
      setState(() {
        _running = false;
        _error = e.toString();
      });
    }
  }

  /// 停止:作废等待中的执行(UI 即时恢复;服务端语句可能仍在执行)
  void _onStop() {
    if (!_running) return;
    _runGeneration++;
    setState(() {
      _running = false;
      _message = '已停止等待结果(服务端可能仍在执行该语句)';
    });
  }

  void _onFind() {
    _findController.findMode();
    _findController.focusOnFindInput();
  }

  void _onDebug() {
    MessageBox.show(
      context,
      title: '调试',
      message: '「调试」功能尚未开发,敬请期待。',
      type: MessageBoxType.info,
    );
  }

  // ── 输入持久化 ──────────────────────────────────────────

  void _onEditorChanged() {
    _appState?.updateRoutineText(_tabKey, _controller.text);
  }

  void _onCommentChanged() {
    _appState?.updateRoutineComment(_tabKey, _commentController.text);
    // 预览区由 ListenableBuilder 订阅重建,无需在此 setState
  }

  // ── 构建 ────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    return Container(
      color: t.background,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _toolbar(t),
          _titleBar(t),
          Expanded(child: _tabs(t)),
          _statusBar(t),
        ],
      ),
    );
  }

  /// 工具栏:保存 / 运行 / 停止 / 调试 / 查找 / 自动换行
  ///
  /// 底色取 secondary(栏底灰),与文档标签条里**选中标签**的底色一致 ——
  /// 本行紧贴标签条、选中的标签底边开放,同色才能连成一整块,不留接缝。
  Widget _toolbar(AppPalette t) {
    final c = AppColors.of(context);
    return Container(
      height: 34,
      color: t.secondary,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        children: [
          ToolbarButton(
            icon: Icons.save_outlined,
            text: '保存',
            enabled: !_saving && !_running,
            onTap: _onSave,
          ),
          _divider(t),
          ToolbarButton(
            icon: Icons.play_arrow,
            iconColor: const Color(0xff1f6feb),
            text: '运行',
            enabled: !_running && !_saving,
            onTap: _onRun,
          ),
          ToolbarButton(
            icon: Icons.stop,
            iconColor: const Color(0xffd93025),
            text: '停止',
            enabled: _running,
            onTap: _onStop,
          ),
          ToolbarButton(
            icon: Icons.bug_report_outlined,
            text: '调试',
            onTap: _onDebug,
          ),
          ToolbarButton(
            icon: Icons.manage_search_outlined,
            text: '查找',
            onTap: _onFind,
          ),
          const Spacer(),
          ToolbarButton(
            icon: Icons.wrap_text,
            iconColor: _wordWrap ? c.iconInfo : null,
            text: '自动换行',
            backgroundColor: _wordWrap
                ? Color.alphaBlend(
                    t.accent.withValues(alpha: 0.18), t.control)
                : null,
            onTap: () => setState(() => _wordWrap = !_wordWrap),
          ),
        ],
      ),
    );
  }

  /// 标题行:设计/新建过程/函数/视图: name @ 连接.数据库
  Widget _titleBar(AppPalette t) => Container(
        height: 28,
        alignment: Alignment.centerLeft,
        padding: const EdgeInsets.only(left: 12),
        color: t.secondary,
        child: Text(
          '${widget.isNew ? '新建' : '设计'}$_label: ${widget.name} @ '
          '${widget.connection}.${widget.database}',
          style: TextStyle(
            fontSize: 12.5,
            color: t.mutedForeground,
            decoration: TextDecoration.none,
            fontWeight: FontWeight.w400,
          ),
        ),
      );

  /// 标签页:定义 / 高级 / 注释 / SQL 预览
  Widget _tabs(AppPalette t) {
    return TabControl(
      contentPadding: EdgeInsets.zero,
      tabBarColor: t.secondary,
      selectedTabColor: t.surface,
      tabs: [
        TabItem(label: '定义', child: _definitionTab(t)),
        TabItem(label: '高级', child: _advancedTab(t)),
        TabItem(label: '注释', child: _commentTab(t)),
        TabItem(label: 'SQL 预览', child: _previewTab(t)),
      ],
    );
  }

  /// 定义标签:re_editor 代码编辑器(行号 + SQL 高亮 + 查找 + 换行)
  Widget _definitionTab(AppPalette t) {
    if (_controller.text.isEmpty && !widget.isNew && _error != null) {
      return Empty(
        icon: const Icon(Icons.error_outline),
        title: '读取 ${widget.name} 定义失败',
        description: _error,
        action: Button(text: '重试', onPressed: _load),
        compact: true,
        maxWidth: 520,
      );
    }
    return _sqlEditor(t, controller: _controller, findBuilder: _buildFind);
  }

  /// 高级标签:返回类型(函数)/ 语言(PG)/ 安全定义者(PG)+ 应用
  Widget _advancedTab(AppPalette t) {
    final isFunction = _category == ObjectCategory.function;
    final isPg = _typeId == 'postgresql';
    return SingleChildScrollView(
      padding: const EdgeInsets.all(20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '高级选项会改写 CREATE 语句头部;函数体保留在「定义」标签中编辑。',
            style: TextStyle(fontSize: 12, color: t.mutedForeground),
          ),
          const SizedBox(height: 16),
          if (isFunction) ...[
            FieldRow(
              label: '返回类型:',
              child: SizedBox(
                width: 260,
                child: Input(
                  controller: TextEditingController(text: _returnType),
                  hint: '如 integer / INT / varchar(100)',
                  onChanged: (v) => _returnType = v.trim(),
                ),
              ),
            ),
            const SizedBox(height: 12),
          ],
          if (isPg) ...[
            FieldRow(
              label: '语言:',
              child: SizedBox(
                width: 200,
                child: ComboBox<String>(
                  items: RoutineSql.languagesOf(_typeId),
                  value: _language.isEmpty ? null : _language,
                  itemToString: (v) => v,
                  onChanged: (v) {
                    if (v != null) setState(() => _language = v);
                  },
                ),
              ),
            ),
            const SizedBox(height: 12),
            FieldRow(
              label: '安全:',
              child: CheckBox(
                value: _securityDefiner,
                onChanged: (v) => setState(() => _securityDefiner = v ?? false),
                label: 'SECURITY DEFINER(以定义者权限执行)',
              ),
            ),
            const SizedBox(height: 12),
          ],
          Button(
            text: '应用到 SQL',
            onPressed: _parsedOk ? _applyAdvanced : null,
          ),
          if (!_parsedOk)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(
                '当前 SQL 与模板结构不一致,无法自动应用高级选项\n'
                '(请直接在「定义」标签中编辑,或在新建模式下先应用)',
                style: TextStyle(fontSize: 11.5, color: t.mutedForeground),
              ),
            ),
        ],
      ),
    );
  }

  /// 注释标签:注释文本(仅 PG 保存为 COMMENT ON;其它类型仅作 SQL 预览注释)
  Widget _commentTab(AppPalette t) {
    return Padding(
      padding: const EdgeInsets.all(8),
      child: Textarea(
        controller: _commentController,
        focusNode: _commentFocusNode,
        expands: true,
        showBorder: false,
        hint: '输入 $_label 注释${RoutineSql.supportsCommentOn(_typeId) ? '(保存时写入 COMMENT ON)' : ''}',
        style: TextStyle(
          fontSize: 13.5,
          height: 20 / 13.5,
          color: t.foreground,
        ),
      ),
    );
  }

  /// SQL 预览标签:定义 SQL + 注释(只读,实时跟随)
  Widget _previewTab(AppPalette t) {
    return ListenableBuilder(
      listenable: Listenable.merge([_controller, _commentController]),
      builder: (context, _) {
        final preview = _previewSql();
        if (_previewController.text != preview) {
          _previewController.text = preview;
        }
        return _sqlEditor(t, controller: _previewController, readOnly: true);
      },
    );
  }

  /// 预览 SQL:定义 + 注释块(PG 附 COMMENT ON 语句,其它类型为 -- 注释行)
  String _previewSql() {
    final buf = StringBuffer(_controller.text.trimRight());
    final comment = _commentController.text.trim();
    if (comment.isEmpty) return buf.toString();
    final commentSql = RoutineSql.commentOnSql(
      typeId: _typeId,
      category: _category,
      name: widget.name,
      schema: widget.schema,
      signature: _parsedSignature,
      comment: comment,
    );
    buf.writeln();
    buf.writeln();
    buf.writeln('-- ===== 注释 =====');
    if (commentSql.isNotEmpty) {
      buf.writeln(commentSql);
    } else {
      for (final line in comment.split('\n')) {
        buf.writeln('-- $line');
      }
    }
    return buf.toString();
  }

  /// SQL 代码编辑器(定义 / 预览共用)
  Widget _sqlEditor(
    AppPalette t, {
    required CodeLineEditingController controller,
    CodeFindBuilder? findBuilder,
    bool readOnly = false,
  }) {
    final isDark = t.background.computeLuminance() < 0.5;
    return CodeEditor(
      controller: controller,
      focusNode: readOnly ? null : _focusNode,
      readOnly: readOnly,
      hint: readOnly ? null : '-- 在此编辑 $_label 定义 SQL',
      wordWrap: _wordWrap,
      chunkAnalyzer: const NonCodeChunkAnalyzer(),
      findBuilder: findBuilder,
      findController: findBuilder != null ? _findController : null,
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
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
      indicatorBuilder:
          (context, editingController, chunkController, notifier) =>
              DefaultCodeLineNumber(
        controller: editingController,
        notifier: notifier,
        textStyle: TextStyle(fontSize: 12.5, color: t.disabledForeground),
        focusedTextStyle: TextStyle(fontSize: 12.5, color: t.mutedForeground),
        minNumberCount: 4,
      ),
    );
  }

  /// 查找面板(CodeEditor.findBuilder):未进入查找模式时零高度
  PreferredSizeWidget _buildFind(
      BuildContext context, CodeFindController controller, bool readOnly) {
    return _RoutineFindPanel(controller: controller);
  }

  /// 状态栏:保存 / 执行状态、结果、错误
  Widget _statusBar(AppPalette t) {
    final isError = _error != null;
    final busy = _saving || _running;
    final text = _error ??
        (_running
            ? '正在执行 ...'
            : _saving
                ? '正在保存 ...'
                : _message);
    if (text == null) return const SizedBox.shrink();
    return Container(
      height: 26,
      color: t.secondary,
      padding: const EdgeInsets.only(left: 12, right: 8),
      child: Row(
        children: [
          if (busy) ...[
            const Spinner(size: 13),
            const SizedBox(width: 6),
          ] else if (isError) ...[
            const Icon(Icons.error_outline, size: 14, color: Color(0xffd93025)),
            const SizedBox(width: 6),
          ],
          Expanded(
            child: Text(
              text,
              style: TextStyle(
                fontSize: 12,
                color: isError ? const Color(0xffd93025) : t.mutedForeground,
              ),
              overflow: TextOverflow.ellipsis,
            ),
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

/// 查找栏:Input + 上一个 / 下一个 / 关闭。
/// 未进入查找模式(controller.value == null)时 preferredSize 为 0,
/// CodeEditor 自动不预留空间、不渲染内容。
class _RoutineFindPanel extends StatefulWidget implements PreferredSizeWidget {
  const _RoutineFindPanel({required this.controller});

  final CodeFindController controller;

  @override
  Size get preferredSize =>
      controller.value == null ? Size.zero : const Size(420, 30);

  @override
  State<_RoutineFindPanel> createState() => _RoutineFindPanelState();
}

class _RoutineFindPanelState extends State<_RoutineFindPanel> {
  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_onChanged);
  }

  @override
  void didUpdateWidget(_RoutineFindPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_onChanged);
      widget.controller.addListener(_onChanged);
    }
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onChanged);
    super.dispose();
  }

  void _onChanged() {
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    if (widget.controller.value == null) return const SizedBox.shrink();
    return Container(
      height: 30,
      color: t.secondary,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        children: [
          Icon(Icons.search, size: 14, color: t.mutedForeground),
          const SizedBox(width: 6),
          SizedBox(
            width: 220,
            child: Input(
              controller: widget.controller.findInputController,
              focusNode: widget.controller.findInputFocusNode,
              hint: '查找',
              textInputAction: TextInputAction.search,
            ),
          ),
          const SizedBox(width: 4),
          ToolbarButton(
            icon: Icons.arrow_upward,
            tooltip: '上一个',
            onTap: widget.controller.previousMatch,
          ),
          ToolbarButton(
            icon: Icons.arrow_downward,
            tooltip: '下一个',
            onTap: widget.controller.nextMatch,
          ),
          const Spacer(),
          ToolbarButton(
            icon: Icons.close,
            tooltip: '关闭',
            onTap: widget.controller.close,
          ),
        ],
      ),
    );
  }
}

/// SQL 语法高亮配色(与查询页一致:明亮近似 SSMS / VS Code Light+,
/// 暗黑近似 VS Code Dark+;中调色主题无关)
const Map<String, TextStyle> sqlLightTheme = {
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

const Map<String, TextStyle> sqlDarkTheme = {
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
