import '../app/app_state.dart';

/// 例程选项(向导第 3 步 / 设计页「高级」)。
///
/// 除 [securityDefiner] 外全部是 PostgreSQL 独有的子句(MySQL / SQL Server
/// 没有对应语法),所以非 PG 的界面上这几项一律禁用 —— 见 AGENTS.md 的
/// 「方言门控」:界面能改、DDL 不生成是最容易长出来的缺陷。
class RoutineOptions {
  const RoutineOptions({
    this.window = false,
    this.volatility = '',
    this.leakProof = false,
    this.strict = false,
    this.securityDefiner = false,
  });

  /// PG `WINDOW`:声明为窗口函数(实际只对 C 语言函数有意义)
  final bool window;

  /// 可变性:`VOLATILE` / `STABLE` / `IMMUTABLE`,空 = 不写该子句
  /// (PG 缺省即 VOLATILE,故不写与写 VOLATILE 语义相同,但输出更贴近参考工具)
  final String volatility;

  /// PG `LEAKPROOF`(仅超级用户可写)
  final bool leakProof;

  /// PG `STRICT`(= RETURNS NULL ON NULL INPUT)
  final bool strict;

  /// PG `SECURITY DEFINER`(以定义者权限执行)vs INVOKER(以调用者权限)
  final bool securityDefiner;

  RoutineOptions copyWith({
    bool? window,
    String? volatility,
    bool? leakProof,
    bool? strict,
    bool? securityDefiner,
  }) =>
      RoutineOptions(
        window: window ?? this.window,
        volatility: volatility ?? this.volatility,
        leakProof: leakProof ?? this.leakProof,
        strict: strict ?? this.strict,
        securityDefiner: securityDefiner ?? this.securityDefiner,
      );

  @override
  bool operator ==(Object other) =>
      other is RoutineOptions &&
      other.window == window &&
      other.volatility == volatility &&
      other.leakProof == leakProof &&
      other.strict == strict &&
      other.securityDefiner == securityDefiner;

  @override
  int get hashCode =>
      Object.hash(window, volatility, leakProof, strict, securityDefiner);
}

/// 例程参数(函数向导第 2 步的参数网格,一行一个)。
/// [mode]:PostgreSQL / MySQL / MariaDB 为 IN / OUT / INOUT;
/// SQL Server 为 'OUTPUT'(输出参数)或 ''(普通参数)。
/// [typeSchema]:类型的命名空间(PG 的 pg_catalog 等;空 = 不限定),
/// MySQL 没有类型模式层,该列在界面上禁用。
class RoutineParam {
  RoutineParam({
    this.mode = 'IN',
    this.name = '',
    this.typeSchema = '',
    this.type = '',
    this.defaultValue = '',
  });

  String mode;
  String name;
  String typeSchema;
  String type;

  /// 参数默认值原文(数字 / 字面量原样拼进 SQL,不加引号)
  String defaultValue;
}

/// 例程 SQL 生成 / 解析工具:
/// 按数据库类型生成 CREATE FUNCTION / CREATE PROCEDURE 模板,
/// 并把模板(或驱动返回的定义)解析回 参数签名 / 返回类型 / 语言 /
/// 安全定义者 / 函数体,供「高级」「SQL 预览」「注释」标签使用。
///
/// 模板格式固定(向导 + 新建设计页共用),解析器只识别本文件生成的格式;
/// 用户手动改写的 SQL 无法识别时返回 null,UI 退化为默认值并提示。
class RoutineSql {
  RoutineSql._();

  /// 无模式层(MySQL / MariaDB / SQLite / Access)时不需要默认模式;
  /// PostgreSQL 默认 public,SQL Server 默认 dbo
  static String? defaultSchema(String typeId) => switch (typeId) {
        'postgresql' => 'public',
        'sqlserver' => 'dbo',
        _ => null,
      };

  /// 数据库类型是否支持例程(函数 / 过程)概念
  static bool supportsRoutine(String typeId) => const {
        'mysql',
        'mariadb',
        'postgresql',
        'sqlserver',
      }.contains(typeId);

