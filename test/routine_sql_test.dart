import 'package:daro/app/app_state.dart';
import 'package:daro/data/routine_sql.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('RoutineParam 签名', () {
    test('PG:参数名加引号、类型可按模式限定、默认值原样拼接', () {
      final sig = RoutineSql.signature('postgresql', [
        RoutineParam(mode: 'IN', name: 'a', typeSchema: 'pg_catalog', type: 'bool'),
        RoutineParam(
            mode: 'OUT', name: 'cnt', type: 'int4', defaultValue: '0'),
        RoutineParam(name: 'inout x', type: 'text', defaultValue: "'a'"),
      ]);
      expect(
        sig,
        'IN "a" pg_catalog.bool, OUT "cnt" int4 DEFAULT 0, IN "inout x" text DEFAULT \'a\'',
      );
    });

    test('MySQL:反引号参数名,无类型模式与默认值', () {
      final sig = RoutineSql.signature('mysql', [
        RoutineParam(
            mode: 'IN',
            name: 'a',
            typeSchema: 'pg_catalog',
            type: 'INT',
            defaultValue: '3'),
      ]);
      expect(sig, 'IN `a` INT');
    });

    test('SQL Server:@ 前缀、= 默认值、OUTPUT 在末尾', () {
      final sig = RoutineSql.signature('sqlserver', [
        RoutineParam(mode: '', name: 'a', type: 'INT'),
        RoutineParam(
            mode: 'OUTPUT', name: 'b', type: 'VARCHAR(50)', defaultValue: "''"),
      ]);
      expect(sig, '@a INT, @b VARCHAR(50) = \'\' OUTPUT');
    });

    test('名称与类型都为空的行不进气泡;只有类型时补占位名', () {
      final sig = RoutineSql.signature('postgresql', [
        RoutineParam(),
        RoutineParam(type: 'int4'),
      ]);
      expect(sig, 'IN "arg1" int4');
    });
  });

  group('RoutineSql 模板生成与解析', () {
    test('PG 过程模板逐字符等于参考工具的形状,且解析-重建幂等', () {
      final sql = RoutineSql.buildTemplate(
        typeId: 'postgresql',
        category: ObjectCategory.procedure,
        name: 'hanshu',
        schema: 'public',
        signature: 'IN "a" pg_catalog.bool',
      );
      expect(sql, '''
CREATE PROCEDURE "public"."hanshu" (IN "a" pg_catalog.bool)
AS \$BODY\$BEGIN
  -- Routine body goes here...

END
\$BODY\$
  LANGUAGE 'plpgsql'
  SECURITY INVOKER;
''');

      final p = parseRoutineSql(sql,
          typeId: 'postgresql', category: ObjectCategory.procedure);
      expect(p, isNotNull);
      expect(p!.signature, 'IN "a" pg_catalog.bool');
      expect(p.language, 'plpgsql');
      expect(p.options.securityDefiner, false);
      expect(p.body, '\n  -- Routine body goes here...\n');

      final rebuilt = RoutineSql.buildWithBody(
        typeId: 'postgresql',
        category: ObjectCategory.procedure,
        name: 'hanshu',
        schema: 'public',
        signature: p.signature,
        body: p.body,
        language: p.language,
        options: p.options,
      );
      expect(rebuilt, sql);
    });

    test('PG 函数模板:RETURNS + SECURITY DEFINER + RETURN 占位', () {
      final sql = RoutineSql.buildTemplate(
        typeId: 'postgresql',
        category: ObjectCategory.function,
        name: 'fn',
        schema: 'public',
        signature: 'IN "x" integer',
        returnType: 'integer',
        language: 'plpgsql',
        options: const RoutineOptions(securityDefiner: true),
      );
      expect(sql, contains('CREATE FUNCTION "public"."fn" (IN "x" integer)'));
      expect(sql, contains('RETURNS integer'));
      expect(sql, contains("  LANGUAGE 'plpgsql'"));
      expect(sql, contains('  SECURITY DEFINER;'));
      expect(sql, contains('  RETURN 0;'));

      final p = parseRoutineSql(sql,
          typeId: 'postgresql', category: ObjectCategory.function);
      expect(p, isNotNull);
      expect(p!.returnType, 'integer');
      expect(p.options.securityDefiner, true);
      expect(p.body, contains('RETURN 0;'));

      // 二次重建仍逐字节相同(空行不叠加)
      final rebuilt = RoutineSql.buildWithBody(
        typeId: 'postgresql',
        category: ObjectCategory.function,
        name: 'fn',
        schema: 'public',
        signature: p.signature,
        body: p.body,
        returnType: p.returnType,
        language: p.language,
        options: p.options,
      );
      expect(rebuilt, sql);
    });

    test('PG 无参数时签名为空括号;驱动风格的 \$function\$ 体也能剥壳', () {
      final sql = RoutineSql.buildTemplate(
        typeId: 'postgresql',
        category: ObjectCategory.procedure,
        name: 'p',
        schema: 'public',
      );
      expect(sql, contains('CREATE PROCEDURE "public"."p" ()'));

      const fromDriver = '''
CREATE OR REPLACE FUNCTION public.f(a integer)
 RETURNS integer
 LANGUAGE plpgsql
AS \$function\$
begin
  return a;
end;
\$function\$
''';
      final p = parseRoutineSql(fromDriver,
          typeId: 'postgresql', category: ObjectCategory.function);
      expect(p, isNotNull);
      expect(p!.signature, 'a integer');
      expect(p.body, contains('return a;'));
      expect(p.body, isNot(contains('begin')));
    });

    test('注释语句按签名原文拼接(仅 PG)', () {
      final comment = RoutineSql.commentOnSql(
        typeId: 'postgresql',
        category: ObjectCategory.function,
        name: 'fn',
        schema: 'public',
        signature: 'IN "x" integer',
        comment: "统计 '当月' 数据",
      );
      expect(
        comment,
        "COMMENT ON FUNCTION \"public\".\"fn\"(IN \"x\" integer) IS '统计 ''当月'' 数据';",
      );
      expect(
        RoutineSql.commentOnSql(
          typeId: 'mysql',
          category: ObjectCategory.function,
          name: 'fn',
          signature: '',
          comment: 'x',
        ),
        isEmpty,
      );
    });

    test('MySQL 模板:反引号 + SQL SECURITY,且解析回签名与函数体', () {
      final proc = RoutineSql.buildTemplate(
        typeId: 'mysql',
        category: ObjectCategory.procedure,
        name: 'p',
        signature: 'OUT `cnt` INT',
      );
      expect(proc, contains('CREATE PROCEDURE `p` (OUT `cnt` INT)'));
      expect(proc, contains('SQL SECURITY INVOKER'));
      final pp = parseRoutineSql(proc,
          typeId: 'mysql', category: ObjectCategory.procedure);
      expect(pp, isNotNull);
      expect(pp!.signature, 'OUT `cnt` INT');
      expect(pp.options.securityDefiner, false);
      expect(pp.body, contains('Routine body goes here'));

      final fn = RoutineSql.buildTemplate(
        typeId: 'mysql',
        category: ObjectCategory.function,
        name: 'f',
        options: const RoutineOptions(securityDefiner: true),
      );
      expect(fn, contains('CREATE FUNCTION `f` ()'));
      expect(fn, contains('RETURNS INT'));
      expect(fn, contains('SQL SECURITY DEFINER'));
      final fp = parseRoutineSql(fn,
          typeId: 'mysql', category: ObjectCategory.function);
      expect(fp, isNotNull);
      expect(fp!.returnType, 'INT');
      expect(fp.options.securityDefiner, true);
    });

    test('SQL Server 模板:WITH EXECUTE AS 不混进参数签名', () {
      final proc = RoutineSql.buildTemplate(
        typeId: 'sqlserver',
        category: ObjectCategory.procedure,
        name: 'p',
        schema: 'dbo',
        signature: '@a INT, @b VARCHAR(50) OUTPUT',
      );
      expect(proc, contains('CREATE PROCEDURE [dbo].[p]'));
      expect(proc, contains('  @a INT,'));
      expect(proc, contains('  @b VARCHAR(50) OUTPUT'));
      expect(proc, contains('WITH EXECUTE AS CALLER'));
      final pp = parseRoutineSql(proc,
          typeId: 'sqlserver', category: ObjectCategory.procedure);
      expect(pp, isNotNull);
      expect(pp!.signature, '@a INT, @b VARCHAR(50) OUTPUT');
      expect(pp.options.securityDefiner, false);

      final fn = RoutineSql.buildTemplate(
        typeId: 'sqlserver',
        category: ObjectCategory.function,
        name: 'f',
        schema: 'dbo',
        signature: '@x INT',
        options: const RoutineOptions(securityDefiner: true),
      );
      expect(fn, contains('CREATE FUNCTION [dbo].[f]'));
      expect(fn, contains('  (@x INT)'));
      expect(fn, contains('RETURNS INT'));
      expect(fn, contains('WITH EXECUTE AS OWNER'));
      final fp = parseRoutineSql(fn,
          typeId: 'sqlserver', category: ObjectCategory.function);
      expect(fp, isNotNull);
      expect(fp!.returnType, 'INT');
      expect(fp.signature, '@x INT');
      expect(fp.options.securityDefiner, true);
    });
  });

  group('方言门控与向导候选', () {
    test('MySQL 没有类型模式层,也不支持参数默认值', () {
      expect(RoutineSql.supportsTypeSchema('mysql'), isFalse);
      expect(RoutineSql.supportsParamDefault('mysql'), isFalse);
      expect(RoutineSql.supportsTypeSchema('postgresql'), isTrue);
      expect(RoutineSql.supportsParamDefault('postgresql'), isTrue);
      expect(RoutineSql.supportsTypeSchema('sqlserver'), isTrue);
      expect(RoutineSql.supportsParamDefault('sqlserver'), isTrue);
    });

    test('PG 的类型模式候选含 pg_catalog 与目标模式', () {
      expect(
        RoutineSql.typeSchemasOf('postgresql', schema: 'app'),
        containsAll(<String>['', 'pg_catalog', 'app']),
      );
      expect(RoutineSql.typeSchemasOf('mysql'), ['']);
    });

    test('SQL Server 的普通参数模式为空串,界面显示「普通」', () {
      expect(RoutineSql.modesOf('sqlserver'), ['', 'OUTPUT']);
      expect(RoutineSql.modeLabel(''), '普通');
      expect(RoutineSql.modeLabel('IN'), 'IN');
    });
  });

  group('返回类型原文与占位字面量', () {
    test('带模式前缀的裸类型加引号,带修饰的原样拼', () {
      expect(RoutineSql.qualifiedType('postgresql',
          schema: 'pg_catalog', type: 'char'), 'pg_catalog."char"');
      expect(RoutineSql.qualifiedType('postgresql',
          schema: 'pg_catalog', type: 'varchar(100)'),
          'pg_catalog.varchar(100)');
      expect(RoutineSql.qualifiedType('postgresql', type: 'char'), 'char');
      expect(RoutineSql.qualifiedType('mysql',
          schema: 'pg_catalog', type: 'int'), 'int');
      expect(RoutineSql.qualifiedType('sqlserver',
          schema: 'dbo', type: 'MyType'), '[dbo].[MyType]');
    });

    test('占位 RETURN 的字面量跟着返回类型走(参考工具即如此)', () {
      expect(RoutineSql.returnPlaceholder('pg_catalog."char"'), "RETURN ' ';");
      expect(RoutineSql.returnPlaceholder('character(1)'), "RETURN ' ';");
      expect(RoutineSql.returnPlaceholder('varchar(100)'), "RETURN '';");
      expect(RoutineSql.returnPlaceholder('pg_catalog.bool'), 'RETURN false;');
      expect(RoutineSql.returnPlaceholder('int8'), 'RETURN 0;');
      expect(RoutineSql.returnPlaceholder('uuid'), 'RETURN NULL;');
      // 没填返回类型时模板补方言默认类型(INT),函数体不能给个不匹配的字面量
      final sql = RoutineSql.buildTemplate(
        typeId: 'postgresql',
        category: ObjectCategory.function,
        name: 'f',
        schema: 'public',
      );
      expect(sql, contains('RETURNS integer'));
      expect(sql, contains('  RETURN 0;'));
    });
  });

  group('PG 函数控件(窗口 / 特性 / 防漏 / STRICT)', () {
    test('向导第 3 步的四个选项各自生成一条子句,顺序固定、解析回幂等', () {
      const options = RoutineOptions(
        window: true,
        volatility: 'STABLE',
        leakProof: true,
        strict: true,
        securityDefiner: true,
      );
      final sql = RoutineSql.buildTemplate(
        typeId: 'postgresql',
        category: ObjectCategory.function,
        name: 'f',
        schema: 'public',
        signature: 'IN "x" integer',
        returnType: 'integer',
        options: options,
      );
      // 子句都在函数体之后,两空格缩进,分号恒在最后一条(SECURITY)
      expect(
        sql,
        endsWith('''
  LANGUAGE 'plpgsql'
  WINDOW
  STABLE
  LEAKPROOF
  STRICT
  SECURITY DEFINER;
'''),
      );

      final p = parseRoutineSql(sql,
          typeId: 'postgresql', category: ObjectCategory.function);
      expect(p, isNotNull);
      expect(p!.options, options);

      final rebuilt = RoutineSql.buildWithBody(
        typeId: 'postgresql',
        category: ObjectCategory.function,
        name: 'f',
        schema: 'public',
        signature: p.signature,
        body: p.body,
        returnType: p.returnType,
        language: p.language,
        options: p.options,
      );
      expect(rebuilt, sql);
    });

    test('全不勾选时只剩 LANGUAGE + SECURITY(不写空的 VOLATILE)', () {
      final sql = RoutineSql.buildTemplate(
        typeId: 'postgresql',
        category: ObjectCategory.procedure,
        name: 'p',
        schema: 'public',
      );
      expect(sql, isNot(contains('WINDOW')));
      expect(sql, isNot(contains('VOLATILE')));
      expect(sql, isNot(contains('LEAKPROOF')));
      expect(sql, isNot(contains('STRICT')));
    });

    test('函数体里的关键字不算选项;NOT LEAKPROOF 是否定式', () {
      const sql = '''
CREATE FUNCTION "public"."f" ()
 RETURNS integer
AS \$BODY\$BEGIN
  -- 窗口函数示例:OVER (WINDOW w AS (...))
  RETURN 0;
END
\$BODY\$
  LANGUAGE 'plpgsql'
  NOT LEAKPROOF
  SECURITY INVOKER;
''';
      final p = parseRoutineSql(sql,
          typeId: 'postgresql', category: ObjectCategory.function);
      expect(p, isNotNull);
      expect(p!.options.window, isFalse, reason: 'WINDOW 只出现在注释里');
      expect(p.options.leakProof, isFalse, reason: 'NOT LEAKPROOF 是否定写法');
      expect(p.options.volatility, '');
      expect(p.body, contains('WINDOW w AS'));
    });

    test('RETURNS NULL ON NULL INPUT 解析为 strict,不会被当成返回类型', () {
      const sql = '''
CREATE FUNCTION "public"."f" ()
 RETURNS NULL ON NULL INPUT
 RETURNS integer
AS \$BODY\$BEGIN
  RETURN 0;
END
\$BODY\$
  LANGUAGE 'plpgsql'
  SECURITY INVOKER;
''';
      final p = parseRoutineSql(sql,
          typeId: 'postgresql', category: ObjectCategory.function);
      expect(p, isNotNull);
      expect(p!.options.strict, isTrue);
      expect(p.returnType, 'integer');
    });
  });
}
