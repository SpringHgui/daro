import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/main.dart';
import 'package:daro/widgets/ribbon.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

import 'pin_system_locale.dart';

/// ribbon 上「焦点」与「选中」必须是两种看得出来的画法。
///
/// 症状（用户截图）：点过「新建查询」后，它和当前分类「表」同时呈现浅蓝底，
/// 像有两个按钮都被激活。根因不在 ribbon 的选中态，而在 ghost 按钮的聚焦底色
/// 与宿主自己铺的选中底色是同一族浅蓝（accent 14% 与 16% 叠 control）。
void main() {
  Future<void> pumpApp(WidgetTester tester) async {
    pinSystemChineseLocale(tester);
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ChangeNotifierProvider<AppState>.value(
        value: AppState(),
        child: const DbApp(),
      ),
    );
    await tester.pump();
  }

  Finder ribbonButtons() => find.descendant(
      of: find.byType(Ribbon), matching: find.byType(Button));

  Element buttonAt(WidgetTester tester, int i) =>
      ribbonButtons().evaluate().toList()[i];

  /// 从按钮往上找到第一个画了底色的 [Container]（ribbon 铺「选中底色」的那一层）。
  Container? wrapperOf(Element buttonElement) {
    Container? found;
    buttonElement.visitAncestorElements((e) {
      final w = e.widget;
      if (w is Container && (w.decoration as BoxDecoration?)?.color != null) {
        found = w;
        return false;
      }
      return e.widget is! Ribbon;
    });
    return found;
  }

  /// Button 自己绘制 hover / pressed / focus 的那层（子树里第一个带装饰的 Container）。
  Container? faceOf(Element buttonElement) {
    Container? found;
    void visit(Element e) {
      if (found != null) return;
      final w = e.widget;
      if (w is Container && w.decoration is BoxDecoration) {
        found = w;
        return;
      }
      e.visitChildren(visit);
    }

    visit(buttonElement);
    return found;
  }

  /// ribbon 选中底色非透明的按钮个数 = 视觉上「被激活」的按钮数
  int selectedCount(WidgetTester tester) => ribbonButtons()
      .evaluate()
      .where((b) {
        final c = (wrapperOf(b)?.decoration as BoxDecoration?)?.color;
        return (c ?? Colors.transparent).a > 0;
      })
      .length;

  testWidgets('点命令按钮后，选中底色仍只有一处，命令按钮只剩描边', (tester) async {
    await pumpApp(tester);
    expect(selectedCount(tester), 1, reason: '初始只有默认分类「表」是选中态');

    await tester.tap(ribbonButtons().at(1)); // 「新建查询」
    await tester.pump();

    expect(selectedCount(tester), 1,
        reason: '出现两处浅蓝底 = 聚焦态又和选中态撞色了');

    final d = faceOf(buttonAt(tester, 1))!.decoration as BoxDecoration;
    expect(d.color!.a, 0, reason: '聚焦不得铺底色，底色是选中态的表达');
    expect((d.border as Border).top.color.a, 1.0,
        reason: '聚焦仍要有描边，否则 Tab 到按钮上看不见落点');
  });

  testWidgets('切分类后高亮跟着走，不留下第二个', (tester) async {
    await pumpApp(tester);
    await tester.tap(ribbonButtons().at(1)); // 新建查询（焦点留在命令按钮上）
    await tester.pump();
    await tester.tap(ribbonButtons().at(3)); // 视图
    await tester.pump();

    expect(tester.takeException(), isNull);
    expect(selectedCount(tester), 1);
    final d = faceOf(buttonAt(tester, 3))!.decoration as BoxDecoration;
    expect(d.color!.a, 0,
        reason: '选中态由外层底色表达，按钮自身仍是 ghost');
  });
}
