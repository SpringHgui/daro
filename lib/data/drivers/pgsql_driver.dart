import 'package:postgres/postgres.dart' hide ConnectionInfo;

import '../database_edit_catalog.dart';
import '../db_data.dart';
import '../db_metadata.dart';
import '../sql_row_cap.dart';
import '../table_design.dart';
import '../user_sql.dart';
import 'db_driver.dart';

/// PostgreSQL 驱动(纯 Dart 实现,基于 postgres 包)。
///
/// 持有单条长连接,元数据查询与数据预览共用同一连接;
/// 连接断开时由上层([ConnectionManager])负责重建。
class PgsqlDriver implements DatabaseDriver {
  PgsqlDriver(this._conn);

  final ConnectionInfo _conn;
  Connection? _connection;

  /// 当前连接的库(null = 未切换过,用连接配置的默认库)
  String? _database;

  /// 当前会话的模式(null = 未显式切换,走连接默认 search_path)
  String? _schema;

  /// `server_version_num`,用于目录列名的版本适配(见 database_edit_catalog)。
  /// 一次连接内不会变,缓存起来免得每个详情都多问一次。
  int? _serverVersion;

  @override
  bool get isConnected => _connection != null && _connection!.isOpen;

  @override
  Future<void> connect() async {
    if (isConnected) return;
    final conn = await Connection.open(
      Endpoint(
        host: _conn.host,
        port: int.tryParse(_conn.port) ?? 5432,
        database:
            _database ?? (_conn.database.isEmpty ? 'postgres' : _conn.database),
        username: _conn.username,
        password: _conn.password,
      ),
      settings: const ConnectionSettings(
        // 本地 / 内网 PostgreSQL 通常未启用 SSL
        sslMode: SslMode.disable,
        connectTimeout: Duration(seconds: 10),
      ),
    );
    _connection = conn;
  }

  @override
  Future<void> close() async {
    final conn = _connection;
    _connection = null;
    if (conn != null && conn.isOpen) {
      await conn.close();
    }
  }

  Connection _get() {
    if (_connection == null || !_connection!.isOpen) {
      throw Exception('PostgreSQL 连接不可用');
    }
    return _connection!;
  }

  /// 双引号包裹标识符,防库/表名与关键字冲突
  String _quoted(String name) => '"${name.replaceAll('"', '""')}"';

  /// 单引号包裹字符串字面量(模式名作为 SQL 字符串比较值使用)
  String _lit(String s) => "'${s.replaceAll("'", "''")}'";

  /// 执行查询并把每行折成 `每列 toString()`(null → 空串)。
  /// 只读元数据用,行数很小,不做流式处理。
  Future<List<List<String>>> _rows(String sql) async {
    final result = await _get().execute(sql);
    return [
      for (final row in result)
        [for (final v in row) v?.toString() ?? ''],
    ];
  }

  /// PG 的布尔列折成 bool(`t` / `true` / `1`)
  bool _boolOf(String v) {
    final s = v.trim().toLowerCase();
    return s == 't' || s == 'true' || s == '1';
  }

  /// 模式名兜底:未显式切换时用默认模式 public
  String get _schemaOrPublic => _schema ?? 'public';

  @override
  Future<List<String>> listDatabases() async {
    final conn = _get();
    final result = await conn.execute(
      'SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname',
    );
    return [
      for (final row in result)
        if (row[0] != null) row[0].toString(),
    ];
  }

  @override
  Future<List<String>> listSchemas(String database) async {
    final conn = _get();
    // 排除系统模式:pg_catalog / information_schema / pg_temp_* / pg_toast*
    final result = await conn.execute(
      "SELECT nspname FROM pg_namespace "
      "WHERE nspname NOT IN ('information_schema', 'pg_catalog') "
      "AND nspname !~ '^pg_' "
      "ORDER BY nspname",
    );
    return [
      for (final row in result)
        if (row[0] != null) row[0].toString(),
    ];
  }

  @override
  Future<List<String>> listTables(String database, {String? schema}) async {
    final conn = _get();
    // 使用 information_schema 查询指定模式的用户表
    final result = await conn.execute(
      "SELECT table_name FROM information_schema.tables "
      "WHERE table_schema = ${_lit(schema ?? _schemaOrPublic)} "
      "AND table_type = 'BASE TABLE' "
      "ORDER BY table_name",
    );
    return [
      for (final row in result)
        if (row[0] != null) row[0].toString(),
    ];
  }

  @override
  Future<List<String>> listViews(String database, {String? schema}) async {
    final conn = _get();
    final result = await conn.execute(
      "SELECT table_name FROM information_schema.views "
      "WHERE table_schema = ${_lit(schema ?? _schemaOrPublic)} ORDER BY table_name",
    );
    return [
      for (final row in result)
        if (row[0] != null) row[0].toString(),
    ];
  }

  /// 执行「名称列 + 描述列」两列查询,聚成 名称 → 注释(trim 后)的 Map。
  Future<Map<String, String>> _objectComments(String sql) async {
    final conn = _get();
    final result = await conn.execute(sql);
    return {
      for (final row in result)
        if (row[0] != null) row[0].toString(): (row[1]?.toString() ?? '').trim(),
    };
  }

  // PG 的对象注释统一存于 pg_description,objsubid=0 表示对象级(非列级)注释。
  // 表 / 视图来自 pg_class(relkind r / v),函数来自 pg_proc,均按 nspname 过滤。
  @override
  Future<Map<String, String>> listTableComments(String database,
          {String? schema}) =>
      _objectComments(
          "SELECT c.relname, d.description FROM pg_class c "
          "JOIN pg_namespace n ON n.oid = c.relnamespace "
          "LEFT JOIN pg_description d ON d.objoid = c.oid AND d.objsubid = 0 "
          "WHERE n.nspname = ${_lit(schema ?? _schemaOrPublic)} "
          "AND c.relkind = 'r'");

  @override
  Future<Map<String, String>> listViewComments(String database,
          {String? schema}) =>
      _objectComments(
          "SELECT c.relname, d.description FROM pg_class c "
          "JOIN pg_namespace n ON n.oid = c.relnamespace "
          "LEFT JOIN pg_description d ON d.objoid = c.oid AND d.objsubid = 0 "
          "WHERE n.nspname = ${_lit(schema ?? _schemaOrPublic)} "
          "AND c.relkind = 'v'");

  @override
  Future<Map<String, int>> listTableRowEstimates(String database,
      {String? schema}) async {
    final conn = _get();
    // reltuples 是统计信息里的估算行数(由 autoanalyze 增量维护,不扫描数据);
    // -1 表示该表从未统计过 → 不返回键,界面显示横杠。分区表的父表 reltuples
    // 恒为 -1 / 0,同样落进「无估算值」一档。
    final result = await conn.execute(
      "SELECT c.relname, c.reltuples::bigint FROM pg_class c "
      "JOIN pg_namespace n ON n.oid = c.relnamespace "
      "WHERE n.nspname = ${_lit(schema ?? _schemaOrPublic)} "
      "AND c.relkind IN ('r', 'm') AND c.reltuples >= 0",
    );
    final out = <String, int>{};
    for (final row in result) {
      final name = row[0]?.toString();
      final rows = parseRowCount(row[1]);
      if (name != null && rows != null) out[name] = rows;
    }
    return out;
  }

  @override
  Future<Map<String, String>> listFunctionComments(String database,
          {String? schema}) =>
      _objectComments(
          "SELECT p.proname, d.description FROM pg_proc p "
          "JOIN pg_namespace n ON n.oid = p.pronamespace "
          "LEFT JOIN pg_description d ON d.objoid = p.oid AND d.objsubid = 0 "
          "WHERE n.nspname = ${_lit(schema ?? _schemaOrPublic)}");

  @override
  Future<List<String>> listMaterializedViews(String database,
      {String? schema}) async {
    final conn = _get();
    final result = await conn.execute(
      "SELECT matviewname FROM pg_matviews "
      "WHERE schemaname = ${_lit(schema ?? _schemaOrPublic)} "
      "ORDER BY matviewname",
    );
    return [
      for (final row in result)
        if (row[0] != null) row[0].toString(),
    ];
  }

  @override
  Future<List<String>> listFunctions(String database, {String? schema}) async {
    final conn = _get();
    final result = await conn.execute(
      "SELECT routine_name FROM information_schema.routines "
      "WHERE routine_schema = ${_lit(schema ?? _schemaOrPublic)} "
      "AND routine_type = 'FUNCTION' "
      "ORDER BY routine_name",
    );
    return [
      for (final row in result)
        if (row[0] != null) row[0].toString(),
    ];
  }

  @override
  Future<List<String>> listProcedures(String database, {String? schema}) async {
    final conn = _get();
    // PostgreSQL 11+ 才支持存储过程;低版本该查询自然返回空
    final result = await conn.execute(
      "SELECT routine_name FROM information_schema.routines "
      "WHERE routine_schema = ${_lit(schema ?? _schemaOrPublic)} "
      "AND routine_type = 'PROCEDURE' "
      "ORDER BY routine_name",
    );
    return [
      for (final row in result)
        if (row[0] != null) row[0].toString(),
    ];
  }

