import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';

import '../data/drivers/kv_driver.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';

/// 校验失败提示的红:与 `create_database_dialog.dart` / `query_page.dart` 同一
/// 中调色,主题无关(色板里没有语义错误色)。
const Color _kErrorColor = Color(0xffd93025);

/// 对话框宽度:一屏放得下「标签 + 控件」两列,再宽就是空的
const double _kDialogWidth = 420;

/// 表单标签列宽(标签右对齐,控件左端对齐)
const double _kLabelWidth = 92;

/// 行内控件高度:沿用键浏览页密度,不用 base-ui 默认的 28
const double _kFieldHeight = 26;

/// 一次提交带回的表单结果。
///
/// 为什么带明文而不是 [KvBytes]:十六进制输入(`HEX:` 前缀)要在**这里**先解析并
/// 就地报错(格式不对就拦在对话框里,不让用户点了确定才在页脚看见一句红字),
/// 校验用的正是解析结果。页面再解一次就丢了这里的判错口径。
class KvFormResult {
  const KvFormResult({
    required this.text,
    this.keyName,
    this.type,
    this.fieldText,
    this.score,
    this.atHead = false,
    this.ttlSeconds,
  });

  /// 值 / 成员 / 元素文本(可能带 `HEX:` 前缀)
  final String text;

  /// 仅「新建键」模式给出
  final String? keyName;

  /// 仅「新建键」模式给出
  final KvKeyType? type;

  /// 仅 hash:字段名
  final String? fieldText;

  /// 仅 zset:分数
  final double? score;

  /// 仅 list:true = LPUSH 头插,false = RPUSH 尾追
  final bool atHead;

  /// 仅「新建键」模式给出;null = 不设过期时间
  final int? ttlSeconds;
}

/// 「新建键」对话框。
///
/// 五种可写类型共用一张表单:值恒有,字段名(hash)/ 分数(zset)/ 插入位置(list)/
/// 过期时间按需出现。**初始内容对容器类型必填**:Redis 建不出空的
/// hash / list / set / zset(一条 HSET 都不发就没有这个键),
/// 收下空值只会得到「点了确定、列表里却没有这个键」。
///
/// [checkName] 在确定时被调用一次:返回 null = 键名可用,返回文本 = 画在键名行
/// 下方并留在原地。查重必须发生在写之前 —— 服务端对已存在的键不会拒绝
/// HSET / RPUSH,而是往里面追加,「新建」就偷偷变成了「改了一个旧键」。
Future<KvFormResult?> showKvNewKeyDialog(
  BuildContext context, {
  required KvKeyType initialType,
  required Future<String?> Function(String name) checkName,
}) =>
    showDialog<KvFormResult>(
      context: context,
      builder: (_) => _NewKeyDialog(initialType: initialType, checkName: checkName),
    );

class _NewKeyDialog extends StatefulWidget {
  const _NewKeyDialog({
    required this.initialType,
    required this.checkName,
  });

  final KvKeyType initialType;
  final Future<String?> Function(String name) checkName;

  @override
  State<_NewKeyDialog> createState() => _NewKeyDialogState();
}

class _NewKeyDialogState extends State<_NewKeyDialog> {
  final _name = TextEditingController();
  final _value = TextEditingController();
  final _field = TextEditingController();
  final _score = TextEditingController(text: '0');
  final _ttl = TextEditingController();
  final _nameFocus = FocusNode();

  late KvKeyType _type = widget.initialType;
  String _unit = 's';
  bool _atHead = false;

  /// 键名查重失败的原因;null = 无错。改键名即清掉,不留着挡下一次提交
  String? _nameError;

  /// 提交在途:期间关掉两个按钮,否则连点确定会发两遍写命令
  bool _busy = false;

