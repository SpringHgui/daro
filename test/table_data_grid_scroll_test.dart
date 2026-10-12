import 'dart:io';

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/table_data_page.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

import 'mcp_fakes.dart';

/// 表数据网格的双向滚动(宿主侧)。
///
/// 两条都容易静默回归:
/// * 纵向条挂在横向滚动区**内部**时画在内容右缘,宽表一横向滚动整条滑出视口
///   —— 表现成「没有纵向滚动条」;
/// * 新增行落在页末,不滚就看不见,而行数与视口高度都要晚一帧才反映到
///   ScrollPosition(底部确认条被顶出来,网格矮一行)。
///
/// 驱动走 `test/mcp_fakes.dart` 的 FakeDriver:只给一页固定数据。

const _conn = ConnectionInfo(
  name: 'c',
  typeId: 'mysql',
  host: 'h',
  port: '3306',
  username: 'u',
  isLive: true,
);

/// 12 列 × 150 + 行号列 22 = 1822 px,远宽于测试视口(800 px)
const _columns = [
  'c0', 'c1', 'c2', 'c3', 'c4', 'c5', //
  'c6', 'c7', 'c8', 'c9', 'c10', 'c11',
];

List<String> _row(int r) => [for (var c = 0; c < _columns.length; c++) 'r$r-c$c'];

Future<void> _pumpPage(WidgetTester tester, FakeDriver driver) async {
  final app = AppState();
  app.addConnections([_conn]);
  app.connectionManager.attachDriverForTest('c', driver);
  await driver.connect();

  await tester.pumpWidget(
    ChangeNotifierProvider<AppState>.value(
      value: app,
      child: MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: kAppLocalizationsDelegates,
        supportedLocales: kSupportedLocales,
        // 与 main.dart 同构:Input 内的 TextField 需要 Material 祖先
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
  // 首页要等 previewTable + describeTable 两跳,跑到网格出现为止
  for (var i = 0; i < 10 && find.byType(DataGridView).evaluate().isEmpty; i++) {
    await _settle(tester);
  }
  expect(find.byType(DataGridView), findsOneWidget,
      reason: '假驱动没能加载出数据,网格不会渲染');
}

/// 跑固定几帧代替 pumpAndSettle:光标闪烁 / 加载指示是持续动画,
/// pumpAndSettle 会一直等到超时
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 8; i++) {
    await tester.pump(const Duration(milliseconds: 60));
  }
}

Finder _vScrollBar() => find.byWidgetPredicate(
    (w) => w is ScrollBar && w.orientation == ScrollBarOrientation.vertical);

ScrollPosition _vPosition(WidgetTester tester) => tester
    .widget<DataGridView>(find.byType(DataGridView))
    .verticalScrollController!
    .position;

/// 40 行 × 28px = 1120px,高于测试视口可分配的网格高度
List<List<String>> _tallPage() => [for (var r = 0; r < 40; r++) _row(r)];

void main() {
  late Directory supportDir;

  setUp(() async {
    // AppState 会异步落盘:指向临时目录,避免污染用户目录
    supportDir = await Directory.systemTemp.createTemp('daro_grid_scroll');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => supportDir.path,
    );
  });

  tearDown(() async => supportDir.delete(recursive: true));

  FakeDriver _driver(List<List<String>> rows) => FakeDriver('c')
    ..preview = TablePreview(columns: _columns, rows: rows, limit: 100);

  group('纵向滚动条', () {
    testWidgets('贴在视口右缘,不被横向滚动带出屏幕', (tester) async {
      await _pumpPage(tester, _driver(_tallPage()));

      expect(_vScrollBar(), findsOneWidget, reason: '页面没有纵向滚动条');
      // 视口宽 800:条挂在横向滚动区内部时它的右缘 = 内容宽 1822,整条在屏外
      expect(tester.getRect(_vScrollBar()).right, 800);
      expect(
        _vPosition(tester).maxScrollExtent,
        greaterThan(0),
        reason: '前提:本页确实高过视口,有可滚的范围',
      );
    });
  });

  group('窗口变宽', () {
    testWidgets('最大化后右侧列要跟着显形,不能留白', (tester) async {
      await _pumpPage(tester, _driver(_tallPage()));
      addTearDown(tester.view.reset);

      expect(find.text('r0-c11'), findsNothing, reason: '前提:800 宽时末列在视口外');

      // 整窗内容宽 1822,放大到 2400 逻辑像素后全部列都该可见
      tester.view.devicePixelRatio = 1.0;
      tester.view.physicalSize = const Size(2400, 600);
      await _settle(tester);

      expect(find.text('r0-c11'), findsOneWidget,
          reason: '视口变宽只走布局不触发重建,横向虚拟化的列窗口没跟上,'
              '右侧列一直空白(表头却是整行渲染的)');
    });
  });

  group('新增行', () {
    testWidgets('自动滚到新行,选中箭头落在可视区内', (tester) async {
      await _pumpPage(tester, _driver(_tallPage()));
      final viewport = tester.getRect(_vScrollBar());

      expect(_vPosition(tester).pixels, 0);
      expect(find.text('r39-c0'), findsNothing, reason: '前提:末行在视口之外');

      await tester.tap(find.byIcon(Icons.add).first,
          kind: PointerDeviceKind.mouse);
      await _settle(tester);

      final pos = _vPosition(tester);
      expect(pos.pixels, pos.maxScrollExtent,
          reason: '新行追加在页末,应当滚到底');
      // 行号列的指向箭头即当前选中行(新行)
      final arrow = find.byIcon(Icons.play_arrow);
      expect(arrow, findsOneWidget);
      final arrowY = tester.getCenter(arrow).dy;
      expect(arrowY, inInclusiveRange(viewport.top, viewport.bottom),
          reason: '新增行没滚进可视区');
    });
  });
}
