import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../data/create_database_catalog.dart';
import '../data/db_create_options.dart';
import '../data/db_data.dart';
import '../theme/app_theme.dart';
import '../theme/sql_highlight.dart';

/// 校验失败提示的红:与 `mcp_settings_dialog.dart` / `query_page.dart` 同一
/// 中调色,主题无关(色板里没有语义错误色)。
const Color _kErrorColor = Color(0xffd93025);

/// 「新建数据库」对话框:按连接类型展示不同选项,执行真实的 CREATE DATABASE。
///
/// 版式对齐 Navicat 的新建库窗口(标签页 + 一行一字段 + 底部确定/取消):
///
/// - 常规:库名 + 该类型专有选项(见 [_generalTab])
/// - 扩展:PostgreSQL 的 `CREATE EXTENSION`,多选列表([CheckedListBox])
/// - 注释:`COMMENT ON DATABASE`
/// - SQL 预览:实时展示将执行的完整脚本
///
/// PostgreSQL 的所有者 / 模板 / 表空间 / 扩展候选在打开时从服务端系统表读取
/// ([AppState.loadCreateDatabaseCatalog]),读取不到回退内置常量,不阻塞使用。
/// 成功时 [Navigator.pop] 返回 true(库列表已由 [AppState.createDatabase] 刷新)。
class CreateDatabaseDialog extends StatefulWidget {
  const CreateDatabaseDialog({super.key, required this.connection});

  /// 目标连接
  final ConnectionInfo connection;

  @override
  State<CreateDatabaseDialog> createState() => _CreateDatabaseDialogState();
}

class _CreateDatabaseDialogState extends State<CreateDatabaseDialog> {
  /// 对话框宽度(适配 daro 既有对话框尺寸,不照搬 Navicat 的 1060)
  static const double _kDialogWidth = 700;

  /// DialogBox 正文固定高度(TabControl 需要**有界**高度才能承载滚动列表)
  static const double _kBodyHeight = 520;

  /// 表单标签列宽(标签右对齐,控件左端对齐)
  static const double _kLabelWidth = 110;

  final _nameController = TextEditingController();
  final _lcCollateController = TextEditingController();
  final _lcCtypeController = TextEditingController();
  final _limitController = TextEditingController(text: '-1');
  final _commentController = TextEditingController();
  final _focusNode = FocusNode();

  /// 创建是否进行中(防止重复提交)
  bool _creating = false;

  /// 当前标签页下标(仅用于让 SQL 预览跟随输入刷新)
  int _tabIndex = 0;

  /// 下拉候选:先放兜底值,异步替换为服务端读取结果
  late DatabaseCreateCatalog _catalog;

  /// 是否成功读到服务端候选(用于提示"以下为内置候选")
  bool get _catalogFromServer => _catalog.loadedFromServer;

  String get _typeId => widget.connection.typeId;

  bool get _isMysqlLike => _typeId == 'mysql' || _typeId == 'mariadb';

  bool get _isPg => _typeId == 'postgresql';

  bool get _isSqlServer => _typeId == 'sqlserver';

  // MySQL / MariaDB
  late String _charset;
  late String _collation;

  // PostgreSQL
  late String _encoding;
  late String _template;
  String _owner = '';
  String _tablespace = '';
  bool _allowConnections = true;
  bool _isTemplate = false;
  final Set<String> _extensions = {};

  // SQL Server('' = 服务器默认)
  String _sqlServerCollation = '';

