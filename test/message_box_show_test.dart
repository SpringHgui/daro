import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// `MessageBox.show` 在 widget 测试里怎么才点得到按钮 —— 键浏览页的删除确认就靠它。
///
/// 两条实测出来的形状,写用例时踩一次就该记住:
/// 1. **要两帧**:`show()` 先插一层 entry,那层里的 `MessageBox` build 只回
///    `SizedBox.shrink()`;它 post-frame 里 `controller.open()`,`ModalOverlay`
///    才插入**第二层** entry 把面板渲染出来。只 `pump()` 一帧时面板是空的。
/// 2. **面板不是 `find.byType(MessageBox)` 的后代**:第二层 entry 挂在 root
///    Overlay 上,与外层 entry 同辈。拿 MessageBox 当查找范围永远命中 0 个。
void main() {
  Future<void> host(WidgetTester tester) async {
    await tester.pumpWidget(
      MaterialApp(home: Material(child: Builder(
        builder: (context) => Center(
          child: Button(
            text: '打开',
            onPressed: () => MessageBox.show(
              context,
              title: '删除',
              message: '确定删除吗?',
              type: MessageBoxType.question,
              buttons: MessageBoxButtons.okCancel,
              okText: '删除',
              cancelText: '取消',
            ),
          ),
        ),
      ))),
    );
  }

  testWidgets('确认框:一帧之后外层已挂载、面板还没渲染', (tester) async {
    await host(tester);
    await tester.tap(find.text('打开'));
    await tester.pump();
    expect(find.byType(MessageBox), findsOneWidget);
    expect(find.text('确定删除吗?'), findsNothing);
  });

  testWidgets('确认框:再推一帧就能按标题+动作键找到,点动作键即回 ok',
      (tester) async {
    await host(tester);
    await tester.tap(find.text('打开'));
    for (var i = 0; i < 4; i++) {
      await tester.pump(const Duration(milliseconds: 50));
    }
    expect(find.text('确定删除吗?'), findsOneWidget);
    // 标题与动作键同词:只有按 Button 找才点得准
    expect(find.text('删除'), findsNWidgets(2));
    final ok = find.widgetWithText(Button, '删除');
    expect(ok, findsOneWidget);
    await tester.tap(ok);
    for (var i = 0; i < 4; i++) {
      await tester.pump(const Duration(milliseconds: 50));
    }
    expect(find.text('确定删除吗?'), findsNothing);
  });
}
