import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:redis/redis.dart' as redis;

import '../db_data.dart';
import 'kv_driver.dart';

/// Redis 的 [KvStoreDriver] 实现。
///
/// 形状由真 Redis 8.10.2 的实测结论决定(`.redis_probe/SELECTION.md`),五条硬约束:
///
/// 1. **每个库一条连接。** `SELECT` 改的是连接上的共享状态,同一条连接并发操作
///    会打到互错的库上,只能整体串行 —— 那等于自废流水线。改成按库号惰性持有
///    独立会话([_Session]),库间并发天然安全,同库内的批量元数据仍能连发。
/// 2. **解析器必须是 `RedisParserBulkBinary`。** 默认解析器对二进制值走
///    `UTF8.decode`,一旦抛 `FormatException`,包内部的串行 future 链被打断,
///    **这条连接之后每条命令都返回同一个异常**(实测连打 3 次 PING 全废)。
///    键名也走同一条路(`KEYS` 命中 `\xff…` 键名即炸)。所以这里一律按字节收,
///    文本解码延后到 [KvBytes.text],解不动就是二进制 —— 不再让解码异常发生。
/// 3. **错误分两类。** 服务端错误(`WRONGTYPE` / `NOAUTH` / `ERR …`)包成
///    [KvServerException],连接照常用;其余异常(解码 / 关闭 / IO)说明这条连接
///    的状态已经不可信,标死并从会话表里摘掉,下次调用自然重连。
/// 4. **没有逐命令超时。** 包的读链是串行的,`Future.timeout()` 只会让调用方提前
///    返回而响应仍被链上吃掉 —— 协议不错位,但**队头阻塞**(实测被弃的 BLPOP 3s
///    让后续 PING 等了 2829ms)。所以阻塞类命令不进本驱动,连接建立之外的超时
///    也不假装能实现。
/// 5. **写没有事务可用,所以一次调用只打一个键。** 跨键的「批量」(删键、批量
///    EXPIRE、列表按下标删)都是连发,中途失败前面几条已经落库。界面因此不许
///    把两个键的写合成一句「要么都成」的操作,报错也必须把服务端原文整条给出。
class RedisKvDriver implements KvStoreDriver {
  RedisKvDriver(this._conn);

  final ConnectionInfo _conn;

  /// 库号('0'..'N-1')→ 会话。断开时整个清空。
  final Map<String, _Session> _sessions = {};

  /// 建连超时。Redis 的 `Socket.connect` 不接受超时参数,只能从外面套。
  static const Duration _connectTimeout = Duration(seconds: 5);

  /// 值预览的单段上限:网格只渲染可见单元格,但一次 GET 200KB 也得进内存,
  /// 所以字符串走 GETRANGE 窗口(实测整取 200KB 3ms,窗口取 0ms)。
  static const int _kValuePreviewBytes = 64 * 1024;

  @override
  KvCapabilities get capabilities => const KvCapabilities(
        multipleNamespaces: true,
        cursorScan: true,
        keyTypes: {
          KvKeyType.string,
          KvKeyType.list,
          KvKeyType.set,
          KvKeyType.zset,
          KvKeyType.hash,
          KvKeyType.stream,
        },
        ttl: true,
        internalEncoding: true,
        memoryUsage: true,
        valueOffsetRead: true,
        idleTime: true,
        commandConsole: true,
        // stream 不进集合:XADD 只能追加,改已有条目不是常规操作,界面不给入口
        writableTypes: {
          KvKeyType.string,
          KvKeyType.list,
          KvKeyType.set,
          KvKeyType.zset,
          KvKeyType.hash,
        },
        maxValuePreviewBytes: _kValuePreviewBytes,
      );

  @override
  bool get isConnected => _sessions.values.any((s) => s.connected);

  @override
  Future<void> connect() async {
    // 不在这里建第一条连接:会话按库号惰性建立,连不上时由第一条命令如实抛错。
    // 只拿一次库数量,好让 listNamespaces 不必硬编码 16。
    await _databaseCount();
  }

