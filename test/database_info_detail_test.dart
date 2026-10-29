import 'dart:io';

import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/database_info.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

// 右侧详情面板的库 / 表两页:属性取自驱动,展示形态对齐 Navicat。
// 这里用假驱动钉住「取数 → 折算 → 排版」的接缝,尤其是两条容易悄悄回归的约定:
// 1) 展示用的行数只读引擎估算值,精确 COUNT 只在点「获取行数」时发生;
// 2) 目录里没有的属性显示占位符,不猜 0 / 空串。

ConnectionInfo _conn({String typeId = 'mysql'}) => ConnectionInfo(
      name: '温附一',
      typeId: typeId,
      host: '10.70.37.113',
      port: '3306',
      username: 'root',
      isLive: true,
    );

/// 只回应详情面板会问到的方法,其余走 noSuchMethod(面板不会碰到)。
class _DetailFakeDriver implements DatabaseDriver {
  _DetailFakeDriver({this.tableDetail, this.databaseDetail, this.deps});

  final TableDetail? tableDetail;
  final DatabaseDetail? databaseDetail;

  /// 「使用 / 被使用」两页的返回内容(两向共用一份,只验接缝)
  final List<DependentObject>? deps;

  /// 被真正调用到的次数:用于断言「没有偷偷扫全表」与「缓存命中」
  int countCalls = 0;
  int dbDetailCalls = 0;
  int tableDetailCalls = 0;
  String? lastDefinitionKind;

  /// 依赖查询问到的方向(true = 被使用),按调用顺序记录
  final List<bool> depCalls = [];

  @override
  bool get isConnected => true;

  @override
  Future<void> connect() async {}

  @override
  Future<void> close() async {}

  @override
  Future<void> useDatabase(String database) async {}

  @override
  Future<DatabaseDetail?> readDatabaseDetail(String database) async {
    dbDetailCalls++;
    return databaseDetail;
  }

  @override
  Future<TableDetail?> readTableDetail(String database, String table,
      {String? schema}) async {
    tableDetailCalls++;
    return tableDetail;
  }

  @override
  Future<List<DependentObject>?> readTableDependencies(
      String database, String table,
      {String? schema, bool usedBy = true}) async {
    depCalls.add(usedBy);
    return deps;
  }

  @override
  Future<int> countTable(String database, String table,
      {String? schema, String? where}) async {
    countCalls++;
    return 4211;
  }

  @override
  Future<String?> getDefinition(String database, String name, String kind,
      {String? schema}) async {
    lastDefinitionKind = kind;
    return 'CREATE TABLE `$name` (\n  `id` int NOT NULL\n) ENGINE=InnoDB;';
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => null;
}

void main() {
  late Directory dir;

  setUp(() {
    TestWidgetsFlutterBinding.ensureInitialized();
    // AppState 的每次改动都会落盘;没有这个通道会抛 MissingPluginException
    dir = Directory.systemTemp;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
            const MethodChannel('plugins.flutter.io/path_provider'),
            (c) async => dir.path);
  });

