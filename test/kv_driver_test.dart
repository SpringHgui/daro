import 'package:flutter_test/flutter_test.dart';

import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/data/drivers/kv_driver.dart';
import 'package:daro/data/drivers/redis_kv_driver.dart';

/// KV 抽象层的离线回归:纯函数与门控,不需要真 Redis。
///
/// 真连行为在 `redis_kv_live_test.dart`(`DARO_LIVE_DB=1`);这里只守住
/// 「界面可以直接依赖」的那些口径:字节/文本判据、类型映射、回包渲染、
/// 以及 SQL 与 KV 两套注册表互不相交。
void main() {
  group('KvBytes:字节是一等公民', () {
    test('UTF-8 可解就是文本,不可解才是二进制', () {
      final txt = KvBytes([0xe4, 0xb8, 0xad, 0xe6, 0x96, 0x87]); // 中文
      expect(txt.text, '中文');
      expect(txt.isBinary, isFalse);
      expect(txt.display, '中文');

      final bin = KvBytes([0x00, 0xff, 0x80, 0xc3, 0x28, 0x1a, 0x04]);
      expect(bin.text, isNull, reason: '解不动必须给 null,不能给空串');
      expect(bin.isBinary, isTrue);
      expect(bin.hex, '00FF80C3281A04');
      expect(bin.display, 'HEX:00FF80C3281A04');
    });

    test('空值是「空文本」而不是二进制', () {
      // HSET 一个空字段值实测就是 0 字节:isBinary 必须为 false,
      // 否则网格里每行空值都会显示成 HEX:
      final empty = KvBytes(const []);
      expect(empty.text, '');
      expect(empty.isBinary, isFalse);
      expect(empty.display, '');
      expect(empty.byteLength, 0);
    });

    test('相等与哈希按字节比,不看同一个 List 的引用', () {
      expect(KvBytes.text('a:1'), KvBytes([0x61, 0x3a, 0x31]));
      expect(KvBytes.text('a:1').hashCode, KvBytes([0x61, 0x3a, 0x31]).hashCode);
      expect(KvBytes.text('a:1') == KvBytes.text('a:2'), isFalse);
      // 键名可以是裸字节:两个来源不同、字节相同的键必须能当同一个 map key
      final m = <KvBytes, int>{KvBytes([0xff, 0xfe]): 1};
      expect(m[KvBytes([0xff, 0xfe])], 1);
    });

    test('toString 截断,日志不会因一个大值刷屏', () {
      expect(KvBytes.text('x' * 100).toString().length, lessThan(60));
    });
  });

  group('类型与注册表', () {
    test('kvKeyTypeOf 覆盖服务端可能回的每一个名字', () {
      expect(kvKeyTypeOf('string'), KvKeyType.string);
      expect(kvKeyTypeOf('list'), KvKeyType.list);
      expect(kvKeyTypeOf('set'), KvKeyType.set);
      expect(kvKeyTypeOf('zset'), KvKeyType.zset);
      expect(kvKeyTypeOf('hash'), KvKeyType.hash);
      expect(kvKeyTypeOf('stream'), KvKeyType.stream);
      expect(kvKeyTypeOf('none'), KvKeyType.none);
      // 认不得的不能塌成 none(那是「键不存在」),要留 unknown 让界面如实显示
      expect(kvKeyTypeOf('module'), KvKeyType.unknown);
      expect(kvKeyTypeOf(''), KvKeyType.unknown);
    });

    test('SQL 与 KV 两套注册表互不相交', () {
      // hasDriver/createDriver 会把认不出的 typeId 当 SQL 引擎用;
      // KV 的 id 一旦混进 kSupportedDriverTypes,树展开就拿到 null 驱动。
      expect(kKvStoreTypes.intersection(kSupportedDriverTypes), isEmpty);
      for (final id in kKvStoreTypes) {
        expect(createDriver(_conn(id)), isNull,
            reason: '$id 不能由 SQL 驱动工厂造');
        expect(createKvDriver(_conn(id)), isNotNull);
      }
    });

    test('isKvStoreConnection 同时认 typeId 与 isLive', () {
      expect(isKvStoreConnection(_conn('redis')), isTrue);
      expect(isKvStoreConnection(_conn('redis', isLive: false)), isFalse);
      expect(isKvStoreConnection(_conn('mysql')), isFalse);
      // 未实现的缓存引擎(如 Memcached)不能被误判成已就绪
      expect(kKvStoreTypes.contains('memcached'), isFalse);
      expect(isKvStoreConnection(_conn('memcached')), isFalse);
    });

    test('Redis 能力表:首版只读,但枚举/编码/偏移读都开着', () {
      final cap = RedisKvDriver(_conn('redis')).capabilities;
      expect(cap.readOnlyFirstVersion, isTrue);
      expect(cap.commandConsole, isTrue);
      expect(cap.multipleNamespaces, isTrue);
      expect(cap.cursorScan, isTrue);
      expect(cap.valueOffsetRead, isTrue);
      expect(cap.supports(KvKeyType.stream), isTrue);
      expect(cap.supports(KvKeyType.unknown), isFalse);
      expect(cap.maxValuePreviewBytes, greaterThan(0));
    });
  });

  group('KvReply:控制台回包的结构与渲染', () {
    test('每种 RESP 类型都有自解释的 summary', () {
      expect(KvReply.nil().summary, '(nil)');
      expect(KvReply.integer(2025).summary, '2025');
      expect(KvReply.simple('PONG').summary, 'PONG');
      expect(KvReply.error('WRONGTYPE x').summary, 'ERR WRONGTYPE x');
      expect(KvReply.bulk(KvBytes.text('1.0.0')).summary, '1.0.0');
      expect(KvReply.bulk(KvBytes([0xff])).summary, 'HEX:FF');
      expect(KvReply.array([KvReply.integer(1), KvReply.nil()]).summary,
          '[1, (nil)]');
      expect(KvReply.error('x').isError, isTrue);
      expect(KvReply.simple('x').isError, isFalse);
    });

    test('bulk 里的换行原样分行(INFO 一类多行回包)', () {
      final info = KvReply.bulk(KvBytes.text('# Server\nredis_version:8.10.2\n'));
      expect(info.render(), ['# Server', 'redis_version:8.10.2', '']);
    });

    test('数组按层缩进,二进制项转 HEX,空数组不消失', () {
      final nested = KvReply.array([
        KvReply.array([KvReply.simple('a'), KvReply.bulk(KvBytes([0xff]))]),
        KvReply.array(const []),
      ]);
      expect(nested.render(), ['    a', '    HEX:FF', '  (空数组)']);
    });

    test('层级过深只截断显示,不递归爆栈', () {
      var reply = KvReply.simple('bottom');
      for (var i = 0; i < 40; i++) {
        reply = KvReply.array([reply]);
      }
      expect(reply.render().last, contains('层级过深'));
    });
  });
}

ConnectionInfo _conn(String typeId, {bool isLive = true}) => ConnectionInfo(
      name: 'probe-$typeId',
      typeId: typeId,
      host: '127.0.0.1',
      port: '6379',
      username: '',
      isLive: isLive,
    );
