import 'dart:convert';

import 'package:mongo_dart/mongo_dart.dart';

import '../db_data.dart';
import '../table_design.dart';
import '../user_sql.dart';
import 'db_driver.dart';

/// MongoDB 驱动 —— **只读阶段**。
///
/// 与关系型驱动的三处根本差异，决定了本文件的形状：
/// 1. **集合没有声明式 schema**。列结构只能从样本推断，故 [describeTable] 标为
///    推断值、[readTableDesign] 返回 null(设计器转只读)。
/// 2. **`runCommand` 在服务端报错时不抛异常**，只回 `{ok: 0, errmsg, codeName}`
///    (实测:Unauthorized / BSONObjectTooLarge 均如此)。所有命令过 [_cmd]，
///    由它把 ok!=1 折成异常 —— 否则界面表现为「空列表」而不是报错。
/// 3. **`Db` 绑死 URI 里的库名**(实测 `collection('db2.users')` 会被当成本库
///    一个叫 `db2.users` 的集合并**静默返回 0 行**)。`databaseName` 可变，
///    换库靠改这个字段；代价是它属于共享状态，所以「改库名 + 发命令」必须
///    整体串行(见 [_serial])，否则两个并发操作会打到互错的库上。
///
/// 未实现(调用即抛，不静默降级):写操作、SQL 查询页、WHERE 过滤。
class MongoDriver implements DatabaseDriver {
  MongoDriver(this._conn);

  final ConnectionInfo _conn;
  Db? _db;

  /// 串行链:护住 `_db.databaseName` 的改写(见类注释第 3 点)。
  Future<void> _tail = Future.value();

  /// 系统命名空间前缀:8.0 实测 `getCollectionNames` 会混进 `system.views`,
  /// 它们不是用户对象,列表与行数估算都要排除。
  static const _kSystemPrefix = 'system.';

  @override
  bool get isConnected => _db?.isConnected ?? false;

  @override
  Future<void> connect() async {
    if (_db != null) return;
    final db = await Db.create(_uri());
    await db.open();
    _db = db;
  }

  @override
  Future<void> close() async {
    final db = _db;
    _db = null;
    if (db == null) return;
    try {
      await db.close();
    } on Object {
      // 已断开的连接重复 close 会抛状态错;断开本来就是目标,吞掉
    }
  }

  /// 组装连接串。
  ///
  /// `authSource` 固定 admin:MongoDB 的用户库位置无法从连接参数推断,
  /// 而「管理员账号建在 admin」是压倒性的常见约定。等连接表单补上
  /// 「认证数据库」输入框后再改这里。
  String _uri() {
    final port = _conn.port.isEmpty ? '27017' : _conn.port;
    final cred = _conn.username.isEmpty
        ? ''
        : '${Uri.encodeComponent(_conn.username)}:'
            '${Uri.encodeComponent(_conn.password)}@';
    return 'mongodb://$cred${_conn.host}:$port/?authSource=admin';
  }

  Db _get() {
    final db = _db;
    if (db == null) throw Exception('MongoDB 未连接');
    return db;
  }

  /// 在串行链上把会话切到 [database] 后执行 [body]。
  Future<T> _serial<T>(String database, Future<T> Function(Db db) body) {
    final next = _tail.then((_) async {
      final db = _get();
      if (database.isNotEmpty) db.databaseName = database;
      return body(db);
    });
    // 链尾吞错:下一次调用不该因为上一次失败而被跳过
    _tail = next.then<void>((_) {}, onError: (_) {});
    return next;
  }

  /// 发一条命令并把 `ok != 1` 折成异常(见类注释第 2 点)。
  Future<Map<String, dynamic>> _cmd(
      String database, Map<String, Object> command, String what) async {
    final r = await _serial(database, (db) => db.runCommand(command));
    final ok = (r['ok'] as num?)?.toDouble();
    if (ok != 1.0) {
      throw Exception(
          '$what 失败:${r['errmsg'] ?? '未知原因'}(${r['codeName'] ?? 'NO_CODE'})');
    }
    return r;
  }