  @override
  Future<void> close() async {
    final all = _sessions.values.toList();
    _sessions.clear();
    for (final s in all) {
      await s.dispose();
    }
  }

  @override
  Future<List<KvNamespace>> listNamespaces() async {
    final count = await _databaseCount();
    // INFO keyspace 只列出「有键的库」,计数用它当估算值;空库给 0。
    final sizes = await _keyspaceSizes();
    return [
      for (var i = 0; i < count; i++)
        KvNamespace(id: '$i', label: 'db$i', keyCount: sizes['$i'] ?? 0),
    ];
  }

  @override
  Future<int?> keyCount(KvNamespace namespace) =>
      _sessionFor(namespace.id).then((s) => _int(s, ['DBSIZE']));

  @override
  Future<KvKeyPage> scanKeys(KvKeyScan request) async {
    final s = await _sessionFor(request.namespace.id);
    // COUNT 是提示值,服务端可以返回更少或略多;游标归零表示遍历完。
    // 一次调用只发一轮 SCAN:过滤命中稀少时服务端会回空页 + 非零游标
    // (见 KvKeyPage 注释),驱动若在这里自己循环到非空,「筛选不到任何键」
    // 就会变成一次静默的全库扫描 —— 大实例上这是几十万次往返。
    // 循环交给调用方,它才知道该停在哪。
    final args = <Object>['SCAN', request.cursor ?? '0', 'COUNT', '${request.count}'];
    final pattern = request.pattern;
    if (pattern != null && pattern.bytes.isNotEmpty) {
      // 键名可以是裸字节,MATCH 也必须按字节发,否则二进制键永远筛不到
      args.addAll(['MATCH', pattern]);
    }
    if (request.type != null) args.addAll(['TYPE', request.type!]);
    final raw = await _exec(s, args);
    final list = _list(raw);
    if (list.length < 2) {
      throw KvConnectionException('SCAN 回包结构异常(期望 [游标, 键数组])');
    }
    final cursor = _bytes(list[0]).text ?? '0';
    return KvKeyPage(
      entries: [
        for (final k in _list(list[1])) KvKeyEntry(key: _bytes(k)),
      ],
      // 游标归零 = 扫完(实测 Redis 回 '0')
      nextCursor: cursor == '0' ? null : cursor,
      totalEstimate: await _int(s, ['DBSIZE']),
    );
  }

  @override
  Future<List<KvKeyMeta>> readKeyMeta(
      KvNamespace namespace, List<KvBytes> keys) async {
    if (keys.isEmpty) return const [];
    final s = await _sessionFor(namespace.id);
    // 元数据分三层,按能力表决定取不取:
    //   TYPE+TTL 必取;ENCODING/MEMORY USAGE/OBJECT IDLETIME 拿不到就是 null。
    // 逐键 await 实测 100 次要 45ms,连发统一等只要 10ms —— 这里必须连发。
    final typeF = <Future<Object?>>[];
    final ttlF = <Future<Object?>>[];
    final encF = capabilities.internalEncoding ? <Future<Object?>>[] : null;
    final memF = capabilities.memoryUsage ? <Future<Object?>>[] : null;
    final idleF = capabilities.idleTime ? <Future<Object?>>[] : null;
    for (final k in keys) {
      typeF.add(_attempt(s, ['TYPE', k]));
      ttlF.add(_attempt(s, ['TTL', k]));
      encF?.add(_attempt(s, ['OBJECT', 'ENCODING', k]));
      memF?.add(_attempt(s, ['MEMORY', 'USAGE', k]));
      idleF?.add(_attempt(s, ['OBJECT', 'IDLETIME', k]));
    }
    final types = await Future.wait(typeF);
    final ttls = await Future.wait(ttlF);
    final encs = encF == null ? const <Object?>[] : await Future.wait(encF);
    final mems = memF == null ? const <Object?>[] : await Future.wait(memF);
    final idles = idleF == null ? const <Object?>[] : await Future.wait(idleF);
    return [
      for (var i = 0; i < keys.length; i++)
        KvKeyMeta(
          type: kvKeyTypeOf(_str(types[i])),
          ttlSeconds: _nullableInt(ttls[i]),
          encoding: _nullableStr(encs.isEmpty ? null : encs[i]),
          byteSize: _nullableInt(mems.isEmpty ? null : mems[i]),
          idleSeconds: _nullableInt(idles.isEmpty ? null : idles[i]),
        ),
    ];
  }

