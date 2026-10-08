import 'drivers/db_driver.dart' show kMaterializedViewTypes;

/// 实体化视图(PostgreSQL 家族)的目录查询与 DDL 生成。
///
/// 与 [RoutineSql] / [UserSql] 同属「界面侧 SQL 工具」:设计器「SQL 预览」
/// 看到的就是保存时执行的语句。之所以不走驱动 `DatabaseDriver.getDefinition`:
/// 实体化视图的差异项(存储参数 / 访问方法 / 是否已填充 / 索引)全部由 PG 目录
/// 函数直接给出,而驱动的 getDefinition 只有一张 kind 文本表,为它加宽接口要
/// 同时改六个驱动 —— 目录查询留在界面侧更划算。
class MatViewSql {
  MatViewSql._();

  /// 该数据库类型有实体化视图概念(与树 / Ribbon 的分类门控同源)
  static bool supports(String typeId) =>
      kMaterializedViewTypes.contains(typeId);

  /// PG 家族默认模式
  static const String defaultSchema = 'public';

  /// `"schema"."name"`;[schema] 为空时落到 public
  static String qualifiedName(String name, {String? schema}) {
    final s = (schema == null || schema.isEmpty) ? defaultSchema : schema;
    return '${ident(s)}.${ident(name)}';
  }

  static String ident(String v) => '"${v.replaceAll('"', '""')}"';

  static String lit(String v) => "'${v.replaceAll("'", "''")}'";

  // ── 目录查询 ────────────────────────────────────────────

  /// 定义主体(SELECT 原文,不含 CREATE 头)。
  /// `pg_get_viewdef(oid, true)` 对 relkind='m' 同样有效,给的是美化过的多行查询。
  static String viewDefinitionSql(String name, {String? schema}) =>
      'SELECT pg_get_viewdef(c.oid, true) FROM pg_class c '
      'JOIN pg_namespace n ON n.oid = c.relnamespace '
      'WHERE n.nspname = ${lit(schema ?? defaultSchema)} '
      'AND c.relname = ${lit(name)} '
      "AND c.relkind = 'm'";

  /// 高级页要的归属与存储属性:所有者 / 表空间 / 访问方法 / 存储参数 /
  /// 是否已填充 / 注释。表空间与访问方法走 LEFT JOIN:未显式指定时
  /// reltablespace / relam 为 0,目录里没有对应行,退化成空串(界面显示「默认」)。
  static String propsSql(String name, {String? schema}) =>
      'SELECT COALESCE(pg_get_userbyid(c.relowner), \'\'), '
      'COALESCE(t.spcname, \'\'), '
      'COALESCE(a.amname, \'\'), '
      'COALESCE(array_to_string(c.reloptions, \',\'), \'\'), '
      'CASE WHEN mv.ispopulated THEN \'true\' ELSE \'false\' END, '
      'COALESCE(d.description, \'\') '
      'FROM pg_class c '
      'JOIN pg_namespace n ON n.oid = c.relnamespace '
      'LEFT JOIN pg_tablespace t ON t.oid = c.reltablespace '
      'LEFT JOIN pg_am a ON a.oid = c.relam '
      'LEFT JOIN pg_matviews mv ON mv.schemaname = n.nspname '
      'AND mv.matviewname = c.relname '
      'LEFT JOIN pg_description d ON d.objoid = c.oid AND d.objsubid = 0 '
      'WHERE n.nspname = ${lit(schema ?? defaultSchema)} '
      'AND c.relname = ${lit(name)} '
      "AND c.relkind = 'm'";

  /// propsSql 的一行 → [MatViewProps];列数不足(版本差异)时按缺省补齐
  static MatViewProps parseProps(List<String> row) {
    String at(int i) => i < row.length ? row[i] : '';
    return MatViewProps(
      owner: at(0),
      tablespace: at(1),
      accessMethod: at(2),
      storageOptions: splitOptions(at(3)),
      populated: at(4) != 'false',
      comment: at(5),
    );
  }

