import 'dart:async';
import 'dart:io';

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/query_page.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:re_editor/re_editor.dart';

// 查询编辑页的运行入口:F5 / F9 / Ctrl+Enter 跑「光标所在语句」,
// 右键菜单只给一个运行项 —— 落点在选区内就是「运行已选择的」,否则是
// 「运行当前语句」;整篇执行归工具栏 ▾ 的「运行全部」。
//
// 这里守的是两件看不见的事:
// 1. 快捷键只能作用在查询页(焦点域是编辑区的祖先 Focus,不吞 Ctrl+C/V/A
//    与撤销 —— 那些归 re_editor);
// 2. 「运行当前语句」真的只发那一条,不是整篇脚本 —— 假驱动记录收到的 SQL,
//    发多条就会露出来。
//
// 光标位置靠点编辑区得到(行高 20、上下留白 8 见 query_page 的 CodeEditorStyle),
// 不用真去数偏移:语句整行独占,点在这一行任意位置都归给同一条。

const String _title = '无标题-查询1';

class _RecordingDriver implements DatabaseDriver {
  _RecordingDriver();

  /// 驱动实际收到的语句(逐条执行 → 一条一项)
  final List<String> sqls = [];

  /// 设为 true 后 executeQuery 挂在 completer 上不放行(模拟长查询)
  bool hang = false;
  final Completer<void> _release = Completer<void>();

  @override
  bool get isConnected => true;
  @override
  Future<void> connect() async {}
  @override
  Future<void> close() async {}
  @override
  Future<void> useDatabase(String database) async {}
  @override
  Future<void> useSchema(String? schema) async {}
  @override
  Future<List<String>> listDatabases() async => const ['d1'];
  @override
  Future<List<String>> listSchemas(String database) async => const [];
  @override
  Future<List<String>> listTables(String database, {String? schema}) async =>
      const [];
  @override
  Future<QueryResult> executeQuery(String sql,
      {int limit = 1000, int offset = 0}) async {
    sqls.add(sql);
    if (hang) await _release.future;
    return const QueryResult(columns: [], rows: []);
  }

  void release() {
    if (!_release.isCompleted) _release.complete();
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => null;
}

class _Ctx {
  _Ctx(this.app, this.driver);

  final AppState app;
  final _RecordingDriver driver;

  /// 编辑区(整个查询页只有一个)
  Finder get editor => find.byType(CodeEditor);

  /// 点到 [line] 行(0 起)把光标落进那一行的语句
  Future<void> tapLine(WidgetTester tester, int line) async {
    final top = tester.getTopLeft(editor);
    await tester.tapAt(
      Offset(top.dx + 90, top.dy + 8 + line * 20 + 10),
      kind: PointerDeviceKind.mouse,
    );
    await tester.pump();
  }

  /// 在编辑区 [line] 行(0 起)按下右键(按下即弹,不等抬起 —— 与网格同一口径)
  Future<void> rightClickLine(WidgetTester tester, int line) async {
    final top = tester.getTopLeft(editor);
    final gesture = await tester.startGesture(
      Offset(top.dx + 90, top.dy + 8 + line * 20 + 10),
      kind: PointerDeviceKind.mouse,
      buttons: kSecondaryButton,
    );
    await gesture.up();
    await _settle(tester);
  }

  /// 在编辑区按下右键(按下即弹,不等抬起 —— 与网格同一口径)
  Future<void> rightClickEditor(WidgetTester tester) =>
      rightClickLine(tester, 1);
}

Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump(const Duration(milliseconds: 60));
  }
}

Future<_Ctx> _openPage(
  WidgetTester tester, {
  required String sql,
}) async {
  final app = AppState();
  final driver = _RecordingDriver();
  final conn = ConnectionInfo(
    name: 'c',
    typeId: 'mysql',
    host: 'h',
    port: '3306',
    username: 'u',
    isLive: true,
  );
  app.addConnections([conn]);
  app.connectionManager.attachDriverForTest(conn.name, driver);
  // initState 从 AppState 取该 tab 的文本,所以必须先写好再挂页面
  app.updateQueryText(AppState.queryTabKey('c', 'd1', _title), sql);

  await tester.pumpWidget(
    ChangeNotifierProvider<AppState>.value(
      value: app,
      child: MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: kAppLocalizationsDelegates,
        supportedLocales: kSupportedLocales,
        home: const Material(
          child: QueryPage(title: _title, connection: 'c', database: 'd1'),
        ),
      ),
    ),
  );
  await _settle(tester);
  return _Ctx(app, driver);
}