  @override
  Future<List<String>> listUsers(String database) async {
    final conn = _get();
    final result = await conn.execute(
      "SELECT rolname FROM pg_roles WHERE rolcanlogin = true ORDER BY rolname",
    );
    return [
      for (final row in result)
        if (row[0] != null) row[0].toString(),
    ];
  }

  // ── 角色详情(「用户 / 角色」设计页) ──────────────────────

  /// 读取角色属性(`pg_roles`)。
  ///
  /// 口令本身读不到(PG 只暴露 `rolpassword` 的哈希,且非超级用户看不到),
  /// 故编辑模式下密码恒为空 = 不改密码;`rolvaliduntil` 回填到「高级」页。
  @override
  Future<UserSpec?> readUser(String database, String account) async {
    final name = account.trim();
    if (name.isEmpty) return null;
    final rows = await _rows(
      'SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, '
      'rolreplication, rolbypassrls, rolconnlimit, '
      "COALESCE(to_char(rolvaliduntil, 'YYYY-MM-DD HH24:MI:SS'), ''), "
      "COALESCE(shobj_description(oid, 'pg_authid'), '') "
      'FROM pg_roles WHERE rolname = ${_lit(name)}',
    );
    if (rows.isEmpty) return null;
    final r = rows.first;
    return UserSpec(
      originalName: name,
      username: name,
      password: '',
      isRole: !_boolOf(r[0]),
      comment: r[9],
      pg: PgRoleAttributes(
        canLogin: _boolOf(r[0]),
        superUser: _boolOf(r[1]),
        createDb: _boolOf(r[2]),
        createRole: _boolOf(r[3]),
        inherit: _boolOf(r[4]),
        replication: _boolOf(r[5]),
        bypassRls: _boolOf(r[6]),
        connectionLimit: int.tryParse(r[7]) ?? -1,
        validUntil: r[8],
      ),
    );
  }

  /// 该角色可加入的角色候选(排除自身)
  @override
  Future<List<String>> listGrantableRoles(String database) async {
    final rows = await _rows('SELECT rolname FROM pg_roles ORDER BY rolname');
    return [
      for (final r in rows)
        if (r.isNotEmpty && r.first.isNotEmpty) r.first,
    ];
  }

  /// PostgreSQL 的权限模型是 ACL(`aclitem[]`),与 MySQL 的 `*_priv` 布尔列
  /// 矩阵完全不同,本页暂不呈现 → 返回空列表,界面隐藏「服务器权限 / 权限」。
  @override
  Future<List<List<String>>> readUserPrivileges(
    String database,
    String account, {
    bool serverLevel = false,
  }) async =>
      const [];

  /// 该角色当前的成员关系(`pg_auth_members`:本角色 ∈ 哪些组)
  @override
  Future<List<String>> readUserRoles(String database, String account) async {
    final name = account.trim();
    if (name.isEmpty) return const [];
    final rows = await _rows(
      'SELECT g.rolname FROM pg_auth_members m '
      'JOIN pg_roles r ON r.oid = m.member '
      'JOIN pg_roles g ON g.oid = m.roleid '
      'WHERE r.rolname = ${_lit(name)} ORDER BY g.rolname',
    );
    return [
      for (final r in rows)
        if (r.isNotEmpty && r.first.isNotEmpty) r.first,
    ];
  }

  /// 属于该角色的成员(反向)
  @override
  Future<List<String>> readRoleMembers(String database, String account) async {
    final name = account.trim();
    if (name.isEmpty) return const [];
    final rows = await _rows(
      'SELECT m.rolname FROM pg_auth_members am '
      'JOIN pg_roles r ON r.oid = am.roleid '
      'JOIN pg_roles m ON m.oid = am.member '
      'WHERE r.rolname = ${_lit(name)} ORDER BY m.rolname',
    );
    return [
      for (final r in rows)
        if (r.isNotEmpty && r.first.isNotEmpty) r.first,
    ];
  }

  /// 序列 = `pg_class(relkind='S')`,并**排除归属列**的那些。
  ///
  /// `serial` 与 `GENERATED … AS IDENTITY` 背后都会自动建一条序列,它由建表语句
  /// (列上的自增属性)一起生成,当成独立对象再同步一遍就重复了。`pg_depend` 里
  /// `deptype='a'`(serial 的 OWNED BY)与 `'i'`(identity 的内部依赖)正是这批。
  @override
  Future<List<String>> listSequences(String database, {String? schema}) async {
    final conn = _get();
    final result = await conn.execute(
      "SELECT c.relname FROM pg_class c "
      "JOIN pg_namespace n ON n.oid = c.relnamespace "
      "WHERE n.nspname = ${_lit(schema ?? _schemaOrPublic)} "
      "AND c.relkind = 'S' "
      "AND NOT EXISTS (SELECT 1 FROM pg_depend d "
      "WHERE d.objid = c.oid AND d.classid = 'pg_class'::regclass "
      "AND d.refclassid = 'pg_class'::regclass "
      "AND d.deptype IN ('a', 'i')) "
      "ORDER BY c.relname",
    );
    return [
      for (final row in result)
        if (row[0] != null) row[0].toString(),
    ];
  }

  @override
  Future<SequenceDef?> readSequence(String database, String name,
      {String? schema}) async {
    final conn = _get();
    final sch = schema ?? _schemaOrPublic;
    // pg_sequences(PG 10+):参数与最后值一次取齐。
    // `last_value` 在「本次启动/重置后还没取过值」时为 NULL,不能当差异比。
    final result = await conn.execute(
      "SELECT data_type, start_value, min_value, max_value, increment_by, "
      "cycle, cache_size, last_value FROM pg_sequences "
      "WHERE schemaname = ${_lit(sch)} AND sequencename = ${_lit(name)}",
    );
    if (result.isEmpty) return null;
    final row = result.first;
    String? at(int i) => row[i]?.toString();
    final dataType = at(0);
    final start = at(1);
    final min = at(2);
    final max = at(3);
    final inc = at(4) ?? '1';
    final cycle = at(5)?.toLowerCase() == 'true';
    final cache = at(6);

    // 重建 CREATE SEQUENCE:两侧都走这条重建逻辑,文本即可直接比对。
    final buf = StringBuffer('CREATE SEQUENCE ')
      ..write(DdlBuilder.qualified('postgresql', sch, name));
    if (dataType != null && dataType.isNotEmpty) buf.write(' AS $dataType');
    if (start != null) buf.write(' START WITH $start');
    buf.write(' INCREMENT BY $inc');
    if (min != null) buf.write(' MINVALUE $min');
    if (max != null) buf.write(' MAXVALUE $max');
    if (cache != null) buf.write(' CACHE $cache');
    buf.write(cycle ? ' CYCLE' : ' NO CYCLE');

    return SequenceDef(
      createSql: buf.toString(),
      lastValue: at(7),
      increment: inc,
    );
  }

  @override
  Future<TablePreview> previewTable(
    String database,
    String table, {
    int limit = 100,
    int offset = 0,
    String? schema,
    String? where,
    String? orderBy,
  }) async {
    final conn = _get();
    // schema 为空时不限定(走 search_path,默认 public);
    // 非空时显式限定 "schema"."table",避免查到其它模式的同名表
    final qualified = schema == null || schema.isEmpty
        ? _quoted(table)
        : '${_quoted(schema)}.${_quoted(table)}';
    final result = await conn.execute(
      'SELECT * FROM $qualified'
      '${whereClauseSql(where)}${orderByClauseSql(orderBy)}'
      ' LIMIT $limit OFFSET $offset',
    );

    // postgres 包的 Result.schema.columns 提供列元信息
    final columns = [
      for (final col in result.schema.columns)
        if (col.columnName != null) col.columnName!,
    ];

    final rows = <List<String>>[];
    // nullMask 记录每格的原始 null 判定:展示层里真 NULL 与字符串 "NULL"
    // 都是 "NULL",导出 / 导入必须靠它区分
    final nullMask = <List<bool>>[];
    for (final row in result) {
      final cells = <String>[];
      final nulls = <bool>[];
      for (var i = 0; i < columns.length; i++) {
        final v = row[i];
        cells.add(v?.toString() ?? 'NULL');
        nulls.add(v == null);
      }
      rows.add(cells);
      nullMask.add(nulls);
    }
    return TablePreview(
        columns: columns, rows: rows, limit: limit, nullMask: nullMask);
  }

  @override
  Future<int> countTable(String database, String table,
      {String? schema, String? where}) async {
    final conn = _get();
    // 与 previewTable 相同的模式限定逻辑
    final qualified = schema == null || schema.isEmpty
        ? _quoted(table)
        : '${_quoted(schema)}.${_quoted(table)}';
    final result = await conn.execute(
      'SELECT COUNT(*) AS cnt FROM $qualified${whereClauseSql(where)}',
    );
    return parseCountValue(result.first[0]);
  }

