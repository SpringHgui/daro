import 'package:daro/data/table_design.dart';
import 'package:flutter_test/flutter_test.dart';

/// 「设计表」保存路径的差异生成:`buildAlterStatements` / `hasChanges` /
/// `alterPreview` / `alterUnsupported`。断言用完整语句文本,方言语法一旦
/// 漂移(引号、子句顺序、约束命名)即可被发现。
void main() {
  const pg = 'postgresql';
  const my = 'mysql';
  const ss = 'sqlserver';

  /// 反查得到的典型原表:`public.t(id int8 PK NOT NULL, name varchar(32))`
  DesignTable base({
    String name = 't',
    String? schema = 'public',
    String comment = '',
    String pkName = '',
  }) =>
      DesignTable()
        ..name = name
        ..schema = schema
        ..tableComment = comment
        ..pkName = pkName
        ..columns.addAll([
          DesignColumn(
              name: 'id', type: 'int8', length: '64', notNull: true, primaryKey: true),
          DesignColumn(name: 'name', type: 'varchar', length: '32'),
        ]);

  DesignTable twoCols({String? schema = 'public'}) => DesignTable()
    ..name = 't'
    ..schema = schema
    ..columns.addAll([
      DesignColumn(name: 'a', type: 'int'),
      DesignColumn(name: 'b', type: 'int'),
    ]);

  /// 以快照为目标(未手改的列与原表属性相同),便于逐例只改一处
  DesignTable target(DesignTable original) => original.snapshot();

  group('无差异', () {
    test('快照相等 → 空列表 + hasChanges false + 无阻断', () {
      final original = base(comment: '订单')
        ..indexes.add(DesignIndex(name: 'idx_name', columns: 'name'));
      final t = target(original);
      expect(DdlBuilder.buildAlterStatements(t, original, pg), isEmpty);
      expect(DdlBuilder.hasChanges(t, original, pg), isFalse);
      expect(DdlBuilder.alterUnsupported(t, original, pg), isNull);
      expect(DdlBuilder.alterPreview(t, original, pg), isEmpty);
    });

    test('仅大小写 / 空格差异不产生语句', () {
      final original = base();
      final t = target(original);
      t.columns.last.name = '  NAME ';
      expect(DdlBuilder.buildAlterStatements(t, original, pg), isEmpty);
    });
  });

  group('PostgreSQL', () {
    test('仅表注释变更 → 只有 COMMENT ON TABLE', () {
      final original = base();
      final t = target(original)..tableComment = '订单';
      expect(DdlBuilder.buildAlterStatements(t, original, pg),
          ['COMMENT ON TABLE "public"."t" IS \'订单\';']);
    });

    test('加列:列定义与注释分两条(新列注释不得静默丢失)', () {
      final original = base();
      final t = target(original)
        ..columns.add(DesignColumn(
            name: 'note', type: 'varchar', length: '64', comment: '备注'));
      expect(DdlBuilder.buildAlterStatements(t, original, pg), [
        'ALTER TABLE "public"."t" ADD COLUMN "note" varchar(64);',
        'COMMENT ON COLUMN "public"."t"."note" IS \'备注\';',
      ]);
    });

    test('删列 → DROP COLUMN(不加 CASCADE,交由数据库报错)', () {
      final original = base();
      final t = target(original)..columns.removeWhere((c) => c.name == 'name');
      expect(DdlBuilder.buildAlterStatements(t, original, pg),
          ['ALTER TABLE "public"."t" DROP COLUMN "name";']);
    });

    test('改类型 / 可空 / 默认值分条 ALTER COLUMN', () {
      final original = base();
      final t = target(original);
      t.columns.last
        ..type = 'text'
        ..length = ''
        ..notNull = true
        ..defaultValue = 'x';
      expect(DdlBuilder.buildAlterStatements(t, original, pg), [
        'ALTER TABLE "public"."t" ALTER COLUMN "name" TYPE text USING "name"::text;',
        'ALTER TABLE "public"."t" ALTER COLUMN "name" SET NOT NULL;',
        'ALTER TABLE "public"."t" ALTER COLUMN "name" SET DEFAULT \'x\';',
      ]);
    });

    test('默认值清空 → DROP DEFAULT;数值不加引号', () {
      final original = base();
      original.columns.last.defaultValue = '0';
      final t = target(original);
      t.columns.last.defaultValue = '  ';
      expect(DdlBuilder.buildAlterStatements(t, original, pg),
          ['ALTER TABLE "public"."t" ALTER COLUMN "name" DROP DEFAULT;']);

      final o2 = base();
      final t2 = target(o2);
      t2.columns.last.defaultValue = '12.5';
      expect(DdlBuilder.buildAlterStatements(t2, o2, pg),
          ['ALTER TABLE "public"."t" ALTER COLUMN "name" SET DEFAULT 12.5;']);
    });

    test('取消非空 → DROP NOT NULL;排序规则变更带 COLLATE 与 USING', () {
      final original = base();
      final t = target(original);
      t.columns.first.notNull = false;
      expect(DdlBuilder.buildAlterStatements(t, original, pg),
          ['ALTER TABLE "public"."t" ALTER COLUMN "id" DROP NOT NULL;']);

      final o2 = base();
      final t2 = target(o2);
      t2.columns.last.collation = 'C';
      expect(DdlBuilder.buildAlterStatements(t2, o2, pg), [
        'ALTER TABLE "public"."t" ALTER COLUMN "name" TYPE varchar(32) '
            'COLLATE "C" USING "name"::varchar(32);',
      ]);
    });

    test('列重命名先于该列的其它变更', () {
      final original = base();
      final t = target(original);
      t.columns.last
        ..name = 'title'
        ..type = 'text'
        ..length = '';
      final stmts = DdlBuilder.buildAlterStatements(t, original, pg);
      expect(stmts, [
        'ALTER TABLE "public"."t" RENAME COLUMN "name" TO "title";',
        'ALTER TABLE "public"."t" ALTER COLUMN "title" TYPE text USING "title"::text;',
      ]);
      expect(stmts.indexWhere((s) => s.contains('RENAME COLUMN')),
          lessThan(stmts.indexWhere((s) => s.contains('ALTER COLUMN'))));
    });

    test('主键重建沿用 original.pkName;名字缺失时按 <表名>_pkey 兜底', () {
      // 反查到的真名随快照带过来,DROP / ADD 用同一真名
      final original = base(pkName: 'orders_pk');
      final t = target(original);
      t.columns.last.primaryKey = true;
      expect(DdlBuilder.buildAlterStatements(t, original, pg), [
        'ALTER TABLE "public"."t" DROP CONSTRAINT "orders_pk";',
        'ALTER TABLE "public"."t" ADD CONSTRAINT "orders_pk" PRIMARY KEY ("id", "name");',
      ]);

      // 两侧都无名字 → 按 PG 约定名 <表名>_pkey
      final o2 = base();
      final t2 = target(o2);
      t2.columns.first.primaryKey = false;
      expect(DdlBuilder.buildAlterStatements(t2, o2, pg),
          ['ALTER TABLE "public"."t" DROP CONSTRAINT "t_pkey";']);

      // 表重命名后新增的约束名跟随新表名
      final o3 = base();
      final t3 = target(o3);
      t3
        ..name = 't2'
        ..pkName = '';
      t3.columns.first.primaryKey = false;
      t3.columns.last.primaryKey = true;
      expect(DdlBuilder.buildAlterStatements(t3, o3, pg), [
        'ALTER TABLE "public"."t" RENAME TO "t2";',
        'ALTER TABLE "public"."t2" DROP CONSTRAINT "t_pkey";',
        'ALTER TABLE "public"."t2" ADD CONSTRAINT "t2_pkey" PRIMARY KEY ("name");',
      ]);
    });

    test('索引变更 → DROP + CREATE;删除项只 DROP', () {
      final original = base()
        ..indexes.add(DesignIndex(name: 'idx_name', columns: 'name', method: 'btree'));
      final t = target(original);
      t.indexes.first
        ..columns = 'id'
        ..method = 'hash';
      expect(DdlBuilder.buildAlterStatements(t, original, pg), [
        'DROP INDEX IF EXISTS "public"."idx_name";',
        'CREATE INDEX "idx_name" ON "public"."t" USING hash ("id");',
      ]);

      final t2 = target(original)..indexes.clear();
      expect(DdlBuilder.buildAlterStatements(t2, original, pg),
          ['DROP INDEX IF EXISTS "public"."idx_name";']);
    });

    test('索引只改「字段」项选项(排序顺序 / Nulls)→ 同样重建索引', () {
      final original = base()
        ..indexes.add(DesignIndex(name: 'idx_name', columns: 'name', method: 'btree'));
      final t = target(original);
      // 逗号串保持不变,只在弹窗里给字段加了选项
      t.indexes.first.columnItems
          .add(DesignIndexColumn(name: 'name', order: 'DESC', nullsOrder: 'FIRST'));
      expect(DdlBuilder.buildAlterStatements(t, original, pg), [
        'DROP INDEX IF EXISTS "public"."idx_name";',
        'CREATE INDEX "idx_name" ON "public"."t" USING btree '
            '("name" DESC NULLS FIRST);',
      ]);

      // 快照为深拷贝:改目标不影响原表基线
      expect(original.indexes.first.columnItems, isEmpty);
      // 未动字段项时不产生语句
      expect(DdlBuilder.buildAlterStatements(target(original), original, pg), isEmpty);
      expect(DdlBuilder.hasChanges(target(original), original, pg), isFalse);
    });

    test('外键注释变更 → 只补 COMMENT ON CONSTRAINT(不重建约束)', () {
      final original = base()
        ..foreignKeys.add(DesignForeignKey(
            name: 'fk_user', columns: 'name', refTable: 'users', refColumns: 'id'));
      // 注释未动 → 无任何语句(约束签名不含注释,注释单独比对)
      expect(DdlBuilder.buildAlterStatements(target(original), original, pg), isEmpty);

      final t = target(original);
      t.foreignKeys.first.comment = '引用用户';
      expect(DdlBuilder.buildAlterStatements(t, original, pg),
          ['COMMENT ON CONSTRAINT "fk_user" ON "public"."t" IS \'引用用户\';']);

      // 清空已有注释 → 显式置 NULL,不静默跳过
      final cleared = target(original..foreignKeys.first.comment = '旧注释');
      cleared.foreignKeys.first.comment = '';
      expect(
        DdlBuilder.buildAlterStatements(cleared, original, pg),
        contains('COMMENT ON CONSTRAINT "fk_user" ON "public"."t" IS NULL;'),
      );
    });

    test('索引 / 唯一键 / 检查 / 排除 注释变更 → 只补 COMMENT ON(不重建对象)', () {
      final original = base()
        ..indexes.add(
            DesignIndex(name: 'idx_name', columns: 'name', method: 'btree'))
        ..uniqueKeys.add(DesignUniqueKey(name: 'uq_name', columns: 'name'))
        ..checks.add(
            DesignCheck(name: 'ck_name', expression: 'char_length(name) > 1'))
        ..excludes.add(
            DesignExclude(name: 'ex_name', columns: 'name with =', method: 'btree'));
      // 注释未动 → 零语句(各项签名都不含注释)
      expect(DdlBuilder.buildAlterStatements(target(original), original, pg), isEmpty);
      expect(DdlBuilder.alterUnsupported(target(original), original, pg), isNull);

      final t = target(original);
      t.indexes.first.comment = '名字索引';
      t.uniqueKeys.first.comment = '名字唯一';
      t.checks.first.comment = '名非空';
      t.excludes.first.comment = '排重';
      // 排除约束的注释由「排除约束」那一段自己补(它可能整条重建,注释要跟着走),
      // 故排在索引 / 唯一键 / 检查之前——后面那段是纯注释变更的共用出口
      expect(DdlBuilder.buildAlterStatements(t, original, pg), [
        'COMMENT ON CONSTRAINT "ex_name" ON "public"."t" IS \'排重\';',
        'COMMENT ON INDEX "public"."idx_name" IS \'名字索引\';',
        'COMMENT ON CONSTRAINT "uq_name" ON "public"."t" IS \'名字唯一\';',
        'COMMENT ON CONSTRAINT "ck_name" ON "public"."t" IS \'名非空\';',
      ]);
    });

    test('MySQL 索引注释内联进 CREATE INDEX(只改注释也重建);SQL Server 阻断', () {
      final original = base(schema: null)
        ..indexes.add(DesignIndex(name: 'idx_name', columns: 'name'));
      final t = target(original)..indexes.first.comment = '名字索引';
      expect(DdlBuilder.buildAlterStatements(t, original, my), [
        'ALTER TABLE `t` DROP INDEX `idx_name`;',
        "CREATE INDEX `idx_name` ON `t` (`name`) COMMENT '名字索引';",
      ]);
      expect(DdlBuilder.alterUnsupported(t, original, my), isNull);

      // SQL Server 无索引层次属性 → 给阻断提示而非静默丢弃
      expect(DdlBuilder.alterUnsupported(t, original, ss),
          contains('索引注释暂仅支持'));
    });

    test('非 PG 的唯一键 / 检查注释变更 → alterUnsupported 阻断', () {
      final original = base(schema: null)
        ..uniqueKeys.add(DesignUniqueKey(name: 'uq_name', columns: 'name'));
      final t = target(original)..uniqueKeys.first.comment = '唯一名';
      expect(DdlBuilder.alterUnsupported(t, original, my),
          contains('唯一键 / 检查约束注释暂仅支持 PostgreSQL'));
      // 未动注释时不误阻
      expect(DdlBuilder.alterUnsupported(target(original), original, my), isNull);
      expect(DdlBuilder.alterUnsupported(target(original), original, ss), isNull);
    });

    test('唯一键 / 外键 / 检查:按名配对 DROP + ADD', () {
      final original = base()
        ..uniqueKeys.add(DesignUniqueKey(name: 'uq_name', columns: 'name'))
        ..foreignKeys.add(DesignForeignKey(
            name: 'fk_user', columns: 'name', refTable: 'users', refColumns: 'id'))
        ..checks.add(DesignCheck(name: 'ck_name', expression: 'char_length(name) > 1'));
      final t = target(original);
      t.uniqueKeys.first.columns = 'id';
      t.foreignKeys.first.onDelete = 'CASCADE';
      t.checks.first.expression = 'char_length(name) > 0';
      expect(DdlBuilder.buildAlterStatements(t, original, pg), [
        'ALTER TABLE "public"."t" DROP CONSTRAINT "uq_name";',
        'ALTER TABLE "public"."t" DROP CONSTRAINT "fk_user";',
        'ALTER TABLE "public"."t" DROP CONSTRAINT "ck_name";',
        'ALTER TABLE "public"."t" ADD CONSTRAINT "uq_name" UNIQUE ("id");',
        'ALTER TABLE "public"."t" ADD CONSTRAINT "fk_user" FOREIGN KEY ("name") '
            'REFERENCES "public"."users" ("id") ON DELETE CASCADE;',
        'ALTER TABLE "public"."t" ADD CONSTRAINT "ck_name" '
            'CHECK (char_length(name) > 0);',
      ]);
    });

    test('IDENTITY 序列选项变更 → DROP IDENTITY 后重建', () {
      final original = base();
      original.columns.first
        ..identityMode = 'BY DEFAULT'
        ..identityIncrement = '1';
      final t = target(original);
      t.columns.first.identityIncrement = '2';
      expect(DdlBuilder.buildAlterStatements(t, original, pg), [
        'ALTER TABLE "public"."t" ALTER COLUMN "id" DROP IDENTITY IF EXISTS;',
        'ALTER TABLE "public"."t" ALTER COLUMN "id" ADD '
            'GENERATED BY DEFAULT AS IDENTITY (INCREMENT BY 2);',
      ]);
    });

    test('表重命名为首条语句,其后语句使用新表名', () {
      final original = base();
      final t = target(original);
      t.name = 't2';
      t.columns.last.notNull = true;
      expect(DdlBuilder.buildAlterStatements(t, original, pg), [
        'ALTER TABLE "public"."t" RENAME TO "t2";',
        'ALTER TABLE "public"."t2" ALTER COLUMN "name" SET NOT NULL;',
      ]);
    });
  });

  group('MySQL', () {
    test('重命名用 CHANGE COLUMN 并携带完整列定义', () {
      final original = base(schema: null);
      final t = target(original);
      t.columns.last
        ..name = 'title'
        ..length = '50';
      expect(DdlBuilder.buildAlterStatements(t, original, my),
          ['ALTER TABLE `t` CHANGE COLUMN `name` `title` varchar(50);']);
    });

    test('自增与列注释内联进 MODIFY COLUMN', () {
      final original = base(schema: null);
      final t = target(original);
      t.columns.first
        ..autoIncrement = true
        ..comment = '主键';
      expect(DdlBuilder.buildAlterStatements(t, original, my), [
        'ALTER TABLE `t` MODIFY COLUMN `id` int8 NOT NULL '
            'AUTO_INCREMENT COMMENT \'主键\';'
      ]);
    });

    test('主键动作与列动作合并为单条 ALTER TABLE', () {
      final original = base(schema: null);
      final t = target(original);
      t.columns.first.primaryKey = false;
      t.columns.last.primaryKey = true;
      expect(DdlBuilder.buildAlterStatements(t, original, my),
          ['ALTER TABLE `t` DROP PRIMARY KEY, ADD PRIMARY KEY (`name`);']);
    });

    test('表注释走 COMMENT =;首列新增带 FIRST,后续列补 AFTER', () {
      final original = base(schema: null);
      final t = target(original)..tableComment = '订单';
      t.columns.insert(0, DesignColumn(name: 'x', type: 'int'));
      final stmts = DdlBuilder.buildAlterStatements(t, original, my);
      expect(stmts, hasLength(1));
      expect(
        stmts.single,
        'ALTER TABLE `t` ADD COLUMN `x` int FIRST, '
        'MODIFY COLUMN `id` int8 NOT NULL AFTER `x`, '
        'MODIFY COLUMN `name` varchar(32) AFTER `id`, '
        'COMMENT = \'订单\';',
      );
    });

    test('重排列序生成 MODIFY COLUMN ... FIRST / AFTER', () {
      final original = twoCols(schema: null);
      final t = target(original);
      t.columns
        ..removeAt(0)
        ..add(DesignColumn(name: 'a', type: 'int'));
      expect(DdlBuilder.buildAlterStatements(t, original, my).single,
          'ALTER TABLE `t` MODIFY COLUMN `b` int FIRST, '
          'MODIFY COLUMN `a` int AFTER `b`;');
    });

    test('表重命名用 RENAME TABLE,限定名忽略模式', () {
      final original = base(schema: 'testdb');
      final t = target(original)..name = 't2';
      expect(DdlBuilder.buildAlterStatements(t, original, my),
          ['RENAME TABLE `t` TO `t2`;']);
    });
  });

  group('SQL Server', () {
    test('类型与可空合并为一条 ALTER COLUMN', () {
      final original = base(schema: 'dbo');
      original.columns.last
        ..type = 'nvarchar'
        ..length = '50';
      final t = target(original);
      t.columns.last.length = '100';
      expect(DdlBuilder.buildAlterStatements(t, original, ss),
          ['ALTER TABLE [dbo].[t] ALTER COLUMN [name] nvarchar(100) NULL;']);
    });

    test('默认值变更:先按 OBJECT_ID 反查真名 DROP,再 ADD CONSTRAINT', () {
      final original = base(schema: 'dbo');
      final t = target(original);
      t.columns.last.defaultValue = 'abc';
      final stmts = DdlBuilder.buildAlterStatements(t, original, ss);
      expect(stmts, hasLength(2));
      expect(stmts[0], contains('OBJECT_ID(N\'dbo.t\')'));
      expect(stmts[0], contains("c.name = N'name'"));
      expect(stmts[0], contains("DROP CONSTRAINT [' + @dc + N']"));
      expect(stmts[1],
          'ALTER TABLE [dbo].[t] ADD CONSTRAINT [DF_t_name] DEFAULT \'abc\' FOR [name];');
    });

    test('表 / 列重命名用 sp_rename 的 OBJECT 与 COLUMN 形式', () {
      final original = base(schema: 'dbo');
      final t = target(original);
      t.name = 't2';
      t.columns.last.name = 'title';
      expect(DdlBuilder.buildAlterStatements(t, original, ss), [
        'EXEC sp_rename \'dbo.t\', \'t2\', \'OBJECT\';',
        'EXEC sp_rename \'dbo.t2.name\', \'title\', \'COLUMN\';',
      ]);
    });

    test('注释按属性有无选 add / update / drop extendedproperty', () {
      final original = base(schema: 'dbo');
      final added = target(original);
      added.columns.last.comment = '备注';
      expect(DdlBuilder.buildAlterStatements(added, original, ss), [
        'EXEC sp_addextendedproperty \'MS_Description\', \'备注\', '
            '\'SCHEMA\', \'dbo\', \'TABLE\', \'t\', \'COLUMN\', \'name\';',
      ]);

      final withComment = base(schema: 'dbo', comment: '旧注');
      final upd = target(withComment);
      upd.tableComment = '新注';
      expect(DdlBuilder.buildAlterStatements(upd, withComment, ss), [
        'EXEC sp_updateextendedproperty \'MS_Description\', \'新注\', '
            '\'SCHEMA\', \'dbo\', \'TABLE\', \'t\';',
      ]);

      final del = target(withComment);
      del.tableComment = '';
      expect(DdlBuilder.buildAlterStatements(del, withComment, ss), [
        'EXEC sp_dropextendedproperty \'MS_Description\', '
            '\'SCHEMA\', \'dbo\', \'TABLE\', \'t\';',
      ]);
    });

    test('主键重建使用反查到的约束名', () {
      final original = base(schema: 'dbo', pkName: 'PK_t');
      final t = target(original);
      t.columns.last.primaryKey = true;
      expect(DdlBuilder.buildAlterStatements(t, original, ss), [
        'ALTER TABLE [dbo].[t] DROP CONSTRAINT [PK_t];',
        'ALTER TABLE [dbo].[t] ADD CONSTRAINT [PK_t] PRIMARY KEY ([id], [name]);',
      ]);
    });
  });

  group('alterUnsupported 阻断不可 ALTER 的变更', () {
    test('不支持编辑的方言', () {
      final original = base();
      expect(
        DdlBuilder.alterUnsupported(target(original), original, 'sqlite'),
        '当前数据库类型不支持编辑已有表结构',
      );
    });

    test('触发器三方言放行;规则 / 排除约束仅 PG 放行', () {
      // 触发器是「整条重建」(DROP + CREATE,见 buildAlterStatements 3b),
      // 三方言都有语法且驱动能反查回基线 → 不该再拦
      final original = base()..triggers.add(DesignTrigger(name: 'trg'));
      final changed = target(original)..triggers.add(DesignTrigger(name: 'trg2'));
      expect(DdlBuilder.alterUnsupported(changed, original, pg), isNull);
      final mysqlOriginal = base(schema: null)
        ..triggers.add(DesignTrigger(name: 'trg'));
      expect(
        DdlBuilder.alterUnsupported(
            target(mysqlOriginal)..triggers.add(DesignTrigger(name: 'trg2')),
            mysqlOriginal,
            my),
        isNull,
      );

      // 规则与排除约束只有 PG 有语法(其余方言连反查都读不回来,基线恒空,
      // 放过去就是静默丢弃)
      final rule =
          target(original)..rules.add(DesignRule(name: 'r1', statement: 'NOTHING'));
      expect(DdlBuilder.alterUnsupported(rule, original, pg), isNull);
      final mysqlRule = target(base(schema: null))
        ..rules.add(DesignRule(name: 'r1', statement: 'NOTHING'));
      expect(
        DdlBuilder.alterUnsupported(
            mysqlRule, base(schema: null), my),
        '规则 / 排除约束仅 PostgreSQL 支持,当前数据库类型无法修改',
      );
      final ex = target(original)..excludes.add(DesignExclude(columns: 'a WITH ='));
      expect(DdlBuilder.alterUnsupported(ex, original, pg), isNull);
      final ssExclude = target(base(schema: null))
        ..excludes.add(DesignExclude(columns: 'a WITH ='));
      expect(
        DdlBuilder.alterUnsupported(
            ssExclude, base(schema: null), ss),
        '规则 / 排除约束仅 PostgreSQL 支持,当前数据库类型无法修改',
      );
      // 原样保留(反查后不动)不阻断
      expect(DdlBuilder.alterUnsupported(target(original), original, pg), isNull);
    });

    test('表空间 / 存储参数:PG 生成 ALTER,非 PG 阻断', () {
      final original = base()..tablespace = 'ts_fast';
      final t = target(original)..tablespace = '';
      // PG 有 ALTER TABLE … SET TABLESPACE(清空即回 pg_default),不再是拦口
      expect(DdlBuilder.alterUnsupported(t, original, pg), isNull);
      expect(DdlBuilder.buildAlterStatements(t, original, pg),
          ['ALTER TABLE "public"."t" SET TABLESPACE "pg_default";']);

      // SQL Server 改填充因子要重建表、MySQL 无表级 reloption → 只能拦
      final mysqlOriginal = base(schema: null)..tablespace = 'ts_fast';
      expect(
        DdlBuilder.alterUnsupported(
            target(mysqlOriginal)..tablespace = '', mysqlOriginal, my),
        '表空间 / 存储参数仅 PostgreSQL 支持在「设计表」中修改',
      );
    });

    test('列序调整:PG / SQL Server 阻断,MySQL 放行', () {
      final original = twoCols();
      final t = target(original);
      t.columns
        ..removeAt(0)
        ..add(DesignColumn(name: 'a', type: 'int'));
      expect(
          DdlBuilder.alterUnsupported(t, original, pg), 'PostgreSQL 不支持调整已有列的顺序');
      final originalSs = twoCols(schema: 'dbo');
      final tSs = target(originalSs);
      tSs.columns
        ..removeAt(0)
        ..add(DesignColumn(name: 'a', type: 'int'));
      expect(DdlBuilder.alterUnsupported(tSs, originalSs, ss),
          'SQL Server 不支持调整已有列的顺序');
      expect(DdlBuilder.alterUnsupported(t, original, my), isNull);
    });

    test('SQL Server:IDENTITY 属性变更与主键名缺失', () {
      final original = base(schema: 'dbo');
      original.columns.first.identityMode = 'BY DEFAULT';
      final t = target(original);
      t.columns.first.identityMode = 'ALWAYS';
      expect(
        DdlBuilder.alterUnsupported(t, original, ss),
        contains('SQL Server 不支持修改已有列的 IDENTITY 属性'),
      );

      final noName = base(schema: 'dbo'); // pkName 空 + 主键列变化
      final t2 = target(noName);
      t2.columns.first.primaryKey = false;
      t2.columns.last.primaryKey = true;
      expect(
        DdlBuilder.alterUnsupported(t2, noName, ss),
        '未能读取该表的主键约束名,无法重建主键',
      );
      // MySQL 主键固定名 PRIMARY,不受该限制
      expect(DdlBuilder.alterUnsupported(t2, noName, my), isNull);
    });
  });

  group('snapshot 深拷贝', () {
    DesignTable fullTable() => base(comment: '订单', pkName: 't_pkey')
      ..indexes.add(DesignIndex(name: 'idx_name', columns: 'name'))
      ..uniqueKeys.add(DesignUniqueKey(name: 'uq_name', columns: 'name'))
      ..foreignKeys.add(DesignForeignKey(
          name: 'fk_u', columns: 'name', refTable: 'users', refColumns: 'id'))
      ..checks.add(DesignCheck(name: 'ck_n', expression: 'name IS NOT NULL'))
      ..excludes.add(DesignExclude(name: 'ex_n', columns: 'name WITH ='))
      ..rules.add(DesignRule(name: 'r', statement: 'NOTHING'))
      ..triggers.add(DesignTrigger(name: 'trg', function: 'public.f'));

    test('改副本不影响基线', () {
      final original = fullTable();
      final snap = original.snapshot();
      expect(identical(snap, original), isFalse);
      expect(identical(snap.columns.first, original.columns.first), isFalse);
      expect(identical(snap.indexes.first, original.indexes.first), isFalse);
      expect(identical(snap.foreignKeys.first, original.foreignKeys.first), isFalse);

      snap
        ..name = 't2'
        ..schema = 'other'
        ..pkName = 'other_pkey'
        ..tableComment = '改过';
      snap.columns.first
        ..name = 'x'
        ..notNull = false
        ..identityMode = 'ALWAYS';
      snap.indexes.first.columns = 'id';
      snap.uniqueKeys.first.name = 'uq_x';
      snap.foreignKeys.first.onDelete = 'CASCADE';
      snap.checks.first.expression = '1=1';
      snap.excludes.first.method = 'btree';
      snap.rules.first.event = 'UPDATE';
      snap.triggers.first.enable = false;

      expect(original.name, 't');
      expect(original.schema, 'public');
      expect(original.pkName, 't_pkey');
      expect(original.tableComment, '订单');
      expect(original.columns.first.name, 'id');
      expect(original.columns.first.notNull, isTrue);
      expect(original.columns.first.identityMode, isEmpty);
      expect(original.indexes.first.columns, 'name');
      expect(original.uniqueKeys.first.name, 'uq_name');
      expect(original.foreignKeys.first.onDelete, isNot('CASCADE'));
      expect(original.checks.first.expression, 'name IS NOT NULL');
      expect(original.excludes.first.method, isNot('btree'));
      expect(original.rules.first.event, 'INSERT');
      expect(original.triggers.first.enable, isTrue);

      // 基线未被污染 → 结构等价的原表仍无差异,改过的副本有差异
      expect(DdlBuilder.buildAlterStatements(fullTable(), original, pg), isEmpty);
      expect(DdlBuilder.buildAlterStatements(snap, original, pg), isNotEmpty);
    });
  });
}
