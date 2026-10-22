import 'package:mysql_client/mysql_client.dart';

import '../db_data.dart';
import '../sql_row_cap.dart';
import '../table_design.dart';
import '../user_sql.dart';
import 'db_driver.dart';
import 'mysql_driver.dart'
    show
        mysqlCellText,
        mysqlCollationNames,
        mysqlExecute,
        mysqlIsBitColumn,
        mysqlReadDatabaseDetail,
        mysqlReadTableDesign,
        mysqlReadTableDetail,
        mysqlSafeProjection,
        openMysqlConnection;
import 'mysql_user_metadata.dart';

/// MariaDB 驱动(纯 Dart 实现,基于 mysql_client)。
///
/// MariaDB 兼容 MySQL 协议,因此复用 mysql_client 包;
/// 持有单条长连接,元数据查询与数据预览共用同一连接;
/// 连接断开时由上层([ConnectionManager])负责重建。
class MariadbDriver implements DatabaseDriver {
  MariadbDriver(this._conn);

  final ConnectionInfo _conn;
  MySQLConnection? _connection;

  @override
  bool get isConnected => _connection?.connected ?? false;

  @override
  Future<void> connect() async {
    if (isConnected) return;
    _connection = await openMysqlConnection(
      host: _conn.host,
      port: int.tryParse(_conn.port) ?? 3306,
      userName: _conn.username,
      password: _conn.password,
      databaseName: _conn.database.isEmpty ? null : _conn.database,
    );
  }

  @override
  Future<void> close() async {
    final conn = _connection;
    _connection = null;
    if (conn != null && conn.connected) {
      await conn.close();
    }
  }

  Future<MySQLConnection> _get() async {
    await connect();
    return _connection!;
  }

  /// 最近一次 USE 的库,避免每次执行都发 USE
  String? _currentDatabase;

  /// 反引号包裹标识符,防库/表名与关键字冲突
  String _quoted(String name) => '`${name.replaceAll('`', '``')}`';

  /// 单引号字符串字面量转义(库名作条件值时用)
  String _literal(String value) => value.replaceAll("'", "''");

  /// 「设计表」反查:与 MySQL 共用同一套 `information_schema` 取数逻辑
  /// (缺的 `MATCH_OPTION` / `CHECK_CONSTRAINTS` 已在其中降级)
  @override
  Future<DesignTable?> readTableDesign(String database, String table,
          {String? schema}) async =>
      mysqlReadTableDesign(await _get(), database, table, 'mariadb');

  /// 库 / 表详情:与 MySQL 共用同一套 `information_schema` 取数逻辑
  @override
  Future<DatabaseDetail?> readDatabaseDetail(String database) async =>
      mysqlReadDatabaseDetail(await _get(), database);

  @override
  Future<TableDetail?> readTableDetail(String database, String table,
          {String? schema}) async =>
      mysqlReadTableDetail(await _get(), database, table);

  /// 依赖关系(使用 / 被使用)是 PostgreSQL 专属页签,不实现
  @override
  Future<List<DependentObject>?> readTableDependencies(
          String database, String table,
          {String? schema, bool usedBy = true}) async =>
      null;

  /// 设计器下拉候选:与 MySQL 同源(只有排序规则目录)。
  @override
  Future<DesignCandidates> readDesignCandidates(String database) async =>
      DesignCandidates(collations: await mysqlCollationNames(await _get()));

  @override
  Future<List<String>> listDatabases() async {
    final conn = await _get();
    final rs = await conn.execute('SHOW DATABASES');
    return [
      for (final row in rs.rows)
        if (row.colAt(0) != null) row.colAt(0)!,
    ];
  }

  @override
  Future<List<String>> listSchemas(String database) async {
    // MariaDB 中 Database 即 Schema,无独立模式层,返回空列表(树不渲染模式节点)
    return const [];
  }

  @override
  Future<List<String>> listTables(String database, {String? schema}) async {
    final conn = await _get();
    final rs = await conn.execute('SHOW TABLES FROM ${_quoted(database)}');
    return [
      for (final row in rs.rows)
        if (row.colAt(0) != null) row.colAt(0)!,
    ];
  }

  @override
  Future<List<String>> listViews(String database, {String? schema}) async {
    final conn = await _get();
    final rs = await conn.execute(
      "SHOW FULL TABLES FROM ${_quoted(database)} WHERE Table_type = 'VIEW'",
    );
    return [
      for (final row in rs.rows)
        if (row.colAt(0) != null) row.colAt(0)!,
    ];
  }