  @override
  Future<void> useDatabase(String database) async {
    if (_database == database ||
        (_database == null && database == _conn.database)) {
      return;
    }
    // PostgreSQL 会话绑定单一库:切换库需要断开重连
    final old = _connection;
    _connection = null;
    await old?.close();
    _database = database;
    // 重连后回到连接默认 search_path,旧模式选择失效
    _schema = null;
    await connect();
  }

  @override
  Future<void> useSchema(String? schema) async {
    if (_schema == schema) return;
    if (schema == null) {
      // 恢复会话默认 search_path(连接 / 角色级配置)
      await _get().execute('RESET search_path');
    } else {
      await _get().execute('SET search_path TO ${_quoted(schema)}');
    }
    _schema = schema;
  }

  @override
  Future<QueryResult> executeQuery(String sql,
      {int limit = 1000, int offset = 0}) async {
    final conn = _get();
    // 封顶必须下推到服务端:postgres 包的 Result 是已物化的 List<ResultRow>,
    // 在调用方 break 救不回来(详见 sql_row_cap.dart);offset 同理由服务端跳过
    final capped = capSelectSql(sql, maxRows: limit + 1, offset: offset);
    final result = await conn.execute(capped ?? sql);

    final columns = [
      for (final col in result.schema.columns)
        if (col.columnName != null && col.columnName!.isNotEmpty)
          col.columnName!,
    ];
    if (columns.isEmpty) {
      // 写操作:无结果集,返回受影响行数
      return QueryResult(
        columns: const [],
        rows: const [],
        affectedRows: result.affectedRows,
        limit: limit,
        offset: offset,
      );
    }

    final rows = <List<String>>[];
    for (final row in result) {
      if (rows.length >= limit) break;
      rows.add([
        for (var i = 0; i < columns.length; i++) row[i]?.toString() ?? 'NULL',
      ]);
    }
    return QueryResult(
      columns: columns,
      rows: rows,
      limit: limit,
      offset: offset,
      // 服务端只被允许返回 limit + 1 行,多出的那行即「还有更多」的确证
      moreRows: capped != null && result.length > limit,
    );
  }

  /// 元数据归一化时使用的方言 id([kPgLikeTypes] 的任一成员即可,
  /// 解析层只按方言族区分)
  static const String _dialect = 'postgresql';

  @override
  Future<int?> serverSessionId() async {
    final result = await _get().execute('SELECT pg_backend_pid()');
    final v = result.first[0];
    return v is int ? v : int.tryParse(v?.toString() ?? '');
  }

  @override
  Future<void> killSession(int sessionId) async {
    // 主连接正被 executeQuery 的 await 占住 → 用第二条临时连接发 pg_cancel_backend
    final killer = await Connection.open(
      Endpoint(
        host: _conn.host,
        port: int.tryParse(_conn.port) ?? 5432,
        database: _conn.database.isEmpty ? 'postgres' : _conn.database,
        username: _conn.username,
        password: _conn.password,
      ),
      settings: const ConnectionSettings(
        sslMode: SslMode.disable,
        connectTimeout: Duration(seconds: 10),
      ),
    );
    try {
      await killer.execute('SELECT pg_cancel_backend($sessionId)');
    } finally {
      await killer.close();
    }
  }

  @override
  Future<List<ColumnDef>> describeTable(String database, String table,
      {String? schema}) async {
    final conn = _get();
    // col_description 需要 pg_class 的 OID,information_schema 里没有,
    // 只能按 (nspname, relname) JOIN pg_namespace / pg_class 后取
    final result = await conn.execute(
      "SELECT c.column_name, c.data_type, c.is_nullable, c.column_default, "
      "col_description(pgc.oid, c.ordinal_position::int) "
      "FROM information_schema.columns c "
      "LEFT JOIN pg_namespace pgn ON pgn.nspname = c.table_schema "
      "LEFT JOIN pg_class pgc "
      "  ON pgc.relnamespace = pgn.oid AND pgc.relname = c.table_name "
      "WHERE c.table_schema = ${_lit(schema ?? _schemaOrPublic)} "
      "AND c.table_name = '${table.replaceAll("'", "''")}' "
      "ORDER BY c.ordinal_position",
    );
    return [
      for (final row in result)
        ColumnDef(
          name: row[0]?.toString() ?? '',
          type: row[1]?.toString() ?? '',
          nullable: (row[2]?.toString() ?? 'YES') == 'YES',
          defaultValue: row[3]?.toString(),
          comment: row[4]?.toString() ?? '',
        ),
    ];
  }

