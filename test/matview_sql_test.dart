import 'package:daro/data/matview_sql.dart';
import 'package:flutter_test/flutter_test.dart';

// 实体化视图的 SQL 生成与目录查询解析(纯函数,不连库)。
//
// 关注点集中在两处「写错了不会报错、只是行为不对」的地方:
// 1. CREATE 的子句顺序 —— PG 文法要求 USING / WITH / TABLESPACE 都在 AS 之前,
//    顺序颠倒服务端直接语法错;
// 2. 编辑保存要 DROP + CREATE 重建 —— 索引原文必须能原样回放,
//    名称与字面量的引用一旦漏转义,重建就会打在别的对象上或干脆报错。

void main() {
  group('能力门控', () {
    test('只有 PostgreSQL 家族支持实体化视图', () {
      expect(MatViewSql.supports('postgresql'), isTrue);
      expect(MatViewSql.supports('aliyun-rds-postgres'), isTrue);
      expect(MatViewSql.supports('aliyun-polardb-postgres'), isTrue);
      expect(MatViewSql.supports('aliyun-oceanbase-postgres'), isTrue);
      for (final other in ['mysql', 'mariadb', 'sqlserver', 'sqlite', 'access']) {
        expect(MatViewSql.supports(other), isFalse, reason: other);
      }
    });
  });

  group('名称与字面量', () {
    test('无模式时落到 public', () {
      expect(MatViewSql.qualifiedName('mv'), '"public"."mv"');
      expect(MatViewSql.qualifiedName('mv', schema: 'report'), '"report"."mv"');
      // 空串按「没有模式」处理,而不是生成 `""."mv"`
      expect(MatViewSql.qualifiedName('mv', schema: ''), '"public"."mv"');
    });

    test('双引号标识符内的引号按 PG 规则翻倍', () {
      expect(MatViewSql.ident('a"b'), '"a""b"');
      expect(MatViewSql.lit("it's"), "'it''s'");
    });
  });

  group('CREATE', () {
    test('最简形态只有名称与查询', () {
      expect(
        MatViewSql.createSql(
            const MatViewSpec(name: 'mv', select: 'SELECT 1 FROM t')),
        'CREATE MATERIALIZED VIEW "public"."mv" AS SELECT 1 FROM t;',
      );
    });

    test('子句顺序:USING → WITH → TABLESPACE → AS → WITH NO DATA', () {
      final sql = MatViewSql.createSql(const MatViewSpec(
        name: 'mv',
        schema: 'report',
        select: 'SELECT 1',
        accessMethod: 'heap',
        storageOptions: ['fillfactor=90', 'autovacuum_enabled=false'],
        tablespace: 'fast',
        withData: false,
      ));
      expect(
        sql,
        'CREATE MATERIALIZED VIEW "report"."mv" USING "heap" '
        'WITH (fillfactor = 90, autovacuum_enabled = false) '
        'TABLESPACE "fast" AS SELECT 1 WITH NO DATA;',
      );
      // 顺序断言单独来一次:上面的整串相等已经锁死,这里补一句可读的定位
      expect(sql.indexOf('USING') < sql.indexOf('WITH ('), isTrue);
      expect(sql.indexOf('WITH (') < sql.indexOf('TABLESPACE'), isTrue);
      expect(sql.indexOf('TABLESPACE') < sql.indexOf(' AS '), isTrue);
    });

    test('空串与纯空白的可选项一律不生成子句', () {
      final sql = MatViewSql.createSql(const MatViewSpec(
        name: 'mv',
        select: 'SELECT 1',
        accessMethod: '  ',
        tablespace: '',
        storageOptions: ['  ', ''],
      ));
      expect(sql, 'CREATE MATERIALIZED VIEW "public"."mv" AS SELECT 1;');
      expect(sql, isNot(contains('WITH')));
      expect(sql, isNot(contains('TABLESPACE')));
    });

    test('查询尾部的分号会被吃掉,不会拼出双分号', () {
      final sql = MatViewSql.createSql(
          const MatViewSpec(name: 'mv', select: 'SELECT 1 ;  ;'));
      expect(sql, 'CREATE MATERIALIZED VIEW "public"."mv" AS SELECT 1;');
    });
  });

  group('刷新 / 删除 / 归属 / 注释', () {
    test('REFRESH 区分并发与非并发', () {
      expect(MatViewSql.refreshSql('mv'),
          'REFRESH MATERIALIZED VIEW "public"."mv";');
      expect(
          MatViewSql.refreshSql('mv', schema: 's', concurrently: true),
          'REFRESH MATERIALIZED VIEW CONCURRENTLY "s"."mv";');
    });

    test('DROP 默认带 IF EXISTS(重建流程里对象可能不存在)', () {
      expect(MatViewSql.dropSql('mv'),
          'DROP MATERIALIZED VIEW IF EXISTS "public"."mv";');
      expect(MatViewSql.dropSql('mv', ifExists: false),
          'DROP MATERIALIZED VIEW "public"."mv";');
    });

    test('ownerSql:留空 = 不改归属,返回空串让调用方跳过', () {
      expect(MatViewSql.ownerSql('mv', '  '), '');
      expect(MatViewSql.ownerSql('mv', 'postgres'),
          'ALTER MATERIALIZED VIEW "public"."mv" OWNER TO "postgres";');
    });

    test('注释里的单引号要转义', () {
      expect(
          MatViewSql.commentSql('mv', "it's ok"),
          "COMMENT ON MATERIALIZED VIEW \"public\".\"mv\" IS 'it''s ok';");
    });
  });

  group('索引', () {
    test('列名按逗号拆分并逐段引用;名字或列为空则不生成', () {
      expect(
        MatViewSql.createIndexSql('mv', 'idx_a', 'a, b', schema: 'report'),
        'CREATE INDEX "report"."idx_a" ON "report"."mv" '
        'USING "btree" ("a", "b");',
      );
      expect(
        MatViewSql.createIndexSql('mv', 'idx_a', 'a', unique: true, method: 'hash'),
        'CREATE UNIQUE INDEX "public"."idx_a" ON "public"."mv" '
        'USING "hash" ("a");',
      );
      expect(MatViewSql.createIndexSql('mv', '', 'a'), '');
      expect(MatViewSql.createIndexSql('mv', 'idx', ' , '), '');
    });

    test('DROP INDEX 只带索引名(索引是 schema 级对象,不像表级那样要 ON)', () {
      expect(MatViewSql.dropIndexSql('idx_a', schema: 'report'),
          'DROP INDEX "report"."idx_a";');
    });

    test('pg_get_indexdef 的原文回放:缺分号才补,不重复补', () {
      final spec = const MatViewSpec(name: 'mv', select: 'SELECT 1');
      final script = MatViewSql.fullScript(
        spec: spec,
        indexes: [
          'CREATE INDEX a ON mv (x)',
          'CREATE INDEX b ON mv (y);',
        ],
      );
      expect(script, contains('CREATE INDEX a ON mv (x);\n'));
      expect(script, isNot(contains('(y);;')));
    });
  });

  group('存储参数', () {
    test('reloptions 数组文本 → 逐项;空串给空列表', () {
      expect(MatViewSql.splitOptions('fillfactor=90, autovacuum_enabled=false'),
          ['fillfactor=90', 'autovacuum_enabled=false']);
      expect(MatViewSql.splitOptions(''), isEmpty);
      expect(MatViewSql.splitOptions(' , '), isEmpty);
    });

    test('normalizeOption 统一成 `k = v`;无等号原样保留', () {
      expect(MatViewSql.normalizeOption('fillfactor=90'), 'fillfactor = 90');
      expect(MatViewSql.normalizeOption('  a  =  b  '), 'a = b');
      expect(MatViewSql.normalizeOption('toast'), 'toast');
      // 值留空时不要生成 `a = `
      expect(MatViewSql.normalizeOption('a='), 'a');
    });

    test('界面一行文本 ↔ 子句 往返一致', () {
      const clause = 'WITH (fillfactor = 90, autovacuum_enabled = false)';
      expect(MatViewSql.storageOptionsClause(
              MatViewSql.parseOptionsText('fillfactor=90\nautovacuum_enabled=false')),
          clause);
      expect(MatViewSql.storageOptionsClause(const []), '');
    });
  });

  group('目录查询', () {
    test('定义查询按 relkind = \'m\' 过滤,不会误命中普通视图', () {
      final sql = MatViewSql.viewDefinitionSql('mv', schema: 'report');
      expect(sql, contains("c.relkind = 'm'"));
      expect(sql, contains('pg_get_viewdef'));
      expect(sql, contains("'report'"));
    });

    test('属性查询带 ispopulated 与注释', () {
      final sql = MatViewSql.propsSql('mv');
      expect(sql, contains('pg_matviews'));
      expect(sql, contains('ispopulated'));
      expect(sql, contains('pg_description'));
      // 表空间 / 访问方法未显式指定时目录里没有行,必须 LEFT JOIN 否则整行查不出
      expect(sql, contains('LEFT JOIN pg_tablespace'));
      expect(sql, contains('LEFT JOIN pg_am'));
    });

    test('索引查询取 pg_get_indexdef 原文(可直接回放)', () {
      expect(MatViewSql.indexesSql('mv'), contains('pg_get_indexdef'));
    });

    test('并发刷新探测:唯一 + 无 WHERE + 覆盖全部列', () {
      final sql = MatViewSql.concurrentlyReadySql('mv');
      expect(sql, contains('indisunique'));
      expect(sql, contains('indpred IS NULL'));
      expect(sql, contains('indnatts'));
    });

    test('访问方法只列表对象类(amtype)', () {
      expect(MatViewSql.accessMethodsSql, contains("amtype = 't'"));
    });

    test('EXPLAIN 探测语句去分号(PG 不接受尾分号)', () {
      expect(MatViewSql.explainSql('SELECT 1;'), 'EXPLAIN SELECT 1');
    });
  });

  group('结果解析', () {
    test('parseProps 列数不足时按缺省补齐(版本差异不炸界面)', () {
      final full = MatViewSql.parseProps(
          ['postgres', 'fast', 'heap', 'fillfactor=90', 'true', '注释']);
      expect(full.owner, 'postgres');
      expect(full.tablespace, 'fast');
      expect(full.accessMethod, 'heap');
      expect(full.storageOptions, ['fillfactor=90']);
      expect(full.populated, isTrue);
      expect(full.comment, '注释');

      final short = MatViewSql.parseProps(['postgres']);
      expect(short.tablespace, '');
      expect(short.storageOptions, isEmpty);
      // ispopulated 读不到时按「已填充」处理:界面不该凭空显示「无数据」
      expect(MatViewSql.parseProps(['', '', '', '', 'false']).populated, isFalse);
      expect(short.populated, isTrue);
    });

    test('parseIndexRow:valid 只在明确 false 时为假', () {
      final r = MatViewSql.parseIndexRow(
          ['idx', 'CREATE INDEX idx ON mv (a)', 'true', 'false']);
      expect(r.name, 'idx');
      expect(r.unique, isTrue);
      expect(r.valid, isFalse);
      // 缺列(老版本驱动少返回一列)不能把索引判成失效
      expect(MatViewSql.parseIndexRow(['idx', 'def']).valid, isTrue);
    });

    test('parseExists 三态:未知留给调用方放行,不当成 false', () {
      expect(MatViewSql.parseExists([['t']]), isTrue);
      expect(MatViewSql.parseExists([['true']]), isTrue);
      expect(MatViewSql.parseExists([['f']]), isFalse);
      expect(MatViewSql.parseExists(const []), isNull);
      expect(MatViewSql.parseExists([[]]), isNull);
      expect(MatViewSql.parseExists([['?']]), isNull);
    });

    test('firstCell 把空白当读不到', () {
      expect(MatViewSql.firstCell([[' SELECT 1 ']]), 'SELECT 1');
      expect(MatViewSql.firstCell([['  ']]), isNull);
      expect(MatViewSql.firstCell(const []), isNull);
    });
  });

  group('完整语句块(SQL 预览与转储共用)', () {
    test('编辑场景:DROP 在前,CREATE / 索引 / 归属 / 注释依次在后', () {
      final spec = const MatViewSpec(name: 'mv', select: 'SELECT 1');
      final script = MatViewSql.fullScript(
        spec: spec,
        dropFirst: MatViewSql.dropSql('mv'),
        indexes: ['CREATE INDEX idx ON mv (a)'],
        owner: 'postgres',
        comment: 'hello',
      );
      final lines = script.split('\n').where((l) => l.trim().isNotEmpty).toList();
      expect(lines[0], 'DROP MATERIALIZED VIEW IF EXISTS "public"."mv";');
      expect(lines[1], 'CREATE MATERIALIZED VIEW "public"."mv" AS SELECT 1;');
      expect(lines[2], 'CREATE INDEX idx ON mv (a);');
      expect(lines[3],
          'ALTER MATERIALIZED VIEW "public"."mv" OWNER TO "postgres";');
      expect(lines[4],
          'COMMENT ON MATERIALIZED VIEW "public"."mv" IS \'hello\';');
    });

    test('新建场景:没有 DROP;归属与注释留空不生成语句', () {
      final script = MatViewSql.fullScript(
        spec: const MatViewSpec(name: 'mv', select: 'SELECT 1'),
        owner: '',
        comment: '   ',
      );
      expect(script, isNot(contains('DROP')));
      expect(script, isNot(contains('OWNER')));
      expect(script, isNot(contains('COMMENT')));
      expect(script.trimRight().endsWith('SELECT 1;'), isTrue);
    });
  });
}
