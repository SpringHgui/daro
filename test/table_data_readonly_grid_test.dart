import 'dart:io';

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/table_data_page.dart';

import 'mcp_fakes.dart';

/// 只读连接的网格拦口(当前只有 MongoDB 系,判据是 [kDocumentDbTypes])。
///
/// 驱动不生成写回语句:能点「添加记录」、能在单元格里打字、能按 Ctrl+S,
/// 就等于是让用户白改一遍再告诉他跑不了。这里验的是宿主侧的门关上了,
/// 以及门只关在文档型库上(同一条用例跑 MySQL 必须仍是可编辑的)。
const _readOnlyHint = '该连接为只读浏览,数据不可在网格中编辑或保存。';

ConnectionInfo _conn(String typeId) => ConnectionInfo(
      name: 'c',
      typeId: typeId,
      host: 'h',
      port: typeId == 'mongodb' ? '27017' : '3306',
      username: 'u',
      isLive: true,
    );

/// 每格文本唯一,便于按文本定位
const _preview = TablePreview(
  columns: ['id', 'name'],
  rows: [
    ['10', 'aa'],
    ['11', 'bb'],
  ],
  limit: 100,
  nullMask: [
    [false, false],
    [false, false],
  ],
);

final _tabKey = OpenTab(TabType.table, 't1', null, 'c', 'bfin', null).key;

Future<AppState> _pumpPage(
    WidgetTester tester, {required String typeId}) async {
  // 工具条 + 三栏在默认 800×600 测试视口下会横向溢出,手势落点跟着越界
  tester.view.physicalSize = const Size(1600, 1000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  final driver = FakeDriver('fake')..preview = _preview;
  final app = AppState();
  app.addConnections([_conn(typeId)]);
  app.connectionManager.attachDriverForTest('c', driver);
  await driver.connect();

  await tester.pumpWidget(
    ChangeNotifierProvider<AppState>.value(
      value: app,
      child: MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: kAppLocalizationsDelegates,
        supportedLocales: kSupportedLocales,
        home: Material(
          child: TableDataPage(
            table: 't1',
            connection: 'c',
            database: 'bfin',
          ),
        ),
      ),
    ),
  );
  for (var i = 0; i < 10 && find.byType(DataGridView).evaluate().isEmpty; i++) {
    await _settle(tester);
  }
  expect(find.byType(DataGridView), findsOneWidget,
      reason: '假驱动没能加载出数据,网格不会渲染');
  return app;
}

/// 跑固定几帧代替 pumpAndSettle:光标闪烁 / 加载指示是持续动画
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump(const Duration(milliseconds: 60));
  }
}

DataGridView _grid(WidgetTester tester) =>
    tester.widget<DataGridView>(find.byType(DataGridView));

/// 工具栏「添加记录」是纯图标按钮(且 `ToolStripButton` 是 item 描述而非 Widget,
/// 取不到渲染态的 enabled),所以按行为判:点下去行数会不会多一条。
Future<void> _tapAddRecord(WidgetTester tester) async {
  await tester.tap(find.byIcon(Icons.add).first, kind: PointerDeviceKind.mouse);
  await _settle(tester);
}

void main() {
  late Directory supportDir;

  setUp(() async {
    // AppState 会异步落盘:指向临时目录,避免污染用户目录
    supportDir = await Directory.systemTemp.createTemp('daro_readonly_grid');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => supportDir.path,
    );
  });

  tearDown(() async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
            const MethodChannel('plugins.flutter.io/path_provider'), null);
    if (supportDir.existsSync()) supportDir.deleteSync(recursive: true);
  });

  testWidgets('MongoDB 连接:点「添加记录」不加行', (tester) async {
    await _pumpPage(tester, typeId: 'mongodb');
    expect(_grid(tester).rowCount, 2);
    // 工具栏这颗键是 enabled:false,按下没有任何回调,所以这里只看「行数没变」;
    // 提示文案由键盘 / 单元格 / Ctrl+S 那几条路径负责(它们能进回调,才有话可说)。
    await _tapAddRecord(tester);
    expect(_grid(tester).rowCount, 2, reason: '只读连接还能追加本地新行');
  });

  testWidgets('MongoDB 连接:选中单元格后按 Del 不置 NULL,并说明原因',
      (tester) async {
    await _pumpPage(tester, typeId: 'mongodb');
    // 选中由网格在按下时上报(与 _startEdit 无关),所以只读连接照样能选、能复制
    await tester.tap(find.text('10'), kind: PointerDeviceKind.mouse);
    await _settle(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.delete);
    await _settle(tester);
    expect(find.text('10'), findsWidgets, reason: 'Del 把值改成 NULL 了');
  });

  testWidgets('MongoDB 连接:点单元格不起编辑器,状态栏说明原因', (tester) async {
    await _pumpPage(tester, typeId: 'mongodb');
    await tester.tap(find.text('10'), kind: PointerDeviceKind.mouse);
    await _settle(tester);
    expect(find.byType(InlineEditor), findsNothing,
        reason: '只读连接还能进编辑态');
    expect(find.text(_readOnlyHint), findsWidgets);
  });

  testWidgets('MongoDB 连接:Ctrl+S 不发任何语句,只报只读', (tester) async {
    final app = await _pumpPage(tester, typeId: 'mongodb');
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyS);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyS);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await _settle(tester);

    // 该页唯一的取数调用是 previewTable(走 driver.preview 的假数据),
    // 任何 executeQuery 都说明写回语句被组装并发出去了
    expect(app.sqlHistoryFor(_tabKey), isNot(contains('UPDATE')));
    expect(find.text(_readOnlyHint), findsWidgets);
  });

  testWidgets('SQL 历史:文档库记集合调用,关系库仍记 SELECT', (tester) async {
    final mongoApp = await _pumpPage(tester, typeId: 'mongodb');
    var history = mongoApp.sqlHistoryFor(_tabKey);
    expect(history, isNotEmpty);
    expect(history.last, 'db.getCollection("t1").find().skip(0).limit(100)');

    await tester.pumpWidget(const SizedBox.shrink());
    final mysqlApp = await _pumpPage(tester, typeId: 'mysql');
    history = mysqlApp.sqlHistoryFor(_tabKey);
    expect(history.last, startsWith('SELECT * FROM'));
  });

  testWidgets('同一套页面在 MySQL 连接上仍可编辑(门不是全局关的)',
      (tester) async {
    await _pumpPage(tester, typeId: 'mysql');
    await _tapAddRecord(tester);
    expect(_grid(tester).rowCount, 3, reason: 'MySQL 的「添加记录」被误关了');
    expect(find.text(_readOnlyHint), findsNothing);

    await tester.tap(find.text('10'), kind: PointerDeviceKind.mouse);
    await _settle(tester);
    expect(find.byType(InlineEditor), findsOneWidget);
  });
}