  /// 是否已经点过一次确定(「必填但为空」的提示只在这之后出现)
  bool _showErrors = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _nameFocus.requestFocus();
    });
  }

  @override
  void dispose() {
    _name.dispose();
    _value.dispose();
    _field.dispose();
    _score.dispose();
    _ttl.dispose();
    _nameFocus.dispose();
    super.dispose();
  }

  bool get _needsField => _type == KvKeyType.hash;

  bool get _needsScore => _type == KvKeyType.zset;

  bool get _needsPosition => _type == KvKeyType.list;

  /// 值文本的十六进制前缀写法是否合法(见 [kvBytesFromInput])
  bool get _valueHexValid => kvBytesFromInput(_value.text) != null;

  double? get _scoreValue => double.tryParse(_score.text.trim());

  /// TTL 留空 = 不过期;填了就必须是大于 0 的整数
  int? get _ttlSeconds {
    final n = int.tryParse(_ttl.text.trim());
    if (n == null) return null;
    return n > 0 ? n * kvTtlUnitSeconds(_unit) : null;
  }

  bool get _ttlValid =>
      _ttl.text.trim().isEmpty ||
      (int.tryParse(_ttl.text.trim()) != null && _ttlSeconds != null);

  /// 「必填但为空」的两处只在点过确定之后才报(刚打开就两处红字太吵);
  /// 格式类错误(十六进制 / 分数 / 时长)实时报,因为那是用户正在改的那格
  bool get _nameMissing => _showErrors && _name.text.trim().isEmpty;

  bool get _valueMissing =>
      _showErrors && _type != KvKeyType.string && _value.text.isEmpty;

  /// 确定按钮**始终可点**:灰掉而不说原因,就等于让用户对着一扇没反应的门
  /// (AGENTS.md「不给点了没反应的控件」的反面 —— 给了反应,才谈得上说实话)
  bool get _formatValid =>
      _valueHexValid && _ttlValid && (!_needsScore || _scoreValue != null);

  Future<void> _submit() async {
    if (_busy) return;
    final l = context.l10n;
    if (!_formatValid || _name.text.trim().isEmpty || _valueMissing) {
      setState(() => _showErrors = true);
      return;
    }
    final name = _name.text.trim();
    setState(() {
      _busy = true;
      _nameError = null;
    });
    final taken = await widget.checkName(name);
    if (!mounted) return;
    if (taken != null) {
      setState(() {
        _busy = false;
        _nameError = taken.isEmpty ? l.kvKeyNameTaken : taken;
      });
      return;
    }
    Navigator.of(context).pop(KvFormResult(
      text: _value.text,
      keyName: name,
      type: _type,
      fieldText: _needsField ? _field.text : null,
      score: _needsScore ? _scoreValue : null,
      atHead: _atHead,
      ttlSeconds: _ttlSeconds,
    ));
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final fields = <Widget>[
      _row(
        l.kvColKeyName,
        Input(
          controller: _name,
          focusNode: _nameFocus,
          height: _kFieldHeight,
          onChanged: (_) => setState(() {
            _nameError = null;
            _showErrors = true;
          }),
          onSubmitted: (_) => _submit(),
          textInputAction: TextInputAction.next,
        ),
        error: _nameError ?? (_nameMissing ? l.kvKeyNameEmpty : null),
      ),
      const SizedBox(height: 10),
      _row(
        l.kvColType,
        ComboBox<KvKeyType>(
          // stream 不在列表里:建一个只能追加的键不是「新建键」的常见意图,
          // 而它在值网格里又是只读的,给了入口就是「能点但不能改」
          items: const [
            KvKeyType.string,
            KvKeyType.hash,
            KvKeyType.list,
            KvKeyType.set,
            KvKeyType.zset,
          ],
          value: _type,
          itemToString: (v) => kvTypeLabel(l, v),
          onChanged: (v) => setState(() => _type = v ?? _type),
        ),
      ),
      if (_needsField) ...[
        const SizedBox(height: 10),
        _row(l.kvColField, Input(controller: _field, height: _kFieldHeight)),
      ],
      if (_needsScore) ...[
        const SizedBox(height: 10),
        _row(
          l.kvColScore,
          Input(
            controller: _score,
            height: _kFieldHeight,
            onChanged: (_) => setState(() {}),
          ),
          error: _scoreInvalid(l, _score),
        ),
      ],
      if (_needsPosition) ...[
        const SizedBox(height: 10),
        _row(
          l.kvPosition,
          ComboBox<bool>(
            items: const [false, true],
            value: _atHead,
            itemToString: (v) => v ? l.kvPositionHead : l.kvPositionTail,
            onChanged: (v) => setState(() => _atHead = v ?? false),
          ),
        ),
      ],
      const SizedBox(height: 10),
      _row(
        l.kvColValue,
        Input(
          controller: _value,
          height: _kFieldHeight,
          onChanged: (_) => setState(() {}),
        ),
        description: l.kvHexHint,
        error: _valueMissing
            ? l.kvValueRequired
            : (_valueHexValid ? null : l.kvHexInvalid),
      ),
      const SizedBox(height: 10),
      _row(
        l.kvFieldTtl,
        _ttlEditor(l),
        description: l.kvTtlHint,
        error: _ttlValid ? null : l.kvTtlInvalid,
      ),
    ];
    return DialogBox(
      title: l.kvNewKeyTitle,
      width: _kDialogWidth,
      onClose: () => Navigator.of(context).pop(),
      footer: _footer(
        context,
        okLabel: l.btnSave,
        okEnabled: !_busy,
        cancelEnabled: !_busy,
        onOk: _submit,
      ),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 18, 20, 18),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: fields,
        ),
      ),
    );
  }

  /// 「数字 + 单位」的时长编辑格。与 [showKvTtlDialog] 同口径,
  /// 共用换算([kvTtlUnitSeconds])与标签([kvTtlUnitLabel]),行本身两处各摆一次:
  /// 抽成组件就成了 daro 里禁止的自造 UI 组件。
  Widget _ttlEditor(AppLocalizations l) => Row(
        children: [
          Expanded(
            child: Input(
              controller: _ttl,
              height: _kFieldHeight,
              onChanged: (_) => setState(() {}),
            ),
          ),
          const SizedBox(width: 6),
          SizedBox(
            width: 92,
            child: ComboBox<String>(
              items: const ['s', 'm', 'h', 'd'],
              value: _unit,
              itemToString: (v) => kvTtlUnitLabel(l, v),
              onChanged: (v) => setState(() => _unit = v ?? _unit),
            ),
          ),
        ],
      );
}