  /// 「设计表」反查:列 / 主键 / 索引 / 外键 / 唯一键 / 检查 / 排除 / 注释 / 存储参数。
  ///
  /// 一律走 `pg_*` 系统目录:identity 序列选项、索引方法、约束真名都无法从
  /// `information_schema` 取得。子查询均为单条语句,失败即抛出由界面展示错误。
  @override
  Future<DesignTable?> readTableDesign(String database, String table,
      {String? schema}) async {
    final conn = _get();
    final sch = _lit(schema ?? _schemaOrPublic);
    final tbl = _lit(table);
    final design = DesignTable()
      ..name = table
      ..schema = schema;

    // ── 列 ──────────────────────────────────────────────────
    // 类型取 format_type 原文(含长度 / 精度 / 数组维度);默认值取
    // pg_get_expr 原文(information_schema 会丢表达式细节);
    // attidentity: a = ALWAYS、d = BY DEFAULT、空 = 非 identity。
    final colResult = await conn.execute(
      'SELECT a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull, '
      'pg_get_expr(d.adbin, d.adrelid), a.attidentity, '
      'col_description(a.attrelid, a.attnum), '
      '(SELECT collname FROM pg_collation WHERE oid = a.attcollation) '
      'FROM pg_attribute a '
      'JOIN pg_class cl ON cl.oid = a.attrelid '
      'JOIN pg_namespace nc ON nc.oid = cl.relnamespace '
      'LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum '
      'WHERE nc.nspname = $sch AND cl.relname = $tbl '
      'AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum',
    );
    for (final row in colResult) {
      final rawType = row[1]?.toString() ?? '';
      // 数组:格式为 `int4[]`,先剥维度标记再归一化类型名,维数单独回写
      final scalarType = rawType.replaceAll('[]', '');
      final dims = RegExp(r'\[\]').allMatches(rawType).length;
      final split = splitColumnType(scalarType);
      design.columns.add(
        DesignColumn(
          name: row[0]?.toString() ?? '',
          type: baseTypeOf(scalarType, _dialect),
          length: split.length,
          decimal: split.decimal,
          notNull: row[2] == true || row[2]?.toString() == 't',
          comment: row[5]?.toString() ?? '',
          // nextval / currval 默认值由 identity 建模承载,不重复写 DEFAULT
          defaultValue: normaliseDefault(row[3]?.toString(), _dialect) ?? '',
          dimension: dims == 0 ? '' : '$dims',
          collation: _collationOf(row[6]?.toString()),
          identityMode: switch (_text(row[4])) {
            'a' => 'ALWAYS',
            'd' => 'BY DEFAULT',
            _ => '',
          },
        ),
      );
    }

    // ── identity 序列选项 ────────────────────────────────────
    // pg_sequence 需 PG 10+,且需序列的读权限;失败只影响序列选项目展示,
    // 不能连带整个反查失败(退化为“仅不预填序列选项”)。
    if (design.columns.any((c) => c.hasIdentity)) {
      try {
        final seqResult = await conn.execute(
          'SELECT a.attname, q.seqincrement, q.seqstart, q.seqmin, q.seqmax, '
          'q.seqcache, q.seqcycle '
          'FROM pg_attribute a '
          'JOIN pg_class cl ON cl.oid = a.attrelid '
          'JOIN pg_namespace nc ON nc.oid = cl.relnamespace '
          'JOIN pg_sequence q ON q.seqrelid = to_regclass('
          "pg_get_serial_sequence(format('%I.%I', nc.nspname, cl.relname), "
          'a.attname)) '
          'WHERE nc.nspname = $sch AND cl.relname = $tbl '
          "AND a.attnum > 0 AND a.attidentity <> ''",
        );
        for (final row in seqResult) {
          final name = row[0]?.toString() ?? '';
          for (final col in design.columns) {
            if (col.name != name) continue;
            col.identityIncrement = _text(row[1]);
            col.identityStart = _text(row[2]);
            col.identityMinValue = _text(row[3]);
            col.identityMaxValue = _text(row[4]);
            col.identityCache = _text(row[5]);
            col.identityCycle = row[6] == true || row[6]?.toString() == 't';
          }
        }
      } catch (_) {
        // 序列选项读不到:保留 identity 模式,选项留空由服务端取默认
      }
    }

    // ── 约束(主键 / 唯一 / 外键 / 检查 / 排除) ──────────────
    final conResult = await conn.execute(
      'SELECT con.conname, con.contype, '
      "(SELECT string_agg(pa.attname, ',' ORDER BY k.ord) "
      'FROM generate_subscripts(con.conkey, 1) AS k(ord) '
      'JOIN pg_attribute pa ON pa.attrelid = con.conrelid '
      'AND pa.attnum = con.conkey[k.ord]), '
      "CASE WHEN con.contype = 'f' THEN "
      "(SELECT string_agg(ra.attname, ',' ORDER BY k.ord) "
      'FROM generate_subscripts(con.confkey, 1) AS k(ord) '
      'JOIN pg_attribute ra ON ra.attrelid = con.confrelid '
      'AND ra.attnum = con.confkey[k.ord]) END, '
      "CASE WHEN con.contype = 'f' THEN "
      '(SELECT rn.nspname FROM pg_class rc '
      'JOIN pg_namespace rn ON rn.oid = rc.relnamespace '
      'WHERE rc.oid = con.confrelid) END, '
      "CASE WHEN con.contype = 'f' THEN "
      '(SELECT rc.relname FROM pg_class rc WHERE rc.oid = con.confrelid) END, '
      'con.confdeltype, con.confupdtype, con.confmatchtype, '
      'con.condeferrable, con.condeferred, '
      "CASE WHEN con.contype IN ('c', 'x') THEN pg_get_constraintdef(con.oid) "
      'END, '
      // 约束注释(`COMMENT ON CONSTRAINT`):设计表的外键「注释」列需回显
      "shobj_description(con.oid, 'pg_constraint'), "
      // NO INHERIT 标记:pg_get_constraintdef 会把它拼进定义,但定义串还要
      // 用来取表达式,单独读列比再解析一遍尾巴可靠。
      'con.conconnoinherit '
      'FROM pg_constraint con '
      'JOIN pg_class cl ON cl.oid = con.conrelid '
      'JOIN pg_namespace nc ON nc.oid = cl.relnamespace '
      'WHERE nc.nspname = $sch AND cl.relname = $tbl ORDER BY con.conname',
    );
    final pkColumns = <String>{};
    for (final row in conResult) {
      final name = _text(row[0]);
      final type = _text(row[1]);
      final cols = _text(row[2]);
      switch (type) {
        case 'p':
          design.pkName = name;
          pkColumns.addAll(_nameList(cols));
        case 'u':
          design.uniqueKeys.add(DesignUniqueKey(
              name: name, columns: cols, comment: _text(row[12])));
        case 'f':
          design.foreignKeys.add(DesignForeignKey(
            name: name,
            columns: cols,
            refColumns: _text(row[3]),
            refSchema: _text(row[4]),
            refTable: _text(row[5]),
            onDelete: _fkAction(row[6]),
            onUpdate: _fkAction(row[7]),
            // confmatchtype: f = MATCH FULL、p = PARTIAL、s = SIMPLE(默认)
            matchAll: _text(row[8]) == 'f',
            deferrable: _text(row[9]).isEmpty
                ? ''
                : (_text(row[9]) == 't' ? 'YES' : 'NO'),
            deferred: _text(row[10]) == 't' ? 'YES' : 'NO',
            comment: _text(row[12]),
          ));
        case 'c':
          final def = _text(row[11]);
          design.checks.add(
            DesignCheck(
                name: name,
                expression: _checkExpr(def),
                noInherit: _boolOf(_text(row[13])),
                comment: _text(row[12])),
          );
        case 'x':
          final parsed = parsePgExcludeDef(_text(row[11]));
          design.excludes.add(DesignExclude(
            name: name,
            method: parsed.method,
            columns: parsed.elements,
            predicate: parsed.predicate,
            deferrable: _text(row[9]).isEmpty
                ? ''
                : (_text(row[9]) == 't' ? 'YES' : 'NO'),
            deferred: _text(row[10]) == 't' ? 'YES' : 'NO',
            comment: _text(row[12]),
          ));
      }
    }
    if (pkColumns.isNotEmpty) {
      for (final col in design.columns) {
        col.primaryKey = pkColumns.contains(col.name);
      }
    }

    // ── 索引(排除约束支撑的索引已由上面作为约束展示) ────────
    // indkey 是 int2vector,下标从 0 开始,而 pg_get_indexdef 的列号从 1 开始:
    // 不 +1 会让首列拿到 0(= 整条索引定义原文),反查出的「字段」就是一句
    // CREATE INDEX,部署时必然报 42703 column does not exist。
    // (约束那边是直接 conkey[k.ord] 下标取数,0 基正好,无需偏移。)
    final idxResult = await conn.execute(
      'SELECT c.relname, am.amname, '
      "(SELECT string_agg(pg_get_indexdef(i.indexrelid, k.ord + 1, true), ',' "
      'ORDER BY k.ord) FROM generate_subscripts(i.indkey, 1) AS k(ord)), '
      'i.indisunique, '
      "(SELECT split_part(o, '=', 2) FROM unnest(c.reloptions) AS o "
      "WHERE o LIKE 'fillfactor=%' LIMIT 1), "
      '(SELECT t2.spcname FROM pg_tablespace t2 WHERE t2.oid = c.reltablespace), '
      // 索引自己的注释(`COMMENT ON INDEX`)
      "obj_description(c.oid, 'pg_class') "
      'FROM pg_index i '
      'JOIN pg_class c ON c.oid = i.indexrelid '
      'JOIN pg_am am ON am.oid = c.relam '
      'JOIN pg_class t ON t.oid = i.indrelid '
      'JOIN pg_namespace nc ON nc.oid = t.relnamespace '
      'WHERE nc.nspname = $sch AND t.relname = $tbl '
      'AND NOT EXISTS (SELECT 1 FROM pg_constraint con '
      'WHERE con.conindid = i.indexrelid) ORDER BY c.relname',
    );
    for (final row in idxResult) {
      design.indexes.add(DesignIndex(
        name: _text(row[0]),
        method: _text(row[1]),
        columns: _text(row[2]),
        unique: row[3] == true || row[3]?.toString() == 't',
        fillFactor: _text(row[4]),
        tablespace: _text(row[5]),
        comment: _text(row[6]),
      ));
    }

    // ── 表注释与存储参数 ──────────────────────────────────
    // 表选项(relpersistence / owner / inherits / cluster)在编辑模式不可改,仍得
    // 回显:否则选项页看起来像“这张表没有所有者”。
    final tableResult = await conn.execute(
      'SELECT obj_description(c.oid), '
      "(SELECT split_part(o, '=', 2) FROM unnest(c.reloptions) AS o "
      "WHERE o LIKE 'fillfactor=%' LIMIT 1), "
      '(SELECT ts.spcname FROM pg_tablespace ts WHERE ts.oid = c.reltablespace), '
      "c.relpersistence, pg_get_userbyid(c.relowner), "
      "(SELECT string_agg(pn.nspname || '.' || pc.relname, ', ') "
      'FROM pg_inherits ih '
      'JOIN pg_class pc ON pc.oid = ih.inhparent '
      'JOIN pg_namespace pn ON pn.oid = pc.relnamespace '
      'WHERE ih.inhrelid = c.oid), '
      '(SELECT ic.relname FROM pg_index ci JOIN pg_class ic '
      'ON ic.oid = ci.indexrelid WHERE ci.indrelid = c.oid '
      'AND ci.indisclustered LIMIT 1), '
      // 表级存储参数(`WITH (...)`):整组取回,界面「选项」按清单逐项回显,
      // 清单外的项(如服务端新加的 reloption)也原样保留
      'c.reloptions '
      'FROM pg_class c '
      'JOIN pg_namespace nc ON nc.oid = c.relnamespace '
      "WHERE nc.nspname = $sch AND c.relname = $tbl "
      "AND c.relkind IN ('r', 'p')",
    );
    if (tableResult.isNotEmpty) {
      final row = tableResult.first;
      design.tableComment = _text(row[0]);
      design.fillFactor = _text(row[1]);
      design.tablespace = _text(row[2]);
      // relpersistence: u = UNLOGGED、p = PERMANENT、t = TEMPORARY
      design.unlogged = _text(row[3]) == 'u';
      design.owner = _text(row[4]);
      design.inherits = _text(row[5]);
      design.cluster = _text(row[6]);
      for (final o in (row[7] as List? ?? const <Object>[])) {
        final e = o.toString();
        final eq = e.indexOf('=');
        if (eq <= 0) continue;
        design.setStorageOption(e.substring(0, eq).trim(), e.substring(eq + 1).trim());
      }
    }

    // ── 触发器 ────────────────────────────────────────────────
    // 只取用户触发器(`NOT tgisinternal`):外键自动生成的
    // `RI_ConstraintTrigger_*` 由约束自身表达,放进设计器会变成一堆
    // 用户没写过的行,而且删约束时它们会跟着消失(反查与依赖页的口径一致)。
    //
    // 用 `pg_get_triggerdef(oid, true)` 拿规范化 DDL 再反解析,而不是逐列读
    // `tgtype` 位掩码:位掩码要自己拼 BEFORE/AFTER/INSTEAD OF + 七个事件位,
    // 拼错了不报错、只是静默生成错 DDL;函数给的是服务端认定的真实定义。
    try {
      final trgResult = await conn.execute(
        'SELECT tg.tgname, pg_get_triggerdef(tg.oid, true), '
        'tg.tgenabled '
        'FROM pg_catalog.pg_trigger tg '
        'JOIN pg_catalog.pg_class c ON c.oid = tg.tgrelid '
        'JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace '
        'WHERE n.nspname = $sch AND c.relname = $tbl '
        'AND NOT tg.tgisinternal '
        'ORDER BY tg.tgname',
      );
      for (final row in trgResult) {
        final parsed = parsePgTriggerDef(
          _text(row[0]),
          _text(row[1]),
          enabled: _text(row[2]) != 'D',
        );
        if (parsed != null) design.triggers.add(parsed);
      }
    } catch (_) {
      // 触发器读不到(权限 / 非 PG 家族服务端):不预填即可,界面仍可手工新增。
      // 与序列选项同一取舍:附加信息失败不牵连整次反查。
    }
    // ── 规则 ──────────────────────────────────────────────────
    // 表上的用户规则都在 pg_rewrite 里(ev_type: n/i/d/a/s),视图自动生成的
    // `_RETURN` 与内部规则一并排除 —— 反查回来的必须是用户写得出来、也删得掉的那些。
    // 与触发器同一思路:用 pg_get_ruledef 拿服务端认定的定义再反解析。
    try {
      final ruleResult = await conn.execute(
        'SELECT r.rulename, pg_get_ruledef(r.oid), '
        "shobj_description(r.oid, 'pg_rewrite'), "
        // ev_enabled:d = DISABLE RULE 关掉的,其余(o / b / R)都算启用
        'r.ev_enabled '
        'FROM pg_catalog.pg_rewrite r '
        'JOIN pg_catalog.pg_class c ON c.oid = r.ev_class '
        'JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace '
        'WHERE n.nspname = $sch AND c.relname = $tbl '
        "AND r.rulename <> '_RETURN' AND NOT pg_catalog.is_internal(r.oid) "
        'ORDER BY r.rulename',
      );
      for (final row in ruleResult) {
        final parsed = parsePgRuleDef(
          _text(row[0]),
          _text(row[1]),
          comment: _text(row[2]),
          enabled: _text(row[3]).toLowerCase() != 'd',
        );
        if (parsed != null) design.rules.add(parsed);
      }
    } catch (_) {
      // 规则读不到(权限 / 服务端版本):与触发器同一取舍,不牵连整次反查。
    }
    return design;
  }