  /// 是否支持过程(PostgreSQL 11+ 才支持;低版本由服务端报错,驱动仍列出)
  static bool supportsProcedure(String typeId) => supportsRoutine(typeId);

  /// 数据库类型是否支持 COMMENT ON(仅 PostgreSQL 有例程注释语法)
  static bool supportsCommentOn(String typeId) => typeId == 'postgresql';

  /// 函数默认返回类型(新建模板使用)
  static String defaultReturnType(String typeId) => switch (typeId) {
        'postgresql' => 'integer',
        'sqlserver' => 'INT',
        _ => 'INT',
      };

  /// PostgreSQL 默认过程语言
  static String defaultLanguage(String typeId) =>
      typeId == 'postgresql' ? 'plpgsql' : '';

  /// 数据库类型可选的过程语言列表(仅 PostgreSQL 有意义)
  static List<String> languagesOf(String typeId) => switch (typeId) {
        'postgresql' => const ['plpgsql', 'sql', 'plpython3u'],
        _ => const [],
      };

  /// 参数模式候选(按数据库类型)
  static List<String> modesOf(String typeId) => switch (typeId) {
        'sqlserver' => const ['', 'OUTPUT'],
        _ => const ['IN', 'OUT', 'INOUT'],
      };

  /// 安全性候选(向导第 3 步;与 SQL 里的 DEFINER / INVOKER 关键字同名)
  static const List<String> securityOptions = ['INVOKER', 'DEFINER'];

  /// 「特性」候选(可变性;空串 = 不写子句,界面上显示「(未指定)」)
  static const List<String> volatilityOptions = [
    '',
    'VOLATILE',
    'STABLE',
    'IMMUTABLE',
  ];

  /// 空可变性在界面上的写法
  static String volatilityLabel(String v) => v.isEmpty ? '(未指定)' : v;

  static bool isPgLike(String typeId) => typeId == 'postgresql';
  static bool isMysqlLike(String typeId) =>
      typeId == 'mysql' || typeId == 'mariadb';
  static bool isSqlServerLike(String typeId) => typeId == 'sqlserver';

  /// 参数网格的「类型模式」列是否可用(仅 PG 与 SQL Server 有类型命名空间)
  static bool supportsTypeSchema(String typeId) =>
      isPgLike(typeId) || isSqlServerLike(typeId);

  /// 参数网格的「默认值」列是否可用(MySQL / MariaDB 的例程参数不支持默认值)
  static bool supportsParamDefault(String typeId) =>
      !isMysqlLike(typeId);

  /// 「类型模式」候选(PG 的类型命名空间)
  static List<String> typeSchemasOf(String typeId, {String? schema}) {
    if (isPgLike(typeId)) {
      final s = (schema == null || schema.isEmpty) ? 'public' : schema;
      return ['', 'pg_catalog', 'information_schema', s];
    }
    if (isSqlServerLike(typeId)) return ['', 'dbo'];
    return const [''];
  }

  /// 「类型」候选(常用类型;仍可手工输入,故网格用可编辑下拉)
  static List<String> typesOf(String typeId) => switch (typeId) {
        'postgresql' => const [
            'bool',
            'bytea',
            'char',
            'varchar',
            'text',
            'int2',
            'int4',
            'int8',
            'numeric',
            'float4',
            'float8',
            'date',
            'time',
            'timestamp',
            'timestamptz',
            'uuid',
            'json',
            'jsonb',
          ],
        'sqlserver' => const [
            'INT',
            'BIGINT',
            'BIT',
            'DECIMAL',
            'VARCHAR',
            'NVARCHAR',
            'TEXT',
            'DATETIME',
            'DATE',
            'UNIQUEIDENTIFIER',
          ],
        _ => const [
            'INT',
            'BIGINT',
            'TINYINT',
            'DECIMAL',
            'CHAR',
            'VARCHAR',
            'TEXT',
            'DATE',
            'DATETIME',
            'TIME',
            'BOOLEAN',
          ],
      };

  /// 「模式」候选的展示文本(SQL Server 的普通参数模式为空串)
  static String modeLabel(String mode) => mode.isEmpty ? '普通' : mode;

