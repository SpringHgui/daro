import 'package:daro/data/drivers/mysql_driver.dart';
import 'package:daro/data/drivers/pgsql_driver.dart';
import 'package:daro/data/drivers/sqlserver_driver.dart';
import 'package:daro/data/table_design.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('DdlBuilder PostgreSQL', () {
    test('完整设计生成 CREATE TABLE + 索引 + 触发器 + 注释', () {
      final d = DesignTable()
        ..name = 'orders'
        ..schema = 'public'
        ..tableComment = '订单表'
        ..fillFactor = '90'
        ..columns.addAll([
          DesignColumn(name: 'id', type: 'BIGSERIAL', notNull: true, primaryKey: true),
          DesignColumn(name: 'user_id', type: 'BIGINT', notNull: true, comment: '用户 ID'),
          DesignColumn(name: 'amount', type: 'DECIMAL', length: '10', decimal: '2', defaultValue: '0'),
          DesignColumn(name: 'status', type: 'VARCHAR', length: '20', notNull: true),
        ])
        ..indexes.add(DesignIndex(name: 'idx_status', columns: 'status', method: 'btree'))
        ..uniqueKeys.add(DesignUniqueKey(name: 'uq_status', columns: 'status'))
        ..checks.add(DesignCheck(name: 'ck_amount', expression: 'amount >= 0'))
        ..foreignKeys.add(DesignForeignKey(
          name: 'fk_user',
          columns: 'user_id',
          refTable: 'users',
          refColumns: 'id',
          onDelete: 'CASCADE',
          onUpdate: 'NO ACTION',
        ))
        ..triggers.add(DesignTrigger(
          name: 'trg_update_ts',
          timing: 'BEFORE',
          insert: true,
          update: true,
          forEach: '行',
          when: 'NEW.status IS NOT NULL',
          function: 'public.update_ts',
          parameters: 'NEW',
        ));

      final stmts = DdlBuilder.buildStatements(d, 'postgresql');
      final sql = stmts.join('\n');

      // CREATE TABLE 主体
      expect(sql, contains('CREATE TABLE "public"."orders" ('));
      expect(sql, contains('"id" BIGSERIAL NOT NULL PRIMARY KEY'));
      expect(sql, contains('"amount" DECIMAL(10,2) DEFAULT 0'));
      expect(sql, contains('WITH (fillfactor = 90)'));
      // 约束内联
      expect(sql, contains('CONSTRAINT "uq_status" UNIQUE ("status")'));
      expect(sql, contains('CONSTRAINT "ck_amount" CHECK (amount >= 0)'));
      expect(sql, contains('CONSTRAINT "fk_user" FOREIGN KEY ("user_id") REFERENCES "public"."users" ("id") ON DELETE CASCADE'));
      // 独立语句
      expect(sql, contains('CREATE INDEX "idx_status" ON "public"."orders" USING btree ("status");'));
      expect(sql, contains('CREATE TRIGGER "trg_update_ts" BEFORE INSERT OR UPDATE ON "public"."orders" FOR EACH ROW WHEN (NEW.status IS NOT NULL) EXECUTE FUNCTION "public"."update_ts"(NEW);'));
      expect(sql, contains('COMMENT ON TABLE "public"."orders" IS \'订单表\';'));
      expect(sql, contains('COMMENT ON COLUMN "public"."orders"."user_id" IS \'用户 ID\';'));

      // 语句拆分:CREATE TABLE / INDEX / TRIGGER / COMMENT TABLE / COMMENT COLUMN
      expect(stmts.length, greaterThanOrEqualTo(5));
    });

    test('空名约束自动命名 + 多列主键表级约束', () {
      final d = DesignTable()
        ..name = 't'
        ..columns.addAll([
          DesignColumn(name: 'a', type: 'INT', primaryKey: true),
          DesignColumn(name: 'b', type: 'INT', primaryKey: true),
        ])
        ..uniqueKeys.add(DesignUniqueKey(columns: 'b'));
      final sql = DdlBuilder.buildPreview(d, 'postgresql');
      expect(sql, contains('PRIMARY KEY ("a", "b")'));
      expect(sql, contains('CONSTRAINT "uq_t_b" UNIQUE ("b")'));
      expect(sql, contains('CREATE TABLE "t" ('));
    });

    test('可延迟 / 延迟 子句', () {
      final d = DesignTable()
        ..name = 't'
        ..columns.add(DesignColumn(name: 'a', type: 'INT'))
        ..foreignKeys.add(DesignForeignKey(
          columns: 'a',
          refTable: 'r',
          refColumns: 'id',
          deferrable: 'YES',
          deferred: 'YES',
        ));
      final sql = DdlBuilder.buildPreview(d, 'postgresql');
      expect(sql, contains('DEFERRABLE INITIALLY DEFERRED'));
    });
  });

  group('DdlBuilder MySQL', () {
    test('内联注释与反引号标识符', () {
      final d = DesignTable()
        ..name = 'user'
        ..tableComment = '用户表'
        ..columns.addAll([
          DesignColumn(name: 'id', type: 'BIGINT', notNull: true, primaryKey: true, comment: '主键'),
          DesignColumn(name: 'name', type: 'VARCHAR', length: '255'),
        ])
        ..indexes.add(DesignIndex(name: 'idx_name', columns: 'name', unique: true));
      final sql = DdlBuilder.buildPreview(d, 'mysql');
      expect(sql, contains('CREATE TABLE `user` ('));
      expect(sql, contains('`id` BIGINT NOT NULL PRIMARY KEY COMMENT \'主键\''));
      expect(sql, contains(') COMMENT = \'用户表\';'));
      expect(sql, contains('CREATE UNIQUE INDEX `idx_name` ON `user` (`name`);'));
      // MySQL 不生成 COMMENT ON
      expect(sql, isNot(contains('COMMENT ON')));
    });

    test('FULLTEXT / SPATIAL 是索引类型关键字,不落成尾部 USING', () {
      final d = DesignTable()
        ..name = 'enter_car'
        ..columns.addAll([
          DesignColumn(name: 'plate', type: 'VARCHAR', length: '64'),
          DesignColumn(name: 'geo', type: 'GEOMETRY'),
          DesignColumn(name: 'start_time', type: 'DATETIME'),
          DesignColumn(name: 'flags', type: 'INT'),
        ])
        ..indexes.addAll([
          DesignIndex(name: 'ft_plate', columns: 'plate', method: 'fulltext'),
          DesignIndex(name: 'sp_geo', columns: 'geo', method: 'SPATIAL'),
          DesignIndex(name: 'ix_time', columns: 'start_time', method: 'btree'),
          DesignIndex(name: 'ix_flags', columns: 'flags', method: 'hash'),
        ]);

      final sql = DdlBuilder.buildStatements(d, 'mysql').join('\n');
      expect(sql, contains('CREATE FULLTEXT INDEX `ft_plate` ON `enter_car` (`plate`);'));
      expect(sql, contains('CREATE SPATIAL INDEX `sp_geo` ON `enter_car` (`geo`);'));
      expect(sql, isNot(contains('USING fulltext')));
      expect(sql, isNot(contains('USING SPATIAL')));
      expect(sql, contains('CREATE INDEX `ix_time` ON `enter_car` (`start_time`) USING btree;'));
      expect(sql, contains('CREATE INDEX `ix_flags` ON `enter_car` (`flags`) USING hash;'));
    });
  });

  group('DdlBuilder IDENTITY(虚拟类型)', () {
    test('PostgreSQL 生成 GENERATED … AS IDENTITY 及序列选项', () {
      final d = DesignTable()
        ..name = 'configs'
        ..schema = 'public'
        ..columns.addAll([
          DesignColumn(
            name: 'id',
            type: 'int8',
            length: '64',
            notNull: true,
            primaryKey: true,
            identityMode: 'BY DEFAULT',
            identityIncrement: '1',
            identityMinValue: '1',
            identityMaxValue: '9223372036854775807',
            identityStart: '1',
            identityCache: '1',
          ),
          DesignColumn(name: 'name', type: 'varchar', length: '32', notNull: true),
        ]);
      final sql = DdlBuilder.buildPreview(d, 'postgresql');
      expect(
        sql,
        contains('"id" int8 GENERATED BY DEFAULT AS IDENTITY '
            '(INCREMENT BY 1 MINVALUE 1 MAXVALUE 9223372036854775807 START WITH 1 CACHE 1) '
            'NOT NULL'),
      );
      // int8 的「长度」是位宽信息,不拼进类型
      expect(sql, isNot(contains('int8(64)')));
      expect(sql, contains('"name" varchar(32) NOT NULL'));
    });

    test('CYCLE 输出 / 留空项不输出 / 无选项时省略括号', () {
      String build({bool cycle = false, String cache = ''}) {
        final d = DesignTable()
          ..name = 't'
          ..columns.add(DesignColumn(
            name: 'id',
            type: 'int4',
            identityMode: 'ALWAYS',
            identityIncrement: '2',
            identityCache: cache,
            identityCycle: cycle,
          ));
        return DdlBuilder.buildPreview(d, 'postgresql');
      }

      expect(
        build(cycle: true, cache: '5'),
        contains('GENERATED ALWAYS AS IDENTITY (INCREMENT BY 2 CACHE 5 CYCLE)'),
      );
      expect(
        build(),
        contains('GENERATED ALWAYS AS IDENTITY (INCREMENT BY 2)'),
      );
      final bare = DesignTable()
        ..name = 't'
        ..columns.add(DesignColumn(name: 'id', type: 'int4', identityMode: 'ALWAYS'));
      final sql = DdlBuilder.buildPreview(bare, 'postgresql');
      expect(sql, contains('GENERATED ALWAYS AS IDENTITY'));
      expect(sql, isNot(contains('IDENTITY (')));
    });

    test('MySQL 降级 AUTO_INCREMENT / SQL Server IDENTITY(seed,inc) / SQLite 忽略', () {
      DesignTable design() => DesignTable()
        ..name = 't'
        ..columns.add(DesignColumn(
          name: 'id',
          type: 'int',
          notNull: true,
          identityMode: 'BY DEFAULT',
          identityStart: '10',
          identityIncrement: '2',
        ));

      expect(DdlBuilder.buildPreview(design(), 'mysql'),
          contains('`id` int NOT NULL AUTO_INCREMENT'));
      expect(DdlBuilder.buildPreview(design(), 'sqlserver'),
          contains('[id] int IDENTITY(10,2) NOT NULL'));
      final lite = DdlBuilder.buildPreview(design(), 'sqlite');
      expect(lite, isNot(contains('IDENTITY')));
      expect(lite, isNot(contains('AUTO_INCREMENT')));
    });

    test('主键序号按列序推导', () {
      final a = DesignColumn(name: 'a', type: 'int', primaryKey: true);
      final b = DesignColumn(name: 'b', type: 'int');
      final c = DesignColumn(name: 'c', type: 'int', primaryKey: true);
      final d = DesignTable()
        ..name = 't'
        ..columns.addAll([a, b, c]);
      final ordinals = DdlBuilder.pkOrdinals(d);
      expect(ordinals[a], 1);
      expect(ordinals[b], isNull);
      expect(ordinals[c], 2);
    });
  });

  group('DdlBuilder 类型渲染', () {
    test('无长度参数的类型不拼接长度 / 小数点', () {
      expect(DesignColumn(type: 'int8', length: '64', decimal: '2').fullType, 'int8');
      expect(DesignColumn(type: 'varchar', length: '32').fullType, 'varchar(32)');
      expect(DesignColumn(type: 'decimal', length: '10', decimal: '2').fullType,
          'decimal(10,2)');
      expect(DesignColumn(type: 'VARCHAR', length: '20').fullType, 'VARCHAR(20)');
      // 新列默认:类型与长度分段录入
      expect(DesignColumn().fullType, 'varchar(255)');
    });

    test('IDENTITY 上限按类型给出', () {
      expect(kIdentityMaxValue('int2'), '32767');
      expect(kIdentityMaxValue('serial'), '2147483647');
      expect(kIdentityMaxValue('int8'), '9223372036854775807');
      expect(kIdentityMaxValue('varchar'), '');
      expect(bitWidthOf('int8'), '64');
      expect(bitWidthOf('varchar'), isNull);
    });
  });

  group('DdlBuilder 校验', () {
    test('空表名 / 空列名校验', () {
      expect(DdlBuilder.validate(DesignTable(), typeId: 'postgresql').ok, isFalse);
      final d = DesignTable()
        ..columns.add(DesignColumn(name: '  '));
      expect(DdlBuilder.validate(d, typeId: 'postgresql').ok, isFalse);
      final ok = DesignTable()
        ..columns.add(DesignColumn(name: 'a', type: 'INT'));
      expect(DdlBuilder.validate(ok, typeId: 'postgresql').ok, isTrue);
    });

    test('IDENTITY 序列项必须是整数', () {
      final d = DesignTable()
        ..columns.add(DesignColumn(
          name: 'id',
          type: 'int8',
          identityMode: 'BY DEFAULT',
          identityIncrement: 'x',
        ));
      final r = DdlBuilder.validate(d, typeId: 'postgresql');
      expect(r.ok, isFalse);
      expect(r.error, contains('递增'));
      // 留空 = 交由数据库默认,不报错
      final okTable = DesignTable()
        ..columns.add(DesignColumn(name: 'id', type: 'int8', identityMode: 'BY DEFAULT'));
      expect(DdlBuilder.validate(okTable, typeId: 'postgresql').ok, isTrue);
    });
  });

  group('DdlBuilder 索引字段项(「选择数据表字段」弹窗)', () {
    DesignTable oneCol() => DesignTable()
      ..name = 't'
      ..columns.add(DesignColumn(name: 'c', type: 'varchar', length: '32'));

    test('PostgreSQL 输出 COLLATE / 运算符类别 / 排序顺序 / Nulls 排序', () {
      final d = oneCol()
        ..indexes.add(DesignIndex(name: 'idx_c', columns: 'c')
          ..columnItems.add(DesignIndexColumn(
            name: 'c',
            collation: 'C',
            order: 'DESC',
            nullsOrder: 'FIRST',
          )));
      final sql = DdlBuilder.buildPreview(d, 'postgresql');
      expect(
        sql,
        contains('CREATE INDEX "idx_c" ON "t" ("c" COLLATE "C" DESC NULLS FIRST);'),
      );
    });

    test('排序规则 / 运算符类别的模式前缀按限定名输出', () {
      final d = oneCol()
        ..indexes.add(DesignIndex(name: 'idx_c', columns: 'c')
          ..columnItems.add(DesignIndexColumn(
            name: 'c',
            collationSchema: 'pg_catalog',
            collation: 'C',
            opClassSchema: 'public',
            opClass: 'varchar_pattern_ops',
          )));
      final sql = DdlBuilder.buildPreview(d, 'postgresql');
      expect(
        sql,
        contains('("c" COLLATE "pg_catalog"."C" "public"."varchar_pattern_ops");'),
      );
    });

    test('非 PostgreSQL 忽略字段项选项(只输出裸字段名)', () {
      final d = oneCol()
        ..indexes.add(DesignIndex(name: 'idx_c', columns: 'c')
          ..columnItems.add(DesignIndexColumn(name: 'c', collation: 'C', order: 'DESC')));
      expect(DdlBuilder.buildPreview(d, 'mysql'), contains('CREATE INDEX `idx_c` ON `t` (`c`);'));
    });

    test('字段项确认后回写逗号串', () {
      final idx = DesignIndex(columns: 'a');
      idx.columnItems.addAll([
        DesignIndexColumn(name: 'b'),
        DesignIndexColumn(name: 'a'),
      ]);
      idx.applyColumnItems();
      expect(idx.columns, 'b, a');
      expect(idx.columnList, ['b', 'a']);
    });

    test('copy 深拷贝字段项(快照基线不被后续编辑污染)', () {
      final idx = DesignIndex(name: 'i', columns: 'c')
        ..columnItems.add(DesignIndexColumn(name: 'c', order: 'ASC'));
      final dup = idx.copy();
      dup.columnItems.first.order = 'DESC';
      expect(idx.columnItems.first.order, 'ASC');
    });
  });

  group('DdlBuilder 表选项(不记录 / 所有者 / 继承 / 集群)', () {
    DesignTable opts() => DesignTable()
      ..name = 't'
      ..schema = 'public'
      ..columns.add(DesignColumn(name: 'a', type: 'int'))
      ..unlogged = true
      ..inherits = 'public.base, other'
      ..owner = 'postgres'
      ..cluster = 'idx_a'
      ..indexes.add(DesignIndex(name: 'idx_a', columns: 'a'));

    test('PostgreSQL 生成 UNLOGGED + INHERITS,并各补一条 OWNER TO / CLUSTER', () {
      final stmts = DdlBuilder.buildStatements(opts(), 'postgresql');
      expect(
        stmts.first,
        'CREATE UNLOGGED TABLE "public"."t" (\n  "a" int\n) INHERITS ("public"."base", "other");',
      );
      expect(stmts, contains('ALTER TABLE "public"."t" OWNER TO "postgres";'));
      expect(stmts, contains('CLUSTER "public"."t" USING "idx_a";'));
    });

    test('MySQL / SQLite 不生成这些子句', () {
      final sql = DdlBuilder.buildPreview(opts(), 'mysql');
      expect(sql, isNot(contains('UNLOGGED')));
      expect(sql, isNot(contains('INHERITS')));
      expect(sql, isNot(contains('OWNER TO')));
      expect(sql, isNot(contains('CLUSTER')));
    });

    test('留空时与从前一致(无多余子句)', () {
      final d = DesignTable()
        ..name = 't'
        ..columns.add(DesignColumn(name: 'a', type: 'int'));
      final stmts = DdlBuilder.buildStatements(d, 'postgresql');
      expect(stmts.length, 1);
      expect(stmts.first, startsWith('CREATE TABLE "t" ('));
    });
  });

  group('DdlBuilder 约束触发器 / 外键注释', () {
    DesignTable base() => DesignTable()
      ..name = 't'
      ..columns.add(DesignColumn(name: 'a', type: 'int'));

    test('可延迟 + 延迟 生成 CREATE CONSTRAINT TRIGGER … DEFERRABLE INITIALLY DEFERRED', () {
      final d = base()
        ..triggers.add(DesignTrigger(
          name: 'trg',
          timing: 'AFTER',
          insert: true,
          function: 'trg_fn',
          deferrable: 'YES',
          deferred: 'YES',
        ));
      expect(
        DdlBuilder.buildPreview(d, 'postgresql'),
        contains('CREATE CONSTRAINT TRIGGER "trg" AFTER INSERT ON "t" '
            'DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION "trg_fn";'),
      );
    });

    test('未置可延迟时不输出 INITIALLY DEFERRED(PG 语法要求两者搭配)', () {
      final d = base()
        ..triggers.add(DesignTrigger(
          name: 'trg',
          timing: 'AFTER',
          insert: true,
          function: 'trg_fn',
          deferred: 'YES',
        ));
      final sql = DdlBuilder.buildPreview(d, 'postgresql');
      expect(sql, isNot(contains('CONSTRAINT TRIGGER')));
      expect(sql, isNot(contains('DEFERRABLE')));
    });

    test('外键注释生成 COMMENT ON CONSTRAINT(仅 PostgreSQL)', () {
      final d = base()
        ..foreignKeys.add(DesignForeignKey(
          name: 'fk_a',
          columns: 'a',
          refTable: 'r',
          refColumns: 'id',
          comment: '级联到用户表',
        ));
      expect(
        DdlBuilder.buildPreview(d, 'postgresql'),
        contains('COMMENT ON CONSTRAINT "fk_a" ON "t" IS \'级联到用户表\';'),
      );
      expect(DdlBuilder.buildPreview(d, 'mysql'), isNot(contains('COMMENT ON')));
    });

    test('索引 / 唯一键 / 检查 / 排除 注释:PG 生成 COMMENT ON,MySQL 索引注释内联', () {
      final d = base()
        ..indexes.add(DesignIndex(name: 'idx_a', columns: 'a', comment: 'A 索引'))
        ..uniqueKeys.add(DesignUniqueKey(name: 'uq_a', columns: 'a', comment: 'A 唯一'))
        ..checks.add(DesignCheck(name: 'ck_a', expression: 'a > 0', comment: 'A 为正'))
        ..excludes.add(DesignExclude(name: 'ex_a', columns: 'a with =', comment: 'A 排重'));
      final pgSql = DdlBuilder.buildPreview(d, 'postgresql');
      expect(pgSql, contains('COMMENT ON INDEX "idx_a" IS \'A 索引\';'));
      expect(pgSql, contains('COMMENT ON CONSTRAINT "uq_a" ON "t" IS \'A 唯一\';'));
      expect(pgSql, contains('COMMENT ON CONSTRAINT "ck_a" ON "t" IS \'A 为正\';'));
      expect(pgSql, contains('COMMENT ON CONSTRAINT "ex_a" ON "t" IS \'A 排重\';'));

      // MySQL 只有索引注释能内联进 CREATE INDEX,不会多生 COMMENT ON 语句
      d
        ..uniqueKeys.clear()
        ..checks.clear()
        ..excludes.clear();
      final mySql = DdlBuilder.buildPreview(d, 'mysql');
      expect(mySql, contains('CREATE INDEX `idx_a` ON `t` (`a`) COMMENT \'A 索引\';'));
      expect(mySql, isNot(contains('COMMENT ON')));
    });
  });

  group('DdlBuilder 触发器按方言生成', () {
    // 只放一个合法列:validate 要求「至少一列 + 列名非空」,触发器用例不关心列
    DesignTable t() => DesignTable()
      ..name = 'orders'
      ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true));

    test('MySQL:单事件保留原名,BEGIN … END 包裹,尾分号不重复', () {
      final d = t()
        ..triggers.add(DesignTrigger(
            name: 'trg_bi', timing: 'BEFORE', insert: true, body: 'SET NEW.id = 1;'));
      final sql = DdlBuilder.buildPreview(d, 'mysql');
      expect(
          sql,
          contains('CREATE TRIGGER `trg_bi` BEFORE INSERT ON `orders` FOR EACH ROW\n'
              'BEGIN\n  SET NEW.id = 1;\nEND;'));
      expect(sql, isNot(contains(';;')));
      // MySQL 没有语句级触发器:界面上的「给每个」不参与生成
      expect(sql, isNot(contains('FOR EACH STATEMENT')));
    });

    test('MySQL:勾多个事件各出一条 CREATE(一个触发器只能挂一个事件)', () {
      final d = t()
        ..triggers.add(DesignTrigger(
            name: 'trg_x', update: true, insert: true, body: 'SELECT 1'));
      final stmts = DdlBuilder.buildStatements(d, 'mysql')
          .where((s) => s.startsWith('CREATE TRIGGER'))
          .toList();
      expect(stmts, hasLength(2));
      // 同名会撞 1359,故按事件加后缀;顺序 = 勾选列的界面顺序
      expect(stmts[0], contains('`trg_x_insert` BEFORE INSERT'));
      expect(stmts[1], contains('`trg_x_update` BEFORE UPDATE'));
    });

    test('MySQL:AFTER 时机与多语句主体原样落在 BEGIN … END 里', () {
      final d = t()
        ..triggers.add(DesignTrigger(
            name: 'trg_a',
            timing: 'AFTER',
            delete: true,
            body: 'DELETE FROM log WHERE a = OLD.id;\nINSERT INTO log VALUES (1)'));
      final sql = DdlBuilder.buildPreview(d, 'mysql');
      expect(sql, contains('`trg_a` AFTER DELETE ON `orders` FOR EACH ROW'));
      expect(sql, contains('BEGIN\n  DELETE FROM log WHERE a = OLD.id;\n'
          'INSERT INTO log VALUES (1);\nEND;'));
    });

    test('SQL Server:事件用逗号列表,一条 CREATE,语句级无 FOR EACH ROW', () {
      final d = t()
        ..triggers.add(DesignTrigger(
            name: 'trg_s', timing: 'AFTER', insert: true, update: true,
            body: 'UPDATE audit SET n = n + 1'));
      final sql = DdlBuilder.buildPreview(d, 'sqlserver');
      expect(
          sql,
          contains('CREATE TRIGGER [trg_s] ON [orders] AFTER INSERT, UPDATE AS BEGIN\n'
              '  UPDATE audit SET n = n + 1;\nEND;'));
      expect(sql, isNot(contains('FOR EACH')));
    });

    test('SQL Server:有模式时触发器名带模式前缀(它是 schema 级对象)', () {
      final d = t()
        ..schema = 'sales'
        ..triggers.add(
            DesignTrigger(name: 'trg_s', timing: 'INSTEAD OF', insert: true, body: 'SELECT 1'));
      final sql = DdlBuilder.buildPreview(d, 'sqlserver');
      expect(sql, contains('CREATE TRIGGER [sales].[trg_s] ON [sales].[orders] '
          'INSTEAD OF INSERT'));
    });

    test('PostgreSQL:输出与改造前逐字一致(重构未改行为)', () {
      final d = t()
        ..schema = 'public'
        ..triggers.add(DesignTrigger(
            name: 'trg_p',
            timing: 'BEFORE',
            insert: true,
            update: true,
            forEach: '行',
            when: 'NEW.id IS NOT NULL',
            function: 'public.f',
            parameters: 'NEW'));
      final sql = DdlBuilder.buildPreview(d, 'postgresql');
      expect(
          sql,
          contains('CREATE TRIGGER "trg_p" BEFORE INSERT OR UPDATE ON "public"."orders" '
              'FOR EACH ROW WHEN (NEW.id IS NOT NULL) EXECUTE FUNCTION "public"."f"(NEW);'));
    });

    test('必填项按方言:PG 要触发函数,MySQL / SQL Server 要触发主体', () {
      final noFn = t()
        ..triggers.add(DesignTrigger(name: 'x', insert: true));
      expect(DdlBuilder.validate(noFn, typeId: 'postgresql').error, contains('触发函数'));
      expect(DdlBuilder.validate(noFn, typeId: 'mysql').error, contains('触发主体'));
      final noBody = t()
        ..triggers.add(DesignTrigger(name: 'x', insert: true, function: 'f'));
      expect(DdlBuilder.validate(noBody, typeId: 'sqlserver').error, contains('触发主体'));
      final ok = t()
        ..triggers.add(DesignTrigger(name: 'x', insert: true, body: 'SELECT 1'));
      expect(DdlBuilder.validate(ok, typeId: 'mysql').ok, isTrue);
    });

    test('TRUNCATE 事件仅 PG;无事件的行不生成;SQLite 整标签不开放', () {
      final trunc = t()
        ..triggers.add(DesignTrigger(
            name: 'x', truncate: true, body: 'SELECT 1', function: 'f'));
      expect(DdlBuilder.validate(trunc, typeId: 'mysql').error, contains('TRUNCATE'));
      expect(DdlBuilder.validate(trunc, typeId: 'postgresql').ok, isTrue);

      final noEvent = t()
        ..triggers.add(DesignTrigger(name: 'x', body: 'SELECT 1'));
      // 一个事件都没勾 → 生成不出可执行的 CREATE,不输出半条语句
      expect(DdlBuilder.buildStatements(noEvent, 'mysql')
          .where((s) => s.contains('CREATE TRIGGER')), isEmpty);

      final sqlite = t()
        ..triggers.add(DesignTrigger(name: 'x', insert: true, body: 'SELECT 1'));
      expect(DdlBuilder.validate(sqlite, typeId: 'sqlite').error,
          contains('不支持在设计器中定义触发器'));
    });
  });

  group('DdlBuilder 新建表:方言表达不了的设计项', () {
    DesignTable t() => DesignTable()
      ..name = 'orders'
      ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true));

    test('PG 上的规则 / 排除不拦,换到 MySQL 上必须拦(从前是静默丢弃)', () {
      final d = t()..rules.add(DesignRule(name: 'r1', event: 'INSERT', statement: 'NOTHING'));
      expect(DdlBuilder.createUnsupported(d, 'postgresql'), isNull);
      expect(DdlBuilder.createUnsupported(d, 'mysql'), contains('规则'));
    });

    test('排除约束仅 PG', () {
      final d = t()..excludes.add(DesignExclude(name: 'ex', columns: 'a WITH ='));
      expect(DdlBuilder.createUnsupported(d, 'postgresql'), isNull);
      expect(DdlBuilder.createUnsupported(d, 'mysql'), contains('排除'));
    });

    test('索引:并发 / 表空间仅 PG,填充因子 PG 与 SQL Server', () {
      final concurrent = t()..indexes.add(DesignIndex(name: 'i', columns: 'id', concurrent: true));
      expect(DdlBuilder.createUnsupported(concurrent, 'mysql'), contains('并发'));
      expect(DdlBuilder.createUnsupported(concurrent, 'postgresql'), isNull);

      final ts = t()..indexes.add(DesignIndex(name: 'i', columns: 'id', tablespace: 'ts1'));
      expect(DdlBuilder.createUnsupported(ts, 'sqlserver'), contains('表空间'));

      final ff = t()..indexes.add(DesignIndex(name: 'i', columns: 'id', fillFactor: '90'));
      expect(DdlBuilder.createUnsupported(ff, 'mysql'), contains('填充因子'));
      expect(DdlBuilder.createUnsupported(ff, 'sqlserver'), isNull);
    });

    test('索引字段弹窗的逐字段选项仅 PG', () {
      final d = t()
        ..indexes.add(DesignIndex(name: 'i', columns: 'id')
          ..columnItems.add(DesignIndexColumn(name: 'id', order: 'DESC')));
      expect(DdlBuilder.createUnsupported(d, 'mysql'), contains('排序'));
      expect(DdlBuilder.createUnsupported(d, 'postgresql'), isNull);
    });

    test('注释:外键 / 唯一键 / 检查 / 排除仅 PG;表与列注释各方言各有出路', () {
      final fk = t()..foreignKeys.add(DesignForeignKey(
          columns: 'id', refTable: 'p', refColumns: 'id', comment: 'C'));
      expect(DdlBuilder.createUnsupported(fk, 'mysql'), contains('外键注释'));
      expect(DdlBuilder.createUnsupported(fk, 'postgresql'), isNull);

      final uk = t()..uniqueKeys.add(DesignUniqueKey(name: 'u', columns: 'id', comment: 'C'));
      expect(DdlBuilder.createUnsupported(uk, 'sqlserver'), contains('唯一键注释'));
    });

    test('表选项:不记录 / 所有者 / 继承 / 集群 / 表空间仅 PG', () {
      for (final entry in <String, void Function(DesignTable)>{
        '不记录': (d) => d.unlogged = true,
        '所有者': (d) => d.owner = 'postgres',
        '继承': (d) => d.inherits = 'p',
        '集群': (d) => d.cluster = 'i',
        '表空间': (d) => d.tablespace = 'ts',
      }.entries) {
        final d = t();
        entry.value(d);
        expect(DdlBuilder.createUnsupported(d, 'mysql'), contains(entry.key),
            reason: entry.key);
        expect(DdlBuilder.createUnsupported(d, 'postgresql'), isNull,
            reason: entry.key);
      }
    });

    test('干净设计一律放行', () {
      expect(DdlBuilder.createUnsupported(t(), 'mysql'), isNull);
      expect(DdlBuilder.createUnsupported(t(), 'sqlserver'), isNull);
      expect(DdlBuilder.createUnsupported(t(), 'postgresql'), isNull);
    });
  });

  group('DdlBuilder SQL Server 注释走扩展属性', () {
    test('新建带注释的表不再发 COMMENT ON(那是 PG 语法,SQL Server 必报语法错)', () {
      final d = DesignTable()
        ..name = 't'
        ..schema = 'dbo'
        ..tableComment = '订单表'
        ..columns.addAll([
          DesignColumn(name: 'id', type: 'int', primaryKey: true),
          DesignColumn(name: 'note', type: 'nvarchar', length: '50', comment: '备注'),
        ]);
      final sql = DdlBuilder.buildPreview(d, 'sqlserver');
      expect(sql, isNot(contains('COMMENT ON')));
      expect(
          sql,
          contains("EXEC sp_addextendedproperty 'MS_Description', '订单表', "
              "'SCHEMA', 'dbo', 'TABLE', 't';"));
      expect(
          sql,
          contains("EXEC sp_addextendedproperty 'MS_Description', '备注', "
              "'SCHEMA', 'dbo', 'TABLE', 't', 'COLUMN', 'note';"));
    });

    test('PG 仍用 COMMENT ON(未被本次改动波及)', () {
      final d = DesignTable()
        ..name = 't'
        ..tableComment = '订单表'
        ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true));
      expect(DdlBuilder.buildPreview(d, 'postgresql'),
          contains('COMMENT ON TABLE "t" IS \'订单表\';'));
    });
  });

  group('DdlBuilder 唯一键的表空间与填充因子', () {
    test('PG 按「WITH → TABLESPACE → DEFERRABLE」顺序输出', () {
      final d = DesignTable()
        ..name = 't'
        ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true))
        ..uniqueKeys.add(DesignUniqueKey(
            name: 'u', columns: 'id', fillFactor: '90', tablespace: 'ts1', deferrable: 'YES'));
      final sql = DdlBuilder.buildPreview(d, 'postgresql');
      expect(
          sql,
          contains('CONSTRAINT "u" UNIQUE ("id") WITH (fillfactor = 90) '
              'TABLESPACE "ts1" DEFERRABLE'));
    });

    test('非 PG 不输出该子句,且由 createUnsupported 拦住填写', () {
      final d = DesignTable()
        ..name = 't'
        ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true))
        ..uniqueKeys.add(DesignUniqueKey(name: 'u', columns: 'id', tablespace: 'ts1'));
      expect(DdlBuilder.buildPreview(d, 'mysql'), isNot(contains('TABLESPACE')));
      expect(DdlBuilder.createUnsupported(d, 'mysql'), contains('唯一键表空间'));
    });
  });

  group('DdlBuilder 外键 MATCH FULL', () {
    test('MySQL 家族同样生成(从前只有 PG 会输出)', () {
      final fk = DesignForeignKey(columns: 'a', refTable: 'p', refColumns: 'id', matchAll: true);
      final d = DesignTable()
        ..name = 't'
        ..columns.add(DesignColumn(name: 'a', type: 'int', primaryKey: true))
        ..foreignKeys.add(fk);
      expect(DdlBuilder.buildPreview(d, 'mysql'), contains(') MATCH FULL'));
      expect(DdlBuilder.buildPreview(d, 'postgresql'), contains(') MATCH FULL'));
      // SQL Server 无 MATCH 子句 → 拦口负责说明,而不是静默丢
      expect(DdlBuilder.createUnsupported(d, 'sqlserver'), contains('符合全部'));
    });
  });

  group('触发器编辑(编辑模式)', () {
    DesignTable base() => DesignTable()
      ..name = 't'
      ..schema = 'public'
      ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true));

    test('新增触发器生成 CREATE(不再被 alterUnsupported 拦住)', () {
      final target = base()
        ..triggers.add(DesignTrigger(
            name: 'trg2', timing: 'AFTER', insert: true, function: 'f2'));
      final original = base();
      expect(DdlBuilder.alterUnsupported(target, original, 'postgresql'), isNull);
      final sql =
          DdlBuilder.buildAlterStatements(target, original, 'postgresql').join('\n');
      expect(sql, contains('CREATE TRIGGER "trg2" AFTER INSERT ON "public"."t"'));
    });

    test('改触发函数 / 改事件按「先 DROP 再 CREATE」重建', () {
      final original = base()
        ..triggers.add(DesignTrigger(
            name: 'trg', timing: 'AFTER', insert: true, function: 'f'));
      final target = base()
        ..triggers.add(DesignTrigger(
            name: 'trg', timing: 'AFTER', insert: true, function: 'f_changed'));
      expect(DdlBuilder.alterUnsupported(target, original, 'postgresql'), isNull);
      final stmts = DdlBuilder.buildAlterStatements(target, original, 'postgresql');
      final sql = stmts.join('\n');
      // DROP 必须先于 CREATE:同名触发器不能并存
      final dropAt = sql.indexOf('DROP TRIGGER');
      final createAt = sql.indexOf('CREATE TRIGGER');
      expect(dropAt, greaterThanOrEqualTo(0));
      expect(createAt, greaterThan(dropAt));
      expect(sql, contains('DROP TRIGGER IF EXISTS "trg" ON "public"."t";'));
      expect(sql, contains('CREATE TRIGGER "trg" AFTER INSERT ON "public"."t"'));
    });

    test('删除触发器只生成 DROP', () {
      final original = base()
        ..triggers.add(DesignTrigger(
            name: 'trg', timing: 'AFTER', insert: true, function: 'f'));
      final target = base();
      expect(DdlBuilder.alterUnsupported(target, original, 'postgresql'), isNull);
      final sql =
          DdlBuilder.buildAlterStatements(target, original, 'postgresql').join('\n');
      expect(sql, contains('DROP TRIGGER IF EXISTS "trg" ON "public"."t";'));
      expect(sql, isNot(contains('CREATE TRIGGER')));
    });

    test('无变化不产生语句;尾分号 / 换行差异不算变更', () {
      final original = base()
        ..triggers.add(DesignTrigger(
            name: 'trg', insert: true, body: 'SELECT 1'));
      final same = base()
        ..triggers.add(DesignTrigger(
            name: 'trg', insert: true, body: 'SELECT 1;'));
      expect(DdlBuilder.alterUnsupported(same, original, 'postgresql'), isNull);
      expect(DdlBuilder.buildAlterStatements(same, original, 'postgresql'), isEmpty);
    });

    test('改名 = 旧名 DROP + 新名 CREATE', () {
      final original = base()
        ..triggers.add(DesignTrigger(
            name: 'trg_old', timing: 'AFTER', insert: true, function: 'f'));
      final target = base()
        ..triggers.add(DesignTrigger(
            name: 'trg_new', timing: 'AFTER', insert: true, function: 'f'));
      final sql =
          DdlBuilder.buildAlterStatements(target, original, 'postgresql').join('\n');
      expect(sql, contains('DROP TRIGGER IF EXISTS "trg_old" ON "public"."t";'));
      expect(sql, contains('CREATE TRIGGER "trg_new"'));
    });

    test('PostgreSQL 禁用触发器补 DISABLE TRIGGER', () {
      final target = base()
        ..triggers.add(DesignTrigger(
            name: 'trg', timing: 'AFTER', insert: true, function: 'f', enable: false));
      final sql = DdlBuilder.buildAlterStatements(target, base(), 'postgresql').join('\n');
      expect(sql, contains('DISABLE TRIGGER "trg";'));
    });

    test('MySQL 多事件在 ALTER 路径同样展开成多条 CREATE', () {
      final target = DesignTable()
        ..name = 't'
        ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true))
        ..triggers.add(DesignTrigger(
            name: 'trg', timing: 'BEFORE', insert: true, update: true, body: 'SET NEW.id = 1'));
      expect(DdlBuilder.alterUnsupported(target, DesignTable()..name = 't', 'mysql'),
          isNull);
      final sql = DdlBuilder
          .buildAlterStatements(target, DesignTable()..name = 't', 'mysql')
          .join('\n');
      expect(sql, contains('CREATE TRIGGER `trg_insert`'));
      expect(sql, contains('CREATE TRIGGER `trg_update`'));
    });

    test('SQL Server 的 DROP 用 schema 限定名、不带 ON', () {
      final original = DesignTable()
        ..name = 't'
        ..schema = 'dbo'
        ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true))
        ..triggers.add(DesignTrigger(
            name: 'trg', timing: 'AFTER', insert: true, body: 'SELECT 1'));
      final sql = DdlBuilder
          .buildAlterStatements(DesignTable()..name = 't'..schema = 'dbo',
              original, 'sqlserver')
          .join('\n');
      expect(sql, contains('DROP TRIGGER IF EXISTS [dbo].[trg];'));
      expect(sql, isNot(contains('DROP TRIGGER IF EXISTS [dbo].[trg] ON')));
    });

    test('规则 / 排除在 PostgreSQL 上放开,换到非 PG 仍按方言阻断', () {
      final original = base()
        ..rules.add(DesignRule(name: 'r', event: 'INSERT', statement: 'SELECT 1'));
      final target = base();
      // PG:删规则是可执行的 ALTER 路径(DROP RULE),不该阻断
      expect(DdlBuilder.alterUnsupported(target, original, 'postgresql'), isNull);
      expect(
          DdlBuilder.buildAlterStatements(target, original, 'postgresql').join('\n'),
          contains('DROP RULE IF EXISTS "r" ON "public"."t";'));
      // 非 PG:库里既读不出也改不了这两类对象,只能拒于保存前
      final mysqlOriginal = DesignTable()
        ..name = 't'
        ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true))
        ..rules.add(DesignRule(name: 'r', event: 'INSERT', statement: 'SELECT 1'));
      expect(
          DdlBuilder.alterUnsupported(
              DesignTable()..name = 't', mysqlOriginal, 'mysql'),
          contains('仅 PostgreSQL 支持'));
    });
  });

  group('触发器反查解析(纯函数)', () {
    test('MySQL 主体剥掉 BEGIN…END 外壳,与生成器往返幂等', () {
      expect(stripMysqlTriggerBeginEnd('BEGIN\n  SET NEW.a = 1;\nEND'),
          'SET NEW.a = 1');
      // 内层 END IF 结尾的不是外层包裹,原样保留
      final nested = 'BEGIN\n  IF NEW.a > 0 THEN\n    SET NEW.b = 1;\n  END IF;\nEND';
      expect(stripMysqlTriggerBeginEnd(nested), isNot(equals('SET NEW.b = 1')));
      expect(stripMysqlTriggerBeginEnd('SET NEW.a = 1'), 'SET NEW.a = 1');
      expect(stripMysqlTriggerBeginEnd('BEGINNING CHECK'), 'BEGINNING CHECK');
      expect(stripMysqlTriggerBeginEnd(''), '');
    });

    test('SQL Server 从完整定义切出 AS 之后的主体', () {
      expect(
          stripSqlServerTriggerBody('CREATE TRIGGER [trg] ON [dbo].[t] AFTER INSERT '
              'AS BEGIN\n  UPDATE audit SET n = n + 1;\nEND'),
          'UPDATE audit SET n = n + 1');
      // 标识符里含 as 字样时不能切错(取最后一个顶层 AS)
      expect(
          stripSqlServerTriggerBody('CREATE TRIGGER [as_trg] ON [dbo].[gas] AFTER INSERT '
              'AS BEGIN\n  SELECT 1;\nEND'),
          'SELECT 1');
      // 没有 AS 时原样返回,不截断丢内容
      expect(stripSqlServerTriggerBody('SELECT 1'), 'SELECT 1');
    });
  });

  group('排除约束(仅 PostgreSQL)', () {
    DesignTable t() => DesignTable()
      ..name = 't'
      ..schema = 'public'
      ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true));

    test('片段按「USING → 元素 → WHERE → 可延迟」顺序输出', () {
      final ex = DesignExclude(
        name: 'ex_rng',
        columns: 'rng WITH &&',
        method: 'gist',
        predicate: "state = 'active'",
        deferrable: 'YES',
        deferred: 'YES',
      );
      expect(
        DdlBuilder.excludeClause(ex, t(), 'postgresql'),
        'CONSTRAINT "ex_rng" EXCLUDE USING gist (rng WITH &&) '
        "WHERE (state = 'active') DEFERRABLE INITIALLY DEFERRED",
      );
    });

    test('方法留空按 gist 兜底;无排除元素不生成子句', () {
      final sql = DdlBuilder.buildCreateTable(
          t()..excludes.add(DesignExclude(name: 'ex', columns: 'id WITH =', method: '')),
          'postgresql');
      expect(sql, contains('CONSTRAINT "ex" EXCLUDE USING gist (id WITH =)'));
      expect(
          DdlBuilder.buildCreateTable(
              t()..excludes.add(DesignExclude(name: 'ex')), 'postgresql'),
          isNot(contains('EXCLUDE')));
    });

    test('无名项的兜底名只取第一个元素的首词(不带 WITH 与空格)', () {
      final sql = DdlBuilder.buildCreateTable(
          t()..excludes.add(DesignExclude(columns: 'lower(id) WITH =', method: 'btree')),
          'postgresql');
      expect(sql,
          contains('CONSTRAINT "ex_t_lower" EXCLUDE USING btree (lower(id) WITH =)'));
    });

    test('未指定字段 / 表达式的排除过不了校验', () {
      final r =
          DdlBuilder.validate(t()..excludes.add(DesignExclude(name: 'ex')),
              typeId: 'postgresql');
      expect(r.ok, isFalse);
      expect(r.error, contains('排除「ex」未指定字段'));
    });

    test('编辑模式:改条件走 DROP + ADD,注释随重建补回且只发一次', () {
      final original = t()
        ..excludes.add(DesignExclude(
            name: 'ex', columns: 'id WITH =', comment: 'C'));
      final target = t()
        ..excludes.add(DesignExclude(
            name: 'ex',
            columns: 'id WITH =',
            predicate: 'id > 0',
            comment: 'C'));
      expect(DdlBuilder.alterUnsupported(target, original, 'postgresql'), isNull);
      final sql =
          DdlBuilder.buildAlterStatements(target, original, 'postgresql').join('\n');
      expect(sql, contains('ALTER TABLE "public"."t" DROP CONSTRAINT "ex";'));
      expect(sql,
          contains('ALTER TABLE "public"."t" ADD CONSTRAINT "ex" EXCLUDE USING gist '
              '(id WITH =) WHERE (id > 0);'));
      // 注释挂在约束对象上,DROP 会一起带走 → 重建路径必须补回
      expect(sql, contains('COMMENT ON CONSTRAINT "ex" ON "public"."t" IS \'C\';'));
      expect(RegExp('COMMENT ON CONSTRAINT').allMatches(sql).length, 1);
    });

    test('编辑模式:只改注释时不重建约束', () {
      final original = t()..excludes.add(DesignExclude(name: 'ex', columns: 'id WITH ='));
      final target = t()
        ..excludes.add(DesignExclude(
            name: 'ex', columns: 'id WITH =', comment: '新注释'));
      final sql =
          DdlBuilder.buildAlterStatements(target, original, 'postgresql').join('\n');
      expect(sql, contains('COMMENT ON CONSTRAINT "ex" ON "public"."t" IS \'新注释\';'));
      expect(sql, isNot(contains('DROP CONSTRAINT')));
      expect(sql, isNot(contains('ADD CONSTRAINT')));
    });
  });

  group('规则(仅 PostgreSQL)', () {
    DesignTable t() => DesignTable()
      ..name = 't'
      ..schema = 'public'
      ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true));

    test('DO ALSO 与限定条件都进语句,主体剥掉尾分号', () {
      expect(
        DdlBuilder.buildCreateRule(
            t(),
            'postgresql',
            DesignRule(
                name: 'r_log',
                event: 'INSERT',
                action: 'ALSO',
                qualification: 'NEW.id > 0',
                statement: 'INSERT INTO log VALUES (1);')),
        'CREATE RULE "r_log" AS ON INSERT TO "public"."t" '
        'WHERE (NEW.id > 0) DO ALSO (INSERT INTO log VALUES (1));',
      );
    });

    test('SELECT 规则固定 DO INSTEAD;行为为 NOTHING 时不生成语句体', () {
      expect(
        DdlBuilder.buildCreateRule(
            t(), 'postgresql',
            DesignRule(name: 'r_s', event: 'SELECT', action: 'ALSO', statement: 'SELECT 1')),
        'CREATE RULE "r_s" AS ON SELECT TO "public"."t" DO INSTEAD (SELECT 1);',
      );
      expect(
        DdlBuilder.buildCreateRule(
            t(), 'postgresql', DesignRule(name: 'r_n', event: 'DELETE', action: 'NOTHING')),
        'CREATE RULE "r_n" AS ON DELETE TO "public"."t" DO NOTHING;',
      );
    });

    test('注释走 COMMENT ON RULE', () {
      final stmts = DdlBuilder.buildStatements(t()
        ..rules.add(DesignRule(
            name: 'r', event: 'INSERT', action: 'NOTHING', comment: '屏蔽插入')),
        'postgresql');
      expect(stmts, contains('COMMENT ON RULE "r" ON "public"."t" IS \'屏蔽插入\';'));
    });

    test('ALSO / INSTEAD 必须有语句,NOTHING 可以没有', () {
      final missing = DdlBuilder.validate(
          t()..rules.add(DesignRule(name: 'r', event: 'INSERT', action: 'INSTEAD')),
          typeId: 'postgresql');
      expect(missing.ok, isFalse);
      expect(missing.error, contains('规则「r」未指定语句'));
      expect(
          DdlBuilder.validate(
              t()..rules.add(DesignRule(name: 'r', event: 'INSERT', action: 'NOTHING')),
              typeId: 'postgresql').ok,
          isTrue);
    });

    test('编辑模式:改动作走 DROP RULE + CREATE RULE,注释跟着补回', () {
      final original = t()
        ..rules.add(DesignRule(
            name: 'r', event: 'INSERT', action: 'ALSO', statement: 'SELECT 1', comment: 'C'));
      final target = t()
        ..rules.add(DesignRule(
            name: 'r', event: 'INSERT', action: 'INSTEAD', statement: 'SELECT 1', comment: 'C'));
      expect(DdlBuilder.alterUnsupported(target, original, 'postgresql'), isNull);
      final sql =
          DdlBuilder.buildAlterStatements(target, original, 'postgresql').join('\n');
      expect(sql, contains('DROP RULE IF EXISTS "r" ON "public"."t";'));
      expect(sql, contains('CREATE RULE "r" AS ON INSERT TO "public"."t" DO INSTEAD (SELECT 1);'));
      expect(sql, contains('COMMENT ON RULE "r" ON "public"."t" IS \'C\';'));
    });

    test('编辑模式:补个换行不算变更;只改注释单发 COMMENT ON RULE', () {
      final original = t()
        ..rules.add(DesignRule(name: 'r', event: 'INSERT', statement: 'SELECT 1;'));
      final same = t()
        ..rules.add(DesignRule(name: 'r', event: 'INSERT', statement: 'SELECT 1'));
      expect(DdlBuilder.buildAlterStatements(same, original, 'postgresql'), isEmpty);

      final commented = t()
        ..rules.add(DesignRule(
            name: 'r', event: 'INSERT', statement: 'SELECT 1', comment: '新'));
      final sql =
          DdlBuilder.buildAlterStatements(commented, original, 'postgresql').join('\n');
      expect(sql, contains('COMMENT ON RULE "r" ON "public"."t" IS \'新\';'));
      expect(sql, isNot(contains('DROP RULE')));
    });
  });

  group('参考工具的网格列:唯一键字段项 / 检查无继承 / 排除元素 / 规则启用', () {
    DesignTable t() => DesignTable()
      ..name = 't'
      ..schema = 'public'
      ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true))
      ..columns.add(DesignColumn(name: 'c', type: 'varchar', length: '32'));

    String sql(DesignTable d, [String type = 'postgresql']) =>
        DdlBuilder.buildStatements(d, type).join('\n');

    test('唯一键「字段」弹窗的逐字段选项进 UNIQUE 括号', () {
      final d = t()
        ..uniqueKeys.add(DesignUniqueKey(name: 'uq_c', columns: 'c')
          ..columnItems.add(DesignIndexColumn(
              name: 'c', collationSchema: 'pg_catalog', collation: 'C', order: 'DESC')));
      expect(sql(d),
          contains('CONSTRAINT "uq_c" UNIQUE ("c" COLLATE "pg_catalog"."C" DESC)'));
      // 非 PG 只出裸字段名:选项是 PG 语法,拼进 MySQL 就是语法错
      final mysql = t()
        ..uniqueKeys.add(DesignUniqueKey(name: 'uq_c', columns: 'c')
          ..columnItems.add(DesignIndexColumn(name: 'c', order: 'DESC')));
      expect(sql(mysql, 'mysql'), contains('CONSTRAINT `uq_c` UNIQUE (`c`)'));
    });

    test('唯一键字段项确认后回写逗号串;copy 深拷贝', () {
      final uk = DesignUniqueKey(name: 'uq', columns: 'id')
        ..columnItems.addAll([
          DesignIndexColumn(name: 'c', order: 'DESC'),
          DesignIndexColumn(name: 'id'),
        ]);
      uk.applyColumnItems();
      expect(uk.columns, 'c, id');
      expect(uk.columnList, ['c', 'id']);
      final dup = uk.copy();
      dup.columnItems.first.order = 'ASC';
      expect(uk.columnItems.first.order, 'DESC');
    });

    test('检查「无继承」落 NO INHERIT(仅 PG),非 PG 由 createUnsupported 拦住', () {
      final d = t()
        ..checks.add(DesignCheck(name: 'ck', expression: 'id > 0', noInherit: true));
      expect(sql(d), contains('CONSTRAINT "ck" CHECK (id > 0) NO INHERIT'));
      expect(sql(d, 'mysql'), isNot(contains('NO INHERIT')));
      expect(DdlBuilder.createUnsupported(d, 'mysql'),
          contains('「无继承」(NO INHERIT)仅 PostgreSQL 支持'));
    });

    test('检查编辑模式:只改「无继承」也算变更,走 DROP + ADD 重建', () {
      final original = t()..checks.add(DesignCheck(name: 'ck', expression: 'id > 0'));
      final target = t()
        ..checks.add(DesignCheck(name: 'ck', expression: 'id > 0', noInherit: true));
      final s = DdlBuilder.buildAlterStatements(target, original, 'postgresql').join('\n');
      expect(s, contains('ALTER TABLE "public"."t" DROP CONSTRAINT "ck";'));
      expect(s,
          contains('ADD CONSTRAINT "ck" CHECK (id > 0) NO INHERIT;'));
    });

    test('排除「元素」弹窗逐项输出 <index_elem> WITH <运算符>', () {
      final d = t()
        ..excludes.add(DesignExclude(name: 'ex', method: 'gist', columns: 'c WITH =')
          ..columnItems.add(DesignIndexColumn(
            name: 'c',
            opClass: 'text_ops',
            order: 'DESC',
            operatorSchema: 'pg_catalog',
            operatorName: '=',
          )));
      expect(
        sql(d),
        contains(
            'CONSTRAINT "ex" EXCLUDE USING gist ("c" "text_ops" DESC WITH "pg_catalog".=)'),
      );
    });

    test('元素项回写 `字段 WITH 运算符`;勾了字段没填运算符过不了校验', () {
      final ex = DesignExclude(name: 'ex', method: 'btree')
        ..columnItems.addAll([
          DesignIndexColumn(name: 'id', operatorName: '='),
          DesignIndexColumn(name: 'c', operatorName: '&&'),
        ]);
      ex.applyColumnItems();
      expect(ex.columns, 'id WITH =, c WITH &&');

      final v = DdlBuilder.validate(
        t()
          ..excludes.add(DesignExclude(name: 'ex', method: 'btree')
            ..columnItems.add(DesignIndexColumn(name: 'id'))),
        typeId: 'postgresql',
      );
      expect(v.ok, isFalse);
      expect(v.error, contains('排除「ex」的元素「id」未指定运算符'));
    });

    test('规则「启用」关掉:新建补 DISABLE RULE,编辑只切开关不重建', () {
      final target = t()
        ..rules.add(DesignRule(
            name: 'r',
            event: 'INSERT',
            action: 'ALSO',
            statement: 'SELECT 1',
            enable: false));
      final s = sql(target);
      expect(
          s, contains('CREATE RULE "r" AS ON INSERT TO "public"."t" DO ALSO (SELECT 1);'));
      expect(s, contains('ALTER TABLE "public"."t" DISABLE RULE "r";'));

      final original = t()
        ..rules.add(DesignRule(
            name: 'r', event: 'INSERT', action: 'ALSO', statement: 'SELECT 1'));
      expect(DdlBuilder.buildAlterStatements(target, original, 'postgresql'),
          ['ALTER TABLE "public"."t" DISABLE RULE "r";']);
      // 反向:库里是关的,勾回启用
      expect(
        DdlBuilder.buildAlterStatements(original, target, 'postgresql'),
        ['ALTER TABLE "public"."t" ENABLE RULE "r";']);
    });
  });

  group('表存储参数(选项标签)', () {
    DesignTable t() => DesignTable()
      ..name = 't'
      ..schema = 'public'
      ..columns.add(DesignColumn(name: 'id', type: 'int', primaryKey: true));

    test('WITH 按清单顺序输出,fillfactor 在最前', () {
      final d = t()
        ..setStorageOption('parallel_workers', '2')
        ..setStorageOption('autovacuum_enabled', 'false')
        ..fillFactor = '90';
      expect(
        DdlBuilder.buildCreateTable(d, 'postgresql'),
        endsWith('WITH (fillfactor = 90, autovacuum_enabled = false, parallel_workers = 2);'),
      );
    });

    test('值清空即从清单里移除(不留空子句)', () {
      final d = t()..fillFactor = '90';
      expect(d.storageOptions.containsKey('fillfactor'), isTrue);
      d.fillFactor = '';
      expect(d.storageOptions, isEmpty);
      expect(DdlBuilder.buildCreateTable(d, 'postgresql'), isNot(contains('WITH (')));
    });

    test('SQL Server 只认 FILLFACTOR,其它参数由 createUnsupported 拦住', () {
      final ff = t()..fillFactor = '80';
      expect(DdlBuilder.buildCreateTable(ff, 'sqlserver'),
          contains('WITH (FILLFACTOR = 80)'));
      expect(DdlBuilder.createUnsupported(ff, 'sqlserver'), isNull);

      final av = t()..setStorageOption('autovacuum_enabled', 'false');
      expect(DdlBuilder.createUnsupported(av, 'sqlserver'), contains('autovacuum_enabled'));
      expect(DdlBuilder.createUnsupported(av, 'mysql'), contains('autovacuum_enabled'));
    });

    test('编辑模式:改参数发 SET、删参数发 RESET、改表空间发 SET TABLESPACE', () {
      final original = t()
        ..fillFactor = '90'
        ..setStorageOption('parallel_workers', '4')
        ..tablespace = 'ts1';
      final target = t()..fillFactor = '80'..tablespace = '';
      expect(DdlBuilder.alterUnsupported(target, original, 'postgresql'), isNull);
      final sql =
          DdlBuilder.buildAlterStatements(target, original, 'postgresql').join('\n');
      expect(sql, contains('ALTER TABLE "public"."t" SET (fillfactor = 80);'));
      expect(sql, contains('ALTER TABLE "public"."t" RESET (parallel_workers);'));
      expect(sql, contains('ALTER TABLE "public"."t" SET TABLESPACE "pg_default";'));
    });

    test('编辑模式:所有者走 OWNER TO,不记录 / 继承 / 集群没有 ALTER → 阻断', () {
      for (final entry in <String, void Function(DesignTable)>{
        '不记录': (d) => d.unlogged = true,
        '继承': (d) => d.inherits = 'p',
        '集群': (d) => d.cluster = 'i',
      }.entries) {
        final target = t();
        entry.value(target);
        expect(DdlBuilder.alterUnsupported(target, t(), 'postgresql'),
            contains(entry.key), reason: entry.key);
      }
      final owner = t()..owner = 'app';
      expect(DdlBuilder.alterUnsupported(owner, t(), 'postgresql'), isNull);
      expect(
          DdlBuilder.buildAlterStatements(owner, t(), 'postgresql').join('\n'),
          contains('ALTER TABLE "public"."t" OWNER TO "app";'));
    });
  });

  group('排除 / 规则反查解析(纯函数)', () {
    test('EXCLUDE 定义拆出方法 / 元素 / WHERE 谓词,谓词剥掉服务端的外层括号', () {
      final p = parsePgExcludeDef(
          "EXCLUDE USING gist (lower(name) WITH =, int4range(a, b) WITH &&) "
          "WHERE ((state = 'active'::text))");
      expect(p.method, 'gist');
      expect(p.elements, 'lower(name) WITH =, int4range(a, b) WITH &&');
      expect(p.predicate, "state = 'active'::text");
    });

    test('无 WHERE / 无 USING 的形态都能读,元素里的括号不误伤', () {
      final plain = parsePgExcludeDef('EXCLUDE USING btree (col WITH =)');
      expect(plain.method, 'btree');
      expect(plain.elements, 'col WITH =');
      expect(plain.predicate, '');
      // 没有 USING(旧服务端的输出形态):方法留空,由生成器按 gist 兜底
      final noAm = parsePgExcludeDef('EXCLUDE (f(x) WITH =)');
      expect(noAm.method, '');
      expect(noAm.elements, 'f(x) WITH =');
    });

    test('CREATE RULE 定义回填事件 / 限定条件 / 动作 / 主体', () {
      final r = parsePgRuleDef(
        'r_log',
        'CREATE RULE r_log AS ON INSERT TO public.t WHERE (new.id > 0) '
        'DO ALSO (INSERT INTO log VALUES (1));',
        comment: 'C',
      );
      expect(r, isNotNull);
      expect(r!.name, 'r_log');
      expect(r.event, 'INSERT');
      expect(r.qualification, 'new.id > 0');
      expect(r.action, 'ALSO');
      expect(r.statement, 'INSERT INTO log VALUES (1)');
      expect(r.comment, 'C');
    });

    test('DO NOTHING 没有语句体;认不出结构返回 null(不猜)', () {
      final nothing = parsePgRuleDef('r', 'CREATE RULE r AS ON DELETE TO public.t DO NOTHING');
      expect(nothing, isNotNull);
      expect(nothing!.action, 'NOTHING');
      expect(nothing.statement, '');
      expect(parsePgRuleDef('r', 'SELECT 1'), isNull);
      expect(parsePgRuleDef('', 'CREATE RULE r AS ON INSERT TO t DO ALSO (SELECT 1)'), isNull);
    });

    test('生成 → 反查 → 再生成:同一条规则往返幂等', () {
      final d = DesignTable()
        ..name = 't'
        ..schema = 'public'
        ..columns.add(DesignColumn(name: 'id', type: 'int'));
      final sql = DdlBuilder.buildCreateRule(
          d,
          'postgresql',
          DesignRule(
              name: 'r', event: 'UPDATE', action: 'ALSO',
              qualification: 'NEW.a <> OLD.a', statement: 'NOTIFY c'));
      final parsed = parsePgRuleDef('r', sql)!;
      expect(DdlBuilder.buildCreateRule(d, 'postgresql', parsed), sql);
    });

    test('反查带回的启用状态:ev_enabled = d 时网格「启用」不该是勾着的', () {
      const def = 'CREATE RULE r AS ON INSERT TO public.t DO ALSO (NOTIFY c);';
      expect(parsePgRuleDef('r', def)!.enable, isTrue);
      expect(parsePgRuleDef('r', def, enabled: false)!.enable, isFalse);
    });
  });
}