  // ── 详情面板:库 / 表属性与依赖关系 ─────────────────────────────────────
  // 三条查询都只读系统目录,不碰数据页;选中节点时触发一次,由
  // ConnectionManager 缓存(见其 databaseDetail / tableDetail / tableDependencies)。

  @override
  Future<DatabaseDetail?> readDatabaseDetail(String database) async {
    final conn = _get();
    // 库属性 SQL 与「编辑数据库」对话框共用:列序是那边的解析契约,
    // 这里只消费解析结果,不另写一份(两份各自适配 PG 18 的列名改名迟早会走偏)
    final props = parseDatabasePropsRow(
      await _textRows(conn.execute(
          pgDatabasePropsSql(database,
              pg18Plus: ((await _serverVersionNum()) ?? 0) >=
                  kPgEncodingColumnRenamedVersion))),
      database,
    );
    if (props == null) return null;
    // OID 不在那条查询里(它不属于表单字段),单独取一次:pg_database 是共享
    // 目录,一行一列,代价可忽略
    final oid = await conn.execute(
      'SELECT oid::text FROM pg_catalog.pg_database '
      'WHERE datname = ${_lit(database)}',
    );
    return DatabaseDetail(
      name: database,
      charset: props.encoding,
      collation: props.lcCollate,
      oid: oid.isEmpty ? '' : _text(oid.first[0]),
      owner: props.owner,
      tablespace: props.tablespace,
      connectionLimit: '${props.connectionLimit}',
      comment: props.comment,
    );
  }

  @override
  Future<TableDetail?> readTableDetail(String database, String table,
      {String? schema}) async {
    final conn = _get();
    final result = await conn.execute(
      'SELECT c.oid::text, pg_catalog.pg_get_userbyid(c.relowner), c.relkind, '
      'c.reltuples::bigint, c.relispartition, '
      "(SELECT string_agg(pn.nspname || '.' || pc.relname, ', ' "
      'ORDER BY ih.inhseqno) '
      'FROM pg_catalog.pg_inherits ih '
      'JOIN pg_catalog.pg_class pc ON pc.oid = ih.inhparent '
      'JOIN pg_catalog.pg_namespace pn ON pn.oid = pc.relnamespace '
      'WHERE ih.inhrelid = c.oid), '
      '(SELECT ts.spcname FROM pg_catalog.pg_tablespace ts '
      'WHERE ts.oid = c.reltablespace), '
      "(SELECT split_part(o, '=', 2) FROM unnest(c.reloptions) AS o "
      "WHERE o LIKE 'fillfactor=%' LIMIT 1), "
      // ACL 逐行拼接用 chr(10):写 E'\n' 的话 Dart 会先把 \n 变成真换行
      'array_to_string(c.relacl, chr(10)), '
      "obj_description(c.oid, 'pg_class'), "
      "'with_oids' = ANY(c.reloptions) "
      'FROM pg_catalog.pg_class c '
      'JOIN pg_catalog.pg_namespace nc ON nc.oid = c.relnamespace '
      'WHERE nc.nspname = ${_lit(schema ?? _schemaOrPublic)} '
      'AND c.relname = ${_lit(table)}',
    );
    if (result.isEmpty) return null;
    final row = result.first;
    String at(int i) => _text(row[i]);
    bool truthy(int i) {
      final v = at(i);
      return v == 't' || v == 'true';
    }

    // 同一个 pg_inherits 结果按 relispartition 分流:分区表的父表是「分区属于」,
    // 普通继承才是「Inherits From」——两者混显示会误导(Navicat 也分开)
    final parents = at(5);
    final isPartition = truthy(4);
    final est = int.tryParse(at(3));
    return TableDetail(
      name: table,
      oid: at(0),
      owner: at(1),
      tableType: at(2),
      // reltuples = -1 是「从未 ANALYZE」的哨兵,不是「这张表有 -1 行」
      rowEstimate: (est == null || est < 0) ? null : est,
      partitionOf: isPartition ? parents : '',
      inheritsFrom: isPartition ? '' : parents,
      tablespace: at(6),
      fillFactor: at(7),
      acl: at(8),
      comment: at(9),
      // PG 12 起 WITH OIDS 被移除,reloptions 里永远不会再出现该项 → false
      hasOids: truthy(10),
    );
  }

