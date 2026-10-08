import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import '../theme/app_theme.dart';

/// 「添加规则」对话框:填一条 PostgreSQL 视图规则的各部分,实时拼出
/// `CREATE RULE … AS ON … TO … [WHERE …] DO [ALSO|INSTEAD] …`。
///
/// 只在 PG 的连接上出现(规则是 PG 独有机制,其余方言的「规则」页签根本不显示)。
/// 对话框本身不碰数据库:「添加」成功后 `Navigator.pop` 回要执行的语句
/// (取消回 null),宿主负责执行、报错与刷新列表。
class ViewRuleDialog extends StatefulWidget {
  const ViewRuleDialog({
    super.key,
    required this.target,
  });

  /// 规则的宿主对象(已按 PG 形式引用,如 `"public"."v_demo"`)
  final String target;

  @override
  State<ViewRuleDialog> createState() => _ViewRuleDialogState();
}

class _ViewRuleDialogState extends State<ViewRuleDialog> {
  static const _events = ['SELECT', 'INSERT', 'UPDATE', 'DELETE'];

  final _name = TextEditingController();
  final _where = TextEditingController();
  final _action = TextEditingController();
  final _focusName = FocusNode();

  String _event = 'INSERT';
  bool _instead = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _focusName.requestFocus();
    });
  }

  @override
  void dispose() {
    _name.dispose();
    _where.dispose();
    _action.dispose();
    _focusName.dispose();
    super.dispose();
  }

  /// PG 只允许 `ON SELECT DO INSTEAD`(视图上的 SELECT 规则是替代查询本身),
  /// 选了 ALSO 会直接报错,所以这一档不给选。
  bool get _isSelect => _event == 'SELECT';

  String get _sql {
    final name = _dq(_name.text.trim());
    final buf = StringBuffer('CREATE RULE $name AS ON $_event '
        'TO ${widget.target} ');
    final where = _where.text.trim();
    if (where.isNotEmpty) {
      // 用户自己括好了就不再套一层,否则条件前会多出 `((`
      buf.write('WHERE ${where.startsWith('(') ? where : '($where)'} ');
    }
    final action = _action.text.trim();
    buf.write('DO ${(_isSelect || _instead) ? 'INSTEAD' : 'ALSO'} ');
    // NOTHING 是关键字不是查询,不能带括号
    buf.write(action.toUpperCase() == 'NOTHING'
        ? 'NOTHING'
        : '($action)');
    return '${buf.toString()};';
  }

  bool get _canSubmit =>
      _name.text.trim().isNotEmpty && _action.text.trim().isNotEmpty;

  String _dq(String v) => '"${v.replaceAll('"', '""')}"';

  void _submit() {
    if (!_canSubmit) return;
    Navigator.of(context).pop(_sql);
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    return DialogBox(
      title: '添加规则',
      width: 560,
      onClose: () => Navigator.of(context).pop(),
      footer: Row(
        mainAxisAlignment: MainAxisAlignment.end,
        children: [
          Button(text: '取消', onPressed: () => Navigator.of(context).pop()),
          const SizedBox(width: 8),
          Button(text: '添加', onPressed: _canSubmit ? _submit : null),
        ],
      ),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 16, 20, 4),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              '为视图 ${widget.target} 新建一条规则:',
              style: TextStyle(color: t.foreground, fontSize: 13),
            ),
            const SizedBox(height: 12),
            FieldRow(
              label: '名称:',
              child: Input(
                controller: _name,
                focusNode: _focusName,
                hint: '规则名称',
                onChanged: (_) => setState(() {}),
              ),
            ),
            const SizedBox(height: 8),
            FieldRow(
              label: '事件:',
              child: ComboBox<String>(
                items: _events,
                value: _event,
                // ON SELECT 只能配 DO INSTEAD,切到它时把 ALSO 一并纠正过来
                onChanged: (v) => setState(() {
                  _event = v ?? _event;
                  if (_event == 'SELECT') _instead = true;
                }),
              ),
            ),
            const SizedBox(height: 8),
            FieldRow(
              label: '条件:',
              child: Input(
                controller: _where,
                hint: '可选,如 NEW.id > 0(不含 WHERE 关键字)',
                onChanged: (_) => setState(() {}),
              ),
            ),
            const SizedBox(height: 8),
            FieldRow(
              label: '动作:',
              child: Textarea(
                controller: _action,
                minLines: 2,
                maxLines: 5,
                hint: '要执行的语句,如 INSERT INTO log VALUES (NEW.id);或 NOTHING',
                onChanged: (_) => setState(() {}),
              ),
            ),
            const SizedBox(height: 8),
            FieldRow(
              label: '',
              child: Row(
                children: [
                  CheckBox(
                    value: _isSelect || _instead,
                    // SELECT 规则恒为 INSTEAD:复选框置灰但不谎报状态
                    enabled: !_isSelect,
                    label: '代替运行(INSTEAD)',
                    onChanged: (v) => setState(() => _instead = v ?? false),
                  ),
                  if (_isSelect)
                    Expanded(
                      child: Text(
                        'ON SELECT 只能是 INSTEAD',
                        style: TextStyle(
                            fontSize: 11.5, color: t.mutedForeground),
                      ),
                    ),
                ],
              ),
            ),
            const SizedBox(height: 12),
            // 实时预览:界面填什么,库就执行什么(不给「看着会生成、其实没生成」)
            Container(
              width: double.infinity,
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
              decoration: BoxDecoration(
                color: t.background,
                border: Border.all(color: t.border),
              ),
              child: Text(
                _canSubmit ? _sql : '填写名称与动作后生成 CREATE RULE 语句',
                style: TextStyle(
                  fontFamily: 'Consolas',
                  fontFamilyFallback: const ['monospace'],
                  fontSize: 12,
                  color: t.mutedForeground,
                  decoration: TextDecoration.none,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