  @override
  Future<KvValueRead> readValue(KvNamespace namespace, KvBytes key,
      {int offset = 0, int? limit}) async {
    final s = await _sessionFor(namespace.id);
    // TYPE 与 TTL 一次往返取齐:TTL 只进 [KvValueRead.ttlSeconds],
    // 供值面板算「过期于几点」。键列表那列 TTL 是扫页时的旧值,不能拿来算绝对时刻。
    final head = await Future.wait<Object?>([
      _exec(s, ['TYPE', key]),
      _attempt(s, ['TTL', key]),
    ]);
    final type = kvKeyTypeOf(_str(head[0]));
    final ttl = _nullableInt(head[1]);
    final cap = limit ?? _kValuePreviewBytes;
    switch (type) {
      case KvKeyType.none:
        return KvValueRead(
            key: key, type: type, rows: const [], ttlSeconds: ttl);
      case KvKeyType.string:
        final total = await _int(s, ['STRLEN', key]);
        // 按窗口取:大值不至于整段进内存(实测 200KB 整取 3ms / 窗口 0ms)
        final take = total == null || total - offset <= cap
            ? (total ?? 0) - offset
            : cap;
        if (take <= 0) {
          return KvValueRead(
            key: key,
            type: type,
            rows: const [],
            totalLength: total,
            totalCount: total,
            truncated: (total ?? 0) > offset,
            rangeStart: offset,
            ttlSeconds: ttl,
          );
        }
        final bytes = _bytes(
            await _exec(s, ['GETRANGE', key, '$offset', '${offset + take - 1}']));
        return KvValueRead(
          key: key,
          type: type,
          rows: [KvValueRow(value: bytes)],
          totalLength: total,
          totalCount: total,
          truncated: (total ?? 0) > offset + bytes.byteLength,
          rangeStart: offset,
          ttlSeconds: ttl,
        );
      case KvKeyType.list:
        final count = await _int(s, ['LLEN', key]);
        final end = count == null ? offset + cap - 1 : offset + cap - 1;
        return _containerRead(
          key: key,
          type: type,
          count: count,
          ttl: ttl,
          raw: await _exec(s, ['LRANGE', key, '$offset', '$end']),
          rowOf: (i, e) => KvValueRow(value: _bytes(e), index: offset + i),
        );
      case KvKeyType.zset:
        final count = await _int(s, ['ZCARD', key]);
        final raw = await _exec(
            s, ['ZRANGE', key, '$offset', '${offset + cap - 1}', 'WITHSCORES']);
        // WITHSCORES 回扁平数组 [member, score, member, score…](实测)
        final items = _list(raw);
        final rows = <KvValueRow>[];
        for (var i = 0; i + 1 < items.length; i += 2) {
          rows.add(KvValueRow(
            value: _bytes(items[i]),
            score: double.tryParse(_bytes(items[i + 1]).text ?? ''),
          ));
        }
        return KvValueRead(
          key: key,
          type: type,
          rows: rows,
          totalCount: count,
          truncated: count != null && count > rows.length,
          ttlSeconds: ttl,
        );
      case KvKeyType.hash:
        return _pagedContainer(
          s: s,
          key: key,
          type: type,
          countCommand: const ['HLEN'],
          wholeCommand: const ['HGETALL'],
          scanCommand: const ['HSCAN'],
          rowsOf: _hashRows,
          cap: cap,
          ttl: ttl,
        );
      case KvKeyType.set:
        return _pagedContainer(
          s: s,
          key: key,
          type: type,
          countCommand: const ['SCARD'],
          wholeCommand: const ['SMEMBERS'],
          scanCommand: const ['SSCAN'],
          rowsOf: _setRows,
          cap: cap,
          ttl: ttl,
        );
      case KvKeyType.stream:
        final count = await _int(s, ['XLEN', key]);
        final raw = await _exec(s, ['XRANGE', key, '-', '+', 'COUNT', '$cap']);
        // 条目 = [id, [f1, v1, f2, v2]]:字段数组是扁平的,不是 [[f,v]](实测)
        final rows = <KvValueRow>[];
        for (final e in _list(raw)) {
          final entry = _list(e);
          final id = _bytes(entry[0]).text ?? '';
          final flat = _list(entry.length > 1 ? entry[1] : const []);
          final fields = <String>[];
          for (var i = 0; i + 1 < flat.length; i += 2) {
            fields.add('${_bytes(flat[i]).display}=${_bytes(flat[i + 1]).display}');
          }
          rows.add(KvValueRow(
            value: KvBytes(utf8.encode(fields.join('; '))),
            field: KvBytes(utf8.encode(id)),
            entryId: id,
          ));
        }
        return KvValueRead(
          key: key,
          type: type,
          rows: rows,
          totalCount: count,
          truncated: count != null && count > rows.length,
          ttlSeconds: ttl,
        );
      case KvKeyType.unknown:
        return KvValueRead(
            key: key, type: type, rows: const [], ttlSeconds: ttl);
    }
  }

