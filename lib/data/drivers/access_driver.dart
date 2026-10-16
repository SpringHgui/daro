import 'package:dart_odbc/dart_odbc.dart';

import '../db_data.dart';
import '../table_design.dart';
import '../user_sql.dart';
import 'db_driver.dart';
import 'odbc_query.dart';

/// Microsoft Access 驱动(基于 dart_odbc,通过 Windows ODBC 访问 .mdb/.accdb)。
///
/// Access 为文件型数据库,一个文件即一个"数据库";
/// [ConnectionInfo.host] 存储数据库文件路径。
/// 需要系统已安装 Microsoft Access Database Engine(ACE ODBC 驱动)。
class AccessDriver implements DatabaseDriver {
  AccessDriver(this._conn);

  final ConnectionInfo _conn;
  DartOdbc? _odbc;

  @override
  bool get isConnected => _odbc != null;

  @override
  Future<void> connect() async {
    if (_odbc != null) return;
    final path = _conn.host;
    if (path.isEmpty) {
      throw Exception('未指定 Access 数据库文件路径');
    }
    final odbc = DartOdbc();
    await odbc.connectWithConnectionString(_connectionString());
    _odbc = odbc;
  }

  /// 构造 ODBC 连接字符串。
  ///
  /// 在 Access 里用「文件 → 信息 → 用密码进行加密」设过**数据库密码**的
  /// .mdb/.accdb,必须带 `PWD=<密码>` 才能打开;未设密码时留空,行为不变。
  ///
  /// 注:仅覆盖数据库密码这一常见场景。用户级安全(工作分组 .mdw)需要
  /// `SystemDB` + `UID`/`PWD`,当前连接模型没有承载 .mdw 路径的字段,暂不支持。
  String _connectionString() {
    final sb = StringBuffer(
      'DRIVER={Microsoft Access Driver (*.mdb, *.accdb)};DBQ=${_conn.host};',
    );
    if (_conn.password.isNotEmpty) {
      sb.write('PWD=${_conn.password};');
    }
    return sb.toString();
  }

  @override
  Future<void> close() async {
    final odbc = _odbc;
    _odbc = null;
    if (odbc != null) {
      try {
        await odbc.disconnect();
      } catch (_) {
        // 断开时忽略错误(文件可能已被移走)
      }
    }
  }

  DartOdbc _get() {
    final odbc = _odbc;
    if (odbc == null) {
      throw Exception('Access 数据库未连接');
    }
    return odbc;
  }

