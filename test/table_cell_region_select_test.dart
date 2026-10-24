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

/// 表数据页的单元格多选 / 框选 / 全选 + 右键目标 + F5 / 应用筛选(宿主侧)。
///
/// 组件层的矩形算法、滚动偏移、单击 vs 拖拽、右键落点的选中同步由 base-ui 的
/// `data_grid_region_select_test.dart` 与 `data_grid_context_select_test.dart`
/// 覆盖;这里只验宿主独有的事:
/// 选中集合按**数据列**存(隐藏列后不漂移)、Ctrl+A 覆盖本页全部可见格、
/// 区域复制的 Tab / 换行格式、Del 整批置 NULL 且状态栏按覆盖行数报数、
/// 右键菜单在编辑态让位给单元格编辑器的文本菜单、菜单文案随落点单格 / 整批切换、
/// F5 走一次取数、只键入 WHERE 文本也能让「应用」按钮立刻可点。
///
/// 驱动走 `test/mcp_fakes.dart` 的 FakeDriver:只给一页固定数据,
/// 不校验服务端 SQL(那是驱动层用例的事)。

const _conn = ConnectionInfo(
  name: 'c',
  typeId: 'mysql',
  host: 'h',
  port: '3306',
  username: 'u',
  isLive: true,
);

const _columns = ['id', 'name', 'addr', 'memo'];

/// r{行}-c{列},全页唯一,便于按文本定位单元格
List<String> _row(int r) => [
      for (var c = 0; c < _columns.length; c++) 'r$r-c$c',
    ];

/// 页面上报状态栏用的 tab key(与 TableDataPage._tabKey 同构)
final _tabKey = OpenTab(TabType.table, 't1', null, 'c', 'bfin', null).key;

Future<AppState> _pumpPage(WidgetTester tester, FakeDriver driver) async {
  // 工具条 + 三栏在默认 800×600 测试视口下会横向溢出,手势落点跟着越界
  tester.view.physicalSize = const Size(1600, 1000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

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
  return app;
}

/// 跑固定几帧代替 pumpAndSettle:光标闪烁 / 加载指示是持续动画,
/// pumpAndSettle 会一直等到超时
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump(const Duration(milliseconds: 60));
  }
}

/// 拖拽框选:从 [from] 格中心按下,拖到 [to] 格中心抬起
Future<void> _boxSelect(WidgetTester tester, String from, String to) async {
  final gesture = await tester.startGesture(
    tester.getCenter(find.text(from)),
    kind: PointerDeviceKind.mouse,
  );
  await tester.pump();
  await gesture.moveTo(tester.getCenter(find.text(to)));
  await tester.pump();
  await gesture.up();
  await tester.pump();
}

Future<void> _hotkey(WidgetTester tester, LogicalKeyboardKey key) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
  await tester.sendKeyDownEvent(key);
  await tester.sendKeyUpEvent(key);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
  await tester.pump();
}

/// 网格当前拿到的选中集合:宿主内部存**数据列**,这里是换算后的网格列
Set<(int, int)>? _selectedGridCells(WidgetTester tester) =>
    tester.widget<DataGridView>(find.byType(DataGridView)).selectedCells;

DataGridView _grid(WidgetTester tester) =>
    tester.widget<DataGridView>(find.byType(DataGridView));

/// 「应用筛选 & 排序」主按钮:禁用态就是 `onPressed == null`
final _applyButtonFinder =
    find.ancestor(of: find.text('应用筛选 & 排序'), matching: find.byType(Button));

Button _applyButton(WidgetTester tester) =>
    tester.widget<Button>(_applyButtonFinder);

/// 只用来数取数次数的假驱动(F5 刷新的回归目标)
class _CountingDriver extends FakeDriver {
  _CountingDriver(super.label);

  int previewCount = 0;

  @override
  Future<TablePreview> previewTable(
    String database,
    String table, {
    int limit = 100,
    int offset = 0,
    String? schema,
    String? where,
    String? orderBy,
  }) {
    previewCount++;
    return super.previewTable(database, table,
        limit: limit,
        offset: offset,
        schema: schema,
        where: where,
        orderBy: orderBy);
  }
}

/// 最近一次 `Clipboard.setData` 写入的文本
String? _clipText;

/// 截获剪贴板写入:Flutter 的 `Clipboard.setData` 走 platform 通道,
/// 装一个 mock handler 才能拿到实际复制出去的文本(而不是只测状态栏提示)
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

/// 展开「列」工具面板(工具标签行上的开关)
Future<void> _openColumnPanel(WidgetTester tester) async {
  await tester.tap(find.text('列').first, kind: PointerDeviceKind.mouse);
  await _settle(tester);
  expect(find.byType(CheckBox), findsWidgets, reason: '列面板没展开');
}