  @override
  Future<KvReply> runCommand(KvNamespace namespace, List<String> arguments) async {
    final s = await _sessionFor(namespace.id);
    // 参数原样按字符串发(数字也是字符串);Redis 自己解析。
    final raw = await _exec(s, [for (final a in arguments) a]);
    return _replyOf(raw);
  }

  // ────────────────────────────────────────────── 写

  @override
  Future<void> setStringValue(KvNamespace namespace, KvBytes key, KvBytes value,
      {int? ttlSeconds}) async {
    final s = await _sessionFor(namespace.id);
    // KEEPTTL 是必须的:裸 SET 会连过期时间一起清掉,而用户改的只是值。
    // 需要 Redis ≥6.0(2020),更低版本服务端回 `ERR syntax error` 原样上抛 ——
    // 不做「先读 TTL 再 SET 再 EXPIRE」的兜底,那条路会把精度损失成整秒且非原子。
    await _exec(s, ttlSeconds == null
        ? ['SET', key, value, 'KEEPTTL']
        : ['SET', key, value, 'EX', '$ttlSeconds']);
  }

  @override
  Future<int> deleteKeys(KvNamespace namespace, List<KvBytes> keys) async {
    if (keys.isEmpty) return 0;
    final s = await _sessionFor(namespace.id);
    // 一条 DEL 删多个键:Redis 保证逐键删除,但不保证与并发读者之间的原子性
    final deleted = await _int(s, ['DEL', ...keys]);
    return deleted ?? 0;
  }

  @override
  Future<int> setTtl(
      KvNamespace namespace, List<KvBytes> keys, int? ttlSeconds) async {
    if (keys.isEmpty) return 0;
    final s = await _sessionFor(namespace.id);
    // 没有批量 EXPIRE,只能一键一句连发一次等齐(与 readKeyMeta 同一理由:
    // 逐键 await 实测慢 4 倍)。用 _exec 而不是 _attempt:_attempt 会把服务端
    // 错误吞成 null,写路径吞错就等于回「没生效」而不说为什么。
    final replies = await Future.wait<Object?>([
      for (final k in keys)
        _exec(s, ttlSeconds == null
            ? ['PERSIST', k]
            : ['EXPIRE', k, '$ttlSeconds']),
    ]);
    // EXPIRE/PERSIST 对不存在的键回 0 而不是报错,所以「生效数 < 选中数」是
    // 正常结果(键刚好在两次点击之间过期了),由调用方如实报数。
    return replies.where((r) => (_nullableInt(r) ?? 0) > 0).length;
  }

