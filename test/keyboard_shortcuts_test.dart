import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/main.dart';
import 'package:daro/widgets/query_page.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/widgets/sql_find_bar.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:re_editor/re_editor.dart';

import 'pin_system_locale.dart';

/// 带 Ctrl(可选 Shift)地按下某个键 —— `sendKeyDownUpSync` 不收修饰键,
/// 复合键只能自己按,顺带让 HardwareKeyboard 的修饰键状态真实起来。
///
/// 结尾多推 150ms:编辑器拿到焦点时 re_editor 会起一个 100ms 的
/// `Future.delayed`(闪标首帧),不排掉它,teardown 就报
/// 「A Timer is still pending even after the widget tree was disposed」。
Future<void> pressCtrl(
  WidgetTester tester,
  LogicalKeyboardKey key, {
  bool shift = false,
}) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
  if (shift) await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyEvent(key);
  if (shift) await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
  await tester.pump();
  await drainBlink(tester);
}

/// 排掉光标闪烁那个 100ms 延时(必须在编辑器还挂在树上时排,
/// 否则回调打到已销毁的 ValueNotifier 上)
Future<void> drainBlink(WidgetTester tester) =>
    tester.pump(const Duration(milliseconds: 150));

/// 收尾:排干闪烁延时再把焦点从编辑器里摘出来,然后让框架拆树。
Future<void> leaveApp(WidgetTester tester) async {
  await drainBlink(tester);
  FocusManager.instance.primaryFocus?.unfocus();
  await tester.pump();
}

void main() {
  Future<AppState> pumpApp(WidgetTester tester) async {
    pinSystemChineseLocale(tester);
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final app = AppState();
    await tester.pumpWidget(
      ChangeNotifierProvider<AppState>.value(
        value: app,
        child: const DbApp(),
      ),
    );
    await tester.pump();
    return app;
  }

  group('应用级快捷键', () {
    testWidgets('Ctrl+N 新建查询标签(启动后没点过任何东西也能按)', (tester) async {
      final app = await pumpApp(tester);
      expect(app.tabs, isEmpty);

      await pressCtrl(tester, LogicalKeyboardKey.keyN);

      expect(app.tabs.length, 1);
      expect(app.tabs.single.type, TabType.query);
      expect(app.activeTab, app.tabs.single.key);
      await leaveApp(tester);
    });

    testWidgets('Ctrl+Tab 循环、Ctrl+Shift+Tab 反向、Ctrl+W 关闭', (tester) async {
      final app = await pumpApp(tester);
      app.newQuery();
      final first = app.tabs.first.key;
      app.activeTab = AppState.objectsTabKey;
      await tester.pump();

      await pressCtrl(tester, LogicalKeyboardKey.tab);
      expect(app.activeTab, first, reason: '从首页往后一格应落到查询标签');

      await pressCtrl(tester, LogicalKeyboardKey.tab, shift: true);
      expect(app.activeTab, AppState.objectsTabKey);

      // 首页关不掉:Ctrl+W 应无副作用
      await pressCtrl(tester, LogicalKeyboardKey.keyW);
      expect(app.tabs.length, 1);

      await pressCtrl(tester, LogicalKeyboardKey.tab);
      await pressCtrl(tester, LogicalKeyboardKey.keyW);
      expect(app.tabs, isEmpty);
      expect(app.activeTab, AppState.objectsTabKey);
      await leaveApp(tester);
    });
  });

  group('SQL 编辑区查找 / 替换', () {
    CodeFindController findController(WidgetTester tester) =>
        tester.widget<SqlFindBar>(find.byType(SqlFindBar)).controller;

    testWidgets('Ctrl+F 展开查找栏并聚焦查找框', (tester) async {
      final app = await pumpApp(tester);
      app.newQuery();
      await tester.pump();
      expect(find.byType(QueryPage), findsOneWidget);

      await pressCtrl(tester, LogicalKeyboardKey.keyF);
      await tester.pump();

      final c = findController(tester);
      expect(c.value?.replaceMode, isFalse, reason: 'Ctrl+F 只开查找,不进替换');
      expect(c.findInputFocusNode.hasFocus, isTrue,
          reason: '查找框要拿到焦点,否则按下 Ctrl+F 还得再点一下才能输入');
      await leaveApp(tester);
    });

    testWidgets('Ctrl+H 展开替换栏(库默认是 Ctrl+Alt+F,已改绑)', (tester) async {
      final app = await pumpApp(tester);
      app.newQuery();
      await tester.pump();

      await pressCtrl(tester, LogicalKeyboardKey.keyH);
      await tester.pump();

      final c = findController(tester);
      expect(c.value?.replaceMode, isTrue);
      expect(find.byType(Input), findsNWidgets(2), reason: '替换态有两个输入框');
      // re_editor 的 replaceMode() 把光标交给替换框(查找框仍保留原词)
      expect(c.replaceInputFocusNode.hasFocus, isTrue);
      await leaveApp(tester);
    });

    testWidgets('Esc 关闭查找栏并把焦点还给编辑器', (tester) async {
      final app = await pumpApp(tester);
      app.newQuery();
      await tester.pump();
      await pressCtrl(tester, LogicalKeyboardKey.keyF);
      await tester.pump();
      final c = findController(tester);
      expect(c.value, isNotNull);

      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pump();

      expect(c.value, isNull, reason: 'Esc 关掉查找栏');
      final editor = tester
          .widget<CodeEditor>(find.byType(CodeEditor))
          .focusNode;
      expect(editor?.hasFocus, isTrue, reason: '关掉后光标回到编辑区');
      await leaveApp(tester);
    });
  });

  group('搜索框快捷键', () {
    const conn = ConnectionInfo(
      name: '本机库',
      typeId: 'mysql',
      host: 'h',
      port: '3306',
      username: 'u',
      password: 'p',
      isLive: false,
    );

    testWidgets('左侧栏:Ctrl+F 进底部搜索框,Esc 清空筛选并把焦点交回树',
        (tester) async {
      final app = await pumpApp(tester);
      app.addConnections([conn]);
      await tester.pump();

      // 点连接行把焦点收进树(行的 onPointerDown 里 requestFocus)
      await tester.tap(find.text('本机库'));
      await tester.pump();

      await pressCtrl(tester, LogicalKeyboardKey.keyF);
      final box = find.descendant(
        of: find.byType(SearchInput),
        matching: find.byType(TextField),
      );
      expect(
        tester.widget<TextField>(box).focusNode!.hasFocus,
        isTrue,
        reason: 'Ctrl+F 要把光标送进搜索框,不然还得再点一下',
      );

      await tester.enterText(box, '匹配不到');
      await tester.pump();
      expect(app.treeSearchText, '匹配不到');

      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pump();
      expect(app.treeSearchText, isEmpty, reason: 'Esc 等同点 ×:清空筛选');
      await leaveApp(tester);
    });
  });

  test('SqlFindShortcuts 只改替换键,其余沿用平台默认', () {
    const builder = SqlFindShortcuts();
    final defaults = const DefaultCodeShortcutsActivatorsBuilder();

    expect(
      builder.build(CodeShortcutType.replace),
      const [SingleActivator(LogicalKeyboardKey.keyH, control: true)],
    );
    expect(builder.build(CodeShortcutType.find),
        defaults.build(CodeShortcutType.find));
    expect(builder.build(CodeShortcutType.copy),
        defaults.build(CodeShortcutType.copy));
  });
}