  /// 集合名单(库级)。[_listNames] 的 sort 参数:服务端不保证顺序,
  /// 界面按名称展示需要稳定序,这里统一排一次。
  Future<List<String>> _listNames(String database,
      {required bool views}) async {
    final infos = await _serial(database, (db) => db.getCollectionInfos());
    final out = <String>[];
    for (final e in infos) {
      final name = e['name'] as String?;
      if (name == null || name.startsWith(_kSystemPrefix)) continue;
      if ((e['type'] == 'view') == views) out.add(name);
    }
    out.sort();
    return out;
  }

  @override
  Future<List<String>> listDatabases() async {
    // listDatabases() 助手自己把命令路由到 admin 库(实测),不需要另建连接
    final names = await _get().listDatabases();
    return names.whereType<String>().toList()..sort();
  }

  @override
  Future<List<String>> listSchemas(String database) async => const [];

  @override
  Future<List<String>> listTables(String database, {String? schema}) =>
      _listNames(database, views: false);

  @override
  Future<List<String>> listViews(String database, {String? schema}) =>
      _listNames(database, views: true);

  /// 集合注释:实测 8.0 的 `create ... {comment: ...}` **不会**被
  /// `listCollections` 回显(options 恒空),没有可用的落点 → 返回空 map,
  /// 界面按「无注释」渲染。
  @override
  Future<Map<String, String>> listTableComments(String database,
          {String? schema}) async =>
      const {};

  @override
  Future<Map<String, String>> listViewComments(String database,
          {String? schema}) async =>
      const {};

  /// 行数估算:走 `count` 命令的**无条件**形态(服务端读集合元数据,不扫文档)。
  ///
  /// 没有「一次覆盖全库」的取法:实测 DB 级 `$collStats` 管道要求命名空间存在,
  /// 打 `$cmd` 直接 NamespaceNotFound。逐集合 count 实测 9 个 7ms,代价与
  /// [listTables] 同级;某个集合取不到就不带键(界面显示横杠),不整体失败。
  @override
  Future<Map<String, int>> listTableRowEstimates(String database,
      {String? schema}) async {
    final names = await _listNames(database, views: false);
    final out = <String, int>{};
    for (final name in names) {
      try {
        final r = await _cmd(database, {'count': name}, '统计集合「$name」行数');
        final n = r['n'];
        if (n is num) out[name] = n.toInt();
      } on Object {
        continue;
      }
    }
    return out;
  }

  @override
  Future<Map<String, String>> listFunctionComments(String database,
          {String? schema}) async =>
      const {};

  @override
  Future<List<String>> listMaterializedViews(String database,
          {String? schema}) async =>
      const [];

  // MongoDB 没有可枚举的存储函数 / 过程 / 序列对象(聚合表达式不是目录项)。
  @override
  Future<List<String>> listFunctions(String database, {String? schema}) async =>
      const [];

  @override
  Future<List<String>> listProcedures(String database,
      {String? schema}) async =>
      const [];

  @override
  Future<List<String>> listSequences(String database,
      {String? schema}) async =>
      const [];

  @override
  Future<SequenceDef?> readSequence(String database, String name,
          {String? schema}) async =>
      null;

  /// 用户列表:MongoDB 的用户属于**具体库**(`usersInfo` 是库级命令),
  /// 而界面的用户分组按 [kUserTypes] 显隐,mongodb 不在其中 → 不会被调用。
  /// 等要做用户管理时再实现,不要在这里放半条路径。
  @override
  Future<List<String>> listUsers(String database) async => const [];

  @override
  Future<UserSpec?> readUser(String database, String account) async => null;

  @override
  Future<List<List<String>>> readUserPrivileges(String database, String account,
      {bool serverLevel = false}) async =>
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
  Future<TablePreview> previewTable(String database, String table,
      {int limit = 100,
      int offset = 0,
      String? schema,
      String? where,
      String? orderBy}) async {
    _rejectSqlWhere(where);
    final sort = mongoSortFromOrderBy(orderBy);
    // 取 limit 行需要 skip(offset):实测 skip 49900 仍 17ms,服务端翻页成立
    final docs = await _serial(
        database,
        (db) => db
            .collection(table)
            .find(_pageSelector(limit: limit, offset: offset, sort: sort))
            .toList());
    return docsToPreview(docs, limit: limit);
  }