  /// 按类型引用的对象名(如 "public"."test" / `test` / [dbo].[test])
  static String qualifiedName(String typeId, String name, {String? schema}) {
    final s = (schema == null || schema.isEmpty)
        ? defaultSchema(typeId)
        : schema;
    switch (typeId) {
      case 'postgresql':
      case 'sqlite':
        final q = (v) => '"${v.replaceAll('"', '""')}"';
        return s == null ? q(name) : '${q(s)}.${q(name)}';
      case 'sqlserver':
      case 'access':
        final q = (v) => '[${v.replaceAll(']', ']]')}]';
        return s == null ? q(name) : '${q(s)}.${q(name)}';
      default: // mysql / mariadb:Database 即 Schema,不做二级限定
        return '`${name.replaceAll('`', '``')}`';
    }
  }

  /// 由参数列表生成签名文本(按数据库类型):
  /// - PostgreSQL:`IN "a" pg_catalog.bool DEFAULT false`
  /// - MySQL / MariaDB:`IN \`a\` int`(无默认值、无类型模式)
  /// - SQL Server:`@a INT = 5 OUTPUT`
  /// 参数名一律按方言引用符包裹(与参考工具一致),类型可由
  /// [RoutineParam.typeSchema] 加模式前缀限定。
  static String signature(String typeId, List<RoutineParam> params) {
    final parts = <String>[];
    for (final p in params) {
      final name = p.name.trim();
      final type = p.type.trim();
      if (name.isEmpty && type.isEmpty) continue;
      final n = name.isEmpty ? 'arg${parts.length + 1}' : name;
      final ty = type.isEmpty ? 'INT' : type;
      final dv = p.defaultValue.trim();
      switch (typeId) {
        case 'postgresql':
          final quoted = '"${n.replaceAll('"', '""')}"';
          final ts = p.typeSchema.trim();
          parts.add([
            p.mode.trim().isEmpty ? 'IN' : p.mode.trim(),
            quoted,
            ts.isEmpty ? ty : '$ts.$ty',
            if (dv.isNotEmpty) 'DEFAULT $dv',
          ].join(' '));
        case 'sqlserver':
          final pn = n.startsWith('@') ? n : '@$n';
          parts.add([
            pn,
            ty,
            if (dv.isNotEmpty) '= $dv',
            if (p.mode.trim().toUpperCase() == 'OUTPUT') 'OUTPUT',
          ].join(' '));
        default: // mysql / mariadb
          final quoted = '`${n.replaceAll('`', '``')}`';
          parts.add([
            p.mode.trim().isEmpty ? 'IN' : p.mode.trim(),
            quoted,
            ty,
          ].join(' '));
      }
    }
    return parts.join(', ');
  }

  /// 生成新建模板(函数体为默认占位,用户在设计页「定义」标签中编辑)。
  /// [signature] 为参数签名原文(向导第 2 步由 [signature] 生成)。
  /// 注释不嵌入模板:由设计页「注释」标签单独管理,保存时作为 COMMENT ON
  /// 执行(仅 PG;见 [commentOnSql])
  static String buildTemplate({
    required String typeId,
    required ObjectCategory category,
    required String name,
    String? schema,
    String signature = '',
    String returnType = '',
    String language = '',
    RoutineOptions options = const RoutineOptions(),
  }) {
    // 占位函数体要按**生效的**返回类型给字面量,所以这里先把空值补成方言
    // 默认类型,再交给 buildWithBody(它自己也会补,两处口径必须一致)。
    final isFunction = category == ObjectCategory.function;
    final ret = !isFunction
        ? ''
        : (returnType.trim().isEmpty
            ? defaultReturnType(typeId)
            : returnType.trim());
    return buildWithBody(
      typeId: typeId,
      category: category,
      name: name,
      schema: schema,
      signature: signature,
      body: defaultBody(isFunction, returnType: ret),
      returnType: ret,
      language: language,
      options: options,
    );
  }

  /// RETURNS 后面的类型原文。参考工具把带模式前缀的类型写成
  /// `pg_catalog."char"`(PG 目录里 `"char"` 与不带引号的 `char` 是两个类型),
  /// 但**只有裸标识符能整体加引号**:`varchar(100)` / `integer[]` 一包就成语法错。
  static String qualifiedType(String typeId,
      {String schema = '', String type = ''}) {
    final ty = type.trim();
    if (ty.isEmpty) return '';
    final ts = schema.trim();
    if (ts.isEmpty || !supportsTypeSchema(typeId)) return ty;
    if (isSqlServerLike(typeId)) {
      final q = (v) => '[${v.replaceAll(']', ']]')}]';
      return '${q(ts)}.${q(ty)}';
    }
    final bare =
        RegExp(r'^[\p{L}_][\w$]*$', unicode: true).hasMatch(ty);
    return bare ? '$ts."${ty.replaceAll('"', '""')}"' : '$ts.$ty';
  }

  /// 函数体占位的 RETURN 字面量,按返回类型给一个能编译过的(参考工具即如此:
  /// 返回 char 时写 `RETURN ' ';`)。认不出的类型一律 `RETURN NULL;`。
  static String returnPlaceholder(String returnType) {
    // 去掉模式前缀、长度修饰与数组标记,只留类型词本体
    final base = returnType
        .trim()
        .toLowerCase()
        .replaceAll(RegExp(r'\(.*\)$'), '')
        .replaceAll('[]', '')
        .split('.')
        .last
        .replaceAll(RegExp(r'^"+|"+$'), '')
        .trim();
    switch (base) {
      case 'char':
      case 'bpchar':
      case 'character':
        return "RETURN ' ';";
      case 'varchar':
      case 'text':
      case 'name':
      case 'citext':
        return "RETURN '';";
      case 'bool':
      case 'boolean':
        return 'RETURN false;';
      case 'int2':
      case 'int4':
      case 'int8':
      case 'smallint':
      case 'integer':
      case 'bigint':
      case 'numeric':
      case 'decimal':
      case 'float4':
      case 'float8':
      case 'real':
      case 'double':
      case 'int':
      case 'tinyint':
      case 'bit':
        return 'RETURN 0;';
      default:
        return 'RETURN NULL;';
    }
  }

  /// PG 的函数体约定:**以换行开头**(紧跟在 `AS $BODY$BEGIN` 之后)、且不以
  /// 换行结尾,使「`BEGIN` + body + `\nEND`」与 [parseRoutineSql] 的剥壳严格互逆
  /// (不遵守这条,每解析重建一次就多出一个空行)。
  /// 其它方言的 body 是 BEGIN…END 之间的裸文本(重建时会 trimRight)。
  static String defaultBody(bool isFunction, {String returnType = ''}) =>
      isFunction
          ? '\n  -- Routine body goes here...\n\n  ${returnPlaceholder(returnType)}'
          : '\n  -- Routine body goes here...\n';

  /// 用指定函数体重建完整 SQL(「高级」标签「应用」时调用)。
  /// [signature] 为参数签名原文(解析自当前 SQL),[body] 为函数体原文。
  /// 三种方言都**显式写出**安全子句,向导第 3 步的选择不会静默丢失:
  /// PG `SECURITY {INVOKER|DEFINER}`、MySQL `SQL SECURITY …`、
  /// SQL Server `WITH EXECUTE AS {CALLER|OWNER}`。
  static String buildWithBody({
    required String typeId,
    required ObjectCategory category,
    required String name,
    String? schema,
    required String signature,
    required String body,
    String returnType = '',
    String language = '',
    RoutineOptions options = const RoutineOptions(),
  }) {
    final qualified = qualifiedName(typeId, name, schema: schema);
    final isFunction = category == ObjectCategory.function;
    final kw = isFunction ? 'FUNCTION' : 'PROCEDURE';
    final ret =
        isFunction ? (returnType.trim().isEmpty ? defaultReturnType(typeId) : returnType.trim()) : '';
    final lang = language.trim().isEmpty ? defaultLanguage(typeId) : language.trim();
    final sig = signature.trim();
    final args = sig.isEmpty ? '()' : '($sig)';
    final securityDefiner = options.securityDefiner;
    final securityWord = securityDefiner ? 'DEFINER' : 'INVOKER';
    final buf = StringBuffer();

    if (isPgLike(typeId)) {
      // 参考工具的形状:选项子句在函数体**之后**,各占一行、两空格缩进;
      // `AS $BODY$` 与 BEGIN 同行;分号收尾在最后一条子句(SECURITY 恒在最后)。
      buf.writeln('CREATE $kw $qualified $args');
      if (isFunction && ret.isNotEmpty) buf.writeln('RETURNS $ret');
      buf.write('AS \$BODY\$BEGIN');
      buf.write(body);
      buf.writeln('\nEND');
      buf.writeln('\$BODY\$');
      if (lang.isNotEmpty) buf.writeln("  LANGUAGE '$lang'");
      if (options.window) buf.writeln('  WINDOW');
      if (options.volatility.trim().isNotEmpty) {
        buf.writeln('  ${options.volatility.trim().toUpperCase()}');
      }
      if (options.leakProof) buf.writeln('  LEAKPROOF');
      if (options.strict) buf.writeln('  STRICT');
      buf.writeln('  SECURITY $securityWord;');
      return buf.toString();
    }

    if (isSqlServerLike(typeId)) {
      buf.writeln('CREATE $kw $qualified');
      if (isFunction) {
        buf.writeln('  $args');
        if (ret.isNotEmpty) buf.writeln('RETURNS $ret');
      } else if (sig.isNotEmpty) {
        buf.writeln('  ${sig.replaceAll(', ', ',\n  ')}');
      }
      buf.writeln('WITH EXECUTE AS ${securityDefiner ? 'OWNER' : 'CALLER'}');
      buf.writeln('AS');
      buf.writeln('BEGIN');
      buf.write(body.trimRight());
      buf.writeln('\nEND;');
      return buf.toString();
    }

    // mysql / mariadb
    buf.writeln('CREATE $kw $qualified $args');
    if (isFunction && ret.isNotEmpty) buf.writeln('RETURNS $ret');
    buf.writeln('SQL SECURITY $securityWord');
    buf.writeln('BEGIN');
    buf.write(body.trimRight());
    buf.writeln('\nEND;');
    return buf.toString();
  }

  /// PG 的 COMMENT ON 语句(供保存时执行;其它类型返回空)
  static String commentOnSql({
    required String typeId,
    required ObjectCategory category,
    required String name,
    String? schema,
    required String signature,
    required String comment,
  }) {
    if (!supportsCommentOn(typeId)) return '';
    final c = comment.trim();
    if (c.isEmpty) return '';
    final qualified = qualifiedName(typeId, name, schema: schema);
    final kw = category == ObjectCategory.function ? 'FUNCTION' : 'PROCEDURE';
    final esc = c.replaceAll("'", "''");
    return "COMMENT ON $kw $qualified($signature) IS '$esc';";
  }
}