/// 「添加内容」对话框:往已存在的键里加一项(hash 字段 / list 元素 / set 或 zset 成员)。
Future<KvFormResult?> showKvAddItemDialog(
  BuildContext context, {
  required KvKeyType type,
  required String keyName,
}) {
  final l = context.l10n;
  final title = switch (type) {
    KvKeyType.hash => l.kvAddFieldTitle,
    KvKeyType.list => l.kvAddElementTitle,
    _ => l.kvAddMemberTitle,
  };
  return showDialog<KvFormResult>(
    context: context,
    builder: (_) => _AddItemDialog(type: type, keyName: keyName, title: title),
  );
}

class _AddItemDialog extends StatefulWidget {
  const _AddItemDialog({
    required this.type,
    required this.keyName,
    required this.title,
  });

  final KvKeyType type;
  final String keyName;
  final String title;

  @override
  State<_AddItemDialog> createState() => _AddItemDialogState();
}

class _AddItemDialogState extends State<_AddItemDialog> {
  final _value = TextEditingController();
  final _field = TextEditingController();
  final _score = TextEditingController(text: '0');
  final _valueFocus = FocusNode();

  bool _atHead = false;

  /// 同「新建键」:点过确定之后才报「值为空」
  bool _showErrors = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _valueFocus.requestFocus();
    });
  }

  @override
  void dispose() {
    _value.dispose();
    _field.dispose();
    _score.dispose();
    _valueFocus.dispose();
    super.dispose();
  }

  bool get _needsField => widget.type == KvKeyType.hash;

  bool get _needsScore => widget.type == KvKeyType.zset;

  bool get _needsPosition => widget.type == KvKeyType.list;

  double? get _scoreValue => double.tryParse(_score.text.trim());

  bool get _valueMissing => _showErrors && _value.text.isEmpty;

  bool get _formatValid =>
      kvBytesFromInput(_value.text) != null && (!_needsScore || _scoreValue != null);

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final fields = <Widget>[
      if (_needsField) ...[
        _row(l.kvColField, Input(controller: _field, height: _kFieldHeight)),
        const SizedBox(height: 10),
      ],
      if (_needsScore) ...[
        _row(
          l.kvColScore,
          Input(
            controller: _score,
            height: _kFieldHeight,
            onChanged: (_) => setState(() {}),
          ),
          error: _scoreInvalid(l, _score),
        ),
        const SizedBox(height: 10),
      ],
      if (_needsPosition) ...[
        _row(
          l.kvPosition,
          ComboBox<bool>(
            items: const [false, true],
            value: _atHead,
            itemToString: (v) => v ? l.kvPositionHead : l.kvPositionTail,
            onChanged: (v) => setState(() => _atHead = v ?? false),
          ),
        ),
        const SizedBox(height: 10),
      ],
      _row(
        l.kvColValue,
        Input(
          controller: _value,
          focusNode: _valueFocus,
          height: _kFieldHeight,
          onChanged: (_) => setState(() {}),
          onSubmitted: (_) => _submit(),
          textInputAction: TextInputAction.done,
        ),
        description: l.kvHexHint,
        error: _valueMissing
            ? l.kvValueRequired
            : (kvBytesFromInput(_value.text) == null ? l.kvHexInvalid : null),
      ),
    ];
    return DialogBox(
      title: '${widget.title} · ${widget.keyName}',
      width: _kDialogWidth,
      onClose: () => Navigator.of(context).pop(),
      footer: _footer(
        context,
        okLabel: l.btnSave,
        okEnabled: true,
        cancelEnabled: true,
        onOk: _submit,
      ),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 18, 20, 18),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: fields,
        ),
      ),
    );
  }

  void _submit() {
    if (!_formatValid || _value.text.isEmpty) {
      setState(() => _showErrors = true);
      return;
    }
    Navigator.of(context).pop(KvFormResult(
      text: _value.text,
      fieldText: _needsField ? _field.text : null,
      score: _needsScore ? _scoreValue : null,
      atHead: _atHead,
    ));
  }
}

