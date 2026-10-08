import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../app/app_state.dart';
import '../data/routine_sql.dart';
import '../theme/app_theme.dart';

/// 函数向导完成结果(由 [FunctionWizardDialog] 返回)
class RoutineWizardResult {
  const RoutineWizardResult({
    required this.category,
    required this.name,
    required this.params,
    required this.options,
    this.returnType = '',
  });

  /// 例程分类:过程 / 函数
  final ObjectCategory category;

  /// 例程名称
  final String name;

  /// 参数列表(向导第 2 步的网格)
  final List<RoutineParam> params;

  /// 选项(向导第 3 步:窗口 / 特性 / 防漏 / STRICT / 安全性)
  final RoutineOptions options;

  /// 返回类型(仅函数;含类型模式前缀,如 `pg_catalog."char"`)
  final String returnType;
}

/// 新建函数 / 过程的统一入口。
///
/// 默认弹出三步向导(例程类型 → 参数 → 选项),完成后打开对应分类的
/// 例程设计页(新建模式);取消勾选「下次显示向导」后本会话内直接打开
/// 设计页(使用自增默认名)。
Future<void> showFunctionWizard(
  BuildContext context, {
  required AppState app,
  required String connection,
  required String database,
  String? schema,
  ObjectCategory initialCategory = ObjectCategory.function,
}) async {
  final conn = app.connectionByName(connection);
  final typeId = conn?.typeId ?? '';

  RoutineWizardResult result;
  if (!FunctionWizardDialog.showOnNextCreate) {
    // 跳过向导:使用自增默认名直接打开设计页
    result = RoutineWizardResult(
      category: initialCategory,
      name: _defaultRoutineName(initialCategory),
      params: const [],
      options: const RoutineOptions(),
    );
  } else {
    final picked = await showDialog<RoutineWizardResult>(
      context: context,
      builder: (_) => FunctionWizardDialog(
        connection: connection,
        database: database,
        typeId: typeId,
        schema: schema,
        initialCategory: initialCategory,
      ),
    );
    if (picked == null) return; // 用户取消
    result = picked;
  }

  // 同步对象浏览上下文到新建的分类,并打开新建设计页
  app.setObjectContext(connection, database,
      category: result.category, schema: schema);
  app.designRoutine(
    result.name,
    connection: connection,
    database: database,
    category: result.category,
    schema: schema,
    isNew: true,
    params: RoutineSql.signature(typeId, result.params),
    options: result.options,
    returnType: result.returnType,
  );
}

int _routineNameCounter = 0;

String _defaultRoutineName(ObjectCategory category) {
  _routineNameCounter++;
  return category == ObjectCategory.procedure
      ? 'procedure_$_routineNameCounter'
      : 'function_$_routineNameCounter';
}

/// 「函数向导」三步对话框(复刻参考工具的向导):
/// 第 2 步 参数网格(+ 函数的返回类型)、第 3 步 选项(PG:窗口 / 特性 /
/// 防漏 / 输入 NULL 时返回 NULL / 安全性;其它方言只有安全性)。
/// 底部为 上一步 / 下一步 / 完成 / 取消,左下角「下次显示向导」复选。
class FunctionWizardDialog extends StatefulWidget {
  const FunctionWizardDialog({
    super.key,
    required this.connection,
    required this.database,
    required this.typeId,
    this.schema,
    this.initialCategory = ObjectCategory.function,
  });

  /// 所属连接名(仅用于对话框副标题展示)
  final String connection;

  /// 所属数据库
  final String database;

  /// 数据库类型 id(决定参数模式 / 类型候选与模板方言)
  final String typeId;

  /// 目标模式(决定 PG「类型模式」列的候选)
  final String? schema;

  /// 初始选中的例程类型(入口按钮决定,用户可改)
  final ObjectCategory initialCategory;

  /// 「下次显示向导」勾选态:勾选 = 下次仍弹向导(默认),取消 = 本会话内
  /// 新建入口跳过向导直接打开设计页。(未持久化,重启后恢复默认)
  static bool showOnNextCreate = true;

  @override
  State<FunctionWizardDialog> createState() => _FunctionWizardDialogState();
}

class _FunctionWizardDialogState extends State<FunctionWizardDialog> {
  /// 三步:类型 → 参数 → 选项
  static const int _stepCount = 3;