  @override
  Future<List<DependentObject>?> readTableDependencies(
      String database, String table,
      {String? schema, bool usedBy = true}) async {
    final conn = _get();
    final sch = _lit(schema ?? _schemaOrPublic);
    final tbl = _lit(table);
    // 方向只换两组列名:「被使用」列的是引用本表的那些对象(refobjid 指向本表),
    // 「使用」列的是本表引用的对象(objid 是本表)。解析目录 objs 两向复用。
    final (srcCls, srcId) =
        usedBy ? ('d.classid', 'd.objid') : ('d.refclassid', 'd.refobjid');
    final (selfCls, selfId) =
        usedBy ? ('d.refclassid', 'd.refobjid') : ('d.classid', 'd.objid');
    final rows = await conn.execute(
      'WITH t AS (SELECT c.oid FROM pg_catalog.pg_class c '
      'JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace '
      'WHERE n.nspname = $sch AND c.relname = $tbl), '
      // 本表名下的约束:表对其它表的依赖不直接记在表上,而是记在它的
      // 外键 / 检查约束上(pg_depend 里 classid = pg_constraint)
      'cons AS (SELECT k.oid FROM pg_catalog.pg_constraint k, t '
      'WHERE k.conrelid = t.oid), '
      'objs AS ($kPgDependObjsSql) '
      // 同一条依赖会按列重复记(引用到的每一列一行),DISTINCT 折成对象级。
      // `::text` 不是排版需要:objid 走扩展协议会按二进制 oid 返回,
      // Dart 侧拿到的是 4 字节 UndecodedBytes,直接解码就抛 FormatException。
      'SELECT DISTINCT $srcId::text AS id, o.sch, o.name, o.kind, d.deptype '
      'FROM pg_catalog.pg_depend d '
      'CROSS JOIN t '
      'JOIN objs o ON o.cls = $srcCls::oid AND o.id = $srcId '
      // 依赖的一端是「本表」或「本表的约束」
      "WHERE (($selfCls = 'pg_catalog.pg_class'::regclass::oid "
      'AND $selfId = t.oid) '
      "OR ($selfCls = 'pg_catalog.pg_constraint'::regclass::oid "
      'AND $selfId IN (SELECT oid FROM cons))) '
      // 自依赖(表依赖自己的列、自己的外键指回自己)不是依赖关系
      "AND NOT ($srcCls = 'pg_catalog.pg_class'::regclass::oid "
      'AND $srcId = t.oid) '
      // 系统模式下的对象(TOAST 表等)不给用户看
      "AND (o.sch IS NULL OR o.sch NOT IN "
      "('pg_catalog', 'information_schema', 'pg_toast')) "
      'ORDER BY o.sch NULLS FIRST, o.name',
    );

    final entries = <(DependentObject, String)>[];
    for (final row in rows) {
      entries.add((
        DependentObject(
          schema: _text(row[1]),
          name: _text(row[2]),
          kind: _text(row[3]),
          degree: _pgDependDegree(_text(row[4])),
        ),
        _text(row[0]),
      ));
    }

    // 约束的内部触发器:PG 给每个外键偷偷建 4 个 RI_ConstraintTrigger_*,
    // 它们不出现在 pg_depend 的对象级依赖里,只能按 tgconstraint 挂回父约束下
    final children = <String, List<DependentObject>>{};
    final conIds = [
      for (final (o, id) in entries)
        if (_pgConstraintKinds.contains(o.kind) && int.tryParse(id) != null) id,
    ];
    if (conIds.isNotEmpty) {
      final trg = await conn.execute(
        'SELECT tg.tgconstraint::text, tg.tgname, n.nspname '
        'FROM pg_catalog.pg_trigger tg '
        'JOIN pg_catalog.pg_class c ON c.oid = tg.tgrelid '
        'JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace '
        'WHERE tg.tgisinternal AND tg.tgconstraint IN (${conIds.join(',')}) '
        'ORDER BY tg.oid',
      );
      for (final row in trg) {
        children
            .putIfAbsent(_text(row[0]), () => [])
            .add(DependentObject(
              schema: _text(row[2]),
              name: _text(row[1]),
              kind: 'TRIGGER',
              degree: 'INTERNAL',
            ));
      }
    }

    return [
      for (final (o, id) in entries)
        if ((children[id] ?? const []).isNotEmpty)
          DependentObject(
            schema: o.schema,
            name: o.name,
            kind: o.kind,
            degree: o.degree,
            children: children[id]!,
          )
        else
          o,
    ];
  }

  /// 结果集 → 纯字符串矩阵:让 `database_edit_catalog` 里那套按 `List<List<String>>`
  /// 写好的解析器能被驱动与 `runQuery` 两条路径共用。
  Future<List<List<String>>> _textRows(Future<Result> query) async {
    final result = await query;
    return [
      for (final row in result) [for (final v in row) _text(v)],
    ];
  }

  /// `server_version_num`(缓存一次);读不到返回 null,调用方按旧版列名兜底。
  Future<int?> _serverVersionNum() async {
    if (_serverVersion != null) return _serverVersion;
    final rows = await _textRows(_get().execute(kPgServerVersionSql));
    return _serverVersion = parseServerVersion(rows);
  }

  /// 设计器下拉候选:排序规则 / 运算符类别 / 表空间均直读系统目录。
  /// 三者都是实例级对象(不按库过滤);名称去重后按字典序返回,候选可达
  /// 上千行(ICU 排序规则),ComboBox 弹层为 `ListView.builder`,不致于卡顿。
  @override
  Future<DesignCandidates> readDesignCandidates(String database) async {
    final conn = _get();
    Future<List<String>> names(String sql) async {
      final rows = await conn.execute(sql);
      return [for (final row in rows) row[0]?.toString() ?? '']
          .where((e) => e.isNotEmpty)
          .toList();
    }

    return DesignCandidates(
      collations:
          await names('SELECT DISTINCT collname FROM pg_collation ORDER BY 1'),
      opClasses:
          await names('SELECT DISTINCT opcname FROM pg_opclass ORDER BY 1'),
      tablespaces:
          await names('SELECT spcname FROM pg_tablespace ORDER BY 1'),
    );
  }

  /// 标量值 → 字符串(null / 空统一为空串,供设计器“留空 = 不输出子句”)
  ///
  /// `"char"`(oid 18,如 `contype` / `attidentity` / `relpersistence`)postgres
  /// 驱动没有注册编解码器,取回来的是 [UndecodedBytes];它的 `toString()` 是
  /// `Instance of 'UndecodedBytes'`,所有字母码分支都会静默落空(主键 / 外键 /
  /// identity 全部消失)。必须走 `asString` 把字节按连接编码解出来。
  static String _text(Object? v) {
    if (v is UndecodedBytes) return v.asString;
    return v?.toString() ?? '';
  }

  /// 逗号分隔的列名串 → 列表(pg_get_indexdef 己按标识符规则加引号)
  static List<String> _nameList(String s) => s
      .split(',')
      .map((e) => e.trim())
      .where((e) => e.isNotEmpty)
      .map((e) => e.replaceAll(RegExp(r'^"|"$'), '').replaceAll('""', '"'))
      .toList();

  /// 列使用了数据库默认排序规则时不回填(避免保存时多出 `COLLATE "default"`)
  static String _collationOf(String? v) {
    final s = v?.trim() ?? '';
    return (s.isEmpty || s == 'default') ? '' : s;
  }

  /// confdeltype / confupdtype 字母码 → SQL 动作关键字
  static String _fkAction(Object? v) => switch (_text(v)) {
        'a' => 'NO ACTION',
        'r' => 'RESTRICT',
        'c' => 'CASCADE',
        'n' => 'SET NULL',
        'd' => 'SET DEFAULT',
        _ => 'NO ACTION',
      };

  /// `CHECK ((expr))` → `expr`(保留内部的类型转换与括号)
  static String _checkExpr(String def) {
    var s = def.trim();
    if (s.length > 6 && s.substring(0, 6).toUpperCase() == 'CHECK ') {
      s = s.substring(6).trim();
    }
    return stripRedundantParens(s);
  }

  @override
  Future<String?> getDefinition(String database, String name, String kind,
      {String? schema}) async {
    final conn = _get();
    // 带 schema 限定的对象引用文本(pg_get_viewdef / regproc 均支持)
    final ref = schema == null || schema.isEmpty
        ? _quoted(name)
        : '${_quoted(schema)}.${_quoted(name)}';
    if (kind == 'table') {
      // PG 没有 `SHOW CREATE TABLE`,只能由目录数据重建。走设计器那套模型
      // (readTableDesign + DdlBuilder)而不是另写一份拼装器:结构同步、
      // 表设计器与详情面板从此输出同一份 DDL。
      final design = await readTableDesign(database, name, schema: schema);
      if (design == null) return null;
      return DdlBuilder.buildPreview(design, _conn.typeId);
    }
    if (kind == 'database') {
      final props = parseDatabasePropsRow(
        await _textRows(conn.execute(pgDatabasePropsSql(name,
            pg18Plus: ((await _serverVersionNum()) ?? 0) >=
                kPgEncodingColumnRenamedVersion))),
        name,
      );
      if (props == null) return null;
      // 编码 / 排序规则建库后不可改,但「按现状导出一份能重建同构库」的
      // DDL 仍要写出来(Navicat 的 DDL 页就是这么给的)
      final clauses = <String>[
        if (props.owner.isNotEmpty) 'OWNER = ${_quoted(props.owner)}',
        if (props.encoding.isNotEmpty) 'ENCODING = ${_lit(props.encoding)}',
        if (props.lcCollate.isNotEmpty) 'LC_COLLATE = ${_lit(props.lcCollate)}',
        if (props.lcCtype.isNotEmpty) 'LC_CTYPE = ${_lit(props.lcCtype)}',
        if (props.tablespace.isNotEmpty && props.tablespace != 'pg_default')
          'TABLESPACE = ${_quoted(props.tablespace)}',
        if (props.connectionLimit != -1)
          'CONNECTION LIMIT = ${props.connectionLimit}',
      ];
      return [
        'CREATE DATABASE ${_quoted(name)}'
            '${clauses.isEmpty ? '' : '\nWITH ' + clauses.join('\n     ')};',
        if (props.comment.isNotEmpty)
          "COMMENT ON DATABASE ${_quoted(name)} IS ${_lit(props.comment)};",
      ].join('\n\n');
    }
    if (kind == 'view') {
      final result = await conn.execute(
        "SELECT 'CREATE OR REPLACE VIEW ${_quoted(name)} AS ' "
        "|| pg_get_viewdef('$ref', true)",
      );
      if (result.isEmpty) return null;
      final v = result.first[0];
      return v == null ? null : v.toString();
    } else {
      final result = await conn.execute(
        "SELECT pg_get_functiondef('$ref'::regproc)",
      );
      if (result.isEmpty) return null;
      final v = result.first[0];
      return v == null ? null : v.toString();
    }
  }
}