  Future<AppState> pump(
    WidgetTester tester, {
    required SelectedNode selection,
    required _DetailFakeDriver driver,
    String typeId = 'mysql',
  }) async {
    final app = AppState()..addConnection(_conn(typeId: typeId));
    app.connectionManager.attachDriverForTest('温附一', driver);
    app.detailSelection.value = selection;
    await tester.pumpWidget(MultiProvider(
      providers: [ChangeNotifierProvider.value(value: app)],
      child: MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: kAppLocalizationsDelegates,
        supportedLocales: kSupportedLocales,
        home: Scaffold(
          body: SizedBox(width: 320, child: DatabaseInfo()),
        ),
      ),
    ));
    // 详情是异步取数,让 FutureBuilder 之外的 setState 落地
    await tester.pump();
    await tester.pump();
    return app;
  }

  final tableDetail = TableDetail(
    name: 'bx11_yz_payorder',
    engine: 'InnoDB',
    rowFormat: 'Dynamic',
    collation: 'utf8mb4_general_ci',
    rowEstimate: 0,
    autoIncrement: 0,
    createTime: DateTime(2026, 3, 3, 14, 43, 56),
    indexLength: 131072,
    dataLength: 16384,
    maxDataLength: 0,
    dataFree: 0,
  );

  final tableNode = const SelectedNode(
    NodeKind.table,
    'bx11_yz_payorder',
    connection: '温附一',
    database: 'bxarchive',
  );

  group('表详情', () {
    testWidgets('按 Navicat 的字段集与取值渲染', (tester) async {
      await pump(tester, selection: tableNode, driver: _DetailFakeDriver(tableDetail: tableDetail));

      for (final label in [
        '行',
        '引擎',
        '自动递增',
        '行格式',
        '修改日期',
        '创建日期',
        '检查时间',
        '索引长度',
        '数据长度',
        '最大数据长度',
        '数据可用空间',
        '排序规则',
        '创建选项',
        '注释',
      ]) {
        expect(find.text(label), findsOneWidget, reason: '缺少字段「$label」');
      }
      expect(find.text('bx11_yz_payorder'), findsOneWidget);
      expect(find.text('表'), findsOneWidget);
      // 上下文行:主机 + 连接名,以及所属库
      expect(find.text('10.70.37.113  温附一'), findsOneWidget);
      expect(find.text('bxarchive'), findsOneWidget);
      // 估算值走目录,不扫全表
      expect(find.text('0 (估算)'), findsOneWidget);
      expect(find.text('128.00 KB (131,072)'), findsOneWidget);
      expect(find.text('16.00 KB (16,384)'), findsOneWidget);
      expect(find.text('2026-03-03 14:43:56'), findsOneWidget);
      expect(find.text('获取行数'), findsOneWidget);
    });

    testWidgets('打开面板时不触发精确 COUNT', (tester) async {
      final driver = _DetailFakeDriver(tableDetail: tableDetail);
      await pump(tester, selection: tableNode, driver: driver);
      expect(driver.countCalls, 0);
    });

    testWidgets('点「获取行数」才扫表,并把估算值换成精确值', (tester) async {
      final driver = _DetailFakeDriver(tableDetail: tableDetail);
      await pump(tester, selection: tableNode, driver: driver);

      await tester.tap(find.text('获取行数'));
      await tester.pump();
      await tester.pump();

      expect(driver.countCalls, 1);
      expect(find.text('4,211'), findsOneWidget);
      expect(find.text('0 (估算)'), findsNothing);
    });

    testWidgets('目录取不到的属性显示占位符而非 0', (tester) async {
      // MyISAM 之外的引擎普遍不维护 UPDATE_TIME / CHECK_TIME
      await pump(
        tester,
        selection: tableNode,
        driver: _DetailFakeDriver(
          tableDetail: const TableDetail(name: 't', engine: 'InnoDB'),
        ),
      );
      // 14 个字段里只有「引擎」取到了值,其余 13 个目录没给 → 全部占位符
      expect(find.text('InnoDB'), findsOneWidget);
      expect(find.text('--'), findsNWidgets(13));
      // 缺失值绝不写成 0:那会把「目录没这个数」呈现成「确实为零」
      expect(find.text('0'), findsNothing);
    });

    testWidgets('切到 DDL 页才取建表语句,并可复制', (tester) async {
      final driver = _DetailFakeDriver(tableDetail: tableDetail);
      await pump(tester, selection: tableNode, driver: driver);
      expect(driver.lastDefinitionKind, isNull);

      await tester.tap(find.text('DDL'));
      await tester.pump();
      await tester.pump();

      expect(driver.lastDefinitionKind, 'table');
      // DDL 页现在是带语法高亮的富文本,finder 必须开 findRichText 才看得见。
      expect(find.textContaining('CREATE TABLE `bx11_yz_payorder`',
              findRichText: true),
          findsOneWidget);
      // 信息页正文此时被 DDL 页替换
      expect(find.text('引擎'), findsNothing);
    });

    testWidgets('非 MySQL 类型回退基础字段,且不提供 DDL 页', (tester) async {
      await pump(
        tester,
        selection: tableNode,
        driver: _DetailFakeDriver(tableDetail: tableDetail),
        typeId: 'sqlite',
      );
      expect(find.text('引擎'), findsNothing);
      expect(find.text('DDL'), findsNothing);
      expect(find.text('bxarchive'), findsOneWidget);
    });
  });

  group('库详情', () {
    testWidgets('展示默认字符集与排序规则', (tester) async {
      await pump(
        tester,
        selection:
            const SelectedNode(NodeKind.database, 'bx11', connection: '温附一'),
        driver: _DetailFakeDriver(
          databaseDetail: const DatabaseDetail(
            name: 'bx11',
            charset: 'utf8mb4',
            collation: 'utf8mb4_general_ci',
          ),
        ),
      );
      expect(find.text('bx11'), findsOneWidget);
      expect(find.text('数据库'), findsOneWidget);
      expect(find.text('字符集'), findsOneWidget);
      expect(find.text('utf8mb4'), findsOneWidget);
      expect(find.text('排序规则'), findsOneWidget);
      expect(find.text('utf8mb4_general_ci'), findsOneWidget);
      expect(find.text('10.70.37.113  温附一'), findsOneWidget);
    });

    testWidgets('同一库来回切换只查一次目录', (tester) async {
      final driver = _DetailFakeDriver(
        databaseDetail: const DatabaseDetail(
            name: 'bx11', charset: 'utf8mb4', collation: 'utf8mb4_general_ci'),
      );
      final node =
          const SelectedNode(NodeKind.database, 'bx11', connection: '温附一');
      await pump(tester, selection: node, driver: driver);
      expect(driver.dbDetailCalls, 1);

      // 选中别处再选回来:视图 State 会重建,但 ConnectionManager 的详情缓存
      // 应当挡住第二次目录查询
      final app = tester.element(find.byType(DatabaseInfo)).read<AppState>();
      app.detailSelection.value = const SelectedNode(
        NodeKind.table,
        'bx11_yz_payorder',
        connection: '温附一',
        database: 'bxarchive',
      );
      await tester.pump();
      expect(driver.tableDetailCalls, 1);

      app.detailSelection.value = node;
      await tester.pump();
      await tester.pump();

      expect(driver.dbDetailCalls, 1);
    });
  });

  group('PostgreSQL 详情', () {
    const pgTableDetail = TableDetail(
      name: 'cameras',
      oid: '44062',
      owner: 'postgres',
      tableType: 'r',
      partitionOf: '',
      tablespace: 'pg_default',
      inheritsFrom: '',
      hasOids: false,
      fillFactor: '100',
      acl: 'postgres=arwdDxt/postgres',
      comment: '摄像头',
      // 未 ANALYZE:目录给的是 -1 哨兵,驱动折成 null,界面不能呈现成 0 行
      rowEstimate: null,
    );

    const pgTableNode = SelectedNode(NodeKind.table, 'cameras',
        connection: '温附一', database: 'daowei_dev');

    final deps = [
      const DependentObject(
        schema: 'public',
        name: 'fk_cameras_parking_gates_gate_id',
        kind: 'FOREIGN KEY',
        degree: 'AUTO',
        children: [
          DependentObject(
            schema: 'public',
            name: 'RI_ConstraintTrigger_a_44087',
            kind: 'TRIGGER',
            degree: 'INTERNAL',
          ),
        ],
      ),
      const DependentObject(
        schema: 'public',
        name: 'ix_cameras_gate_id',
        kind: 'INDEX',
        degree: 'AUTO',
      ),
    ];

    testWidgets('表信息页按 Navicat 的 PG 字段集渲染', (tester) async {
      await pump(tester,
          selection: pgTableNode,
          driver: _DetailFakeDriver(tableDetail: pgTableDetail),
          typeId: 'postgresql');

      for (final label in [
        'OID',
        '所有者',
        '行',
        'Table Type',
        '分区属于',
        '表空间',
        'Inherits From',
        'Has OIDs',
        '填充因子',
        'ACL',
        '注释',
      ]) {
        expect(find.text(label), findsOneWidget, reason: '缺少字段「$label」');
      }
      // MySQL 家族的字段绝不该串台
      expect(find.text('引擎'), findsNothing);
      expect(find.text('行格式'), findsNothing);
      expect(find.text('44062'), findsOneWidget);
      expect(find.text('postgres'), findsOneWidget);
      // Navicat 的中文界面给「常规」，不是 SQL 关键字
      expect(find.text('常规'), findsOneWidget);
      expect(find.text('r'), findsNothing);
      // 布尔属性走 是 / 否,不显示 t / f
      expect(find.text('否'), findsOneWidget);
      // 目录没给行数 → 占位符,且绝不自动 COUNT
      expect(find.text('--'), findsNWidgets(3));
      expect(find.text('获取行数'), findsOneWidget);
    });

    testWidgets('库信息页列 OID / 所有者 / 表空间 / 编码 / 连接限制,且 -1 折成「无」',
        (tester) async {
      await pump(
        tester,
        selection: const SelectedNode(NodeKind.database, 'daowei_dev',
            connection: '温附一'),
        driver: _DetailFakeDriver(
          databaseDetail: const DatabaseDetail(
            name: 'daowei_dev',
            oid: '43604',
            owner: 'postgres',
            tablespace: 'pg_default',
            charset: 'UTF8',
            collation: 'en_US.utf8',
            connectionLimit: '-1',
          ),
        ),
        typeId: 'postgresql',
      );
      expect(find.text('编码'), findsOneWidget);
      expect(find.text('UTF8'), findsOneWidget);
      expect(find.text('排序规则排序'), findsOneWidget);
      expect(find.text('en_US.utf8'), findsOneWidget);
      expect(find.text('连接限制'), findsOneWidget);
      expect(find.text('无'), findsOneWidget);
      expect(find.text('-1'), findsNothing);
      // MySQL 的「字符集」标签不再出现在 PG 页上
      expect(find.text('字符集'), findsNothing);
    });

    testWidgets('依赖页只在点开时取数,并渲染类型后缀与触发器子项', (tester) async {
      final driver =
          _DetailFakeDriver(tableDetail: pgTableDetail, deps: deps);
      await pump(tester,
          selection: pgTableNode, driver: driver, typeId: 'postgresql');

      // PG 有 DDL 页与两个依赖页签;打开面板时一条依赖查询都不发
      expect(find.text('DDL'), findsOneWidget);
      expect(find.byIcon(Icons.call_made), findsOneWidget);
      expect(find.byIcon(Icons.call_received), findsOneWidget);
      expect(driver.depCalls, isEmpty);

      await tester.tap(find.byIcon(Icons.call_received));
      await tester.pump();
      await tester.pump();

      expect(driver.depCalls, [true], reason: '「被使用」图标应以 usedBy=true 取数');
      expect(find.text('public.fk_cameras_parking_gates_gate_id'),
          findsOneWidget);
      expect(find.text('FOREIGN KEY (AUTO)'), findsOneWidget);
      // 外键默认展开:内部触发器不点也能看见
      expect(find.text('public.RI_ConstraintTrigger_a_44087'), findsOneWidget);
      expect(find.text('TRIGGER (INTERNAL)'), findsOneWidget);
      expect(find.text('public.ix_cameras_gate_id'), findsOneWidget);
      // 信息页正文已被替换
      expect(find.text('填充因子'), findsNothing);

      // 另一向各自取数(不共用缓存键)
      await tester.tap(find.byIcon(Icons.call_made));
      await tester.pump();
      await tester.pump();
      expect(driver.depCalls, [true, false]);
    });

    testWidgets('MySQL 表没有依赖页签', (tester) async {
      await pump(tester,
          selection: tableNode,
          driver: _DetailFakeDriver(tableDetail: tableDetail));
      expect(find.byIcon(Icons.call_made), findsNothing);
      expect(find.byIcon(Icons.call_received), findsNothing);
    });
  });
}