  /// `reloptions` 数组文本(逗号分隔的 `k=v`)→ 逐项列表;空串给空列表
  static List<String> splitOptions(String arrayText) {
    final s = arrayText.trim();
    if (s.isEmpty) return const [];
    return [
      for (final part in s.split(','))
        if (part.trim().isNotEmpty) part.trim(),
    ];
  }

  /// 存储参数列表 → `WITH (k = v, ...) 片段;无参数时返回空串
  static String storageOptionsClause(List<String> options) {
    final cleaned = [
      for (final o in options)
        if (o.trim().isNotEmpty) normalizeOption(o),
    ];
    if (cleaned.isEmpty) return '';
    return 'WITH (${cleaned.join(', ')})';
  }

  /// `fillfactor=90` → `fillfactor = 90`(PG 两种写法都收,统一成易读的一种)
  static String normalizeOption(String option) {
    final i = option.indexOf('=');
    if (i < 0) return option.trim();
    final key = option.substring(0, i).trim();
    final value = option.substring(i + 1).trim();
    return value.isEmpty ? key : '$key = $value';
  }

  /// 界面「存储参数」输入框的一行文本 → 逐项列表(与 [storageOptionsClause] 往返)
  static List<String> parseOptionsText(String text) =>
      splitOptions(text.replaceAll('\n', ','));

  /// 输出列清单(索引页填列名时的提示)
  static String columnsSql(String name, {String? schema}) =>
      'SELECT a.attname FROM pg_attribute a '
      'JOIN pg_class c ON c.oid = a.attrelid '
      'JOIN pg_namespace n ON n.oid = c.relnamespace '
      'WHERE n.nspname = ${lit(schema ?? defaultSchema)} '
      'AND c.relname = ${lit(name)} '
      'AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum';

  /// 已有索引:`pg_get_indexdef` 给的是可直接回放的完整 CREATE INDEX 语句。
  /// 编辑保存要 DROP + CREATE 重建实体化视图,索引不在 CREATE 子句里,
  /// 靠这份原文在重建后回放,否则用户的选择性索引会静默消失。
  static String indexesSql(String name, {String? schema}) =>
      'SELECT cl.relname, pg_get_indexdef(cl.oid), '
      'CASE WHEN i.indisunique THEN \'true\' ELSE \'false\' END, '
      'CASE WHEN i.indisvalid THEN \'true\' ELSE \'false\' END '
      'FROM pg_index i '
      'JOIN pg_class cl ON cl.oid = i.indexrelid '
      'JOIN pg_class t ON t.oid = i.indrelid '
      'JOIN pg_namespace n ON n.oid = t.relnamespace '
      'WHERE n.nspname = ${lit(schema ?? defaultSchema)} '
      'AND t.relname = ${lit(name)} '
      'ORDER BY cl.relname';

  /// indexesSql 的一行 → [MatViewIndexRow]
  static MatViewIndexRow parseIndexRow(List<String> row) {
    String at(int i) => i < row.length ? row[i] : '';
    return MatViewIndexRow(
      name: at(0),
      definition: at(1),
      unique: at(2) == 'true',
      valid: at(3) != 'false',
    );
  }

  /// 能否并发刷新:PG 要求存在一个**无 WHERE 条件且覆盖全部列**的唯一索引。
  /// 用 `indnatts >= 列数` 近似(PG 11+ 起有 indnatts);判定偏松不影响正确性 ——
  /// 真不满足时服务端会给出准确报错,界面不臆断。
  static String concurrentlyReadySql(String name, {String? schema}) {
    final target = '(SELECT c.oid FROM pg_class c '
        'JOIN pg_namespace n ON n.oid = c.relnamespace '
        'WHERE n.nspname = ${lit(schema ?? defaultSchema)} '
        'AND c.relname = ${lit(name)} '
        "AND c.relkind = 'm')";
    return 'SELECT EXISTS (SELECT 1 FROM pg_index i '
        'WHERE i.indrelid = $target AND i.indisunique AND i.indpred IS NULL '
        'AND i.indnatts >= (SELECT count(*) FROM pg_attribute a '
        'WHERE a.attrelid = i.indrelid AND a.attnum > 0 '
        'AND NOT a.attisdropped))';
  }