  /// 对话框尺寸(参考工具的向导窗口固定大小,切步不变)
  static const double _kDialogW = 900;
  static const double _kDialogH = 560;

  /// 标签列宽(标签左对齐,与标题同一起线)
  static const double _kLabelW = 190;

  /// 第 1 / 3 步的字段宽度
  static const double _kFieldW = 420;

  /// 参数网格行高
  static const double _kRowH = 26;

  final TextEditingController _nameController = TextEditingController();
  final FocusNode _nameFocus = FocusNode();

  /// 网格的键盘导航(方向键 / Enter / Esc)
  final FocusNode _gridFocus = FocusNode();

  /// 当前单元格编辑器共用的焦点(同一时刻只有一个格在编辑)
  final FocusNode _editorFocus = FocusNode();

  /// 当前步骤(0 = 例程类型,1 = 参数,2 = 选项)
  int _step = 0;

  /// 选中的例程类型(默认随入口,用户可切换)
  ObjectCategory _category = ObjectCategory.function;

  /// 参数列表(第 2 步的网格编辑)
  final List<RoutineParam> _params = [];

  /// 网格当前格 / 正在编辑的格
  (int, int)? _curCell;
  (int, int)? _editingCell;

  /// 「下次显示向导」勾选态
  bool _showNext = true;

  /// 第 3 步的选项(窗口 / 特性 / 防漏 / STRICT 只有 PG 有对应子句)
  bool _window = false;
  String _volatility = '';
  bool _leakProof = false;
  bool _strict = false;
  String _security = 'INVOKER';

  /// 第 2 步的返回类型(仅函数;类型模式 + 类型两列拼成 RETURNS 后的原文)
  String _returnTypeSchema = '';
  String _returnType = '';

  /// 只有 PG 的 CREATE FUNCTION 接受这四个子句(见 [RoutineOptions])
  bool get _pgOptions => RoutineSql.isPgLike(widget.typeId);

  bool get _isFunction => _category == ObjectCategory.function;

  /// RETURNS 后面的类型原文(引号规则见 [RoutineSql.qualifiedType],
  /// 与参考工具的 `pg_catalog."char"` 一致)
  String get _returnTypeText => RoutineSql.qualifiedType(widget.typeId,
      schema: _returnTypeSchema, type: _returnType);

  List<String> get _modes => RoutineSql.modesOf(widget.typeId);

  bool get _typeSchemaEnabled =>
      RoutineSql.supportsTypeSchema(widget.typeId);

  bool get _defaultEnabled =>
      RoutineSql.supportsParamDefault(widget.typeId);

