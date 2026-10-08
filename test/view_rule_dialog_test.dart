import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:daro/widgets/view_rule_dialog.dart';

/// 「添加规则」对话框的语句生成回归:它不碰数据库,只回一条 CREATE RULE,
/// 所以每条分支(条件括号 / NOTHING / ON SELECT 恒 INSTEAD)都能离线断言。
void main() {
  String? popped;

  Widget harness() {
    popped = null;
    return MaterialApp(
      theme: ThemeData(brightness: Brightness.light),
      home: Material(
        child: Align(
          alignment: Alignment.topLeft,
          child: Builder(builder: (context) {
            return Button(
              text: 'open',
              onPressed: () async {
                popped = await showDialog<String>(
                  context: context,
                  builder: (_) =>
                      const ViewRuleDialog(target: '"public"."v_demo"'),
                );
              },
            );
          }),
        ),
      ),
    );
  }

  Future<void> open(WidgetTester tester) async {
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(harness());
    await tester.pumpAndSettle();
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
  }

  /// 预览条里的语句(未填完时是提示文字)
  String preview(WidgetTester tester) {
    final box = tester
        .widgetList<Container>(find.byType(Container))
        .map((c) => c.child)
        .whereType<Text>()
        .firstWhere(
            (tx) => (tx.data ?? '').startsWith('CREATE RULE'),
            orElse: () => throw StateError('预览条里没有 CREATE RULE:\n'
                '${tester.widgetList<Text>(find.byType(Text)).map((t) => t.data).join('\n')}'));
    return box.data!;
  }

  Future<void> fill(WidgetTester tester,
      {required String name, required String action, String? where}) async {
    await tester.enterText(find.byType(Input).at(0), name);
    if (where != null) await tester.enterText(find.byType(Input).at(1), where);
    await tester.enterText(find.byType(Textarea), action);
    await tester.pumpAndSettle();
  }

  Button addBtn(WidgetTester tester) =>
      tester.widget<Button>(find.widgetWithText(Button, '添加'));

  testWidgets('没填名称或动作时不给提交', (tester) async {
    await open(tester);
    expect(addBtn(tester).onPressed, isNull);

    await fill(tester, name: 'r_log', action: '');
    expect(addBtn(tester).onPressed, isNull, reason: '动作仍为空');

    await fill(tester, name: '', action: 'SELECT 1');
    expect(addBtn(tester).onPressed, isNull, reason: '名称仍为空');
  });

  testWidgets('条件 / 动作 / ALSO 按界面所填拼进 CREATE RULE', (tester) async {
    await open(tester);
    await fill(tester,
        name: 'r_log',
        where: 'NEW.id > 0',
        action: 'INSERT INTO log VALUES (NEW.id)');

    expect(
        preview(tester),
        'CREATE RULE "r_log" AS ON INSERT TO "public"."v_demo" '
        'WHERE (NEW.id > 0) DO ALSO (INSERT INTO log VALUES (NEW.id));');
    expect(addBtn(tester).onPressed, isNotNull);
  });

  testWidgets('动作写 NOTHING 时不能带括号(它不是查询)', (tester) async {
    await open(tester);
    await fill(tester, name: 'r_skip', action: 'NOTHING');
    expect(preview(tester),
        'CREATE RULE "r_skip" AS ON INSERT TO "public"."v_demo" DO ALSO NOTHING;');
  });

  testWidgets('自己括好的条件不再套一层括号', (tester) async {
    await open(tester);
    await fill(tester,
        name: 'r_log', where: '(NEW.id > 0)', action: 'SELECT 1');
    expect(preview(tester), contains('WHERE (NEW.id > 0) DO'));
  });

  testWidgets('勾代替运行 → DO INSTEAD;ON SELECT 恒为 INSTEAD 且复选框置灰',
      (tester) async {
    await open(tester);
    await fill(tester, name: 'r_log', action: 'SELECT 1');

    await tester.tap(find.byType(CheckBox));
    await tester.pumpAndSettle();
    expect(preview(tester), contains('DO INSTEAD (SELECT 1);'));

    // 切事件:ComboBox 选中即回调
    await tester.tap(find.byWidgetPredicate((w) => w is ComboBox<String>));
    await tester.pumpAndSettle();
    await tester.tap(find.text('SELECT').last);
    await tester.pumpAndSettle();

    expect(preview(tester), contains('AS ON SELECT'));
    expect(preview(tester), contains('DO INSTEAD'));
    expect(tester.widget<CheckBox>(find.byType(CheckBox)).value, isTrue);
    expect(tester.widget<CheckBox>(find.byType(CheckBox)).enabled, isFalse);
  });

  testWidgets('点「添加」把语句回给宿主,取消回 null', (tester) async {
    await open(tester);
    await fill(tester, name: 'r_log', action: 'SELECT 1');
    await tester.tap(find.text('添加'));
    await tester.pumpAndSettle();
    expect(popped,
        'CREATE RULE "r_log" AS ON INSERT TO "public"."v_demo" DO ALSO (SELECT 1);');

    await open(tester);
    await tester.tap(find.text('取消'));
    await tester.pumpAndSettle();
    expect(popped, isNull);
  });
}
