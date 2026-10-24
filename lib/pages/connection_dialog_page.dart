import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../app/app_state.dart';
import '../data/db_data.dart';
import '../data/db_types.dart';
import '../data/drivers/kv_driver.dart';
import '../theme/app_theme.dart';
import '../widgets/connection_group_prompt.dart';
import '../widgets/connection_form_page.dart';

/// 「新建 / 编辑连接」弹窗的客户区尺寸(逻辑像素)。
///
/// 两步向导共用一个尺寸:写死一次比每步按内容重排更稳(选类型与填表单内容高度差很多)。
const Size kConnectionEditorContentSize = Size(960, 620);

/// 连接向导的两步内容体:与弹窗外壳分离,[ConnectionDialogPage] 负责标题、
/// 关闭按钮与遮罩,这里只有步骤内容。
/// 内部维护一个两步状态机:
///   1. `select` 步:展示数据库类型网格/列表,选择后点「下一步」切到下一步
///   2. `form` 步:连接配置表单(参见 [ConnectionFormPage]),可点「上一步」回退
///
/// 传入 [initial] 时为「编辑连接」模式:跳过类型选择直接进入表单,表单预填该连接
/// 现有配置。结果一律经 [onResult] 上报,null 表示取消。
class ConnectionWizard extends StatefulWidget {
  const ConnectionWizard({
    super.key,
    required this.onResult,
    required this.onTestConnection,
    this.initial,
    this.groupOptions = const [],
    this.onCreateGroup,
  });

  /// 编辑已有连接时传入的初始配置;新建连接时为 null
  final ConnectionInfo? initial;

  /// 用户点「确定」时收到连接信息,点「取消」或关闭窗口时收到 null
  final ValueChanged<ConnectionInfo?> onResult;

  /// 由宿主提供的「测试连接」实现:内容体不碰 AppState,便于单独排版与测试
  final Future<(bool, String)> Function(ConnectionInfo) onTestConnection;

  /// 由宿主提供的分组候选与「新建分组」动作(同样不碰 AppState)
  final List<String> groupOptions;
  final Future<String?> Function()? onCreateGroup;

  @override
  State<ConnectionWizard> createState() => _ConnectionWizardState();
}

class _ConnectionWizardState extends State<ConnectionWizard> {
  /// 当前选中的数据库类型 id(select 步)
  String? _selectedId;

  /// 当前选中的完整数据库类型对象,非空 = 已进入 form 步
  DbType? _selectedType;

  /// 搜索关键词
  String _keyword = '';

  /// 网格 / 列表 视图
  bool _gridView = true;

  final _searchController = TextEditingController();

  @override
  void initState() {
    super.initState();
    // 编辑模式:按原连接类型直接进入 form 步(类型不可更改)
    final initial = widget.initial;
    if (initial != null) {
      _selectedId = initial.typeId;
      _selectedType = _findType(initial.typeId);
    }
  }

  @override
  void dispose() {
    _searchController.dispose();
    super.dispose();
  }

  /// 按 id 在所有已知类型里查找完整 [DbType]
  DbType? _findType(String id) {
    for (final t in kAllDbTypes) {
      if (t.id == id) return t;
    }
    return null;
  }

  /// select 步:按关键词过滤数据库列表
  List<DbType> _filter(List<DbType> source) {
    if (_keyword.isEmpty) return source;
    final k = _keyword.toLowerCase();
    return source.where((d) => d.label.toLowerCase().contains(k)).toList();
  }

  /// 「下一步」:select → form
  void _onNext() {
    final id = _selectedId;
    if (id == null) return;
    if (!isEngineTypeSupported(id)) return;
    final type = _findType(id);
    if (type == null) return;
    setState(() => _selectedType = type);
  }

  /// 双击类型卡片:直接选中并进入 form 步,跳过「下一步」。
  void _onDoubleSelect(DbType type) {
    if (!isEngineTypeSupported(type.id)) return;
    setState(() {
      _selectedId = type.id;
      _selectedType = type;
    });
  }

