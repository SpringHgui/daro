import 'dart:io';

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

// 查询页输出区的「可选、可复制」:
// 1. 消息 tab 的整份日志是**一个**选择区(不是每条一个 Text),否则排查报错时
//    要「成功几行 + 挂在哪句」一起复制就得点 N 次;
// 2. 结果 tab 里的报错原文可选(贴给同事 / 搜错误码都靠复制,重敲不现实)。
// 结果网格的格选 + Ctrl+C 由 test/table_cell_region_select_test.dart 覆盖。

const String _title = '无标题-查询1';

/// 第二条语句报错的执行器:第一条正常返回一行
class _FlakyDriver implements DatabaseDriver {
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
    if (sql.contains('bad_col')) {
      throw StateError("Unknown column 'bad_col' in 'field list'");
    }
    return const QueryResult(columns: ['id'], rows: [['1']]);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => null;
}

/// 最近一次 `Clipboard.setData` 写入的文本
String? _clipText;

void _watchClipboard(WidgetTester tester) {
  _clipText = null;
  TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
      .setMockMethodCallHandler(SystemChannels.platform, (call) async {
    if (call.method == 'Clipboard.setData') {
      _clipText = (call.arguments as Map<Object?, Object?>)['text'] as String?;
    }
    return null;
  });
}

Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 8; i++) {
    await tester.pump(const Duration(milliseconds: 60));
  }
}

/// 跑一条成功 + 一条报错的脚本,停在指定 tab 上
Future<void> _runTo(WidgetTester tester, String tabLabel) async {
  final app = AppState();
  final conn = ConnectionInfo(
    name: 'c',
    typeId: 'mysql',
    host: 'h',
    port: '3306',
    username: 'u',
    isLive: true,
  );
  app.addConnections([conn]);
  app.connectionManager.attachDriverForTest(conn.name, _FlakyDriver());
  app.updateQueryText(
      AppState.queryTabKey('c', 'd1', _title), 'SELECT id FROM t;\nSELECT bad_col;');

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
  // 让两条语句都跑:右键编辑区 →「运行全部」
  // (F5 只跑光标所在那条,拿不到「一条成功 + 一条出错」的日志)
  final editorTop = tester.getTopLeft(find.byType(CodeEditor));
  final rightClick = await tester.startGesture(
    Offset(editorTop.dx + 90, editorTop.dy + 28),
    kind: PointerDeviceKind.mouse,
    buttons: kSecondaryButton,
  );
  await rightClick.up();
  await _settle(tester);
  await tester.tap(find.descendant(
    of: find.byType(Overlay),
    matching: find.text('运行全部'),
  ), kind: PointerDeviceKind.mouse);
  await _settle(tester);
  await tester.tap(find.text(tabLabel).first, kind: PointerDeviceKind.mouse);
  await _settle(tester);
}

/// 取 SelectableText 的纯文本:rich 构造只有 textSpan,字符串构造只有 data
String _plainText(Finder finder, WidgetTester tester) {
  final w = tester.widget<SelectableText>(finder);
  return w.data ?? w.textSpan!.toPlainText();
}

void main() {
  late Directory supportDir;

  setUp(() async {
    // AppState 会异步落盘:指向临时目录,避免污染用户目录
    supportDir = await Directory.systemTemp.createTemp('daro_query_copy');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => supportDir.path,
    );
  });

  tearDown(() async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, null);
    await supportDir.delete(recursive: true);
  });

  testWidgets('消息 tab:整份日志落在一个选择区里,语句回显与出错行都在里面',
      (tester) async {
    await _runTo(tester, '消息');

    final log = find.byType(SelectableText);
    expect(log, findsOneWidget, reason: '每条一个 Text 就选不出「前几条成功、第几条挂」');
    final text = _plainText(log, tester);
    // 回显的是切分后的语句本体(结束符被切分器吃掉,不带分号)
    expect(text, contains('SELECT id FROM t'));
    expect(text, contains('SELECT bad_col'));
    expect(text, contains('语句 2 出错'));
    expect(text, contains("Unknown column 'bad_col'"));
    expect(text, contains('语句 1:1 行'));
  });

  testWidgets('消息 tab:拖选出文本后 Ctrl+C 真的写进剪贴板', (tester) async {
    await _runTo(tester, '消息');
    _watchClipboard(tester);

    final log = find.byType(SelectableText);
    final text = _plainText(log, tester);
    // 第一行是语句回显(等宽 12):从行首按住拖到中段,落在同一条视觉行内
    final topLeft = tester.getTopLeft(log);
    final drag = await tester.startGesture(
      Offset(topLeft.dx + 6, topLeft.dy + 8),
      kind: PointerDeviceKind.mouse,
    );
    await tester.pump();
    await drag.moveBy(const Offset(24, 0));
    await tester.pump();
    await drag.moveBy(const Offset(30, 0));
    await tester.pump();
    await drag.up();
    await _settle(tester);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await _settle(tester);

    expect(_clipText, isNotNull, reason: '选中后 Ctrl+C 没有产生剪贴板写入');
    expect(_clipText, isNotEmpty);
    expect(text, contains(_clipText), reason: '复制出去的文本不是日志里的内容');
  });

  testWidgets('结果 tab 的报错原文可选,不是只能看', (tester) async {
    await _runTo(tester, '结果 2');

    final pane = find.byType(SelectableText);
    expect(pane, findsOneWidget);
    expect(_plainText(pane, tester), contains("Unknown column 'bad_col'"));
  });
}