  @override
  Future<void> hashWrite(KvNamespace namespace, KvBytes key,
      {List<(KvBytes field, KvBytes value)> set = const [],
      List<KvBytes> removeFields = const []}) async {
    final s = await _sessionFor(namespace.id);
    // 改值与加字段是同一句 HSET(服务端自己区分新增/覆盖,回包只是计数),
    // 多字段一次发完;空 HSET 会被服务端拒,调用方不给「一条内容都没有」的入口。
    if (set.isNotEmpty) {
      await _exec(s, ['HSET', key, for (final (f, v) in set) ...[f, v]]);
    }
    if (removeFields.isNotEmpty) {
      await _exec(s, ['HDEL', key, ...removeFields]);
    }
  }

  @override
  Future<void> listWrite(KvNamespace namespace, KvBytes key,
      {List<(int index, KvBytes value)> set = const [],
      List<int> removeIndexes = const [],
      KvBytes? prepend,
      KvBytes? append}) async {
    final s = await _sessionFor(namespace.id);
    if (prepend != null) await _exec(s, ['LPUSH', key, prepend]);
    if (append != null) await _exec(s, ['RPUSH', key, append]);
    if (set.isNotEmpty) {
      // LSET 一次只改一格;下标由调用方按同一份快照给出,互不重复,连发才不位移
      await Future.wait<Object?>([
        for (final (i, v) in set) _exec(s, ['LSET', key, '$i', v]),
      ]);
    }
    if (removeIndexes.isEmpty) return;
    // LREM 按**值**删,列表有重复元素时直接用它删的不是用户点中的那一行。
    // 所以:先把每个目标下标 LSET 成一个随机哨兵,**全部替换完**再逐个 LREM ——
    // 中途删除会让后面目标左移,先替换就没这件事。哨兵撞车的概率可忽略,
    // 代价是这几步非原子:并发读者可能短暂看见一格哨兵值。
    final targets = <int, KvBytes>{
      for (final i in removeIndexes.toSet()) i: _sentinel(),
    };
    await Future.wait<Object?>([
      for (final e in targets.entries) _exec(s, ['LSET', key, '${e.key}', e.value]),
    ]);
    await Future.wait<Object?>([
      for (final v in targets.values) _exec(s, ['LREM', key, '1', v]),
    ]);
  }

  @override
  Future<void> setWrite(KvNamespace namespace, KvBytes key,
      {List<KvBytes> add = const [],
      List<KvBytes> removeMembers = const []}) async {
    final s = await _sessionFor(namespace.id);
    if (add.isNotEmpty) await _exec(s, ['SADD', key, ...add]);
    if (removeMembers.isNotEmpty) {
      await _exec(s, ['SREM', key, ...removeMembers]);
    }
  }

  @override
  Future<void> zsetWrite(KvNamespace namespace, KvBytes key,
      {List<(KvBytes member, double score)> add = const [],
      List<KvBytes> removeMembers = const []}) async {
    final s = await _sessionFor(namespace.id);
    if (add.isNotEmpty) {
      // 改分数与加成员同一句 ZADD。'${double}' 出的文本(含 1e+21)服务端都能解析;
      // NaN / Infinity 会被拒成「value is not a valid float」,原样上抛。
      await _exec(s, [
        'ZADD',
        key,
        for (final (m, score) in add) ...['$score', m],
      ]);
    }
    if (removeMembers.isNotEmpty) {
      await _exec(s, ['ZREM', key, ...removeMembers]);
    }
  }

  /// 删列表元素用的临时哨兵值(见 [listWrite])。16 随机字节:撞车概率可忽略,
  /// 而且几乎必然不是合法 UTF-8 —— 即便被并发读者瞥见也是 `HEX:…`,不会冒充正常文本。
  static KvBytes _sentinel() =>
      KvBytes([for (var i = 0; i < 16; i++) _random.nextInt(256)]);

  static final math.Random _random = math.Random.secure();