/// `pg_depend` 的对象解析表:把 `(classid, objid)` 二元组翻译成「模式 / 名称 / 类型」。
///
/// pg_depend 只记 OID,不含任何可读名,且引用方可能是十几张目录表中的任意一张,
/// 所以依赖页必须有一次「全目录 UNION」。这里覆盖 Navicat 依赖页会列出的类型:
/// 表 / 索引 / 序列 / 视图 / 物化视图 / 外部表 / 分区表 / 约束 / 类型 / 函数 /
/// 排序规则 / 转换 / 默认值 / 模式 / 角色。
///
/// 两个容易踩的点:
/// - 角色依赖在 pg_depend 里记的 classid 是 **pg_authid**(`pg_roles` 只是它的视图,
///   没有独立 OID),所以 `cls` 用 pg_authid 而数据从 `pg_roles` 取;
///   且 `pg_authid` 在 PG 16+ 对非超级用户**不可读**,直接查它会整页报权限错。
/// - `relkind` / `contype` 是 `"char"`,与 regclass 的 oid 比较无碍,但 CASE 要按
///   字母码写死,不能依赖驱动的解码结果。
///
/// 不含末尾分号:调用方以 `objs AS ($kPgDependObjsSql)` 嵌进 CTE。
const String kPgDependObjsSql = '''
SELECT 'pg_catalog.pg_class'::regclass::oid AS cls, c.oid AS id,
       n.nspname AS sch, c.relname AS name,
       CASE c.relkind
         WHEN 'i' THEN 'INDEX'
         WHEN 'I' THEN 'PARTITIONED INDEX'
         WHEN 'S' THEN 'SEQUENCE'
         WHEN 'v' THEN 'VIEW'
         WHEN 'm' THEN 'MATERIALIZED VIEW'
         WHEN 'f' THEN 'FOREIGN TABLE'
         WHEN 'p' THEN 'PARTITIONED TABLE'
         WHEN 't' THEN 'TOAST TABLE'
         ELSE 'TABLE' END AS kind
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
UNION ALL
SELECT 'pg_catalog.pg_constraint'::regclass::oid, con.oid,
       n.nspname, con.conname,
       CASE con.contype
         WHEN 'p' THEN 'PRIMARY KEY'
         WHEN 'u' THEN 'UNIQUE'
         WHEN 'f' THEN 'FOREIGN KEY'
         WHEN 'c' THEN 'CHECK'
         WHEN 'x' THEN 'EXCLUSION'
         WHEN 't' THEN 'NOT NULL'
         ELSE 'CONSTRAINT' END
FROM pg_catalog.pg_constraint con
LEFT JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
LEFT JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
UNION ALL
SELECT 'pg_catalog.pg_type'::regclass::oid, t.oid, n.nspname, t.typname, 'TYPE'
FROM pg_catalog.pg_type t
JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace
UNION ALL
SELECT 'pg_catalog.pg_proc'::regclass::oid, p.oid, n.nspname, p.proname, 'FUNCTION'
FROM pg_catalog.pg_proc p
JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
UNION ALL
SELECT 'pg_catalog.pg_collation'::regclass::oid, cl.oid, n.nspname, cl.collname,
       'COLLATION'
FROM pg_catalog.pg_collation cl
JOIN pg_catalog.pg_namespace n ON n.oid = cl.collnamespace
UNION ALL
SELECT 'pg_catalog.pg_conversion'::regclass::oid, cv.oid, n.nspname, cv.conname,
       'CONVERSION'
FROM pg_catalog.pg_conversion cv
JOIN pg_catalog.pg_namespace n ON n.oid = cv.connamespace
UNION ALL
SELECT 'pg_catalog.pg_attrdef'::regclass::oid, ad.oid, n.nspname,
       c.relname || '_default', 'DEFAULT'
FROM pg_catalog.pg_attrdef ad
JOIN pg_catalog.pg_class c ON c.oid = ad.adrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
UNION ALL
SELECT 'pg_catalog.pg_namespace'::regclass::oid, n.oid, NULL, n.nspname, 'SCHEMA'
FROM pg_catalog.pg_namespace n
UNION ALL
SELECT 'pg_catalog.pg_authid'::regclass::oid, r.oid, NULL,
       r.rolname, 'ROLE'
FROM pg_catalog.pg_roles r''';

/// `pg_depend.deptype` 字母码 → 界面用的依赖性质标签(与 Navicat 用词一致)。
///
/// 传入前要先过驱动的 `_text()` 解码:`deptype` 是 `"char"`,
/// 直接 `toString()` 得到的是 `Instance of 'UndecodedBytes'`,分支会全落空。
String _pgDependDegree(String code) => switch (code) {
      'i' => 'INTERNAL',
      'a' => 'AUTO',
      'e' || 'x' => 'EXTENSION',
      _ => 'NORMAL',
    };

/// 会带内部触发器子项的约束类型(PG 给每个外键偷偷建 4 个
/// `RI_ConstraintTrigger_*`,`pg_depend` 不记它们,只能按 `tgconstraint` 挂回父约束)
const Set<String> _pgConstraintKinds = {
  'FOREIGN KEY',
  'PRIMARY KEY',
  'UNIQUE',
  'CHECK',
  'NOT NULL',
  'EXCLUSION',
};

/// 把 `pg_get_triggerdef()` 输出的一行 DDL 反解析成设计器用的 [DesignTrigger]。
///
/// 输入形如:
/// ```
/// CREATE TRIGGER trg AFTER INSERT OR UPDATE OF a ON t
///   FOR EACH ROW WHEN ((new.a > 0)) EXECUTE FUNCTION f('x')
/// ```
/// 可延迟触发器则是 `CREATE CONSTRAINT TRIGGER … DEFERRABLE INITIALLY DEFERRED`。
///
/// 解析失败返回 `null`(不把半条数据塞进设计器,宁可少一行让人肉眼看见)。
/// 全程大小写不敏感:PG 关键字输出为大写,但标识符与 WHEN 条件原样保留大小写。
DesignTrigger? parsePgTriggerDef(
  String name,
  String def, {
  bool enabled = true,
}) {
  if (name.trim().isEmpty || def.trim().isEmpty) return null;
  final s = def.trim();
  final upper = s.toUpperCase();

  // 1. CONSTRAINT TRIGGER 与 TRIGGER 都要认(可延迟的只能建成约束触发器)
  final isConstraint = upper.startsWith('CREATE CONSTRAINT TRIGGER ');
  final headLen = isConstraint
      ? 'CREATE CONSTRAINT TRIGGER '.length
      : 'CREATE TRIGGER '.length;
  if (!isConstraint && !upper.startsWith('CREATE TRIGGER ')) return null;

  // 2. 触发器名:声明的形状是
  //    `CREATE TRIGGER <name> <timing> <events> ON <table>`,
  //    名字在 `<timing>` 关键字之前结束 —— 不能截到 " ON " 为止,那样会把
  //    `<timing> <events>` 一起当成名字(实测踩过)。
  final head = upper.substring(headLen);
  final stop = <int>[
    head.indexOf(' BEFORE '),
    head.indexOf(' AFTER '),
    head.indexOf(' INSTEAD OF '),
  ].where((e) => e >= 0).fold<int>(-1, (a, b) => a < 0 ? b : (b < a ? b : a));
  if (stop < 0) return null; // 认不出时机关键字 → 不是可解析的 PG 触发器定义
  final timing = stop == head.indexOf(' BEFORE ')
      ? 'BEFORE'
      : (stop == head.indexOf(' INSTEAD OF ') ? 'INSTEAD OF' : 'AFTER');
  final declName = _pgUnquote(s.substring(headLen, headLen + stop).trim());
  if (declName.isEmpty) return null;

  // 事件清单的右边界:到 " ON " 为止
  final rest = s.substring(headLen + stop);
  final onIdx = _pgTopLevelOnIndex(rest);
  if (onIdx < 0) return null;

  // 3. 事件勾选(只在 `<timing> … ON` 这一段里找,函数体 / WHEN 条件里的
  //    INSERT / DELETE 字样不会被误判)
  var insert = false;
  var update = false;
  var delete = false;
  var truncate = false;
  final declSeg = rest.substring(0, onIdx).toUpperCase();
  for (final ev
      in RegExp(r'\b(INSERT|UPDATE|DELETE|TRUNCATE)\b').allMatches(declSeg)) {
    switch (ev.group(1)) {
      case 'INSERT':
        insert = true;
      case 'UPDATE':
        update = true;
      case 'DELETE':
        delete = true;
      case 'TRUNCATE':
        truncate = true;
    }
  }
  final updateColumns = _pgUpdateOfColumns(rest.substring(0, onIdx));

  // 5. FOR EACH ROW / STATEMENT
  final forEach =
      RegExp(r'FOR\s+EACH\s+STATEMENT', caseSensitive: false).hasMatch(s)
          ? '语句'
          : '行';

  // 6. WHEN (…) —— 括号配对,条件里可能自带括号
  var when = '';
  final whenMatch =
      RegExp(r'\bWHEN\s*\(', caseSensitive: false).firstMatch(s);
  if (whenMatch != null) {
    final open = s.indexOf('(', whenMatch.start);
    final close = _pgMatchParen(s, open);
    if (close > open) when = s.substring(open + 1, close).trim();
  }

  // 7. EXECUTE FUNCTION / PROCEDURE <fn>(args)
  var function = '';
  var parameters = '';
  final execMatch = RegExp(
    r'EXECUTE\s+(?:FUNCTION|PROCEDURE)\s+',
    caseSensitive: false,
  ).firstMatch(s);
  if (execMatch != null) {
    final tail = s.substring(execMatch.end).trim();
    final paren = tail.indexOf('(');
    if (paren < 0) {
      function = _pgUnquote(tail.replaceAll(';', '').trim());
    } else {
      function = _pgUnquote(tail.substring(0, paren).trim());
      final close = _pgMatchParen(tail, paren);
      if (close > paren) {
        parameters = tail.substring(paren + 1, close).trim();
      }
    }
  }

  // 8. 可延迟 / 延迟
  var deferrable = '';
  var deferred = '';
  if (isConstraint || upper.contains('DEFERRABLE')) {
    deferrable = 'YES';
    deferred = upper.contains('INITIALLY DEFERRED') ? 'YES' : 'NO';
  }

  return DesignTrigger(
    name: declName.isEmpty ? name.trim() : declName,
    forEach: forEach,
    timing: timing,
    insert: insert,
    update: update,
    delete: delete,
    truncate: truncate,
    updateColumns: updateColumns,
    enable: enabled,
    when: when,
    function: function,
    parameters: parameters,
    deferrable: deferrable,
    deferred: deferred,
  );
}