  /// 执行「名称列 + 注释列」两列查询,聚成 名称 → 注释(trim 后)的 Map。
  /// 空注释(多数表/函数未写 COMMENT)会归一为 ''。
  Future<Map<String, String>> _objectComments(String sql) async {
    final conn = await _get();
    final rs = await conn.execute(sql);
    return {
      for (final row in rs.rows)
        if (row.colAt(0) != null) row.colAt(0)!: (row.colAt(1) ?? '').trim(),
    };
  }

  @override
  Future<Map<String, String>> listTableComments(String database,
          {String? schema}) =>
      _objectComments("SELECT TABLE_NAME, TABLE_COMMENT "
          "FROM information_schema.TABLES "
          "WHERE TABLE_SCHEMA = '${_literal(database)}' "
          "AND TABLE_TYPE = 'BASE TABLE'");

  @override
  Future<Map<String, String>> listViewComments(String database,
          {String? schema}) =>
      _objectComments("SELECT TABLE_NAME, TABLE_COMMENT "
          "FROM information_schema.TABLES "
          "WHERE TABLE_SCHEMA = '${_literal(database)}' "
          "AND TABLE_TYPE = 'VIEW'");

  @override
  Future<Map<String, int>> listTableRowEstimates(String database,
      {String? schema}) async {
    final conn = await _get();
    // 与 MySQL 驱动同口径:TABLE_ROWS 是存储引擎的统计值,读元数据不扫描数据
    final rs = await conn.execute(
      "SELECT TABLE_NAME, TABLE_ROWS FROM information_schema.TABLES "
      "WHERE TABLE_SCHEMA = '${_literal(database)}' "
      "AND TABLE_TYPE = 'BASE TABLE' AND TABLE_ROWS IS NOT NULL",
    );
    final out = <String, int>{};
    for (final row in rs.rows) {
      final name = row.colAt(0);
      final rows = parseRowCount(row.colAt(1));
      if (name != null && rows != null) out[name] = rows;
    }
    return out;
  }

  @override
  Future<Map<String, String>> listFunctionComments(String database,
          {String? schema}) =>
      _objectComments("SELECT ROUTINE_NAME, ROUTINE_COMMENT "
          "FROM information_schema.ROUTINES "
          "WHERE ROUTINE_SCHEMA = '${_literal(database)}' "
          "AND ROUTINE_TYPE = 'FUNCTION'");

  @override
  Future<List<String>> listMaterializedViews(String database,
          {String? schema}) async =>
      const <String>[];

  @override
  Future<List<String>> listFunctions(String database, {String? schema}) async {
    final conn = await _get();
    final rs = await conn.execute(
      "SELECT ROUTINE_NAME FROM information_schema.ROUTINES "
      "WHERE ROUTINE_SCHEMA = '${_literal(database)}' "
      "AND ROUTINE_TYPE = 'FUNCTION' ORDER BY ROUTINE_NAME",
    );
    return [
      for (final row in rs.rows)
        if (row.colAt(0) != null) row.colAt(0)!,
    ];
  }

  @override
  Future<List<String>> listProcedures(String database, {String? schema}) async {
    final conn = await _get();
    final rs = await conn.execute(
      "SELECT ROUTINE_NAME FROM information_schema.ROUTINES "
      "WHERE ROUTINE_SCHEMA = '${_literal(database)}' "
      "AND ROUTINE_TYPE = 'PROCEDURE' ORDER BY ROUTINE_NAME",
    );
    return [
      for (final row in rs.rows)
        if (row.colAt(0) != null) row.colAt(0)!,
    ];
  }

  /// MariaDB 没有独立序列对象(AUTO_INCREMENT 是列属性),见 [kSequenceTypes]。
  /// 注:MariaDB 10.3+ 其实有 `CREATE SEQUENCE`,但目录口径与 PG/SQL Server 不同
  /// (序列在 information_schema 里以 SEQUENCE 表类型出现),暂未接入。
  @override
  Future<List<String>> listSequences(String database, {String? schema}) async =>
      const [];

  @override
  Future<SequenceDef?> readSequence(String database, String name,
          {String? schema}) async =>
      null;

  @override
  Future<List<String>> listUsers(String database) async {
    final conn = await _get();
    final rs = await conn.execute(
      "SELECT CONCAT(User, '@', Host) AS account FROM mysql.user ORDER BY User, Host",
    );
    return [
      for (final row in rs.rows)
        if (row.colAt(0) != null) row.colAt(0)!,
    ];
  }

