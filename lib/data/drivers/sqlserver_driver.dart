import 'dart:async';

import 'package:dart_odbc/dart_odbc.dart';

import '../db_data.dart';
import '../db_metadata.dart';
import '../table_design.dart';
import '../user_sql.dart';
import 'db_driver.dart';
import 'odbc_query.dart';

/// SQL Server 驱动,单一后端:系统 ODBC(dart_odbc)。
///
/// 通过 Windows ODBC 驱动管理器访问 SQL Server,天然同时支持:
/// - SQL Server 身份验证([ConnectionInfo.authMethod] != 'windows'):
///   连接串带 UID / PWD;
/// - Windows 身份验证('windows'):
///   连接串带 `Trusted_Connection=yes`,使用当前 Windows 账户集成认证。
///
/// 前提:本机已安装 SQL Server ODBC 驱动(优先 ODBC Driver 18/17 for SQL
/// Server,其次 Native Client 11 / 旧版 SQL Server 驱动,依次尝试)。
///
/// 持有单条长连接,元数据查询与数据预览共用同一连接;
/// 连接断开时由上层([ConnectionManager])负责重建。
class SqlServerDriver implements DatabaseDriver {
  SqlServerDriver(this._conn);

  final ConnectionInfo _conn;
  DartOdbc? _odbc;

  @override
  bool get isConnected => _odbc != null;

  /// 连接超时(dart_odbc 的 SQLDriverConnectW 无内置超时,
  /// 服务器不可达时会在 isolate 里无限阻塞,这里在 Dart 层兜底)
  static const _connectTimeout = Duration(seconds: 10);

  @override
  Future<void> connect() async {
    if (_odbc != null) return;
    // 常见 SQL Server ODBC 驱动名依次尝试,兼容不同本机安装
    const drivers = [
      'ODBC Driver 18 for SQL Server',
      'ODBC Driver 17 for SQL Server',
      'SQL Server Native Client 11.0',
      'SQL Server',
    ];
    String? lastError;
    for (final driver in drivers) {
      final odbc = DartOdbc();
      try {
        await odbc
            .connectWithConnectionString(_connectionString(driver))
            .timeout(_connectTimeout);
        _odbc = odbc;
        return;
      } on TimeoutException {
        // 底层 ODBC 调用在 isolate 里阻塞:不再复用该实例,
        // 后台尝试关闭其 isolate,避免遗留僵尸连接
        unawaited(odbc.disconnect().catchError((_) {}));
        throw Exception(
          '连接超时(${_connectTimeout.inSeconds} 秒无响应),'
          '请检查主机 / 端口 / 防火墙及服务器状态',
        );
      } catch (e) {
        lastError = e.toString();
        try {
          await odbc.disconnect();
        } catch (_) {}
      }
    }
    throw Exception(
      'SQL Server 连接失败(请确认本机已安装 SQL Server ODBC 驱动): '
      '$lastError',
    );
  }

  /// 组装 ODBC 连接串:按认证方式选择 Trusted_Connection 或 UID/PWD。
  /// 对齐原 mssql_connection 的 encrypt=false + trustServerCertificate=true,
  /// 关闭强制加密以兼容未配置证书的服务器
  String _connectionString(String driver) {
    final db = _conn.database.isEmpty ? 'master' : _conn.database;
    final port = _conn.port.isEmpty ? '1433' : _conn.port;
    final buf = StringBuffer()
      ..write('DRIVER={$driver};SERVER=${_conn.host},$port;DATABASE=$db;')
      ..write('Encrypt=No;TrustServerCertificate=Yes;');
    if (_conn.authMethod == 'windows') {
      buf.write('Trusted_Connection=yes;');
    } else {
      buf.write('UID=${_conn.username};PWD=${_conn.password};');
    }
    return buf.toString();
  }

  /// 统一 SQL 执行入口:返回 列名 + 行数据(列名 → 值)
  Future<({List<String> columns, List<Map<String, dynamic>> rows})>
      _runSql(String sql) async {
    final rows = await _get().execute(sql);
    final columns =
        rows.isEmpty ? const <String>[] : rows.first.keys.toList();
    return (columns: columns, rows: rows);
  }

  @override
  Future<void> close() async {
    final odbc = _odbc;
    _odbc = null;
    if (odbc != null) {
      try {
        await odbc.disconnect();
      } catch (_) {
        // 断开时忽略错误
      }
    }
  }

  DartOdbc _get() {
    final odbc = _odbc;
    if (odbc == null) {
      throw Exception('SQL Server 未连接');
    }
    return odbc;
  }

  /// 最近一次 USE 的库,避免每次执行都发 USE
  String? _currentDatabase;

