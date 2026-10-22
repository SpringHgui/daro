import 'package:flutter_test/flutter_test.dart';

import 'package:daro/data/drivers/db_driver.dart';
import 'package:daro/data/drivers/mysql_driver.dart';

/// 钉住「裸字节列」的读取与回写口径。
///
/// mysql_client 对文本协议的**每个**字段一律按 UTF-8 解码
/// (`packet/packet_result_set_row.dart`),而服务端把 `bit` / `blob` / 几何列
/// 按裸字节发:能解出来的渲染成豆腐块,解不出来的直接抛
/// `FormatException: Unexpected extension byte`,整张表打不开
/// (实测:含 `cert_file blob` 的 `bx_wxcoupon_mch`)。
///
/// 下面的字节串一律用 `String.fromCharCode` 构造(等价于裸码元,即
/// mysql_client 解码后的形态),避免测试源文件里出现控制字符。
void main() {
  group('mysqlBitAsInt 把裸字节折成十进制', () {
    test('bit(1) 的 0 / 1', () {
      expect(mysqlBitAsInt(String.fromCharCode(0)), '0');
      expect(mysqlBitAsInt(String.fromCharCode(1)), '1');
    });

    test('多字节按大端折回,与 CAST(col AS UNSIGNED) 同值', () {
      // bit(9) 存 256 → 两字节 01 00
      expect(mysqlBitAsInt(String.fromCharCodes([1, 0])), '256');
      // bit(7) 存 127 → 单字节 7f
      expect(mysqlBitAsInt(String.fromCharCode(0x7f)), '127');
    });

    test('空值仍是空串(不折成 0)', () {
      expect(mysqlBitAsInt(''), '');
    });
  });

  group('mysqlCellText 逐列分派', () {
    test('真 NULL 不能被折成数字', () {
      // 'NULL' 的四个码元按大端折是 1296247884,漏判就会显示成这个数
      expect(mysqlCellText(null, true), 'NULL');
      expect(mysqlCellText(null, false), 'NULL');
    });

    test('非 bit 列原样返回', () {
      expect(mysqlCellText(String.fromCharCode(1), false),
          String.fromCharCode(1));
      expect(mysqlCellText('abc', false), 'abc');
    });
  });

  group('mysqlSafeProjectionOf 生成把裸字节换成可打印文本的投影', () {
    test('无 bit / 二进制列返回 null,调用方保持 SELECT *', () {
      expect(
          mysqlSafeProjectionOf([
            (name: 'id', type: 'bigint'),
            (name: 'note', type: 'varchar'),
          ]),
          isNull);
    });

    test('BIT 走 CAST、BLOB 族走 HEX,列名与顺序不变', () {
      final plan = mysqlSafeProjectionOf([
        (name: 'id', type: 'bigint'),
        (name: 'status', type: 'bit'),
        (name: 'cert_file', type: 'blob'),
      ]);
      expect(
          plan?.select,
          '`id`, CAST(`status` AS UNSIGNED) AS `status`, '
          'HEX(`cert_file`) AS `cert_file`');
      // 只有 HEX 化的列要回报:bit 已被折成十进制,回写仍是普通文本
      expect(plan?.hexColumns, {'cert_file'});
    });

    test('回归:只有 BLOB 列的表(bx_wxcoupon_mch)也必须有投影', () {
      // 旧实现只认 bit,这张证书表取不到投影 → 原样抛 FormatException,表打不开
      final plan = mysqlSafeProjectionOf([
        (name: 'id', type: 'bigint'),
        (name: 'sender_mch_id', type: 'varchar'),
        (name: 'status', type: 'tinyint'),
        (name: 'syscreatetime', type: 'datetime'),
        (name: 'cert_file', type: 'blob'),
      ]);
      expect(
          plan?.select,
          '`id`, `sender_mch_id`, `status`, `syscreatetime`, '
          'HEX(`cert_file`) AS `cert_file`');
    });

    test('二进制族全覆盖:binary / varbinary / 三种 BLOB / 几何', () {
      for (final t in [
        'binary',
        'varbinary',
        'tinyblob',
        'blob',
        'mediumblob',
        'longblob',
        'geometry',
        'point',
        'linestring',
        'polygon',
        'multipoint',
        'multilinestring',
        'multipolygon',
        'geometrycollection',
      ]) {
        final plan = mysqlSafeProjectionOf([(name: 'c', type: t)]);
        expect(plan?.select, 'HEX(`c`) AS `c`', reason: t);
        expect(plan?.hexColumns, {'c'}, reason: t);
      }
    });

    test('DATA_TYPE 大小写不敏感', () {
      expect(
          mysqlSafeProjectionOf([(name: 'c', type: 'BLOB')])?.select,
          'HEX(`c`) AS `c`');
      expect(
          mysqlSafeProjectionOf([(name: 'c', type: 'BIT')])?.select,
          'CAST(`c` AS UNSIGNED) AS `c`');
    });

    test('反引号转义,列名里的反引号不炸语句', () {
      expect(
          mysqlSafeProjectionOf([(name: 'a`b', type: 'blob')])?.select,
          'HEX(`a``b`) AS `a``b`');
    });
  });

  group('binaryLiteralFromHex 把十六进制转储还原成二进制字面量', () {
    test('纯十六进制 → X\'…\'', () {
      expect(TablePreview.binaryLiteralFromHex('308204A2'), "X'308204A2'");
      expect(TablePreview.binaryLiteralFromHex('abcdef'), "X'abcdef'");
    });

    test('空串是合法的零长二进制值', () {
      expect(TablePreview.binaryLiteralFromHex(''), "X''");
    });

    test('非十六进制 / 奇数长度返回 null(调用方按文本回写)', () {
      expect(TablePreview.binaryLiteralFromHex('hello'), isNull);
      expect(TablePreview.binaryLiteralFromHex('12g4'), isNull);
      expect(TablePreview.binaryLiteralFromHex('abc'), isNull);
    });
  });

  group('kMysqlBinaryDataTypes 与列类型标记的口径', () {
    test('TEXT 族不在其中(它们是字符集转码后的文本,不会解码失败)', () {
      for (final t in ['text', 'tinytext', 'mediumtext', 'longtext', 'json']) {
        expect(kMysqlBinaryDataTypes.contains(t), isFalse, reason: t);
      }
    });
  });
}
