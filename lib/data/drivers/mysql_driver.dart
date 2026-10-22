import 'dart:async';
import 'dart:io';

import 'package:meta/meta.dart';
import 'package:mysql_client/exception.dart';
import 'package:mysql_client/mysql_client.dart';

import '../db_data.dart';
import '../db_metadata.dart';
import '../sql_row_cap.dart';
import '../table_design.dart';
import '../user_sql.dart';
import 'db_driver.dart';
import 'mysql_user_metadata.dart';

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
    if (!mysqlTlsUnavailable(e)) rethrow;
  }
  try {
    return await attempt(false);
  } on MySQLClientException catch (e, st) {
    // 服务器没开 SSL,账号又是 caching_sha2_password:两条路都走不通,
    // 把库里的英文报错换成人能看懂的处置建议(保留原始堆栈与底层报错)
    if (e.message.contains('caching_sha2_password')) {
      Error.throwWithStackTrace(
        MySQLClientException(
          '账号使用 caching_sha2_password 认证,必须走加密连接,'
          '但服务器未启用 SSL。请在服务器上开启 SSL,'
          '或把该账号改为 mysql_native_password 认证。'
          '(原始错误:${e.message})',
        ),
        st,
      );
    }
    rethrow;
  }
}

/// 失败是否属于「TLS 不可用」——只有这种情况才值得回退明文重试。
///
/// 密码错误 / 权限不足(服务端已明确应答)必须原样抛出,否则会被一次明文
/// 重试掩盖成误导性报错;主机不可达同理(重试只是白等一轮超时)——
/// 包括 RST 类失败(SocketException)与防火墙 drop 端口类失败(TimeoutException)。
/// `@visibleForTesting` 导出仅为回归钉住分类规则。
@visibleForTesting
bool mysqlTlsUnavailable(Object error) {
  if (error is MySQLServerException) return false;
  if (error is SocketException) return false;
  // 超时抛的是 dart:async 的 TimeoutException(非 SocketException 子类),
  // 与主机不可达同理:换明文重试解决不了,只会再等满一轮超时
  if (error is TimeoutException) return false;
  if (error is MySQLClientException) {
    // 认证插件不匹配:换明文也解决不了
    return !error.message.contains('auth plugin') &&
        !error.message.contains('caching_sha2_password');
  }
  // 其余按 TLS 不可用处理:握手期连接被关闭、TLS 版本/证书异常等
  // (HandshakeException / TlsException 均继承 IOException,不是 SocketException 子类)
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

  /// MySQL 没有独立序列对象(AUTO_INCREMENT 是列属性),见 [kSequenceTypes]。
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

  // ── 账号详情 / 权限(「用户 / 角色」设计页) ────────────────
  // 行解析集中在 MysqlUserMetadata(与 MariaDB 共用);这里只提供"执行 SQL"。

  MysqlUserMetadata get _userMeta =>
      MysqlUserMetadata(executor: _userRows, isMariaDb: false);

  /// 执行 SQL 并把结果行折成 `列名(小写) → 值`(null → 空串)。
  ///
  /// `SELECT *` 的列名大小写随 MySQL 版本 / 系统表而异(`User` vs `user`),
  /// 统一转小写便于读取端写得简单。
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
      // 多半是 BIT / BLOB 列的裸字节在包内 UTF-8 解码时就炸了
      // (见 [mysqlSafeProjection]),只能让服务端改发整数 / 十六进制重试一次;
      // 取不到投影说明另有原因,原样抛出
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
    // BIT 列的原始值是裸字节,见 [mysqlBitAsInt]
    final isBit = [for (final col in colDefs) mysqlIsBitColumn(col)];
    // nullMask 记录每格的原始 null 判定:展示层里真 NULL 与字符串 "NULL"
    // 都是 "NULL",导出 / 导入必须靠它区分
    final nullMask = <List<bool>>[];
    final rows = <List<String>>[];
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

    // BIT 列的原始值是裸字节,见 [mysqlBitAsInt]
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
      // 详情面板的 DDL 页:MySQL 的表定义由引擎给出原文,无需自行重建
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

/// 执行一条查询,并把「包内 UTF-8 解码失败」换成人能看懂的处置建议。
///
/// 服务端在文本协议里对 `BIT` / `BLOB` / 几何列发的是裸字节,而 mysql_client
/// 对每个字段一律按 UTF-8 解码(见 [mysqlSafeProjectionOf])。表浏览能自动换
/// 安全投影重试,用户手写的 SQL 换不了——只能提示他自己把该列写成
/// `HEX(col)` / `CAST(col AS UNSIGNED)`。
///
/// 解码失败时该连接已被强关,调用方下次 `_get()` 会自动重连(与表浏览同路)。
Future<IResultSet> mysqlExecute(MySQLConnection conn, String sql) async {
  try {
    return await conn.execute(sql);
  } on FormatException catch (e) {
    throw MySQLClientException(
      '结果集里有无法按 UTF-8 解码的裸字节:服务端在文本协议里把 BIT / BLOB / '
      '几何列按原始字节发送。请在 SQL 里把该列改写成 HEX(col) 或 '
      'CAST(col AS UNSIGNED) 再查询(表数据页会自动这样处理)。'
      '(原始错误:$e)',
    );
  }
}

/// `MYSQL_TYPE_BIT`(协议常量 0x10),见 [mysqlIsBitColumn]。
const int kMysqlTypeBit = 0x10;

/// 该结果集列是否为 `BIT` 类型。
bool mysqlIsBitColumn(ResultSetColumn col) => col.type.intVal == kMysqlTypeBit;

/// 把 `BIT` 列的原始报文值(按列宽度大端打包的裸字节)转成十进制整数字符串。
///
/// 为什么必须转:`mysql_client` 把 bit 归进 String 族、对文本协议的字段一律按
/// UTF-8 解码(`mysql_protocol/mysql_column_type.dart`),于是 `bit(1)` 的 0 / 1
/// 变成 `\u0000` / `\u0001` 两个不可打印字符,表格里渲染成豆腐块、复制出来是乱码。
/// 解码后的每个码元恰好等于原字节(0x00–0x7F 单字节区),按大端折回去就与
/// `CAST(col AS UNSIGNED)` 同值。
///
/// 已知边界:字节 ≥ 0x80 时(如 `bit(8)` 存 128–255)在包内 UTF-8 解码阶段就抛
/// `FormatException`,值到不了这里——那条与 BLOB / 几何列一样,只能靠
/// [mysqlSafeProjection] 换服务端编码。
String mysqlBitAsInt(String raw) {
  if (raw.isEmpty) return raw;
  var value = BigInt.zero;
  for (final byte in raw.codeUnits) {
    value = (value << 8) | BigInt.from(byte & 0xff);
  }
  return '$value';
}

/// 单元格文本:`BIT` 列把原始字节换成整数,其余列原样返回。
///
/// null 先归一成 `NULL` 再返回——真 NULL 绝不能喂给 [mysqlBitAsInt],
/// 否则字符串 `'NULL'` 的四个码元会被折成 `1296247884`。
String mysqlCellText(String? raw, bool isBit) {
  if (raw == null) return 'NULL';
  return isBit ? mysqlBitAsInt(raw) : raw;
}

/// 服务端按**裸字节**发送、必须换编码才能取回的列类型(`DATA_TYPE` 小写)。
///
/// `BIT` 走 `CAST(… AS UNSIGNED)`;其余二进制族与几何族走 `HEX(…)`:它们没有
/// 「整数」这一层语义,而十六进制文本既能原样展示,也能用 `X'…'` 字面量精确写回。
/// 几何列服务端发的是内部二进制格式,与 BLOB 同属一个崩溃类。
const Set<String> kMysqlBinaryDataTypes = {
  'binary',
  'varbinary',
  'tinyblob',
  'blob',
  'mediumblob',
  'longblob',
  'geometry',
  'point',
  'linestring',
  'polygon',
  'multipoint',
  'multilinestring',
  'multipolygon',
  'geometrycollection',
};

/// [mysqlSafeProjectionOf] 的结果。
class MysqlSafeProjection {
  const MysqlSafeProjection(this.select, this.hexColumns);

  /// 可直接拼进 `SELECT … FROM` 的投影列表(列名与顺序保持不变)
  final String select;

  /// 其中被换成十六进制文本的二进制列名。上层回写时必须还原成 `X'…'`
  /// 二进制字面量,否则会把十六进制字符本身存进列里(5 字节变 10 字节)——
  /// 见 table_data_page.dart 的 `sqlValueLiteral`。
  final Set<String> hexColumns;
}

/// 由「列名 + DATA_TYPE」清单生成把裸字节换成可打印文本的 SELECT 投影;
/// 清单里没有需要换编码的列时返回 null(调用方据此保持 `SELECT *`)。
///
/// 为什么需要投影:mysql_client 对文本协议的**每个**字段一律按 UTF-8 解码
/// (`packet/packet_result_set_row.dart` 直接调 `getUtf8LengthEncodedString`),
/// 而服务端把 `BIT` / `BLOB` / 几何列按裸字节发(结果集字符集只转码文本列)。
/// 只要字节序列不是合法 UTF-8(含 ≥ 0x80 的字节,如 `bit(8)` 存 128–255、
/// 或 BLOB 里的一段 DER 证书),解码就抛 `FormatException`,整条查询失败——
/// [mysqlBitAsInt] 那样的事后转换根本拿不到值。
/// 唯一出路是让服务端别发裸字节:`CAST(col AS UNSIGNED)` / `HEX(col)`。
///
/// 纯函数,便于离线回归。
@visibleForTesting
MysqlSafeProjection? mysqlSafeProjectionOf(
    List<({String name, String type})> cols) {
  String norm(String type) => type.toLowerCase();
  bool isBinary(String type) => kMysqlBinaryDataTypes.contains(norm(type));
  bool needsText(String type) => norm(type) == 'bit' || isBinary(type);
  if (!cols.any((c) => needsText(c.type))) return null;
  String quote(String id) => '`${id.replaceAll('`', '``')}`';
  return MysqlSafeProjection(
    [
      for (final c in cols)
        if (norm(c.type) == 'bit')
          'CAST(${quote(c.name)} AS UNSIGNED) AS ${quote(c.name)}'
        else if (isBinary(c.type))
          'HEX(${quote(c.name)}) AS ${quote(c.name)}'
        else
          quote(c.name),
    ].join(', '),
    {for (final c in cols) if (isBinary(c.type)) c.name},
  );
}

/// 读表(或视图)的列清单并生成安全投影,见 [mysqlSafeProjectionOf]。
///
/// 只在 `SELECT *` 已经抛 FormatException 之后调用,正常表不付这次元数据查询的代价。
/// 已知边界:列清单来自 `information_schema.COLUMNS`,MySQL 8.0.23+ 的 INVISIBLE 列
/// 也在其中而 `SELECT *` 不返回它们——同时有裸字节列和隐藏列的表会多出一列,
/// 这类表极罕见,不值得为它探测服务端版本。
Future<MysqlSafeProjection?> mysqlSafeProjection(
    MySQLConnection conn, String database, String table) async {
  String lit(String v) => "'${v.replaceAll("'", "''")}'";
  final rs = await conn.execute(
    'SELECT COLUMN_NAME, DATA_TYPE FROM information_schema.COLUMNS '
    'WHERE TABLE_SCHEMA = ${lit(database)} AND TABLE_NAME = ${lit(table)} '
    'ORDER BY ORDINAL_POSITION',
  );
  return mysqlSafeProjectionOf([
    for (final row in rs.rows)
      (name: row.colAt(0) ?? '', type: (row.colAt(1) ?? '').toLowerCase()),
  ]);
}

/// MySQL / MariaDB 共用的库级详情(`information_schema.SCHEMATA`)。
///
/// 库不存在时返回 null(而非抛异常):详情面板把它当作「无信息」处理。
Future<DatabaseDetail?> mysqlReadDatabaseDetail(
    MySQLConnection conn, String database) async {
  final rs = await conn.execute(
    'SELECT DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME '
    'FROM information_schema.SCHEMATA '
    "WHERE SCHEMA_NAME = '${database.replaceAll("'", "''")}'",
  );
  if (rs.rows.isEmpty) return null;
  final row = rs.rows.first;
  return DatabaseDetail(
    name: database,
    charset: row.colAt(0) ?? '',
    collation: row.colAt(1) ?? '',
  );
}

/// MySQL / MariaDB 共用的表级详情(`information_schema.TABLES` 单行)。
///
/// 一条目录查询取齐全部属性:引擎统计由服务端维护,读它不扫描数据,
/// 因此选中表时即时调用是安全的(代价与列表查询同级)。
/// [colAt] 一律返回字符串(NULL 返回 null),数字与时间在此就地解析;
/// 解析失败(如 `0000-00-00 00:00:00` 零值日期)归一为 null,界面显示占位符。
Future<TableDetail?> mysqlReadTableDetail(
    MySQLConnection conn, String database, String table) async {
  String lit(String v) => "'${v.replaceAll("'", "''")}'";
  final rs = await conn.execute(
    'SELECT ENGINE, ROW_FORMAT, TABLE_COLLATION, CREATE_OPTIONS, TABLE_COMMENT, '
    'TABLE_ROWS, AUTO_INCREMENT, CREATE_TIME, UPDATE_TIME, CHECK_TIME, '
    'DATA_LENGTH, INDEX_LENGTH, MAX_DATA_LENGTH, DATA_FREE '
    'FROM information_schema.TABLES '
    "WHERE TABLE_SCHEMA = ${lit(database)} AND TABLE_NAME = ${lit(table)}",
  );
  if (rs.rows.isEmpty) return null;
  final row = rs.rows.first;
  int? numAt(int i) => parseRowCount(row.colAt(i));
  DateTime? timeAt(int i) {
    final v = row.colAt(i)?.trim();
    if (v == null || v.isEmpty) return null;
    return DateTime.tryParse(v.replaceFirst(' ', 'T'));
  }

  return TableDetail(
    name: table,
    engine: row.colAt(0) ?? '',
    rowFormat: row.colAt(1) ?? '',
    collation: row.colAt(2) ?? '',
    createOptions: row.colAt(3) ?? '',
    comment: row.colAt(4) ?? '',
    rowEstimate: numAt(5),
    autoIncrement: numAt(6),
    createTime: timeAt(7),
    updateTime: timeAt(8),
    checkTime: timeAt(9),
    dataLength: numAt(10),
    indexLength: numAt(11),
    maxDataLength: numAt(12),
    dataFree: numAt(13),
  );
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
  final kcuRs = await conn.execute(
    'SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_NAME, '
    'REFERENCED_COLUMN_NAME, ORDINAL_POSITION '
    'FROM information_schema.KEY_COLUMN_USAGE '
    "WHERE TABLE_SCHEMA = $db AND TABLE_NAME = $tbl "
    'ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION',
  );
  final kcu = parseMysqlKeyColumnUsage([
    for (final row in kcuRs.rows)
      [
        row.colAt(0) ?? '',
        row.colAt(1) ?? '',
        row.colAt(2) ?? '',
        row.colAt(3) ?? '',
        row.colAt(4) ?? '',
      ],
  ]);
  for (final name in uniqueNames) {
    design.uniqueKeys
        .add(DesignUniqueKey(name: name, columns: kcu.localColumns(name)));
  }
  // 主键列标记(字段页「键」列的 钥匙 + 序号 靠它)
  final pkSet =
      (kcu.localCols[design.pkName] ?? const <String>[]).toSet();
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
        columns: kcu.fkColumns(name),
        // MySQL / MariaDB 无独立模式层(库即模式),不填 refSchema,引用回当前库
        refTable: m[0],
        refColumns: kcu.fkRefColumns(name),
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

  // ── 触发器 ─────────────────────────────────────────────────
  // `information_schema.TRIGGERS` 一个触发器一行,ACTION_TIMING /
  // EVENT_MANIPULATION / ACTION_STATEMENT / ACTION_ORDER 全都现成,
  // 不需要像 PG 那样反解析 DDL。
  //
  // 主体形态:MySQL 存在库里的是 `BEGIN … END` 包裹后的完整过程体,而设计器的
  // [DesignTrigger.body] 存的是**不带包裹的语句**(生成时恒用 BEGIN…END 包,
  // 见 DdlBuilder._mysqlTriggerSql)。所以这里要去掉外层包裹还原成裸语句,
  // 否则「读回来 → 原样保存」会变成 `BEGIN BEGIN … END; END;` 双层嵌套。
  try {
    final trgRs = await conn.execute(
      'SELECT TRIGGER_NAME, ACTION_TIMING, EVENT_MANIPULATION, '
      'ACTION_STATEMENT, ACTION_ORIENTATION '
      'FROM information_schema.TRIGGERS '
      'WHERE EVENT_OBJECT_SCHEMA = $db AND EVENT_OBJECT_TABLE = $tbl '
      'ORDER BY TRIGGER_NAME',
    );
    for (final row in trgRs.rows) {
      final name = row.colAt(0) ?? '';
      final event = (row.colAt(2) ?? '').toUpperCase();
      if (name.isEmpty || event.isEmpty) continue;
      design.triggers.add(DesignTrigger(
        name: name,
        // MySQL 只有行级触发器,ACTION_ORIENTATION 恒为 ROW;设计器的
        // 「给每个」候选在该方言下也只有「行」,写死即可
        forEach: '行',
        timing: (row.colAt(1) ?? 'BEFORE').toUpperCase(),
        insert: event == 'INSERT',
        update: event == 'UPDATE',
        delete: event == 'DELETE',
        body: stripMysqlTriggerBeginEnd(row.colAt(3) ?? ''),
      ));
    }
  } catch (_) {
    // 触发器读不到(权限 / 旧版服务端无该视图):不预填即可,界面仍可手工新增。
    // 与表注释之外的附加信息同一取舍:不牵连整次反查。
  }
  return design;
}

/// 去掉 MySQL 触发器主体的 `BEGIN … END` 外壳,还原成设计器里存的裸语句。
///
/// 服务端把 `CREATE TRIGGER … BEGIN <body>; END` 整体存进 `ACTION_STATEMENT`
/// (含外层 BEGIN/END),而设计器只在最外层包一次。不做这层还原的话:
/// 「反查 → 原样保存」会生成 `BEGIN BEGIN … END; END;`,再存进去又叠一层。
///
/// 只在**最外层**确实是 BEGIN…END 时剥壳;单语句 / 已带 IF…END IF 这类
/// 内层结构的主体原样返回。缩进按生成器写回的 `\n  ` 反推,尽量保持幂等。
String stripMysqlTriggerBeginEnd(String raw) {
  var s = raw.trim();
  if (s.isEmpty) return s;
  // 去掉整体末尾分号(服务端通常不带,手工建的库可能带)
  while (s.endsWith(';')) {
    s = s.substring(0, s.length - 1).trimRight();
  }
  final upper = s.toUpperCase();
  if (!upper.startsWith('BEGIN')) return s;
  // BEGIN 之后必须是语句边界(空格 / 换行),排除 `BEGINNING` 之类的误判
  if (s.length > 5 && !RegExp(r'[\s]').hasMatch(s[5])) return s;
  if (!upper.endsWith('END')) return s;
  // `END IF` / `END WHILE` / `END LOOP` / `END CASE` 结尾的不是最外层包裹
  if (RegExp(r'END\s+(IF|WHILE|LOOP|CASE|REPEAT)\s*$', caseSensitive: false)
      .hasMatch(upper)) {
    return s;
  }
  final inner = s.substring(5, s.length - 3).trim();
  // 内层末尾分号同样剥掉:生成器写回时会补
  var body = inner;
  while (body.endsWith(';')) {
    body = body.substring(0, body.length - 1).trimRight();
  }
  return body;
}

/// [parseMysqlKeyColumnUsage] 的归组结果。
class MysqlKeyUsage {
  const MysqlKeyUsage(this.localCols, this.fkCols, this.fkRefCols);

  /// 主键 / 唯一键:约束名 → 本表列(按 `ORDINAL_POSITION` 定序)
  final Map<String, List<String>> localCols;

  /// 外键:约束名 → 本表列 / 引用列
  final Map<String, List<String>> fkCols;
  final Map<String, List<String>> fkRefCols;

  static String _of(Map<String, List<String>> m, String name) =>
      (m[name] ?? const <String>[]).join(', ');

  String localColumns(String name) => _of(localCols, name);
  String fkColumns(String name) => _of(fkCols, name);
  String fkRefColumns(String name) => _of(fkRefCols, name);
}

/// 归组 `information_schema.KEY_COLUMN_USAGE` 的行；每行按查询列序传
/// `[约束名, 本表列, 引用表, 引用列, 列序号]`（NULL 传空串）。纯函数，便于离线回归。
///
/// 为什么不按约束名一把归：MySQL 的**索引名与外键约束名是两套命名空间**，
/// 同名的 UNIQUE 与 FOREIGN KEY 可以合法共存（实测 `tc_etc` 两者都叫 `channel_id`）。
/// 混在一个 map 里会把两条约束的列并成一串，生成
/// `UNIQUE (\`channel_id\`, \`channel_id\`)` → 部署报 1060 Duplicate column name。
/// 外键行以 `REFERENCED_TABLE_NAME` 非空识别；同名同序号只取首行，
/// 因为 MySQL 会为外键附带引用侧索引的重复行。
MysqlKeyUsage parseMysqlKeyColumnUsage(Iterable<List<String>> rows) {
  final local = <String, List<String>>{};
  final fk = <String, List<String>>{};
  final fkRef = <String, List<String>>{};
  final seen = <String>{};
  for (final r in rows) {
    final name = r[0];
    if (name.isEmpty) continue;
    final isFk = r[2].isNotEmpty;
    if (!seen.add('${isFk ? 'f' : 'l'}$name ${r[4]}')) continue;
    final bucket = isFk ? fk : local;
    if (r[1].isNotEmpty) (bucket[name] ??= []).add(r[1]);
    if (isFk && r[3].isNotEmpty) (fkRef[name] ??= []).add(r[3]);
  }
  return MysqlKeyUsage(local, fk, fkRef);
}
