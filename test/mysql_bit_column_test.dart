import 'package:flutter_test/flutter_test.dart';

import 'package:daro/data/drivers/mysql_driver.dart';

/// 钉住 bit 列的展示修复:mysql_client 把文本协议的字段一律按 UTF-8 解码,
/// `bit(1)` 的原始字节 0x00/0x01 因此变成不可打印字符,表格里是豆腐块。
void main() {
  group('mysqlBitAsInt 把裸字节折成十进制', () {
    test('bit(1) 的 0 / 1', () {
      expect(mysqlBitAsInt('\u0000'), '0');
      expect(mysqlBitAsInt('\u0001'), '1');
    });

    test('多字节按大端折回,与 CAST(col AS UNSIGNED) 同值', () {
      // bit(9) 存 256 → 两字节 01 00
      expect(mysqlBitAsInt('\u0001\u0000'), '256');
      // bit(7) 存 127 → 单字节 7f
      expect(mysqlBitAsInt('\u007f'), '127');
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
      expect(mysqlCellText('\u0001', false), '\u0001');
      expect(mysqlCellText('abc', false), 'abc');
    });
  });

  group('mysqlBitProjectionOf 生成把 BIT 换成整数的投影', () {
    test('无 bit 列返回 null,调用方保持 SELECT *', () {
      expect(
          mysqlBitProjectionOf([
            (name: 'id', type: 'bigint'),
            (name: 'note', type: 'varchar'),
          ]),
          isNull);
    });

    test('只包 bit 列,顺序与列名不变', () {
      expect(
          mysqlBitProjectionOf([
            (name: 'id', type: 'bigint'),
            (name: 'status', type: 'bit'),
          ]),
          '`id`, CAST(`status` AS UNSIGNED) AS `status`');
    });

    test('反引号转义,列名里的反引号不炸语句', () {
      expect(
          mysqlBitProjectionOf([(name: 'a`b', type: 'bit')]),
          'CAST(`a``b` AS UNSIGNED) AS `a``b`');
    });
  });
}
