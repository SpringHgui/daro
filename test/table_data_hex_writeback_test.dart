import 'package:flutter_test/flutter_test.dart';

import 'package:daro/widgets/table_data_page.dart';

/// 钉住「驱动把二进制列换成十六进制取回后,回写必须还原成二进制字面量」。
///
/// 表浏览打开含 BLOB 的表时,驱动会让服务端改发 `HEX(col)`
/// (见 mysql_driver.dart 的 `mysqlSafeProjection`)。此时格子里的文本是转储,
/// 按文本回写会把十六进制字符本身存进列里(5 字节变 10 字节),
/// 而 WHERE 里的文本比较根本匹配不到行——表现成「回『保存成功』,库里一条没变」。
void main() {
  test('NULL 仍是 SQL NULL 关键字', () {
    expect(sqlValueLiteral('NULL'), 'NULL');
    expect(sqlValueLiteral('NULL', hex: true), 'NULL');
  });

  test('普通列按字符串字面量,内部单引号翻倍', () {
    expect(sqlValueLiteral("o'k"), "'o''k'");
    expect(sqlValueLiteral(''), "''");
  });

  test('十六进制列还原成 X\'…\' 二进制字面量', () {
    expect(sqlValueLiteral('308204A2', hex: true), "X'308204A2'");
    // 零长二进制列的转储就是空串
    expect(sqlValueLiteral('', hex: true), "X''");
  });

  test('用户把转储改成非十六进制时按文本回写', () {
    expect(sqlValueLiteral('hello', hex: true), "'hello'");
    expect(sqlValueLiteral('abc', hex: true), "'abc'");
  });

  test('未标记为十六进制的列不受影响(含长得像 hex 的文本)', () {
    // 驱动没换投影时,格子里就是原始文本,不能自作主张转成二进制
    expect(sqlValueLiteral('1e46'), "'1e46'");
  });
}