  static const String ownersSql =
      'SELECT rolname FROM pg_roles ORDER BY rolname';

  static const String tablespacesSql =
      'SELECT spcname FROM pg_tablespace ORDER BY spcname';

  /// 可承载表对象的访问方法(`amtype = 't'`,排除 TOAST 等内部类型)
  static const String accessMethodsSql =
      "SELECT amname FROM pg_am WHERE amtype = 't' ORDER BY amname";

  /// 探测定义是否可用:PG 会解析并规划但不执行,语法 / 列错误当场暴露。
  /// 编辑保存前用它把关 —— 重建是破坏性的,不能让「DROP 成功、CREATE 失败」
  /// 把对象删掉留下残局。
  static String explainSql(String select) => 'EXPLAIN ${stripTrailingSemicolon(select)}';

  /// 去掉尾部空白与分号(驱动按单条语句执行)
  static String stripTrailingSemicolon(String sql) {
    var s = sql.trim();
    while (s.endsWith(';')) {
      s = s.substring(0, s.length - 1).trimRight();
    }
    return s;
  }

  /// 取结果集首行首列(目录查询都只回一行一列);无行/无列返回 null
  static String? firstCell(List<List<String>> rows) {
    if (rows.isEmpty || rows.first.isEmpty) return null;
    final v = rows.first.first.trim();
    return v.isEmpty ? null : v;
  }

  /// `SELECT EXISTS (...)` 的结果 → 三态。驱动把布尔渲染成 't' / 'true' / '1'
  /// 因版本而异,都认;读不出返回 null(未知),调用方据此放行给服务端判定。
  static bool? parseExists(List<List<String>> rows) {
    if (rows.isEmpty || rows.first.isEmpty) return null;
    final v = rows.first.first;
    if (v == 'f' || v == 'false' || v == '0') return false;
    if (v == 't' || v == 'true' || v == '1') return true;
    return null;
  }

  // ── DDL ─────────────────────────────────────────────────

  /// CREATE MATERIALIZED VIEW。子句顺序按 PG 文法:
  /// `名称 [USING 方法] [WITH (参数)] [TABLESPACE 表空间] AS 查询 [WITH [NO] DATA]`
  static String createSql(MatViewSpec spec) {
    final q = qualifiedName(spec.name, schema: spec.schema);
    final buf = StringBuffer('CREATE MATERIALIZED VIEW $q');
    final am = spec.accessMethod?.trim() ?? '';
    if (am.isNotEmpty) buf.write(' USING ${ident(am)}');
    final withClause = storageOptionsClause(spec.storageOptions);
    if (withClause.isNotEmpty) buf.write(' $withClause');
    final ts = spec.tablespace?.trim() ?? '';
    if (ts.isNotEmpty) buf.write(' TABLESPACE ${ident(ts)}');
    buf.write(' AS ${stripTrailingSemicolon(spec.select)}');
    if (!spec.withData) buf.write(' WITH NO DATA');
    buf.write(';');
    return buf.toString();
  }

  static String dropSql(String name, {String? schema, bool ifExists = true}) =>
      'DROP MATERIALIZED VIEW ${ifExists ? 'IF EXISTS ' : ''}'
      '${qualifiedName(name, schema: schema)};';

  static String refreshSql(String name,
          {String? schema, bool concurrently = false}) =>
      'REFRESH MATERIALIZED VIEW '
      '${concurrently ? 'CONCURRENTLY ' : ''}'
      '${qualifiedName(name, schema: schema)};';

  static String commentSql(String name, String comment, {String? schema}) =>
      'COMMENT ON MATERIALIZED VIEW ${qualifiedName(name, schema: schema)} '
      'IS ${lit(comment)};';

  /// 所有者为空串时返回空串(界面「默认 / 不变」一档),调用方据此跳过执行
  static String ownerSql(String name, String owner, {String? schema}) {
    final o = owner.trim();
    if (o.isEmpty) return '';
    return 'ALTER MATERIALIZED VIEW ${qualifiedName(name, schema: schema)} '
        'OWNER TO ${ident(o)};';
  }