  @override
  Future<KvServerInfo?> readServerInfo() async {
    final s = await _sessionFor('0');
    final text = _bytes(await _exec(s, ['INFO'])).text;
    if (text == null) return null;
    final sections = <KvInfoSection>[];
    var current = <MapEntry<String, String>>[];
    var name = '# General';
    final metrics = <String, String>{};
    for (final line in text.split('\n')) {
      final t = line.trim();
      if (t.isEmpty || t.startsWith('- ')) continue;
      if (t.startsWith('#')) {
        if (current.isNotEmpty) sections.add(KvInfoSection(name: name, entries: current));
        name = t.substring(1).trim();
        current = [];
        continue;
      }
      final i = t.indexOf(':');
      if (i <= 0) continue;
      final k = t.substring(0, i);
      final v = t.substring(i + 1);
      current.add(MapEntry(k, v));
      // 只抽详情面板要用的几个,key 是稳定 ID,标签由 UI 翻译
      if (const {'redis_version', 'used_memory_human', 'connected_clients',
            'total_commands_processed', 'uptime_in_days', 'maxmemory_human',
            'role'}
          .contains(k)) {
        metrics[k] = v;
      }
    }
    if (current.isNotEmpty) sections.add(KvInfoSection(name: name, entries: current));
    return KvServerInfo(
      engineLabel: 'Redis',
      version: metrics['redis_version'] ?? '',
      sections: sections,
      metrics: metrics,
    );
  }

  // ---- 内部 ----

  /// 容器类型通用收尾:`rows` 由 [rowOf] 逐元素造,数量与截断标记统一算。
  KvValueRead _containerRead({
    required KvBytes key,
    required KvKeyType type,
    required int? count,
    required int? ttl,
    required Object? raw,
    required KvValueRow Function(int index, Object? element) rowOf,
  }) {
    final items = _list(raw);
    return KvValueRead(
      key: key,
      type: type,
      rows: [for (var i = 0; i < items.length; i++) rowOf(i, items[i])],
      totalCount: count,
      truncated: count != null && count > items.length,
      ttlSeconds: ttl,
    );
  }

  /// hash / set 的分页:元素数已知且不超过 [cap] 时整取(一次往返),
  /// 否则走 HSCAN/SSCAN 凑够一页 —— 游标不对外暴露,只保证「第一页够用」。
  /// 三个命令参数都是**前缀**,键名由这里统一追加。
  ///
  /// [rowsOf] 拿到的是服务端原样数组:**两者都是扁平的**,但配对规则不同 ——
  /// HGETALL/HSCAN 是 `[f, v, f, v]`(成对),SMEMBERS/SSCAN 是 `[m, m, m]`
  /// (每个元素就是一个成员)。混用会把集合的成员吃掉一半(实测)。
  Future<KvValueRead> _pagedContainer({
    required _Session s,
    required KvBytes key,
    required KvKeyType type,
    required List<Object> countCommand,
    required List<Object> wholeCommand,
    required List<Object> scanCommand,
    required List<KvValueRow> Function(List<Object?> items) rowsOf,
    required int cap,
    required int? ttl,
  }) async {
    final count = await _int(s, [...countCommand, key]);
    if (count != null && count <= cap) {
      return KvValueRead(
          key: key,
          type: type,
          rows: rowsOf(_list(await _exec(s, [...wholeCommand, key]))),
          totalCount: count,
          truncated: false,
          ttlSeconds: ttl);
    }
    var cursor = '0';
    final collected = <Object?>[];
    while (true) {
      final resp = _list(
          await _exec(s, [...scanCommand, key, cursor, 'COUNT', '$cap']));
      cursor = _bytes(resp[0]).text ?? '0';
      collected.addAll(_list(resp.length > 1 ? resp[1] : const []));
      if (cursor == '0' || collected.length >= cap * 2) break;
    }
    return KvValueRead(
      key: key,
      type: type,
      rows: rowsOf(collected.take(cap).toList()),
      totalCount: count,
      truncated: true,
      ttlSeconds: ttl,
    );
  }

  /// HGETALL / HSCAN 的扁平数组 → 字段/值行
  static List<KvValueRow> _hashRows(List<Object?> items) => [
        for (var i = 0; i + 1 < items.length; i += 2)
          KvValueRow(field: _bytes(items[i]), value: _bytes(items[i + 1])),
      ];

