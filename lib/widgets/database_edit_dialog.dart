import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../data/create_database_catalog.dart';
import '../data/database_edit_catalog.dart';
import '../data/db_data.dart';
import '../data/db_edit_options.dart';
import '../theme/app_theme.dart';
import '../theme/sql_highlight.dart';

/// 校验失败提示的红:与「新建数据库」同一中调色,主题无关(色板里没有语义错误色)。
const Color _kErrorColor = Color(0xffd93025);

/// 「编辑数据库」对话框(PostgreSQL 家族):修改已存在库的属性、注释与扩展。
///
/// 版式对齐 Navicat 的编辑库窗口(标签页 + 底部确定/取消):
///
/// - 常规:所有者 / 表空间 / 连接限制 / 允许连接 / 是否模板 可改;
///   库名与编码 / 排序规则 / 字符分类 建库后不可变,只读展示
/// - 扩展:「可用 / 已安装」双列表转移,`>` 装、`<` 卸(双击同效)
/// - 注释:`COMMENT ON DATABASE`
/// - SQL 预览:实时展示将执行的差异语句
///
/// 打开时一次性读取服务端现状([AppState.loadDatabaseEditSnapshot])作为表单初值
/// 与差异基准;「确定」只提交真正变化过的语句([buildEditDatabaseStatements]),
/// 现状读不到时禁用提交(基准不可信,否则会重放一堆无谓的 `ALTER DATABASE`)。
class DatabaseEditDialog extends StatefulWidget {
  const DatabaseEditDialog({
    super.key,
    required this.connection,
    required this.database,
  });

  /// 目标连接
  final ConnectionInfo connection;

  /// 目标库名
  final String database;

  @override
  State<DatabaseEditDialog> createState() => _DatabaseEditDialogState();
}

class _DatabaseEditDialogState extends State<DatabaseEditDialog> {
  /// 对话框宽度:比新建库宽一点,扩展页要并排放下两个列表
  static const double _kDialogWidth = 760;

  /// DialogBox 正文固定高度(TabControl 需要**有界**高度才能承载滚动列表)
  static const double _kBodyHeight = 520;

  /// 表单标签列宽(与新建库对话框一致)
  static const double _kLabelWidth = 110;

  /// 扩展列表「版本」列宽
  static const double _kVersionColumnWidth = 88;

  final _limitController = TextEditingController();
  final _commentController = TextEditingController();

  /// 只读展示的不可变属性(库名 / 编码 / 排序规则 / 字符分类)
  final _nameController = TextEditingController();
  final _encodingController = TextEditingController();
  final _collateController = TextEditingController();
  final _ctypeController = TextEditingController();

  /// 服务端现状;null = 仍在读取
  DatabaseEditSnapshot? _snapshot;

  /// 读取快照失败的原因(为空 = 成功)
  String? _loadError;

  /// 提交是否进行中(防止重复点击)
  bool _applying = false;

  /// 当前标签页下标(仅用于让 SQL 预览跟随输入刷新)
  int _tabIndex = 0;

  // ── 常规页表单值(快照到达后回填) ──────────────────────────────────────────
  String _owner = '';
  String _tablespace = '';
  bool _allowConnections = true;
  bool _isTemplate = false;

  // ── 扩展页:两份列表 + 待提交的名字集合 ────────────────────────────────────
  List<PgExtensionInfo> _available = const [];
  List<PgExtensionInfo> _installed = const [];

  /// 被 `>` 移到「已安装」的扩展名(待执行 CREATE EXTENSION)
  final Set<String> _toInstall = {};

  /// 被 `<` 移回「可用」的扩展名(待执行 DROP EXTENSION)
  final Set<String> _toDrop = {};

  int? _selectedAvailable;
  int? _selectedInstalled;

  String get _typeId => widget.connection.typeId;

  @override
  void initState() {
    super.initState();
    _nameController.text = widget.database;
    WidgetsBinding.instance.addPostFrameCallback((_) => _load());
  }