  /// 「上一步」:form → select(保留已选类型以便回填)
  void _onBack() {
    setState(() => _selectedType = null);
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    // FocusTraversalGroup:Tab / 方向键在控件间移动焦点
    return FocusTraversalGroup(
      policy: WidgetOrderTraversalPolicy(),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // 这一步的高度由宿主给的 Expanded 吸收,内容超出走各自的内部滚动
          Expanded(
            child: _selectedType != null
                ? ConnectionFormPage(
                    type: _selectedType!,
                    // 编辑模式无类型选择步可回退,隐藏「上一步」
                    onBack: widget.initial == null ? _onBack : null,
                    onCancel: () => widget.onResult(null),
                    onConfirm: widget.onResult,
                    onTestConnection: widget.onTestConnection,
                    initial: widget.initial,
                    groupOptions: widget.groupOptions,
                    onCreateGroup: widget.onCreateGroup,
                  )
                : _selectStep(context, t),
          ),
          // select 步的底部按钮(form 步自带底部按钮,不重复渲染)
          if (_selectedType == null) _footer(),
        ],
      ),
    );
  }

  /// select 步:类型选择内容(标题说明 + 搜索 + 网格)
  Widget _selectStep(BuildContext context, AppPalette t) {
    final all = _filter(kAllDbTypes);

    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _header(context, t),
          const SizedBox(height: 14),
          Expanded(
            child: SingleChildScrollView(
              child: _typeGrid(context, all),
            ),
          ),
        ],
      ),
    );
  }

  // 顶部:标题 + 视图切换 + 搜索框
  Widget _header(BuildContext context, AppPalette t) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        Text(
          '选择一个连接类型:',
          style: TextStyle(
            color: t.foreground,
            fontSize: 14,
            fontWeight: FontWeight.w600,
          ),
        ),
        const Spacer(),
        _ViewToggle(
          gridView: _gridView,
          onChanged: (g) => setState(() => _gridView = g),
          tokens: t,
        ),
        const SizedBox(width: 12),
        SizedBox(
          width: 240,
          height: 30,
          child: Row(
            children: [
              const Padding(
                padding: EdgeInsets.only(right: 6),
                child: Icon(Icons.search, size: 16),
              ),
              Expanded(
                child: Input(
                  controller: _searchController,
                  hint: '搜索',
                  onChanged: (v) => setState(() => _keyword = v),
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _typeGrid(BuildContext context, List<DbType> items) {
    return _TypeGrid(
      items: items,
      gridView: _gridView,
      selectedId: _selectedId,
      onSelect: (t) => setState(() => _selectedId = t.id),
      onDoubleSelect: _onDoubleSelect,
    );
  }

  /// select 步底部按钮:留白与 form 步一致,两步切换时按钮不跳位
  Widget _footer() {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 10, 20, 12),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.end,
        children: [
          Button(
            text: '取消',
            onPressed: () => widget.onResult(null),
          ),
          const SizedBox(width: 8),
          Button(
            text: '下一步',
            onPressed: _selectedId != null &&
                    isEngineTypeSupported(_selectedId!)
                ? _onNext
                : null,
          ),
        ],
      ),
    );
  }
}

/// 「新建 / 编辑连接」弹窗宿主:以 base-ui `DialogBox` 承载 [ConnectionWizard],
/// 结果经 [Navigator.pop] 回传(取消为 null)。
class ConnectionDialogPage extends StatelessWidget {
  const ConnectionDialogPage({super.key, this.initial});

  /// 编辑已有连接时传入的初始配置;新建连接时为 null
  final ConnectionInfo? initial;

  @override
  Widget build(BuildContext context) {
    final app = Provider.of<AppState>(context, listen: false);
    return DialogBox(
      title: initial == null ? '新建连接' : '编辑连接',
      width: kConnectionEditorContentSize.width,
      // 两步向导都有可变高度语义的区域,不固定就会被 DialogBox 的收缩排版压成一小块。
      height: kConnectionEditorContentSize.height,
      onClose: () => Navigator.of(context).pop(),
      child: ConnectionWizard(
        initial: initial,
        // 缓存 / KV 引擎(redis 等)走另一条驱动链,SQL 侧的 testConnection 不认它;
        // 这里按类型 id 分流 —— 测试用的 ConnectionInfo 没有 isLive,
        // 不能用 isKvStoreConnection 判
        onTestConnection: (conn) => kKvStoreTypes.contains(conn.typeId)
            ? app.kvManager.testConnection(conn)
            : app.connectionManager.testConnection(conn),
        onResult: (result) => Navigator.of(context).pop(result),
        // 分组候选与「新建分组」由宿主(唯一知道 AppState 的一层)提供
        groupOptions: app.groupNames,
        onCreateGroup: () async {
          final name = await promptGroupName(
            context,
            app,
            title: '新建连接分组',
            okText: '新建',
          );
          return name == null ? null : app.addGroup(name);
        },
      ),
    );
  }
}

/// 网格 / 列表 视图切换(选中态用主文字色,未选中用次要色)
class _ViewToggle extends StatelessWidget {
  const _ViewToggle({
    required this.gridView,
    required this.onChanged,
    required this.tokens,
  });

  final bool gridView;
  final ValueChanged<bool> onChanged;
  final AppPalette tokens;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        IconBtn(
          icon: Icons.grid_view_rounded,
          iconSize: 15,
          color: gridView ? tokens.foreground : tokens.mutedForeground,
          onTap: () => onChanged(true),
        ),
        const SizedBox(width: 4),
        IconBtn(
          icon: Icons.view_list_rounded,
          iconSize: 15,
          color: !gridView ? tokens.foreground : tokens.mutedForeground,
          onTap: () => onChanged(false),
        ),
      ],
    );
  }
}

