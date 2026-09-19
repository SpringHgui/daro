import 'dart:io';

import 'package:mysql_client/exception.dart';
import 'package:mysql_client/mysql_client.dart';

import '../db_data.dart';
import '../db_metadata.dart';
import '../sql_row_cap.dart';
import '../table_design.dart';
import 'db_driver.dart';

/// 建立一条 MySQL 协议连接:优先 TLS,服务端不支持时回退明文。
///
/// MySQL 8 默认认证插件 `caching_sha2_password` 的密码交换只在加密通道上
/// 进行(mysql_client 未实现 RSA 公钥交换),明文连接必然报
/// 「Auth plugin caching_sha2_password is supported only with secure connections」;
/// 而旧版 / 未开启 SSL 的服务器又不接受 TLS 握手。故先试 TLS,
/// 仅在确认「TLS 不可用」时回退明文——此时服务端用的是
/// `mysql_native_password`,明文可正常认证。
Future<MySQLConnection> openMysqlConnection({
  required String host,
  required int port,
  required String userName,
  required String password,
  String? databaseName,
  int timeoutMs = 10000,
}) async {
  Future<MySQLConnection> attempt(bool secure) async {
    final conn = await MySQLConnection.createConnection(
      host: host,
      port: port,
      userName: userName,
      password: password,
      databaseName: databaseName,
      secure: secure,
    );
    try {
      await conn.connect(timeoutMs: timeoutMs);
    } catch (_) {
      // 握手失败时 mysql_client 已销毁 socket;这里兜住「已建立连接、
      // 但随后的 SET 语句失败」留下的残留连接
      if (conn.connected) {
        try {
          await conn.close();
        } catch (_) {}
      }
      rethrow;
    }
    return conn;
  }

  try {
    return await attempt(true);
  } catch (e) {
    if (!_tlsUnavailable(e)) rethrow;
  }
  try {
    return await attempt(false);
  } on MySQLClientException catch (e) {
    // 服务器没开 SSL,账号又是 caching_sha2_password:两条路都走不通,
    // 把库里的英文报错换成人能看懂的处置建议
    if (e.message.contains('caching_sha2_password')) {
      throw MySQLClientException(
        '账号使用 caching_sha2_password 认证,必须走加密连接,'
        '但服务器未启用 SSL。请在服务器上开启 SSL,'
        '或把该账号改为 mysql_native_password 认证。',
      );
    }
    rethrow;
  }
}

/// 失败是否属于「TLS 不可用」——只有这种情况才值得回退明文重试。
///
/// 密码错误 / 权限不足(服务端已明确应答)必须原样抛出,否则会被一次明文
/// 重试掩盖成误导性报错;主机不可达同理(重试只是白等一轮超时)。
bool _tlsUnavailable(Object error) {
  if (error is MySQLServerException) return false;
  if (error is SocketException) return false;
  if (error is MySQLClientException) {
    // 认证插件不匹配:换明文也解决不了
    return !error.message.contains('auth plugin') &&
        !error.message.contains('caching_sha2_password');
  }
  // 其余(多为 TLS 握手失败:服务端只支持旧版 TLS / 证书异常等)按 TLS 不可用处理
  return true;
}

/// MySQL 驱动(纯 Dart 实现,基于 mysql_client)。
///
/// 持有单条长连接,元数据查询与数据预览共用同一连接;
/// 连接断开时由上层([ConnectionManager])负责重建。
class MysqlDriver implements DatabaseDriver {
  MysqlDriver(this._conn);

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

  @override
  Future<DesignTable?> readTableDesign(String database, String table,
          {String? schema}) async =>
      mysqlReadTableDesign(await _get(), database, table, 'mysql');

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
    // MySQL 中 Database 即 Schema,无独立模式层,返回空列表(树不渲染模式节点)
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
    // TABLE_ROWS 取自存储引擎的统计信息(InnoDB 随 ANALYZE 刷新,可能滞后;
    // MyISAM 恰为精确值),读元数据不扫描数据,代价与列表查询同级
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
    final conn = await _get();
    final rs = await conn.execute(
      'SELECT * FROM ${_quoted(database)}.${_quoted(table)}'
      '${whereClauseSql(where)}${orderByClauseSql(orderBy)}'
      ' LIMIT $limit OFFSET $offset',
    );

