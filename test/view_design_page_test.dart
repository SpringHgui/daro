import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:re_editor/re_editor.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/widgets/view_design_page.dart';
import 'package:daro/l10n/locale_config.dart';

/// 工具栏已是「纯图标 + tooltip」(与「设计表」同档),按钮上的文字不再是
/// 可断言的锚点,统一改按图标定位。
final _save = find.byIcon(Icons.save_outlined);
final _preview = find.byIcon(Icons.visibility_outlined);
final _explain = find.byIcon(Icons.account_tree_outlined);
final _builder = find.byIcon(Icons.construction_outlined);
final _beautify = find.byIcon(Icons.auto_fix_high_outlined);
final _addRule = find.byIcon(Icons.add_circle_outline);
final _delRule = find.byIcon(Icons.remove_circle_outline);

Widget harness({String? dialect, bool isNew = false}) =>
    ChangeNotifierProvider(
      create: (_) => AppState(),
      child: MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: kAppLocalizationsDelegates,
        supportedLocales: kSupportedLocales,
        theme: ThemeData(brightness: Brightness.dark),
        // 与 main.dart 同构:应用根部有 Material,base-ui 的 Input(TextField)需要
        home: Material(
          type: MaterialType.transparency,
          child: ViewDesignPage(
            // 同一用例内换配置重开页面必须给 key:基线、已读回标记等只在
            // initState 建立,复用元素会带着上一份状态。
            key: ValueKey('$dialect|${isNew ? 'new' : 'edit'}'),
            name: 'v_demo',
            connection: '无此连接',
            database: 'db',
            category: ObjectCategory.view,
            schema: 'public',
            isNew: isNew,
            dialectOverride: dialect,
          ),
        ),
      ),
    );

Future<void> openTab(WidgetTester tester, String label) async {
  await tester.tap(find.text(label).first);
  await tester.pumpAndSettle();
}

/// re_editor 的正文是自绘的,`find.text` 看不见,只能读控制器
/// (与 `table_designer_page_test.dart` 的 `previewSql` 同一口径)
String changeSql(WidgetTester tester) => tester
    .widgetList<CodeEditor>(find.byType(CodeEditor))
    .map((e) => e.controller?.text ?? '')
    .join('\n');

/// `find.byIcon` 命中的是按钮内部的 Icon;要看可用性得拿到外层 ToolbarButton
ToolbarButton ruleBtn(WidgetTester tester, IconData icon) => tester.widget(
    find.byWidgetPredicate(
        (w) => w is ToolbarButton && w.icon == icon));