  /// 方括号包裹标识符,防库/表名与关键字冲突
  String _quoted(String name) => '[${name.replaceAll(']', ']]')}]';

  /// 转义字符串字面量中的单引号(翻倍),调用处自行包裹单引号(与 mysql/mariadb 驱动一致)
  String _literal(String value) => value.replaceAll("'", "''");

  @override
  Future<List<String>> listDatabases() async {
    final r = await _runSql('SELECT name FROM sys.databases ORDER BY name');
    return [
      for (final row in r.rows)
        if (row['name'] != null) row['name'].toString(),
    ];
  }

  @override
  Future<List<String>> listSchemas(String database) async {
    // 排除系统模式:sys / guest / INFORMATION_SCHEMA / db_* 固定角色
    final r = await _runSql(
      'SELECT name FROM ${_quoted(database)}.sys.schemas '
      "WHERE name NOT IN ('guest', 'INFORMATION_SCHEMA', 'sys') "
      "AND name NOT LIKE 'db\\_%' ESCAPE '\\' ORDER BY name",
    );
    return [
      for (final row in r.rows)
        if (row['name'] != null) row['name'].toString(),
    ];
  }

  @override
  Future<List<String>> listTables(String database, {String? schema}) async {
    final schemaFilter = schema == null
        ? ''
        : "AND TABLE_SCHEMA = '${_literal(schema)}' ";
    final r = await _runSql(
      "SELECT TABLE_NAME FROM ${_quoted(database)}.INFORMATION_SCHEMA.TABLES "
      "WHERE TABLE_TYPE = 'BASE TABLE' $schemaFilter"
      "ORDER BY TABLE_NAME",
    );
    return [
      for (final row in r.rows)
        if (row['TABLE_NAME'] != null) row['TABLE_NAME'].toString(),
    ];
  }

  @override
  Future<List<String>> listViews(String database, {String? schema}) async {
    final schemaFilter = schema == null
        ? ''
        : "WHERE TABLE_SCHEMA = '${_literal(schema)}' ";
    final r = await _runSql(
      'SELECT TABLE_NAME FROM ${_quoted(database)}.INFORMATION_SCHEMA.VIEWS '
      '$schemaFilter'
      'ORDER BY TABLE_NAME',
    );
    return [
      for (final row in r.rows)
        if (row['TABLE_NAME'] != null) row['TABLE_NAME'].toString(),
    ];
  }

  /// 执行「OBJ_NAME + CMT」两列查询,聚成 名称 → 注释(trim 后)的 Map。
  Future<Map<String, String>> _objectComments(String sql) async {
    final r = await _runSql(sql);
    return {
      for (final row in r.rows)
        if (row['OBJ_NAME'] != null)
          row['OBJ_NAME'].toString(): (row['CMT']?.toString() ?? '').trim(),
    };
  }

  // SQL Server 对象级注释存于 sys.extended_properties(class=1、minor_id=0、
  // name='MS_Description');ep.value 是 sql_variant,CAST 成 NVARCHAR 便于读取。
  @override
  Future<Map<String, String>> listTableComments(String database,
      {String? schema}) {
    final qdb = _quoted(database);
    final sf = schema == null ? '' : "AND s.name = '${_literal(schema)}' ";
    return _objectComments(
      "SELECT t.name AS OBJ_NAME, CAST(ep.value AS NVARCHAR(4000)) AS CMT "
      "FROM $qdb.sys.tables t "
      "JOIN $qdb.sys.schemas s ON s.schema_id = t.schema_id "
      "LEFT JOIN $qdb.sys.extended_properties ep "
      "  ON ep.class = 1 AND ep.major_id = t.object_id AND ep.minor_id = 0 "
      " AND ep.name = 'MS_Description' "
      "WHERE 1 = 1 $sf",
    );
  }

  @override
  Future<Map<String, String>> listViewComments(String database,
      {String? schema}) {
    final qdb = _quoted(database);
    final sf = schema == null ? '' : "AND s.name = '${_literal(schema)}' ";
    return _objectComments(
      "SELECT v.name AS OBJ_NAME, CAST(ep.value AS NVARCHAR(4000)) AS CMT "
      "FROM $qdb.sys.views v "
      "JOIN $qdb.sys.schemas s ON s.schema_id = v.schema_id "
      "LEFT JOIN $qdb.sys.extended_properties ep "
      "  ON ep.class = 1 AND ep.major_id = v.object_id AND ep.minor_id = 0 "
      " AND ep.name = 'MS_Description' "
      "WHERE 1 = 1 $sf",
    );
  }

  @override
  Future<Map<String, int>> listTableRowEstimates(String database,
      {String? schema}) async {
    final qdb = _quoted(database);
    final sf = schema == null ? '' : "AND s.name = '${_literal(schema)}' ";
    // sys.partitions.rows 由存储引擎维护(增删行时更新),读目录不扫描数据;
    // index_id 0 = 堆、1 = 聚集索引,二者取一即表本身的行数
    final r = await _runSql(
      "SELECT t.name AS OBJ_NAME, SUM(p.rows) AS ROWS "
      "FROM $qdb.sys.tables t "
      "JOIN $qdb.sys.schemas s ON s.schema_id = t.schema_id "
      "JOIN $qdb.sys.partitions p "
      "  ON p.object_id = t.object_id AND p.index_id IN (0, 1) "
      "WHERE 1 = 1 $sf "
      "GROUP BY t.name",
    );
    final out = <String, int>{};
    for (final row in r.rows) {
      final name = row['OBJ_NAME']?.toString();
      final rows = parseRowCount(row['ROWS']);
      if (name != null && rows != null) out[name] = rows;
    }
    return out;
  }

  @override
  Future<Map<String, String>> listFunctionComments(String database,
      {String? schema}) {
    final qdb = _quoted(database);
    final sf = schema == null ? '' : "AND s.name = '${_literal(schema)}' ";
    // FN 标量函数 / IF 内联表值 / TF 表值 / AF 聚合 / PC 程序化 CLR 函数
    return _objectComments(
      "SELECT o.name AS OBJ_NAME, CAST(ep.value AS NVARCHAR(4000)) AS CMT "
      "FROM $qdb.sys.objects o "
      "JOIN $qdb.sys.schemas s ON s.schema_id = o.schema_id "
      "LEFT JOIN $qdb.sys.extended_properties ep "
      "  ON ep.class = 1 AND ep.major_id = o.object_id AND ep.minor_id = 0 "
      " AND ep.name = 'MS_Description' "
      "WHERE o.type IN ('FN', 'IF', 'TF', 'AF', 'PC') $sf",
    );
  }

  @override
  Future<List<String>> listMaterializedViews(String database,
          {String? schema}) async =>
      const <String>[];

  @override
  Future<List<String>> listFunctions(String database, {String? schema}) async {
    final r = await _runSql(
      'SELECT ROUTINE_NAME FROM ${_quoted(database)}.INFORMATION_SCHEMA.ROUTINES '
      "WHERE ROUTINE_TYPE = 'FUNCTION' "
      "AND ROUTINE_SCHEMA = '${_literal(schema ?? 'dbo')}' "
      'ORDER BY ROUTINE_NAME',
    );
    return [
      for (final row in r.rows)
        if (row['ROUTINE_NAME'] != null) row['ROUTINE_NAME'].toString(),
    ];
  }

  @override
  Future<List<String>> listProcedures(String database, {String? schema}) async {
    final r = await _runSql(
      'SELECT ROUTINE_NAME FROM ${_quoted(database)}.INFORMATION_SCHEMA.ROUTINES '
      "WHERE ROUTINE_TYPE = 'PROCEDURE' "
      "AND ROUTINE_SCHEMA = '${_literal(schema ?? 'dbo')}' "
      'ORDER BY ROUTINE_NAME',
    );
    return [
      for (final row in r.rows)
        if (row['ROUTINE_NAME'] != null) row['ROUTINE_NAME'].toString(),
    ];
  }

  @override
  Future<List<String>> listUsers(String database) async {
    final r = await _runSql(
      'SELECT name FROM ${_quoted(database)}.sys.database_principals '
      "WHERE type IN ('S', 'U', 'G') AND principal_id > 0 "
      'ORDER BY name',
    );
    return [
      for (final row in r.rows)
        if (row['name'] != null) row['name'].toString(),
    ];
  }

  // ── 主体详情 / 成员关系(「用户 / 角色」设计页) ────────────
  // SQL Server 的"用户"是**库级**主体,与服务器级登录名(login)分离;
  // 设计页编辑的是库级主体,故这里的查询都限定在 [database] 内。

  @override
  Future<UserSpec?> readUser(String database, String account) async {
    final name = account.trim();
    if (name.isEmpty) return null;
    final r = await _runSql(
      'SELECT type, default_schema_name FROM ${_quoted(database)}.sys.database_principals '
      "WHERE name = N'${_literal(name)}' AND principal_id > 0",
    );
    if (r.rows.isEmpty) return null;
    final row = r.rows.first;
    final type = (row['type'] ?? '').toString();
    return UserSpec(
      originalName: name,
      username: name,
      password: '',
      isRole: type == 'R',
      sqlPrincipalType:
          SqlPrincipalType.fromCode(type) ?? SqlPrincipalType.sqlUser,
      // 默认模式存在 default_schema_name;借「注释」字段以外的槽位不合适,
      // 这里暂不映射(高级页对 SQL Server 只暴露主体类型)
      comment: '',
    );
  }

  /// 可授予的数据库角色候选(`type = 'R'`,排除固定成员角色)
  @override
  Future<List<String>> listGrantableRoles(String database) async {
    final r = await _runSql(
      'SELECT name FROM ${_quoted(database)}.sys.database_principals '
      "WHERE type = 'R' AND principal_id > 0 ORDER BY name",
    );
    return [
      for (final row in r.rows)
        if (row['name'] != null) row['name'].toString(),
    ];
  }

  /// SQL Server 的权限模型是 `database_permissions` / `sys.fn_my_permissions`,
  /// 与 MySQL 的 `*_priv` 布尔列矩阵不同,本页暂不呈现 → 空列表。
  @override
  Future<List<List<String>>> readUserPrivileges(
    String database,
    String account, {
    bool serverLevel = false,
  }) async =>
      const [];

  /// 该主体所属的数据库角色
  @override
  Future<List<String>> readUserRoles(String database, String account) async {
    final name = account.trim();
    if (name.isEmpty) return const [];
    final r = await _runSql(
      'SELECT r.name FROM ${_quoted(database)}.sys.database_role_members m '
      'JOIN ${_quoted(database)}.sys.database_principals r '
      '  ON r.principal_id = m.role_principal_id '
      'JOIN ${_quoted(database)}.sys.database_principals p '
      '  ON p.principal_id = m.member_principal_id '
      "WHERE p.name = N'${_literal(name)}' ORDER BY r.name",
    );
    return [
      for (final row in r.rows)
        if (row['name'] != null) row['name'].toString(),
    ];
  }

  /// 属于该角色的成员(反向)
  @override
  Future<List<String>> readRoleMembers(String database, String account) async {
    final name = account.trim();
    if (name.isEmpty) return const [];
    final r = await _runSql(
      'SELECT p.name FROM ${_quoted(database)}.sys.database_role_members m '
      'JOIN ${_quoted(database)}.sys.database_principals r '
      '  ON r.principal_id = m.role_principal_id '
      'JOIN ${_quoted(database)}.sys.database_principals p '
      '  ON p.principal_id = m.member_principal_id '
      "WHERE r.name = N'${_literal(name)}' ORDER BY p.name",
    );
    return [
      for (final row in r.rows)
        if (row['name'] != null) row['name'].toString(),
    ];
  }

  /// 序列:SQL Server 的序列是独立对象(与 `IDENTITY` 列属性不同),走 sys.sequences。
  @override
  Future<List<String>> listSequences(String database, {String? schema}) async {
    final where = schema == null ? '' : "WHERE s.name = '${_literal(schema)}' ";
    final r = await _runSql(
      'SELECT q.name FROM ${_quoted(database)}.sys.sequences q '
      'JOIN ${_quoted(database)}.sys.schemas s ON s.schema_id = q.schema_id '
      '$where'
      'ORDER BY q.name',
    );
    return [
      for (final row in r.rows)
        if (row['name'] != null) row['name'].toString(),
    ];
  }

  @override
  Future<SequenceDef?> readSequence(String database, String name,
      {String? schema}) async {
    final sch = schema ?? 'dbo';
    // current_value 与 identity 一样只在**分发过**值之后才有意义(未用过为 NULL)。
    final r = await _runSql(
      'SELECT q.start_value, q.increment, q.minimum_value, q.maximum_value, '
      'q.is_cycling, q.cache_size, q.current_value, t.name AS type_name '
      'FROM ${_quoted(database)}.sys.sequences q '
      'JOIN ${_quoted(database)}.sys.schemas s ON s.schema_id = q.schema_id '
      'JOIN ${_quoted(database)}.sys.types t ON t.user_type_id = q.user_type_id '
      "WHERE s.name = '${_literal(sch)}' AND q.name = '${_literal(name)}'",
    );
    if (r.rows.isEmpty) return null;
    final row = r.rows.first;
    String? at(String key) => row[key]?.toString();

    final dataType = at('type_name');
    final start = at('start_value');
    final inc = at('increment') ?? '1';
    final min = at('minimum_value');
    final max = at('maximum_value');
    final cycle =
        (at('is_cycling') ?? '').toLowerCase() == 'true' || at('is_cycling') == '1';
    final cache = at('cache_size');

    final buf = StringBuffer('CREATE SEQUENCE ')
      ..write(DdlBuilder.qualified('sqlserver', sch, name));
    if (dataType != null && dataType.isNotEmpty) buf.write(' AS $dataType');
    if (start != null) buf.write(' START WITH $start');
    buf.write(' INCREMENT BY $inc');
    if (min != null) buf.write(' MINVALUE $min');
    if (max != null) buf.write(' MAXVALUE $max');
    if (cache != null) buf.write(' CACHE $cache');
    buf.write(cycle ? ' CYCLE' : ' NO CYCLE');

    return SequenceDef(
      createSql: buf.toString(),
      lastValue: at('current_value'),
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
    // schema 为空时用默认模式 dbo;非空时显式限定 db.schema.table
    final schemaPart = _quoted(schema ?? 'dbo');
    // OFFSET/FETCH 要求 ORDER BY;无排序时用常量表达式占位(不改变行序)
    final order = orderByClauseSql(orderBy).isNotEmpty
        ? ' ORDER BY $orderBy'
        : ' ORDER BY (SELECT NULL)';
    final r = await _runSql(
      'SELECT * FROM ${_quoted(database)}.$schemaPart.${_quoted(table)}'
      '${whereClauseSql(where)}$order'
      ' OFFSET $offset ROWS FETCH NEXT $limit ROWS ONLY',
    );

    final rows = <List<String>>[];
    // nullMask 记录每格的原始 null 判定:展示层里真 NULL 与字符串 "NULL"
    // 都是 "NULL",导出 / 导入必须靠它区分
    final nullMask = <List<bool>>[];
    for (final row in r.rows) {
      final cells = <String>[];
      final nulls = <bool>[];
      for (final col in r.columns) {
        final v = row[col];
        cells.add(v?.toString() ?? 'NULL');
        nulls.add(v == null);
      }
      rows.add(cells);
      nullMask.add(nulls);
    }
    return TablePreview(
      columns: r.columns,
      rows: rows,
      limit: limit,
      nullMask: nullMask,
    );
  }

  @override
  Future<int> countTable(String database, String table,
      {String? schema, String? where}) async {
    final schemaPart = _quoted(schema ?? 'dbo');
    final r = await _runSql(
      'SELECT COUNT(*) AS cnt FROM ${_quoted(database)}.$schemaPart.${_quoted(table)}'
      '${whereClauseSql(where)}',
    );
    return parseCountValue(r.rows.first[r.columns.first]);
  }

  @override
  Future<void> useDatabase(String database) async {
    if (_currentDatabase == database) return;
    await _runSql('USE ${_quoted(database)}');
    _currentDatabase = database;
  }

  @override
  Future<void> useSchema(String? schema) {
    // SQL Server 虽有模式层但无会话级默认模式机制,仅 PostgreSQL 家族支持;
    // UI 层按类型显隐,不会对其调用本方法
    throw UnsupportedError('该数据库类型不支持会话级模式切换');
  }

  @override
  Future<QueryResult> executeQuery(String sql,
      {int limit = 1000, int offset = 0}) async {
    // 走封顶流式,不能用 _runSql:dart_odbc 的 execute 会把整棵结果集在
    // ODBC isolate 里抽干后整体拷回,`SELECT * FROM 大表` 会把进程内存
    // 顶到数 GB(实测 5.3 GB)并报 HY001「Memory allocation failure」,
    // 且要等全表读完才出结果。详见 odbcQueryCapped。
    final r = await odbcQueryCapped(_get(), sql, limit: limit, offset: offset);
    return QueryResult(
      columns: r.columns,
      rows: r.rows,
      limit: limit,
      offset: offset,
      moreRows: r.moreRows,
    );
  }

  @override
  Future<int?> serverSessionId() async {
    final r = await _runSql('SELECT @@SPID AS spid');
    if (r.rows.isEmpty) return null;
    return int.tryParse(r.rows.first['spid']?.toString() ?? '');
  }

  @override
  Future<void> killSession(int sessionId) async {
    // 主连接被 executeQuery 占住 → 第二条 ODBC 连接发 KILL;依次试已装驱动
    const drivers = [
      'ODBC Driver 18 for SQL Server',
      'ODBC Driver 17 for SQL Server',
      'SQL Server Native Client 11.0',
      'SQL Server',
    ];
    Object? lastError;
    for (final driver in drivers) {
      final odbc = DartOdbc();
      try {
        await odbc.connectWithConnectionString(_connectionString(driver));
        await odbc.execute('KILL $sessionId');
        return;
      } catch (e) {
        lastError = e;
      } finally {
        try {
          await odbc.disconnect();
        } catch (_) {}
      }
    }
    if (lastError != null) throw Exception('取消查询失败: $lastError');
  }

  @override
  Future<List<ColumnDef>> describeTable(String database, String table,
      {String? schema}) async {
    final schemaFilter = schema == null
        ? ''
        : "AND c.TABLE_SCHEMA = '${_literal(schema)}' ";
    final qdb = _quoted(database);
    // SQL Server 列注释存于 sys.extended_properties(name='MS_Description'),
    // INFORMATION_SCHEMA 拿不到;按 表名 → 模式 → 列 三级 JOIN 定位到
    // (major_id, minor_id) 后再取 ep.value。ep.value 是 sql_variant,
    // CAST 成 NVARCHAR(4000) 便于驱动直接读文本
    final r = await _runSql(
      "SELECT c.COLUMN_NAME, c.DATA_TYPE, c.IS_NULLABLE, c.COLUMN_DEFAULT, "
      "CAST(ep.value AS NVARCHAR(4000)) AS COLUMN_COMMENT "
      "FROM $qdb.INFORMATION_SCHEMA.COLUMNS c "
      "LEFT JOIN $qdb.sys.tables tb ON tb.name = c.TABLE_NAME "
      "LEFT JOIN $qdb.sys.schemas s "
      "  ON s.schema_id = tb.schema_id AND s.name = c.TABLE_SCHEMA "
      "LEFT JOIN $qdb.sys.columns sc "
      "  ON sc.object_id = tb.object_id AND sc.name = c.COLUMN_NAME "
      "LEFT JOIN $qdb.sys.extended_properties ep "
      "  ON ep.class = 1 AND ep.major_id = sc.object_id "
      " AND ep.minor_id = sc.column_id AND ep.name = 'MS_Description' "
      "WHERE c.TABLE_NAME = '${_literal(table)}' $schemaFilter"
      "ORDER BY c.ORDINAL_POSITION",
    );
    return [
      for (final row in r.rows)
        ColumnDef(
          name: row['COLUMN_NAME']?.toString() ?? '',
          type: row['DATA_TYPE']?.toString() ?? '',
          nullable: (row['IS_NULLABLE']?.toString() ?? 'YES') == 'YES',
          defaultValue: row['COLUMN_DEFAULT']?.toString(),
          comment: row['COLUMN_COMMENT']?.toString() ?? '',
        ),
    ];
  }

  /// 设计器下拉候选:SQL Server 只有排序规则目录(`sys.fn_helpcollations()`);
  /// 文件组与运算符类别无可类比的目录。
  @override
  Future<DesignCandidates> readDesignCandidates(String database) async {
    final r = await _runSql('SELECT name AS collname '
        'FROM sys.fn_helpcollations() ORDER BY name');
    return DesignCandidates(
      collations: [
        for (final row in r.rows) row['collname']?.toString() ?? '',
      ].where((e) => e.isNotEmpty).toList(),
    );
  }

  /// 「设计表」反查:列 / 主键 / 索引 / 外键 / 唯一键 / 检查 / 表注释。
  ///
  /// 一律走 `sys.*` 目录(`INFORMATION_SCHEMA` 拿不到 identity 种子、索引
  /// fill_factor 与扩展属性);多列索引 / 约束在 Dart 端按序号聚合
  /// (`STRING_AGG` 需 SQL Server 2017+,不为此引入降级分支)。
  /// 计算列只展示基础类型与约性,其表达式无法由 ALTER COLUMN 回写:
  /// 不改它就不会生成语句,改它则由服务端报错并原文回显。
  @override
  Future<DesignTable?> readTableDesign(String database, String table,
      {String? schema}) async {
    final design = DesignTable()
      ..name = table
      ..schema = schema;
    final db = _quoted(database);
    final sch = _literal(schema ?? 'dbo');
    final tbl = _literal(table);
    // OBJECT_ID 接受 '库.模式.表' 形式的名称串(与 getDefinition 一致约定)
    final obj = "N'${_literal(database)}.${sch}.${tbl}'";

    // ── 列 ─────────────────────────────────────────────────
    final colRs = await _runSql(
      'SELECT c.column_id AS ORD, c.name AS COL, LOWER(ty.name) AS TYP, '
      'c.max_length AS MAXLEN, c.precision AS PREC, c.scale AS SCALE, '
      'c.is_nullable AS NULLABLE, c.is_identity AS IDENT, '
      'c.collation_name AS COLL, '
      '(SELECT TOP 1 dc.definition FROM ${db}.sys.default_constraints dc '
      'WHERE dc.parent_object_id = c.object_id '
      'AND dc.parent_column_id = c.column_id) AS DEF, '
      '(SELECT TOP 1 ic.seed_value FROM ${db}.sys.identity_columns ic '
      'WHERE ic.object_id = c.object_id '
      'AND ic.column_id = c.column_id) AS SEED, '
      '(SELECT TOP 1 ic.increment_value FROM ${db}.sys.identity_columns ic '
      'WHERE ic.object_id = c.object_id '
      'AND ic.column_id = c.column_id) AS INCR, '
      "(SELECT TOP 1 CAST(ep.value AS nvarchar(4000)) "
      'FROM ${db}.sys.extended_properties ep '
      'WHERE ep.major_id = c.object_id AND ep.minor_id = c.column_id '
      "AND ep.name = 'MS_Description') AS CMT "
      'FROM ${db}.sys.columns c '
      'JOIN ${db}.sys.tables tb ON tb.object_id = c.object_id '
      'JOIN ${db}.sys.schemas sc ON sc.schema_id = tb.schema_id '
      'JOIN ${db}.sys.types ty ON ty.user_type_id = c.user_type_id '
      "WHERE tb.name = '$tbl' AND sc.name = '$sch' ORDER BY c.column_id",
    );
    for (final row in colRs.rows) {
      final base = (row['TYP']?.toString() ?? '');
      final maxLen = _intOf(row['MAXLEN']);
      final size = _ssColumnSize(base, maxLen, _intOf(row['PREC']), _intOf(row['SCALE']));
      design.columns.add(
        DesignColumn(
          name: row['COL']?.toString() ?? '',
          type: baseTypeOf(base, 'sqlserver'),
          length: size.length,
          decimal: size.decimal,
          notNull: !_truthy(row['NULLABLE']),
          defaultValue: normaliseDefault(row['DEF']?.toString(), 'sqlserver') ?? '',
          comment: row['CMT']?.toString() ?? '',
          collation: row['COLL']?.toString() ?? '',
          // SQL Server 的 IDENTITY 只有 seed / increment 两项,其余序列选项无对应
          identityMode: _truthy(row['IDENT']) ? 'ALWAYS' : '',
          identityStart: _text(row['SEED']),
          identityIncrement: _text(row['INCR']),
        ),
      );
    }

    // ── 主键 / 唯一约束(名称与列序取自 sys.key_constraints) ────
    final keyRs = await _runSql(
      'SELECT kc.name AS INAME, kc.type AS KTYPE, ic.key_ordinal AS ORD, '
      'c.name AS COL '
      'FROM ${db}.sys.key_constraints kc '
      'JOIN ${db}.sys.indexes i ON i.object_id = kc.parent_object_id '
      'AND i.index_id = kc.unique_index_id '
      'JOIN ${db}.sys.index_columns ic ON ic.object_id = i.object_id '
      'AND ic.index_id = i.index_id '
      'JOIN ${db}.sys.columns c ON c.object_id = ic.object_id '
      'AND c.column_id = ic.column_id '
      'WHERE kc.parent_object_id = OBJECT_ID($obj) '
      'ORDER BY kc.name, ic.key_ordinal',
    );
    final keyCols = <String, List<String>>{};
    final keyKind = <String, String>{};
    for (final row in keyRs.rows) {
      final name = row['INAME']?.toString() ?? '';
      if (name.isEmpty) continue;
      (keyCols[name] ??= []).add(row['COL']?.toString() ?? '');
      keyKind[name] = row['KTYPE']?.toString() ?? '';
    }
    final pkColumns = <String>{};
    for (final e in keyCols.entries) {
      final cols = e.value.where((c) => c.isNotEmpty).join(', ');
      if (keyKind[e.key] == 'PRIMARY_KEY') {
        design.pkName = e.key;
        pkColumns.addAll(e.value);
      } else {
        design.uniqueKeys.add(DesignUniqueKey(name: e.key, columns: cols));
      }
    }
    for (final c in design.columns) {
      c.primaryKey = pkColumns.contains(c.name);
    }

    // ── 索引(排除主键与唯一约束支撑的索引) ────────────────
    final idxRs = await _runSql(
      'SELECT i.name AS INAME, i.is_unique AS UNIQ, i.fill_factor AS FF, '
      'i.has_filter AS HASFILTER, '
      'ic.key_ordinal AS ORD, c.name AS COL '
      'FROM ${db}.sys.indexes i '
      'JOIN ${db}.sys.tables tb ON tb.object_id = i.object_id '
      'JOIN ${db}.sys.schemas sc ON sc.schema_id = tb.schema_id '
      'JOIN ${db}.sys.index_columns ic ON ic.object_id = i.object_id '
      'AND ic.index_id = i.index_id '
      'JOIN ${db}.sys.columns c ON c.object_id = ic.object_id '
      'AND c.column_id = ic.column_id '
      "WHERE tb.name = '$tbl' AND sc.name = '$sch' "
      'AND i.is_primary_key = 0 AND i.is_unique_constraint = 0 '
      'ORDER BY i.name, ic.key_ordinal',
    );
    final idxCols = <String, List<String>>{};
    final idxMeta = <String, List<String>>{};
    for (final row in idxRs.rows) {
      final name = row['INAME']?.toString() ?? '';
      if (name.isEmpty) continue;
      (idxCols[name] ??= []).add(row['COL']?.toString() ?? '');
      final ff = _intOf(row['FF']) ?? 0;
      // 筛选项索引(FILTERED)的谓词无法由 DesignIndex 回写,不纳入列表以免
      // 保存时被当成「新增同名索引」而失败
      idxMeta[name] = [
        _truthy(row['UNIQ']) ? '1' : '',
        ff <= 0 ? '' : '$ff',
        _truthy(row['HASFILTER']) ? '1' : '',
      ];
    }
    for (final e in idxCols.entries) {
      if (idxMeta[e.key]![2] == '1') continue;
      design.indexes.add(DesignIndex(
        name: e.key,
        columns: e.value.where((c) => c.isNotEmpty).join(', '),
        unique: idxMeta[e.key]![0] == '1',
        fillFactor: idxMeta[e.key]![1],
      ));
    }

    // ── 外键 ────────────────────────────────────────────────
    final fkRs = await _runSql(
      'SELECT fk.name AS INAME, fk.delete_action_desc AS DELACT, '
      'fk.update_action_desc AS UPACT, '
      'OBJECT_SCHEMA_NAME(fk.referenced_object_id) AS RSCH, '
      'OBJECT_NAME(fk.referenced_object_id) AS RTBL, '
      'fc.constraint_column_id AS ORD, '
      'c.name AS COL, rc.name AS REFCOL '
      'FROM ${db}.sys.foreign_keys fk '
      'JOIN ${db}.sys.foreign_key_columns fc '
      'ON fc.constraint_object_id = fk.object_id '
      'JOIN ${db}.sys.columns c ON c.object_id = fk.parent_object_id '
      'AND c.column_id = fc.parent_column_id '
      'JOIN ${db}.sys.columns rc ON rc.object_id = fk.referenced_object_id '
      'AND rc.column_id = fc.referenced_column_id '
      'WHERE fk.parent_object_id = OBJECT_ID($obj) '
      'ORDER BY fk.name, fc.constraint_column_id',
    );
    final fkCols = <String, List<String>>{};
    final fkRefCols = <String, List<String>>{};
    final fkMeta = <String, List<String>>{};
    for (final row in fkRs.rows) {
      final name = row['INAME']?.toString() ?? '';
      if (name.isEmpty) continue;
      (fkCols[name] ??= []).add(row['COL']?.toString() ?? '');
      (fkRefCols[name] ??= []).add(row['REFCOL']?.toString() ?? '');
      fkMeta[name] = [
        row['RTBL']?.toString() ?? '',
        row['RSCH']?.toString() ?? '',
        (row['DELACT']?.toString() ?? 'NO_ACTION').replaceAll('_', ' '),
        (row['UPACT']?.toString() ?? 'NO_ACTION').replaceAll('_', ' '),
      ];
    }
    for (final name in fkCols.keys) {
      final m = fkMeta[name]!;
      design.foreignKeys.add(DesignForeignKey(
        name: name,
        columns: fkCols[name]!.join(', '),
        refColumns: fkRefCols[name]!.join(', '),
        refTable: m[0],
        refSchema: m[1],
        onDelete: m[2],
        onUpdate: m[3],
      ));
    }

    // ── 检查约束 ────────────────────────────────────────────
    final ckRs = await _runSql(
      'SELECT cc.name AS INAME, cc.definition AS EXPR '
      'FROM ${db}.sys.check_constraints cc '
      'WHERE cc.parent_object_id = OBJECT_ID($obj) ORDER BY cc.name',
    );
    for (final row in ckRs.rows) {
      design.checks.add(DesignCheck(
        name: row['INAME']?.toString() ?? '',
        expression: stripRedundantParens(row['EXPR']?.toString() ?? ''),
      ));
    }

    // ── 表注释(扩展属性 MS_Description,minor_id = 0) ────────
    final cmtRs = await _runSql(
      'SELECT CAST(ep.value AS nvarchar(4000)) AS V '
      'FROM ${db}.sys.extended_properties ep '
      'JOIN ${db}.sys.tables tb ON tb.object_id = ep.major_id '
      'JOIN ${db}.sys.schemas sc ON sc.schema_id = tb.schema_id '
      "WHERE ep.name = 'MS_Description' AND ep.minor_id = 0 "
      "AND tb.name = '$tbl' AND sc.name = '$sch'",
    );
    if (cmtRs.rows.isNotEmpty) {
      design.tableComment = cmtRs.rows.first['V']?.toString() ?? '';
    }

    // ── 触发器 ────────────────────────────────────────────────
    // 一个触发器可能挂在多个事件上,`sys.trigger_events` 是 1:N,故按
    // OBJECT_ID 聚合出事件位图,再一行一个触发器。
    //
    // 时机:SQL Server 只有 AFTER / INSTEAD OF(无 BEFORE),时机由
    // `sys.triggers.is_instead_of_trigger` 给出。
    // 主体:`OBJECT_DEFINITION()` 返回的是整条 `CREATE TRIGGER … AS <body>`,
    // 而设计器的 [DesignTrigger.body] 只存 `AS` 之后的那段,得切出来。
    // `is_disabled`:设计器只对 PG 生成 DISABLE(见 DdlBuilder),这里读出来
    // 仍回填,免得上层把它当成「用户改过」而误判脏。
    try {
      final trgRs = await _runSql(
        'SELECT tr.name AS TNAME, tr.is_instead_of_trigger AS INSTEAD, '
        'tr.is_disabled AS DISABLED, '
        "(SELECT STRING_AGG(te.type_desc, ',') FROM ${db}.sys.trigger_events te "
        'WHERE te.object_id = tr.object_id) AS EVENTS, '
        'OBJECT_DEFINITION(tr.object_id) AS DEF '
        'FROM ${db}.sys.triggers tr '
        'WHERE tr.parent_id = OBJECT_ID($obj) '
        'AND tr.is_ms_shipped = 0 ORDER BY tr.name',
      );
      for (final row in trgRs.rows) {
        final name = row['TNAME']?.toString() ?? '';
        if (name.isEmpty) continue;
        final events = (row['EVENTS']?.toString() ?? '').toUpperCase();
        design.triggers.add(DesignTrigger(
          name: name,
          // SQL Server 触发器是语句级:没有 FOR EACH ROW 子句
          forEach: '语句',
          timing: _truthy(row['INSTEAD']) ? 'INSTEAD OF' : 'AFTER',
          insert: events.contains('INSERT'),
          update: events.contains('UPDATE'),
          delete: events.contains('DELETE'),
          enable: !_truthy(row['DISABLED']),
          // TRUNCATE 在 SQL Server 里是独立事件类型,设计器不支持 → 忽略
          body: stripSqlServerTriggerBody(row['DEF']?.toString() ?? ''),
        ));
      }
    } catch (_) {
      // 触发器读不到(权限 / 旧版本):不预填即可,界面仍可手工新增
    }
    return design;
  }

  /// SQL Server 列的长度 / 小数位折算。
  ///
  /// `sys.columns.max_length` 是字节数(nvarchar(50) 返 100)、-1 表示 `max`;
  /// `decimal` / `numeric` 用 precision + scale;`datetime2` / `time` 的
  /// `scale` 即括号里的精度;其余类型(含 int / bit / date / uniqueidentifier)
  /// 无长度参数,不得把字节数当成长度写进 DDL。
  static ({String length, String decimal}) _ssColumnSize(
      String type, int? maxLen, int? prec, int? scale) {
    switch (type) {
      case 'decimal':
      case 'numeric':
        return (length: prec == null ? '' : '$prec', decimal: scale == null ? '' : '$scale');
      case 'float':
        return (length: prec == null ? '' : '$prec', decimal: '');
      case 'datetime2':
      case 'datetimeoffset':
      case 'time':
        return (length: scale == null ? '' : '$scale', decimal: '');
      case 'char':
      case 'varchar':
      case 'nchar':
      case 'nvarchar':
      case 'binary':
      case 'varbinary':
        if (maxLen == null) return (length: '', decimal: '');
        if (maxLen < 0) return (length: 'max', decimal: '');
        final len = lengthFromBytes(maxLen, type) ?? maxLen;
        return (length: '$len', decimal: '');
      default:
        return (length: '', decimal: '');
    }
  }

  /// bit 列的 ODBC 回值形态不一(bool / 0-1 / 字符),统一成布尔
  static bool _truthy(Object? v) {
    if (v == null) return false;
    if (v is bool) return v;
    if (v is num) return v != 0;
    final s = v.toString().toLowerCase();
    return s == '1' || s == 't' || s == 'true' || s == 'yes';
  }

  static int? _intOf(Object? v) => v is int ? v : int.tryParse('${v ?? ''}');

  static String _text(Object? v) => v?.toString() ?? '';

  /// 库 / 表详情:本轮只实现了 MySQL 家族的详情面板,SQL Server 返回 null
  /// 使界面回退到基础展示(大小 / 行格式需查 sys.dm_db_index_physical_stats,
  /// 权限与折算规则差异大,不做半套)。
  @override
  Future<DatabaseDetail?> readDatabaseDetail(String database) async => null;

  @override
  Future<TableDetail?> readTableDetail(String database, String table,
          {String? schema}) async =>
      null;

  /// 依赖关系(使用 / 被使用)是 PostgreSQL 专属页签,不实现
  @override
  Future<List<DependentObject>?> readTableDependencies(
          String database, String table,
          {String? schema, bool usedBy = true}) async =>
      null;

  @override
  Future<String?> getDefinition(String database, String name, String kind,
      {String? schema}) async {
    // SQL Server 视图 / 函数均为可定义对象,OBJECT_DEFINITION 返回完整 CREATE 文本
    final objName = '$database.${schema ?? 'dbo'}.$name';
    final r = await _runSql(
      "SELECT OBJECT_DEFINITION(OBJECT_ID('${_literal(objName)}')) AS def",
    );
    if (r.rows.isEmpty) return null;
    final v = r.rows.first['def'];
    return v?.toString();
  }
}