  /// 方括号包裹标识符,防表名与关键字冲突
  String _quoted(String name) => '[${name.replaceAll(']', ']]')}]';

  @override
  Future<List<String>> listDatabases() async {
    // Access 一个文件即一个数据库,返回文件名作为唯一"数据库"
    final path = _conn.host;
    if (path.isEmpty) return [];
    final name = path.replaceAll('\\', '/').split('/').last;
    return [name];
  }

  @override
  Future<List<String>> listSchemas(String database) async {
    // Access 单文件即一个数据库,无模式层,返回空列表(树不渲染模式节点)
    return const [];
  }

  @override
  Future<List<String>> listTables(String database, {String? schema}) async {
    final odbc = _get();
    // ODBC SQLTables 返回 TABLE_NAME / TABLE_TYPE 等列;
    // 过滤 TABLE_TYPE = 'TABLE' 且排除系统表(MSys 前缀)
    final tables = await odbc.getTables();
    return [
      for (final t in tables)
        if (t['TABLE_TYPE'] == 'TABLE' &&
            !(t['TABLE_NAME']?.toString().startsWith('MSys') ?? true))
          t['TABLE_NAME'].toString(),
    ];
  }

  @override
  Future<List<String>> listViews(String database, {String? schema}) async {
    final odbc = _get();
    final tables = await odbc.getTables();
    return [
      for (final t in tables)
        if (t['TABLE_TYPE'] == 'VIEW' &&
            !(t['TABLE_NAME']?.toString().startsWith('MSys') ?? true))
          t['TABLE_NAME'].toString(),
    ];
  }

  // Access 无表 / 视图 / 函数注释概念,补全侧无注释可展示
  @override
  Future<Map<String, String>> listTableComments(String database,
          {String? schema}) async =>
      const {};

  @override
  Future<Map<String, String>> listViewComments(String database,
          {String? schema}) async =>
      const {};

  @override
  Future<Map<String, int>> listTableRowEstimates(String database,
          {String? schema}) async =>
      const {};

  @override
  Future<Map<String, String>> listFunctionComments(String database,
          {String? schema}) async =>
      const {};

  @override
  Future<List<String>> listMaterializedViews(String database,
          {String? schema}) async =>
      const <String>[];

  @override
  Future<List<String>> listFunctions(String database, {String? schema}) async {
    // Access 无存储函数枚举机制
    return const [];
  }

  @override
  Future<List<String>> listProcedures(String database, {String? schema}) async {
    // Access 无存储过程枚举机制
    return const [];
  }

  @override
  Future<List<String>> listSequences(String database, {String? schema}) async {
    // Access 无独立序列对象(「自动编号」是列属性)
    return const [];
  }

  @override
  Future<SequenceDef?> readSequence(String database, String name,
          {String? schema}) async =>
      null;

  @override
  Future<List<String>> listUsers(String database) async {
    try {
      final odbc = _get();
      // MSysAccounts 仅在未启用用户级安全时包含 Admin
      final rows = await odbc.execute(
        'SELECT Name FROM MSysAccounts WHERE Pid IS NOT NULL ORDER BY Name',
      );
      return [
        for (final row in rows)
          if (row['Name'] != null) row['Name'].toString(),
      ];
    } catch (_) {
      // 系统表默认不可访问时静默返回空列表
      return const [];
    }
  }

  // Access 无服务端账号管理(MSysAccounts 是工作组的本地账户表,
  // 不支持 CREATE USER / GRANT),故下列方法全部空实现。
  @override
  Future<UserSpec?> readUser(String database, String account) async => null;

  @override
  Future<List<List<String>>> readUserPrivileges(
    String database,
    String account, {
    bool serverLevel = false,
  }) async =>
      const [];

  @override
  Future<List<String>> listGrantableRoles(String database) async => const [];

  @override
  Future<List<String>> readUserRoles(String database, String account) async =>
      const [];

  @override
  Future<List<String>> readRoleMembers(String database, String account) async =>
      const [];

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
    final odbc = _get();
    // Jet SQL 无 OFFSET 语法:一次取到 offset+limit 行,再本地跳过 offset 截取 limit
    final fetched = await odbc.execute(
      'SELECT TOP ${offset + limit} * FROM ${_quoted(table)}'
      '${whereClauseSql(where)}${orderByClauseSql(orderBy)}',
    );
    final rows = offset > 0 && fetched.length > offset
        ? fetched.skip(offset).take(limit).toList()
        : fetched;

    if (rows.isEmpty) {
      return TablePreview(
          columns: const [], rows: const [], limit: limit, nullMask: const []);
    }
    // 从首行的 key 集推断列名(保持 Map 迭代顺序)
    final columns = rows.first.keys.toList();
    final outRows = <List<String>>[];
    // nullMask 记录每格的原始 null 判定:展示层里真 NULL 与字符串 "NULL"
    // 都是 "NULL",导出 / 导入必须靠它区分
    final nullMask = <List<bool>>[];
    for (final row in rows) {
      final cells = <String>[];
      final nulls = <bool>[];
      for (final col in columns) {
        final v = row[col];
        cells.add(v?.toString() ?? 'NULL');
        nulls.add(v == null);
      }
      outRows.add(cells);
      nullMask.add(nulls);
    }
    return TablePreview(
      columns: columns,
      rows: outRows,
      limit: limit,
      nullMask: nullMask,
    );
  }

  @override
  Future<int> countTable(String database, String table,
      {String? schema, String? where}) async {
    final odbc = _get();
    final rows = await odbc.execute(
      'SELECT COUNT(*) AS cnt FROM ${_quoted(table)}${whereClauseSql(where)}',
    );
    return parseCountValue(rows.isNotEmpty ? rows.first['cnt'] : 0);
  }

  @override
  Future<void> useDatabase(String database) async {
    // Access 单文件即一个库,无需切换
  }

  @override
  Future<void> useSchema(String? schema) {
    // Access 无模式概念,仅 PostgreSQL 家族支持;
    // UI 层按类型显隐,不会对其调用本方法
    throw UnsupportedError('该数据库类型不支持会话级模式切换');
  }

  @override
  Future<QueryResult> executeQuery(String sql,
      {int limit = 1000, int offset = 0}) async {
    // 与 SQL Server 驱动共用封顶流式取数:odbc.execute 会把整棵结果集
    // 一次性抽干拷回,大表 SELECT * 会顶爆内存(详见 odbcQueryCapped)
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
  Future<int?> serverSessionId() async => null;

  @override
  Future<void> killSession(int sessionId) async {
    // Access 本地文件型,查询在进程内执行,无服务端会话可取消
  }

  @override
  Future<List<ColumnDef>> describeTable(String database, String table,
      {String? schema}) async {
    // Access 经 ODBC 难以稳定获取列类型,这里回退为预览列名(类型标记为 "—")
    final preview = await previewTable(database, table, limit: 1);
    return [
      for (final col in preview.columns)
        ColumnDef(name: col, type: '—'),
    ];
  }

  /// 「设计表」不支持编辑:ACE ODBC 的元数据能力不足以反查约束 / 索引,
  /// 且 JET SQL 无 ALTER COLUMN。返回 null 使界面转为只读展示。
  @override
  Future<DesignTable?> readTableDesign(String database, String table,
          {String? schema}) async =>
      null;

  /// 设计器下拉候选:ACE ODBC 无排序规则 / 表空间目录,返回空集退化为手输。
  @override
  Future<DesignCandidates> readDesignCandidates(String database) async =>
      DesignCandidates.empty;

  /// 库 / 表详情:ACE/Jet 是文件型引擎,无字符集 / 存储引擎 / 段大小这类
  /// 目录统计,返回 null 使详情面板回退到基础展示。
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
          {String? schema}) async =>
      null;
}
