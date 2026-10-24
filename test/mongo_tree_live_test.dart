import 'dart:io';

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/data/table_design.dart';
import 'package:daro/main.dart';
import 'package:daro/widgets/database_tree.dart';
import 'package:daro/widgets/table_data_page.dart';

import 'mcp_fakes.dart';
import 'pin_system_locale.dart';

/// **真服务端数据 + 真实界面**:主窗口连接树打开 MongoDB 连接 → 列集合与视图 →
/// 双击集合在表数据页里看到真文档。
///
/// 界面用例吃的数据不是手写假值:`setUpAll` 用**真 MongoDriver**连探针容器实抓
/// 一遍快照(库名 / 集合名 / 视图名 / 文档预览 / 列描述 / 表统计),所以这里验的是
/// 「树与网格怎么接待 mongodb 这个类型」,网络那一头由 `mongo_driver_live_test.dart` 负责。
///
/// 为什么不在 widget 测试里直接走真网络(试过,不通):`testWidgets` 跑在 FakeAsync
/// 区域里,套接字回包的微任务只在 `pump()` 时冲刷,而 `pump()` 不允许出现在
/// `tester.runAsync` 里 —— 实测链路停在 `LoadStatus.loading` 不动(4 秒假时钟 +
/// 1.5 秒真时钟都等不到 loaded)。故拆两段:区域外抓真数据,区域内跑真界面。
///
/// 默认不跑,且必须显式给密码:
///   DARO_LIVE_DB=1 DARO_MONGO_PWD='…' flutter test test/mongo_tree_live_test.dart
const _connName = 'mongo 探针';
const _db = 'probe_db';
const _orders = 'orders';

void main() {
  final enabled = Platform.environment['DARO_LIVE_DB'] == '1';
  final password = Platform.environment['DARO_MONGO_PWD'] ?? '';
  final skip = !enabled
      ? '未开启实连(DARO_LIVE_DB=1 时才跑)'
      : password.isEmpty
          ? '未给 DARO_MONGO_PWD'
          : null;
  if (skip != null) {
    // ignore: avoid_print
    print('实连界面测试跳过:$skip');
    test('实连:主窗口浏览 MongoDB(跳过)', () {}, skip: skip);
    return;
  }

  final conn = ConnectionInfo(
    name: _connName,
    typeId: 'mongodb',
    host: Platform.environment['DARO_MONGO_HOST'] ?? '127.0.0.1',
    port: Platform.environment['DARO_MONGO_PORT'] ?? '27017',
    username: Platform.environment['DARO_MONGO_USER'] ?? 'admin',
    password: password,
    isLive: true,
  );

  // 探针容器实抓的快照(在 setUpAll 里填充,widget 用例只读)
  late List<String> dbs;
  late List<String> tables;
  late List<String> views;
  late TablePreview ordersPreview;
  late List<ColumnDef> ordersColumns;
  late Map<String, int> estimates;
  late TableDetail? ordersDetail;
  late String? viewDefinition;

  late Directory dir;
  setUp(() async {
    TestWidgetsFlutterBinding.ensureInitialized();
    dir = await Directory.systemTemp.createTemp('daro_mongo_tree');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
            const MethodChannel('plugins.flutter.io/path_provider'),
            (c) async => dir.path);
  });
  tearDown(() async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
            const MethodChannel('plugins.flutter.io/path_provider'), null);
    // 探针连接可能落了缓存文件,删不掉不影响结论
    try {
      if (dir.existsSync()) dir.deleteSync(recursive: true);
    } on FileSystemException {
      // ignore
    }
  });

  setUpAll(() async {
    final real = createDriver(conn)!;
    await real.connect();
    dbs = await real.listDatabases();
    tables = await real.listTables(_db);
    views = await real.listViews(_db);
    ordersPreview = await real.previewTable(_db, _orders);
    ordersColumns = await real.describeTable(_db, _orders);
    estimates = await real.listTableRowEstimates(_db);
    ordersDetail = await real.readTableDetail(_db, _orders);
    viewDefinition = await real.getDefinition(_db, 'paid_view', 'view');
    await real.close();
  });

  Finder treeText(String text) =>
      find.descendant(of: find.byType(DatabaseTree), matching: find.text(text));

  /// 树内全部文本(失败时打出来,定位「点了但没展开」卡在哪一步)
  String treeDump(WidgetTester tester) =>
      '树内文本: ${tester.widgetList<Text>(find.descendant(
              of: find.byType(DatabaseTree), matching: find.byType(Text)))
          .map((w) => w.data)
          .toList()}';

  /// 推进若干帧:树在「打开中」有无循环转圈动画,pumpAndSettle 等不到静止
  Future<void> pumpFrames(WidgetTester tester) async {
    for (var i = 0; i < 8; i++) {
      await tester.pump(const Duration(milliseconds: 60));
    }
  }

  /// 双击树内某行(双击判定窗口 500ms,两次按下要落在同一窗口内)
  Future<void> doubleTapRow(WidgetTester tester, String name) async {
    await tester.tap(treeText(name).first);
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(treeText(name).first);
    await pumpFrames(tester);
  }

  testWidgets('实连数据:树里打开 MongoDB 连接 → 集合列表 → 双击进网格',
      (tester) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    pinSystemChineseLocale(tester);

    final app = AppState();
    await tester.pumpWidget(ChangeNotifierProvider<AppState>.value(
      value: app,
      child: const DbApp(),
    ));
    await tester.pump();
    app.addConnections([conn]);
    // createDriver 必须认得 mongodb,否则树上只会弹「不支持该类型」
    expect(createDriver(conn), isNotNull, reason: 'createDriver 没登记 mongodb');
    app.connectionManager.attachDriverForTest(
        conn.name,
        _ProbeDriver(
          dbs: dbs,
          tables: tables,
          views: views,
          preview: ordersPreview,
          columns: ordersColumns,
          estimates: estimates,
          ordersDetail: ordersDetail,
          viewDefinition: viewDefinition,
        ));
    await tester.pump();

    expect(treeText(_connName), findsOneWidget);
    await doubleTapRow(tester, _connName);
    expect(treeText(_db), findsOneWidget, reason: treeDump(tester));
    expect(treeText('admin'), findsOneWidget, reason: treeDump(tester));

    await doubleTapRow(tester, _db);
    // 库展开后先出分类分组头(Navicat 式两级:库 → 分类 → 对象)
    expect(treeText('表'), findsOneWidget, reason: treeDump(tester));
    expect(treeText('视图'), findsOneWidget, reason: treeDump(tester));
    // MongoDB 没有函数 / 存储过程 / 用户,这些分组头压根不该出现
    expect(treeText('函数'), findsNothing, reason: treeDump(tester));
    expect(treeText('存储过程'), findsNothing, reason: treeDump(tester));

    await doubleTapRow(tester, '表');
    // 中文集合名与含点号的集合名都要能过树的路径
    expect(treeText(_orders), findsOneWidget, reason: treeDump(tester));
    expect(treeText('中文集合'), findsOneWidget, reason: treeDump(tester));
    expect(treeText('a.b'), findsOneWidget, reason: treeDump(tester));
    // 服务端的内部集合不能露出来(探针实测 system.views 会混进列表)
    expect(treeText('system.views'), findsNothing, reason: treeDump(tester));

    await doubleTapRow(tester, '视图');
    // 视图分组:paid_view 是视图,不该混进集合列表里
    expect(treeText('paid_view'), findsOneWidget, reason: treeDump(tester));

    await doubleTapRow(tester, _orders);
    expect(find.byType(TableDataPage), findsOneWidget,
        reason: '双击集合没有打开表数据页');
    expect(find.byType(DataGridView), findsOneWidget);
    // 真文档里的值(ObjectId / 二进制 / 缺键)按驱动口径渲染成字符串与 NULL。
    // 缺键 / null → 'NULL';只查前几列,网格按需构建可见单元,靠右的列这时还没上屏。
    expect(find.text('NULL'), findsWidgets, reason: '缺键 / null 没按 NULL 显示');
    for (final cell in ordersPreview.rows.first.take(4)) {
      if (cell == 'NULL') continue;
      expect(find.text(cell), findsWidgets, reason: '文档值「$cell」没渲染进网格');
    }
  });
}