/// 从 `CREATE TRIGGER … AS <body>` 的完整定义里切出 `AS` 之后的主体。
///
/// `OBJECT_DEFINITION()` 给的是服务端存下的整条语句,含头部与 `AS`;
/// 设计器只存主体(生成时自己拼 `AS BEGIN … END`)。切不干净的话,
/// 「反查 → 原样保存」会把头部再套一层。
///
/// 取**最后**一个 ` AS ` 而非第一个:触发器名 / 表名里可能自带 `as` 字样
/// (如 `[as_trg]`、`[gas]`),而真正的 `AS` 在事件列表之后。找不到时原样
/// 返回 —— 宁可多留一段,也不截断丢掉用户的过程体。
///
/// 导出为顶层函数(而非驱动静态方法)便于离线回归,同 MySQL 的
/// [stripMysqlTriggerBeginEnd]。
String stripSqlServerTriggerBody(String def) {
  final s = def.trim();
  if (s.isEmpty) return s;
  final m = RegExp(r'\sAS\s', caseSensitive: false).allMatches(s).toList();
  if (m.isEmpty) return s;
  final hit = m.last;
  var body = s.substring(hit.end).trim();
  // 去一层生成器写的 BEGIN…END 包裹,回到裸语句(与生成器对称)
  final upper = body.toUpperCase();
  if (upper.startsWith('BEGIN') && upper.endsWith('END')) {
    body = body.substring(5, body.length - 3).trim();
  }
  while (body.endsWith(';')) {
    body = body.substring(0, body.length - 1).trimRight();
  }
  return body;
}