/// 三条各占一行的脚本:点第 N 行 = 光标在第 N 条语句里
const String _script = 'SELECT 1;\nSELECT 2;\nSELECT 3;';

/// 工具栏按钮(按控件类型限定,别和同名的菜单项混在一起)
Finder _toolButton(String label) => find.descendant(
      of: find.byType(ToolbarButton),
      matching: find.text(label),
    );

Finder _menuItem(String label) => find.descendant(
      // 限定在浮层面板里:工具栏主按钮在有选区时与「运行已选择的」同字,
      // 按文本全页找会歧义(点下去就是点工具栏了)
      of: find.byType(ContextMenuPanel),
      matching: find.text(label),
    );

void main() {
  late Directory supportDir;

  setUp(() async {
    // AppState 会异步落盘:指向临时目录,避免污染用户目录也避免悬空 future
    supportDir = await Directory.systemTemp.createTemp('daro_query_run_test');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => supportDir.path,
    );
  });

  group('运行快捷键', () {
    testWidgets('F5 只跑光标所在的那条语句', (tester) async {
      final ctx = await _openPage(tester, sql: _script);

      await ctx.tapLine(tester, 1);
      await tester.sendKeyEvent(LogicalKeyboardKey.f5);
      await _settle(tester);

      expect(ctx.driver.sqls, ['SELECT 2'],
          reason: 'F5 不该把整篇脚本发出去');
    });

    testWidgets('F9 与 F5 同一口径(SSMS 肌肉记忆)', (tester) async {
      final ctx = await _openPage(tester, sql: _script);

      await ctx.tapLine(tester, 2);
      await tester.sendKeyEvent(LogicalKeyboardKey.f9);
      await _settle(tester);

      expect(ctx.driver.sqls, ['SELECT 3']);
    });

    testWidgets('Ctrl+Enter 跑光标所在语句(它默认是换行,得接管 Intent)',
        (tester) async {
      final ctx = await _openPage(tester, sql: _script);

      await ctx.tapLine(tester, 0);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await _settle(tester);

      expect(ctx.driver.sqls, ['SELECT 1']);
    });

    testWidgets('有选区时跑选区内容(菜单「全选」→「运行已选择的」)',
        (tester) async {
      final ctx = await _openPage(tester, sql: _script);

      await ctx.tapLine(tester, 1);
      // 全选走菜单项(直接调 controller.selectAll),不碰 re_editor 的 Ctrl+A:
      // 那条快捷键在裸编辑器探针里根本不触发,与本页改动无关
      await ctx.rightClickEditor(tester);
      await tester.tap(_menuItem('全选'));
      await _settle(tester);
      await ctx.rightClickEditor(tester);
      await tester.tap(_menuItem('运行已选择的'));
      await _settle(tester);

      expect(ctx.driver.sqls, ['SELECT 1', 'SELECT 2', 'SELECT 3']);
    });

    testWidgets('有选区时主按钮自己写成「运行已选择的」,取消选区后变回「运行」',
        (tester) async {
      final ctx = await _openPage(tester, sql: _script);

      // 无选区:按钮就是「运行」(点它跑光标所在语句)
      expect(_toolButton('运行'), findsOneWidget);
      expect(_toolButton('运行已选择的'), findsNothing);

      // 走菜单「全选」造选区(不碰 Ctrl+A,那条在裸编辑器探针里不触发)
      await ctx.tapLine(tester, 1);
      await ctx.rightClickEditor(tester);
      await tester.tap(_menuItem('全选'));
      await _settle(tester);

      expect(_toolButton('运行已选择的'), findsOneWidget,
          reason: '选中一段后,主按钮必须自己说清这一下跑的是选区');
      expect(_toolButton('运行'), findsNothing);

      // 直接点主按钮(不开下拉):跑的是选区内容
      await tester.tap(
          _toolButton('运行已选择的'), kind: PointerDeviceKind.mouse);
      await _settle(tester);
      expect(ctx.driver.sqls, ['SELECT 1', 'SELECT 2', 'SELECT 3']);

      // 点一下编辑区把选区收掉 → 文案得跟着回去,否则按钮从此在说谎
      await ctx.tapLine(tester, 0);
      await _settle(tester);
      expect(_toolButton('运行'), findsOneWidget);
      expect(_toolButton('运行已选择的'), findsNothing);
    });

    testWidgets('回车仍然换行(接管 Intent 不能把换行一起吞了)', (tester) async {
      final ctx = await _openPage(tester, sql: 'SELECT 1;');

      await ctx.tapLine(tester, 0);
      final before = ctx.app.queryTextFor(
          AppState.queryTabKey('c', 'd1', _title));
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await _settle(tester);

      expect(ctx.driver.sqls, isEmpty, reason: '换行不该触发运行');
      expect(
          ctx.app
              .queryTextFor(AppState.queryTabKey('c', 'd1', _title))
              .split('\n')
              .length,
          before.split('\n').length + 1,
          reason: '不带 Ctrl 的回车必须还是换行');
    });

    testWidgets('运行中再按 F5 不重入(与按钮禁用同一口径)', (tester) async {
      final ctx = await _openPage(tester, sql: _script);
      ctx.driver.hang = true;

      await ctx.tapLine(tester, 0);
      await tester.sendKeyEvent(LogicalKeyboardKey.f5);
      await tester.pump(const Duration(milliseconds: 60));
      await tester.sendKeyEvent(LogicalKeyboardKey.f5);
      await tester.pump(const Duration(milliseconds: 60));

      ctx.driver.release();
      await _settle(tester);

      expect(ctx.driver.sqls, ['SELECT 1'], reason: '第二次按键应被忽略');
    });
  });

  group('编辑区右键菜单', () {
    testWidgets('无选区:运行项只有「运行当前语句」,带 F5 提示', (tester) async {
      final ctx = await _openPage(tester, sql: _script);

      await ctx.rightClickEditor(tester);

      expect(_menuItem('运行当前语句'), findsOneWidget);
      expect(_menuItem('运行已选择的'), findsNothing,
          reason: '运行项只给一个,落点不在选区内就不该出现「已选择的」');
      expect(_menuItem('运行全部'), findsNothing,
          reason: '整篇执行归工具栏 ▾,右键菜单不再并列三项');
      expect(_menuItem('复制'), findsOneWidget);
      expect(_menuItem('粘贴'), findsOneWidget);
      expect(_menuItem('全选'), findsOneWidget);
    });

    testWidgets('右键落在选区内:运行项换成「运行已选择的」', (tester) async {
      final ctx = await _openPage(tester, sql: _script);

      await ctx.tapLine(tester, 1);
      await ctx.rightClickEditor(tester);
      await tester.tap(_menuItem('全选'));
      await _settle(tester);
      // 右键点(第 1 行)仍在全选范围内 → 选区保留,文案跟着换
      await ctx.rightClickLine(tester, 1);

      expect(_menuItem('运行已选择的'), findsOneWidget);
      expect(_menuItem('运行当前语句'), findsNothing);
    });

    testWidgets('右键落在选区外:选区收掉,只跑落点那条', (tester) async {
      final ctx = await _openPage(tester, sql: _script);

      // 直接给控制器造「前两行」的选区:拖选在 widget 测试里被滚动手势抢走、
      // Shift+方向键走 IME 通道也造不出,而本页要守的是右键落点与选区的关系
      tester.widget<CodeEditor>(ctx.editor).controller!.selection =
          const CodeLineSelection(
              baseIndex: 0, baseOffset: 0, extentIndex: 1, extentOffset: 8);
      await _settle(tester);
      await ctx.rightClickLine(tester, 2);
      await tester.tap(_menuItem('运行当前语句'));
      await _settle(tester);

      expect(ctx.driver.sqls, ['SELECT 3'],
          reason: '右键点在选区外,光标已移过去,跑的不是刚才那段选区');
    });

    testWidgets('工具栏 ▾ 的「运行全部」逐条发送三条语句', (tester) async {
      final ctx = await _openPage(tester, sql: _script);

      await tester.tap(find.byIcon(Icons.arrow_drop_down),
          kind: PointerDeviceKind.mouse);
      await _settle(tester);
      await tester.tap(find.text('运行全部'), kind: PointerDeviceKind.mouse);
      await _settle(tester);

      expect(ctx.driver.sqls, ['SELECT 1', 'SELECT 2', 'SELECT 3']);
    });

    testWidgets('「运行当前语句」与 F5 同口径:只跑光标那条', (tester) async {
      final ctx = await _openPage(tester, sql: _script);

      await ctx.tapLine(tester, 1);
      await ctx.rightClickEditor(tester);
      await tester.tap(_menuItem('运行当前语句'));
      await _settle(tester);

      expect(ctx.driver.sqls, ['SELECT 2']);
    });
  });
}