void main() {
  late Directory supportDir;

  setUp(() async {
    // AppState 会异步落盘:指向临时目录,避免污染用户目录
    supportDir = await Directory.systemTemp.createTemp('daro_cell_select');
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

  FakeDriver _driver(List<List<String>> rows) => FakeDriver('c')
    ..preview = TablePreview(
      columns: _columns,
      rows: rows,
      limit: 100,
    );

  group('框选', () {
    testWidgets('拖过的格全部进选中集合,且不开就地编辑器', (tester) async {
      await _pumpPage(tester, _driver([_row(0), _row(1), _row(2)]));

      await _boxSelect(tester, 'r0-c0', 'r1-c1');

      expect(_selectedGridCells(tester), {(0, 0), (0, 1), (1, 0), (1, 1)});
      expect(
        _grid(tester).editingCell,
        isNull,
        reason: '框选不算单击,不应弹编辑器(否则会抢焦点打断拖拽)',
      );
    });

    testWidgets('单击仍然即编辑(与框选共存)', (tester) async {
      await _pumpPage(tester, _driver([_row(0), _row(1)]));

      await tester.tap(find.text('r0-c0'), kind: PointerDeviceKind.mouse);
      await tester.pump();

      expect(_selectedGridCells(tester), {(0, 0)});
      expect(_grid(tester).editingCell, (0, 0));
    });

    testWidgets('Shift 从锚点连选成矩形,锚点不被移动', (tester) async {
      await _pumpPage(tester, _driver([_row(0), _row(1), _row(2)]));

      await _boxSelect(tester, 'r1-c1', 'r1-c1'); // 落锚点 (1,1)
      final anchor = _grid(tester).anchorCell;
      expect(anchor, (1, 1));

      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await _boxSelect(tester, 'r0-c0', 'r0-c0');
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.pump();

      expect(_selectedGridCells(tester),
          {(0, 0), (0, 1), (1, 0), (1, 1)},
          reason: 'Shift 应把锚点到点击格之间的矩形并入选区');
      expect(_grid(tester).anchorCell, anchor, reason: '连选时活动格不变(Excel 语义)');
    });
  });

  group('全选', () {
    testWidgets('Ctrl+A 覆盖本页全部可见格', (tester) async {
      final app = await _pumpPage(tester, _driver([_row(0), _row(1), _row(2)]));

      // 先横向拖两格:既把焦点落到页面上,又不会弹编辑器
      // (单击会起编辑器,而编辑器里的 Ctrl+A 归文本框自己)
      await _boxSelect(tester, 'r0-c0', 'r0-c1');
      await _hotkey(tester, LogicalKeyboardKey.keyA);

      expect(_selectedGridCells(tester)?.length, 3 * _columns.length);
      expect(_grid(tester).anchorCell, (0, 0));
      expect(
        app.tableStatusFor(_tabKey)?.selectedRowCount,
        3,
        reason: '状态栏按区域覆盖的行数报「已选 N 行」',
      );
    });

    testWidgets('隐藏列后全选只落可见列,坐标不漂移', (tester) async {
      await _pumpPage(tester, _driver([_row(0), _row(1)]));
      _watchClipboard(tester);

      // 列面板第一行 = id 列,取消勾选即隐藏
      await _openColumnPanel(tester);
      await tester.tap(find.byType(CheckBox).first, kind: PointerDeviceKind.mouse);
      await _settle(tester);

      expect(_grid(tester).columns.map((c) => c.title), _columns.sublist(1));

      await _boxSelect(tester, 'r0-c1', 'r1-c2');
      // 网格列 0/1 对应数据列 1/2:复制出来必须是 name / addr 的值
      expect(_selectedGridCells(tester), {(0, 0), (0, 1), (1, 0), (1, 1)});

      await _hotkey(tester, LogicalKeyboardKey.keyC);
      await tester.pump();
      expect(
        _clipText,
        'r0-c1\tr0-c2\nr1-c1\tr1-c2',
        reason: '选中集合按数据列存,隐藏列不会把坐标带偏',
      );
    });
  });

  group('区域动作', () {
    testWidgets('Ctrl+C 按行分组、同行列升序,Tab + 换行', (tester) async {
      await _pumpPage(tester, _driver([_row(0), _row(1), _row(2)]));
      _watchClipboard(tester);

      await _boxSelect(tester, 'r0-c1', 'r2-c2');
      await _hotkey(tester, LogicalKeyboardKey.keyC);
      await tester.pump();

      expect(_clipText,
          'r0-c1\tr0-c2\nr1-c1\tr1-c2\nr2-c1\tr2-c2');
    });

    testWidgets('Del 把区域内每格置 NULL,无需逐格确认', (tester) async {
      final app = await _pumpPage(tester, _driver([_row(0), _row(1)]));

      await _boxSelect(tester, 'r0-c0', 'r1-c1');
      await tester.sendKeyEvent(LogicalKeyboardKey.delete);
      await tester.pump();

      expect(find.text('NULL'), findsNWidgets(4));
      expect(find.text('r0-c0'), findsNothing);
      expect(find.byType(DataGridView), findsOneWidget, reason: '不应弹确认框');
      expect(app.tableStatusFor(_tabKey)?.selectedRowCount, 2);
    });
  });

  group('右键', () {
    /// 网格里的那一格:单元格编辑器打开后,右侧「单元格编辑器」面板会显示
    /// 同一个值,只有网格内的落点才是右键目标
    Finder _cell(String value) => find.descendant(
        of: find.byType(DataGridView), matching: find.text(value));

    Future<void> rightClick(WidgetTester tester, String value) async {
      final gesture = await tester.startGesture(
        tester.getCenter(_cell(value)),
        kind: PointerDeviceKind.mouse,
        buttons: kSecondaryButton,
      );
      await gesture.up();
      await _settle(tester);
    }

    testWidgets('编辑态右键不再叠一层单元格菜单(交给编辑器的文本菜单)',
        (tester) async {
      await _pumpPage(tester, _driver([_row(0), _row(1)]));

      await tester.tap(_cell('r0-c0'), kind: PointerDeviceKind.mouse);
      await tester.pump();
      expect(_grid(tester).editingCell, (0, 0));

      await rightClick(tester, 'r0-c0');
      expect(find.text('设置为空白字符串'), findsNothing);
    });

    testWidgets('未编辑时右键照旧弹单元格菜单', (tester) async {
      await _pumpPage(tester, _driver([_row(0), _row(1)]));

      await rightClick(tester, 'r1-c1');
      expect(find.text('设置为空白字符串'), findsOneWidget);
    });

    testWidgets('右键落在单格上:菜单给「复制单元格」,复制的就是那一格',
        (tester) async {
      _watchClipboard(tester);
      await _pumpPage(tester, _driver([_row(0), _row(1)]));

      await rightClick(tester, 'r1-c1');
      expect(find.text('复制单元格'), findsOneWidget);

      await tester.tap(find.text('复制单元格'));
      await _settle(tester);
      expect(_clipText, 'r1-c1');
    });

    testWidgets('右键落在区域外:目标收缩为该格,菜单文案跟着变单数',
        (tester) async {
      await _pumpPage(tester, _driver([_row(0), _row(1)]));

      await _boxSelect(tester, 'r0-c0', 'r1-c1');
      expect(_selectedGridCells(tester), isNotNull);

      await rightClick(tester, 'r0-c3');

      expect(_selectedGridCells(tester), {(0, 3)});
      expect(find.text('复制单元格'), findsOneWidget);
      expect(find.text('复制 4 个单元格'), findsNothing);
    });

    testWidgets('右键落在选区内:整批保留,不给单格项', (tester) async {
      await _pumpPage(tester, _driver([_row(0), _row(1)]));

      await _boxSelect(tester, 'r0-c0', 'r1-c1');
      await rightClick(tester, 'r0-c0');

      expect(_selectedGridCells(tester), {(0, 0), (0, 1), (1, 0), (1, 1)},
          reason: '区域内右键不该把框选拆成单格');
      expect(find.text('复制 4 个单元格'), findsOneWidget);
      expect(find.text('复制单元格'), findsNothing);
    });
  });

  group('F5 刷新 与「应用筛选」按钮', () {
    testWidgets('F5 重新取当前页数据(有未保存修改时先确认,这里没有)', (tester) async {
      final driver = _CountingDriver('c')
        ..preview = TablePreview(
          columns: _columns,
          rows: [_row(0), _row(1)],
          limit: 100,
        );
      await _pumpPage(tester, driver);
      final before = driver.previewCount;

      await tester.sendKeyEvent(LogicalKeyboardKey.f5);
      await _settle(tester);

      expect(driver.previewCount, greaterThan(before),
          reason: 'F5 应走一次取数');
      expect(find.text('设置为空白字符串'), findsNothing,
          reason: '刷新不该带出任何弹窗 / 菜单');
    });

    testWidgets('只键入 WHERE 文本也要让「应用筛选 & 排序」立刻可点', (tester) async {
      await _pumpPage(tester, _driver([_row(0), _row(1)]));

      await tester.tap(find.text('筛选 & 排序'));
      await _settle(tester);
      // 切到「文本」模式并先应用掉:此后草稿 == 已应用,按钮必须是灰的
      await tester.tap(find.text('文本').last);
      await _settle(tester);
      await tester.tap(_applyButtonFinder);
      await _settle(tester);
      expect(_applyButton(tester).onPressed, isNull,
          reason: '草稿与已应用一致时不该可点');

      // 这一步只有草稿信号、没有整页 setState —— 旧版按钮没挂信号,一直是灰的
      await tester.enterText(find.byType(Textarea).last, 'id > 1');
      await _settle(tester);

      expect(_applyButton(tester).onPressed, isNotNull,
          reason: '改了条件就必须让用户看得见能应用');
    });
  });
}
