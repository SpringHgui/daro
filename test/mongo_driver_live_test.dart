import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';

/// **实连**回归:MongoDB 只读链路在真服务端上打得开、读得对。
///
/// 离线单测(test/mongo_driver_test.dart)只验纯函数,验不到这四条只能实测的前提:
/// 1. `runCommand` 失败时**不抛异常**,回包里 `ok:0` —— 不检查就是把错误当空数据显示;
/// 2. `Db` 绑在一个库名上,读错命名空间**静默返回 0 行**(`collection('别的库.集合')`
///    不报错),所以每次操作都要改 `databaseName` 并串行化;
/// 3. 驱动要过滤 `system.*`(实测 `system.views` 会混进集合列表);
/// 4. 服务端 8.0 的 `collStats` 已无 `n`,行数只能另取 `count`。
///
/// 全程只读(只 find / count / listCollections,绝不写、绝不 DDL)。
/// 默认**不跑**,且必须显式给密码 —— 免得别人拉下仓库就连到某个真实实例:
///   DARO_LIVE_DB=1 DARO_MONGO_PWD='...' flutter test test/mongo_driver_live_test.dart
const _db = 'probe_db';
const _db2 = 'probe_db2';
const _orders = 'orders';

void main() {
  final enabled = Platform.environment['DARO_LIVE_DB'] == '1';
  final password = Platform.environment['DARO_MONGO_PWD'] ?? '';
  final skip = !enabled
      ? '未开启实连(DARO_LIVE_DB=1 时才跑)'
      : password.isEmpty
          ? '未给 DARO_MONGO_PWD(实连需要本机探针容器的密码)'
          : null;
  if (skip != null) {
    // ignore: avoid_print
    print('实连测试跳过:$skip');
    test('实连:MongoDB 只读链路(跳过)', () {}, skip: skip);
    return;
  }

  final conn = ConnectionInfo(
    name: 'mongo 探针',
    typeId: 'mongodb',
    host: Platform.environment['DARO_MONGO_HOST'] ?? '127.0.0.1',
    port: Platform.environment['DARO_MONGO_PORT'] ?? '27017',
    username: Platform.environment['DARO_MONGO_USER'] ?? 'admin',
    password: password,
  );
  late DatabaseDriver driver;

  setUpAll(() async {
    driver = createDriver(conn)!;
    await driver.connect();
  });
  tearDownAll(() async => driver.close());

  test('实连:列库 / 集合 / 视图,且 system.* 不进列表', () async {
    final dbs = await driver.listDatabases();
    expect(dbs, containsAll(<String>[_db, _db2]));

    // MongoDB 无模式层:返回空表,树据此不渲染模式节点
    expect(await driver.listSchemas(_db), isEmpty);

    final tables = await driver.listTables(_db);
    expect(tables, containsAll(<String>[_orders, 'big_docs', '中文集合', 'a.b']));
    expect(tables.any((t) => t.startsWith('system.')), isFalse,
        reason: 'system.views 泄漏到用户集合里(探针实测到的坑)');
    // 视图不能混进集合列表
    expect(tables, isNot(contains('paid_view')));

    expect(await driver.listViews(_db), contains('paid_view'));
  });

  test('实连:读不存在的库返回空而不是报错 —— 服务端就这么含糊(实测钉住)', () async {
    // mongosh 同场景实测:listCollections → []，find → []，count → {n:0, ok:1}。
    // 驱动无从把「库不存在」和「库是空的」区分开,所以树里只能都渲染成空库;
    // 这条用例的价值是把该边界钉住,别人改成「抛异常」时这里会红。
    expect(await driver.listTables('no_such_db_zzz'), isEmpty);
    expect(await driver.listViews('no_such_db_zzz'), isEmpty);
    expect(await driver.countTable('no_such_db_zzz', 'foo'), 0);
  });

  test('实连:行数估算按集合逐个取,空集合给 0 而不是缺键', () async {
    final estimates = await driver.listTableRowEstimates(_db);
    expect(estimates[_orders], 3);
    expect(estimates['empty_coll'], 0);
    expect(estimates.keys, isNot(contains('system.views')));
    expect(await driver.countTable(_db, _orders), 3);
  });

  test('实连:集合首页取回 BSON 各类型,单元格是文本不是壳', () async {
    final p = await driver.previewTable(_db, _orders, limit: 10);
    expect(p.columns, contains('_id'));
    expect(p.columns, containsAll(<String>['sku', 'qty', 'created', 'meta']));
    expect(p.rows.length, 3);
    expect(p.nullMask!.length, p.rows.length);
    for (final row in p.rows) {
      expect(row.length, p.columns.length);
    }

    final idCol = p.columns.indexOf('_id');
    // ObjectId 应该是 24 位十六进制本体,而不是 ObjectId("…")
    expect(p.rows.first[idCol], matches(r'^[0-9a-f]{24}$'));

    if (p.columns.contains('created')) {
      final created = p.columns.indexOf('created');
      expect(p.rows[0][created], matches(r'^\d{4}-\d{2}-\d{2}T'));
    }
    // 二进制列走文本化,不登记十六进制列(那是 MySQL 的恢复投影)
    expect(p.hexColumns, isNull);
  });

  test('实连:点列头排序真的排,跨库切换后不串数据', () async {
    final asc =
        await driver.previewTable(_db, _orders, limit: 10, orderBy: 'sku ASC');
    final skus = asc.rows.map((r) => r[asc.columns.indexOf('sku')]).toList();
    // 只缺 sku 的那条按 BSON 排序规则排在字符串之前,故只断言两条已知值的相对次序
    expect(skus.indexOf('A-1001') < skus.indexOf('A-1002'), isTrue,
        reason: '升序没生效:$skus');

    final desc = await driver.previewTable(_db, _orders,
        limit: 10, orderBy: '`sku` DESC');
    final descSkus =
        desc.rows.map((r) => r[desc.columns.indexOf('sku')]).toList();
    expect(descSkus.indexOf('A-1002') < descSkus.indexOf('A-1001'), isTrue,
        reason: '降序没生效:$descSkus');

    // 排序串不是「单列 [ASC|DESC]」就必须拒绝,不能猜
    await expectLater(
        driver.previewTable(_db, _orders, orderBy: 'sku, qty'),
        throwsUnsupportedError);

    // 同一驱动实例切到另一个库再切回来:两次都要拿到自己的数据
    final users = await driver.previewTable(_db2, 'users', limit: 10);
    expect(users.columns, contains('name'));
    expect(users.rows.length, 2);
    final back = await driver.previewTable(_db, _orders, limit: 10);
    expect(back.rows.length, 3, reason: 'databaseName 没切回来,读成空表了');
  });

  test('实连:WHERE 过滤器直接拒绝(不能把没过滤的数据当成过滤过的)', () async {
    await expectLater(
        driver.previewTable(_db, _orders, where: "sku = 'A-1001'"),
        throwsUnsupportedError);
    await expectLater(driver.countTable(_db, _orders, where: 'qty > 1'),
        throwsUnsupportedError);
  });

  test('实连:SQL 执行拒绝,describeTable 从样本推断列并把 _id 记为主键', () async {
    await expectLater(driver.executeQuery('db.orders.find()'),
        throwsUnsupportedError);

    final cols = await driver.describeTable(_db, _orders);
    final id = cols.firstWhere((c) => c.name == '_id');
    expect(id.primaryKey, isTrue);
    final sku = cols.firstWhere((c) => c.name == 'sku');
    expect(sku.type, 'string');
    // 类型标注不能被 runtimeType 兜底成小写壳(裸类型名模式的坑)
    expect(cols.map((c) => c.type), isNot(contains('objectid')));
  });

  test('实连:集合详情 / 视图定义能读,设计器与库详情按能力退化', () async {
    final detail = await driver.readTableDetail(_db, _orders);
    expect(detail?.rowEstimate, 3);
    expect(detail?.name, _orders);

    final viewSql = await driver.getDefinition(_db, 'paid_view', 'view');
    expect(viewSql, allOf(contains('createView'), contains('paid_view')));

    // 文档库没有声明式 schema:设计器读不出基线,界面才不会拿着空表覆盖真表
    expect(await driver.readTableDesign(_db, _orders), isNull);
    expect(await driver.readDatabaseDetail(_db), isNull);
  });

  test('实连:会话 id 可取,取完之后原库照样读(currentOp 走 admin 的坑)', () async {
    final sid = await driver.serverSessionId();
    // 探针实测:currentOp / killOp 只能在 admin 库跑,驱动跑完要把库名切回来;
    // 没恢复的话这里就会拿到空列表(且服务端不报错,是最难发现的一类回归)。
    final tables = await driver.listTables(_db);
    expect(tables, contains(_orders));
    // 不真去 killSession:杀的是本用例自己这条连接,后面每条用例都会撞 Closed。
    expect(sid == null || sid > 0, isTrue);
  });

  test('实连:重复读同一页结果稳定(串行链不留半开状态)', () async {
    final a = await driver.previewTable(_db, _orders, limit: 2);
    final b = await driver.previewTable(_db, _orders, limit: 2);
    expect(b.rows, a.rows);
    expect(b.columns, a.columns);
  });
}
