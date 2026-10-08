import 'dart:io';

import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/table_context_menu.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

// 视图 / 实体化视图实例右键菜单的能力裁剪。
//
// 菜单项不是一律摆出来再禁用,而是按引擎真实能力裁剪:
// 「设计」与「转储SQL」都依赖驱动 getDefinition 读出 CREATE 定义 ——
// Access 该方法恒返回 null,MySQL 系的 CREATE 文本丢了 SQL SECURITY 语义,
// 实体化视图则驱动侧根本没有对应 kind。这三条各自对应下面一个用例。
//
// 交互全部落在 AppState(打开 / 设计 / 删除),驱动走假实现:
// 只记录收到的 SQL,不断言服务端行为。

/// 记录收到的 SQL;getDefinition 返回预设的定义文本(空串 = 读不到,模拟 Access)
class _FakeDriver implements DatabaseDriver {
  _FakeDriver();

  final List<String> sqls = [];
  final String definition = 'CREATE VIEW `v1` AS SELECT 1';

  @override
  bool get isConnected => true;
  @override
  Future<void> connect() async {}
  @override
  Future<void> close() async {}
  @override
  Future<void> useDatabase(String database) async {}
  @override
  // PG 系走 sessionFor 时会调到它;不重写的话 noSuchMethod 返回 null,
  // await 一个非 Future<void> 会直接抛 _TypeError,DDL 永远发不出去
  Future<void> useSchema(String? schema) async {}
  @override
  Future<List<String>> listSchemas(String database) async => const [];
  @override
  Future<List<String>> listDatabases() async => const ['bfin'];
  @override
  Future<List<String>> listTables(String database, {String? schema}) async =>
      const [];
  @override
  Future<List<String>> listViews(String database, {String? schema}) async =>
      const ['v1'];
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
  Future<Map<String, String>> listViewComments(String database,
          {String? schema}) async =>
      const {};
  @override
  Future<Map<String, int>> listTableRowEstimates(String database,
          {String? schema}) async =>
      const {};
  @override
  Future<String?> getDefinition(String database, String name, String kind,
          {String? schema}) async =>
      definition.isEmpty ? null : definition;
  @override
  Future<QueryResult> executeQuery(String sql,
      {int limit = 1000, int offset = 0}) async {
    sqls.add(sql);
    return const QueryResult(columns: [], rows: []);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => null;
}

ConnectionInfo _connOf(String typeId) => ConnectionInfo(
      name: 'c',
      typeId: typeId,
      host: 'h',
      port: '3306',
      username: 'u',
      isLive: true,
    );

/// 挂好假驱动并把菜单弹出来;返回菜单里可见的文案集合。
Future<({AppState app, _FakeDriver driver, Set<String> labels})> _openMenu(
  WidgetTester tester, {
  required String typeId,
  ObjectCategory category = ObjectCategory.view,
  String name = 'v1',
}) async {
  final app = AppState();
  final driver = _FakeDriver();
  final conn = _connOf(typeId);
  app.addConnections([conn]);
  app.connectionManager.attachDriverForTest(conn.name, driver);
  // 必须先跑完一次加载:AppState 构造后会异步落盘 / 重载连接,不等它跑完
  // 就去点删除,dropObjects 里的 connectionByName 可能已经查不到这条连接
  // (于是静默返回"全部失败",连 SQL 都不会发)
  await app.connectionManager.expandDatabase(conn, 'bfin');
  expect(app.connectionByName(conn.name), isNotNull,
      reason: '假连接没能留在 AppState 里,后续 DDL 断言会假失败');

  await tester.pumpWidget(
    ChangeNotifierProvider<AppState>.value(
      value: app,
      child: MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: kAppLocalizationsDelegates,
        supportedLocales: kSupportedLocales,
        home: Material(
          child: Builder(
            builder: (context) {
              return TextButton(
                onPressed: () => showViewContextMenu(
                  context: context,
                  app: app,
                  category: category,
                  conn: conn,
                  database: 'bfin',
                  name: name,
                  position: Offset.zero,
                ),
                child: const Text('go'),
              );
            },
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();

  await tester.tap(find.text('go'));
  await tester.pumpAndSettle();

  final labels = <String>{};
  for (final element in tester.widgetList<Text>(find.byType(Text))) {
    final data = element.data;
    if (data != null) labels.add(data);
  }
  return (app: app, driver: driver, labels: labels);
}

Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump(const Duration(milliseconds: 60));
  }
}

void main() {
  late Directory supportDir;

  setUp(() async {
    // AppState 会异步落盘:指向临时目录,避免污染用户目录也避免悬空 future
    supportDir = await Directory.systemTemp.createTemp('daro_view_menu_test');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => supportDir.path,
    );
  });

  testWidgets('PostgreSQL 视图:打开 / 设计 / 删除 / 转储SQL 四项齐全', (tester) async {
    final ctx = await _openMenu(tester, typeId: 'postgresql');

    expect(ctx.labels, contains('打开视图'));
    expect(ctx.labels, contains('设计视图'));
    expect(ctx.labels, contains('删除视图'));
    expect(ctx.labels, contains('转储SQL文件'));
  });

  testWidgets('MySQL 视图:不给「设计」—— CREATE 文本丢了 SQL SECURITY 语义',
      (tester) async {
    final ctx = await _openMenu(tester, typeId: 'mysql');

    expect(ctx.labels, contains('打开视图'));
    // 保存一次就会把视图的 DEFINER/INVOKER 悄悄改回引擎默认,宁可不给入口
    expect(ctx.labels, isNot(contains('设计视图')));
    // 但定义本身是读得到的,转储照常可用
    expect(ctx.labels, contains('转储SQL文件'));
  });

  testWidgets('MariaDB 与 MySQL 同源,同样不给「设计」', (tester) async {
    final ctx = await _openMenu(tester, typeId: 'mariadb');
    expect(ctx.labels, isNot(contains('设计视图')));
    expect(ctx.labels, contains('转储SQL文件'));
  });

  testWidgets('Access 视图:getDefinition 恒为 null → 不给「设计」与「转储SQL」',
      (tester) async {
    final ctx = await _openMenu(tester, typeId: 'access');

    expect(ctx.labels, contains('打开视图'));
    expect(ctx.labels, contains('删除视图'));
    // 摆出来只会是空编辑器 / 空文件,不如不摆
    expect(ctx.labels, isNot(contains('设计视图')));
    expect(ctx.labels, isNot(contains('转储SQL文件')));
  });

  testWidgets('实体化视图:定义由界面侧 SQL 读取 → 设计 / 刷新 / 并发刷新 / 转储都在',
      (tester) async {
    final ctx = await _openMenu(
      tester,
      typeId: 'postgresql',
      category: ObjectCategory.materializedView,
    );

    expect(ctx.labels, contains('打开实体化视图'));
    expect(ctx.labels, contains('删除实体化视图'));
    expect(ctx.labels, contains('设计实体化视图'));
    expect(ctx.labels, contains('转储SQL文件'));
    // 刷新是实体化视图独有的一对(视图没有物化内容可刷)
    expect(ctx.labels, contains('刷新实体化视图'));
    expect(ctx.labels, contains('并发刷新实体化视图'));
  });

  testWidgets('普通视图不给「刷新」—— 没有可刷新的物化内容', (tester) async {
    final ctx = await _openMenu(tester, typeId: 'postgresql');
    expect(ctx.labels, isNot(contains('刷新视图')));
    expect(ctx.labels, isNot(contains('并发刷新视图')));
  });

  testWidgets('刷新:落到 REFRESH MATERIALIZED VIEW,结束后报耗时', (tester) async {
    final ctx = await _openMenu(
      tester,
      typeId: 'postgresql',
      category: ObjectCategory.materializedView,
    );

    await tester.tap(find.text('刷新实体化视图'));
    await _settle(tester);
    expect(ctx.driver.sqls, contains('REFRESH MATERIALIZED VIEW "public"."v1";'));
    expect(find.textContaining('已刷新实体化视图「v1」'), findsOneWidget);
  });

  testWidgets('并发刷新:带 CONCURRENTLY;探测不出唯一索引时放行交给服务端',
      (tester) async {
    // 假驱动的 executeQuery 回空集 → 探测读不出 → 不弹二次确认,直接执行
    final ctx = await _openMenu(
      tester,
      typeId: 'postgresql',
      category: ObjectCategory.materializedView,
    );

    await tester.tap(find.text('并发刷新实体化视图'));
    await _settle(tester);
    expect(
        ctx.driver.sqls,
        contains('REFRESH MATERIALIZED VIEW CONCURRENTLY "public"."v1";'));
    expect(find.text('未检测到满足条件的唯一索引'), findsNothing);
  });

  testWidgets('dropObjects 对视图分类直接产出 DROP VIEW(隔离驱动链路)',
      (tester) async {
    final ctx = await _openMenu(tester, typeId: 'postgresql');
    final failed = await ctx.app.dropObjects(
      ObjectCategory.view,
      ['v1'],
      connection: 'c',
      database: 'bfin',
    );
    expect(failed, isEmpty);
    expect(ctx.driver.sqls, contains('DROP VIEW IF EXISTS "v1"'));
  });

  testWidgets('删除视图:确认后走 DROP VIEW IF EXISTS', (tester) async {
    final ctx = await _openMenu(tester, typeId: 'postgresql');

    await tester.tap(find.text('删除视图'));
    await _settle(tester);
    expect(find.textContaining('确定要删除视图「v1」'), findsOneWidget);

    await tester.tap(find.text('删除'));
    await _settle(tester);

    expect(ctx.driver.sqls, contains('DROP VIEW IF EXISTS "v1"'));
  });
}