/// 每个用例都要够宽:工具栏 + 四个页签在窄测试窗口下会溢出成异常帧。
void setView(WidgetTester tester) {
  tester.view.physicalSize = const Size(1800, 1000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
}

void main() {
  testWidgets('非 PG 没有「规则」页签,工具栏为纯图标', (tester) async {
    setView(tester);
    await tester.pumpWidget(harness());
    await tester.pumpAndSettle();

    for (final tab in ['定义', '高级', '注释', 'SQL 预览']) {
      expect(find.text(tab), findsWidgets, reason: '缺少页签: $tab');
    }
    expect(find.text('规则'), findsNothing);
    for (final btn in [_save, _preview, _explain, _builder, _beautify]) {
      expect(btn, findsOneWidget, reason: '缺少工具栏图标按钮');
    }
    // 文字标签已经不在按钮上(改成 tooltip 了),不该再出现
    expect(find.text('保存'), findsNothing);
    expect(find.text('美化 SQL'), findsNothing);
    expect(_addRule, findsNothing);
    expect(_delRule, findsNothing);
    expect(find.byIcon(Icons.fullscreen), findsOneWidget);
  });

  testWidgets('PG:规则页签 + 网格表头 + 位置/定义字段,工具栏追加规则按钮',
      (tester) async {
    setView(tester);
    await tester.pumpWidget(harness(dialect: 'postgresql'));
    await tester.pumpAndSettle();

    await openTab(tester, '规则');

    for (final col in ['名称', 'OID', '事件', '代替运行', '注释']) {
      expect(find.text(col), findsWidgets, reason: '缺少列头: $col');
    }
    expect(find.text('位置:'), findsOneWidget);
    expect(find.text('定义:'), findsOneWidget);
    expect(_addRule, findsOneWidget);
    // 没选中规则时删除不可用(按钮仍在,置灰)
    expect(_delRule, findsOneWidget);
  });

  testWidgets('「添加规则」打开对话框;新建模式下按钮置灰', (tester) async {
    setView(tester);
    await tester.pumpWidget(harness(dialect: 'postgresql'));
    await tester.pumpAndSettle();

    await openTab(tester, '规则');
    expect(ruleBtn(tester, Icons.add_circle_outline).enabled, isTrue);
    await tester.tap(_addRule);
    await tester.pumpAndSettle();

    expect(find.text('添加规则'), findsOneWidget, reason: '对话框标题');
    // 对话框里是「事件 + 动作 + 代替运行」,不是又一个网格
    expect(find.text('名称:'), findsOneWidget);
    expect(find.text('动作:'), findsOneWidget);
    expect(find.textContaining('CREATE RULE "'), findsNothing,
        reason: '未填完时预览条应是提示文字');
    await tester.tap(find.text('取消'));
    await tester.pumpAndSettle();
    expect(find.text('添加规则'), findsNothing);

    // 视图还不存在时 CREATE RULE 必失败,不给入口
    await tester.pumpWidget(harness(dialect: 'postgresql', isNew: true));
    await tester.pumpAndSettle();
    await openTab(tester, '规则');
    expect(ruleBtn(tester, Icons.add_circle_outline).enabled, isFalse);
  });

  testWidgets('高级页:所有者 / 检查选项 / 安全屏障齐全', (tester) async {
    setView(tester);
    await tester.pumpWidget(harness(dialect: 'postgresql'));
    await tester.pumpAndSettle();

    await openTab(tester, '高级');
    expect(find.text('所有者:'), findsOneWidget);
    expect(find.text('检查选项:'), findsOneWidget);
    expect(find.text('安全屏障'), findsOneWidget);
  });

  testWidgets('SQL 预览:更改页给出保存将执行的全套语句(先删后建)',
      (tester) async {
    setView(tester);
    await tester.pumpWidget(harness(dialect: 'postgresql'));
    await tester.pumpAndSettle();

    await openTab(tester, 'SQL 预览');
    expect(find.text('更改'), findsOneWidget);
    expect(find.text('DDL'), findsOneWidget);
    expect(find.textContaining('「保存」将按以下顺序执行这些语句'), findsOneWidget);
    // 编辑模式:DROP + CREATE 两条都在,且执行的是界面里那份 SELECT
    final change = changeSql(tester);
    expect(change, contains('DROP VIEW IF EXISTS "public"."v_demo";'));
    expect(change, contains('CREATE VIEW "public"."v_demo"'));

    await tester.tap(find.text('DDL'));
    await tester.pumpAndSettle();
    expect(find.byType(TextDiffView), findsOneWidget);
    expect(find.text('之前:'), findsOneWidget);
    expect(find.text('之后:'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('新建模式没有 DROP 语句', (tester) async {
    setView(tester);
    await tester.pumpWidget(harness(dialect: 'postgresql', isNew: true));
    await tester.pumpAndSettle();

    await openTab(tester, 'SQL 预览');
    final change = changeSql(tester);
    expect(change, isNot(contains('DROP VIEW')));
    expect(change, contains('CREATE VIEW "public"."v_demo"'));
  });

  testWidgets('定义为空时预览只回消息,不抛异常', (tester) async {
    setView(tester);
    await tester.pumpWidget(harness());
    await tester.pumpAndSettle();

    await tester.tap(_preview);
    await tester.pumpAndSettle();

    expect(find.text('消息'), findsOneWidget);
    expect(find.textContaining('视图定义为空'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('全屏按钮收起 / 恢复左右侧栏', (tester) async {
    setView(tester);
    final app = AppState();
    await tester.pumpWidget(ChangeNotifierProvider.value(
      value: app,
      child: MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: kAppLocalizationsDelegates,
        supportedLocales: kSupportedLocales,
        theme: ThemeData(brightness: Brightness.dark),
        home: const SizedBox.expand(
          child: ViewDesignPage(
            name: 'v_demo',
            connection: '无此连接',
            database: 'db',
            category: ObjectCategory.view,
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

  group('splitViewDefinition', () {
    test('剥掉尾部 WITH … CHECK OPTION 同时把它记下来(否则保存会静默丢掉)',
        () {
      final r = splitViewDefinition(
          'CREATE VIEW `v` AS select `t`.`a` AS `a` from `t` '
          'WITH LOCAL CHECK OPTION');
      expect(r.select, 'select `t`.`a` AS `a` from `t`');
      expect(r.checkOption, 'LOCAL');
    });

    test('不写关键字的 CHECK OPTION 回填成显式 CASCADED(MySQL 的默认值)', () {
      final r = splitViewDefinition('CREATE VIEW v AS SELECT 1 WITH CHECK OPTION');
      expect(r.select, 'SELECT 1');
      expect(r.checkOption, 'CASCADED');
    });

    test('没有检查选项时为空串', () {
      final r = splitViewDefinition('CREATE VIEW v AS SELECT 1');
      expect(r.checkOption, '');
    });

    test('MySQL 定义头部带 DEFINER 也能取到 SELECT 主体', () {
      final r = splitViewDefinition('CREATE ALGORITHM=UNDEFINED '
          "DEFINER=`root`@`localhost` SQL SECURITY DEFINER "
          'VIEW `v` AS select 1 AS `1`;');
      expect(r.select, 'select 1 AS `1`');
    });

    test('PG 的 CREATE OR REPLACE 头部与 security_barrier', () {
      final r = splitViewDefinition(
          'CREATE OR REPLACE VIEW "v" WITH (security_barrier) AS SELECT 1');
      expect(r.select, 'SELECT 1');
      expect(r.securityBarrier, isTrue);
    });

    test('用户直接写 SELECT 时原样回显', () {
      final r = splitViewDefinition('SELECT a, b FROM t;');
      expect(r.select, 'SELECT a, b FROM t');
    });
  });
}