/// 解析结果(模板 / 定义文本的结构化视图)
class RoutineParsed {
  const RoutineParsed({
    required this.signature,
    required this.returnType,
    required this.language,
    required this.options,
    required this.body,
  });

  /// 参数签名原文(如 "IN a INT, IN b VARCHAR(50)")
  final String signature;

  /// 函数返回类型(过程为空)
  final String returnType;

  /// 过程语言(PG 等;其它为空)
  final String language;

  /// 选项(可变性 / STRICT / LEAKPROOF / WINDOW / 安全性)
  final RoutineOptions options;

  /// 函数体原文(不含包裹符 / BEGIN END)
  final String body;
}

/// 解析例程 SQL,提取 参数签名 / 返回类型 / 语言 / 安全定义者 / 函数体。
/// 仅识别 [RoutineSql] 生成的模板格式及常见变体(PG 的 $function$ / $$ 包裹、
/// 驱动 SHOW CREATE 输出);无法识别返回 null。
RoutineParsed? parseRoutineSql(
  String sql, {
  required String typeId,
  required ObjectCategory category,
}) {
  if (sql.trim().isEmpty) return null;
  final isFunction = category == ObjectCategory.function;

  switch (typeId) {
    case 'postgresql':
      final m = RegExp(
        r'^CREATE(?:\s+OR\s+REPLACE)?\s+(FUNCTION|PROCEDURE)\s+'
        // 名称:带/不带引号、可带模式前缀(驱动反查回来的定义是不带引号的)
        r'(?:[\w\s]+|"[^"]*")(?:\s*\.\s*(?:[\w\s]+|"[^"]*"))?\s*'
        r'\((?<params>[^\n]*)\)',
        caseSensitive: false,
        multiLine: true,
      ).firstMatch(sql.trim());
      if (m == null) return null;
      final params = m.namedGroup('params')!.trim();
      final shell = _stripDollarBody(sql);
      final ret = isFunction
          // 排除 `RETURNS NULL ON NULL INPUT`(STRICT 的另一种写法),
          // 否则返回类型会读成 'NULL ON NULL INPUT'
          ? RegExp(r'RETURNS\s+(?!NULL\s+ON\s+NULL\b)(?<ret>[^\n]+)',
                  caseSensitive: false)
              .firstMatch(shell)
              ?.namedGroup('ret')
              ?.trim() ?? ''
          : '';
      final lang = RegExp(r"LANGUAGE\s+'?(?<lang>\w+)'?",
              caseSensitive: false)
          .firstMatch(shell)
          ?.namedGroup('lang') ?? '';
      final body = _extractDollarBody(sql);
      if (body == null) return null;
      return RoutineParsed(
        signature: params,
        returnType: ret,
        language: lang,
        options: RoutineOptions(
          window: RegExp(r'\bWINDOW\b', caseSensitive: false).hasMatch(shell),
          volatility: RegExp(r'\b(IMMUTABLE|STABLE|VOLATILE)\b',
                  caseSensitive: false)
              .firstMatch(shell)
              ?.group(1)
              ?.toUpperCase() ?? '',
          // NOT LEAKPROOF 是反义,不能当成开了防漏
          leakProof: RegExp(r'(?<!NOT\s)\bLEAKPROOF\b', caseSensitive: false)
              .hasMatch(shell),
          strict: RegExp(
            r'\bSTRICT\b|\bRETURNS\s+NULL\s+ON\s+NULL\s+INPUT\b',
            caseSensitive: false,
          ).hasMatch(shell),
          securityDefiner:
              RegExp(r'SECURITY\s+DEFINER', caseSensitive: false)
                  .hasMatch(shell),
        ),
        body: body,
      );
    case 'mysql':
    case 'mariadb':
      final m = RegExp(
        r'^CREATE\s+(FUNCTION|PROCEDURE)\s+'
        r'`?[\w$]+`?(?:\s*\.\s*`?[\w$]+`?)?\s*'
        r'\((?<params>[^\n]*)\)',
        caseSensitive: false,
      ).firstMatch(sql.trim());
      if (m == null) return null;
      final params = m.namedGroup('params')!.trim();
      final ret = isFunction
          ? RegExp(r'RETURNS\s+(?<ret>\S+)', caseSensitive: false)
              .firstMatch(sql)
              ?.namedGroup('ret')
              ?.trim() ?? ''
          : '';
      final body = _extractBeginEnd(sql);
      if (body == null) return null;
      return RoutineParsed(
        signature: params,
        returnType: ret,
        language: '',
        options: RoutineOptions(
          securityDefiner:
              RegExp(r'SECURITY\s+DEFINER', caseSensitive: false).hasMatch(sql),
        ),
        body: body,
      );
    case 'sqlserver':
      // 函数:CREATE FUNCTION [dbo].[name] (@a INT) RETURNS INT AS BEGIN ... END
      // 过程:CREATE PROCEDURE [dbo].[name] @a INT, @b INT OUTPUT AS BEGIN ... END
      final kwMatch = RegExp(
        r'^CREATE\s+(FUNCTION|PROCEDURE)\s+'
        r'\[?[\w\s#]+\]?(?:\s*\.\s*\[?[\w\s]+\]?)?',
        caseSensitive: false,
      ).firstMatch(sql.trim());
      if (kwMatch == null) return null;
      final head = sql.substring(kwMatch.end).trimLeft();
      String params;
      if (isFunction) {
        final pm = RegExp(r'^\((?<params>[^\n]*)\)', caseSensitive: false)
            .firstMatch(head);
        if (pm == null) return null;
        params = pm.namedGroup('params')!.trim();
      } else {
        // 过程:参数为 AS 之前的整段,去掉行尾分号
        final asIdx = RegExp(r'\bAS\b', caseSensitive: false)
            .allMatches(head)
            .map((m) => m.start)
            .lastOrNull;
        if (asIdx == null) return null;
        final paramText = head
            .substring(0, asIdx)
            .trim()
            // 安全子句夹在参数与 AS 之间,不剥掉就会混进签名
            .replaceAll(
                RegExp(r'\bWITH\s+EXECUTE\s+AS\s+\w+', caseSensitive: false),
                '')
            .replaceAll(RegExp(r'^\s*\(\s*|\s*\)\s*$'), '')
            // 行尾逗号去掉,避免与换行替换产生的 ',,' 连排
            .replaceAll(RegExp(r',\s*\n'), '\n')
            .replaceAll(RegExp(r'\s*\n\s*'), ', ')
            // 剥掉安全子句后可能留下行尾逗号
            .replaceAll(RegExp(r',\s*$'), '');
        params = paramText.trim();
      }
      final ret = isFunction
          ? RegExp(r'RETURNS\s+(?<ret>[^\n]+)', caseSensitive: false)
              .firstMatch(sql)
              ?.namedGroup('ret')
              ?.trim() ?? ''
          : '';
      final body = _extractBeginEnd(sql);
      if (body == null) return null;
      return RoutineParsed(
        signature: params,
        returnType: ret,
        language: '',
        options: RoutineOptions(
          securityDefiner:
              RegExp(r'EXECUTE\s+AS\s+OWNER', caseSensitive: false)
                  .hasMatch(sql),
        ),
        body: body,
      );
    default:
      return null;
  }
}

