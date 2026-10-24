import 'dart:io';

import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/db_types.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/data/drivers/kv_driver.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/database_info.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

// 详情面板顶部的品牌图标带「右下角连接状态点 + 离线去饱和」。
// 这条状态必须按驱动链分流取:Redis 等缓存型引擎的驱动在 kvManager 里,
// 只读 SQL 侧会让开着库的连接恒判「未连接」—— 界面显示成灰底 + 灰点。
// 本用例钉住两侧都取对,并钉住「SQL 侧没被改坏」。

ConnectionInfo _conn({required String name, required String typeId}) =>
    ConnectionInfo(
      name: name,
      typeId: typeId,
      host: '127.0.0.1',
      port: '6379',
      username: 'default',
      isLive: true,
    );

/// 只回答「连上了没有」,其余方法面板碰不到(走 noSuchMethod)。
class _KvFake implements KvStoreDriver {
  @override
  bool get isConnected => true;

  @override
  dynamic noSuchMethod(Invocation invocation) => null;
}

class _SqlFake implements DatabaseDriver {
  @override
  bool get isConnected => true;

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

  Future<DbTypeIcon> pump(
    WidgetTester tester, {
    required ConnectionInfo conn,
    KvStoreDriver? kvDriver,
    DatabaseDriver? sqlDriver,
  }) async {
    final app = AppState()..addConnection(conn);
    if (kvDriver != null) app.kvManager.attachDriverForTest(conn.name, kvDriver);
    if (sqlDriver != null) {
      app.connectionManager.attachDriverForTest(conn.name, sqlDriver);
    }
    app.detailSelection.value =
        SelectedNode(NodeKind.connection, conn.name, connection: conn.name);
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
    await tester.pump();
    return tester.widget<DbTypeIcon>(find.byType(DbTypeIcon));
  }

  testWidgets('Redis 连接已打开:状态点为「在线」,品牌色不去饱和', (tester) async {
    final icon = await pump(
      tester,
      conn: _conn(name: 'redis-open', typeId: 'redis'),
      kvDriver: _KvFake(),
    );
    expect(icon.connected, isTrue);
  });

  testWidgets('Redis 连接未打开:状态点为「离线」', (tester) async {
    final icon = await pump(
      tester,
      conn: _conn(name: 'redis-closed', typeId: 'redis'),
    );
    expect(icon.connected, isFalse);
  });

  testWidgets('SQL 连接仍按 connectionManager 判定(未被 KV 分支改坏)',
      (tester) async {
    final open = await pump(
      tester,
      conn: _conn(name: 'mysql-open', typeId: 'mysql'),
      sqlDriver: _SqlFake(),
    );
    expect(open.connected, isTrue);

    await tester.pumpWidget(const SizedBox.shrink());
    final closed = await pump(
      tester,
      conn: _conn(name: 'mysql-closed', typeId: 'mysql'),
    );
    expect(closed.connected, isFalse);
  });
}
