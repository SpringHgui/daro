import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/theme/app_theme.dart';
import 'package:daro/widgets/database_edit_dialog.dart';
import 'package:daro/l10n/locale_config.dart';

/// 渲染 + 行为守护:「编辑数据库」对话框的四标签页结构、扩展页的可用/已安装
/// 转移交互,以及「确定」实际打到服务端的语句序列。
///
/// 服务端由 [_CatalogDriver] 假扮:按 SQL 特征回目录数据,并记录每条执行过的
/// 语句与每次会话切库。这样能在离线环境下把「读现状 → 改 → 只提交差异」整条
/// 链路钉住(真机目录查询的正确性由 database_edit_live_test.dart 实连覆盖)。
const _db = 'demo';

class _CatalogDriver implements DatabaseDriver {
  _CatalogDriver({this.failProps = false});

  /// 属性查询失败(模拟无权限 / 库不存在)
  final bool failProps;

  /// 按执行顺序记录的语句
  final List<String> executed = [];

  /// 按执行顺序记录的 useDatabase 调用(会话被切到哪个库)
  final List<String> switched = [];

  @override
  bool get isConnected => true;

  @override
  Future<void> connect() async {}

  @override
  Future<void> close() async {}

  @override
  Future<void> useDatabase(String database) async => switched.add(database);

  // sessionFor 对 PG 家族一律调 useSchema,不实现就落到 noSuchMethod,
  // 三条目录查询全被 try/catch 吞掉(表现为快照永远是兜底值)
  @override
  Future<void> useSchema(String? schema) async {}

  @override
  Future<QueryResult> executeQuery(String sql,
      {int limit = 1000, int offset = 0}) async {
    executed.add(sql);
    final rows = _respond(sql);
    if (rows == null) throw StateError('未预期的查询:$sql');
    return QueryResult(columns: const [], rows: rows, limit: limit);
  }