  @override
  Future<int> countTable(String database, String table,
      {String? schema, String? where}) async {
    _rejectSqlWhere(where);
    final r = await _cmd(database, {'count': table}, '统计集合「$table」行数');
    final n = r['n'];
    if (n is! num) {
      throw Exception('统计集合「$table」行数 失败:回包没有 n 字段');
    }
    return n.toInt();
  }

  @override
  Future<void> useDatabase(String database) =>
      _serial(database, (db) async {});

  @override
  Future<void> useSchema(String? schema) async =>
      throw UnsupportedError('该数据库类型不支持会话级模式切换');

  /// SQL 控制台对文档库没有意义:查询页送来的是 SQL 文本,这里不接受
  /// 「看着像就翻译一下」的糊弄,直接明确拒绝。
  /// 下一阶段若要在查询页跑 JSON 过滤器 / 聚合管道,得连带改查询页的
  /// 输入语义与补全,不是在这里加个正则。
  @override
  Future<QueryResult> executeQuery(String sql,
      {int limit = 1000, int offset = 0}) async {
    throw UnsupportedError(
        'MongoDB 连接不支持 SQL 执行;集合数据请在表数据页浏览(聚合查询在后续阶段提供)。');
  }

  @override
  Future<int?> serverSessionId() async {
    final r = await _get().runCommand({'hello': 1});
    final id = r['connectionId'];
    return id is num ? id.toInt() : null;
  }

  /// 取消正在执行的操作。
  ///
  /// `killOp` 要的是 **opid**,而 [serverSessionId] 给的是 connectionId,
  /// 两者不是一回事(实测 currentOp 回包里两个键都有)。所以先查 currentOp
  /// 按 `desc == 'conn<connectionId>'` 找到对应 op 再杀。
  /// currentOp 只能在 admin 库上跑(实测在用户库直接 ok:0),故临时改库名。
  @override
  Future<void> killSession(int sessionId) async {
    final bound = _db?.databaseName;
    final ops = await _serial('admin', (db) async {
      final r = await db.runCommand({'currentOp': 1});
      return (r['inprog'] as List?) ?? const [];
    });
    // currentOp 只能在 admin 上跑,但它会连带把会话改绑到 admin —— 用完还原,
    // 否则调用方下次不显式 useDatabase 的操作就打到 admin 库上
    if (bound != null) await _serial(bound, (db) async {});
    final desc = 'conn$sessionId';
    for (final o in ops) {
      if (o is Map && o['desc'] == desc) {
        final opid = o['opid'];
        if (opid is num) {
          await _cmd('admin', {'killOp': 1, 'op': opid.toInt()}, '取消会话');
        }
        return;
      }
    }
    // 没有匹配的在跑操作 = 没什么可取消,不是错误
  }

  /// 列结构:**从样本推断**,不是声明式 schema(见类注释第 1 点)。
  ///
  /// 类型取 BSON 类型名的并集(同一列出现多种类型会并列展示),`_id` 标主键。
  /// 界面把它当「设计表」的只读初值用,不参与 DDL 生成。
  @override
  Future<List<ColumnDef>> describeTable(String database, String table,
      {String? schema}) async {
    final docs = await _serial(database,
        (db) => db.collection(table).find(where.limit(_kShapeSample)).toList());
    if (docs.isEmpty) return const [];
    final order = <String>[];
    final types = <String, Set<String>>{};
    final sawNull = <String>{};
    for (final doc in docs) {
      doc.forEach((key, value) {
        if (!types.containsKey(key)) {
          order.add(key);
          types[key] = <String>{};
        }
        if (value == null) {
          sawNull.add(key);
        } else {
          types[key]!.add(mongoBsonTypeName(value));
        }
      });
    }
    return [
      for (final name in order)
        ColumnDef(
          name: name,
          type: types[name]!.isEmpty ? 'null' : (types[name]!.toList()..sort()).join(' | '),
          nullable: sawNull.contains(name) || types[name]!.length != 1,
          primaryKey: name == '_id',
        ),
    ];
  }

  @override
  Future<DesignTable?> readTableDesign(String database, String table,
          {String? schema}) async =>
      null;

  @override
  Future<DesignCandidates> readDesignCandidates(String database) async =>
      DesignCandidates.empty;

  /// 库级详情:dbStats 给的是 collections/dataSize 这类量,
  /// 而 [DatabaseDetail] 的槽位全是字符集 / 排序规则 / 所有者,装不下 → 返回 null,
  /// 界面回退基础展示。
  @override
  Future<DatabaseDetail?> readDatabaseDetail(String database) async => null;