/// 类型网格 / 列表渲染
class _TypeGrid extends StatelessWidget {
  const _TypeGrid({
    required this.items,
    required this.gridView,
    required this.selectedId,
    required this.onSelect,
    required this.onDoubleSelect,
  });

  final List<DbType> items;
  final bool gridView;
  final String? selectedId;
  final ValueChanged<DbType> onSelect;
  final ValueChanged<DbType> onDoubleSelect;

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    if (gridView) {
      return Wrap(
        spacing: 8,
        runSpacing: 14,
        children: [
          for (final type in items)
            SelectableCard(
              width: 120,
              selected: type.id == selectedId,
              disabled: !isEngineTypeSupported(type.id),
              disabledLabel: '未实现',
              selectedColor: t.accent,
              borderRadius: BorderRadius.circular(14),
              onSelect: () => onSelect(type),
              onDoubleTap: () => onDoubleSelect(type),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  DbTypeIcon(type: type, size: 80),
                  const SizedBox(height: 8),
                  for (final (i, line) in type.label.split('\n').indexed)
                    Text(
                      line,
                      textAlign: TextAlign.center,
                      style: TextStyle(
                        color: t.foreground,
                        fontSize: 12,
                        fontWeight: i == 0 ? FontWeight.w600 : FontWeight.w400,
                        height: 1.3,
                      ),
                    ),
                ],
              ),
            ),
        ],
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final type in items)
          ListItem(
            height: 36,
            selected: type.id == selectedId,
            selectedColor: t.accent.withValues(alpha: 0.15),
            enabled: isEngineTypeSupported(type.id),
            onSelect: () => onSelect(type),
            onDoubleTap: () => onDoubleSelect(type),
            leading: Opacity(
              opacity: isEngineTypeSupported(type.id) ? 1.0 : 0.35,
              child: DbTypeIcon(type: type, size: 24),
            ),
            title: type.label.replaceAll('\n', ' '),
            trailing: isEngineTypeSupported(type.id)
                ? (type.id == selectedId
                    ? Icon(Icons.check, color: t.accent, size: 18)
                    : null)
                : Text(
                    '未实现',
                    style: TextStyle(
                      fontSize: 11,
                      color: t.foreground.withValues(alpha: 0.45),
                    ),
                  ),
          ),
      ],
    );
  }
}
