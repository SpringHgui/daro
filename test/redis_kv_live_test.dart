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
  /// 写回环全部打在 **db1**(探针库里是空库):种在 db0 的 2000+ 样本数据
  /// 一条都不碰,跑完还能验回 0。
  late KvNamespace db1;

  /// 把带过滤的扫描翻到底:SCAN 的空页只代表「这一轮没命中」,
  /// 只有 [KvKeyPage.done] 之后才能断定命中了什么(或什么都没命中)。
  Future<List<KvKeyEntry>> _drainIn(
    KvNamespace ns, {
    KvBytes? pattern,
    String? type,
  }) async {
    final all = <KvKeyEntry>[];
    String? cursor;
    for (var i = 0; i < 200; i++) {
      final page = await driver.scanKeys(KvKeyScan(
        namespace: ns,
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

  Future<List<KvKeyEntry>> _drain({KvBytes? pattern, String? type}) =>
      _drainIn(db0, pattern: pattern, type: type);

  setUpAll(() async {
    driver = createKvDriver(conn)!;
    await driver.connect();
    final ns = await driver.listNamespaces();
    db0 = ns.firstWhere((n) => n.id == '0');
    db1 = ns.firstWhere((n) => n.id == '1');
  });

  tearDownAll(() async {
    // 探针键都在 `daro:ui_probe:` 前缀下:整批复删,不依赖用例自己收尾
    final left = await _drainIn(db1, pattern: KvBytes.text('daro:ui_probe:*'));
    if (left.isNotEmpty) {
      await driver.deleteKeys(db1, [for (final e in left) e.key]);
    }
    expect(await driver.keyCount(db1), 0, reason: '写回环必须把探针库清干净');
    await driver.close();
  });

  test('工厂与能力门:redis 走 KV 抽象而不是 SQL 驱动', () {
    expect(isKvStoreConnection(conn), isTrue);
    expect(driver.isConnected, isTrue);
    expect(driver.capabilities.multipleNamespaces, isTrue);
    expect(driver.capabilities.cursorScan, isTrue);
    expect(driver.capabilities.writable(KvKeyType.string), isTrue);
    expect(driver.capabilities.writable(KvKeyType.stream), isFalse);
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
    final page = await driver.scanKeys(KvKeyScan(namespace: db1, count: 100));
    expect(page.entries, isEmpty);
    expect(await driver.keyCount(db1), 0);
    final missing = await driver.readValue(db1, KvBytes.text('nope'));
    expect(missing.type, KvKeyType.none);
    expect(missing.rows, isEmpty);
  });

  // ─────────────────────────────────────── 写回环(全部打在 db1 探针库)

  /// 本文件写路径专用键名前缀:`tearDownAll` 按它整批清场
  KvBytes probe(String name) => KvBytes.text('daro:ui_probe:$name');

  test('写回环 · 字符串:新建即落库,改值保 TTL,显式 TTL 重算起算点', () async {
    final k = probe('str');
    await driver.setStringValue(db1, k, KvBytes.text('v1'));
    var read = await driver.readValue(db1, k);
    expect(read.type, KvKeyType.string);
    expect(read.rows.single.value.text, 'v1');
    expect(read.ttlSeconds, -1, reason: '新建裸键没有过期时间');

    // 先给一个 TTL,再只改值 —— 裸 SET 会把过期时间清掉,UI 上就是「改个值,
    // 键变成了永久」,所以驱动必须发 KEEPTTL。这条断言只有真服务端能验。
    expect(await driver.setTtl(db1, [k], 300), 1);
    await driver.setStringValue(db1, k, KvBytes.text('v2'));
    read = await driver.readValue(db1, k);
    expect(read.rows.single.value.text, 'v2');
    expect(read.ttlSeconds, greaterThan(0), reason: 'SET KEEPTTL 必须保住 TTL');
    expect(read.ttlSeconds, lessThanOrEqualTo(300));

    // 显式传 TTL 时改用 EX:从此刻起算,不是保留旧值
    await driver.setStringValue(db1, k, KvBytes.text('v3'), ttlSeconds: 60);
    read = await driver.readValue(db1, k);
    expect(read.rows.single.value.text, 'v3');
    expect(read.ttlSeconds, lessThanOrEqualTo(60));

    // 过期时间能清:PERSIST 之后 TTL 回 -1
    expect(await driver.setTtl(db1, [k], null), 1);
    read = await driver.readValue(db1, k);
    expect(read.ttlSeconds, -1);
  });

  test('写回环 · 字符串:二进制值整段往返,不经过 UTF-8 也不被截断', () async {
    final k = probe('strbin');
    final raw = KvBytes([0, 255, 128, 0xC3, 0x28, 1, 254]);
    await driver.setStringValue(db1, k, raw);
    final read = await driver.readValue(db1, k);
    expect(read.rows.single.value.isBinary, isTrue);
    expect(read.rows.single.value.hex, '00FF80C32801FE');
    expect(read.totalLength, raw.bytes.length);
  });

  test('写回环 · 二进制键名:能写进去、扫得出来、也删得掉', () async {
    final k = KvBytes([0xff, 0xfe, 0x00, 0x41]);
    await driver.setStringValue(db1, k, KvBytes.text('binary key name'));
    final found = await _drainIn(db1);
    expect(found.map((e) => e.key.bytes).any((b) =>
        b.length == 4 && b[0] == 0xff && b[1] == 0xfe && b[2] == 0 && b[3] == 0x41),
        isTrue,
        reason: 'SCAN 不能按字符串解码键名,否则字节键名会消失');
    expect(await driver.deleteKeys(db1, [k]), 1);
  });

  test('写回环 · 哈希:一句 HSET 同时改旧字段与加新字段,HDEL 只删点名的', () async {
    final k = probe('hash');
    await driver.hashWrite(db1, k, set: [
      (KvBytes.text('name'), KvBytes.text('daro')),
      (KvBytes.text('role'), KvBytes.text('tool')),
    ]);
    var read = await driver.readValue(db1, k);
    expect(read.type, KvKeyType.hash);
    expect(read.totalCount, 2);
    expect(read.rows.map((r) => r.field!.text).toList(), ['name', 'role']);

    // 改值 + 新增走同一条 HSET:服务端自己区分覆盖/插入,回包只是计数
    await driver.hashWrite(db1, k, set: [
      (KvBytes.text('name'), KvBytes.text('daro-desktop')),
      (KvBytes.text('extra'), KvBytes.text('1')),
    ]);
    read = await driver.readValue(db1, k);
    expect(read.totalCount, 3);
    expect(read.rows.first.value.text, 'daro-desktop');

    await driver.hashWrite(db1, k, removeFields: [KvBytes.text('role')]);
    read = await driver.readValue(db1, k);
    expect(read.rows.map((r) => r.field!.text).toList(), ['name', 'extra']);
    // 字段值可以为空串(种子数据里就有 empty=),不能被判成「没内容」而漏掉
    await driver.hashWrite(
        db1, k, set: [(KvBytes.text('empty'), KvBytes.text(''))]);
    read = await driver.readValue(db1, k);
    expect(read.rows.last.field!.text, 'empty');
    expect(read.rows.last.value.text, '');
  });

  test('写回环 · 列表:下标增删改都按位置生效,重复值不会删错行', () async {
    final k = probe('list');
    // RPUSH / LPUSH 各自建键与追加,顺序即断言
    await driver.listWrite(db1, k, append: KvBytes.text('a'));
    await driver.listWrite(db1, k, append: KvBytes.text('dup'));
    await driver.listWrite(db1, k, prepend: KvBytes.text('head'));
    await driver.listWrite(db1, k, append: KvBytes.text('b'));
    await driver.listWrite(db1, k, append: KvBytes.text('dup'));
    Future<List<String>> shown() async {
      final r = await driver.readValue(db1, k);
      return [for (final row in r.rows) row.value.text ?? row.value.display];
    }

    expect(await shown(), ['head', 'a', 'dup', 'b', 'dup']);

    await driver.listWrite(db1, k, set: [(2, KvBytes.text('x'))]);
    expect(await shown(), ['head', 'a', 'x', 'b', 'dup']);

    // 关键:LREM 按值删。若直接对下标 2 发 LREM,删掉的会是「dup」而不是
    // 用户点中的那一行。哨兵替换法必须让末尾的 dup 活下来。
    await driver.listWrite(db1, k, removeIndexes: [2]);
    expect(await shown(), ['head', 'a', 'b', 'dup']);

    // 多行连删(先全部替换再逐个删,中途不位移)
    await driver.listWrite(db1, k, removeIndexes: [0, 2]);
    expect(await shown(), ['a', 'dup']);

    // 越界下标是服务端错误,原样上抛而不是静默少删
    await expectLater(
      driver.listWrite(db1, k, removeIndexes: [99]),
      throwsA(isA<KvServerException>()),
    );
  });

  test('写回环 · 集合与有序集:成员去重、分数覆盖、ZREM/SREM 精确删除', () async {
    final s = probe('set');
    await driver.setWrite(db1, s, add: [
      KvBytes.text('hot'),
      KvBytes.text('daro'),
      KvBytes.text('hot'), // 重复成员:SADD 去重,不应变成两条
    ]);
    var read = await driver.readValue(db1, s);
    expect(read.totalCount, 2);
    expect(read.rows.map((r) => r.value.text).toSet(), {'hot', 'daro'});

    await driver.setWrite(db1, s, removeMembers: [KvBytes.text('hot')]);
    read = await driver.readValue(db1, s);
    expect(read.rows.map((r) => r.value.text).toList(), ['daro']);

    final z = probe('zset');
    await driver.zsetWrite(db1, z, add: [
      (KvBytes.text('alice'), 100),
      (KvBytes.text('bob'), 92.5),
    ]);
    read = await driver.readValue(db1, z);
    // ZRANGE WITHSCORES:低分在前
    expect(read.rows.map((r) => r.value.text).toList(), ['bob', 'alice']);
    expect(read.rows.map((r) => r.score).toList(), [92.5, 100.0]);

    // 改分数与加成员同一句 ZADD:负分与小数都要原样落库
    await driver.zsetWrite(db1, z, add: [
      (KvBytes.text('alice'), -3.25),
      (KvBytes.text('carol'), 1),
    ]);
    read = await driver.readValue(db1, z);
    expect(read.rows.map((r) => r.value.text).toList(), ['alice', 'carol', 'bob']);
    expect(read.rows.first.score, -3.25);

    await driver.zsetWrite(db1, z, removeMembers: [KvBytes.text('carol')]);
    read = await driver.readValue(db1, z);
    expect(read.rows.map((r) => r.value.text).toList(), ['alice', 'bob']);
  });

  test('写回环 · DEL 批量:只数真正删掉的,不存在的键不计且不报错', () async {
    final keys = [probe('del1'), probe('del2'), probe('ghost')];
    await driver.setStringValue(db1, keys[0], KvBytes.text('1'));
    await driver.setStringValue(db1, keys[1], KvBytes.text('2'));
    expect(await driver.deleteKeys(db1, keys), 2);
    expect(await driver.deleteKeys(db1, keys), 0, reason: '再删一次全都不存在');
    expect(await driver.deleteKeys(db1, const []), 0);
  });

  test('写回环 · setTtl 如实报数:键在两次点击之间过期了不算失败', () async {
    final k = probe('ttlcount');
    await driver.setStringValue(db1, k, KvBytes.text('v'));
    expect(
      await driver.setTtl(db1, [k, probe('nope-ttl')], 120),
      1,
      reason: '不存在的键 EXPIRE 回 0,生效数小于选中数是正常结果',
    );
    expect(await driver.setTtl(db1, [k], null), 1);
  });

  test('写回环 · 类型不符:服务端 WRONGTYPE 原文上抛,连接之后照常用', () async {
    final k = probe('wrongtype');
    await driver.setStringValue(db1, k, KvBytes.text('not a hash'));
    await expectLater(
      driver.hashWrite(db1, k, set: [(KvBytes.text('f'), KvBytes.text('v'))]),
      throwsA(isA<KvServerException>()
          .having((e) => e.message, 'message', contains('WRONGTYPE'))),
    );
    // 错误不能让解析链错位:同一会话下一次读必须对得上
    expect((await driver.readValue(db1, k)).rows.single.value.text,
        'not a hash');
    expect((await driver.runCommand(db1, ['PING'])).summary, 'PONG');
    await driver.deleteKeys(db1, [k]);
  });

  test('写回环 · 能力表外类型不给入口:stream 只读', () async {
    expect(driver.capabilities.writable(KvKeyType.stream), isFalse);
    final k = probe('stream');
    await driver.runCommand(db1, ['XADD', k.text!, '*', 'note', 'seed']);
    expect(await _drainIn(db1, type: 'stream'), isNotEmpty);
    // 页面对只读类型压根不发写命令;驱动层也不为 stream 提供写方法(编译期就挡了)
    await driver.deleteKeys(db1, [k]);
  });
}