  @override
  Future<TableDetail?> readTableDetail(String database, String table,
      {String? schema}) async {
    final r = await _cmd(database, {'collStats': table}, '读取集合「$table」统计');
    // 8.0 实测 collStats 已无 n / nsAvgObjSize,行数只能另取 count(见上方估算)
    final wt = r['wiredTiger'];
    return TableDetail(
      name: table,
      engine: wt is Map ? 'WiredTiger' : '',
      rowEstimate: await _countOrNull(database, table),
      dataLength: (r['size'] as num?)?.toInt(),
      indexLength: (r['totalIndexSize'] as num?)?.toInt(),
      tableType: 'collection',
    );
  }

  Future<int?> _countOrNull(String database, String table) async {
    try {
      final r = await _cmd(database, {'count': table}, '统计行数');
      final n = r['n'];
      return n is num ? n.toInt() : null;
    } on Object {
      return null;
    }
  }

  /// MongoDB 没有依赖目录(视图对集合的引用能从 viewOn 读到,但不是可枚举的
  /// 反向依赖图),返回 null → 详情面板不出「使用 / 被使用」两个页签。
  @override
  Future<List<DependentObject>?> readTableDependencies(String database,
          String table,
          {String? schema, bool usedBy = true}) async =>
      null;

  @override
  Future<String?> getDefinition(String database, String name, String kind,
      {String? schema}) async {
    if (kind != 'view') return null;
    final infos = await _serial(database,
        (db) => db.getCollectionInfos({'name': name}));
    if (infos.isEmpty) return null;
    final options = infos.first['options'];
    if (options is! Map || options['viewOn'] == null) return null;
    // 服务端只存 viewOn + pipeline,没有定义文本;按 createView 的形状重建,
    // 让 DDL 页能看见真实管道而不是空白
    return 'db.createView(${jsonEncode(name)}, '
        '${jsonEncode(options['viewOn'])}, '
        '${jsonEncode(options['pipeline'] ?? <Object>[])});';
  }

  // ── 只读阶段的拦口 ──────────────────────────────────────

  /// 表数据页的筛选框发的是 SQL `WHERE` 片段。Mongo 的过滤器是 JSON,
  /// 二者语法交集小到「按字符串切分」必然切错(`a = 1` 里的 `=`、`_id` 的
  /// ObjectId、`AND/OR` 优先级)。这里一律拒绝,不做半吊子翻译:
  /// 静默忽略过滤器会让用户看见**没被过滤**的数据却以为过滤了。
  void _rejectSqlWhere(String? where) {
    if (where == null || where.trim().isEmpty) return;
    throw UnsupportedError(
        'MongoDB 集合暂不支持 SQL WHERE 过滤(过滤器语法与 SQL 不同)。');
  }

  /// 首页取数的 SelectorBuilder:limit/skip/orderby 都交给服务端。
  SelectorBuilder _pageSelector(
      {required int limit, required int offset, Map<String, Object>? sort}) {
    var builder = where.limit(limit);
    if (offset > 0) builder = builder.skip(offset);
    if (sort != null) {
      sort.forEach((column, dir) {
        builder = builder.sortBy(column, descending: dir == -1);
      });
    }
    return builder;
  }

  /// 文档 → 表格:列 = 本页文档键的并集(保持首见顺序),
  /// 单元格按 [cell] 文本化,并同步记 nullMask。
  ///
  /// 集合里字段本来就可能缺(实测同集合三条文档键数为 10/9/3),
  /// 缺键与显式 null 在这里都算「无值」:网格没有第三种槽位,
  /// 但 nullMask 要写对,否则导出时把空串和 NULL 混成一类。
  TablePreview docsToPreview(List<Map<String, dynamic>> docs,
      {int limit = 100}) {
    final columns = <String>[];
    for (final doc in docs) {
      for (final key in doc.keys) {
        if (!columns.contains(key)) columns.add(key);
      }
    }
    final rows = <List<String>>[];
    final nullMask = <List<bool>>[];
    for (final doc in docs) {
      final row = <String>[];
      final mask = <bool>[];
      for (final column in columns) {
        final value = doc[column];
        row.add(cell(value));
        mask.add(value == null || !doc.containsKey(column));
      }
      rows.add(row);
      nullMask.add(mask);
    }
    return TablePreview(
        columns: columns, rows: rows, limit: limit, nullMask: nullMask);
  }

