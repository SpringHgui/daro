import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/kv_driver.dart';

/// 实连回归:用真 Redis 验证 [KvStoreDriver] 抽象是否够用。
///
/// 默认**不跑**:`DARO_LIVE_DB=1 flutter test test/redis_kv_live_test.dart`
/// 密码只从环境变量读(`DARO_REDIS_PWD`),仓库里不留凭据。
///
/// 目标库是本地探针容器(`.redis_probe/seed*.sh` 种的数据):
/// db0 约 2000+ 个 `scan:bench:*` 键 + 五种类型 + 真二进制值 `bin:blob2`
/// + 二进制键名 + 200KB 大值 `big:huge`,db15 一个键,db1 空。
void main() {
  final enabled = Platform.environment['DARO_LIVE_DB'] == '1';
  final host = Platform.environment['DARO_REDIS_HOST'] ?? '127.0.0.1';
  final port = Platform.environment['DARO_REDIS_PORT'] ?? '6379';
  final pwd = Platform.environment['DARO_REDIS_PWD'] ?? '';
  final skip = enabled
      ? null
      : '未开启实连(DARO_LIVE_DB=1 时才跑)';

  if (skip != null) {
    // ignore: avoid_print
    print('实连测试跳过:$skip');
    test('实连:KV 抽象(跳过)', () {}, skip: skip);
    return;
  }

  final conn = ConnectionInfo(
    name: 'Redis 探针',
    typeId: 'redis',
    host: host,
    port: port,
    username: '',
    password: pwd,
    isLive: true,
  );

  late KvStoreDriver driver;
  late KvNamespace db0;

  setUpAll(() async {
    driver = createKvDriver(conn)!;
    await driver.connect();
    final ns = await driver.listNamespaces();
    db0 = ns.firstWhere((n) => n.id == '0');
  });

  tearDownAll(() async => driver.close());

  /// 把带过滤的扫描翻到底:SCAN 的空页只代表「这一轮没命中」,
  /// 只有 [KvKeyPage.done] 之后才能断定命中了什么(或什么都没命中)。
  Future<List<KvKeyEntry>> _drain({KvBytes? pattern, String? type}) async {
    final all = <KvKeyEntry>[];
    String? cursor;
    for (var i = 0; i < 200; i++) {
      final page = await driver.scanKeys(KvKeyScan(
        namespace: db0,
        cursor: cursor,
        pattern: pattern,
        type: type,
        count: 200,
      ));
      all.addAll(page.entries);
      cursor = page.nextCursor;
      if (cursor == null) return all;
    }
    throw StateError('过滤扫描在 200 页内没有收敛(游标始终不为零)');
  }

  test('工厂与能力门:redis 走 KV 抽象而不是 SQL 驱动', () {
    expect(isKvStoreConnection(conn), isTrue);
    expect(driver.isConnected, isTrue);
    expect(driver.capabilities.multipleNamespaces, isTrue);
    expect(driver.capabilities.cursorScan, isTrue);
    expect(driver.capabilities.readOnlyFirstVersion, isTrue);
    expect(driver.capabilities.supports(KvKeyType.stream), isTrue);
  });

  test('命名空间:16 个库,计数只来自 INFO keyspace', () async {
    final ns = await driver.listNamespaces();
    expect(ns.length, greaterThanOrEqualTo(16));
    expect(ns.map((n) => n.label).take(3).toList(), ['db0', 'db1', 'db2']);
    final db15 = ns.firstWhere((n) => n.id == '15');
    expect(db15.keyCount, 1);
    // db1 是空库:显示 0(不是横杠 —— DBSIZE 给得出确定值)
    expect(ns.firstWhere((n) => n.id == '1').keyCount, 0);
    expect(await driver.keyCount(db0), greaterThan(2000));
  });

  test('SCAN 分页:游标不为零时能续扫,归零即结束', () async {
    final first = await driver.scanKeys(KvKeyScan(namespace: db0, count: 100));
    expect(first.entries, isNotEmpty);
    expect(first.done, isFalse, reason: '两千键不可能一页扫完');
    expect(first.totalEstimate, greaterThan(2000));

    var cursor = first.nextCursor;
    var pages = 1;
    final all = <String>[for (final e in first.entries) e.key.display];
    while (cursor != null && pages < 60) {
      final page = await driver
          .scanKeys(KvKeyScan(namespace: db0, cursor: cursor, count: 200));
      cursor = page.nextCursor;
      pages++;
      all.addAll([for (final e in page.entries) e.key.display]);
    }
    expect(cursor, isNull, reason: '游标最终必须归零');
    expect(all.toSet().length, all.length, reason: '游标遍历不应重复给同一个键');
    expect(all.any((k) => k.startsWith('scan:bench:')), isTrue);
  });

  test('SCAN 过滤:MATCH 与 TYPE 都在服务端做', () async {
    final bench = await driver.scanKeys(KvKeyScan(
        namespace: db0,
        pattern: KvBytes.text('scan:bench:*'),
        count: 200));
    expect(bench.entries, isNotEmpty);
    expect(
        bench.entries.every((e) => e.key.text!.startsWith('scan:bench:')), isTrue);

    // 实测:db0 有 2025 个键但只有 1 个 hash,`SCAN 0 COUNT 200 TYPE hash`
    // 回的是「游标 744 + 空数组」。空页只说明这一轮没命中,不代表库里没有 ——
    // 必须一路翻到 done 才能下结论,所以这里循环取页而不只看第一页。
    final firstHashPage = await driver
        .scanKeys(KvKeyScan(namespace: db0, type: 'hash', count: 200));
    expect(firstHashPage.entries, isEmpty, reason: '命中稀少的过滤首轮就是空页');
    expect(firstHashPage.done, isFalse, reason: '空页 ≠ 扫完');

    final hashes = await _drain(type: 'hash');
    expect(hashes.map((e) => e.key.text), contains('app:session:s1'));
    final metas = await driver
        .readKeyMeta(db0, [for (final e in hashes.take(5)) e.key]);
    expect(metas, isNotEmpty);
    expect(metas.every((m) => m.type == KvKeyType.hash), isTrue);

    // 反向:筛选条件命中不了时,翻到底应当是「空」而不是「异常/卡住」
    expect(await _drain(type: 'stream', pattern: KvBytes.text('no-such:*')),
        isEmpty);
  });

  test('二进制键名不会被当成解码失败(整库扫描里就能拿到)', () async {
    var cursor = '0';
    KvBytes? binaryKey;
    do {
      final page = await driver
          .scanKeys(KvKeyScan(namespace: db0, cursor: cursor, count: 200));
      cursor = page.nextCursor ?? '0';
      for (final e in page.entries) {
        if (e.key.isBinary) {
          binaryKey = e.key;
          break;
        }
      }
    } while (cursor != '0' && binaryKey == null);

    expect(binaryKey, isNotNull, reason: '种进去的 \\xff\\xfe… 键名必须被读到');
    expect(binaryKey!.text, isNull);
    expect(binaryKey.hex, isNotEmpty);
    // 拿到字节键名后,元数据与值都应按这个键回读成功
    final meta = (await driver.readKeyMeta(db0, [binaryKey])).single;
    expect(meta.type, KvKeyType.string);
    final value = await driver.readValue(db0, binaryKey);
    expect(value.rows.single.value.text, 'binval');
  });

  test('批量元数据:类型 / TTL / 编码 / 字节数各就各位', () async {
    final keys = [
      'app:config:version', // string, 无 TTL
      'app:user:1001', // string, 有 TTL
      'app:session:s1', // hash
      'queue:tasks', // list
      'tag:post:1', // set
      'rank:score', // zset
      'events:orders', // stream
      'no-such-key-at-all', // 不存在
    ].map(KvBytes.text).toList();
    final metas = await driver.readKeyMeta(db0, keys);
    expect(metas.length, keys.length);
    expect(metas.map((m) => m.type).toList(), [
      KvKeyType.string,
      KvKeyType.string,
      KvKeyType.hash,
      KvKeyType.list,
      KvKeyType.set,
      KvKeyType.zset,
      KvKeyType.stream,
      KvKeyType.none,
    ]);
    expect(metas[0].hasTtl, isFalse);
    expect(metas[1].ttlSeconds, greaterThan(0));
    expect(metas[0].encoding, anyOf('embstr', 'raw'));
    expect(metas[2].encoding, 'listpack');
    expect(metas[7].ttlSeconds, -2); // 不存在的键:-2
    expect(metas[7].byteSize, isNull); // MEMORY USAGE 回 nil,不是 0
    expect(metas[0].byteSize, greaterThan(0));
  });

  test('字符串值:文本 / 真二进制 / 大值截断', () async {
    final txt = await driver.readValue(db0, KvBytes.text('txt:len70'));
    expect(txt.type, KvKeyType.string);
    expect(txt.totalLength, 70);
    expect(txt.rows.single.value.text, 'E' * 70);
    expect(txt.truncated, isFalse);

    // bin:blob2 是真 7 字节 00 ff 80 c3 28 1a 04:UTF-8 解不动 → HEX 展示
    final bin = await driver.readValue(db0, KvBytes.text('bin:blob2'));
    expect(bin.rows.single.value.isBinary, isTrue);
    expect(bin.rows.single.value.hex, '00FF80C3281A04');
    expect(bin.rows.single.value.display, 'HEX:00FF80C3281A04');

    final big = await driver.readValue(db0, KvBytes.text('big:huge'), limit: 4096);
    expect(big.totalLength, 200000);
    expect(big.rows.single.value.byteLength, 4096);
    expect(big.truncated, isTrue);
    expect(big.rangeStart, 0);

    // 偏移窗口:取中段而不是重头
    final mid = await driver.readValue(db0, KvBytes.text('big:huge'),
        offset: 100000, limit: 100);
    expect(mid.rows.single.value.byteLength, 100);
    expect(mid.rangeStart, 100000);
    expect(mid.truncated, isTrue);
  });

  test('容器类型:各类型的行结构都能落到同一张表上', () async {
    final hash = await driver.readValue(db0, KvBytes.text('app:session:s1'));
    expect(hash.totalCount, 4);
    expect(hash.rows.map((r) => r.field!.text).toList(),
        ['user', 'ip', 'ua', 'empty']);
    expect(hash.rows.first.value.text, '1001');

    final list = await driver.readValue(db0, KvBytes.text('queue:tasks'));
    expect(list.totalCount, 5);
    expect(list.rows.first.index, 0);
    expect(list.rows.first.value.text, '开始处理');

    final set = await driver.readValue(db0, KvBytes.text('tag:post:1'));
    expect(set.totalCount, 4);
    expect(set.rows.every((r) => r.field == null), isTrue);
    expect(set.rows.map((r) => r.value.text).toSet(),
        {'热门', '推荐', 'redis', 'daro'});

    final zset = await driver.readValue(db0, KvBytes.text('rank:score'));
    expect(zset.totalCount, 3);
    expect(zset.rows.map((r) => r.value.text).toList(), ['carol', 'bob', 'alice']);
    expect(zset.rows.map((r) => r.score).toList(), [-3, 92.5, 100]);

    final stream = await driver.readValue(db0, KvBytes.text('events:orders'));
    expect(stream.totalCount, greaterThanOrEqualTo(4));
    expect(stream.rows.length, stream.totalCount);
    expect(stream.rows.every((r) => (r.entryId ?? '').contains('-')), isTrue);
    expect(stream.rows.first.field!.text, stream.rows.first.entryId);
    // 中文与二进制混在同一条流里:值要么解码,要么 HEX,不能整体失败
    final joined = [for (final r in stream.rows) r.value.text].join('|');
    expect(joined, contains('中文备注'));
    expect(joined, contains('HEX:FFFE62696E'));
  });

  test('服务端错误是 KvServerException,且连接不受影响', () async {
    // 对 list 发 HGETALL:典型 WRONGTYPE
    await expectLater(
      driver.runCommand(db0, ['HGETALL', 'queue:tasks']),
      throwsA(isA<KvServerException>()),
    );
    // 实测:这类错误不破坏解析链 —— 之后同连接照常用
    final pong = await driver.runCommand(db0, ['PING']);
    expect(pong.summary, 'PONG');
    final meta = await driver.readKeyMeta(db0, [KvBytes.text('queue:tasks')]);
    expect(meta.single.type, KvKeyType.list);
  });

  test('控制台回包结构:整数 / 简单串 / bulk / 数组 / nil 各归其类', () async {
    expect((await driver.runCommand(db0, ['PING'])).kind, KvReplyKind.simple);
    expect((await driver.runCommand(db0, ['DBSIZE'])).kind, KvReplyKind.integer);
    expect((await driver.runCommand(db0, ['GET', 'app:config:version'])).kind,
        KvReplyKind.bulk);
    expect((await driver.runCommand(db0, ['GET', 'no-such-key-at-all'])).kind,
        KvReplyKind.nil);
    final keys = await driver.runCommand(db0, ['KEYS', 'txt:*']);
    expect(keys.kind, KvReplyKind.array);
    expect((keys.items ?? []).length, greaterThanOrEqualTo(4));
    final info = await driver.runCommand(db0, ['INFO', 'keyspace']);
    expect(info.render().any((l) => l.contains('db0:keys=')), isTrue);
  });

  test('服务器概览:INFO 分段与关键指标都解析出来了', () async {
    final s = await driver.readServerInfo();
    expect(s, isNotNull);
    expect(s!.engineLabel, 'Redis');
    expect(s.version, isNotEmpty);
    expect(s.sections.map((x) => x.name), contains('Keyspace'));
    expect(s.metrics.containsKey('used_memory_human'), isTrue);
  });

  test('切库互不串味:同一驱动实例上两个库各自独立', () async {
    final db15 = (await driver.listNamespaces()).firstWhere((n) => n.id == '15');
    final c15 = await driver.keyCount(db15);
    final c0 = await driver.keyCount(db0);
    expect(c15, 1);
    expect(c0, greaterThan(2000));
    // 交替读:若两个库共用一条连接又没串行化,这里必然读到对方的库
    final k15 = await driver.scanKeys(KvKeyScan(namespace: db15, count: 10));
    final k0 = await driver.scanKeys(KvKeyScan(namespace: db0, count: 10));
    expect(k15.entries.length, 1);
    expect(k0.entries.length, 10);
    expect(await driver.keyCount(db15), c15, reason: '读 db0 之后 db15 计数不应被带跑');
  });

  test('空库与不存在的键:回空集合而不是报错', () async {
    final db1 = (await driver.listNamespaces()).firstWhere((n) => n.id == '1');
    final page = await driver.scanKeys(KvKeyScan(namespace: db1, count: 100));
    expect(page.entries, isEmpty);
    expect(await driver.keyCount(db1), 0);
    final missing = await driver.readValue(db1, KvBytes.text('nope'));
    expect(missing.type, KvKeyType.none);
    expect(missing.rows, isEmpty);
  });
}