  @override
  void initState() {
    super.initState();
    _charset = kMysqlCharsets.first;
    _collation = mysqlCollationsFor(_charset).first;
    _encoding = kPgEncodings.first;
    _template = kPgTemplates.last; // template1(默认)
    _owner = widget.connection.username.trim();
    _catalog = DatabaseCreateCatalog.fallback(
      username: widget.connection.username,
    );
    // 打开后聚焦库名输入框
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _focusNode.requestFocus();
    });
    _loadCatalog();
  }

  @override
  void dispose() {
    _nameController.dispose();
    _lcCollateController.dispose();
    _lcCtypeController.dispose();
    _limitController.dispose();
    _commentController.dispose();
    _focusNode.dispose();
    super.dispose();
  }

  /// 异步读取服务端候选(所有者 / 模板 / 表空间 / 扩展)。
  /// 失败或未连接时保持兜底值,不弹错误(建库本身仍会照常校验)。
  Future<void> _loadCatalog() async {
    final app = context.read<AppState>();
    final catalog = await app.loadCreateDatabaseCatalog(widget.connection);
    if (!mounted) return;
    setState(() {
      _catalog = catalog;
      if (_isPg) {
        if (_owner.isEmpty && catalog.owners.isNotEmpty) {
          _owner = catalog.owners.first;
        }
        if (!catalog.templates.contains(_template) &&
            catalog.templates.isNotEmpty) {
          // 服务端模板列表与内置不同(自定义模板库):保留 template1 语义
          _template = catalog.templates.contains('template1')
              ? 'template1'
              : catalog.templates.first;
        }
      }
    });
  }

  /// 当前表单选项(供 SQL 预览与提交共用)
  CreateDatabaseOptions get _options => CreateDatabaseOptions(
        name: _nameController.text,
        charset: _isMysqlLike ? _charset : null,
        collation: _isMysqlLike
            ? _collation
            : (_isSqlServer ? _sqlServerCollation : null),
        encoding: _isPg ? _encoding : null,
        template: _isPg ? _template : null,
        owner: _isPg ? _owner : null,
        lcCollate: _isPg ? _lcCollateController.text : null,
        lcCtype: _isPg ? _lcCtypeController.text : null,
        tablespace: _isPg ? _tablespace : null,
        connectionLimit: _isPg ? _connectionLimit : null,
        allowConnections: _allowConnections,
        isTemplate: _isTemplate,
        extensions: _isPg ? (_extensions.toList()..sort()) : const [],
        comment: _isPg ? _commentController.text : null,
      );

  /// 连接限制(`-1` = 无限制;空 / 非法 = 不输出该子句)
  int? get _connectionLimit {
    final raw = _limitController.text.trim();
    if (raw.isEmpty) return null;
    return int.tryParse(raw);
  }

  /// 连接限制输入非法(非整数)
  bool get _limitInvalid {
    final raw = _limitController.text.trim();
    return raw.isNotEmpty && int.tryParse(raw) == null;
  }

  /// 字符集变化:排序规则跟随切换到该字符集的首选
  void _onCharsetChanged(String charset) {
    setState(() {
      _charset = charset;
      _collation = mysqlCollationsFor(charset).first;
    });
  }

  /// 编码变化:非 UTF8 编码必须使用 template0 模板,自动切换并提示
  void _onEncodingChanged(String encoding) {
    setState(() {
      _encoding = encoding;
      if (encoding != 'UTF8' && _template == 'template1') {
        _template = 'template0';
      }
    });
  }

  /// 模板提示:模板与编码 / 排序规则冲突时给出指引(见 PostgreSQL 文档)
  String? get _templateHint {
    if (!_isPg) return null;
    final needsTemplate0 = _encoding != 'UTF8' ||
        _lcCollateController.text.trim().isNotEmpty ||
        _lcCtypeController.text.trim().isNotEmpty;
    if (needsTemplate0 && _template == 'template1') {
      return '指定编码 / 排序规则 / 字符分类时需要 template0 模板';
    }
    return null;
  }

  /// 下拉候选中补上当前值(服务端列表里没有该值时也要能显示)
  List<String> _withValue(List<String> items, String value) {
    if (value.isEmpty || items.contains(value)) return items;
    return [value, ...items];
  }

  Future<void> _create() async {
    if (_creating) return;
    if (_nameController.text.trim().isEmpty) return;
    if (_limitInvalid) return;
    setState(() => _creating = true);

    final app = context.read<AppState>();
    final outcome = await app.createDatabase(widget.connection, _options);
    if (!mounted) return;

    if (outcome.ok) {
      Navigator.of(context).pop(true);
      return;
    }
    setState(() => _creating = false);
    MessageBox.show(
      context,
      title: '新建数据库',
      message: '创建失败:\n${outcome.error}',
      type: MessageBoxType.error,
      okText: '知道了',
    );
  }

  /// TabControl 的标签条 + 面板底边线高度(与 stripHeight 同源,勿硬编码)
  static double _tabChromeHeight(DesktopTokens tokens) =>
      TabControl.stripHeight(tokens) + tokens.borderWidth;

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final dt = t.desktopTokensFor(context);
    final bodyHeight = _kBodyHeight - _tabChromeHeight(dt);
    return DialogBox(
      title: '新建数据库',
      width: _kDialogWidth,
      height: _kBodyHeight,
      onClose: () => Navigator.of(context).pop(),
      footer: _footer(t, dt),
      child: SizedBox(
        height: _kBodyHeight,
        child: TabControl(
          initialIndex: _tabIndex,
          contentPadding: EdgeInsets.zero,
          onChanged: (i) => setState(() => _tabIndex = i),
          // 每个页正文都要自己有界高度:TabControl 的正文列是
          // `mainAxisSize: min`,不给子级高度约束,滚动区会无限撑开。
          tabs: [
            TabItem(
              label: '常规',
              child: SizedBox(height: bodyHeight, child: _generalTab(t)),
            ),
            TabItem(
              label: '扩展',
              child: SizedBox(height: bodyHeight, child: _extensionTab(t)),
            ),
            TabItem(
              label: '注释',
              child: SizedBox(height: bodyHeight, child: _commentTab(t)),
            ),
            TabItem(
              label: 'SQL 预览',
              child: SizedBox(height: bodyHeight, child: _previewTab(t)),
            ),
          ],
        ),
      ),
    );
  }

  /// 底部动作条:确定(默认按钮,白底 + 强调色边框)/ 取消,均右对齐
  Widget _footer(AppPalette t, DesktopTokens dt) {
    final canSubmit =
        _nameController.text.trim().isNotEmpty && !_creating && !_limitInvalid;
    return Row(
      mainAxisAlignment: MainAxisAlignment.end,
      children: [
        Button(
          text: _creating ? '创建中...' : '确定',
          // base-ui 尚无「默认按钮」变体,用令牌覆盖出白底 + 强调色边框
          tokens: dt.copyWith(buttonBorderColor: t.accent),
          onPressed: canSubmit ? _create : null,
        ),
        const SizedBox(width: 8),
        Button(
          text: '取消',
          onPressed: _creating ? null : () => Navigator.of(context).pop(),
        ),
      ],
    );
  }

  // ── ① 常规 ────────────────────────────────────────────────────────────────

  Widget _generalTab(AppPalette t) {
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _row(
            '数据库名称:',
            Input(
              controller: _nameController,
              focusNode: _focusNode,
              onChanged: (_) => setState(() {}),
              onSubmitted: (_) => _create(),
              textInputAction: TextInputAction.go,
            ),
          ),
          const SizedBox(height: 12),
          if (_isMysqlLike) ..._mysqlRows(),
          if (_isPg) ..._pgRows(t),
          if (_isSqlServer) ..._sqlServerRows(),
        ],
      ),
    );
  }

  /// 一行「右对齐标签 + 铺满剩余宽度的控件」
  Widget _row(String label, Widget control) => FieldRow(
        label: label,
        labelWidth: _kLabelWidth,
        child: SizedBox(width: double.infinity, child: control),
      );

  /// 本对话框内的下拉框统一取纸白底:参考窗口里输入框与下拉框没有底色差,
  /// 不覆盖会落到 base-ui 的铬件灰(controlColor)。
  Widget _combo<T extends Object>({
    required List<T> items,
    required T? value,
    required ValueChanged<T?> onChanged,
    String Function(T)? itemToString,
  }) {
    final p = Tokens.of(context);
    return ComboBox<T>(
      items: items,
      value: value,
      onChanged: onChanged,
      itemToString: itemToString,
      tokens: p.desktopTokensFor(context).copyWith(controlColor: p.background),
    );
  }

  List<Widget> _mysqlRows() => [
        _row(
          '字符集:',
          _combo<String>(
            items: kMysqlCharsets,
            value: _charset,
            onChanged: (v) {
              if (v != null) _onCharsetChanged(v);
            },
          ),
        ),
        const SizedBox(height: 12),
        _row(
          '排序规则:',
          _combo<String>(
            items: mysqlCollationsFor(_charset),
            value: _collation,
            onChanged: (v) {
              if (v != null) setState(() => _collation = v);
            },
          ),
        ),
      ];

  List<Widget> _pgRows(AppPalette t) {
    final hint = _templateHint;
    return [
      _row(
        '所有者:',
        _combo<String>(
          items: _withValue(_catalog.owners, _owner),
          value: _owner.isEmpty ? null : _owner,
          onChanged: (v) => setState(() => _owner = v ?? ''),
        ),
      ),
      const SizedBox(height: 12),
      _row(
        '模板:',
        _combo<String>(
          items: _withValue(_catalog.templates, _template),
          value: _template.isEmpty ? null : _template,
          onChanged: (v) => setState(() => _template = v ?? ''),
        ),
      ),
      const SizedBox(height: 12),
      _row(
        '编码:',
        _combo<String>(
          items: kPgEncodings,
          value: _encoding,
          onChanged: (v) {
            if (v != null) _onEncodingChanged(v);
          },
        ),
      ),
      const SizedBox(height: 12),
      // 排序规则(LC_COLLATE):自由文本,留空 = 跟随模板
      _row('排序规则:', _textField(_lcCollateController)),
      const SizedBox(height: 12),
      // 字符分类(LC_CTYPE)
      _row('字符分类:', _textField(_lcCtypeController)),
      const SizedBox(height: 12),
      _row(
        '表空间:',
        _combo<String>(
          items: _catalog.tablespaces,
          value: _tablespace.isEmpty ? null : _tablespace,
          onChanged: (v) => setState(() => _tablespace = v ?? ''),
        ),
      ),
      const SizedBox(height: 12),
      _row(
        '连接限制:',
        Input(
          controller: _limitController,
          keyboardType: TextInputType.number,
          onChanged: (_) => setState(() {}),
        ),
      ),
      if (_limitInvalid) ...[
        const SizedBox(height: 6),
        _hintText('连接限制必须是整数(-1 表示无限制)', _kErrorColor),
      ],
      const SizedBox(height: 14),
      // 两个布尔选项与标签列左边缘对齐(与参考窗口一致,不缩进)
      CheckBox(
        value: _allowConnections,
        onChanged: (v) => setState(() => _allowConnections = v ?? true),
        label: '允许连接',
      ),
      const SizedBox(height: 8),
      CheckBox(
        value: _isTemplate,
        onChanged: (v) => setState(() => _isTemplate = v ?? false),
        label: '是否模板',
      ),
      if (hint != null) ...[
        const SizedBox(height: 10),
        _hintText(hint, t.mutedForeground),
      ],
    ];
  }

  List<Widget> _sqlServerRows() => [
        _row(
          '排序规则:',
          _combo<String>(
            items: kSqlServerCollations,
            value: _sqlServerCollation,
            onChanged: (v) {
              if (v != null) setState(() => _sqlServerCollation = v);
            },
            itemToString: (v) => v.isEmpty ? '服务器默认' : v,
          ),
        ),
      ];

  Widget _textField(TextEditingController controller) => Input(
        controller: controller,
        onChanged: (_) => setState(() {}),
      );

  Widget _hintText(String text, Color color) => Padding(
        padding: const EdgeInsets.only(left: _kLabelWidth + 10),
        child: Text(
          text,
          style: TextStyle(
            fontSize: 11,
            color: color,
            decoration: TextDecoration.none,
          ),
        ),
      );

  // ── ② 扩展(PostgreSQL) ────────────────────────────────────────────────────

  Widget _extensionTab(AppPalette t) {
    if (!_isPg) return _unsupportedTab(t, '扩展', '仅 PostgreSQL 支持扩展');
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 14, 20, 14),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '勾选需要的扩展,建库后在新库中执行 CREATE EXTENSION IF NOT EXISTS:',
            style: TextStyle(
              fontSize: 11,
              color: t.mutedForeground,
              decoration: TextDecoration.none,
            ),
          ),
          const SizedBox(height: 10),
          Expanded(
            child: CheckedListBox<PgExtensionInfo>(
              items: _catalog.extensions,
              checkedIndices: _checkedExtensionIndices,
              itemToString: (e) =>
                  e.comment.isEmpty ? e.name : '${e.name}    ${e.comment}',
              onItemCheckChanged: _onExtensionsChanged,
            ),
          ),
          if (!_catalogFromServer) ...[
            const SizedBox(height: 8),
            Text(
              '未连接服务器,以上为内置候选列表',
              style: TextStyle(
                fontSize: 11,
                color: t.mutedForeground,
                decoration: TextDecoration.none,
              ),
            ),
          ],
        ],
      ),
    );
  }

  /// 已勾选扩展在候选列表中的下标集合(CheckedListBox 按下标工作)
  Set<int> get _checkedExtensionIndices {
    final items = _catalog.extensions;
    return {
      for (var i = 0; i < items.length; i++)
        if (_extensions.contains(items[i].name)) i,
    };
  }

  void _onExtensionsChanged(Set<int> indices) {
    final items = _catalog.extensions;
    setState(() {
      _extensions
        ..clear()
        ..addAll(
          indices.where((i) => i >= 0 && i < items.length).map((i) => items[i].name),
        );
    });
  }

  // ── ③ 注释(PostgreSQL) ───────────────────────────────────────────────────

  Widget _commentTab(AppPalette t) {
    if (!_isPg) return _unsupportedTab(t, '注释', '仅 PostgreSQL 支持数据库注释');
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 14, 20, 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '数据库注释(COMMENT ON DATABASE),留空则不生成注释语句:',
            style: TextStyle(
              fontSize: 11,
              color: t.mutedForeground,
              decoration: TextDecoration.none,
            ),
          ),
          const SizedBox(height: 10),
          Expanded(
            child: Textarea(
              controller: _commentController,
              expands: true,
              onChanged: (_) => setState(() {}),
            ),
          ),
        ],
      ),
    );
  }

  // ── ④ SQL 预览 ───────────────────────────────────────────────────────────

  Widget _previewTab(AppPalette t) {
    final script = buildCreateDatabaseScript(_typeId, _options);
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 14, 20, 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (_nameController.text.trim().isEmpty) ...[
            Text(
              '请先填写数据库名称,名称会作为标识符写入语句。',
              style: TextStyle(
                fontSize: 11,
                color: t.mutedForeground,
                decoration: TextDecoration.none,
              ),
            ),
            const SizedBox(height: 8),
          ],
          Expanded(child: _previewBox(t, script)),
        ],
      ),
    );
  }

  Widget _previewBox(AppPalette t, String script) {
    return Container(
      width: double.infinity,
      decoration: BoxDecoration(
        color: t.background,
        border: Border.all(color: t.border),
      ),
      child: SingleChildScrollView(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
        child: SelectableText.rich(
          // 与详情面板 DDL 页同款高亮;行距沿用这里原本的默认值,不顺手改排版。
          highlightedSqlSpan(script, palette: t, height: null),
        ),
      ),
    );
  }

  Widget _unsupportedTab(AppPalette t, String name, String message) => Padding(
        padding: const EdgeInsets.all(20),
        child: Text(
          '$message,「$name」页不可用。',
          style: TextStyle(
            fontSize: 12,
            color: t.mutedForeground,
            decoration: TextDecoration.none,
          ),
        ),
      );
}
