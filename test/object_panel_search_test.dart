import 'dart:io';

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/app/connection_manager.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/object_panel.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

// 对象面板工具栏搜索框的三条约定:
//   1. 输入即过滤当前分类的对象列表;
//   2. 过滤词是**全局状态**(AppState.objectSearchText),面板销毁重建后仍在,
//      且输入框保持展开——曾经切去表数据页再点回「对象」就什么都没了;
//   3. 换连接 / 换库 / 换分类都沿用同一份过滤词,不再随上下文重置。
class _FakeDriver implements DatabaseDriver {
  _FakeDriver({this.tables = const [], this.views = const []});

  final List<String> tables;
  final List<String> views;

  @override
  bool get isConnected => true;
  @override
  Future<void> connect() async {}
  @override
  Future<void> close() async {}
  @override
  Future<void> useDatabase(String database) async {}
  @override
  Future<List<String>> listSchemas(String database) async => const [];
  @override
  Future<List<String>> listDatabases() async => const ['bfin'];
  @override
  Future<List<String>> listTables(String database, {String? schema}) async =>
      List.of(tables);
  @override
  Future<List<String>> listViews(String database, {String? schema}) async =>
      List.of(views);
  @override
  Future<List<String>> listMaterializedViews(String database,
          {String? schema}) async =>
      const [];
  @override
  Future<List<String>> listFunctions(String database, {String? schema}) async =>
      const [];
  @override
  Future<List<String>> listProcedures(String database,
          {String? schema}) async =>
      const [];
  @override
  Future<List<String>> listUsers(String database) async => const [];
  @override
  Future<Map<String, String>> listTableComments(String database,
          {String? schema}) async =>
      const {};
  @override
  Future<Map<String, int>> listTableRowEstimates(String database,
          {String? schema}) async =>
      const {};
  @override
  dynamic noSuchMethod(Invocation invocation) => null;
}

const _connA = ConnectionInfo(
  name: 'A',
  typeId: 'mysql',
  host: 'h',
  port: '3306',
  username: 'u',
  isLive: true,
);

const _connB = ConnectionInfo(
  name: 'B',
  typeId: 'mysql',
  host: 'h',
  port: '3306',
  username: 'u',
  isLive: true,
);

/// 跑固定几帧代替 pumpAndSettle:输入框光标闪烁是持续动画,pumpAndSettle 会超时
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump(const Duration(milliseconds: 60));
  }
}

/// 建好两条已加载对象的连接(A: 3 表 2 视图;B: 2 表),返回 AppState
Future<AppState> _makeApp() async {
  final app = AppState();
  app.addConnections([_connA, _connB]);
  app.connectionManager.attachDriverForTest(
    'A',
    _FakeDriver(
      tables: const ['tc_car', 'tc_area', 'tc_carport'],
      views: const ['v_car_owner', 'v_area_tree'],
    ),
  );
  app.connectionManager.attachDriverForTest(
    'B',
    _FakeDriver(tables: const ['order_car', 'invoice']),
  );
  await app.connectionManager.expandDatabase(_connA, 'bfin');
  await app.connectionManager.expandDatabase(_connB, 'bfin');
  app.setObjectContext('A', 'bfin');
  expect(
    app.connectionManager.tableStateOf('A', 'bfin').status,
    LoadStatus.loaded,
    reason: '假驱动没能完成对象列表加载,面板不会渲染表项',
  );
  return app;
}

Future<void> _pumpPanel(WidgetTester tester, AppState app) async {
  await tester.pumpWidget(ChangeNotifierProvider<AppState>.value(
    value: app,
    child: MaterialApp(
      locale: const Locale('zh'),
      localizationsDelegates: kAppLocalizationsDelegates,
      supportedLocales: kSupportedLocales,
      // 与 main.dart 同构:SearchInput 内的 TextField 需要 Material 祖先
      home: const Material(child: ObjectPanel()),
    ),
  ));
  await _settle(tester);
}

/// 点开放大镜并输入 [text]
Future<void> _search(WidgetTester tester, String text) async {
  await tester.tap(find.byIcon(Icons.search));
  await _settle(tester);
  await tester.enterText(find.byType(SearchInput), text);
  await _settle(tester);
}

void main() {
  late Directory supportDir;

  setUp(() async {
    // AppState 会异步落盘:指向临时目录,避免污染用户目录也避免悬空 future
    supportDir = await Directory.systemTemp.createTemp('daro_search_test');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => supportDir.path,
    );
  });

  testWidgets('输入即过滤:只留名字含关键词的对象', (tester) async {
    final app = await _makeApp();
    await _pumpPanel(tester, app);

    expect(find.text('tc_area'), findsOneWidget);
    await _search(tester, 'car');

    expect(find.text('tc_car'), findsOneWidget);
    expect(find.text('tc_carport'), findsOneWidget);
    expect(find.text('tc_area'), findsNothing);
    expect(app.objectSearchText, 'car');
  });

  testWidgets('面板销毁重建(切标签页再回来)后,过滤词与展开态都还在',
      (tester) async {
    final app = await _makeApp();
    await _pumpPanel(tester, app);
    await _search(tester, 'car');

    // 模拟切到表数据页:面板整棵子树被摘掉,再切回来是全新实例
    await tester.pumpWidget(const SizedBox());
    await _pumpPanel(tester, app);

    expect(app.objectSearchText, 'car');
    // 输入框仍是展开态且带着原关键词(不是收进放大镜里)
    expect(find.byType(SearchInput), findsOneWidget);
    expect(find.text('car'), findsOneWidget);
    expect(find.text('tc_area'), findsNothing);
  });

  testWidgets('换连接 / 换分类沿用同一份过滤词(全局口径)', (tester) async {
    final app = await _makeApp();
    await _pumpPanel(tester, app);
    await _search(tester, 'car');

    // 换到另一条连接:面板仍在树上,列表按同一关键词过滤
    app.setObjectContext('B', 'bfin');
    await tester.pump();
    await _settle(tester);
    expect(find.text('order_car'), findsOneWidget);
    expect(find.text('invoice'), findsNothing);

    // 换分类同理:视图列表也只留含 car 的
    app.setObjectContext('A', 'bfin', category: ObjectCategory.view);
    await tester.pump();
    await _settle(tester);
    expect(find.text('v_car_owner'), findsOneWidget);
    expect(find.text('v_area_tree'), findsNothing);
  });

  testWidgets('清空关键词后列表恢复全量', (tester) async {
    final app = await _makeApp();
    await _pumpPanel(tester, app);
    await _search(tester, 'car');
    expect(find.text('tc_area'), findsNothing);

    await tester.enterText(find.byType(SearchInput), '');
    await _settle(tester);
    expect(app.objectSearchText, isEmpty);
    expect(find.text('tc_area'), findsOneWidget);
  });
}