  @override
  void dispose() {
    _limitController.dispose();
    _commentController.dispose();
    _nameController.dispose();
    _encodingController.dispose();
    _collateController.dispose();
    _ctypeController.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    final app = context.read<AppState>();
    try {
      final snapshot =
          await app.loadDatabaseEditSnapshot(widget.connection, widget.database);
      if (!mounted) return;
      setState(() {
        _snapshot = snapshot;
        _owner = snapshot.props.owner;
        _tablespace = snapshot.props.tablespace;
        _allowConnections = snapshot.props.allowConnections;
        _isTemplate = snapshot.props.isTemplate;
        _limitController.text = '${snapshot.props.connectionLimit}';
        _commentController.text = snapshot.props.comment;
        _encodingController.text = snapshot.props.encoding;
        _collateController.text = snapshot.props.lcCollate;
        _ctypeController.text = snapshot.props.lcCtype;
        _available = snapshot.availableExtensions;
        _installed = snapshot.installedExtensions;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _loadError = e.toString());
    }
  }

  /// 当前表单选项(供 SQL 预览与提交共用)
  DatabaseEditForm get _form => DatabaseEditForm(
        owner: _owner,
        tablespace: _tablespace,
        connectionLimit: _connectionLimit,
        allowConnections: _allowConnections,
        isTemplate: _isTemplate,
        comment: _commentController.text,
        installExtensions: _toInstall.toList()..sort(),
        uninstallExtensions: _toDrop.toList()..sort(),
      );

  /// 连接限制(`-1` = 无限制;空 / 非法按无限制处理,与 PG 默认一致)
  int get _connectionLimit =>
      int.tryParse(_limitController.text.trim()) ?? -1;

  /// 连接限制输入非法(非整数)
  bool get _limitInvalid {
    final raw = _limitController.text.trim();
    return raw.isNotEmpty && int.tryParse(raw) == null;
  }

  /// 有可提交的改动,且现状基准可信、输入合法、未在提交中
  bool get _canApply {
    final s = _snapshot;
    if (s == null || _applying || !s.loadedFromServer || _limitInvalid) {
      return false;
    }
    return buildEditDatabaseStatements(
          typeId: _typeId,
          current: s.props,
          form: _form,
        ).isNotEmpty;
  }

  Future<void> _apply() async {
    final s = _snapshot;
    if (s == null || !_canApply) return;
    setState(() => _applying = true);

    final outcome =
        await context.read<AppState>().applyDatabaseEdits(_conn, s, _form);
    if (!mounted) return;

    if (outcome.ok) {
      Navigator.of(context).pop(true);
      return;
    }
    setState(() => _applying = false);
    MessageBox.show(
      context,
      title: '编辑数据库',
      message: '保存失败,以下语句未执行成功:\n${outcome.error}',
      type: MessageBoxType.error,
      okText: '知道了',
    );
  }

  ConnectionInfo get _conn => widget.connection;

  // ── 扩展页转移逻辑 ─────────────────────────────────────────────────────────

  /// `>`:把选中的可用扩展移入「已安装」(标记待安装)
  void _installSelected() {
    final i = _selectedAvailable;
    if (i == null || i < 0 || i >= _available.length) return;
    final moved = _available[i];
    setState(() {
      _available = [..._available]..removeAt(i);
      _installed = [..._installed, moved]
        ..sort((a, b) => a.name.compareTo(b.name));
      _toInstall.add(moved.name);
      _toDrop.remove(moved.name);
      _selectedAvailable = null;
      _selectedInstalled = _installed.indexOf(moved);
    });
  }

  /// `<`:把选中的已安装扩展移回「可用」(标记待卸载)
  void _uninstallSelected() {
    final i = _selectedInstalled;
    if (i == null || i < 0 || i >= _installed.length) return;
    final moved = _installed[i];
    setState(() {
      _installed = [..._installed]..removeAt(i);
      _available = [..._available, moved]
        ..sort((a, b) => a.name.compareTo(b.name));
      _toDrop.add(moved.name);
      _toInstall.remove(moved.name);
      _selectedInstalled = null;
      _selectedAvailable = _available.indexOf(moved);
    });
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final dt = t.desktopTokensFor(context);
    final bodyHeight = _kBodyHeight - _tabChromeHeight(dt);
    return DialogBox(
      title: '编辑数据库',
      width: _kDialogWidth,
      height: _kBodyHeight,
      onClose: () => Navigator.of(context).pop(),
      footer: _footer(t, dt),
      child: SizedBox(
        height: _kBodyHeight,
        child: _snapshot == null
            ? _loading(t, bodyHeight)
            : TabControl(
                initialIndex: _tabIndex,
                contentPadding: EdgeInsets.zero,
                onChanged: (i) => setState(() => _tabIndex = i),
                // 每个页正文都要自己有界高度:TabControl 的正文列是
                // `mainAxisSize: min`,不给子级高度约束,滚动列表会无限撑开。
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

  /// 读取中 / 读取失败占位:保持同样的正文高度,避免对话框尺寸跳动
  Widget _loading(AppPalette t, double bodyHeight) {
    final msg = _loadError == null
        ? '正在读取「${widget.database}」的信息...'
        : '读取库信息失败:\n$_loadError';
    return SizedBox(
      height: bodyHeight,
      child: Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Text(
            msg,
            textAlign: TextAlign.center,
            style: TextStyle(
              fontSize: 12,
              color: _loadError == null ? t.mutedForeground : _kErrorColor,
              decoration: TextDecoration.none,
            ),
          ),
        ),
      ),
    );
  }

  /// TabControl 的标签条 + 面板底边线高度(与 stripHeight 同源,勿硬编码)
  static double _tabChromeHeight(DesktopTokens tokens) =>
      TabControl.stripHeight(tokens) + tokens.borderWidth;

  /// 底部动作条:确定(有改动才可点)/ 取消,均右对齐
  Widget _footer(AppPalette t, DesktopTokens dt) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.end,
      children: [
        Button(
          text: _applying ? '保存中...' : '确定',
          // base-ui 尚无「默认按钮」变体,用令牌覆盖出白底 + 强调色边框
          tokens: dt.copyWith(buttonBorderColor: t.accent),
          onPressed: _canApply ? _apply : null,
        ),
        const SizedBox(width: 8),
        Button(
          text: '取消',
          onPressed: _applying ? null : () => Navigator.of(context).pop(),
        ),
      ],
    );
  }

  // ── ① 常规 ────────────────────────────────────────────────────────────────

  Widget _generalTab(AppPalette t) {
    final s = _snapshot!;
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _row('数据库名称:', _readOnlyField(_nameController)),
          const SizedBox(height: 12),
          _row(
            '所有者:',
            _combo<String>(
              items: s.owners,
              value: _owner.isEmpty ? null : _owner,
              onChanged: (v) => setState(() => _owner = v ?? ''),
            ),
          ),
          const SizedBox(height: 12),
          _row(
            '表空间:',
            _combo<String>(
              items: s.tablespaces,
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
          const SizedBox(height: 16),
          _row('编码:', _readOnlyField(_encodingController)),
          const SizedBox(height: 12),
          _row('排序规则:', _readOnlyField(_collateController)),
          const SizedBox(height: 12),
          _row('字符分类:', _readOnlyField(_ctypeController)),
          const SizedBox(height: 10),
          _hintText('编码与排序规则建库后不可修改;移动表空间需要该库没有其它连接。',
              t.mutedForeground),
          if (!s.loadedFromServer) ...[
            const SizedBox(height: 10),
            _hintText('未能读取该库的当前属性,已禁用保存(避免按错误的现状生成语句)。',
                _kErrorColor),
          ],
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

  /// 不可改的现状字段:禁用输入框展示(比纯文本更容易与其它行对齐扫读)
  Widget _readOnlyField(TextEditingController controller) =>
      Input(controller: controller, enabled: false);

  /// 下拉候选统一取纸白底:与新建库对话框一致,不覆盖会落到铬件灰
  Widget _combo<T extends Object>({
    required List<T> items,
    required T? value,
    required ValueChanged<T?> onChanged,
  }) {
    final p = Tokens.of(context);
    return ComboBox<T>(
      items: items,
      value: value,
      onChanged: onChanged,
      tokens: p.desktopTokensFor(context).copyWith(controlColor: p.background),
    );
  }

  Widget _hintText(String text, Color color) => Text(
        text,
        style: TextStyle(
          fontSize: 11,
          color: color,
          decoration: TextDecoration.none,
        ),
      );

  // ── ② 扩展(可用 / 已安装 转移) ────────────────────────────────────────────

  Widget _extensionTab(AppPalette t) {
    final s = _snapshot!;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 14, 20, 14),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '选中一行后用中间按钮(或直接双击行)在两个列表间移动;确定时按移动结果执行 '
            'CREATE / DROP EXTENSION:',
            style: TextStyle(
              fontSize: 11,
              color: t.mutedForeground,
              decoration: TextDecoration.none,
            ),
          ),
          const SizedBox(height: 10),
          Expanded(
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Expanded(
                  child: _extensionList(
                    t,
                    title: '可用',
                    items: _available,
                    selected: _selectedAvailable,
                    onSelect: (i) => setState(() => _selectedAvailable = i),
                    onActivate: _installSelected,
                  ),
                ),
                _transferColumn(),
                Expanded(
                  child: _extensionList(
                    t,
                    title: '已安装',
                    items: _installed,
                    selected: _selectedInstalled,
                    onSelect: (i) => setState(() => _selectedInstalled = i),
                    onActivate: _uninstallSelected,
                  ),
                ),
              ],
            ),
          ),
          if (!s.extensionsLoaded) ...[
            const SizedBox(height: 8),
            _hintText('未能读取该库的扩展清单(服务端版本过低或权限不足)。',
                _kErrorColor),
          ],
        ],
      ),
    );
  }

  /// 中间一列转移按钮:上下排列,无选中时禁用
  Widget _transferColumn() {
    return Container(
      width: 96,
      alignment: Alignment.center,
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Button(
            text: '>',
            onPressed: (_selectedAvailable ?? -1) >= 0 ? _installSelected : null,
          ),
          const SizedBox(height: 10),
          Button(
            text: '<',
            onPressed:
                (_selectedInstalled ?? -1) >= 0 ? _uninstallSelected : null,
          ),
        ],
      ),
    );
  }

  /// 一个扩展列表:标题 + 名称/版本 两列网格 + 选中项说明
  Widget _extensionList(
    AppPalette t, {
    required String title,
    required List<PgExtensionInfo> items,
    required int? selected,
    required ValueChanged<int> onSelect,
    required VoidCallback onActivate,
  }) {
    final current = (selected != null && selected >= 0 && selected < items.length)
        ? items[selected]
        : null;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Label(title),
        const SizedBox(height: 6),
        Expanded(
          child: Container(
            decoration: BoxDecoration(
              color: t.background,
              border: Border.all(color: t.border),
            ),
            child: DataGridView(
              columns: const [
                DataGridViewColumn(title: '名称'),
                DataGridViewColumn(
                  title: '版本',
                  width: _kVersionColumnWidth,
                  flex: 0,
                ),
              ],
              rowCount: items.length,
              selectedRow: selected,
              onRowSelected: onSelect,
              onCellDoubleTap: (row, _) => onActivate(),
              cellBuilder: (row, col) {
                final e = items[row];
                final text = col == 0 ? e.name : e.version;
                return Text(
                  text,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: t.foreground,
                    decoration: TextDecoration.none,
                  ),
                );
              },
            ),
          ),
        ),
        const SizedBox(height: 6),
        SizedBox(
          height: 16,
          child: Text(
            current == null ? '' : current.comment,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(
              fontSize: 11,
              color: t.mutedForeground,
              decoration: TextDecoration.none,
            ),
          ),
        ),
      ],
    );
  }

  // ──  注释 ────────────────────────────────────────────────────────────────

  Widget _commentTab(AppPalette t) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 14, 20, 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '数据库注释(COMMENT ON DATABASE),清空后保存会移除现有注释:',
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
    final s = _snapshot!;
    final script = buildEditDatabaseScript(
      typeId: _typeId,
      current: s.props,
      form: _form,
    );
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 14, 20, 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Container(
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
            ),
          ),
        ],
      ),
    );
  }
}