  /// SMEMBERS / SSCAN 的扁平数组 → 每元素一行(左列空)
  static List<KvValueRow> _setRows(List<Object?> items) =>
      [for (final m in items) KvValueRow(value: _bytes(m))];

  Future<_Session> _sessionFor(String dbIndex) async {
    final existing = _sessions[dbIndex];
    if (existing != null && existing.connected && !existing.dead) {
      return existing;
    }
    if (existing != null) {
      _sessions.remove(dbIndex);
      await existing.dispose();
    }
    final s = _Session(_conn, dbIndex);
    await s.open();
    _sessions[dbIndex] = s;
    return s;
  }

  /// 库数量:`CONFIG GET databases`。CONFIG 可能被 ACL 挡,退回
  /// INFO keyspace 里出现过的最大库号 + 1,再退回 1(单库)。
  Future<int> _databaseCount() async {
    try {
      final s = await _sessionFor('0');
      final flat = _list(await _exec(s, ['CONFIG', 'GET', 'databases']));
      if (flat.length >= 2) {
        final n = int.tryParse(_bytes(flat[1]).text ?? '');
        if (n != null && n > 0) return n;
      }
    } on KvServerException {
      // 权限不足:按下面两条兜底
    }
    final sizes = await _keyspaceSizes();
    var max = -1;
    for (final k in sizes.keys) {
      final i = int.tryParse(k) ?? -1;
      if (i > max) max = i;
    }
    return max < 0 ? 1 : max + 1;
  }

  /// `INFO keyspace` 的 `dbN:keys=…,expires=…` → {库号: 键数}
  Future<Map<String, int>> _keyspaceSizes() async {
    final s = await _sessionFor('0');
    final text = _bytes(await _exec(s, ['INFO', 'keyspace'])).text ?? '';
    final out = <String, int>{};
    for (final line in text.split('\n')) {
      final m = RegExp(r'^db(\d+):keys=(\d+)').firstMatch(line.trim());
      if (m != null) out[m.group(1)!] = int.parse(m.group(2)!);
    }
    return out;
  }

  /// 发一条命令,回原始解析结果。异常分类是这层的唯一职责。
  ///
  /// [args] 里的 [KvBytes] 先转成 `RedisBulk`:序列化器只认
  /// String / int / Iterable / RedisBulk / null,直接塞 KvBytes 会抛
  /// 「cant serialize such type」(实测),而二进制键名**必须**按裸字节发。
  Future<Object?> _exec(_Session s, List<Object> args) async {
    // 返回类型必须是可空的 Object?:包的 send_object 声明的是无类型 Future,
    // nil bulk 回包就是 null(实测 MEMORY USAGE 打在缺失键上、GET 打在不存在的键),
    // 收成 Object 会抛「type 'Null' is not a subtype of type 'Object'」。
    final Object? raw;
    try {
      raw = await s.command!
          .send_object([for (final a in args) _wireArg(a)]);
    } on redis.RedisError catch (e) {
      // 服务端错误:连接照常可用(实测 WRONGTYPE 后 PING 仍 PONG)
      throw KvServerException(e.error.trim());
    } on Object catch (e) {
      // 其余异常都说明这条连接的字节流不再对齐,必须作废
      s.dead = true;
      throw KvConnectionException('Redis 连接异常', cause: e);
    }
    if (raw is redis.RedisError) {
      throw KvServerException(raw.error.trim());
    }
    return raw;
  }

  static Object _wireArg(Object a) =>
      a is KvBytes ? redis.RedisBulk(a.bytes) : a;

  /// 元数据类批量取数用:单条失败不影响其它键(拿不到就是 null)。
  Future<Object?> _attempt(_Session s, List<Object> args) async {
    try {
      return await _exec(s, args);
    } on KvServerException {
      return null;
    } on KvConnectionException {
      rethrow;
    }
  }

  Future<int?> _int(_Session s, List<Object> args) async =>
      _nullableInt(await _exec(s, args));

