import 'dart:io';

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/data/schema_sync.dart';
import 'package:daro/widgets/schema_sync_dialog.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:daro/l10n/locale_config.dart';

// 结构同步向导弹窗的「冒烟」widget 测试:只验证界面装配与浅层交互——打开即渲染
// 设置页(源/目标 + 信息面板)、预填当前树选中的连接、未选库时比较禁用、连接下拉
// 过滤掉文件型库、选项弹窗可开并如实回写;集成用例走完整向导:
// 比较 → 差异页(分组表 + DDL 比较 / 部署脚本)→ 下一步 → 部署页 → 开始 → 自动重比。
// 数据层逻辑(schema_sync.dart)另有覆盖,这里只借假驱动验证界面流转。

/// 与 main.dart 同层级:Provider 必须在 MaterialApp 之上,showDialog 的弹层路由
/// 才能拿到 AppState(Tokens.of / AppColors.of 依赖它)。
Widget harness(AppState app, Widget home) => ChangeNotifierProvider<AppState>.value(
      value: app,
      child: MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: kAppLocalizationsDelegates,
        supportedLocales: kSupportedLocales,
        theme: ThemeData(brightness: Brightness.light),
        home: home,
      ),
    );

/// 与「工具 → 结构同步...」一致的入口:showDialog 打开大弹窗。
Widget entryPage(AppState app) => Builder(
      builder: (context) => Material(
        child: Button(
          text: '打开结构同步',
          onPressed: () => showSchemaSyncDialog(context, app: app),
        ),
      ),
    );

/// 假驱动:isConnected 恒真 → [ConnectionManager] 复用注入的实例,不再新建真实连接;
/// listDatabases 给两条库名,足以驱动下拉框渲染。未覆写的成员走 noSuchMethod 返回 null。
class _FakeDriver implements DatabaseDriver {
  @override
  bool get isConnected => true;
  @override
  Future<void> connect() async {}
  @override
  Future<void> close() async {}
  @override
  Future<void> useDatabase(String database) async {}
  @override
  Future<List<String>> listDatabases() async => const ['dev', 'test'];
  @override
  dynamic noSuchMethod(Invocation invocation) => null;
}

ConnectionInfo _mysql(String name) => ConnectionInfo(
      name: name,
      typeId: 'mysql',
      host: '10.255.255.1', // 不可路由;测试里永不真正连接(下拉/预填走假驱动)
      port: '3306',
      username: 'u',
      isLive: true,
    );

/// 有模式层的类型(postgresql),用于验证「模式」行的显隐与默认选中。
ConnectionInfo _pg(String name) => ConnectionInfo(
      name: name,
      typeId: 'postgresql',
      host: '10.255.255.1',
      port: '5432',
      username: 'postgres',
      isLive: true,
    );

