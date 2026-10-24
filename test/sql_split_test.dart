import 'package:daro/data/sql_split.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('splitSqlStatements', () {
    test('空 / 纯空白脚本返回空列表', () {
      expect(splitSqlStatements(''), isEmpty);
      expect(splitSqlStatements('   \n\t  '), isEmpty);
      expect(splitSqlStatements(';;;'), isEmpty);
    });

    test('单条语句无分号', () {
      expect(splitSqlStatements('SELECT 1'), ['SELECT 1']);
    });

    test('多条语句按顶层分号拆分并 trim', () {
      expect(
        splitSqlStatements('SELECT 1; SELECT 2 ;\nSELECT 3'),
        ['SELECT 1', 'SELECT 2', 'SELECT 3'],
      );
    });

    test('单引号字符串内的分号不拆分', () {
      expect(
        splitSqlStatements("SELECT 'a;b' FROM t; SELECT 2"),
        ["SELECT 'a;b' FROM t", 'SELECT 2'],
      );
    });

    test('单引号双写转义不提前结束字符串', () {
      expect(
        splitSqlStatements("SELECT 'it''s; ok'; SELECT 2"),
        ["SELECT 'it''s; ok'", 'SELECT 2'],
      );
    });

    test('双引号标识符内的分号不拆分', () {
      expect(
        splitSqlStatements('SELECT "col;name" FROM t; SELECT 2'),
        ['SELECT "col;name" FROM t', 'SELECT 2'],
      );
    });

    test('行注释内的分号不拆分', () {
      expect(
        splitSqlStatements('SELECT 1 -- a;b comment\n; SELECT 2'),
        ['SELECT 1 -- a;b comment', 'SELECT 2'],
      );
    });

    test('块注释内的分号不拆分', () {
      expect(
        splitSqlStatements('SELECT /* ; */ 1; SELECT 2'),
        ['SELECT /* ; */ 1', 'SELECT 2'],
      );
    });

    test('嵌套块注释内的分号不拆分(PostgreSQL)', () {
      expect(
        splitSqlStatements('SELECT /* outer /* inner; */ still; */ 1; SELECT 2'),
        ['SELECT /* outer /* inner; */ still; */ 1', 'SELECT 2'],
      );
    });

    test('美元引号函数体内的分号不拆分(PostgreSQL)', () {
      const sql = 'CREATE FUNCTION f() RETURNS int AS \$\$\n'
          'BEGIN\n'
          '  RETURN 1;\n'
          'END\n'
          '\$\$ LANGUAGE plpgsql;\n'
          'SELECT f();';
      final parts = splitSqlStatements(sql);
      expect(parts.length, 2);
      expect(parts[0], contains('RETURN 1;'));
      expect(parts[0], contains('\$\$ LANGUAGE plpgsql'));
      expect(parts[1], 'SELECT f()');
    });

    test('带标签的美元引号 \$tag\$ 同样识别', () {
      const sql = 'CREATE FUNCTION g() RETURNS int AS \$body\$\n'
          'BEGIN RETURN 2; END\n'
          '\$body\$ LANGUAGE plpgsql; SELECT g();';
      final parts = splitSqlStatements(sql);
      expect(parts.length, 2);
      expect(parts[0], contains('RETURN 2;'));
      expect(parts[1], 'SELECT g()');
    });

    test('\$1 位置参数不被误判为美元引号', () {
      expect(
        splitSqlStatements('SELECT \$1; SELECT 2'),
        ['SELECT \$1', 'SELECT 2'],
      );
    });

    test('末尾多余分号不产生空语句', () {
      expect(splitSqlStatements('SELECT 1;;;'), ['SELECT 1']);
    });

    test('SQL Server 方括号标识符内的分号不拆分', () {
      expect(
        splitSqlStatements('SELECT [col;name] FROM t; SELECT 2'),
        ['SELECT [col;name] FROM t', 'SELECT 2'],
      );
    });

    test('MySQL 反引号标识符内的分号不拆分', () {
      expect(
        splitSqlStatements('SELECT `col;name` FROM t; SELECT 2'),
        ['SELECT `col;name` FROM t', 'SELECT 2'],
      );
    });
  });

  group('SqlStatementSplitter 增量喂入', () {
    /// 语料:各类引号 / 注释 / 美元引号 / 指令行都混在一起
    const corpus = [
      'SELECT 1; SELECT 2;',
      "INSERT INTO t VALUES ('a;b', \"c;d\"), ('e');",
      'SELECT /* a ; b */ 1; -- c ; d\nSELECT 2;',
      'CREATE FUNCTION f() RETURNS int AS \$\$ BEGIN RETURN 1; END \$\$;'
          ' SELECT f();',
      'DELIMITER \$\$\nCREATE PROCEDURE p() BEGIN SELECT 1; END\$\$\n'
          'DELIMITER ;\nSELECT 2;',
      'CREATE PROCEDURE q() BEGIN SELECT 1; END;\nGO\nSELECT 2;\nGO',
      'SELECT [x;y], `a;b`; SELECT 3',
      '没有结束符的最后一条语句',
    ];

    test('任意块长喂入的结果与整段切分一致', () {
      for (final script in corpus) {
        for (final size in [1, 2, 3, 7, 64]) {
          expect(_feedIn(script, size), splitSqlStatements(script),
              reason: '块长 $size 时结果应一致:$script');
        }
      }
    });

    test('跨块的未闭合字符串与注释能续接', () {
      const script = "SELECT 'ab\ncd;ef' /* x; */ , 1; SELECT 2";
      expect(_feedIn(script, 5), splitSqlStatements(script));
    });

    test('DELIMITER 指令行本身不作为语句输出', () {
      final parts = splitSqlStatements(
          'DELIMITER //\nCREATE TRIGGER t BEFORE INSERT ON a FOR EACH ROW'
          ' BEGIN SELECT 1; END//\nDELIMITER ;\nSELECT 2;');
      expect(parts.length, 2);
      expect(parts[0], contains('SELECT 1;'));
      expect(parts[0], startsWith('CREATE TRIGGER'));
      expect(parts[1], 'SELECT 2');
    });

    test('DELIMITER 指令被数据块边界切断仍然生效', () {
      const script = 'DELIMITER \$\$\nSELECT 1; END\$\$\nSELECT 2;';
      final splitter = SqlStatementSplitter();
      final out = <String>[];
      for (var i = 0; i < script.length; i += 4) {
        final end =
            (i + 4) > script.length ? script.length : i + 4;
        out.addAll(splitter.feed(script.substring(i, end)));
      }
      out.addAll(splitter.finish());
      expect(out, ['SELECT 1; END', 'SELECT 2;']);
      expect(splitter.delimiter, '\$\$');
    });

    test('自定义结束符 \$\$ 不被误判为 PostgreSQL 美元引号', () {
      // MySQL 转储用 DELIMITER \$\$ 包裹过程体:同一段文本若按 PG 词法处理,
      // 过程体的结束符会被当成字符串起始而永不闭合
      final parts = splitSqlStatements(
          'DELIMITER \$\$\nCREATE PROCEDURE p() BEGIN SELECT 1; END\$\$\n'
          'DELIMITER ;\nSELECT 2;');
      expect(parts.first, endsWith('END'));
      expect(parts.length, 2);
    });

    test('GO 结束当前批但不作为语句输出', () {
      expect(
        splitSqlStatements('SELECT 1;\nGO\nGO 2\nSELECT 2'),
        ['SELECT 1', 'SELECT 2'],
      );
    });

    test('GO 出现在标识符或字符串中不作为批分隔', () {
      expect(
        splitSqlStatements("SELECT 'GO', gog; SELECT 2"),
        ["SELECT 'GO', gog", 'SELECT 2'],
      );
    });
  });

  group('sqlStatementAt', () {
    test('空 / 纯空白脚本返回 null', () {
      expect(sqlStatementAt('', 0), isNull);
      expect(sqlStatementAt('   \n\t', 4), isNull);
    });

    test('单条语句:光标在任意位置都命中它', () {
      const sql = 'SELECT 1';
      expect(sqlStatementAt(sql, 0), 'SELECT 1');
      expect(sqlStatementAt(sql, 4), 'SELECT 1');
      expect(sqlStatementAt(sql, sql.length), 'SELECT 1');
    });

    test('多条语句按光标所在的那条返回', () {
      const sql = 'SELECT 1; SELECT 2';
      final second = sql.indexOf('SELECT 2');
      expect(sqlStatementAt(sql, 3), 'SELECT 1');
      expect(sqlStatementAt(sql, 8), 'SELECT 1'); // 结束符位置仍归前一条
      expect(sqlStatementAt(sql, 9), 'SELECT 2'); // 两条之间的空白归后一条
      expect(sqlStatementAt(sql, second + 4), 'SELECT 2');
    });

    test('光标越过末尾仍返回最后一条', () {
      const sql = 'SELECT 1;\nSELECT 2;\n';
      expect(sqlStatementAt(sql, sql.length + 5), 'SELECT 2');
    });

    test('归属与 splitSqlStatements 的分组逐条一致(注释跟着下一条)', () {
      const sql = "SELECT 'a;b' FROM t; -- x;y\nSELECT 2";
      expect(
        sqlStatementAt(sql, sql.indexOf("'a;b'") + 4),
        "SELECT 'a;b' FROM t",
      );
      // 行注释与其后的语句之间没有分号,切分器把它们算同一条
      expect(
        sqlStatementAt(sql, sql.indexOf('-- x;y') + 3),
        '-- x;y\nSELECT 2',
      );
      expect(sqlStatementAt(sql, sql.indexOf('SELECT 2')), '-- x;y\nSELECT 2');
    });

    test('美元引号函数体内的光标命中整条 CREATE FUNCTION', () {
      const sql = 'CREATE FUNCTION f() RETURNS int AS \$\$\n'
          'BEGIN\n'
          '  RETURN 1;\n'
          'END\n'
          '\$\$ LANGUAGE plpgsql;\n'
          'SELECT f();';
      expect(
        sqlStatementAt(sql, sql.indexOf('RETURN 1')),
        contains('RETURN 1;'),
      );
      expect(sqlStatementAt(sql, sql.indexOf('SELECT f()')), 'SELECT f()');
    });
  });
}

/// 按固定块长 [size] 把 [script] 喂给切分器,汇总所有产出的语句
List<String> _feedIn(String script, int size) {
  final splitter = SqlStatementSplitter();
  final out = <String>[];
  for (var i = 0; i < script.length; i += size) {
    final end = (i + size) > script.length ? script.length : i + size;
    out.addAll(splitter.feed(script.substring(i, end)));
  }
  out.addAll(splitter.finish());
  return out;
}