  // ── 账号详情 / 权限(与 MySQL 共用解析,差异仅"无角色"一处) ──

  MysqlUserMetadata get _userMeta =>
      MysqlUserMetadata(executor: _userRows, isMariaDb: true);

  /// 执行 SQL 并折成 `列名(小写) → 值`(null → 空串)
  Future<List<Map<String, String>>> _userRows(String sql) async {
    final conn = await _get();
    final rs = await conn.execute(sql);
    return [
      for (final row in rs.rows)
        {
          for (final col in rs.cols)
            col.name.toLowerCase(): row.colByName(col.name) ?? '',
        },
    ];
  }

  @override
  Future<UserSpec?> readUser(String database, String account) =>
      _userMeta.read(database, account);

  @override
  Future<List<List<String>>> readUserPrivileges(
    String database,
    String account, {
    bool serverLevel = false,
  }) =>
      serverLevel
          ? _userMeta.readServerPrivileges(database, account)
          : _userMeta.readDatabasePrivileges(database, account);

  @override
  Future<List<String>> listGrantableRoles(String database) =>
      _userMeta.listRoles(database);

  @override
  Future<List<String>> readUserRoles(String database, String account) =>
      _userMeta.readUserRoles(database, account);

  @override
  Future<List<String>> readRoleMembers(String database, String account) =>
      _userMeta.readRoleMembers(database, account);

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
    var conn = await _get();
    final fromSql = ' FROM ${_quoted(database)}.${_quoted(table)}'
        '${whereClauseSql(where)}${orderByClauseSql(orderBy)}'
        ' LIMIT $limit OFFSET $offset';
    late final IResultSet rs;
    // 被驱动换成十六进制文本取回的二进制列,交回上层决定回写形式
    Set<String> hexColumns = const {};
    try {
      rs = await conn.execute('SELECT *$fromSql');
    } on FormatException {
      // 多半是 BIT / BLOB 列的裸字节在包内 UTF-8 解码时就炸了(见 mysql_driver.dart
      // 的 mysqlSafeProjection),只能让服务端改发整数 / 十六进制重试一次;
      // 取不到投影说明另有原因
      //
      // 解码失败时 mysql_client 已把这条连接强关(_forceClose),必须换一条再试,
      // 顺带忘掉 USE 状态——新会话停在连接的默认库上
      _currentDatabase = null;
      conn = await _get();
      final plan = await mysqlSafeProjection(conn, database, table);
      if (plan == null) rethrow;
      hexColumns = plan.hexColumns;
      rs = await conn.execute('SELECT ${plan.select}$fromSql');
    }