void main() {
  const src = '源库A';
  const tgt = '目标库B';
  const file = '文件库C'; // sqlite:被 structureSyncUnsupported 过滤,不进下拉

  late AppState app;
  late Directory supportDir;

  setUp(() async {
    // 指向临时目录:让 AppState 的异步落盘真实完成(而非悬到测试结束后),
    // 同时不污染用户目录。见 navicat_import_dialog_test 同款处理。
    supportDir = await Directory.systemTemp.createTemp('daro_sync_test');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => supportDir.path,
    );
    app = AppState();
    app.addConnections([
      _mysql(src),
      _mysql(tgt),
      ConnectionInfo(
        name: file,
        typeId: 'sqlite',
        host: '',
        port: '',
        username: '',
        isLive: true,
      ),
    ]);
    // 给两条 MySQL 连接挂假驱动:预填会自动 expandConnection,
    // 借假驱动完成,避免测试期发起真实网络。
    app.connectionManager.attachDriverForTest(src, _FakeDriver());
    app.connectionManager.attachDriverForTest(tgt, _FakeDriver());
  });

  testWidgets('打开即渲染源/目标、预填当前树选中连接、未选库时比较/部署禁用', (tester) async {
    _bigSurface(tester);
    // 预填:仅源侧连接名跟随树的选中项
    app.detailSelection.value =
        SelectedNode(NodeKind.connection, src, connection: src);

    await tester.pumpWidget(harness(app, entryPage(app)));
    await tester.tap(find.text('打开结构同步'));
    await tester.pumpAndSettle();

    expect(find.text('结构同步'), findsOneWidget); // DialogBox 标题
    expect(find.text('源'), findsOneWidget);
    expect(find.text('目标'), findsOneWidget);
    // 源侧已预填该连接(只读下拉展示当前值),目标侧留空
    expect(find.text(src), findsWidgets);
    expect(find.text('选择连接'), findsWidgets);
    // 未选库 → 尚不可比较;部署入口(「开始」)只在向导最后一步出现
    expect(_buttonEnabled(tester, '比较'), isFalse);
    expect(find.text('选项'), findsOneWidget);
    expect(find.text('开始'), findsNothing);
  });

  testWidgets('连接下拉过滤掉文件型库(sqlite 不参与结构同步)', (tester) async {
    _bigSurface(tester);
    await tester.pumpWidget(harness(app, entryPage(app)));
    await tester.tap(find.text('打开结构同步'));
    await tester.pumpAndSettle();

    // 展开源侧连接下拉(第一个 ConnectionInfo 组合框)
    await tester.tap(find.byType(ComboBox<ConnectionInfo>).first);
    await tester.pumpAndSettle();

    expect(find.text(src), findsWidgets);
    expect(find.text(tgt), findsWidgets);
    expect(find.text(file), findsNothing); // 文件型库被过滤
  });

  testWidgets('端点下拉支持输入筛选:输入即窄化候选,值仍须点选', (tester) async {
    _bigSurface(tester);
    await tester.pumpWidget(harness(app, entryPage(app)));
    await tester.tap(find.text('打开结构同步'));
    await tester.pumpAndSettle();

    await tester.tap(find.byType(ComboBox<ConnectionInfo>).first);
    await tester.pumpAndSettle();

    // 打开即全量候选 + 面板顶部的搜索行(库 / 模式动辄上百个,靠滚动找人太慢)
    expect(find.text(src), findsWidgets);
    expect(find.text(tgt), findsWidgets);
    expect(find.text('输入以筛选…'), findsOneWidget);

    await tester.enterText(find.byType(TextField), '目标');
    await tester.pumpAndSettle();
    expect(find.text(tgt), findsOneWidget);
    expect(find.text(src), findsNothing);

    // 过滤只是窄化列表,仍要从列表里点选才落到端点
    await tester.tap(find.text(tgt));
    await tester.pumpAndSettle();
    expect(find.text('输入以筛选…'), findsNothing); // 面板已收起
    // 已落到端点:闭合框 + 顶部横幅 + 「连接名称」信息行都会回显这个名字
    expect(find.text(tgt), findsWidgets);
  });

  testWidgets('选项弹窗:开关齐全、父项不勾子项置灰、确定回写并作废旧比对', (tester) async {
    _bigSurface(tester);
    await tester.pumpWidget(harness(app, entryPage(app)));
    await tester.tap(find.text('打开结构同步'));
    await tester.pumpAndSettle();

    await tester.tap(find.text('选项'));
    await tester.pumpAndSettle();

    // 版式对齐参考工具的「比较选项」:强调色小节标题 + 一棵勾选列表
    expect(find.text('比较选项'), findsOneWidget);
    for (final label in const [
      '比较表',
      '比较主键',
      '比较外键',
      '比较唯一键',
      '比较检查',
      '比较排除',
      '比较视图',
      '比较函数',
      '比较索引',
      '比较序列',
      '比较触发器',
      '比较规则',
      '比较所有者',
      '用级联删除',
      '比较序列最后值',
    ]) {
      expect(_checkEnabled(tester, label), isNotNull, reason: '缺少开关:$label');
    }
    // 参考工具里没有的三项(以及被合并掉的「过程」)不该再出现在弹窗里
    for (final gone in const ['过程', '忽略注释差异', '定义比较忽略空白差异']) {
      expect(find.text(gone), findsNothing, reason: '旧开关应已删除:$gone');
    }
    // 默认值直接来自 SyncOptions:级联删除关(误删代价大,要用户自己勾)
    expect(_checkValue(tester, '用级联删除'), isFalse);
    expect(_checkValue(tester, '比较序列最后值'), isTrue);

    // 父项取消 → 子项一并置灰,且点不动(值保留,重新勾上即恢复)
    await tester.tap(find.text('比较表'));
    await tester.pumpAndSettle();
    expect(_checkValue(tester, '比较表'), isFalse);
    expect(_checkEnabled(tester, '比较主键'), isFalse, reason: '父项不勾子项应置灰');
    await tester.tap(find.text('比较主键'));
    await tester.pumpAndSettle();
    expect(_checkValue(tester, '比较主键'), isTrue, reason: '置灰后不该被点动');

    await tester.tap(find.text('用级联删除'));
    await tester.pumpAndSettle();
    expect(_checkValue(tester, '用级联删除'), isTrue);

    await tester.tap(find.text('确定'));
    await tester.pumpAndSettle();

    // 选项弹窗已关闭,主弹窗仍在(比较按钮回来了)
    expect(find.text('比较选项'), findsNothing);
    expect(find.text('比较'), findsOneWidget);
    // 改了选项 → 旧比对作废:回到设置页(源/目标两列仍在)
    expect(find.text('源'), findsOneWidget);
    expect(find.text('目标'), findsOneWidget);

    // 回写确实落到主弹窗:重开选项,刚才的改动都还在
    // (这条也顺带钉住「加开关忘记在 _openOptions 里抄字段」的老毛病)
    await tester.tap(find.text('选项'));
    await tester.pumpAndSettle();
    expect(_checkValue(tester, '用级联删除'), isTrue, reason: '勾选应回写');
    expect(_checkValue(tester, '比较表'), isFalse, reason: '父项状态应回写');
    expect(_checkValue(tester, '比较主键'), isTrue, reason: '子项状态应回写');
  });

  testWidgets('无模式层的类型(MySQL)整行不渲染「模式」下拉', (tester) async {
    _bigSurface(tester);
    app.detailSelection.value =
        SelectedNode(NodeKind.connection, src, connection: src);

    await tester.pumpWidget(harness(app, entryPage(app)));
    await tester.tap(find.text('打开结构同步'));
    await tester.pumpAndSettle();

    // 库即模式:标签与下拉都不渲染(而不是留一个恒禁用的空下拉)
    expect(find.text('模式:'), findsNothing);
    expect(find.text('该类型无模式层'), findsNothing);
    // 连接 / 数据库两行仍在
    expect(find.text('连接:'), findsNWidgets(2));
    expect(find.text('数据库:'), findsNWidgets(2));
  });

  testWidgets('PG:库下只有唯一模式时自动选中;多模式仍留空待选', (tester) async {
    _bigSurface(tester);

    final pgApp = AppState();
    pgApp.addConnections([_pg('pg_src'), _pg('pg_tgt')]);
    pgApp.connectionManager.attachDriverForTest('pg_src',
        _SchemaDriver(db: 'db1', schemas: const ['public']));
    pgApp.connectionManager.attachDriverForTest(
        'pg_tgt', _SchemaDriver(db: 'db2', schemas: const ['a', 'b']));
    pgApp.detailSelection.value =
        SelectedNode(NodeKind.connection, 'pg_src', connection: 'pg_src');

    await tester.pumpWidget(harness(pgApp, entryPage(pgApp)));
    await tester.tap(find.text('打开结构同步'));
    await tester.pumpAndSettle();

    // 有模式层 → 模式行在
    expect(find.text('模式:'), findsNWidgets(2));

    // 源库唯一模式 public → 自动选中,不停在"选择模式"提示上
    await tester.tap(find.byType(ComboBox<String>).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('db1').last);
    await tester.pumpAndSettle();
    expect(find.text('public'), findsWidgets);
    expect(find.text('选择模式'), findsNothing);

    // 目标库两个模式 → 不替用户拿主意,留空待选
    await tester.tap(find.byType(ComboBox<ConnectionInfo>).last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('pg_tgt').last);
    await tester.pumpAndSettle();
    await tester.tap(find.byType(ComboBox<String>).at(2));
    await tester.pumpAndSettle();
    await tester.tap(find.text('db2').last);
    await tester.pumpAndSettle();
    expect(find.text('选择模式'), findsOneWidget);
  });

  testWidgets('集成:假驱动比较 → 差异页(分组/勾选/DDL)→ 部署页 → 部署完停在部署页',
      (tester) async {
    _bigSurface(tester);

    final app = AppState();
    app.addConnections([_mysql('sync_src'), _mysql('sync_tgt')]);
    // 元数据缓存用假驱动填库名(源 srcdb / 目标 tgtdb),不触网。
    app.connectionManager
        .attachDriverForTest('sync_src', _Driver(db: 'srcdb', source: true));
    app.connectionManager
        .attachDriverForTest('sync_tgt', _Driver(db: 'tgtdb', source: false));
    // 源侧连接随树选中预填
    app.detailSelection.value =
        SelectedNode(NodeKind.connection, 'sync_src', connection: 'sync_src');

    // 比对 / 部署用工厂:按连接名回假驱动(视图三类差异)。
    final SyncDriverFactory factory = (ConnectionInfo c) => c.name == 'sync_src'
        ? _Driver(db: 'srcdb', source: true)
        : _Driver(db: 'tgtdb', source: false);

    await tester.pumpWidget(harness(
      app,
      Builder(
        builder: (context) => Material(
          child: Button(
            text: '打开结构同步',
            onPressed: () =>
                showSchemaSyncDialog(context, app: app, driverFactory: factory),
          ),
        ),
      ),
    ));
    await tester.tap(find.text('打开结构同步'));
    await tester.pumpAndSettle();

    // 选源库
    await tester.tap(find.byType(ComboBox<String>).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('srcdb'));
    await tester.pumpAndSettle();
    // 选目标连接
    await tester.tap(find.byType(ComboBox<ConnectionInfo>).last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('sync_tgt'));
    await tester.pumpAndSettle();
    // 选目标库(MySQL 无模式层 → String 下拉只有「源库 / 目标库」两个)
    await tester.tap(find.byType(ComboBox<String>).last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('tgtdb'));
    await tester.pumpAndSettle();

    // 两侧齐备 → 比较可用
    expect(_buttonEnabled(tester, '比较'), isTrue);
    await tester.tap(find.text('比较'));
    await tester.pumpAndSettle();

    // 自动进入差异页:三列差异表 + 分组标题(勾选数 / 总数)。
    // 默认勾选:新建 + 修改入勾,删除不入勾。
    expect(find.text('源对象'), findsOneWidget);
    expect(find.text('目标对象'), findsOneWidget);
    expect(find.text('要创建的对象 (已选择 1 个（共 1 个）)'), findsOneWidget);
    expect(find.text('要修改的对象 (已选择 1 个（共 1 个）)'), findsOneWidget);
    expect(find.text('要删除的对象 (已选择 0 个（共 1 个）)'), findsOneWidget);
    // 新建行只有源列,删除行只有目标列,修改行两列都有名字
    expect(find.text('v_only_src'), findsOneWidget);
    expect(find.text('v_only_tgt'), findsOneWidget);
    expect(find.text('v_diff'), findsNWidgets(2));

    // 未选对象时底部「DDL 比较」页是占位提示
    expect(find.text('在上方选择一个对象查看 DDL 比较。'), findsOneWidget);

    // 点选“要创建”的视图 → DDL 比较渲染源侧定义
    await tester.tap(find.text('v_only_src'));
    await tester.pumpAndSettle();
    expect(find.textContaining('源 · v_only_src'), findsOneWidget);
    // DDL / 脚本区现在是带高亮的富文本,finder 必须开 findRichText 才看得见。
    expect(find.textContaining('CREATE VIEW v_only_src', findRichText: true),
        findsWidgets);

    // 切到底部「部署脚本」页:拼接勾选对象的语句(删除未勾,不在脚本里)
    await tester.tap(find.text('部署脚本'));
    await tester.pumpAndSettle();
    expect(find.textContaining('CREATE VIEW v_only_src', findRichText: true),
        findsWidgets);
    expect(find.textContaining('DROP VIEW v_only_tgt', findRichText: true),
        findsNothing);

    // 下一步 → 部署页:目标服务器说明 + 复制脚本 + 部署选项,「开始」可用
    await tester.tap(find.text('下一步'));
    await tester.pumpAndSettle();
    expect(find.textContaining('（以下脚本将在此服务器上运行）'), findsOneWidget);
    expect(find.text('复制脚本'), findsOneWidget);
    expect(find.text('部署选项'), findsOneWidget);
    expect(find.text('消息日志'), findsOneWidget);
    expect(_buttonEnabled(tester, '开始'), isTrue);

    // 开始(未勾删除 → 无需二次确认)→ 部署完成。
    // 部署收尾**不自动重比**(重比是全量的,库大时慢),停在部署页让人看日志。
    await tester.tap(find.text('开始'));
    await tester.pumpAndSettle();
    expect(find.text('部署选项'), findsOneWidget); // 仍在部署页
    expect(find.text('要创建的对象 (已选择 1 个（共 1 个）)'), findsNothing);
    // 执行过程落在消息日志里(部署开始会自动切到这个标签页)
    final logText = tester
        .widgetList<SelectableText>(find.byType(SelectableText))
        .map(_selectablePlainText)
        .join('\n');
    expect(logText, contains('--End--'));
    expect(logText, contains('Result: OK'));
    // 「在消息日志中包含部署查询」默认不勾 → 日志只留结果行,不展开 SQL
    expect(logText, isNot(contains('Query:')));

    // 手动点页脚「重新比较」才回到差异页
    await tester.tap(find.text('重新比较'));
    await tester.pumpAndSettle();
    expect(find.text('要创建的对象 (已选择 1 个（共 1 个）)'), findsOneWidget);
  });

  testWidgets('部署选项:两项都勾上才生效 —— 部分失败时失败弹窗必弹且停在部署页',
      (tester) async {
    _bigSurface(tester);

    final app = AppState();
    app.addConnections([_mysql('sync_src'), _mysql('sync_tgt')]);
    app.connectionManager
        .attachDriverForTest('sync_src', _Driver(db: 'srcdb', source: true));
    app.connectionManager
        .attachDriverForTest('sync_tgt', _Driver(db: 'tgtdb', source: false));
    app.detailSelection.value =
        SelectedNode(NodeKind.connection, 'sync_src', connection: 'sync_src');

    // 目标侧让 v_diff 的 DDL 抛错 → 1 成功(新建 v_only_src) + 1 失败(修改 v_diff),
    // 正是「重比开着会不会吞掉失败提示」那个组合。
    final SyncDriverFactory factory = (ConnectionInfo c) => c.name == 'sync_src'
        ? _Driver(db: 'srcdb', source: true)
        : _Driver(db: 'tgtdb', source: false, failOn: 'v_diff');

    await tester.pumpWidget(harness(
      app,
      Builder(
        builder: (context) => Material(
          child: Button(
            text: '打开结构同步',
            onPressed: () =>
                showSchemaSyncDialog(context, app: app, driverFactory: factory),
          ),
        ),
      ),
    ));
    await tester.tap(find.text('打开结构同步'));
    await tester.pumpAndSettle();

    await tester.tap(find.byType(ComboBox<String>).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('srcdb'));
    await tester.pumpAndSettle();
    await tester.tap(find.byType(ComboBox<ConnectionInfo>).last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('sync_tgt'));
    await tester.pumpAndSettle();
    await tester.tap(find.byType(ComboBox<String>).last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('tgtdb'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('比较'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('下一步'));
    await tester.pumpAndSettle();

    // 部署选项弹窗:默认两项都不勾,这里都勾上 —— 顺带验证开关真生效。
    await tester.tap(find.text('部署选项'));
    await tester.pumpAndSettle();
    // 弹窗里一处「部署选项」(强调色小节标题)+ 页脚按钮那处 = 2
    expect(find.text('选项'), findsOneWidget);
    expect(find.text('部署选项'), findsNWidgets(2));
    expect(_checkValue(tester, '遇到错误时继续'), isFalse, reason: '默认不勾');
    expect(_checkValue(tester, '在消息日志中包含部署查询'), isFalse, reason: '默认不勾');
    // 取消:弹窗改的是副本,勾了也不该回写主弹窗
    await tester.tap(find.text('遇到错误时继续'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('取消'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('部署选项'));
    await tester.pumpAndSettle();
    expect(_checkValue(tester, '遇到错误时继续'), isFalse, reason: '取消不该回写');
    // 这次两项都勾上并确定
    await tester.tap(find.text('遇到错误时继续'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('在消息日志中包含部署查询'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('确定'));
    await tester.pumpAndSettle();

    await tester.tap(find.text('开始'));
    await tester.pumpAndSettle();

    // 关键:失败明细弹窗必须出现 —— 修之前这条分支被自动重比的 `return` 短路,
    // 「有成功也有失败」时用户看不到任何失败提示。
    expect(find.text('部署结果'), findsOneWidget);
    expect(find.textContaining('失败 1 个'), findsOneWidget);

    await tester.tap(find.text('知道了'));
    await tester.pumpAndSettle();

    // 收尾不自动换页:仍停在部署页,要重比得自己点页脚的「重新比较」
    expect(find.text('部署选项'), findsOneWidget);
    expect(find.text('要创建的对象 (已选择 1 个（共 1 个）)'), findsNothing);
    // 勾了「在消息日志中包含部署查询」→ 日志展开 SQL,成功与失败各记一条
    final logText = tester
        .widgetList<SelectableText>(find.byType(SelectableText))
        .map(_selectablePlainText)
        .join('\n');
    expect(logText, contains('Query:'));
    expect(logText, contains('Result: OK'));
    expect(logText, contains('ERROR:'));
  });

  testWidgets('部署页:脚本与日志都是可划选文本(鼠标拖拽能出选区)', (tester) async {
    _bigSurface(tester);

    final app = AppState();
    app.addConnections([_mysql('sync_src'), _mysql('sync_tgt')]);
    app.connectionManager
        .attachDriverForTest('sync_src', _Driver(db: 'srcdb', source: true));
    app.connectionManager
        .attachDriverForTest('sync_tgt', _Driver(db: 'tgtdb', source: false));
    app.detailSelection.value =
        SelectedNode(NodeKind.connection, 'sync_src', connection: 'sync_src');

    final SyncDriverFactory factory = (ConnectionInfo c) => c.name == 'sync_src'
        ? _Driver(db: 'srcdb', source: true)
        : _Driver(db: 'tgtdb', source: false);

    await tester.pumpWidget(harness(
      app,
      Builder(
        builder: (context) => Material(
          child: Button(
            text: '打开结构同步',
            onPressed: () =>
                showSchemaSyncDialog(context, app: app, driverFactory: factory),
          ),
        ),
      ),
    ));
    await tester.tap(find.text('打开结构同步'));
    await tester.pumpAndSettle();

    await tester.tap(find.byType(ComboBox<String>).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('srcdb'));
    await tester.pumpAndSettle();
    await tester.tap(find.byType(ComboBox<ConnectionInfo>).last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('sync_tgt'));
    await tester.pumpAndSettle();
    await tester.tap(find.byType(ComboBox<String>).last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('tgtdb'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('比较'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('下一步'));
    await tester.pumpAndSettle();

    // 部署脚本页:只读的 SelectableText(不是 Text,也不是可编辑输入框)。
    final script = tester.widget<SelectableText>(find.byType(SelectableText));
    expect(script.selectionEnabled, isTrue);

    // 鼠标从脚本左上角往右拖 → 真的产出一个非折叠选区。
    // (必须用鼠标手势:touch 拖拽会被外层 SingleChildScrollView 当成滚动。)
    final box = tester.renderObject<RenderBox>(find.byType(EditableText));
    final from = box.localToGlobal(const Offset(6, 6));
    final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer(location: from);
    await mouse.down(from); // addPointer 只登记指针,按下要显式 down
    await mouse.moveTo(from + const Offset(140, 0));
    await mouse.up();
    await tester.pumpAndSettle();
    final sel = tester
        .state<EditableTextState>(find.byType(EditableText))
        .textEditingValue
        .selection;
    expect(sel.isCollapsed, isFalse, reason: '脚本应能划出选区');
    expect(sel.textInside(_selectablePlainText(script)), isNotEmpty);

    // 消息日志页同样是可划选文本(空态占位亦为 SelectableText)。
    await tester.tap(find.text('消息日志'));
    await tester.pumpAndSettle();
    final log = tester.widget<SelectableText>(find.byType(SelectableText));
    expect(log.selectionEnabled, isTrue);
    expect(log.data, '（尚未执行部署）');
  });
}

/// 取只读文本区的内容:裸文本走 `data`,带语法高亮的是 `textSpan`
/// (见 lib/theme/sql_highlight.dart),两者都当成纯字符串看。
String _selectablePlainText(SelectableText w) =>
    w.data ?? w.textSpan?.toPlainText() ?? '';

/// 放大测试视口,避免 1040×700 大弹窗被默认 800×600 压扁 / 命中重叠。
void _bigSurface(WidgetTester tester) {
  tester.view.physicalSize = const Size(1600, 1200);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
}

/// base-ui Button 以 onPressed==null 表示禁用。
bool _buttonEnabled(WidgetTester tester, String text) {
  final el = tester.element(find.text(text));
  final button = el.findAncestorWidgetOfExactType<Button>();
  expect(button, isNotNull, reason: '$text 未包在 base-ui Button 中');
  return button!.onPressed != null;
}

/// 按 label 定位选项复选框(缺开关时直接断言失败,报错比 null 解引用清楚)。
CheckBox _checkBox(WidgetTester tester, String label) {
  final f = find.byWidgetPredicate((w) => w is CheckBox && w.label == label);
  expect(f, findsOneWidget, reason: '找不到开关:$label');
  return tester.widget<CheckBox>(f);
}

/// 按 label 定位选项复选框并读取勾选态。
bool _checkValue(WidgetTester tester, String label) =>
    _checkBox(tester, label).value;

/// 按 label 定位选项复选框并读取可用性(父项不勾时子项置灰)。
bool _checkEnabled(WidgetTester tester, String label) =>
    _checkBox(tester, label).enabled;

/// 集成测试用假驱动:既能填库名(下拉),也提供视图定义(比对)。
/// 差异只来自视图:源独有 v_only_src(新建)、两侧 v_diff 定义不同(修改)、
/// 目标独有 v_only_tgt(删除);表 / 函数 / 过程留空避免走 readTableDesign。
/// 假驱动:一个库 + 指定模式列表,用来驱动「模式」行的显隐与「唯一模式自动选中」。
class _SchemaDriver implements DatabaseDriver {
  _SchemaDriver({required this.db, required this.schemas});

  final String db;
  final List<String> schemas;

  @override
  bool get isConnected => true;
  @override
  Future<void> connect() async {}
  @override
  Future<void> close() async {}
  @override
  Future<void> useDatabase(String database) async {}
  @override
  Future<void> useSchema(String? schema) async {}
  @override
  Future<List<String>> listDatabases() async => [db];
  @override
  Future<List<String>> listSchemas(String database) async => schemas;
  @override
  Future<List<String>> listTables(String database, {String? schema}) async =>
      const [];
  @override
  Future<List<String>> listViews(String database, {String? schema}) async =>
      const [];
  @override
  Future<List<String>> listFunctions(String database, {String? schema}) async =>
      const [];
  @override
  Future<List<String>> listProcedures(String database,
          {String? schema}) async =>
      const [];
  // 序列类别新增后必须显式覆写:noSuchMethod 返回 null,而声明返回 List,
  // await 后迭代会抛,被收成「读取序列列表失败」混进 plan.errors。
  @override
  Future<List<String>> listSequences(String database, {String? schema}) async =>
      const [];
  @override
  dynamic noSuchMethod(Invocation invocation) => null;
}

class _Driver implements DatabaseDriver {
  _Driver({required this.db, required this.source, this.failOn});

  final String db;
  final bool source;

  /// 命中该子串的 DDL 抛错,用来构造「部分成功 + 部分失败」的部署。
  final String? failOn;

  Map<String, String?> get _views => source
      ? {
          'v_only_src': 'CREATE VIEW v_only_src AS SELECT 1',
          'v_diff': 'CREATE VIEW v_diff AS SELECT 1',
        }
      : {
          'v_diff': 'CREATE VIEW v_diff AS SELECT 2',
          'v_only_tgt': 'CREATE VIEW v_only_tgt AS SELECT 9',
        };

  @override
  bool get isConnected => true;
  @override
  Future<void> connect() async {}
  @override
  Future<void> close() async {}
  @override
  Future<void> useDatabase(String database) async {}
  @override
  Future<void> useSchema(String? schema) async {}
  @override
  Future<List<String>> listDatabases() async => [db];
  @override
  Future<List<String>> listSchemas(String database) async => const [];
  @override
  Future<List<String>> listTables(String database, {String? schema}) async =>
      const [];
  @override
  Future<List<String>> listViews(String database, {String? schema}) async =>
      _views.keys.toList();
  @override
  Future<List<String>> listFunctions(String database, {String? schema}) async =>
      const [];
  @override
  Future<List<String>> listProcedures(String database,
      {String? schema}) async => const [];
  @override
  Future<List<String>> listSequences(String database, {String? schema}) async =>
      const [];
  @override
  Future<String?> getDefinition(String database, String name, String kind,
      {String? schema}) async =>
      _views[name];
  @override
  Future<QueryResult> executeQuery(String sql,
      {int limit = 1000, int offset = 0}) async {
    // 部署假执行:默认吞掉 DDL;failOn 命中时抛错以模拟部署失败。
    final fail = failOn;
    if (fail != null && sql.contains(fail)) {
      throw Exception('模拟执行失败');
    }
    return QueryResult(columns: const [], rows: const []);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => null;
}
