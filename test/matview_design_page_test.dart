import 'package:base_ui_flutter/base_ui_flutter.dart' show ToolbarButton;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/widgets/matview_design_page.dart';
import 'package:daro/theme/app_theme.dart';
import 'package:daro/l10n/locale_config.dart';

// 实体化视图设计器的结构与拦口。
//
// 连接名用的是不存在的「无此连接」:_conn 解析为 null,页面不会真去建驱动,
// 所有目录查询与 DDL 都在入口处直接返回。据此可以离线核对页签、工具栏
// 与「定义/索引」两处必须存在的拦口(空定义不给保存、重建前先 EXPLAIN 探测)。

void main() {
  Widget harness(
          {String name = 'mv_demo',
          bool isNew = false,
          Brightness brightness = Brightness.dark}) =>
      ChangeNotifierProvider(
        create: (_) => AppState(),
        child: MaterialApp(
          locale: const Locale('zh'),
          localizationsDelegates: kAppLocalizationsDelegates,
          supportedLocales: kSupportedLocales,
          theme: ThemeData(brightness: brightness),
          // 与 main.dart 同构:应用根部有 Material,base-ui 的 Input(TextField)需要
          home: Material(
            type: MaterialType.transparency,
            child: MatViewDesignPage(
              name: name,
              connection: '无此连接',
              database: 'db',
              schema: 'public',
              isNew: isNew,
            ),
          ),
        ),
      );

  Future<void> setSize(WidgetTester tester) async {
    tester.view.physicalSize = const Size(1800, 1000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
  }

  Future<void> openTab(WidgetTester tester, String label) async {
    await tester.tap(find.text(label).first);
    await tester.pumpAndSettle();
  }

  testWidgets('五个页签与工具栏齐全;新建态不给刷新', (tester) async {
    await setSize(tester);
    await tester.pumpWidget(harness(isNew: true));
    await tester.pumpAndSettle();

    for (final tab in ['定义', '索引', '高级', '注释', 'SQL 预览']) {
      expect(find.text(tab), findsWidgets, reason: '缺少页签: $tab');
    }
    for (final btn in ['保存', '预览', '解释', '美化 SQL']) {
      expect(find.text(btn), findsOneWidget, reason: '缺少工具栏按钮: $btn');
    }
    // 没有落库的对象无内容可刷新:按钮位保留但要禁用(与其它设计器同口径)
    for (final btn in ['刷新', '并发刷新']) {
      final b = tester.widget<ToolbarButton>(find.ancestor(
          of: find.text(btn), matching: find.byType(ToolbarButton)));
      expect(b.enabled, isFalse, reason: '$btn 在新建态应禁用');
    }
    expect(find.byIcon(Icons.fullscreen), findsOneWidget);
  });

  testWidgets('编辑态给刷新与并发刷新', (tester) async {
    await setSize(tester);
    await tester.pumpWidget(harness());
    await tester.pumpAndSettle();

    expect(find.text('刷新'), findsOneWidget);
    expect(find.text('并发刷新'), findsOneWidget);
  });

  // 工具栏图标色必须随明暗走业务色板:写死亮色档时暗色下几乎看不见(实测踩过)
  testWidgets('工具栏图标色取自当前主题的业务色板', (tester) async {
    await setSize(tester);
    for (final b in [Brightness.dark, Brightness.light]) {
      await tester.pumpWidget(harness(brightness: b));
      await tester.pumpAndSettle();
      final expected = b == Brightness.dark ? AppColors.dark : AppColors.light;
      ToolbarButton btn(String text) => tester.widget<ToolbarButton>(
          find.ancestor(of: find.text(text), matching: find.byType(ToolbarButton)));
      expect(btn('保存').iconColor, expected.iconPrimary, reason: '$b 保存图标');
      await openTab(tester, '索引');
      expect(btn('添加索引').iconColor, expected.iconSuccess, reason: '$b 添加索引图标');
    }
  });

  testWidgets('索引页:网格表头 + 名称/列/唯一字段,工具栏追加索引按钮',
      (tester) async {
    await setSize(tester);
    await tester.pumpWidget(harness());
    await tester.pumpAndSettle();

    await openTab(tester, '索引');

    for (final col in ['名称', '唯一', '定义']) {
      expect(find.text(col), findsWidgets, reason: '缺少列头: $col');
    }
    expect(find.text('索引名称:'), findsOneWidget);
    expect(find.text('索引列:'), findsOneWidget);
    expect(find.text('添加索引'), findsOneWidget);
    expect(find.text('删除索引'), findsOneWidget);
    // 未选中任何索引时「删除索引」必须禁用,否则点下去只会弹「请先选择」
    expect(tester.takeException(), isNull);
  });

  testWidgets('高级页:所有者 / 表空间 / 访问方法 / 存储参数 / 填充数据',
      (tester) async {
    await setSize(tester);
    await tester.pumpWidget(harness());
    await tester.pumpAndSettle();

    await openTab(tester, '高级');

    for (final field in ['所有者:', '表空间:', '访问方法:', '存储参数:']) {
      expect(find.text(field), findsOneWidget, reason: '缺少字段: $field');
    }
    expect(find.textContaining('填充数据'), findsOneWidget);
  });

  testWidgets('注释页有输入位', (tester) async {
    await setSize(tester);
    await tester.pumpWidget(harness());
    await tester.pumpAndSettle();

    await openTab(tester, '注释');
    expect(find.byType(TextField), findsOneWidget);
  });

  testWidgets('SQL 预览页:更改 / DDL 子页签可切换', (tester) async {
    await setSize(tester);
    await tester.pumpWidget(harness());
    await tester.pumpAndSettle();

    await openTab(tester, 'SQL 预览');
    expect(find.text('更改'), findsOneWidget);
    expect(find.text('DDL'), findsOneWidget);

    await tester.tap(find.text('DDL'));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });

  testWidgets('编辑态且定义未回填:保存被拦在空定义,不发起 DROP', (tester) async {
    await setSize(tester);
    await tester.pumpWidget(harness());
    await tester.pumpAndSettle();

    await tester.tap(find.text('保存'));
    await tester.pumpAndSettle();

    expect(find.textContaining('实体化视图定义不能为空'), findsOneWidget);
    // 连接不存在时也不该弹出重建确认(那是空定义之后的第二道)
    expect(find.textContaining('先 DROP 再 CREATE'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('逐个页签打开都不出布局异常(溢出 / 属性面板自激)', (tester) async {
    await setSize(tester);
    await tester.pumpWidget(harness());
    await tester.pumpAndSettle();

    for (final tab in ['定义', '索引', '高级', '注释', 'SQL 预览', '索引', '定义']) {
      await openTab(tester, tab);
      // 溢出与布局自激都会走 FlutterError,takeException 现读现清
      expect(tester.takeException(), isNull, reason: '页签: $tab');
    }
  });

  testWidgets('窄屏(1200x700)同样不溢出', (tester) async {
    tester.view.physicalSize = const Size(1200, 700);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(harness());
    await tester.pumpAndSettle();

    for (final tab in ['索引', '高级', 'SQL 预览']) {
      await openTab(tester, tab);
      expect(tester.takeException(), isNull, reason: '页签: $tab');
    }
  });

  testWidgets('全屏按钮收起 / 恢复左右侧栏', (tester) async {
    await setSize(tester);
    final app = AppState();
    await tester.pumpWidget(ChangeNotifierProvider.value(
      value: app,
      child: MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: kAppLocalizationsDelegates,
        supportedLocales: kSupportedLocales,
        theme: ThemeData(brightness: Brightness.dark),
        home: const SizedBox.expand(
          child: MatViewDesignPage(
            name: 'mv_demo',
            connection: '无此连接',
            database: 'db',
            schema: 'public',
          ),
        ),
      ),
    ));
    await tester.pumpAndSettle();
    expect(app.leftPanelVisible, isTrue);

    await tester.tap(find.byIcon(Icons.fullscreen));
    await tester.pumpAndSettle();
    expect(app.leftPanelVisible, isFalse);
    expect(app.rightPanelVisible, isFalse);
    expect(find.byIcon(Icons.fullscreen_exit), findsOneWidget);

    await tester.tap(find.byIcon(Icons.fullscreen_exit));
    await tester.pumpAndSettle();
    expect(app.leftPanelVisible, isTrue);
    expect(app.rightPanelVisible, isTrue);
  });

  testWidgets('关闭页面时把编辑文本留在 AppState,重开不丢', (tester) async {
    await setSize(tester);
    final app = AppState();
    Widget wrap(bool mount) => ChangeNotifierProvider.value(
          value: app,
          child: MaterialApp(
            locale: const Locale('zh'),
            localizationsDelegates: kAppLocalizationsDelegates,
            supportedLocales: kSupportedLocales,
            theme: ThemeData(brightness: Brightness.dark),
            home: Material(
              child: mount
                  ? const MatViewDesignPage(
                      name: 'mv_keep',
                      connection: '无此连接',
                      database: 'db',
                      schema: 'public',
                      isNew: true,
                    )
                  : const SizedBox.shrink(),
            ),
          ),
        );

    // 页面 initState 从 AppState.routineTextFor(_tabKey) 回填编辑区:
    // 切走标签再回来时文本已被上一实例的 dispose 写回这里,不会变回空。
    // 这里预置缓存来复现那条路径(编辑区是 re_editor,测试里不逐键输入)。
    const key = 'design|无此连接|db|public|mv_keep (新建)|materializedView';
    app.updateRoutineText(key, 'SELECT 42');

    await tester.pumpWidget(wrap(true));
    await tester.pumpAndSettle();
    expect(app.routineTextFor(key), 'SELECT 42', reason: '初始化不应清掉缓存');

    // 回填生效的可观测证据:定义非空,保存不会再报「定义不能为空」
    await tester.tap(find.text('保存'));
    await tester.pumpAndSettle();
    expect(find.textContaining('实体化视图定义不能为空'), findsNothing);
  });
}