  static String _str(Object? v) {
    if (v == null) return '';
    if (v is String) return v;
    if (v is List) return _bytes(v.cast<int>()).text ?? '';
    return '$v';
  }

  static String? _nullableStr(Object? v) {
    if (v == null) return null;
    final s = _str(v);
    return s.isEmpty ? null : s;
  }

  static int? _nullableInt(Object? v) {
    if (v == null) return null;
    if (v is int) return v;
    return int.tryParse(_str(v));
  }

  static List<Object?> _list(Object? v) =>
      v is List ? v.cast<Object?>() : const [];

  /// 任何回包都收成字节载体:bulk 是 List<int>,简单串是 String,int 是回包文本
  static KvBytes _bytes(Object? v) {
    if (v == null) return KvBytes(const []);
    if (v is List) return KvBytes(v.cast<int>());
    if (v is String) return KvBytes(utf8.encode(v));
    if (v is int) return KvBytes(utf8.encode('$v'));
    return KvBytes(utf8.encode('$v'));
  }

  /// 原始回包 → [KvReply] 结构树。二进制解析器下 bulk 一律 List<int>。
  KvReply _replyOf(Object? v) {
    if (v is redis.RedisError) return KvReply.error(v.error.trim());
    if (v == null) return KvReply.nil();
    if (v is int) return KvReply.integer(v);
    if (v is String) return KvReply.simple(v);
    if (v is List) {
      // 嵌套数组里混着 bulk(List<int>)与整数,靠元素运行时类型分流
      if (v.isNotEmpty && v.every((e) => e is int)) {
        return KvReply.bulk(KvBytes(v.cast<int>()));
      }
      return KvReply.array([for (final e in v) _replyOf(e)]);
    }
    return KvReply.simple('$v');
  }
}

/// 一个库一条连接:持有已切到 [dbIndex] 的 `Command`。
class _Session {
  _Session(this._conn, this.dbIndex);

  final ConnectionInfo _conn;
  final String dbIndex;

  redis.RedisConnection? _raw;
  redis.Command? command;

  /// 连接已不可信(解码 / IO 异常),下一次取会话时重建
  bool dead = false;

  bool get connected => command != null && !dead;

  Future<void> open() async {
    final raw = redis.RedisConnection();
    late final redis.Command cmd;
    try {
      cmd = await raw
          .connect(_conn.host, int.tryParse(_conn.port) ?? 6379)
          .timeout(RedisKvDriver._connectTimeout);
    } on Object catch (e) {
      await _quietDispose(raw);
      throw KvConnectionException('无法连接 Redis ${_conn.host}:${_conn.port}',
          cause: e);
    }
    // 二进制解析器:见类注释第 2 点
    cmd.setParser(redis.RedisParserBulkBinary());
    command = cmd;
    _raw = raw;
    try {
      final pwd = _conn.password;
      if (pwd.isNotEmpty) {
        // 有用户名走 ACL 双参数,没有走 requirepass 单参数(两种实测都回 +OK)
        final args = _conn.username.isEmpty
            ? <Object>['AUTH', pwd]
            : <Object>['AUTH', _conn.username, pwd];
        _checkOk(await cmd.send_object(args));
      }
      if (dbIndex.isNotEmpty && dbIndex != '0') {
        _checkOk(await cmd.send_object(['SELECT', dbIndex]));
      }
    } on redis.RedisError catch (e) {
      await dispose();
      throw KvServerException(e.error.trim());
    } on Object catch (e) {
      await dispose();
      throw KvConnectionException('Redis 认证或切库失败', cause: e);
    }
  }

  static void _checkOk(Object? v) {
    if (v is redis.RedisError) throw KvServerException(v.error.trim());
  }

  Future<void> dispose() async {
    final raw = _raw;
    command = null;
    _raw = null;
    dead = true;
    if (raw != null) await _quietDispose(raw);
  }

  static Future<void> _quietDispose(redis.RedisConnection raw) async {
    try {
      await raw.close();
    } on Object {
      // 断开本来就是目标;包里 close() 还会因「sink 已关」抛 StateError
    }
  }
}