  @override
  void initState() {
    super.initState();
    _category = widget.initialCategory;
    // 类型不支持过程时回退到函数
    if (_category == ObjectCategory.procedure &&
        !RoutineSql.supportsProcedure(widget.typeId)) {
      _category = ObjectCategory.function;
    }
    _showNext = FunctionWizardDialog.showOnNextCreate;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _nameFocus.requestFocus();
    });
  }

  @override
  void dispose() {
    _nameController.dispose();
    _nameFocus.dispose();
    _gridFocus.dispose();
    _editorFocus.dispose();
    super.dispose();
  }

  String get _name => _nameController.text.trim();

  bool get _canFinish => _name.isNotEmpty;

  void _finish() {
    if (!_canFinish) return;
    FunctionWizardDialog.showOnNextCreate = _showNext;
    Navigator.of(context).pop(RoutineWizardResult(
      category: _category,
      name: _name,
      params: List<RoutineParam>.from(_params),
      options: RoutineOptions(
        window: _window,
        volatility: _volatility,
        leakProof: _leakProof,
        strict: _strict,
        securityDefiner: _security == 'DEFINER',
      ),
      returnType: _isFunction ? _returnTypeText : '',
    ));
  }

  void _goStep(int step) {
    setState(() {
      _step = step.clamp(0, _stepCount - 1);
      _editingCell = null;
      // 首次进入参数页时预置一行空参数(与参考工具一致)
      if (_step == 1 && _params.isEmpty) {
        _params.add(RoutineParam(mode: _modes.first));
        _curCell = (0, 0);
      }
    });
    if (_step == 1) _gridFocus.requestFocus();
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    return DialogBox(
      title: '函数向导',
      width: _kDialogW,
      height: _kDialogH,
      onClose: () => Navigator.of(context).pop(),
      footer: Row(
        children: [
          CheckBox(
            value: _showNext,
            onChanged: (v) => setState(() => _showNext = v ?? true),
            label: '下次显示向导',
          ),
          const Spacer(),
          SizedBox(
            width: 92,
            child: Button(
              text: '< 上一步',
              onPressed: _step > 0 ? () => _goStep(_step - 1) : null,
            ),
          ),
          const SizedBox(width: 8),
          SizedBox(
            width: 92,
            child: Button(
              text: '下一步 >',
              onPressed:
                  _step < _stepCount - 1 ? () => _goStep(_step + 1) : null,
            ),
          ),
          const SizedBox(width: 8),
          SizedBox(
            width: 92,
            child: Button(text: '完成', onPressed: _canFinish ? _finish : null),
          ),
          const SizedBox(width: 8),
          SizedBox(
            width: 92,
            child: Button(
              text: '取消',
              onPressed: () => Navigator.of(context).pop(),
            ),
          ),
        ],
      ),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(28, 26, 28, 10),
        child: switch (_step) {
          0 => _stepType(t),
          1 => _stepParams(t),
          _ => _stepOptions(t),
        },
      ),
    );
  }

  /// 步骤标题(参考工具用强调色的大号文字,而不是普通 Label)
  Widget _heading(AppPalette t, String text) => Text(
        text,
        style: TextStyle(
          fontSize: 15,
          color: t.accent,
          fontWeight: FontWeight.w600,
          decoration: TextDecoration.none,
        ),
      );

  /// 标签列:左对齐(参考工具的标签与标题同一起线,不是右对齐)
  Widget _label(AppPalette t, String text) =>
      SizedBox(width: _kLabelW, child: Label(text));

  // ── 第 1 步:例程类型 ────────────────────────────────────

  Widget _stepType(AppPalette t) {
    final canProcedure = RoutineSql.supportsProcedure(widget.typeId);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _heading(t, '请选择你要创建的例程类型'),
        const SizedBox(height: 40),
        Row(
          children: [
            _label(t, '名称:'),
            SizedBox(
              width: _kFieldW,
              child: Input(
                controller: _nameController,
                focusNode: _nameFocus,
                onChanged: (_) => setState(() {}),
                onSubmitted: (_) => _goStep(1),
                textInputAction: TextInputAction.next,
              ),
            ),
          ],
        ),
        const SizedBox(height: 10),
        // 单选按钮与标签同列起线、竖向排列(参考工具即如此)
        Padding(
          padding: const EdgeInsets.only(left: 4),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              RadioButton<ObjectCategory>(
                value: ObjectCategory.procedure,
                groupValue: _category,
                enabled: canProcedure,
                onChanged: (v) {
                  if (v != null) setState(() => _category = v);
                },
                label: '过程',
              ),
              const SizedBox(height: 6),
              RadioButton<ObjectCategory>(
                value: ObjectCategory.function,
                groupValue: _category,
                onChanged: (v) {
                  if (v != null) setState(() => _category = v);
                },
                label: '函数',
              ),
            ],
          ),
        ),
        const SizedBox(height: 10),
        Padding(
          padding: const EdgeInsets.only(left: _kLabelW + 4),
          child: Text(
            canProcedure
                ? '创建位置:${widget.connection} · ${widget.database}'
                : '当前数据库类型(${widget.typeId})不支持存储过程,已选函数',
            style: TextStyle(fontSize: 11.5, color: t.mutedForeground),
          ),
        ),
      ],
    );
  }

  // ── 第 2 步:参数网格 ────────────────────────────────────

  Widget _stepParams(AppPalette t) {
    // 网格按内容取高(参考工具不会随一行参数就撑满窗口);上限是可用高度,
    // 参数再多也只到网格内部滚动,不会把底部的按钮行顶出去。
    final gridH = _kRowH * (1 + _params.length).clamp(1, 11);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _heading(t, '请输入这个例程的参数'),
        const SizedBox(height: 40),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: _label(t, '参数:'),
            ),
            Expanded(
              child: SizedBox(
                height: gridH,
                child: Focus(
                  focusNode: _gridFocus,
                  onKeyEvent: _onGridKey,
                  child: DataGridView(
                    columns: const [
                      DataGridViewColumn(title: '名称', flex: 5),
                      DataGridViewColumn(title: '类型模式', flex: 5),
                      DataGridViewColumn(title: '类型', flex: 5),
                      DataGridViewColumn(title: '模式', flex: 3),
                      DataGridViewColumn(title: '默认值', flex: 3),
                    ],
                    rowCount: _params.length,
                    rowHeight: _kRowH,
                    showRowNumbers: true,
                    rowNumberWidth: 22,
                    selectedRow: _curCell?.$1,
                    selectedCell: _curCell,
                    editingCell: _editingCell,
                    cellBuilder: _buildCell,
                    onCellSelected: (row, col) {
                      // 单元格编辑器的按下会被外层格子的 Listener 一并收到
                      // (选中走 onPointerDown 是零延迟的代价)。点的就是正在
                      // 编辑的那一格时必须原样放过,否则编辑器当场被拆掉:
                      // 下拉打不开、输入框里点一下定位光标就丢编辑态。
                      if (_editingCell == (row, col)) return;
                      setState(() {
                        _curCell = (row, col);
                        _editingCell = null;
                      });
                    },
                    onCellDoubleTap: _startEdit,
                    rowNumberBuilder: (row, selected) => selected
                        ? Container(width: 8, height: 10, color: t.accent)
                        : const SizedBox(width: 8, height: 10),
                  ),
                ),
              ),
            ),
          ],
        ),
        // 增删/上下移动与返回类型都钉在内容区底部(参考工具即如此),
        // 中间留白随参数行数伸缩。
        const Spacer(),
        Row(
          children: [
            _gridButton(t, Icons.add, '添加参数', true, _addParam),
            const SizedBox(width: 4),
            _gridButton(t, Icons.remove, '删除参数', _curCell != null, _removeParam),
            const SizedBox(width: 4),
            _gridButton(t, Icons.arrow_upward, '上移参数',
                _curCell != null && _curCell!.$1 > 0, () => _moveParam(-1)),
            const SizedBox(width: 4),
            _gridButton(
                t,
                Icons.arrow_downward,
                '下移参数',
                _curCell != null && _curCell!.$1 < _params.length - 1,
                () => _moveParam(1)),
          ],
        ),
        // 过程没有 RETURNS 子句,这一行就不出现(避免「改了不生成」)
        if (_isFunction) ...[
          const SizedBox(height: 14),
          Row(
            children: [
              _label(t, '返回类型:'),
              // 类型模式前缀只有 PG / SQL Server 的 RETURNS 认(MySQL 没有
              // 类型命名空间),不给「改了不生效」的入口
              if (_typeSchemaEnabled) ...[
                SizedBox(
                  width: 200,
                  child: ComboBox<String>(
                    items: RoutineSql.typeSchemasOf(widget.typeId,
                        schema: widget.schema),
                    value: _returnTypeSchema,
                    itemToString: (v) => v.isEmpty ? '(无)' : v,
                    onChanged: (v) =>
                        setState(() => _returnTypeSchema = v ?? ''),
                  ),
                ),
                const SizedBox(width: 10),
              ],
              SizedBox(
                width: 200,
                child: ComboBox<String>(
                  items: RoutineSql.typesOf(widget.typeId),
                  value: _returnType.isEmpty ? null : _returnType,
                  editable: true,
                  itemToString: (v) => v,
                  onChanged: (v) => setState(() => _returnType = v ?? ''),
                  onTextChanged: (v) => setState(() => _returnType = v),
                ),
              ),
            ],
          ),
        ],
        const SizedBox(height: 8),
      ],
    );
  }

  Widget _gridButton(AppPalette t, IconData icon, String tooltip, bool enabled,
          VoidCallback onTap) =>
      IconBtn(
        icon: icon,
        iconSize: 14,
        color: enabled ? t.foreground : t.disabledForeground,
        tooltip: tooltip,
        outline: true,
        size: const Size(26, 26),
        onTap: enabled ? onTap : null,
      );

  void _addParam() {
    final at = _curCell == null ? _params.length : _curCell!.$1 + 1;
    setState(() {
      _params.insert(at, RoutineParam(mode: _modes.first));
      _curCell = (at, 0);
      _editingCell = null;
    });
    _startEdit(at, 0);
  }

  void _removeParam() {
    final row = _curCell?.$1;
    if (row == null) return;
    setState(() {
      _params.removeAt(row);
      _curCell = _params.isEmpty ? null : (row.clamp(0, _params.length - 1), 0);
      _editingCell = null;
    });
  }

  void _moveParam(int delta) {
    final row = _curCell?.$1;
    if (row == null) return;
    final to = row + delta;
    if (to < 0 || to >= _params.length) return;
    setState(() {
      final moved = _params.removeAt(row);
      _params.insert(to, moved);
      _curCell = (to, _curCell!.$2);
      _editingCell = null;
    });
  }

  KeyEventResult _onGridKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent && event is! KeyRepeatEvent) {
      return KeyEventResult.ignored;
    }
    final key = event.logicalKey;
    if (key == LogicalKeyboardKey.escape) {
      if (_editingCell == null) return KeyEventResult.ignored;
      _endEdit();
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.enter ||
        key == LogicalKeyboardKey.numpadEnter ||
        key == LogicalKeyboardKey.f2) {
      final c = _curCell;
      if (c == null) return KeyEventResult.ignored;
      _startEdit(c.$1, c.$2);
      return KeyEventResult.handled;
    }
    final isVertical = key == LogicalKeyboardKey.arrowUp ||
        key == LogicalKeyboardKey.arrowDown;
    final isHorizontal = key == LogicalKeyboardKey.arrowLeft ||
        key == LogicalKeyboardKey.arrowRight;
    if (!isVertical && !isHorizontal) return KeyEventResult.ignored;
    if (_curCell == null) return KeyEventResult.ignored;
    final delta = (isVertical && key == LogicalKeyboardKey.arrowDown) ||
            (isHorizontal && key == LogicalKeyboardKey.arrowRight)
        ? 1
        : -1;
    setState(() {
      var (row, col) = _curCell!;
      if (isVertical) {
        row = (row + delta).clamp(0, _params.length - 1);
      } else {
        col = (col + delta).clamp(0, 4);
      }
      _curCell = (row, col);
      _editingCell = null;
    });
    return KeyEventResult.handled;
  }

  void _startEdit(int row, int col) {
    if (!_cellEditable(col)) return;
    setState(() {
      _curCell = (row, col);
      _editingCell = (row, col);
    });
    // 编辑器是这一帧才挂上去的,下一帧才能抢焦点
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && _editingCell != null) _editorFocus.requestFocus();
    });
  }

  /// 退出编辑:把焦点交还网格,否则方向键导航就断了
  void _endEdit() {
    if (_editingCell == null) return;
    setState(() => _editingCell = null);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && _editingCell == null) _gridFocus.requestFocus();
    });
  }

  /// 方言用不上的列不给编辑入口(MySQL 无类型模式、且参数不支持默认值)
  bool _cellEditable(int col) => switch (col) {
        1 => _typeSchemaEnabled,
        4 => _defaultEnabled,
        _ => true,
      };

  String _cellText(RoutineParam p, int col) => switch (col) {
        0 => p.name,
        1 => p.typeSchema,
        2 => p.type,
        3 => p.mode,
        _ => p.defaultValue,
      };

  Widget _buildCell(int row, int col) {
    final t = Tokens.of(context);
    final p = _params[row];
    if (_editingCell == (row, col)) return _buildEditor(p, col, t);
    final raw = _cellText(p, col);
    final text = col == 3 ? RoutineSql.modeLabel(raw) : raw;
    final enabled = _cellEditable(col);
    return Text(
      text,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: TextStyle(
        fontSize: 12.5,
        color: enabled ? t.foreground : t.disabledForeground,
        decoration: TextDecoration.none,
      ),
    );
  }

  Widget _buildEditor(RoutineParam p, int col, AppPalette t) {
    switch (col) {
      case 0:
        return _CellInput(
          key: ValueKey('n$p'),
          focusNode: _editorFocus,
          initial: p.name,
          onCommit: (v) => p.name = v,
          onDone: _endEdit,
        );
      case 1:
        return ComboBox<String>(
          items: RoutineSql.typeSchemasOf(widget.typeId, schema: widget.schema),
          value: p.typeSchema,
          focusNode: _editorFocus,
          showBorder: false,
          itemToString: (v) => v.isEmpty ? '(无)' : v,
          onChanged: (v) {
            p.typeSchema = v ?? '';
            _endEdit();
          },
        );
      case 2:
        return ComboBox<String>(
          items: RoutineSql.typesOf(widget.typeId),
          value: p.type.isEmpty ? null : p.type,
          focusNode: _editorFocus,
          editable: true,
          showBorder: false,
          onChanged: (v) => p.type = v ?? '',
          onTextChanged: (v) => p.type = v,
        );
      case 3:
        return ComboBox<String>(
          items: _modes,
          value: p.mode,
          focusNode: _editorFocus,
          showBorder: false,
          itemToString: (v) => RoutineSql.modeLabel(v),
          onChanged: (v) {
            p.mode = v ?? p.mode;
            _endEdit();
          },
        );
      default:
        return _CellInput(
          key: ValueKey('d$p'),
          focusNode: _editorFocus,
          initial: p.defaultValue,
          onCommit: (v) => p.defaultValue = v,
          onDone: _endEdit,
        );
    }
  }

  // ── 第 3 步:选项 ────────────────────────────────────────

  Widget _stepOptions(AppPalette t) {
    // 窗口 / 特性 / 防漏 / STRICT 只有 PG 的 CREATE FUNCTION 认,其它方言
    // 只剩安全性一行(且顶到第一行位置,不留空位)。
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _heading(t, '请选择函数会有的选项'),
        const SizedBox(height: 40),
        if (_pgOptions) ...[
          CheckBox(
            value: _window,
            onChanged: (v) => setState(() => _window = v ?? false),
            label: '窗口',
          ),
          const SizedBox(height: 10),
          Row(
            children: [
              _label(t, '特性:'),
              SizedBox(
                width: 200,
                child: ComboBox<String>(
                  items: RoutineSql.volatilityOptions,
                  value: _volatility,
                  itemToString: RoutineSql.volatilityLabel,
                  onChanged: (v) => setState(() => _volatility = v ?? ''),
                ),
              ),
            ],
          ),
          const SizedBox(height: 10),
          CheckBox(
            value: _leakProof,
            onChanged: (v) => setState(() => _leakProof = v ?? false),
            label: '防漏',
          ),
          const SizedBox(height: 10),
          CheckBox(
            value: _strict,
            onChanged: (v) => setState(() => _strict = v ?? false),
            label: '输入 NULL 时返回 NULL',
          ),
          const SizedBox(height: 10),
        ],
        Row(
          children: [
            _label(t, '安全性:'),
            SizedBox(
              width: 200,
              child: ComboBox<String>(
                items: RoutineSql.securityOptions,
                value: _security,
                onChanged: (v) {
                  if (v != null) setState(() => _security = v);
                },
              ),
            ),
          ],
        ),
        const SizedBox(height: 16),
        Padding(
          padding: const EdgeInsets.only(left: 4),
          child: Text(
            _pgOptions
                ? 'INVOKER = 以调用者权限执行,DEFINER = 以定义者权限执行;'
                    '窗口 / 特性 / 防漏 / STRICT 会各自生成一条 CREATE FUNCTION 子句。'
                : 'INVOKER = 以调用者权限执行,DEFINER = 以定义者权限执行。',
            style: TextStyle(fontSize: 11.5, color: t.mutedForeground),
          ),
        ),
      ],
    );
  }
}