  /// 索引页「添加索引」:列名按逗号拆分后逐段引用,`a, b` 与 `a,b` 同义。
  /// 索引名与实体化视图同模式(索引是 schema 级对象)。
  static String createIndexSql(String matView, String indexName, String columns,
      {String? schema, bool unique = false, String method = 'btree'}) {
    final cols = [
      for (final c in columns.split(','))
        if (c.trim().isNotEmpty) ident(c.trim()),
    ];
    final n = indexName.trim();
    if (n.isEmpty || cols.isEmpty) return '';
    final idx = qualifiedName(n, schema: schema);
    final q = qualifiedName(matView, schema: schema);
    return '${unique ? 'CREATE UNIQUE INDEX ' : 'CREATE INDEX '}$idx '
        'ON $q USING ${ident(method)} (${cols.join(', ')});';
  }

  static String dropIndexSql(String indexName, {String? schema}) =>
      'DROP INDEX ${qualifiedName(indexName, schema: schema)};';

  /// 转储 / 设计页共用的完整语句块:CREATE + 索引 + 归属 + 注释。
  /// [indexes] 为重建后回放的索引原文(编辑场景由调用方在 DROP 前取到)。
  static String fullScript({
    required MatViewSpec spec,
    String? dropFirst,
    List<String> indexes = const [],
    String owner = '',
    String comment = '',
  }) {
    final buf = StringBuffer();
    if (dropFirst != null && dropFirst.isNotEmpty) {
      buf.writeln(dropFirst);
      buf.writeln();
    }
    buf.writeln(createSql(spec));
    for (final idx in indexes) {
      buf.writeln();
      buf.writeln(idx.trimRight().endsWith(';') ? idx : '$idx;');
    }
    final ownerSqlText = ownerSql(spec.name, owner, schema: spec.schema);
    if (ownerSqlText.isNotEmpty) {
      buf.writeln();
      buf.writeln(ownerSqlText);
    }
    if (comment.trim().isNotEmpty) {
      buf.writeln();
      buf.writeln(commentSql(spec.name, comment.trim(), schema: spec.schema));
    }
    return buf.toString().trimRight();
  }
}

/// 高级页采集的创建期选项(字段多且逐项可空,故单独成类)
class MatViewSpec {
  const MatViewSpec({
    required this.name,
    this.schema,
    required this.select,
    this.tablespace,
    this.accessMethod,
    this.storageOptions = const [],
    this.withData = true,
  });

  final String name;
  final String? schema;

  /// 定义主体:只存 SELECT,CREATE 头由 [MatViewSql.createSql] 拼
  final String select;
  final String? tablespace;
  final String? accessMethod;

  /// `fillfactor = 90` 形式的存储参数项
  final List<String> storageOptions;

  /// false = `WITH NO DATA`(建完不填充,不可查询)
  final bool withData;
}

/// 实体化视图当前归属与存储属性(由 [MatViewSql.propsSql] 反查)
class MatViewProps {
  const MatViewProps({
    this.owner = '',
    this.tablespace = '',
    this.accessMethod = '',
    this.storageOptions = const [],
    this.populated = true,
    this.comment = '',
  });

  final String owner;

  /// 空串 = 未显式指定,继承库默认表空间
  final String tablespace;

  /// 空串 = 未指定访问方法(走默认 heap)
  final String accessMethod;
  final List<String> storageOptions;

  /// false = 当前是 `WITH NO DATA` 状态,数据页查询会失败
  final bool populated;
  final String comment;
}

/// 实体化视图上的一条索引
class MatViewIndexRow {
  const MatViewIndexRow({
    required this.name,
    required this.definition,
    this.unique = false,
    this.valid = true,
  });

  final String name;

  /// 可直接回放的 CREATE INDEX 原文
  final String definition;
  final bool unique;

  /// false = 索引处于失效状态(并发建索引中断等),回放前应提醒用户
  final bool valid;
}
