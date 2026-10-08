import 'dart:io';

import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/main.dart';
import 'package:daro/theme/app_theme.dart' show Tokens;
import 'package:daro/widgets/database_tree.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

import 'pin_system_locale.dart';

// 新增连接的默认选中与滚动定位:连接向导确定 / 复制连接后,连接树应
// 选中该连接、右侧详情面板跟随,并把节点滚入可视区(长列表远端也例外)。
// 批量导入(addConnections)刻意不触发,不在本文件覆盖范围。

ConnectionInfo _conn(String name, {String group = ''}) => ConnectionInfo(
      name: name,
      typeId: 'mysql',
      host: '10.0.0.1',
      port: '3306',
      username: 'root',
      password: 'p',
      group: group,
      isLive: true,
    );

void main() {
  late Directory dir;

  setUp(() async {
    TestWidgetsFlutterBinding.ensureInitialized();
    // AppState 每次改动都会 _persist → path_provider,没有该通道会抛
    dir = await Directory.systemTemp.createTemp('daro_tree_reveal_test');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
            const MethodChannel('plugins.flutter.io/path_provider'),
            (c) async => dir.path);
  });

  /// [seed] 在挂树之前执行:此时树还没监听 connectionAdded,
  /// 造数据不会触发定位(只有测试里的真「新增」才应触发)。
  Future<AppState> pumpApp(WidgetTester tester,
      {void Function(AppState app)? seed}) async {
    pinSystemChineseLocale(tester);
    // 视口压矮:让长列表必然需要滚动
    tester.view.physicalSize = const Size(1600, 700);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final app = AppState();
    seed?.call(app);
    await tester.pumpWidget(ChangeNotifierProvider<AppState>.value(
      value: app,
      child: const DbApp(),
    ));
    await tester.pump();
    return app;
  }

  Finder inTree(Finder matching) =>
      find.descendant(of: find.byType(DatabaseTree), matching: matching);

  Finder treeText(String text) => inTree(find.text(text));

  /// 行内 Text(排除搜索框 EditableText——搜索词与连接名相同时 find.text 会撞)
  Finder treeRowText(String text) =>
      inTree(find.byWidgetPredicate((w) => w is Text && w.data == text));

  /// 连接树 ListView 的滚动位置(底部搜索条在 ListView 之外,不影响查找)
  ScrollPosition treeScrollPosition(WidgetTester tester) {
    final listView = find.descendant(
        of: find.byType(DatabaseTree), matching: find.byType(ListView));
    final scrollable =
        find.descendant(of: listView, matching: find.byType(Scrollable)).first;
    return tester.state<ScrollableState>(scrollable).position;
  }

  testWidgets('新增连接默认选中:行高亮 + 详情面板跟随', (tester) async {
    final app = await pumpApp(tester, seed: (app) => app.addConnection(_conn('甲')));

    app.addConnection(_conn('乙'));
    await tester.pump();

    expect(app.detailSelection.value?.kind, NodeKind.connection);
    expect(app.detailSelection.value?.name, '乙');
    expect(app.detailSelection.value?.connection, '乙');

    // 行选中底色
    final rowContext = tester.element(treeText('乙'));
    final container = rowContext.findAncestorWidgetOfExactType<Container>()!;
    final t = Tokens.of(rowContext);
    expect((container.decoration as BoxDecoration).color, t.treeSelectedBg);
  });

  testWidgets('长列表远端新增连接:自动滚动把节点滚入可视区', (tester) async {
    final app = await pumpApp(tester, seed: (app) {
      for (var i = 1; i <= 40; i++) {
        app.addConnection(_conn('连接 ${i.toString().padLeft(2, '0')}'));
      }
    });

    expect(treeScrollPosition(tester).pixels, 0.0, reason: '前置:初始停在顶部');
    expect(treeText('新连接'), findsNothing, reason: '前置:新节点远在视口外,行未构建');

    app.addConnection(_conn('新连接'));
    await tester.pump(); // 构建带定位 key 的行
    await tester.pump(); // post-frame 估算跳转生效
    await tester.pump(); // 第二段精确定位生效

    expect(treeScrollPosition(tester).pixels, greaterThan(0),
        reason: '应向下滚动去接新节点');

    final treeRect = tester.getRect(find.byType(DatabaseTree));
    final rowRect = tester.getRect(treeText('新连接'));
    expect(rowRect.top, greaterThanOrEqualTo(treeRect.top),
        reason: '行不得滚出视口上方');
    // 底部还有 32px 的搜索条,行必须完整落在其上方(EnsureVisible:行底对齐视口底)
    expect(rowRect.bottom, lessThanOrEqualTo(treeRect.bottom - 31),
        reason: '行必须完整落在树可视区内');
  });

  testWidgets('重名避让:定位到追加序号后的最终名', (tester) async {
    final app = await pumpApp(tester, seed: (app) => app.addConnection(_conn('甲')));

    app.addConnection(_conn('甲'));
    await tester.pump();
    await tester.pump();

    expect(app.connections.last.name, '甲 (2)');
    expect(app.detailSelection.value?.connection, '甲 (2)');
    expect(treeText('甲 (2)'), findsOneWidget);
  });

  testWidgets('复制连接同样选中并定位', (tester) async {
    final app = await pumpApp(tester, seed: (app) => app.addConnection(_conn('甲')));

    app.copyConnection(_conn('甲'));
    await tester.pump();

    expect(app.detailSelection.value?.connection, '甲 (2)');
  });

  testWidgets('新增连接所在分组处于折叠态:先展开再定位', (tester) async {
    final app = await pumpApp(tester, seed: (app) {
      app.addConnection(_conn('甲', group: '生产'));
      app.addConnection(_conn('乙', group: '生产'));
    });

    // 点分组行左侧箭头热区(depth 0:缩进 4 + 箭头 20,箭头中心在行左 +14;
    // 文本左侧依次是 20 箭头 + 4 间隙 + 16 图标 + 6 间隙,故 -36 落在箭头内)
    final topLeft = tester.getTopLeft(treeText('生产'));
    await tester.tapAt(topLeft + const Offset(-36, 10));
    await tester.pump();
    expect(treeText('乙'), findsNothing, reason: '前置:分组已折叠');

    app.addConnection(_conn('丙', group: '生产'));
    await tester.pump();

    expect(treeText('丙'), findsOneWidget, reason: '折叠的分组应随新增自动展开');
    expect(treeText('乙'), findsOneWidget);
    expect(app.detailSelection.value?.connection, '丙');
  });

  testWidgets('关闭筛选(×)后:滚回当前操作的连接位置', (tester) async {
    final app = await pumpApp(tester, seed: (app) {
      for (var i = 1; i <= 40; i++) {
        app.addConnection(_conn('连接 ${i.toString().padLeft(2, '0')}'));
      }
    });

    // 搜索「连接 40」→ 列表只剩它,点击选中
    final searchField = find.descendant(
        of: find.byType(DatabaseTree), matching: find.byType(TextField));
    await tester.enterText(searchField, '连接 40');
    await tester.pump();
    expect(treeText('连接 01'), findsNothing, reason: '前置:筛选已生效');
    await tester.tap(treeRowText('连接 40'));
    await tester.pump();
    expect(app.detailSelection.value?.connection, '连接 40', reason: '前置:已选中');

    // 点搜索框的 × 清除筛选 → 全量列表回顶,应滚回「连接 40」
    await tester.tap(find.descendant(
        of: find.byType(DatabaseTree), matching: find.byIcon(Icons.close)));
    await tester.pump(); // 清除筛选 + 登记定位
    await tester.pump(); // post-frame 估算跳转生效
    await tester.pump(); // 第二段精确定位生效

    expect(app.treeSearchText, isEmpty, reason: '筛选已清除');
    // 滚动定位生效后,视口停在列表尾部:列表头的「连接 01」已懒加载出视口,
    // 用目标行的邻居「连接 39」证明全量列表回归
    expect(treeRowText('连接 39'), findsOneWidget, reason: '全量列表回归');
    expect(treeScrollPosition(tester).pixels, greaterThan(0),
        reason: '应滚动回当前操作的连接位置');

    final treeRect = tester.getRect(find.byType(DatabaseTree));
    final rowRect = tester.getRect(treeRowText('连接 40'));
    expect(rowRect.top, greaterThanOrEqualTo(treeRect.top));
    expect(rowRect.bottom, lessThanOrEqualTo(treeRect.bottom - 31),
        reason: '操作的连接必须完整落在树可视区内');
  });

  testWidgets('选中为空时清除筛选:不滚动也不报错', (tester) async {
    await pumpApp(tester, seed: (app) {
      for (var i = 1; i <= 40; i++) {
        app.addConnection(_conn('连接 ${i.toString().padLeft(2, '0')}'));
      }
    });

    final searchField = find.descendant(
        of: find.byType(DatabaseTree), matching: find.byType(TextField));
    await tester.enterText(searchField, '连接 40');
    await tester.pump();
    // 不选中任何节点,直接清除
    await tester.tap(find.descendant(
        of: find.byType(DatabaseTree), matching: find.byIcon(Icons.close)));
    await tester.pump();
    await tester.pump();

    expect(treeText('连接 01'), findsOneWidget);
    expect(treeScrollPosition(tester).pixels, 0.0, reason: '无选中,不应滚动');
  });
}
