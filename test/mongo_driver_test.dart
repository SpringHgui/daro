import 'package:flutter_test/flutter_test.dart';
import 'package:mongo_dart/mongo_dart.dart';

import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/mongo_driver.dart';

/// 钉住 MongoDB 驱动里**不碰网络**的三件事:单元格文本化、排序字符串解析、
/// 文档 → 表格的列推导。
///
/// 驱动其余部分要连库才有意义,由 test/mongo_driver_live_test.dart 实连覆盖;
/// 这里只收「解析错了就把数据显示错 / 把整张表弄打不开」的纯函数。
void main() {
  MongoDriver driver() => MongoDriver(const ConnectionInfo(
        name: '离线',
        typeId: 'mongodb',
        host: '127.0.0.1',
        port: '27017',
        username: 'admin',
        password: 'x',
      ));

  group('cell:BSON 值 → 单元格文本', () {
    test('null 打成 NULL(与 MySQL 驱动同一口径,空值语义交给 nullMask)', () {
      expect(cell(null), 'NULL');
    });

    test('ObjectId 取 24 位十六进制本体,不带 ObjectId(...) 的类名壳', () {
      expect(
          cell(ObjectId.fromHexString('507f1f77bcf86cd799439011')),
          '507f1f77bcf86cd799439011');
    });

    test('二进制:长度 + 十六进制转储(小写),与 MySQL 的 HEX 列同一读法', () {
      expect(cell(BsonBinary.from([0, 16, 255])), '3B 0010ff');
    });

    test('超长二进制截到 64 个十六进制字符并加省略号', () {
      final long = cell(BsonBinary.from(List.filled(64, 0xAB)));
      expect(long, startsWith('64B ${'ab' * 32}'));
      expect(long, endsWith('…'));
      expect(long.length, lessThan(80));
    });

    test('DateTime 一律 ISO8601 + UTC(实测驱动解码即 UTC)', () {
      expect(cell(DateTime.utc(2026, 10, 23, 8, 30)),
          '2026-10-23T08:30:00.000Z');
      // 本地时区的时间也按 UTC 出,不然同一条文档在两台机器上显示不同
      expect(cell(DateTime.utc(2026, 1, 2, 3, 4, 5).toLocal()),
          contains('T03:04:05'));
    });

    test('嵌套文档 / 数组 → 紧凑 JSON(ObjectId 由 bson 的 toJson 折成十六进制)', () {
      expect(cell({'a': 1, 'b': 'x'}), '{"a":1,"b":"x"}');
      expect(cell([1, 'two', null]), '[1,"two",null]');
      expect(
          cell({'_id': ObjectId.fromHexString('507f1f77bcf86cd799439011')}),
          '{"_id":"507f1f77bcf86cd799439011"}');
    });

    test('jsonEncode 处理不了的 BSON 叶子退化成 toString:不能炸掉整张表', () {
      // BsonBinary 没有 toJson,jsonEncode 会抛 —— 必须走 fallback 分支
      final text = cell({'blob': BsonBinary.from([1, 2])});
      expect(text, isNotEmpty);
      expect(text, isNot(contains('Exception')));
    });

    test('标量按原样出(bool / int / double / String)', () {
      expect(cell(true), 'true');
      expect(cell(42), '42');
      expect(cell(1.5), '1.5');
      expect(cell('hi'), 'hi');
    });
  });

  group('mongoBsonTypeName:describeTable 的列类型标注', () {
    test('常见 BSON 类型', () {
      expect(mongoBsonTypeName(ObjectId.fromHexString('507f1f77bcf86cd799439011')), 'objectId');
      expect(mongoBsonTypeName(BsonBinary.from([1])), 'binary');
      expect(mongoBsonTypeName(DateTime.utc(2026)), 'date');
      expect(mongoBsonTypeName(true), 'bool');
      expect(mongoBsonTypeName(1), 'int');
      expect(mongoBsonTypeName(1.0), 'double');
      expect(mongoBsonTypeName('x'), 'string');
      expect(mongoBsonTypeName({'a': 1}), 'object');
      expect(mongoBsonTypeName([1]), 'array');
    });
  });

  group('mongoSortFromOrderBy:只认单列 ORDER BY', () {
    test('空 / null 不排序', () {
      expect(mongoSortFromOrderBy(null), isNull);
      expect(mongoSortFromOrderBy('   '), isNull);
    });

    test('裸标识符 = 升序;ASC / DESC 大小写都收', () {
      expect(mongoSortFromOrderBy('name'), {'name': 1});
      expect(mongoSortFromOrderBy('name ASC'), {'name': 1});
      expect(mongoSortFromOrderBy('name desc'), {'name': -1});
      expect(mongoSortFromOrderBy('  name   Desc  '), {'name': -1});
    });

    test('各方言引用符都剥掉:反引号 / 方括号 / 双引号', () {
      expect(mongoSortFromOrderBy('`name`'), {'name': 1});
      expect(mongoSortFromOrderBy('[name] DESC'), {'name': -1});
      expect(mongoSortFromOrderBy('"user name"'), {'user name': 1});
    });

    test('点号子文档键与中文列名都是合法集合字段', () {
      expect(mongoSortFromOrderBy('address.city'), {'address.city': 1});
      expect(mongoSortFromOrderBy('姓名 desc'), {'姓名': -1});
    });

    test('复合 / 函数 / 表达式排序一律拒绝(认错方向就是把数据排错)', () {
      expect(() => mongoSortFromOrderBy('a, b'), throwsUnsupportedError);
      expect(() => mongoSortFromOrderBy('upper(name)'), throwsUnsupportedError);
      expect(() => mongoSortFromOrderBy('a ASC, b DESC'),
          throwsUnsupportedError);
      expect(() => mongoSortFromOrderBy('a = 1'), throwsUnsupportedError);
    });
  });

  group('docsToPreview:列 = 键并集,缺键与显式 null 都进 nullMask', () {
    test('列按首见顺序取并集,后出现的键排在后面', () {
      final p = driver().docsToPreview([
        {'b': 1, 'a': 2},
        {'c': 3, 'a': 4},
      ]);
      expect(p.columns, ['b', 'a', 'c']);
    });

    test('缺键补空串,掩码记 true;显式 null 同样记 true', () {
      final p = driver().docsToPreview([
        {'a': 1, 'b': null},
        {'a': 2},
      ]);
      expect(p.columns, ['a', 'b']);
      expect(p.rows, [
        ['1', 'NULL'],
        ['2', 'NULL'],
      ]);
      expect(p.nullMask, [
        [false, true],
        [false, true],
      ]);
    });

    test('空集合给空表而不是崩', () {
      final p = driver().docsToPreview([]);
      expect(p.columns, isEmpty);
      expect(p.rows, isEmpty);
      expect(p.nullMask, isEmpty);
    });

    test('limit 透传(界面用它判断「还有更多」)', () {
      expect(driver().docsToPreview([{'a': 1}], limit: 50).limit, 50);
    });
  });

  group('TablePreview 只读链路:驱动不填 hexColumns', () {
    test('MongoDB 没有十六进制列(二进制在 cell() 里就地文本化)', () {
      final p = driver().docsToPreview([
        {'b': BsonBinary.from([1, 2])},
      ]);
      expect(p.hexColumns, isNull);
      expect(p.rows.single.single, '2B 0102');
    });
  });
}