  /// 样本行数:describeTable 用。取 100 条足以覆盖常见字段分布,
  /// 又不把大集合整份拉回来。
  static const _kShapeSample = 100;
}

/// 单元格文本化:与 MySQL 驱动的口径一致(值一律转字符串,NULL 交给 nullMask)。
///
/// BSON 的类型各自有专门的展示形态,直接 `'$v'` 会得到
/// `ObjectId("…")` / `BsonBinary(…)` 这种带类名的壳,所以按类型剥开:
/// - ObjectId → 24 位十六进制本体(`.oid`)
/// - 二进制 → 长度 + 十六进制转储(与 MySQL 的 HEX 列同一读法)
/// - 嵌套文档 / 数组 → 紧凑 JSON(网格单元格里可读,超长由界面截断)
/// - DateTime → ISO8601(实测解码即 UTC)
String cell(Object? v) {
  if (v == null) return 'NULL';
  if (v is ObjectId) return v.oid;
  if (v is BsonBinary) {
    final hex = v.hexString;
    return '${v.byteList.length}B ${hex.length > 64 ? '${hex.substring(0, 64)}…' : hex}';
  }
  if (v is DateTime) return v.toUtc().toIso8601String();
  if (v is Map || v is List) return _json(v);
  return '$v';
}

/// BSON 类型名(供 describeTable 标注推断出的列类型)。
///
/// Int64 / Decimal 来自 mongo_dart 未转出的传递依赖包(fixnum、decimal),
/// 不为此多引两个包 —— 它们的 `runtimeType` 名就够用且稳定。
///
/// 每个分支都必须写成**对象模式** `ObjectId()` 而不是裸 `ObjectId`：模式里的
/// 裸类名会被解析成「值等于该 Type 字面量」的常量模式，永远不成立，于是所有
/// 类型都静默走到最后的 runtimeType 兜底（`objectid` / `_map<dynamic, dynamic>`）。
/// 编译器不报错，靠 [test/mongo_driver_test.dart] 钉住。
String mongoBsonTypeName(Object v) => switch (v) {
      ObjectId() => 'objectId',
      BsonBinary() => 'binary',
      DateTime() => 'date',
      bool() => 'bool',
      int() => 'int',
      double() => 'double',
      String() => 'string',
      Map() => 'object',
      List() => 'array',
      _ => v.runtimeType.toString().toLowerCase(),
    };

String _json(Object v) {
  try {
    return jsonEncode(v);
  } on Object {
    // 文档里可能有 jsonEncode 处理不了的 BSON 叶子(如未预期的类型),
    // 退化成 toString 让单元格至少可读,不让整张表打不开
    return '$v';
  }
}

/// 只认「单列 `ORDER BY x [ASC|DESC]`」这一种形态(点列头排序产生的就是它),
/// 标识符按驱动收到的写法剥掉引用符。复合 / 表达式 / 带函数的排序一律拒绝,
/// 理由同上:认错方向就是把数据排错给用户看。
///
/// 锚点写在原始字符串里就是 `$` 本身 —— 写成 `\$` 会被正则当成「字面美元符」,
/// 于是任何正常排序都匹配不上、全部走拒绝分支(第一版就是这么写的)。
final RegExp _kOrderBySingle = RegExp(
    r'^\s*(?:`([^`]+)`|\[([^\]]+)\]|"([^"]+)"|([A-Za-z0-9_.$一-鿿]+))'
    r'\s*(asc|desc)?\s*$',
    caseSensitive: false);

Map<String, Object>? mongoSortFromOrderBy(String? orderBy) {
  final text = orderBy?.trim();
  if (text == null || text.isEmpty) return null;
  final m = _kOrderBySingle.firstMatch(text);
  if (m == null) {
    throw UnsupportedError('MongoDB 集合暂只支持按单列排序:$orderBy');
  }
  final column = m.group(1) ?? m.group(2) ?? m.group(3) ?? m.group(4)!;
  final dir = (m.group(5) ?? '').toLowerCase() == 'desc' ? -1 : 1;
  return {column: dir};
}