/// 网格单元格里的文本编辑器:自持 controller,焦点节点由宿主给(共用一个),
/// 提交(Enter / 失焦)时写回模型。
class _CellInput extends StatefulWidget {
  const _CellInput({
    super.key,
    required this.focusNode,
    required this.initial,
    required this.onCommit,
    required this.onDone,
  });

  final FocusNode focusNode;
  final String initial;
  final ValueChanged<String> onCommit;
  final VoidCallback onDone;

  @override
  State<_CellInput> createState() => _CellInputState();
}

class _CellInputState extends State<_CellInput> {
  late final TextEditingController _controller =
      TextEditingController(text: widget.initial);

  @override
  void initState() {
    super.initState();
    // 失焦即提交:单元格里没有「确定」按钮,不这样会丢字
    widget.focusNode.addListener(_onFocusChange);
  }

  void _onFocusChange() {
    if (!widget.focusNode.hasFocus) widget.onCommit(_controller.text);
  }

  @override
  void dispose() {
    widget.focusNode.removeListener(_onFocusChange);
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Input(
      controller: _controller,
      focusNode: widget.focusNode,
      showBorder: false,
      onChanged: widget.onCommit,
      onSubmitted: (v) {
        widget.onCommit(v);
        widget.onDone();
      },
      textInputAction: TextInputAction.done,
    );
  }
}