/// 从 `UPDATE OF col1, col2` 里取出列清单(逗号分隔);没有该子句返回空串。
/// 结果里去掉标识符引号,与设计器 [DesignTrigger.updateColumns] 的存储格式一致。
String _pgUpdateOfColumns(String s) {
  final m = RegExp(r'\bUPDATE\s+OF\s+', caseSensitive: false).firstMatch(s);
  if (m == null) return '';
  // 列清单一路读到下一个事件关键字(OR DELETE / OR INSERT / OR TRUNCATE)
  // 或 " ON " 为止
  final tail = s.substring(m.end);
  var cut = tail.length;
  for (final stop in [
    RegExp(r'\s+OR\s+', caseSensitive: false),
    RegExp(r'\s+ON\s+', caseSensitive: false),
    RegExp(r'\s+FOR\s+EACH\s+', caseSensitive: false),
    RegExp(r'\s+WHEN\s*\(', caseSensitive: false),
  ]) {
    final hit = stop.firstMatch(tail);
    if (hit != null && hit.start < cut) cut = hit.start;
  }
  return tail
      .substring(0, cut)
      .split(',')
      .map(_pgUnquote)
      .where((e) => e.isNotEmpty)
      .join(', ');
}

/// 找 `rest` 里第一个**不在双引号内**的 ` ON ` 起始位置(返回 'O' 的下标)。
/// 触发器名可能被引号包着并含空格(如 `"my trg on t"`),不能直接 indexOf。
int _pgTopLevelOnIndex(String rest) {
  var inQuote = false;
  for (var i = 0; i < rest.length; i++) {
    final c = rest[i];
    if (c == '"') {
      inQuote = !inQuote;
      continue;
    }
    if (inQuote) continue;
    if (i + 4 <= rest.length &&
        rest.substring(i, i + 4).toUpperCase() == ' ON ') {
      return i;
    }
  }
  return -1;
}

/// 去掉 PG 标识符的双引号并还原转义(`""` → `"`);未加引号的原样返回。
String _pgUnquote(String s) {
  var v = s.trim();
  if (v.length >= 2 && v.startsWith('"') && v.endsWith('"')) {
    v = v.substring(1, v.length - 1).replaceAll('""', '"');
  }
  return v.trim();
}

/// 从 [open] 处的 `(` 出发返回配对 `)` 的下标;找不到返回 -1。忽略引号内的括号。
int _pgMatchParen(String s, int open) {
  if (open < 0 || open >= s.length || s[open] != '(') return -1;
  var depth = 0;
  var inSingle = false;
  for (var i = open; i < s.length; i++) {
    final c = s[i];
    if (c == "'") {
      inSingle = !inSingle;
      continue;
    }
    if (inSingle) continue;
    if (c == '(') depth++;
    if (c == ')') {
      depth--;
      if (depth == 0) return i;
    }
  }
  return -1;
}

/// 解析 `pg_get_constraintdef` 给出的排除约束定义。
///
/// 形态:`EXCLUDE USING gist (元素 WITH 操作符, ...) [WHERE (谓词)]
/// [WITH (索引参数)] [TABLESPACE ts]`。括号一律配对扫描,因为排除元素里
/// 常带函数调用(`lower(name) WITH =`),用 `.*` 正则会一路吞到 WHERE 的括号。
({String method, String elements, String predicate}) parsePgExcludeDef(String def) {
  var s = def.trim();
  var method = '';
  final head = RegExp(r'^EXCLUDE\s+USING\s+(\S+)\s*', caseSensitive: false).firstMatch(s);
  if (head != null) {
    method = _pgUnquote(head.group(1)!);
    s = s.substring(head.end);
  } else {
    final bare = RegExp(r'^EXCLUDE\s*', caseSensitive: false).firstMatch(s);
    if (bare != null) s = s.substring(bare.end);
  }
  final open = s.indexOf('(');
  if (open < 0) return (method: method, elements: '', predicate: s.trim());
  final close = _pgMatchParen(s, open);
  if (close < 0) {
    return (method: method, elements: s.substring(open + 1).trim(), predicate: '');
  }
  final elements = s.substring(open + 1, close).trim();
  return (
    method: method,
    elements: elements,
    predicate: _pgParenContentAfter(s.substring(close + 1), r'\bWHERE\s*\('),
  );
}

/// 解析 `pg_get_ruledef` 给出的规则定义;认不出结构时返回 null(不猜)。
///
/// 形态:`CREATE RULE name AS ON INSERT TO tbl [WHERE (条件)]
/// DO ALSO (语句)` / `DO NOTHING`。规则挂在哪个表由设计器当前表决定,
/// 故 `TO` 与 `WHERE` / `DO` 之间那一段直接跳过。
DesignRule? parsePgRuleDef(String name, String def,
    {String comment = '', bool enabled = true}) {
  if (name.trim().isEmpty || def.trim().isEmpty) return null;
  final s = def.trim();
  final head = RegExp(
    r'^CREATE\s+RULE\s+(\S+)\s+AS\s+ON\s+(\w+)\s+TO\s+',
    caseSensitive: false,
  ).firstMatch(s);
  if (head == null) return null;
  final event = head.group(2)!.toUpperCase();
  final tail = s.substring(head.end);
  final doMatch = RegExp(r'\bDO\s+(ALSO|INSTEAD|NOTHING)\b', caseSensitive: false)
      .firstMatch(tail);
  if (doMatch == null) return null;
  // `TO 表名` 的右边界就是 WHERE / DO,取在先出现的那个
  final whereMatch =
      RegExp(r'\bWHERE\s*\(', caseSensitive: false).firstMatch(tail);
  final bodyStart = doMatch.end;
  final stmt = _pgParenContentAfter(tail.substring(bodyStart), r'^\s*\(');
  return DesignRule(
    name: _pgUnquote(head.group(1)!),
    event: event,
    qualification: whereMatch == null
        ? ''
        : _pgParenContentAfter(tail.substring(whereMatch.start), r'\bWHERE\s*\('),
    action: doMatch.group(1)!.toUpperCase(),
    statement: stmt,
    enable: enabled,
    comment: comment,
  );
}

/// 在 [s] 里找 [lead] 匹配到的第一个左括号,返回其配对括号内的内容(去掉
/// 服务端可能多加的一层外层括号)。找不到返回空串。
String _pgParenContentAfter(String s, String lead) {
  final m = RegExp(lead, caseSensitive: false).firstMatch(s);
  if (m == null) return '';
  final open = s.indexOf('(', m.start);
  if (open < 0) return '';
  final close = _pgMatchParen(s, open);
  if (close <= open) return '';
  var v = s.substring(open + 1, close).trim();
  // PG 把谓词写成 `WHERE ((a = b))`:外层括号是语法要求,不是用户输入的
  if (v.startsWith('(') && _pgMatchParen(v, 0) == v.length - 1) {
    v = v.substring(1, v.length - 1).trim();
  }
  return v;
}