/// 用探针实抓数据回答界面会问的元数据调用。
///
/// 三个必须与真驱动同口径的点(否则「打开库」永远停在 loading):
/// 1. [isConnected] 必须为真。`attachDriverForTest` 只塞字典,`_driverFor` 见到
///    未连接的注入驱动会**丢掉它另建一个真的**,于是假时钟里等不到真握手。
/// 2. 库级详情 / 表依赖 / 设计候选要返回 `null`(真 MongoDriver 就是返回 null,
///    表示「该类型不提供这类信息」)。FakeDriver 没实现这些 → `noSuchMethod` 抛
///    UnsupportedError → 上层按「读取失败」把驱动摘掉并 close,后面的展开只能重连。
/// 3. 抛异常只在真驱动确实读不到时才该出现,快照里没有「读失败」这回事。
class _ProbeDriver extends FakeDriver {
  _ProbeDriver({
    required List<String> dbs,
    required List<String> tables,
    required List<String> views,
    required TablePreview preview,
    required List<ColumnDef> columns,
    required Map<String, int> estimates,
    required this.ordersDetail,
    required String? viewDefinition,
  })  : _dbs = dbs,
        super(_connName) {
    this.tables = tables;
    this.views = views;
    schemas = const [];
    functions = const [];
    procedures = const [];
    this.columns = columns;
    this.preview = preview;
    rowEstimates = estimates;
    tableComments = {};
    definition = viewDefinition;
  }

  final List<String> _dbs;
  final TableDetail? ordersDetail;

  @override
  bool get isConnected => true;

  @override
  Future<List<String>> listDatabases() async => _dbs;

  @override
  Future<DatabaseDetail?> readDatabaseDetail(String database) async => null;

  @override
  Future<List<DependentObject>?> readTableDependencies(
          String database, String table,
          {String? schema, bool usedBy = true}) async =>
      null;

  @override
  Future<DesignCandidates> readDesignCandidates(String database) async =>
      DesignCandidates.empty;

  @override
  Future<TableDetail?> readTableDetail(String database, String table,
          {String? schema}) async =>
      table == _orders ? ordersDetail : null;
}
