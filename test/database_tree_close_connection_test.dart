import 'dart:io';

import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/data/drivers/kv_driver.dart';
import 'package:daro/main.dart';
import 'package:daro/widgets/database_tree.dart';
import 'package:flutter/gestures.dart' show kSecondaryButton;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

import 'pin_system_locale.dart';

// 连接节点右键菜单的「关闭连接」对三类引擎都要可用。
// 菜单此前只读 ConnectionManager 的状态,于是走独立驱动链的 Redis 恒显示
// 「打开连接」(永远关不掉),SQL 侧的 MongoDB 则只看驱动是否真已建立。

const _redis = ConnectionInfo(
  name: '缓存',
  typeId: 'redis',
  username: '',
  host: '127.0.0.1',
  port: '6379',
  isLive: true,
);

const _mongo = ConnectionInfo(
  name: '文档库',
  typeId: 'mongodb',
  username: '',
  host: '127.0.0.1',
  port: '27017',
  isLive: true,
);

const _mysql = ConnectionInfo(
  name: '关系库',
  typeId: 'mysql',
  host: '127.0.0.1',
  port: '3306',
  username: 'root',
  password: 'p',
  isLive: true,
);

/// 只关心「连着 / 断开」的 KV 假驱动
class _FakeKvDriver implements KvStoreDriver {

  bool connected = true;
  int closeCalls = 0;

  @override
  bool get isConnected => connected;

  @override
  Future<void> connect() async => connected = true;

  @override
  Future<void> close() async {
    closeCalls++;
    connected = false;
  }

  @override
  KvCapabilities get capabilities => const KvCapabilities(
        multipleNamespaces: true,
        cursorScan: true,
        keyTypes: {},
        writableTypes: {},
        ttl: false,
        internalEncoding: false,
        memoryUsage: false,
        commandConsole: false,
      );

  @override
  Future<List<KvNamespace>> listNamespaces() async =>
      const [KvNamespace(id: '0', label: 'db0')];

  @override
  dynamic noSuchMethod(Invocation invocation) => null;
}

/// SQL 侧假驱动:同样只表达连接状态
class _FakeSqlDriver implements DatabaseDriver {

  bool connected = true;
  int closeCalls = 0;

  @override
  bool get isConnected => connected;

  @override
  Future<void> connect() async => connected = true;

  @override
  Future<void> close() async {
    closeCalls++;
    connected = false;
  }

  @override
  Future<List<String>> listDatabases() async => const ['d1'];

  @override
  dynamic noSuchMethod(Invocation invocation) => null;
}

void main() {
  late Directory dir;

  setUp(() async {
    TestWidgetsFlutterBinding.ensureInitialized();
    dir = await Directory.systemTemp.createTemp('daro_tree_close_test');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
            const MethodChannel('plugins.flutter.io/path_provider'),
            (c) async => dir.path);
  });

  Future<AppState> pumpApp(WidgetTester tester, ConnectionInfo conn) async {
    pinSystemChineseLocale(tester);
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final app = AppState()..addConnection(conn);
    await tester.pumpWidget(ChangeNotifierProvider<AppState>.value(
      value: app,
      child: const DbApp(),
    ));
    await tester.pump();
    return app;
  }

  /// 右键树内的连接行,断言菜单里出现 [items]
  Future<void> openMenu(WidgetTester tester, String connName) async {
    await tester.tapAt(
        tester.getCenter(find.descendant(
            of: find.byType(DatabaseTree), matching: find.text(connName))),
        buttons: kSecondaryButton);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 50));
  }

  Future<void> tapItem(WidgetTester tester, String text) async {
    await tester.tap(find.descendant(
        of: find.byType(Overlay), matching: find.text(text)));
    await tester.pump();
  }

  for (final (label, conn, isKv) in [
    ('Redis', _redis, true),
    ('MongoDB', _mongo, false),
    ('MySQL', _mysql, false),
  ]) {
    testWidgets('$label:已连接时右键菜单显示「关闭连接」,点击后驱动断开',
        (tester) async {
      final app = await pumpApp(tester, conn);
      // 注入「已连接」的驱动:等价于用户已打开过这条连接
      final kvDriver = _FakeKvDriver();
      final sqlDriver = _FakeSqlDriver();
      if (isKv) {
        app.kvManager.attachDriverForTest(conn.name, kvDriver);
        await app.kvManager.expandConnection(conn);
      } else {
        app.connectionManager.attachDriverForTest(conn.name, sqlDriver);
        await app.connectionManager.expandConnection(conn);
      }
      await tester.pump();
      expect(
          isKv
              ? app.kvManager.isConnected(conn.name)
              : app.connectionManager.isConnected(conn.name),
          isTrue,
          reason: '前置条件:$label 驱动应已建立');

      await openMenu(tester, conn.name);
      expect(find.text('关闭连接'), findsOneWidget,
          reason: '$label 已连接却只提供「打开连接」= 关不掉');
      expect(find.text('打开连接'), findsNothing);

      // 禁用项会吞掉点击(菜单不关),可用项点击后菜单收起 —— 以此区分状态
      await tapItem(tester, '刷新');
      await tester.pump();
      expect(find.text('刷新'), findsNothing,
          reason: '$label 已列出子节点,「刷新」应可用');

      if (isKv) {
        // Redis 的库数由服务端 databases 配置决定,客户端建不出新库
        await openMenu(tester, conn.name);
        await tapItem(tester, '新建数据库');
        await tester.pump();
        expect(find.text('新建数据库'), findsOneWidget,
            reason: '缓存型引擎的「新建数据库」该置灰,而不是点了报错');
      }

      await openMenu(tester, conn.name);
      await tapItem(tester, '关闭连接');
      expect(
          isKv
              ? app.kvManager.isConnected(conn.name)
              : app.connectionManager.isConnected(conn.name),
          isFalse);
      expect(isKv ? kvDriver.closeCalls : sqlDriver.closeCalls, 1,
          reason: '应当真的关掉驱动,而不是只清 UI 状态');
    });
  }

  testWidgets('未连接时右键菜单显示「打开连接」而不是「关闭连接」', (tester) async {
    await pumpApp(tester, _redis);
    await openMenu(tester, _redis.name);
    expect(find.text('打开连接'), findsOneWidget);
    expect(find.text('关闭连接'), findsNothing);
  });
}