    final columns = [for (final col in rs.cols) col.name];
    final rows = <List<String>>[];
    // nullMask 记录每格的原始 null 判定:展示层里真 NULL 与字符串 "NULL"
    // 都是 "NULL",导出 / 导入必须靠它区分
    final nullMask = <List<bool>>[];
    for (final row in rs.rows) {
      final cells = <String>[];
      final nulls = <bool>[];
      for (var i = 0; i < columns.length; i++) {
        final v = row.colAt(i);
        cells.add(v ?? 'NULL');
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
    // MySQL 无会话级模式概念(库即模式),仅 PostgreSQL 家族支持;
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
    final rs = await conn.execute(capped ?? sql);

    final columns = [for (final col in rs.cols) col.name];
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

    final rows = <List<String>>[];
    for (final row in rs.rows) {
      if (rows.length >= limit) break;
      rows.add([
        for (var i = 0; i < columns.length; i++) row.colAt(i) ?? 'NULL',
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

  /// 设计器下拉候选:MySQL / MariaDB 只有排序规则目录(`information_schema.COLLATIONS`);
  /// 运算符类别无此概念,表空间需存储引擎级对象(默认无可列举)。
  @override
  Future<DesignCandidates> readDesignCandidates(String database) async =>
      DesignCandidates(collations: await mysqlCollationNames(await _get()));

  @override
  Future<String?> getDefinition(String database, String name, String kind,
      {String? schema}) async {
    final conn = await _get();
    final sql = switch (kind) {
      'view' => 'SHOW CREATE VIEW ${_quoted(database)}.${_quoted(name)}',
      'procedure' =>
        'SHOW CREATE PROCEDURE ${_quoted(database)}.${_quoted(name)}',
      _ => 'SHOW CREATE FUNCTION ${_quoted(database)}.${_quoted(name)}',
    };
    final rs = await conn.execute(sql);
    if (rs.rows.isEmpty) return null;
    final row = rs.rows.first;
    final colName = switch (kind) {
      'view' => 'Create View',
      'procedure' => 'Create Procedure',
      _ => 'Create Function',
    };
    final cols = rs.cols.toList();
    final idx = cols.indexWhere((c) => c.name == colName);
    if (idx < 0) return null;
    return row.colAt(idx);
  }
}

/// MySQL / MariaDB 共用的排序规则候选(`information_schema.COLLATIONS`)。
///
/// 不按库过滤:排序规则是实例级对象。行数可达数百,ComboBox 弹层为
/// `ListView.builder`,懒渲染不致于卡顿。
Future<List<String>> mysqlCollationNames(MySQLConnection conn) async {
  final rs = await conn.execute('SELECT COLLATION_NAME '
      'FROM information_schema.COLLATIONS ORDER BY 1');
  return [
    for (final row in rs.rows) row.colAt(0) ?? '',
  ].where((e) => e.isNotEmpty).toList();
}

/// MySQL / MariaDB 共用的「设计表」反查(两者的 `information_schema` 结构一致)。
///
/// 抽为库级函数而非让 MariaDB 驱动依赖 [MysqlDriver] 实例:两者仅个别视图列
/// 有别(`REFERENTIAL_CONSTRAINTS.MATCH_OPTION` 与 `CHECK_CONSTRAINTS` 在旧版
/// MariaDB 不存在),取数与映射完全相同,那些差异就地降级处理。
/// [dialect] 为调用方的连接类型 id,用于默认值 / 类型名归一化。
///
/// 未采集的信息(模型无对应字段,且不会生成 ALTER 而丢数据):
/// 表达式索引的索引表达式、生成列定义、字符集(只保留排序规则)。
Future<DesignTable?> mysqlReadTableDesign(MySQLConnection conn,
    String database, String table, String dialect) async {
  final db = "'${database.replaceAll("'", "''")}'";
  final tbl = "'${table.replaceAll("'", "''")}'";
  final design = DesignTable()..name = table;

  // ── 列 ──────────────────────────────────────────────────────
  // COLUMN_TYPE 带完整长度 / 精度 / unsigned,交共用的 splitColumnType 拆分
  final colRs = await conn.execute(
    'SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, '
    'COLUMN_COMMENT, COLLATION_NAME, EXTRA FROM information_schema.COLUMNS '
    "WHERE TABLE_SCHEMA = $db AND TABLE_NAME = $tbl "
    'ORDER BY ORDINAL_POSITION',
  );
  for (final row in colRs.rows) {
    final rawType = row.colAt(1) ?? '';
    final split = splitColumnType(rawType);
    final extra = (row.colAt(6) ?? '').toLowerCase();
    var def = normaliseDefault(row.colAt(3), dialect) ?? '';
    // `ON UPDATE CURRENT_TIMESTAMP` 在 EXTRA 里,模型无独立字段:随默认值一起
    // 回写(`DEFAULT CURRENT_TIMESTAMP ON UPDATE ...` 为合法语法),否则改该列
    // 会静默丢掉自动更新语义
    final on = RegExp(r'on update (.+)$').firstMatch(extra);
    if (on != null && def.isNotEmpty) def = '$def ON UPDATE ${on.group(1)}';
    design.columns.add(
      DesignColumn(
        name: row.colAt(0) ?? '',
        type: baseTypeOf(rawType, dialect),
        length: split.length,
        decimal: split.decimal,
        notNull: (row.colAt(2) ?? 'YES') == 'NO',
        defaultValue: def,
        comment: row.colAt(4) ?? '',
        collation: row.colAt(5) ?? '',
        autoIncrement: extra.contains('auto_increment'),
      ),
    );
  }

  // ── 约束分类(主键 / 唯一 / 外键 / 检查的真名) ────────────
  final conRs = await conn.execute(
    'SELECT CONSTRAINT_NAME, CONSTRAINT_TYPE FROM information_schema.TABLE_CONSTRAINTS '
    "WHERE TABLE_SCHEMA = $db AND TABLE_NAME = $tbl ORDER BY CONSTRAINT_NAME",
  );
  final uniqueNames = <String>{};
  final fkNames = <String>[];
  final checkNames = <String>{};
  for (final row in conRs.rows) {
    final name = row.colAt(0) ?? '';
    switch (row.colAt(1) ?? '') {
      case 'PRIMARY KEY':
        design.pkName = name;
      case 'UNIQUE':
        uniqueNames.add(name);
      case 'FOREIGN KEY':
        fkNames.add(name);
      case 'CHECK':
        checkNames.add(name);
    }
  }

  // ── 约束列(一条约束多列时按 ORDINAL_POSITION 定序) ────────
  final keyCols = <String, List<String>>{};
  final keyRefCols = <String, List<String>>{};
  final kcuRs = await conn.execute(
    'SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_COLUMN_NAME '
    'FROM information_schema.KEY_COLUMN_USAGE '
    "WHERE TABLE_SCHEMA = $db AND TABLE_NAME = $tbl "
    'ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION',
  );
  for (final row in kcuRs.rows) {
    final name = row.colAt(0) ?? '';
    if (name.isEmpty) continue;
    final col = row.colAt(1) ?? '';
    final refCol = row.colAt(2) ?? '';
    if (col.isNotEmpty) (keyCols[name] ??= []).add(col);
    if (refCol.isNotEmpty) (keyRefCols[name] ??= []).add(refCol);
  }
  String colsOf(String name) => (keyCols[name] ?? const <String>[]).join(', ');
  String refColsOf(String name) =>
      (keyRefCols[name] ?? const <String>[]).join(', ');
  for (final name in uniqueNames) {
    design.uniqueKeys.add(DesignUniqueKey(name: name, columns: colsOf(name)));
  }
  // 主键列标记(字段页「键」列的 钥匙 + 序号 靠它)
  final pkSet = (keyCols[design.pkName] ?? const <String>[]).toSet();
  if (pkSet.isNotEmpty) {
    for (final c in design.columns) {
      c.primaryKey = pkSet.contains(c.name);
    }
  }

  // ── 外键动作(MATCH_OPTION 为 MySQL 8 新增列,旧版 / MariaDB 降级) ──
  if (fkNames.isNotEmpty) {
    // 值形式: [引用表, ON DELETE, ON UPDATE, MATCH 选项]
    final refMeta = <String, List<String>>{};
    var hasMatchOption = true;
    try {
      final rs = await conn.execute(
        'SELECT CONSTRAINT_NAME, REFERENCED_TABLE_NAME, DELETE_RULE, '
        'UPDATE_RULE, MATCH_OPTION FROM information_schema.REFERENTIAL_CONSTRAINTS '
        "WHERE CONSTRAINT_SCHEMA = $db AND TABLE_NAME = $tbl",
      );
      for (final row in rs.rows) {
        refMeta[row.colAt(0) ?? ''] = [
          row.colAt(1) ?? '',
          row.colAt(2) ?? 'NO ACTION',
          row.colAt(3) ?? 'NO ACTION',
          row.colAt(4) ?? '',
        ];
      }
    } catch (_) {
      hasMatchOption = false;
      refMeta.clear();
      final rs = await conn.execute(
        'SELECT CONSTRAINT_NAME, REFERENCED_TABLE_NAME, DELETE_RULE, '
        'UPDATE_RULE FROM information_schema.REFERENTIAL_CONSTRAINTS '
        "WHERE CONSTRAINT_SCHEMA = $db AND TABLE_NAME = $tbl",
      );
      for (final row in rs.rows) {
        refMeta[row.colAt(0) ?? ''] = [
          row.colAt(1) ?? '',
          row.colAt(2) ?? 'NO ACTION',
          row.colAt(3) ?? 'NO ACTION',
          '',
        ];
      }
    }
    for (final name in fkNames) {
      final m = refMeta[name] ?? const ['', 'NO ACTION', 'NO ACTION', ''];
      design.foreignKeys.add(DesignForeignKey(
        name: name,
        columns: colsOf(name),
        // MySQL / MariaDB 无独立模式层(库即模式),不填 refSchema,引用回当前库
        refTable: m[0],
        refColumns: refColsOf(name),
        onDelete: m[1],
        onUpdate: m[2],
        matchAll: hasMatchOption && m[3].toUpperCase() == 'FULL',
      ));
    }
  }

  // ── 检查约束(旧版 MariaDB 无该视图:降级为不展示) ──────────
  if (checkNames.isNotEmpty) {
    try {
      final rs = await conn.execute(
        'SELECT CONSTRAINT_NAME, CHECK_CLAUSE FROM information_schema.CHECK_CONSTRAINTS '
        "WHERE CONSTRAINT_SCHEMA = $db AND TABLE_NAME = $tbl "
        'ORDER BY CONSTRAINT_NAME',
      );
      for (final row in rs.rows) {
        design.checks.add(DesignCheck(
          name: row.colAt(0) ?? '',
          expression: stripRedundantParens(row.colAt(1) ?? ''),
        ));
      }
    } catch (_) {
      // 视图不存在(MySQL < 8.0.16 / 旧 MariaDB):检查约保持为空
    }
  }

  // ── 索引(一条索引占多行,在 Dart 端按名聚合;STRING_AGG 两方言不对齐) ──
  // 主键与唯一约束自带的索引不重复列入(已由 pkName / uniqueKeys 承载)
  final idxRs = await conn.execute(
    'SELECT INDEX_NAME, COLUMN_NAME, NON_UNIQUE, INDEX_TYPE '
    'FROM information_schema.STATISTICS '
    "WHERE TABLE_SCHEMA = $db AND TABLE_NAME = $tbl AND INDEX_NAME <> 'PRIMARY' "
    'ORDER BY INDEX_NAME, SEQ_IN_INDEX',
  );
  final byIndex = <String, List<String>>{};
  for (final row in idxRs.rows) {
    final name = row.colAt(0) ?? '';
    if (name.isEmpty || uniqueNames.contains(name)) continue;
    final entry = (byIndex[name] ??= ['', '', '']);
    final col = row.colAt(1);
    // 表达式索引的 COLUMN_NAME 为 NULL:无法安全回写,不纳入字段列表
    if (col != null && col.isNotEmpty) {
      entry[0] = entry[0].isEmpty ? col : '${entry[0]}, $col';
    }
    entry[1] = (row.colAt(2) ?? '1') == '0' ? '1' : entry[1];
    entry[2] = (row.colAt(3) ?? '').toLowerCase();
  }
  for (final e in byIndex.entries) {
    if (e.value[0].isEmpty) continue; // 纯表达式索引:跳过
    design.indexes.add(DesignIndex(
      name: e.key,
      columns: e.value[0],
      method: e.value[2],
      unique: e.value[1] == '1',
    ));
  }

  // ── 表注释 ─────────────────────────────────────────────────
  final tabRs = await conn.execute(
    'SELECT TABLE_COMMENT FROM information_schema.TABLES '
    "WHERE TABLE_SCHEMA = $db AND TABLE_NAME = $tbl",
  );
  if (tabRs.rows.isNotEmpty) design.tableComment = tabRs.rows.first.colAt(0) ?? '';

  return design;
}