    final colDefs = rs.cols.toList();
    final columns = [for (final col in colDefs) col.name];
    // BIT 列的原始值是裸字节,见 mysql_driver.dart 的 mysqlBitAsInt
    final isBit = [for (final col in colDefs) mysqlIsBitColumn(col)];
    final rows = <List<String>>[];
    // nullMask 记录每格的原始 null 判定:展示层里真 NULL 与字符串 "NULL"
    // 都是 "NULL",导出 / 导入必须靠它区分
    final nullMask = <List<bool>>[];
    for (final row in rs.rows) {
      final cells = <String>[];
      final nulls = <bool>[];
      for (var i = 0; i < columns.length; i++) {
        final v = row.colAt(i);
        cells.add(mysqlCellText(v, isBit[i]));
        nulls.add(v == null);
      }
      rows.add(cells);
      nullMask.add(nulls);
    }
    return TablePreview(
        columns: columns,
        rows: rows,
        limit: limit,
        nullMask: nullMask,
        hexColumns: hexColumns);
  }

  @override
  Future<int> countTable(String database, String table,
      {String? schema, String? where}) async {
    final conn = await _get();
    final rs = await conn.execute(
      'SELECT COUNT(*) AS cnt FROM ${_quoted(database)}.${_quoted(table)}'
      '${whereClauseSql(where)}',
    );
    return parseCountValue(rs.rows.first.colAt(0));
  }

  @override
  Future<void> useDatabase(String database) async {
    if (_currentDatabase == database) return;
    final conn = await _get();
    await conn.execute('USE ${_quoted(database)}');
    _currentDatabase = database;
  }

  @override
  Future<void> useSchema(String? schema) {
    // MariaDB 无会话级模式概念(库即模式),仅 PostgreSQL 家族支持;
    // UI 层按类型显隐,不会对其调用本方法
    throw UnsupportedError('该数据库类型不支持会话级模式切换');
  }

  @override
  Future<QueryResult> executeQuery(String sql,
      {int limit = 1000, int offset = 0}) async {
    final conn = await _get();
    // 封顶必须下推到服务端:mysql_client 会把整棵结果集读进内存才交回,
    // 在调用方 break 救不回来(详见 sql_row_cap.dart);offset 同理由服务端跳过
    final capped = capSelectSql(sql, maxRows: limit + 1, offset: offset);
    final rs = await mysqlExecute(conn, capped ?? sql);

    final colDefs = rs.cols.toList();
    final columns = [for (final col in colDefs) col.name];
    if (columns.isEmpty) {
      // 写操作:无结果集,返回受影响行数
      return QueryResult(
        columns: const [],
        rows: const [],
        affectedRows: rs.affectedRows.toInt(),
        limit: limit,
        offset: offset,
      );
    }

    // BIT 列的原始值是裸字节,见 mysql_driver.dart 的 mysqlBitAsInt
    final isBit = [for (final col in colDefs) mysqlIsBitColumn(col)];
    final rows = <List<String>>[];
    for (final row in rs.rows) {
      if (rows.length >= limit) break;
      rows.add([
        for (var i = 0; i < columns.length; i++)
          mysqlCellText(row.colAt(i), isBit[i]),
      ]);
    }
    return QueryResult(
      columns: columns,
      rows: rows,
      limit: limit,
      offset: offset,
      // 服务端只被允许返回 limit + 1 行,多出的那行即「还有更多」的确证
      moreRows: capped != null && rs.rows.length > limit,
    );
  }

  @override
  Future<int?> serverSessionId() async {
    final conn = await _get();
    final rs = await conn.execute('SELECT CONNECTION_ID()');
    return int.tryParse(rs.rows.first.colAt(0) ?? '');
  }

  @override
  Future<void> killSession(int sessionId) async {
    // 主连接正被 executeQuery 的 await 占住,无法自取消 → 用第二条临时连接发 KILL
    final killer = await openMysqlConnection(
      host: _conn.host,
      port: int.tryParse(_conn.port) ?? 3306,
      userName: _conn.username,
      password: _conn.password,
    );
    try {
      await killer.execute('KILL $sessionId');
    } finally {
      await killer.close();
    }
  }

  @override
  Future<List<ColumnDef>> describeTable(String database, String table,
      {String? schema}) async {
    final conn = await _get();
    final rs = await conn.execute(
      "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, "
      "COLUMN_DEFAULT, COLUMN_COMMENT FROM information_schema.COLUMNS "
      "WHERE TABLE_SCHEMA = '${_literal(database)}' "
      "AND TABLE_NAME = '${_literal(table)}' ORDER BY ORDINAL_POSITION",
    );
    return [
      for (final row in rs.rows)
        ColumnDef(
          name: row.colAt(0) ?? '',
          type: row.colAt(1) ?? '',
          nullable: (row.colAt(2) ?? 'YES') == 'YES',
          primaryKey: (row.colAt(3) ?? '') == 'PRI',
          defaultValue: row.colAt(4),
          comment: row.colAt(5) ?? '',
        ),
    ];
  }

  @override
  Future<String?> getDefinition(String database, String name, String kind,
      {String? schema}) async {
    final conn = await _get();
    final sql = switch (kind) {
      'view' => 'SHOW CREATE VIEW ${_quoted(database)}.${_quoted(name)}',
      'procedure' =>
        'SHOW CREATE PROCEDURE ${_quoted(database)}.${_quoted(name)}',
      // 详情面板的 DDL 页:表定义由引擎给出原文,无需自行重建
      'table' => 'SHOW CREATE TABLE ${_quoted(database)}.${_quoted(name)}',
      // 库没有「所在库」这一层,对象名即目标
      'database' => 'SHOW CREATE DATABASE ${_quoted(name)}',
      _ => 'SHOW CREATE FUNCTION ${_quoted(database)}.${_quoted(name)}',
    };
    final rs = await conn.execute(sql);
    if (rs.rows.isEmpty) return null;
    final row = rs.rows.first;
    final colName = switch (kind) {
      'view' => 'Create View',
      'procedure' => 'Create Procedure',
      'table' => 'Create Table',
      'database' => 'Create Database',
      _ => 'Create Function',
    };
    final cols = rs.cols.toList();
    final idx = cols.indexWhere((c) => c.name == colName);
    if (idx < 0) return null;
    return row.colAt(idx);
  }
}