  /// 按 SQL 特征分发;顺序敏感(属性查询里也含 pg_tablespace / pg_roles)
  List<List<String>>? _respond(String sql) {
    // 写语句(差异提交过去的 COMMENT / ALTER / CREATE-DROP EXTENSION)没有结果集
    if (!sql.trimLeft().toUpperCase().startsWith('SELECT')) return const [];
    if (sql.contains('server_version_num')) return [['180003']];
    if (sql.contains('FROM pg_catalog.pg_database d')) {
      if (failProps) throw StateError('permission denied on pg_database');
      return [
        ['postgres', 'pg_default', '-1', '1', '0', '示例库', 'UTF8', 'C', 'C']
      ];
    }
    if (sql.contains('pg_roles WHERE rolcanlogin')) {
      return [
        ['postgres'],
        ['app_owner']
      ];
    }
    if (sql.contains('FROM pg_catalog.pg_tablespace')) {
      return [
        ['pg_default'],
        ['fast_ssd']
      ];
    }
    if (sql.contains('pg_available_extensions') &&
        sql.contains('installed_version IS NULL')) {
      return [
        ['btree_gin', '1.3', 'GIN 索引的 B-tree 操作符类'],
        ['pg_trgm', '1.6', '三元组模糊匹配'],
      ];
    }
    if (sql.contains('FROM pg_catalog.pg_extension e')) {
      return [
        ['hstore', '1.8', '键值对存储类型'],
        ['plpgsql', '1.0', 'PL/pgSQL 过程语言'],
      ];
    }
    return null;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnsupportedError('未预期的驱动调用:${invocation.memberName}');
}

ConnectionInfo _conn() => const ConnectionInfo(
      name: 'local',
      typeId: 'postgresql',
      host: '127.0.0.1',
      port: '5432',
      username: 'postgres',
      database: _db,
    );

/// 打开对话框并等快照落地,返回挂上去的假驱动
Future<_CatalogDriver> _open(
  WidgetTester tester, {
  bool failProps = false,
}) async {
  tester.view.physicalSize = const Size(1600, 1000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  final conn = _conn();
  final driver = _CatalogDriver(failProps: failProps);
  final app = AppState();
  app.connectionManager.attachDriverForTest(conn.name, driver);

  await tester.pumpWidget(ChangeNotifierProvider<AppState>.value(
    value: app,
    child: MaterialApp(
      locale: const Locale('zh'),
      localizationsDelegates: kAppLocalizationsDelegates,
      supportedLocales: kSupportedLocales,
      theme: ThemeData(brightness: Brightness.light),
      home: TokenScope(
        tokens: AppTheme.light.toDesktopTokens(),
        child: Scaffold(
          body: Builder(
            builder: (context) => Center(
              child: GestureDetector(
                onTap: () => showDialog<void>(
                  context: context,
                  builder: (_) => DatabaseEditDialog(
                    connection: conn,
                    database: _db,
                  ),
                ),
                child: const Text('open'),
              ),
            ),
          ),
        ),
      ),
    ),
  ));
  await tester.tap(find.text('open'));
  await tester.pumpAndSettle();
  return driver;
}

/// 转移 / 确定按钮的禁用态看 onPressed:find.text 命中的是 Button 里的 Text,
/// 直接 `widget<Button>(find.text(...))` 会强转失败
Button _button(WidgetTester tester, String label) =>
    tester.widget<Button>(find.widgetWithText(Button, label));

/// 点标签页(标签条上的项与正文里的同名文本可能并存,取第一个即标签条)
Future<void> _showTab(WidgetTester tester, String label) async {
  await tester.tap(find.text(label).first);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('四个标签页齐全,常规页按现状回填且不可变字段只读', (tester) async {
    await _open(tester);

    for (final label in ['常规', '扩展', '注释', 'SQL 预览']) {
      expect(find.text(label), findsWidgets, reason: '缺少标签页 $label');
    }
    // 现状回填:所有者 / 表空间 / 连接限制 / 两个布尔 / 编码
    expect(find.text('postgres'), findsWidgets);
    expect(find.text('pg_default'), findsWidgets);
    expect(find.text('-1'), findsOneWidget);
    expect(find.text('允许连接'), findsOneWidget);
    expect(find.text('是否模板'), findsOneWidget);
    expect(find.text('UTF8'), findsOneWidget);

    // 库名与编码 / 排序规则 / 字符分类 四格禁用输入框
    expect(
      tester
          .widgetList<Input>(find.byType(Input))
          .where((i) => !i.enabled)
          .length,
      4,
      reason: '名称 / 编码 / 排序规则 / 字符分类 应为只读',
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('扩展页:选中可用项点 > 后进入已安装列表,SQL 预览跟随',
      (tester) async {
    await _open(tester);
    await _showTab(tester, '扩展');

    // 初始两侧各两项,互不重叠
    expect(find.text('pg_trgm'), findsOneWidget);
    expect(find.text('hstore'), findsOneWidget);
    // 未选中时两个转移按钮都禁用
    expect(_button(tester, '>').onPressed, isNull);
    expect(_button(tester, '<').onPressed, isNull);

    await tester.tap(find.text('pg_trgm'));
    await tester.pumpAndSettle();
    expect(_button(tester, '>').onPressed, isNotNull);
    await tester.tap(find.text('>'));
    await tester.pumpAndSettle();

    // 移到右侧后仍只出现一次(不会两边都有)
    expect(find.text('pg_trgm'), findsOneWidget);
    expect(find.text('hstore'), findsOneWidget);

    await _showTab(tester, 'SQL 预览');
    // SQL 预览区现在是带语法高亮的富文本,finder 必须开 findRichText 才看得见。
    expect(
        find.textContaining('CREATE EXTENSION IF NOT EXISTS "pg_trgm"',
            findRichText: true),
        findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('反向转移:已安装项点 < 移回可用,预览出 DROP 语句', (tester) async {
    await _open(tester);
    await _showTab(tester, '扩展');

    await tester.tap(find.text('hstore'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('<'));
    await tester.pumpAndSettle();
    expect(find.text('hstore'), findsOneWidget);

    await _showTab(tester, 'SQL 预览');
    expect(find.textContaining('DROP EXTENSION IF EXISTS "hstore"',
            findRichText: true),
        findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('双击可用项等同点 >(零延迟选中 + 单独双击判定)', (tester) async {
    await _open(tester);
    await _showTab(tester, '扩展');

    // 两次点击之间要推进时钟:pump() 不带时长会让两拍落在同一时刻,
    // DoubleTap 识别器不把第二次当双击(与真机上「手指抬起再按下」不同)
    await tester.tap(find.text('btree_gin'));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.text('btree_gin'));
    await tester.pumpAndSettle();

    await _showTab(tester, 'SQL 预览');
    expect(
        find.textContaining('CREATE EXTENSION IF NOT EXISTS "btree_gin"',
            findRichText: true),
        findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('无改动时确定禁用;改注释 + 装扩展后只提交这两类语句',
      (tester) async {
    final driver = await _open(tester);
    // 快照读取本身跑过的查询:版本 + 属性 + 角色 + 表空间 + 两条扩展清单
    expect(driver.executed, hasLength(6));
    // 两条扩展清单必须在目标库上下文里查
    expect(driver.switched, [_db, _db]);
    final readsBefore = driver.executed.length;
    final switchesBefore = driver.switched.length;

    // 刚打开时没有任何差异 → 确定禁用
    expect(_button(tester, '确定').onPressed, isNull);

    await _showTab(tester, '注释');
    await tester.enterText(find.byType(Textarea), '订单库');
    await tester.pumpAndSettle();
    expect(_button(tester, '确定').onPressed, isNotNull);

    await _showTab(tester, '扩展');
    await tester.tap(find.text('pg_trgm'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('>'));
    await tester.pumpAndSettle();

    await tester.tap(find.text('确定'));
    await tester.pumpAndSettle();

    final written = driver.executed.sublist(readsBefore);
    expect(written, [
      """COMMENT ON DATABASE "demo" IS '订单库'""",
      'CREATE EXTENSION IF NOT EXISTS "pg_trgm"',
    ], reason: '只提交差异,且库级语句在前、扩展语句在后');
    // 库级语句不切会话(切过去反而会挡住自己),只有扩展语句落进目标库
    expect(driver.switched.sublist(switchesBefore), [_db]);
  });

  testWidgets('现状读不到时禁用确定,并给出原因提示', (tester) async {
    final driver = await _open(tester, failProps: true);
    expect(driver.switched, [_db, _db], reason: '扩展清单仍应读到');

    // 原因提示落在「常规」页(基准不可信就是这一页的表单问题),切走就不在树里了
    expect(find.textContaining('未能读取该库的当前属性'), findsOneWidget);
    expect(_button(tester, '确定').onPressed, isNull);

    await _showTab(tester, '注释');
    await tester.enterText(find.byType(Textarea), '订单库');
    await tester.pumpAndSettle();
    expect(_button(tester, '确定').onPressed, isNull);
    expect(tester.takeException(), isNull);
  });

  testWidgets('SQL 预览无改动时显示说明而非空框', (tester) async {
    await _open(tester);
    await _showTab(tester, 'SQL 预览');
    expect(find.textContaining('没有需要执行的改动', findRichText: true),
        findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