/// 抹掉美元引号包裹的函数体(换成一个空格),只留下子句「外壳」。
/// 解析 WINDOW / STRICT / VOLATILE 这类裸单词必须先剥壳:用户函数体里
/// 一句 `... WINDOW w AS (...)` 或变量名 `strict` 就会被当成子句读出来。
String _stripDollarBody(String sql) =>
    sql.replaceAll(RegExp(r'\$\w*\$[\s\S]*?\$\w*\$'), ' ');

/// 提取 PG 的 $tag$ ... $tag$ 函数体(支持 $BODY$ / $function$ / $$ 等)。
/// 返回遵循 [RoutineSql.defaultBody] 的约定:以换行开头、不以换行结尾,
/// 以便 [RoutineSql.buildWithBody] 原样拼回 `AS $BODY$BEGIN … END`。
/// 包裹符之外没有 BEGIN…END 时(如 LANGUAGE sql 的表达式体)按原文返回。
String? _extractDollarBody(String sql) {
  final m = RegExp(r'\bAS\s*\$(?<tag>\w*)\$(?<inner>[\s\S]*?)\$(?<tag2>\w*)\$')
      .firstMatch(sql);
  if (m == null) return null;
  if (m.namedGroup('tag') != m.namedGroup('tag2')) return null;
  final inner = m.namedGroup('inner')!;
  final be = RegExp(r'^\s*BEGIN\b(?<b>[\s\S]*?)\bEND\b\s*;?\s*$',
          caseSensitive: false)
      .firstMatch(inner);
  if (be == null) return inner;
  // 去掉 END 前那个换行(它由生成侧的 `\nEND` 补回),保证往返幂等
  var body = be.namedGroup('b')!;
  if (body.endsWith('\n')) body = body.substring(0, body.length - 1);
  return body;
}

/// 提取 MySQL / SQL Server 的 BEGIN ... END 函数体(取第一个 BEGIN 到最后一个 END)
String? _extractBeginEnd(String sql) {
  final beginIdx = RegExp(r'\bBEGIN\b', caseSensitive: false)
      .allMatches(sql)
      .map((m) => m.start)
      .firstOrNull;
  if (beginIdx == null) return null;
  final endIdx = RegExp(r'\bEND\b', caseSensitive: false)
      .allMatches(sql)
      .map((m) => m.start)
      .lastOrNull;
  if (endIdx == null) return null;
  final body = sql.substring(beginIdx + 'BEGIN'.length, endIdx).trim();
  return body;
}