/// 「设置过期时间」对话框:数值 + 单位,确定回剩余秒数;取消回 null。
///
/// 只做相对时长,不做「过期于某年某月某日」:`EXPIRE` 收的就是秒,
/// 而界面算出的绝对时刻会随本地时钟与在途延迟漂移,反而比倒计时容易读错。
Future<int?> showKvTtlDialog(BuildContext context, {int? initialSeconds}) =>
    showDialog<int>(
      context: context,
      builder: (_) => _TtlDialog(initialSeconds: initialSeconds),
    );

class _TtlDialog extends StatefulWidget {
  const _TtlDialog({this.initialSeconds});

  final int? initialSeconds;

  @override
  State<_TtlDialog> createState() => _TtlDialogState();
}

class _TtlDialogState extends State<_TtlDialog> {
  final _amount = TextEditingController();
  final _amountFocus = FocusNode();
  String _unit = 's';

  @override
  void initState() {
    super.initState();
    // 回填值按最大整除单位显示:7200 要说成「2 小时」,而不是让用户对着一屏 7200 换算
    final secs = widget.initialSeconds;
    if (secs != null && secs > 0) {
      for (final unit in const ['d', 'h', 'm', 's']) {
        final mult = kvTtlUnitSeconds(unit);
        if (secs % mult == 0) {
          _unit = unit;
          _amount.text = '${secs ~/ mult}';
          break;
        }
      }
    }
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _amountFocus.requestFocus();
    });
  }

  @override
  void dispose() {
    _amount.dispose();
    _amountFocus.dispose();
    super.dispose();
  }

  int? get _seconds {
    final n = int.tryParse(_amount.text.trim());
    if (n == null || n <= 0) return null;
    return n * kvTtlUnitSeconds(_unit);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return DialogBox(
      title: l.kvTtlSetTitle,
      width: _kDialogWidth,
      onClose: () => Navigator.of(context).pop(),
      footer: _footer(
        context,
        okLabel: l.btnOk,
        okEnabled: _seconds != null,
        cancelEnabled: true,
        onOk: () {
          final seconds = _seconds;
          if (seconds != null) Navigator.of(context).pop(seconds);
        },
      ),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 18, 20, 18),
        child: FieldRow(
          label: '${l.kvFieldTtl}:',
          labelWidth: _kLabelWidth,
          child: Row(
            children: [
              Expanded(
                child: Input(
                  controller: _amount,
                  focusNode: _amountFocus,
                  height: _kFieldHeight,
                  onChanged: (_) => setState(() {}),
                  onSubmitted: (_) {},
                ),
              ),
              const SizedBox(width: 6),
              SizedBox(
                width: 92,
                child: ComboBox<String>(
                  items: const ['s', 'm', 'h', 'd'],
                  value: _unit,
                  itemToString: (v) => kvTtlUnitLabel(l, v),
                  onChanged: (v) => setState(() => _unit = v ?? _unit),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

// ───────────────────────────────────────────────── 文件内共用的小件

/// 一行「右对齐标签 + 铺满剩余宽度的控件」,下方可选错误行 / 说明行。
Widget _row(String label, Widget control, {String? error, String? description}) =>
    Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        FieldRow(
          label: '$label:',
          labelWidth: _kLabelWidth,
          child: SizedBox(width: double.infinity, child: control),
        ),
        // 错误优先于说明:两者同位置,写失败的原文不能被提示语盖掉
        if (error != null || description != null)
          Padding(
            padding: const EdgeInsets.only(top: 4, left: _kLabelWidth + 10),
            child: Text(
              error ?? description!,
              style: TextStyle(
                fontSize: 11.5,
                color: error != null ? _kErrorColor : null,
                decoration: TextDecoration.none,
                fontWeight: FontWeight.w400,
              ),
            ),
          ),
      ],
    );

/// 底部动作条:确定(强调色边框)/ 取消,右对齐。
Widget _footer(
  BuildContext context, {
  required String okLabel,
  required bool okEnabled,
  required bool cancelEnabled,
  required VoidCallback onOk,
}) {
  final t = Tokens.of(context);
  final l = context.l10n;
  return Row(
    mainAxisAlignment: MainAxisAlignment.end,
    children: [
      Button(
        text: okLabel,
        // base-ui 尚无「默认按钮」变体,用令牌覆盖出白底 + 强调色边框
        tokens: t.desktopTokensFor(context).copyWith(buttonBorderColor: t.accent),
        onPressed: okEnabled ? onOk : null,
      ),
      const SizedBox(width: 8),
      Button(
        text: l.btnCancel,
        onPressed: cancelEnabled ? () => Navigator.of(context).pop() : null,
      ),
    ],
  );
}

String? _scoreInvalid(AppLocalizations l, TextEditingController controller) {
  final text = controller.text.trim();
  if (text.isEmpty || double.tryParse(text) != null) return null;
  return l.kvScoreInvalid;
}

/// 单位 id → 秒数。`s` / `m` / `h` / `d` 是内部 ID,不进翻译。
int kvTtlUnitSeconds(String unit) => switch (unit) {
      'm' => 60,
      'h' => 3600,
      'd' => 86400,
      _ => 1,
    };

String kvTtlUnitLabel(AppLocalizations l, String unit) => switch (unit) {
      'm' => l.kvTtlUnitMinutes,
      'h' => l.kvTtlUnitHours,
      'd' => l.kvTtlUnitDays,
      _ => l.kvTtlUnitSeconds,
    };

/// 容器类型的中文名。键浏览页与对话框都要用,而 `KvBrowserPage` 里的同名私有
/// 方法不能跨文件引用,所以提到这里由两边共用。
String kvTypeLabel(AppLocalizations l, KvKeyType? type) => switch (type) {
      null => '--',
      KvKeyType.string => l.kvTypeString,
      KvKeyType.list => l.kvTypeList,
      KvKeyType.set => l.kvTypeSet,
      KvKeyType.zset => l.kvTypeZset,
      KvKeyType.hash => l.kvTypeHash,
      KvKeyType.stream => l.kvTypeStream,
      KvKeyType.none => l.kvTypeNone,
      KvKeyType.unknown => l.kvTypeUnknown,
    };
