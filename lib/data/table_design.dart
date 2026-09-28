/// 新建表设计器的数据模型与按数据库方言生成 DDL 的工具。
///
/// 模型描述 pgAdmin 风格表设计器各标签页采集的内容:
/// 字段 / 索引 / 外键 / 唯一键 / 检查 / 排除 / 规则 / 触发器 / 选项 / 注释。
/// [DdlBuilder] 将 [DesignTable] 转换为目标数据库类型(连接类型 id)的
/// CREATE TABLE 及其关联语句;SQL 预览标签与「保存」共用同一生成逻辑,
/// 保证预览即所见、保存即所写。
library;

/// PostgreSQL 家族类型 id(触发器 / 规则 / 排除约束等仅此家族生成 DDL)
const Set<String> kPgLikeTypes = {
  'postgresql',
  'aliyun-rds-postgres',
  'aliyun-polardb-postgres',
  'aliyun-oceanbase-postgres',
};

/// MySQL / MariaDB 家族类型 id
const Set<String> kMysqlLikeTypes = {'mysql', 'mariadb'};

/// SQL Server 家族类型 id
const Set<String> kSqlServerLikeTypes = {'sqlserver', 'aliyun-rds-sqlserver'};

/// 填充因子的 reloption 名(各方言共用这一项)
const String kPgFillFactor = 'fillfactor';

/// PostgreSQL 表级存储参数的稳定输出顺序(`fillfactor` 恒在最前,其余按此清单
/// 排,清单外的键按字典序排在末尾)。界面只开放参考工具有的那几项,库里读到的
/// 其它 reloption 不显示但**原样保留并参与生成**,不设计就不会被保存清掉。
const List<String> kPgStorageOptionOrder = [
  'autovacuum_enabled',
  'autovacuum_vacuum_threshold',
  'autovacuum_vacuum_scale_factor',
  'autovacuum_analyze_threshold',
  'autovacuum_analyze_scale_factor',
  'autovacuum_vacuum_insert_threshold',
  'autovacuum_vacuum_insert_scale_factor',
  'autovacuum_vacuum_cost_limit',
  'autovacuum_freeze_max_age',
  'log_autovacuum_min_duration',
  'toast_autovacuum_enabled',
  'parallel_workers',
  'user_catalog_table',
];

/// 不接受长度 / 精度参数的类型(小写比较)。
/// 「长度」列对类型是信息性展示(如 int8 的 64 位宽),
/// 拼进 DDL 会得到 `int8(64)` 这类非法语句,故 [DesignColumn.fullType] 跳过。
const Set<String> kNoLengthTypes = {
  'bool',
  'boolean',
  'int',
  'int2',
  'int4',
  'int8',
  'integer',
  'smallint',
  'mediumint',
  'bigint',
  'tinyint',
  'serial',
  'smallserial',
  'bigserial',
  'uuid',
  'date',
  'money',
  'text',
  'tinytext',
  'mediumtext',
  'longtext',
  'json',
  'jsonb',
  'xml',
  'bytea',
  'blob',
  'tinyblob',
  'mediumblob',
  'longblob',
  'inet',
  'cidr',
  'macaddr',
  'real',
  'double precision',
  'point',
  'line',
  'lseg',
  'box',
  'path',
  'polygon',
  'circle',
  'tsvector',
  'tsquery',
};

/// MySQL 系类型修饰符(出现在类型名之后,如 `int unsigned` / `decimal(10,2) zerofill`)。
/// 设计器把修饰符与基础类型同存于 [DesignColumn.type],渲染时由 [DesignColumn.fullType]
/// 重新放到长度之后,避免 `unsigned` 被静默丢弃而改变列的符号性。
const Set<String> kTypeModifiers = {'unsigned', 'signed', 'zerofill'};

/// 类型修饰符拆分结果:基础类型名 + 修饰符(小写,空串 = 无)。
typedef TypeModifierSplit = ({String base, String modifier});

/// 从类型名中分离修饰符:`int unsigned` → `(base: int, modifier: unsigned)`。
/// 大小写不敏感,基础类型名保留原样(供 DDL 输出)。
TypeModifierSplit splitTypeModifier(String type) {
  final parts = type.trim().split(RegExp(r'\s+')).where((p) => p.isNotEmpty).toList();
  final mods = <String>[];
  final words = <String>[];
  for (final p in parts) {
    if (kTypeModifiers.contains(p.toLowerCase())) {
      mods.add(p.toLowerCase());
    } else {
      words.add(p);
    }
  }
  return (base: words.join(' '), modifier: mods.join(' '));
}

/// 整数类型的位宽(仅用于「长度」列的信息性展示;不参与 DDL 渲染)。
/// 返回 null 表示该类型有真实的长度 / 精度语义。
String? bitWidthOf(String type) {
  switch (splitTypeModifier(type).base.toLowerCase()) {
    case 'int2':
    case 'smallint':
    case 'smallserial':
      return '16';
    case 'tinyint':
      return '8';
    case 'mediumint':
      return '24';
    case 'int':
    case 'int4':
    case 'integer':
    case 'serial':
      return '32';
    case 'int8':
    case 'bigint':
    case 'bigserial':
      return '64';
    default:
      return null;
  }
}

/// 整数类型作为 IDENTITY 序列上限时的 MAXVALUE(空串 = 无法确定,留空由服务端取默认)。
String kIdentityMaxValue(String type) {
  switch (splitTypeModifier(type).base.toLowerCase()) {
    case 'int2':
    case 'smallint':
    case 'smallserial':
    case 'tinyint':
      return '32767';
    case 'int':
    case 'int4':
    case 'integer':
    case 'serial':
    case 'mediumint':
      return '2147483647';
    case 'int8':
    case 'bigint':
    case 'bigserial':
      return '9223372036854775807';
    default:
      return '';
  }
}

/// 字段(列)设计。类型以「基础类型 + 长度 + 小数点」分段录入,
/// 渲染 DDL 时拼为 `base(len[,decimal])`;长度 / 小数点留空则只输出基础类型。
/// 属于 [kNoLengthTypes] 的类型(如 int8 / text / uuid)本身无长度参数:
/// 其「长度」仅作信息性展示(位宽),不进入 [fullType]。
/// IDENTITY(界面「虚拟类型」)由 [identityMode] 及其序列选项描述。
class DesignColumn {
  DesignColumn({
    this.name = '',
    this.type = 'varchar',
    this.length = '255',
    this.decimal = '',
    this.notNull = false,
    this.primaryKey = false,
    this.autoIncrement = false,
    this.comment = '',
    this.defaultValue = '',
    this.collation = '',
    this.dimension = '',
    this.identityMode = '',
    this.identityIncrement = '',
    this.identityMinValue = '',
    this.identityMaxValue = '',
    this.identityStart = '',
    this.identityCache = '',
    this.identityCycle = false,
  });

  /// 列名
  String name;

  /// 基础类型名(如 varchar / int / decimal)
  String type;

  /// 长度(如 varchar(255) 的 255;非数值类型可留空)
  String length;

  /// 小数点精度(如 decimal(10,2) 的 2)
  String decimal;

  /// 是否 NOT NULL
  bool notNull;

  /// 是否主键
  bool primaryKey;

  /// 列注释
  String comment;

  /// 默认值(字符串需自行加引号,与既有建表约定一致)
  String defaultValue;

  /// 排序规则
  String collation;

  /// 维度(数组类型维数,PostgreSQL;生成 DDL 时拼为 `base[dim]`)
  String dimension;

  /// 自增(仅 MySQL / MariaDB 生成 AUTO_INCREMENT)
  bool autoIncrement;

  /// IDENTITY 模式:空(无) / `ALWAYS` / `BY DEFAULT`(界面「虚拟类型」)
  /// PostgreSQL 生成 GENERATED … AS IDENTITY;MySQL 降级为 AUTO_INCREMENT;
  /// SQL Server 生成 IDENTITY(seed, increment);SQLite / Access 忽略。
  String identityMode;

  /// IDENTITY 序列选项(空 = 不输出该子句,交由数据库默认)
  String identityIncrement;
  String identityMinValue;
  String identityMaxValue;
  String identityStart;
  String identityCache;

  /// IDENTITY 是否循环(CYCLE)
  bool identityCycle;

  /// 是否已启用 IDENTITY
  bool get hasIdentity => identityMode.trim().isNotEmpty;

  /// 渲染到 DDL 的完整类型文本。
  ///
  /// [type] 允许携带 MySQL 修饰符(如 `int unsigned`):修饰符渲染在长度
  /// 之后(`int(11) unsigned`),否则会得到非法的 `int unsigned(11)`。
  String get fullType {
    final mod = splitTypeModifier(type);
    final t = mod.base;
    if (t.isEmpty) return '';
    final tail = mod.modifier.isEmpty ? '' : ' ${mod.modifier}';
    // 无长度参数的类型:长度 / 小数点仅作界面信息展示,不拼接
    if (kNoLengthTypes.contains(t.toLowerCase())) return '$t$tail';
    final len = length.trim();
    final dec = decimal.trim();
    if (len.isEmpty) return dec.isEmpty ? '$t$tail' : '$t($dec)$tail';
    return dec.isEmpty ? '$t($len)$tail' : '$t($len,$dec)$tail';
  }

  /// 深拷贝:「设计表」以原始快照为基线生成 ALTER 差异,不能与编辑中的对象共享引用
  DesignColumn copy() => DesignColumn(
        name: name,
        type: type,
        length: length,
        decimal: decimal,
        notNull: notNull,
        primaryKey: primaryKey,
        autoIncrement: autoIncrement,
        comment: comment,
        defaultValue: defaultValue,
        collation: collation,
        dimension: dimension,
        identityMode: identityMode,
        identityIncrement: identityIncrement,
        identityMinValue: identityMinValue,
        identityMaxValue: identityMaxValue,
        identityStart: identityStart,
        identityCache: identityCache,
        identityCycle: identityCycle,
      );
}

/// 索引字段项(对应「选择数据表字段」弹窗的一行)。
///
/// 各选项留空即不输出对应子句(交由数据库默认),因此未用过弹窗的既有设计
/// 数据(含驱动反查结果)生成的 DDL 与从前完全一致。
/// 子句仅 PostgreSQL 家族输出;其余类型按裸字段名渲染。
class DesignIndexColumn {
  DesignIndexColumn({
    this.name = '',
    this.collationSchema = '',
    this.collation = '',
    this.opClassSchema = '',
    this.opClass = '',
    this.order = '',
    this.nullsOrder = '',
    this.operatorSchema = '',
    this.operatorName = '',
  });

  /// 字段名
  String name;

  /// 排序规则模式(排序规则所在 schema,界面「排序规则模式」)
  String collationSchema;

  /// 排序规则(界面「排序规则」)
  String collation;

  /// 运算符类别模式(opclass 所在 schema,界面「运算符类别模式」)
  String opClassSchema;

  /// 运算符类别(界面「运算符类别」)
  String opClass;

  /// 排序顺序:空 / ASC / DESC(界面「排序顺序」)
  String order;

  /// Nulls 排序:空 / FIRST / LAST(界面「Nulls 排序」)
  String nullsOrder;

  /// 运算符模式(运算符所在 schema,仅排除约束的「元素」用,界面「运算符模式」)
  String operatorSchema;

  /// 比较运算符(仅排除约束的「元素」用,界面「运算符」):非空即输出
  /// `<元素> WITH <运算符>`;索引字段项忽略此项。
  String operatorName;

  /// 是否携带任何需要落进 DDL 的选项
  bool get hasOptions =>
      collation.trim().isNotEmpty ||
      opClass.trim().isNotEmpty ||
      order.trim().isNotEmpty ||
      nullsOrder.trim().isNotEmpty;

  DesignIndexColumn copy() => DesignIndexColumn(
        name: name,
        collationSchema: collationSchema,
        collation: collation,
        opClassSchema: opClassSchema,
        opClass: opClass,
        order: order,
        nullsOrder: nullsOrder,
        operatorSchema: operatorSchema,
        operatorName: operatorName,
      );
}

/// 索引设计。字段以逗号分隔(支持多列),索引方法留空用数据库默认。
class DesignIndex {
  DesignIndex({
    this.name = '',
    this.columns = '',
    this.method = '',
    this.unique = false,
    this.concurrent = false,
    this.comment = '',
    this.tablespace = '',
    this.fillFactor = '',
  });

  String name;
  String columns;
  String method;
  bool unique;
  bool concurrent;
  String comment;
  String tablespace;
  String fillFactor;

  /// 索引字段项(由「选择数据表字段」弹窗写入)。非空时 DDL 按本列表输出
  /// 每项的 COLLATE / 运算符类别 / 排序顺序 / Nulls 排序;为空时按 [columns] 输出裸字段名。
  final List<DesignIndexColumn> columnItems = [];

  List<String> get columnList =>
      columns.split(',').map((e) => e.trim()).where((e) => e.isNotEmpty).toList();

  /// 弹窗确认后把字段项回写成逗号串(网格「字段」格与差异比较都以 [columns] 为准)
  void applyColumnItems() {
    columns = columnItems
        .map((e) => e.name.trim())
        .where((e) => e.isNotEmpty)
        .join(', ');
  }

  DesignIndex copy() => DesignIndex(
        name: name,
        columns: columns,
        method: method,
        unique: unique,
        concurrent: concurrent,
        comment: comment,
        tablespace: tablespace,
        fillFactor: fillFactor,
      )..columnItems.addAll(columnItems.map((e) => e.copy()));
}

/// 外键设计。
class DesignForeignKey {
  DesignForeignKey({
    this.name = '',
    this.columns = '',
    this.refSchema = '',
    this.refTable = '',
    this.refColumns = '',
    this.onDelete = 'NO ACTION',
    this.onUpdate = 'NO ACTION',
    this.matchAll = false,
    this.deferrable = '',
    this.deferred = '',
    this.comment = '',
  });

  String name;
  String columns;
  String refSchema;
  String refTable;
  String refColumns;

  /// 删除时行为:NO ACTION / RESTRICT / CASCADE / SET NULL / SET DEFAULT
  String onDelete;

  /// 更新时行为:同上
  String onUpdate;

  /// 是否符合全部(MATCH FULL;仅 PostgreSQL 生成)
  bool matchAll;

  /// 可延迟:空 / YES / NO
  String deferrable;

  /// 延迟:空 / YES / NO
  String deferred;

  /// 约束注释(仅 PostgreSQL 生成 COMMENT ON CONSTRAINT)
  String comment;

  List<String> get columnList =>
      columns.split(',').map((e) => e.trim()).where((e) => e.isNotEmpty).toList();

  List<String> get refColumnList =>
      refColumns.split(',').map((e) => e.trim()).where((e) => e.isNotEmpty).toList();

  DesignForeignKey copy() => DesignForeignKey(
        name: name,
        columns: columns,
        refSchema: refSchema,
        refTable: refTable,
        refColumns: refColumns,
        onDelete: onDelete,
        onUpdate: onUpdate,
        matchAll: matchAll,
        deferrable: deferrable,
        deferred: deferred,
        comment: comment,
      );
}

/// 唯一键设计。
class DesignUniqueKey {
  DesignUniqueKey({
    this.name = '',
    this.columns = '',
    this.comment = '',
    this.tablespace = '',
    this.fillFactor = '',
    this.deferrable = '',
    this.deferred = '',
  });

  String name;
  String columns;
  String comment;
  String tablespace;
  String fillFactor;
  String deferrable;
  String deferred;

  List<String> get columnList =>
      columns.split(',').map((e) => e.trim()).where((e) => e.isNotEmpty).toList();

  /// 字段项(网格「字段」格的 `...` 弹窗写入)。非空时 UNIQUE 约束按逐项输出
  /// COLLATE / 运算符类别 / 排序;为空时按 [columns] 输出裸字段名。
  final List<DesignIndexColumn> columnItems = [];

  /// 弹窗确认后把字段项回写成逗号串(网格格与差异比较都以 [columns] 为准)
  void applyColumnItems() {
    columns = columnItems
        .map((e) => e.name.trim())
        .where((e) => e.isNotEmpty)
        .join(', ');
  }

  DesignUniqueKey copy() => DesignUniqueKey(
        name: name,
        columns: columns,
        comment: comment,
        tablespace: tablespace,
        fillFactor: fillFactor,
        deferrable: deferrable,
        deferred: deferred,
      )..columnItems.addAll(columnItems.map((e) => e.copy()));
}

/// 检查约束设计。
class DesignCheck {
  DesignCheck({
    this.name = '',
    this.expression = '',
    this.noInherit = false,
    this.comment = '',
  });

  String name;
  String expression;

  /// NO INHERIT(仅 PostgreSQL):子表不继承该约束
  bool noInherit;
  String comment;

  DesignCheck copy() => DesignCheck(
      name: name, expression: expression, noInherit: noInherit, comment: comment);
}

/// 排除约束设计(仅 PostgreSQL 家族生成 EXCLUDE USING ...)。
class DesignExclude {
  DesignExclude({
    this.name = '',
    this.columns = '',
    this.method = 'gist',
    this.predicate = '',
    this.deferrable = '',
    this.deferred = '',
    this.comment = '',
  });

  String name;

  /// 排除元素列表,PG 原样进括号,故每列的操作符在这里写全:
  /// `col WITH =` / `lower(name) WITH =` / `int4range(a,b) WITH &&`。
  String columns;

  /// 索引访问方法(gist / spgist / gin / brin / btree ...)
  String method;

  /// 部分排除条件的表达式(不带 WHERE 关键字),如 `state = 'active'`
  String predicate;

  /// 可延迟 / 初始延迟(YES / NO / 空 = 不生成子句,与唯一键同一口径)
  String deferrable;
  String deferred;
  String comment;

  /// 排除元素项(网格「元素」格的 `...` 弹窗写入)。非空时按逐项输出
  /// `<字段> [COLLATE …] [运算符类别] [ASC|DESC] [NULLS …] WITH <运算符>`;
  /// 为空时按 [columns] 原样进括号(手输与驱动反查走这条路径)。
  final List<DesignIndexColumn> columnItems = [];

  /// 弹窗确认后把元素项回写成 `字段 WITH 运算符` 逗号串(网格与差异比对看的是
  /// 这一串);生成 DDL 时 [DdlBuilder.excludeClause] 直接读 [columnItems],
  /// 排序规则 / 运算符类别等选项不会因为这层的简写而丢失。
  void applyColumnItems() {
    columns = columnItems
        .map((e) {
          final name = e.name.trim();
          final op = e.operatorName.trim();
          if (name.isEmpty) return op;
          return op.isEmpty ? name : '$name WITH $op';
        })
        .where((e) => e.trim().isNotEmpty)
        .join(', ');
  }

  DesignExclude copy() => DesignExclude(
        name: name,
        columns: columns,
        method: method,
        predicate: predicate,
        deferrable: deferrable,
        deferred: deferred,
        comment: comment,
      )..columnItems.addAll(columnItems.map((e) => e.copy()));
}

/// 规则设计(仅 PostgreSQL 家族生成 CREATE RULE)。
class DesignRule {
  DesignRule({
    this.name = '',
    this.event = 'INSERT',
    this.qualification = '',
    this.action = 'ALSO',
    this.statement = '',
    this.enable = true,
    this.comment = '',
  });

  String name;

  /// 事件:INSERT / UPDATE / DELETE / SELECT
  String event;

  /// 限定条件(不带 WHERE 关键字),即 `CREATE RULE ... WHERE ( ... )`
  String qualification;

  /// DO 后的动作变体:ALSO / INSTEAD / NOTHING
  String action;

  /// 规则主体(`DO ALSO|INSTEAD` 后的语句;[action] 为 NOTHING 时不生成)
  String statement;

  /// 启用(false 时补一条 ALTER TABLE … DISABLE RULE,与触发器的「启用」同口径)
  bool enable;
  String comment;

  DesignRule copy() => DesignRule(
        name: name,
        event: event,
        qualification: qualification,
        action: action,
        statement: statement,
        enable: enable,
        comment: comment,
      );
}

/// 触发器设计。触发函数 / 参数 / 当 为 PostgreSQL 形态;
/// 生成 DDL 仅 PostgreSQL 家族,其余类型在 SQL 预览中以注释说明。
class DesignTrigger {
  DesignTrigger({
    this.name = '',
    this.forEach = '行',
    this.timing = 'BEFORE',
    this.insert = false,
    this.update = false,
    this.delete = false,
    this.truncate = false,
    this.updateColumns = '',
    this.enable = true,
    this.comment = '',
    this.when = '',
    this.function = '',
    this.parameters = '',
    this.body = '',
    this.deferrable = '',
    this.deferred = '',
  });

  String name;

  /// 行 / 语句(FOR EACH ROW / FOR EACH STATEMENT)
  String forEach;

  /// BEFORE / AFTER / INSTEAD OF
  String timing;

  bool insert;
  bool update;
  bool delete;
  bool truncate;

  /// 更新字段(UPDATE OF c1, c2)
  String updateColumns;
  bool enable;
  String comment;

  /// WHEN 条件(不带 WHEN 关键字)
  String when;

  /// 触发函数(PostgreSQL 形态)
  String function;

  /// 函数参数(如 NEW.id, 空则不输出括号)
  String parameters;

  /// 触发主体(MySQL / SQL Server 形态:过程体写在触发器里,不引用函数)。
  /// PostgreSQL 用 [function] + [parameters],本字段留空即完全不参与生成,
  /// 因此既有设计数据(含驱动反查结果)的输出与从前逐字一致。
  String body;

  /// 可延迟(界面「约束」子页):YES 生成 CREATE CONSTRAINT TRIGGER … DEFERRABLE
  String deferrable;

  /// 延迟:仅在 [deferrable] 为 YES 时生成 INITIALLY DEFERRED(否则 PG 语法非法)
  String deferred;

  DesignTrigger copy() => DesignTrigger(
        name: name,
        forEach: forEach,
        timing: timing,
        insert: insert,
        update: update,
        delete: delete,
        truncate: truncate,
        updateColumns: updateColumns,
        enable: enable,
        comment: comment,
        when: when,
        function: function,
        parameters: parameters,
        body: body,
        deferrable: deferrable,
        deferred: deferred,
      );
}

/// 设计器下拉候选:由驱动从数据库系统目录读出的可选项(仍可手输)。
///
/// 只承载候选文本,不参与 DDL 生成;驱动没有对应系统目录时返回
/// [DesignCandidates.empty],界面退化为纯输入框。
class DesignCandidates {
  const DesignCandidates({
    this.collations = const [],
    this.opClasses = const [],
    this.tablespaces = const [],
  });

  /// 排序规则名(`pg_collation` / 各库等价目录)
  final List<String> collations;

  /// 运算符类别名(`pg_opclass`)
  final List<String> opClasses;

  /// 表空间名(`pg_tablespace`)
  final List<String> tablespaces;

  static const DesignCandidates empty = DesignCandidates();
}

/// 新建表设计器的完整设计数据。
/// 新建表的占位表名。
///
/// 设计器页面与 SQL 预览都拿它当「还没起名」的展示值(参考工具同样给个
/// Untitled 占位),但**它不算已命名**:点保存时仍要弹窗要真名,否则会把
/// 字面上的 Untitled 建成表。判「缺名」一律用 [isUntitledTableName]。
const String kUntitledTableName = 'Untitled';

/// [name] 是否等于占位表名(去空白、大小写不敏感)。
bool isUntitledTableName(String name) =>
    name.trim().isEmpty || name.trim().toLowerCase() == 'untitled';

class DesignTable {
  DesignTable();

  /// 表名(默认 [kUntitledTableName],与参考工具一致,保存前可改)
  String name = kUntitledTableName;

  /// 目标模式(由打开设计器时的树上下文决定,PostgreSQL 等有模式层的类型使用)
  String? schema;

  final List<DesignColumn> columns = [];
  final List<DesignIndex> indexes = [];
  final List<DesignForeignKey> foreignKeys = [];
  final List<DesignUniqueKey> uniqueKeys = [];
  final List<DesignCheck> checks = [];
  final List<DesignExclude> excludes = [];
  final List<DesignRule> rules = [];
  final List<DesignTrigger> triggers = [];

  /// 表注释(注释标签页)
  String tableComment = '';

  /// 表空间(PostgreSQL 生成 `TABLESPACE`;SQL Server 的文件组是另一套语法)
  String tablespace = '';

  /// 表级存储参数(界面「选项」标签):键为 PostgreSQL reloption 名
  /// (`fillfactor` / `autovacuum_*` / `parallel_workers` …),值为录入文本。
  /// 空值项不生成任何子句;[fillFactor] 是本表里唯一有具名读写的项。
  final Map<String, String> storageOptions = {};

  /// 填充因子(%):`WITH (fillfactor = n)`,SQL Server 也认这一项
  /// (输出 `WITH (FILLFACTOR = n)`)。存进 [storageOptions],保留具名读写口。
  String get fillFactor => storageOptions[kPgFillFactor] ?? '';
  set fillFactor(String v) => setStorageOption(kPgFillFactor, v);

  /// 写一项存储参数;值留空即从清单里移除(生成时不再输出该子句)。
  void setStorageOption(String key, String value) {
    final v = value.trim();
    if (v.isEmpty) {
      storageOptions.remove(key);
    } else {
      storageOptions[key] = v;
    }
  }

  /// 生成用的有序参数对:`fillfactor` 最先,其后按 [kPgStorageOptionOrder]
  /// 的清单顺序,再末尾挂清单外的项(库里读到但界面没有的 reloption)。
  List<MapEntry<String, String>> storageOptionPairs() {
    final ordered = <MapEntry<String, String>>[];
    void take(String key) {
      final v = storageOptions[key];
      if (v != null && v.trim().isNotEmpty) ordered.add(MapEntry(key, v.trim()));
    }

    take(kPgFillFactor);
    for (final key in kPgStorageOptionOrder) {
      take(key);
    }
    final known = {kPgFillFactor, ...kPgStorageOptionOrder};
    final extra = [
      for (final e in storageOptions.entries)
        if (!known.contains(e.key) && e.value.trim().isNotEmpty) e,
    ]..sort((a, b) => a.key.compareTo(b.key));
    return [...ordered, ...extra];
  }

  /// 主键约束名。新建模式留空(由 DDL 按各方言约定命名);「设计表」模式
  /// 由驱动反查填入真名——ALTER 删主键必须按真名 DROP CONSTRAINT。
  String pkName = '';

  /// 表选项(仅新建表生成,均为 PostgreSQL 家族专属):不记录(UNLOGGED) /
  /// 所有者(补一条 ALTER … OWNER TO) / 继承自(INHERITS) / 集群(CLUSTER … USING)。
  bool unlogged = false;
  String owner = '';

  /// 父表列表(逗号分隔,可带 schema 前缀)
  String inherits = '';

  /// 集群索引名
  String cluster = '';

  /// 深拷贝为一份独立快照(用作「设计表」的原始基线)。
  /// 列表与元素均重新构造,编辑中的 [columns] 变化不会污染基线。
  DesignTable snapshot() {
    final s = DesignTable()
      ..name = name
      ..schema = schema
      ..tableComment = tableComment
      ..tablespace = tablespace
      ..unlogged = unlogged
      ..owner = owner
      ..inherits = inherits
      ..cluster = cluster
      ..pkName = pkName;
    s.storageOptions.addAll(storageOptions);
    s.columns.addAll(columns.map((e) => e.copy()));
    s.indexes.addAll(indexes.map((e) => e.copy()));
    s.foreignKeys.addAll(foreignKeys.map((e) => e.copy()));
    s.uniqueKeys.addAll(uniqueKeys.map((e) => e.copy()));
    s.checks.addAll(checks.map((e) => e.copy()));
    s.excludes.addAll(excludes.map((e) => e.copy()));
    s.rules.addAll(rules.map((e) => e.copy()));
    s.triggers.addAll(triggers.map((e) => e.copy()));
    return s;
  }
}

/// 校验结果:ok 为是否通过,error 携带原因。
class DesignValidate {
  const DesignValidate(this.ok, [this.error]);
  final bool ok;
  final String? error;
}

/// 按数据库类型方言生成建表 DDL 的工具。
/// 标识符引用 / 默认值规则与 AppState 既有建表逻辑保持一致。
class DdlBuilder {
  DdlBuilder._();

  static bool isPgLike(String typeId) => kPgLikeTypes.contains(typeId);
  static bool isMysqlLike(String typeId) => kMysqlLikeTypes.contains(typeId);
  static bool isSqlServerLike(String typeId) => kSqlServerLikeTypes.contains(typeId);

  /// 标识符引用:PostgreSQL / SQLite 用双引号,SQL Server / Access 用方括号,
  /// MySQL / MariaDB 用反引号。
  static String ident(String typeId, String name) {
    switch (typeId) {
      case 'postgresql':
      case 'aliyun-rds-postgres':
      case 'aliyun-polardb-postgres':
      case 'aliyun-oceanbase-postgres':
      case 'sqlite':
        return '"${name.replaceAll('"', '""')}"';
      case 'sqlserver':
      case 'aliyun-rds-sqlserver':
      case 'access':
        return '[${name.replaceAll(']', ']]')}]';
      default: // mysql / mariadb
        return '`${name.replaceAll('`', '``')}`';
    }
  }

  /// 带模式限定的标识符;模式为空或无模式层的类型退化为普通标识符。
  static String qualified(String typeId, String? schema, String name) {
    final id = ident(typeId, name);
    if (schema == null || schema.isEmpty) return id;
    if (isPgLike(typeId) || typeId == 'sqlite' || isSqlServerLike(typeId) || typeId == 'access') {
      return '${ident(typeId, schema)}.$id';
    }
    return id;
  }

  /// 单引号字符串字面量
  static String lit(String s) => "'${s.replaceAll("'", "''")}'";

  /// DEFAULT 值智能渲染:数字 / NULL / 布尔 / 关键字 / 函数调用(含括号)/
  /// 已自带引号的值原样输出;其余按字符串字面量加引号(自动转义内部单引号)。
  static String _defaultLiteral(String raw) {
    final v = raw.trim();
    if (v.isEmpty) return '';
    if (_looksRawDefault(v)) return v;
    return lit(v);
  }

  static bool _looksRawDefault(String v) {
    if (v.startsWith("'") && v.endsWith("'")) return true; // 用户已自行加引号
    if (v.startsWith('"') && v.endsWith('"')) return true;
    if (RegExp(
      r'^(null|true|false|current_timestamp|current_date|current_time|current_user|session_user|localtime|localtimestamp)\b',
      caseSensitive: false,
    ).hasMatch(v)) {
      return true;
    }
    if (RegExp(r'^[+-]?(\d+\.?\d*|\.\d+)$').hasMatch(v)) return true; // 数字
    if (v.contains('(') || v.startsWith('(')) return true; // 函数调用 / 表达式
    // 位串 / 十六进制 / 字符集 introducer 形式的字面量:`b'1'` / `x'FF'` / `N'abc'`
    if (RegExp(r"^(?:[bBxXnN]|_[a-zA-Z0-9]+)?'").hasMatch(v)) return true;
    return false;
  }

  /// SQL Server 注释语句:该方言没有 `COMMENT ON`,注释是 `MS_Description`
  /// 扩展属性。[proc] 取 `sp_addextendedproperty` / `sp_updateextendedproperty`
  /// / `sp_dropextendedproperty`(属性原值决定该用哪个),[value] 仅在 drop 时
  /// 被忽略。
  ///
  /// 「设计表」的 ALTER 路径从前自带一份等价拼装,但 CREATE 路径漏了它、直接
  /// 复用 PG 的 `COMMENT ON …`,结果是**在 SQL Server 上新建一张带注释的表必然
  /// 语法报错**。两条路现在共用本方法。
  static String sqlServerCommentSql(
      {String? schema,
      required String table,
      String? column,
      required String value,
      String proc = 'sp_addextendedproperty'}) {
    final levels = column == null
        ? "'SCHEMA', ${lit(schema ?? 'dbo')}, 'TABLE', ${lit(table)}"
        : "'SCHEMA', ${lit(schema ?? 'dbo')}, "
            "'TABLE', ${lit(table)}, "
            "'COLUMN', ${lit(column)}";
    final args = proc == 'sp_dropextendedproperty'
        ? lit('MS_Description')
        : '${lit('MS_Description')}, ${lit(value)}';
    return 'EXEC $proc $args, $levels;';
  }

  /// 校验设计数据;不合规返回原因,可通过时 ok=true。
  ///
  /// [typeId] 为连接类型 id:触发器的必填项按方言不同(PostgreSQL 必填触发
  /// 函数,MySQL / SQL Server 必填过程体),没有方言就无法判断哪一侧算缺失,
  /// 故为必传。
  ///
  /// [requireName] 为 false 时**不**把「表名为空」算作错误:表设计器的工具栏是
  /// 纯图标(参考工具如此),表名不在页面上填,而是点「保存」时弹窗要 ——
  /// 若在这里拦死,新建表就永远按不下保存键。其余校验(列 / 类型 / 重名)照做。
  static DesignValidate validate(DesignTable d,
      {required String typeId, bool requireName = true}) {
    final tableName = d.name.trim();
    if (tableName.isEmpty && requireName) {
      return const DesignValidate(false, '表名不能为空');
    }
    if (d.columns.isEmpty) return const DesignValidate(false, '至少需要一列');
    for (var i = 0; i < d.columns.length; i++) {
      final c = d.columns[i];
      if (c.name.trim().isEmpty) return const DesignValidate(false, '存在未命名的列,请补全列名');
      if (c.fullType.isEmpty) {
        return DesignValidate(false, '第 ${i + 1} 列的「类型」不能为空');
      }
      if (c.hasIdentity) {
        const labels = ['递增', '最小', '最大', '开始值', '缓存'];
        final values = [
          c.identityIncrement,
          c.identityMinValue,
          c.identityMaxValue,
          c.identityStart,
          c.identityCache,
        ];
        for (var k = 0; k < labels.length; k++) {
          final v = values[k].trim();
          if (v.isEmpty) continue; // 留空 = 交由数据库取默认
          if (!RegExp(r'^[+-]?\d+$').hasMatch(v)) {
            return DesignValidate(false, '第 ${i + 1} 列的「${labels[k]}」必须是整数');
          }
        }
      }
    }
    final seenCols = <String>{};
    for (final c in d.columns) {
      if (!seenCols.add(c.name.trim().toLowerCase())) {
        return DesignValidate(false, '存在重复的列名: ${c.name.trim()}');
      }
    }
    for (final fk in d.foreignKeys) {
      if (fk.columnList.isEmpty) {
        return DesignValidate(false, '外键「${_nameOr(fk.name, '未命名')}」未指定字段');
      }
      if (fk.refTable.trim().isEmpty) {
        return DesignValidate(false, '外键「${_nameOr(fk.name, '未命名')}」未指定被引用的表');
      }
      if (fk.refColumnList.isEmpty) {
        return DesignValidate(false, '外键「${_nameOr(fk.name, '未命名')}」未指定被引用的字段');
      }
    }
    for (final idx in d.indexes) {
      if (idx.columnList.isEmpty) {
        return DesignValidate(false, '索引「${_nameOr(idx.name, '未命名')}」未指定字段');
      }
    }
    for (final uk in d.uniqueKeys) {
      if (uk.columnList.isEmpty) {
        return DesignValidate(false, '唯一键「${_nameOr(uk.name, '未命名')}」未指定字段');
      }
    }
    for (final ck in d.checks) {
      if (ck.expression.trim().isEmpty) {
        return DesignValidate(false, '检查「${_nameOr(ck.name, '未命名')}」未指定条件');
      }
    }
    for (final ex in d.excludes) {
      // 逗号串与弹窗元素项二者有一即算填了(excludeClause 优先按元素项生成)
      if (ex.columns.trim().isEmpty && ex.columnItems.isEmpty) {
        return DesignValidate(false, '排除「${_nameOr(ex.name, '未命名')}」未指定字段 / 表达式');
      }
      // 弹窗里勾了字段却没填运算符 → EXCLUDE 括号里会是 `col`(缺 WITH),
      // PG 直接报语法错。执行前拦住,不让用户拿一条非法语句去撞数据库。
      for (final e in ex.columnItems) {
        if (e.name.trim().isNotEmpty && e.operatorName.trim().isEmpty) {
          return DesignValidate(false,
              '排除「${_nameOr(ex.name, '未命名')}」的元素「${e.name.trim()}」未指定运算符');
        }
      }
    }
    for (final r in d.rules) {
      final action = r.action.trim().isEmpty ? 'ALSO' : r.action.trim().toUpperCase();
      if (action == 'NOTHING') continue; // DO NOTHING 本就没有语句体
      if (r.statement.trim().isEmpty) {
        return DesignValidate(false,
            '规则「${_nameOr(r.name, _ruleName(r, d))}」未指定语句(DO $action 后的动作)');
      }
    }
    for (final tr in d.triggers) {
      if (tr.name.trim().isEmpty) return const DesignValidate(false, '存在未命名的触发器');
      // 触发器的「必填的另一半」按方言不同:PG 走独立函数,MySQL / SQL Server
      // 把过程体写进触发器本身。留空即生成不出可执行的 CREATE。
      if (isPgLike(typeId)) {
        if (tr.function.trim().isEmpty) {
          return DesignValidate(false, '触发器「${tr.name}」未指定触发函数');
        }
      } else if (isMysqlLike(typeId) || isSqlServerLike(typeId)) {
        if (tr.truncate) {
          return DesignValidate(false, '触发器「${tr.name}」:TRUNCATE 事件仅 PostgreSQL 支持');
        }
        if (tr.body.trim().isEmpty) {
          return DesignValidate(false, '触发器「${tr.name}」未指定触发主体');
        }
        if (triggerEvents(tr, includeTruncate: false).isEmpty) {
          return DesignValidate(false, '触发器「${tr.name}」未勾选触发事件');
        }
      } else {
        // SQLite / Access:语法上 SQLite 有触发器,但这两个驱动都没有结构反查,
        // 存进去的设计器里再也读不回来 —— 与其静默丢弃,不如在保存前说清楚。
        return DesignValidate(false, '当前数据库类型不支持在设计器中定义触发器');
      }
    }
    return const DesignValidate(true);
  }

  static String _nameOr(String name, String fallback) => name.trim().isEmpty ? fallback : name.trim();

  /// 生成完整建表脚本(按语句拆分,便于逐条执行 / 预览)。
  static List<String> buildStatements(DesignTable d, String typeId) {
    final stmts = <String>[];
    stmts.add(buildCreateTable(d, typeId));

    for (final idx in d.indexes) {
      stmts.add(buildCreateIndex(d, typeId, idx));
    }
    for (final rule in d.rules) {
      if (!isPgLike(typeId)) continue;
      stmts.add(buildCreateRule(d, typeId, rule));
      if (!rule.enable) {
        stmts.add(buildSetRuleEnable(d, typeId, _ruleName(rule, d), false));
      }
      if (rule.comment.trim().isNotEmpty) {
        stmts.add(buildRuleComment(d, typeId, rule));
      }
    }
    for (final tr in d.triggers) {
      // 各方言的触发器语法互不相容(事件个数 / 行级语句级 / 主体形态都不同),
      // 统一走 buildCreateTriggers 内部分流;SQLite 无触发器目录可反查 → 返回空。
      stmts.addAll(buildCreateTriggers(d, typeId, tr));
      if (isPgLike(typeId) && !tr.enable) {
        stmts.add('ALTER TABLE ${_qualifiedTable(d, typeId)} DISABLE TRIGGER ${ident(typeId, tr.name.trim())};');
      }
    }
    // 表所有者 / 集群:PG 无内联子句,按 Navicat 做法补独立语句(集群需索引已建好)
    if (isPgLike(typeId) && d.owner.trim().isNotEmpty) {
      stmts.add('ALTER TABLE ${_qualifiedTable(d, typeId)} OWNER TO ${ident(typeId, d.owner.trim())};');
    }
    if (isPgLike(typeId) && d.cluster.trim().isNotEmpty) {
      stmts.add('CLUSTER ${_qualifiedTable(d, typeId)} USING ${ident(typeId, d.cluster.trim())};');
    }
    // 注释:MySQL / MariaDB 已内联进 CREATE TABLE;SQL Server 走 MS_Description
    // 扩展属性(它没有 COMMENT ON 语法);其余类型走 COMMENT ON … IS '…'
    if (isSqlServerLike(typeId)) {
      final tbl = d.name.trim();
      if (d.tableComment.trim().isNotEmpty) {
        stmts.add(sqlServerCommentSql(
            schema: d.schema, table: tbl, value: d.tableComment.trim()));
      }
      for (final c in d.columns) {
        if (c.comment.trim().isEmpty) continue;
        stmts.add(sqlServerCommentSql(
            schema: d.schema,
            table: tbl,
            column: c.name.trim(),
            value: c.comment.trim()));
      }
    } else if (!isMysqlLike(typeId)) {
      if (d.tableComment.trim().isNotEmpty) {
        stmts.add('COMMENT ON TABLE ${_qualifiedTable(d, typeId)} IS ${lit(d.tableComment.trim())};');
      }
      for (final c in d.columns) {
        if (c.comment.trim().isNotEmpty) {
          stmts.add('COMMENT ON COLUMN ${_qualifiedTable(d, typeId)}.${ident(typeId, c.name.trim())} IS ${lit(c.comment.trim())};');
        }
      }
      if (isPgLike(typeId)) {
        for (final fk in d.foreignKeys) {
          if (fk.comment.trim().isEmpty) continue;
          stmts.add('COMMENT ON CONSTRAINT ${ident(typeId, _fkName(fk, d))} '
              'ON ${_qualifiedTable(d, typeId)} IS ${lit(fk.comment.trim())};');
        }
        // 索引 / 唯一键注释:PG 无内联子句,靠 COMMENT ON 补独立语句
        for (final idx in d.indexes) {
          if (idx.comment.trim().isEmpty) continue;
          stmts.add('COMMENT ON INDEX '
              '${qualified(typeId, d.schema, _indexName(idx, d))} '
              'IS ${lit(idx.comment.trim())};');
        }
        for (final uk in d.uniqueKeys) {
          if (uk.comment.trim().isEmpty) continue;
          stmts.add('COMMENT ON CONSTRAINT '
              '${ident(typeId, _uniqueName(uk, d))} '
              'ON ${_qualifiedTable(d, typeId)} IS ${lit(uk.comment.trim())};');
        }
        // 检查 / 排除约束同为 pg_constraint 对象,注释走 COMMENT ON CONSTRAINT
        for (final ck in d.checks) {
          if (ck.comment.trim().isEmpty) continue;
          stmts.add('COMMENT ON CONSTRAINT '
              '${ident(typeId, _checkName(ck, d))} '
              'ON ${_qualifiedTable(d, typeId)} IS ${lit(ck.comment.trim())};');
        }
        for (final ex in d.excludes) {
          if (ex.comment.trim().isEmpty) continue;
          stmts.add('COMMENT ON CONSTRAINT '
              '${ident(typeId, _excludeName(ex, d))} '
              'ON ${_qualifiedTable(d, typeId)} IS ${lit(ex.comment.trim())};');
        }
      }
    }
    return stmts;
  }

  /// 生成「SQL 预览」文本(语句以换行拼接)
  static String buildPreview(DesignTable d, String typeId) =>
      buildStatements(d, typeId).join('\n');

  static String _qualifiedTable(DesignTable d, String typeId) =>
      qualified(typeId, d.schema, d.name.trim());

  // ── CREATE TABLE ─────────────────────────────────────────────

  /// 单列定义片段(不含缩进):`"id" int8 GENERATED ... AS IDENTITY (...) NOT NULL DEFAULT ...`。
  ///
  /// CREATE TABLE 内联列、ALTER TABLE `ADD COLUMN` 与 `MODIFY COLUMN` 共用本方法,
  /// 避免同一方言的列语法在两条路径上各自维护而漂移。
  /// [inlinePrimaryKey] 为 true 时内联 `PRIMARY KEY`(单列主键情形);
  /// [inlineComment] 为 false 时不输出 MySQL 的列内联注释。
  static String columnDefSql(DesignColumn c, String typeId,
      {bool inlinePrimaryKey = false, bool inlineComment = true}) {
    var typeText = c.fullType;
    // PostgreSQL 数组维度:base[len][dim]
    final dim = c.dimension.trim();
    if (dim.isNotEmpty && isPgLike(typeId)) typeText += '[$dim]';
    final buf = StringBuffer('${ident(typeId, c.name.trim())} $typeText');
    if (c.collation.trim().isNotEmpty &&
        (isPgLike(typeId) || isMysqlLike(typeId))) {
      buf.write(' COLLATE ${ident(typeId, c.collation.trim())}');
    }
    // IDENTITY 子句:PG 语法要求位于列约束(NOT NULL / DEFAULT)之前
    final identity = _identityClause(c, typeId);
    if (identity.isNotEmpty) buf.write(' $identity');
    if (c.notNull) buf.write(' NOT NULL');
    if (c.defaultValue.trim().isNotEmpty) {
      buf.write(' DEFAULT ${_defaultLiteral(c.defaultValue.trim())}');
    }
    // MySQL / MariaDB 自增(需配合键使用,交由用户保证);
    // 「虚拟类型」在该方言下降级为 AUTO_INCREMENT,与手工勾选等价
    if (isMysqlLike(typeId) && (c.autoIncrement || c.hasIdentity)) {
      buf.write(' AUTO_INCREMENT');
    }
    if (inlinePrimaryKey) buf.write(' PRIMARY KEY');
    if (inlineComment && isMysqlLike(typeId) && c.comment.trim().isNotEmpty) {
      buf.write(' COMMENT ${lit(c.comment.trim())}');
    }
    return buf.toString();
  }

  /// 唯一键约束片段(不含缩进);无字段时返回空串。
  /// [d] 仅用于自动命名(`uq_<表名>_<首列>`)。
  static String uniqueKeyClause(DesignUniqueKey uk, DesignTable d, String typeId) {
    final cols = uk.columnList;
    if (cols.isEmpty) return '';
    final name = uk.name.trim().isEmpty
        ? 'uq_${d.name.trim()}_${cols.first}'
        : uk.name.trim();
    final buf = StringBuffer(
        'CONSTRAINT ${ident(typeId, name)} UNIQUE (${_uniqueColumnsSql(uk, cols, typeId)})');
    if (isPgLike(typeId)) {
      // PG 的 UNIQUE 约束语法顺序:字段列表 → WITH(存储参数)→ TABLESPACE → 约束属性。
      // 从前这两项只是界面上能填、这里从不读,等于给了两个死控件。
      final ff = uk.fillFactor.trim();
      if (ff.isNotEmpty) buf.write(' WITH (fillfactor = $ff)');
      final ts = uk.tablespace.trim();
      if (ts.isNotEmpty) buf.write(' TABLESPACE ${ident(typeId, ts)}');
      buf.write(_deferrableClause(uk.deferrable, uk.deferred));
    }
    return buf.toString();
  }

  /// 检查约束片段(不含缩进);无条件时返回空串。
  /// [autoSuffix] 用于无名检查的序号兜底命名(与 CREATE 路径一致)。
  static String checkClause(DesignCheck ck, DesignTable d, String typeId,
      {int autoSuffix = 1}) {
    if (ck.expression.trim().isEmpty) return '';
    final name = ck.name.trim().isEmpty
        ? 'ck_${d.name.trim()}_$autoSuffix'
        : ck.name.trim();
    final buf = StringBuffer(
        'CONSTRAINT ${ident(typeId, name)} CHECK (${ck.expression.trim()})');
    // NO INHERIT 仅 PG 有(界面「无继承」格);子表继承检查时不加即视为继承。
    if (isPgLike(typeId) && ck.noInherit) buf.write(' NO INHERIT');
    return buf.toString();
  }

  /// 排除约束片段(不含缩进);无排除元素时返回空串。
  ///
  /// 语法顺序按 PG:`CONSTRAINT name EXCLUDE USING am (元素 WITH 操作符, ...)
  /// [WHERE (谓词)] [DEFERRABLE ...]`。「元素」弹窗写入了 [DesignExclude.columnItems]
  /// 时逐项生成;否则把 [DesignExclude.columns] 逗号串原样带出(手输表达式、
  /// 驱动反查结果都走这条路径,里面的 `WITH 操作符` 不重新包装)。
  static String excludeClause(DesignExclude ex, DesignTable d, String typeId) {
    final cols = ex.columnItems.isEmpty
        ? ex.columns.trim()
        : ex.columnItems
            .map((e) => excludeElementSql(e, typeId))
            .where((e) => e.trim().isNotEmpty)
            .join(', ');
    if (cols.isEmpty) return '';
    final method = ex.method.trim().isEmpty ? 'gist' : ex.method.trim();
    final buf = StringBuffer(
        'CONSTRAINT ${ident(typeId, _excludeName(ex, d))} EXCLUDE USING $method ($cols)');
    final pred = ex.predicate.trim();
    if (pred.isNotEmpty) buf.write(' WHERE ($pred)');
    if (isPgLike(typeId)) buf.write(_deferrableClause(ex.deferrable, ex.deferred));
    return buf.toString();
  }

  /// 外键约束片段(不含缩进);信息不全时返回空串。
  static String foreignKeyClause(
      DesignForeignKey fk, DesignTable d, String typeId) {
    final cols = fk.columnList;
    final refCols = fk.refColumnList;
    if (cols.isEmpty || fk.refTable.trim().isEmpty || refCols.isEmpty) return '';
    final name = fk.name.trim().isEmpty
        ? 'fk_${d.name.trim()}_${cols.first}'
        : fk.name.trim();
    final refTable = fk.refSchema.trim().isNotEmpty
        ? qualified(typeId, fk.refSchema.trim(), fk.refTable.trim())
        : qualified(typeId, d.schema, fk.refTable.trim());
    final buf = StringBuffer(
        'CONSTRAINT ${ident(typeId, name)} FOREIGN KEY (${cols.map((n) => ident(typeId, n)).join(', ')}) REFERENCES $refTable (${refCols.map((n) => ident(typeId, n)).join(', ')})');
    // MATCH FULL:PostgreSQL 与 MySQL 家族都有该子句(SQL Server 没有,由
    // [createUnsupported] 拦住,不让它静默消失)
    if ((isPgLike(typeId) || isMysqlLike(typeId)) && fk.matchAll) {
      buf.write(' MATCH FULL');
    }
    final od = fk.onDelete.trim().toUpperCase();
    if (od.isNotEmpty && od != 'NO ACTION') buf.write(' ON DELETE $od');
    final ou = fk.onUpdate.trim().toUpperCase();
    if (ou.isNotEmpty && ou != 'NO ACTION') buf.write(' ON UPDATE $ou');
    if (isPgLike(typeId)) buf.write(_deferrableClause(fk.deferrable, fk.deferred));
    return buf.toString();
  }

  /// 主键列名清单(按 [pkOrdinals] 的列序)。
  static List<String> pkColumnNames(DesignTable d) => [
        for (final c in d.columns)
          if (c.primaryKey) c.name.trim(),
      ];

  static String buildCreateTable(DesignTable d, String typeId) {
    final lines = <String>[];
    final pkCols = pkColumnNames(d);

    for (final c in d.columns) {
      lines.add('  ${columnDefSql(c, typeId,
          inlinePrimaryKey: c.primaryKey && pkCols.length == 1)}');
    }

    // 表级约束:多列主键 / 唯一键 / 检查 / 排除(PG) / 外键
    if (pkCols.length > 1) {
      lines.add('  PRIMARY KEY (${pkCols.map((n) => ident(typeId, n)).join(', ')})');
    }
    var checkNo = 0;
    for (final uk in d.uniqueKeys) {
      final clause = uniqueKeyClause(uk, d, typeId);
      if (clause.isEmpty) continue;
      lines.add('  $clause');
    }
    for (final ck in d.checks) {
      final clause = checkClause(ck, d, typeId, autoSuffix: ++checkNo);
      if (clause.isEmpty) continue;
      lines.add('  $clause');
    }
    for (final ex in d.excludes) {
      if (!isPgLike(typeId)) continue;
      final clause = excludeClause(ex, d, typeId);
      if (clause.isEmpty) continue;
      lines.add('  $clause');
    }
    for (final fk in d.foreignKeys) {
      final clause = foreignKeyClause(fk, d, typeId);
      if (clause.isEmpty) continue;
      lines.add('  $clause');
    }

    // 表选项(存储参数 + 表空间;MySQL 表注释内联)
    final suffix = <String>[];
    if (isPgLike(typeId)) {
      final opts = d.storageOptionPairs();
      if (opts.isNotEmpty) {
        suffix.add('WITH (${opts.map((e) => '${e.key} = ${e.value}').join(', ')})');
      }
    } else if (isSqlServerLike(typeId)) {
      // SQL Server 只有填充因子这一项,且关键字大写、无其它 reloption
      final ff = d.fillFactor.trim();
      if (ff.isNotEmpty) suffix.add('WITH (FILLFACTOR = $ff)');
    }
    final ts = d.tablespace.trim();
    if (ts.isNotEmpty && isPgLike(typeId)) suffix.add('TABLESPACE ${ident(typeId, ts)}');
    if (isMysqlLike(typeId) && d.tableComment.trim().isNotEmpty) {
      suffix.add('COMMENT = ${lit(d.tableComment.trim())}');
    }
    final tail = suffix.isEmpty ? '' : ' ${suffix.join(' ')}';
    // UNLOGGED 位于关键字前,INHERITS 位于字段列表后、WITH 前(PG 语法顺序)
    final unlogged = d.unlogged && isPgLike(typeId) ? 'UNLOGGED ' : '';
    final inherits =
        isPgLike(typeId) && d.inherits.trim().isNotEmpty ? ' INHERITS (${_inheritRefs(d, typeId)})' : '';

    return 'CREATE ${unlogged}TABLE ${_qualifiedTable(d, typeId)} '
        '(\n${lines.join(',\n')}\n)$inherits$tail;';
  }

  /// INHERITS 父表引用:逗号分隔,单项可带 `schema.table` 前缀
  static String _inheritRefs(DesignTable d, String typeId) {
    return d.inherits.split(',').map((e) => e.trim()).where((e) => e.isNotEmpty).map((e) {
      final parts = e.split('.');
      if (parts.length < 2) return ident(typeId, e);
      final table = parts.removeLast().trim();
      return '${parts.map((s) => ident(typeId, s.trim())).join('.')}.${ident(typeId, table)}';
    }).join(', ');
  }

  /// 主键序号(列 → 在 PRIMARY KEY 中的顺序,1 起)。
  /// 界面「键」列的 🔑n 与表级 `PRIMARY KEY (...)` 列序共用本结果,保证一致。
  static Map<DesignColumn, int> pkOrdinals(DesignTable d) {
    final map = <DesignColumn, int>{};
    var n = 0;
    for (final c in d.columns) {
      if (c.primaryKey) map[c] = ++n;
    }
    return map;
  }

  /// IDENTITY 子句(按方言):PostgreSQL 完整生成 GENERATED … AS IDENTITY
  /// 及其序列选项(仅输出非空项);SQL Server 生成 IDENTITY(seed,increment);
  /// MySQL / MariaDB 由 AUTO_INCREMENT 分支承担,SQLite / Access 不生成。
  static String _identityClause(DesignColumn c, String typeId) {
    final mode = c.identityMode.trim().toUpperCase();
    if (mode.isEmpty) return '';
    if (isPgLike(typeId)) {
      final kw = mode == 'ALWAYS' ? 'ALWAYS' : 'BY DEFAULT';
      final opts = <String>[
        if (c.identityIncrement.trim().isNotEmpty) 'INCREMENT BY ${c.identityIncrement.trim()}',
        if (c.identityMinValue.trim().isNotEmpty) 'MINVALUE ${c.identityMinValue.trim()}',
        if (c.identityMaxValue.trim().isNotEmpty) 'MAXVALUE ${c.identityMaxValue.trim()}',
        if (c.identityStart.trim().isNotEmpty) 'START WITH ${c.identityStart.trim()}',
        if (c.identityCache.trim().isNotEmpty) 'CACHE ${c.identityCache.trim()}',
        if (c.identityCycle) 'CYCLE',
      ];
      final base = 'GENERATED $kw AS IDENTITY';
      return opts.isEmpty ? base : '$base (${opts.join(' ')})';
    }
    if (isSqlServerLike(typeId)) {
      final seed = c.identityStart.trim().isEmpty ? '1' : c.identityStart.trim();
      final inc = c.identityIncrement.trim().isEmpty ? '1' : c.identityIncrement.trim();
      return 'IDENTITY($seed,$inc)';
    }
    return '';
  }

  /// 可延迟 / 延迟 子句(仅 PostgreSQL):DEFERRABLE / NOT DEFERRABLE + INITIALLY DEFERRED
  static String _deferrableClause(String deferrable, String deferred) {
    final def = deferrable.trim().toUpperCase();
    final dff = deferred.trim().toUpperCase();
    final buf = StringBuffer();
    if (def == 'YES') {
      buf.write(' DEFERRABLE');
    } else if (def == 'NO') {
      buf.write(' NOT DEFERRABLE');
    }
    if (dff == 'YES') {
      buf.write(' INITIALLY DEFERRED');
    } else if (dff == 'NO') {
      buf.write(' INITIALLY IMMEDIATE');
    }
    return buf.toString();
  }

  // ── CREATE INDEX ─────────────────────────────────────────────

  static String buildCreateIndex(DesignTable d, String typeId, DesignIndex idx) {
    final cols = idx.columnList;
    // 无名项统一走 `_indexName`(它对空字段做了兼容,不重复实现)
    final name = _indexName(idx, d);
    final buf = StringBuffer('CREATE ');
    final method = idx.method.trim();
    final lowerMethod = method.toLowerCase();
    // MySQL 的 FULLTEXT / SPATIAL 是**索引类型关键字**,必须紧跟 CREATE 之后;
    // 尾部 USING 只接受 BTREE / HASH,写成 `... USING fulltext` 会 1064。
    final typeKeyword =
        isMysqlLike(typeId) && (lowerMethod == 'fulltext' || lowerMethod == 'spatial')
            ? lowerMethod
            : null;
    if (typeKeyword != null) {
      // MySQL 语法里 UNIQUE 与 FULLTEXT/SPATIAL 互斥,取类型关键字
      buf.write('${typeKeyword.toUpperCase()} ');
    } else if (idx.unique) {
      buf.write('UNIQUE ');
    }
    buf.write('INDEX ');
    if (idx.concurrent && isPgLike(typeId)) buf.write('CONCURRENTLY ');
    buf.write('${ident(typeId, name)} ON ${_qualifiedTable(d, typeId)}');
    // MySQL 的 `USING method` 位于字段列表之后,PG / SQL Server 位于之前
    if (method.isNotEmpty && !isMysqlLike(typeId)) buf.write(' USING $method');
    buf.write(' (${_indexColumnsSql(idx, cols, typeId)})');
    if (typeKeyword == null && method.isNotEmpty && isMysqlLike(typeId)) {
      buf.write(' USING $method');
    }
    final ff = idx.fillFactor.trim();
    if (ff.isNotEmpty) {
      if (isPgLike(typeId)) buf.write(' WITH (fillfactor = $ff)');
      if (isSqlServerLike(typeId)) buf.write(' WITH (FILLFACTOR = $ff)');
    }
    final ts = idx.tablespace.trim();
    if (ts.isNotEmpty && isPgLike(typeId)) buf.write(' TABLESPACE ${ident(typeId, ts)}');
    // MySQL 家族的索引注释内联在 CREATE INDEX 的 index_option 里
    final idxComment = idx.comment.trim();
    if (idxComment.isNotEmpty && isMysqlLike(typeId)) {
      buf.write(' COMMENT ${lit(idxComment)}');
    }
    return '$buf;';
  }

  /// 索引字段列表 → DDL。弹窗写入了 [DesignIndex.columnItems] 时逐项输出选项,
  /// 否则按逗号串输出裸字段名(驱动反查结果与手输内容走这条路径)。
  static String _indexColumnsSql(DesignIndex idx, List<String> cols, String typeId) {
    if (idx.columnItems.isEmpty) return cols.map((n) => ident(typeId, n)).join(', ');
    return idx.columnItems.map((e) => indexColumnSql(e, typeId)).join(', ');
  }

  /// 唯一键字段列表 → DDL:「选择数据表字段」弹窗写入 [DesignUniqueKey.columnItems]
  /// 时逐项输出排序规则 / 运算符类别 / 排序顺序,否则按逗号串输出裸字段名。
  static String _uniqueColumnsSql(
      DesignUniqueKey uk, List<String> cols, String typeId) {
    if (uk.columnItems.isEmpty) {
      return cols.map((n) => ident(typeId, n)).join(', ');
    }
    return uk.columnItems.map((e) => indexColumnSql(e, typeId)).join(', ');
  }

  /// 单个索引字段项 → DDL。PG 语法顺序:
  /// `字段 [COLLATE 排序规则] [运算符类别] [ASC|DESC] [NULLS FIRST|LAST]`;
  /// 非 PG 类型忽略全部选项(不生成非法子句)。
  static String indexColumnSql(DesignIndexColumn c, String typeId) {
    final name = ident(typeId, c.name.trim());
    if (!isPgLike(typeId)) return name;
    final parts = <String>[name];
    final collation = c.collation.trim();
    if (collation.isNotEmpty) {
      parts.add('COLLATE ${qualified(typeId, c.collationSchema.trim(), collation)}');
    }
    final opClass = c.opClass.trim();
    if (opClass.isNotEmpty) {
      parts.add(qualified(typeId, c.opClassSchema.trim(), opClass));
    }
    final order = c.order.trim().toUpperCase();
    if (order == 'ASC' || order == 'DESC') parts.add(order);
    final nulls = c.nullsOrder.trim().toUpperCase();
    if (nulls == 'FIRST' || nulls == 'LAST') parts.add('NULLS $nulls');
    return parts.join(' ');
  }

  /// 单个排除约束元素 → DDL:`<索引字段项> WITH <运算符>`。
  /// 运算符名不套引号(PG 的 `&&` / `<=` 这类写法带引号反而不认),但可带
  /// schema 前缀;没有运算符的项退化为裸字段项(手输逗号串与反查结果不经
  /// 本方法,由 [excludeClause] 原样带出)。
  static String excludeElementSql(DesignIndexColumn c, String typeId) {
    final name = c.name.trim();
    if (name.isEmpty) return '';
    final item = indexColumnSql(c, typeId);
    final op = c.operatorName.trim();
    if (op.isEmpty) return item;
    final schema = c.operatorSchema.trim();
    final opSql = isPgLike(typeId) && schema.isNotEmpty
        ? '${ident(typeId, schema)}.$op'
        : op;
    return '$item WITH $opSql';
  }

  // ── CREATE RULE(仅 PostgreSQL) ───────────────────────────────

  /// 规则名:无名项按 `rule_表名_事件` 补齐(CREATE / 注释 / 差异配对共用一口径)
  static String _ruleName(DesignRule r, DesignTable d) {
    if (r.name.trim().isNotEmpty) return r.name.trim();
    final event = r.event.trim().isEmpty ? 'insert' : r.event.trim().toLowerCase();
    return 'rule_${d.name.trim()}_$event';
  }

  static String buildCreateRule(DesignTable d, String typeId, DesignRule rule) {
    final name = _ruleName(rule, d);
    final event = rule.event.trim().isEmpty ? 'INSERT' : rule.event.trim().toUpperCase();
    final qual = rule.qualification.trim();
    final where = qual.isEmpty ? '' : ' WHERE ($qual)';
    final stmt = rule.statement.trim();
    // SELECT 规则只能 DO INSTEAD(PG 语法限制);DO NOTHING 不带语句体。
    final action = event == 'SELECT'
        ? 'INSTEAD'
        : (rule.action.trim().isEmpty ? 'ALSO' : rule.action.trim().toUpperCase());
    final body = action == 'NOTHING' || stmt.isEmpty
        ? 'DO NOTHING'
        : 'DO $action (${_normalizeBody(stmt)})';
    return 'CREATE RULE ${ident(typeId, name)} AS ON $event TO ${_qualifiedTable(d, typeId)}'
        '$where $body;';
  }

  /// 规则注释语句(PG 的 RULE 是独立对象,注释走 `COMMENT ON RULE … ON 表`)。
  static String buildRuleComment(DesignTable d, String typeId, DesignRule rule) =>
      'COMMENT ON RULE ${ident(typeId, _ruleName(rule, d))} ON ${_qualifiedTable(d, typeId)} '
          'IS ${_commentValue(rule.comment)};';

  /// 删除规则语句(名字是表局部作用域,必须带 ON)。
  static String buildDropRule(DesignTable d, String typeId, String name) =>
      'DROP RULE IF EXISTS ${ident(typeId, name.trim())} ON ${_qualifiedTable(d, typeId)};';

  /// 规则「启用」开关:PG 的启用状态不属于 CREATE RULE 文本,建成后再切。
  static String buildSetRuleEnable(
          DesignTable d, String typeId, String name, bool enable) =>
      'ALTER TABLE ${_qualifiedTable(d, typeId)} '
      '${enable ? 'ENABLE' : 'DISABLE'} RULE ${ident(typeId, name.trim())};';

  // ── CREATE TRIGGER ───────────────────────────────────────────

  /// 按方言生成 `CREATE TRIGGER`(可能多条,见 MySQL 分支);该方言不支持
  /// (或 SQLite 这类无触发器目录的类型)时返回空列表,由调用方跳过。
  ///
  /// 三种语法形态互不相容,各自成函数,不共享拼装分支:
  /// - PostgreSQL:`FOR EACH ROW/STATEMENT` + `WHEN` + `EXECUTE FUNCTION`,
  ///   事件可 `INSERT OR UPDATE OF col OR DELETE OR TRUNCATE` 多个。
  /// - MySQL / MariaDB:**一个触发器只能对应一个事件**,无 `WHEN`、无语句级、
  ///   无 `TRUNCATE`,主体内联在触发器里(`NEW.col` / `OLD.col`)。
  /// - SQL Server:语句级(无 `FOR EACH ROW`)、无 `BEFORE`(用 `AFTER`)、
  ///   无 `TRUNCATE`,事件用逗号列表,主体内联(`inserted` / `deleted` 伪表)。
  static List<String> buildCreateTriggers(DesignTable d, String typeId, DesignTrigger tr) {
    if (isPgLike(typeId)) return [_pgTriggerSql(d, typeId, tr)];
    if (isMysqlLike(typeId)) return _mysqlTriggerSql(d, typeId, tr);
    if (isSqlServerLike(typeId)) return _sqlServerTriggerSql(d, typeId, tr);
    return const [];
  }

  /// 触发事件清单(勾选顺序即输出顺序);一个都没勾时按各方言的默认事件。
  static List<String> triggerEvents(DesignTrigger tr, {bool includeTruncate = true}) => [
        if (tr.insert) 'INSERT',
        if (tr.update) 'UPDATE',
        if (tr.delete) 'DELETE',
        if (includeTruncate && tr.truncate) 'TRUNCATE',
      ];

  /// 过程体归一化:去掉首尾空白与末尾多余的 `;`,由调用方按方言语法补回。
  /// 用户从客户端里整段粘进来时通常带着结尾分号,直接拼会得到 `;;`。
  static String _normalizeBody(String body) {
    var s = body.trim();
    while (s.endsWith(';')) {
      s = s.substring(0, s.length - 1).trimRight();
    }
    return s;
  }

  static String _pgTriggerSql(DesignTable d, String typeId, DesignTrigger tr) {
    final events = triggerEvents(tr);
    final eventsText = events.isEmpty ? 'INSERT' : events.join(' OR ');
    final updateOf = tr.updateColumns.trim().isNotEmpty
        ? ' OF ${tr.updateColumns.split(',').map((n) => ident(typeId, n.trim())).join(', ')}'
        : '';
    final buf = StringBuffer('CREATE ');
    // 可延迟的触发器在 PG 里只能以约束触发器形式创建
    final deferAble = tr.deferrable.trim().toUpperCase() == 'YES';
    if (deferAble) buf.write('CONSTRAINT ');
    buf.write('TRIGGER ${ident(typeId, tr.name.trim())} ');
    buf.write('${tr.timing.trim().toUpperCase()} $eventsText$updateOf ON ${_qualifiedTable(d, typeId)}');
    // DEFERRABLE 子句必须在 FOR EACH 之前(PG 语法)
    if (deferAble) {
      buf.write(' DEFERRABLE');
      if (tr.deferred.trim().toUpperCase() == 'YES') buf.write(' INITIALLY DEFERRED');
    }
    buf.write(tr.forEach.trim() == '语句' ? ' FOR EACH STATEMENT' : ' FOR EACH ROW');
    if (tr.when.trim().isNotEmpty) buf.write(' WHEN (${tr.when.trim()})');
    final fn = tr.function.trim();
    final params = tr.parameters.trim();
    buf.write(' EXECUTE FUNCTION ${_pgFunctionRef(fn, params)}');
    return '$buf;';
  }

  /// MySQL / MariaDB:`CREATE TRIGGER name {BEFORE|AFTER} {INSERT|UPDATE|DELETE}
  /// ON tbl FOR EACH ROW BEGIN … END`。
  ///
  /// MySQL 的一个触发器只能挂一个事件,而界面上「插入 / 更新 / 删除」是三列
  /// 独立勾选(与 PG 的 `OR` 列表共用同一张网格)。勾了多个时按事件**各出一条
  /// CREATE**,而不是静默只取第一个 —— 后者会让用户以为三个事件都会触发。
  /// 单事件保留原名(与手工写的 DDL 一致),多事件才加 `_insert` / `_update`
  /// 后缀,否则同名会撞 1359。
  static List<String> _mysqlTriggerSql(DesignTable d, String typeId, DesignTrigger tr) {
    final events = triggerEvents(tr, includeTruncate: false);
    if (events.isEmpty) return const [];
    final body = _normalizeBody(tr.body);
    if (body.isEmpty) return const [];
    final name = tr.name.trim();
    // MySQL 只有行级触发器;INSTEAD OF 在该方言不存在,按 BEFORE 直译
    final timing =
        tr.timing.trim().toUpperCase() == 'BEFORE' ? 'BEFORE' : 'AFTER';
    return [
      for (final ev in events)
        'CREATE TRIGGER ${ident(typeId, events.length == 1 ? name : '${name}_${ev.toLowerCase()}')} '
            '$timing $ev ON ${_qualifiedTable(d, typeId)} FOR EACH ROW\n'
            // 恒用 BEGIN … END 包裹:单语句同样合法,且免去「数有几条语句」
            // 这类易错判定。分号由服务端解析,与 mysql CLI 的 DELIMITER 无关。
            'BEGIN\n  $body;\nEND;'
    ];
  }

  /// SQL Server:`CREATE TRIGGER name ON tbl {AFTER|INSTEAD OF} <事件列表> AS <主体>`。
  /// 语句级(无 `FOR EACH ROW`);该方言没有 `BEFORE`,最接近的是 `INSTEAD OF`,
  /// 其余时机一律按 `AFTER` 输出。事件用逗号列表,一条 CREATE 即可表达。
  static List<String> _sqlServerTriggerSql(DesignTable d, String typeId, DesignTrigger tr) {
    final events = triggerEvents(tr, includeTruncate: false);
    if (events.isEmpty) return const [];
    final body = _normalizeBody(tr.body);
    if (body.isEmpty) return const [];
    final timing =
        tr.timing.trim().toUpperCase() == 'INSTEAD OF' ? 'INSTEAD OF' : 'AFTER';
    // 触发器是 schema 级对象,与表同模式落位
    final trgName = tr.name.trim();
    final qualifiedName = d.schema == null || d.schema!.isEmpty
        ? ident(typeId, trgName)
        : '${ident(typeId, d.schema!)}.${ident(typeId, trgName)}';
    return [
      'CREATE TRIGGER $qualifiedName ON ${_qualifiedTable(d, typeId)} '
          '$timing ${events.join(', ')} AS BEGIN\n  $body;\nEND;'
    ];
  }

  /// 触发函数引用:函数名可能带模式前缀,参数为空时不输出括号。
  static String _pgFunctionRef(String function, String params) {
    final parts = function.trim().split('.');
    String name;
    String? schema;
    if (parts.length >= 2) {
      schema = parts.sublist(0, parts.length - 1).join('.');
      name = parts.last.trim();
    } else {
      name = function.trim();
    }
    final quotedName = '"${name.replaceAll('"', '""')}"';
    final ref = schema == null || schema.isEmpty
        ? quotedName
        : '"${schema.replaceAll('"', '""')}".$quotedName';
    return params.isEmpty ? ref : '$ref($params)';
  }

  // ── 「设计表」保存:原始快照 vs 目标设计 → ALTER 差异 ────────

  /// 名称配对用的键(去空格 + 小写)
  static String _nameKey(String n) => n.trim().toLowerCase();

  static bool _sameText(String a, String b) => a.trim() == b.trim();

  static bool _sameList(List<String> a, List<String> b) =>
      a.length == b.length &&
      List.generate(a.length, (i) => a[i] == b[i]).every((e) => e);

  /// 列的类型文本(含 PostgreSQL 数组维度),用于差异比较与 TYPE 子句输出
  static String _columnTypeSql(DesignColumn c, String typeId) {
    var t = c.fullType;
    final dim = c.dimension.trim();
    if (dim.isNotEmpty && isPgLike(typeId)) t += '[$dim]';
    return t;
  }

  /// 列属性签名(不含列名):两侧签名相同即该列无需 ALTER。
  /// MySQL 的列注释内联在列定义里,故计入签名;其余方言走独立的注释语句。
  static String _columnSignature(DesignColumn c, String typeId) => [
        _columnTypeSql(c, typeId),
        c.collation.trim().toLowerCase(),
        c.notNull ? 'notnull' : 'null',
        c.defaultValue.trim(),
        c.identityMode.trim().toUpperCase(),
        c.identityIncrement.trim(),
        c.identityMinValue.trim(),
        c.identityMaxValue.trim(),
        c.identityStart.trim(),
        c.identityCache.trim(),
        c.identityCycle ? 'cycle' : '',
        c.autoIncrement ? 'auto_increment' : '',
        if (isMysqlLike(typeId)) c.comment.trim(),
      ].join('|');

  /// 索引配对名(无名项按 CREATE 路径的自动命名规则补齐)
  static String _indexName(DesignIndex idx, DesignTable d) {
    if (idx.name.trim().isNotEmpty) return idx.name.trim();
    final cols = idx.columnList;
    return 'idx_${d.name.trim()}_${cols.isEmpty ? 'x' : cols.first}';
  }

  static String _indexSignature(DesignIndex e, String typeId) =>
      '${e.columns}|${e.method}|${e.unique}|${e.concurrent}|${e.fillFactor}|${e.tablespace}'
      // 弹窗的逐字段选项计入签名:改排序顺序 / 运算符类别应触发重建索引
      '|${e.columnItems.map((c) => '${c.name}:${c.collationSchema}.${c.collation}'
          ':${c.opClassSchema}.${c.opClass}:${c.order}:${c.nullsOrder}').join('/')}'
      // MySQL 家族的注释内联在 CREATE INDEX 里,只能靠重建生效;PG 有
      // COMMENT ON INDEX(见注释段),不计入签名以免无谓重建
      '${isMysqlLike(typeId) ? '|${e.comment.trim()}' : ''}';

  /// 唯一键 / 索引字段项的逐字段选项 → 签名片段(弹窗改排序规则、排序顺序
  /// 都要算「已修改」,与 [_indexSignature] 同口径)。
  static String _columnItemsSignature(List<DesignIndexColumn> items) => items
      .map((c) => '${c.name}:${c.collationSchema}.${c.collation}'
          ':${c.opClassSchema}.${c.opClass}:${c.order}:${c.nullsOrder}'
          ':${c.operatorSchema}.${c.operatorName}')
      .join('/');

  static String _uniqueSignature(DesignUniqueKey e) =>
      '${e.columns}|${e.deferrable}|${e.deferred}|${e.fillFactor}|${e.tablespace}'
      '|${_columnItemsSignature(e.columnItems)}';

  /// 检查约束签名:表达式 + 无继承(它改变 CREATE 语句)。
  static String _checkSignature(DesignCheck e) =>
      '${e.expression.trim()}|${e.noInherit}';

  /// 排除约束的「会影响 CREATE 语句」签名(注释不参与,与索引同一口径)。
  static String _excludeSignature(DesignExclude e) => [
        e.columns.trim(),
        _columnItemsSignature(e.columnItems),
        e.method.trim().toLowerCase(),
        e.predicate.trim(),
        e.deferrable.trim().toUpperCase(),
        e.deferred.trim().toUpperCase(),
      ].join('|');

  /// 规则的签名:主体归一化后比较(补个换行、多个分号不算一次变更)。
  static String _ruleSignature(DesignRule e) => [
        e.event.trim().toUpperCase(),
        e.action.trim().toUpperCase(),
        e.qualification.trim(),
        _normalizeBody(e.statement),
      ].join('|');

  /// 表级存储参数签名(编辑模式差异比对口径)。
  static String _storageSignature(DesignTable d) =>
      d.storageOptionPairs().map((e) => '${e.key}=${e.value}').join(',');

  /// 触发器的「会影响 CREATE 语句」签名。
  ///
  /// 必须覆盖**所有**参与生成的字段:旧版只比 `name|function|timing`,于是
  /// 改主体 / 改勾选事件 / 改 WHEN 都不算变更,「设计表」按「无变更」直接不保存,
  /// 用户改完什么都没发生。[DesignTrigger.comment] 不进来 —— 没有任何方言有
  /// COMMENT ON TRIGGER,它不参与生成,自然也不该触发「已修改」。
  static String _triggerSignature(DesignTrigger e) => [
        e.name.trim(),
        e.timing,
        e.forEach,
        '${e.insert}${e.update}${e.delete}${e.truncate}',
        e.updateColumns.trim(),
        e.when.trim(),
        e.function.trim(),
        e.parameters.trim(),
        // 归一化后比较:补个换行、删个尾分号不该被当成一次结构变更
        _normalizeBody(e.body),
        e.deferrable,
        e.deferred,
        '${e.enable}',
      ].join('|');


  static String _fkSignature(DesignForeignKey e) =>
      '${e.columns}|${e.refSchema}|${e.refTable}|${e.refColumns}|${e.onDelete}|${e.onUpdate}|${e.matchAll}|${e.deferrable}|${e.deferred}';

  static String _uniqueName(DesignUniqueKey uk, DesignTable d) {
    if (uk.name.trim().isNotEmpty) return uk.name.trim();
    final cols = uk.columnList;
    return 'uq_${d.name.trim()}_${cols.isEmpty ? 'x' : cols.first}';
  }

  static String _checkName(DesignCheck ck, DesignTable d) =>
      ck.name.trim().isEmpty ? 'ck_${d.name.trim()}_${_nameKey(ck.expression)}' : ck.name.trim();

  static String _excludeName(DesignExclude ex, DesignTable d) {
    if (ex.name.trim().isNotEmpty) return ex.name.trim();
    // 排除元素写的是 `col WITH =` / `lower(id) WITH =`,无名兜底只取第一个元素
    // 的标识符部分,否则约束名里会带进空格和括号(`ex_t_lower(id) WITH =`)。
    final word = ex.columns
        .split(',')
        .map((e) => e.trim())
        .firstWhere((e) => e.isNotEmpty, orElse: () => '')
        .split(RegExp(r'\s+'))
        .first;
    final bare = word.replaceFirst(RegExp(r'^["`\[]+'), '');
    final head = RegExp(r'^[\w$]+').firstMatch(bare)?.group(0) ?? '';
    return 'ex_${d.name.trim()}_${head.isEmpty ? 'x' : head}';
  }

  /// 同名配对的注释是否发生了变更(非 PG 方言无对应语法时用于阻断提示)。
  static bool _hasCommentLoss<T>(
    List<T> target,
    List<T> original,
    String Function(T) nameOf,
    String Function(T) commentOf,
  ) {
    final orig = <String, T>{
      for (final o in original) nameOf(o).toLowerCase(): o,
    };
    for (final t in target) {
      final o = orig[nameOf(t).toLowerCase()];
      if (!_sameText(o == null ? '' : commentOf(o), commentOf(t))) return true;
    }
    return false;
  }

  static String _fkName(DesignForeignKey fk, DesignTable d) {
    if (fk.name.trim().isNotEmpty) return fk.name.trim();
    final cols = fk.columnList;
    return 'fk_${d.name.trim()}_${cols.isEmpty ? 'x' : cols.first}';
  }

  /// SQL Server 的裸对象名(schema.table,不加引号;sp_rename 要求此形式)
  static String _rawSqlServerTable(DesignTable d) {
    final schema = d.schema?.trim();
    return '${schema == null || schema.isEmpty ? 'dbo' : schema}.${d.name.trim()}';
  }

  /// SQL Server:按列反查并删除默认值约束。
  /// 未显式命名的默认值约束由服务端生成随机后缀名,无法预测,
  /// 故用一可重入批次在库内查找真名后动态 DROP。
  static String _sqlServerDropDefaultSql(DesignTable d, String column) {
    final t = _rawSqlServerTable(d).replaceAll("'", "''");
    final c = column.trim().replaceAll("'", "''");
    return "DECLARE @dc nvarchar(260) = (SELECT dc.name FROM sys.default_constraints dc "
        "JOIN sys.columns c ON c.object_id = dc.parent_object_id "
        "AND c.column_id = dc.parent_column_id "
        "WHERE dc.parent_object_id = OBJECT_ID(N'$t') AND c.name = N'$c'); "
        "IF @dc IS NOT NULL EXEC(N'ALTER TABLE $t DROP CONSTRAINT [' + @dc + N']');";
  }

  /// 注释语句(PostgreSQL / SQL Server;MySQL 表注释合入 ALTER 动作)
  static String _commentValue(String v) =>
      v.trim().isEmpty ? 'NULL' : lit(v.trim());

  /// 各方言主键约束名的约定形式(反查不到真实名字时的兜底)。
  ///
  /// MySQL / MariaDB 的主键固定名为 `PRIMARY`;PostgreSQL 为 `<表名>_pkey`;
  /// SQL Server 未显式命名时由服务端生成随机后缀,无法预测,故返回空串
  /// (调用方按 `sys.key_constraints` 反查真名,不猜)。
  static String defaultPkName(String dialect, String table) {
    if (isMysqlLike(dialect)) return 'PRIMARY';
    if (isPgLike(dialect)) return '${table}_pkey';
    return '';
  }

  /// 删除索引语句(各方言形式不同)
  static String _dropIndexSql(DesignTable d, String typeId, String name) {
    final tbl = _qualifiedTable(d, typeId);
    if (isSqlServerLike(typeId)) {
      return 'DROP INDEX IF EXISTS ${ident(typeId, name)} ON $tbl;';
    }
    if (isMysqlLike(typeId)) {
      return 'ALTER TABLE $tbl DROP INDEX ${ident(typeId, name)};';
    }
    return 'DROP INDEX IF EXISTS ${qualified(typeId, d.schema, name)};';
  }

  /// 触发器限定名:PG / SQL Server 的触发器是 schema 级对象,与表同模式落位;
  /// MySQL 的触发器挂在库上、没有模式层,只用裸名(与 [_mysqlTriggerSql] 一致)。
  static String _triggerRef(DesignTable d, String typeId, String name) {
    if (isPgLike(typeId) || isSqlServerLike(typeId)) {
      return qualified(typeId, d.schema, name);
    }
    return ident(typeId, name);
  }

  /// 删除触发器语句(各方言形式不同)。
  ///
  /// - PostgreSQL:`DROP TRIGGER name ON table` —— 名字是表局部作用域,必须带 ON。
  /// - SQL Server:`DROP TRIGGER schema.name` —— 名字是 schema 级,不带 ON。
  /// - MySQL:`DROP TRIGGER name` —— 名字是库级,不带 ON。
  ///
  /// 一律带 `IF EXISTS`:反查基线可能因为权限 / 内部触发器过滤而与库内实际
  /// 不完全一致,少一条不该让整串变更失败。
  static String _dropTriggerSql(DesignTable d, String typeId, String name) {
    final id = ident(typeId, name.trim());
    if (isPgLike(typeId)) {
      return 'DROP TRIGGER IF EXISTS $id ON ${_qualifiedTable(d, typeId)};';
    }
    if (isSqlServerLike(typeId)) {
      return 'DROP TRIGGER IF EXISTS ${_triggerRef(d, typeId, name.trim())};';
    }
    return 'DROP TRIGGER IF EXISTS $id;';
  }

  /// 按名配对的两路差异:结构变了 → DROP + CREATE;只有注释变了 → 单发注释。
  ///
  /// 与 [buildAlterStatements] 里的局部 `diffByName` 的区别就在注释:排除约束 /
  /// 规则的注释是挂在对象上的独立语句,一旦走 DROP + CREATE 就随对象一起没了,
  /// 所以必须知道这一行到底有没有被重建 —— `diffByName` 只交得出「删谁 / 加谁」,
  /// 配不上对,补不回来。
  static void _diffNamedObjects<T>(
    List<T> olds,
    List<T> news,
    String Function(T) nameOf,
    String Function(T) sigOf,
    String Function(T) commentOf, {
    required void Function(T) onDrop,
    required void Function(T) onRecreate,
    required void Function(T) onComment,
  }) {
    final oldMap = <String, T>{for (final e in olds) nameOf(e).toLowerCase(): e};
    final newMap = <String, T>{for (final e in news) nameOf(e).toLowerCase(): e};
    for (final o in olds) {
      final t = newMap[nameOf(o).toLowerCase()];
      if (t == null || sigOf(t) != sigOf(o)) onDrop(o);
    }
    for (final n in news) {
      final o = oldMap[nameOf(n).toLowerCase()];
      if (o == null || sigOf(o) != sigOf(n)) {
        onRecreate(n);
        if (commentOf(n).trim().isNotEmpty) onComment(n);
        continue;
      }
      if (!_sameText(commentOf(o), commentOf(n))) onComment(n);
    }
  }

  /// 生成「设计表」的变更语句:比较 [original](打开时的反查快照)与 [target]
  /// (界面当前值),只输出必要的 ALTER 语句;无差异返回空列表。
  ///
  /// 列配对策略:按列名(忽略大小写)匹配;同序号且双方都无同名配对的
  /// 「删+增」判定为重命名。语句顺序:表重命名 → 索引/约束删除 → 列重命名 →
  /// 删列 → 加列 → 改列 → 主键重建 → 索引/约束新增 → 注释。
  /// MySQL 把所有列与主键动作合并为单条 `ALTER TABLE`(单次扫表,并避开
  /// 「AUTO_INCREMENT 列必须是键」的逐步限制)。
  static List<String> buildAlterStatements(
      DesignTable target, DesignTable original, String typeId) {
    final isPg = isPgLike(typeId);
    final isMysql = isMysqlLike(typeId);
    final tbl = _qualifiedTable(target, typeId);

    final head = <String>[];
    final drops = <String>[];
    final colSqls = <String>[]; // PostgreSQL / SQL Server 的逐条列级语句
    final mysqlActions = <String>[]; // MySQL 合并为单条 ALTER 的动作片段
    final adds = <String>[];
    final comments = <String>[];

    // 1. 表重命名:其后语句一律以新名限定
    if (!_sameText(original.name, target.name)) {
      final oldRef = _qualifiedTable(original, typeId);
      final bare = ident(typeId, target.name.trim());
      if (isPg) {
        head.add('ALTER TABLE $oldRef RENAME TO $bare;');
      } else if (isMysql) {
        head.add('RENAME TABLE $oldRef TO $bare;');
      } else {
        head.add('EXEC sp_rename ${lit(_rawSqlServerTable(original))}, '
            '${lit(target.name.trim())}, ${lit('OBJECT')};');
      }
    }

    // 2. 列配对
    final origByName = <String, DesignColumn>{
      for (final c in original.columns) _nameKey(c.name): c,
    };
    final targByName = <String, DesignColumn>{
      for (final c in target.columns) _nameKey(c.name): c,
    };
    final added = <DesignColumn>[
      for (final c in target.columns)
        if (!origByName.containsKey(_nameKey(c.name))) c,
    ];
    final removed = <DesignColumn>[
      for (final o in original.columns)
        if (!targByName.containsKey(_nameKey(o.name))) o,
    ];
    final renameOf = <DesignColumn, DesignColumn>{}; // 目标列 -> 原列
    final targetOf = <DesignColumn, DesignColumn>{}; // 原列 -> 目标列
    final usedAdded = <DesignColumn>{};
    final usedRemoved = <DesignColumn>{};
    final n = original.columns.length < target.columns.length
        ? original.columns.length
        : target.columns.length;
    for (var i = 0; i < n; i++) {
      final o = original.columns[i];
      final t = target.columns[i];
      if (removed.contains(o) &&
          added.contains(t) &&
          !usedRemoved.contains(o) &&
          !usedAdded.contains(t)) {
        usedRemoved.add(o);
        usedAdded.add(t);
        renameOf[t] = o;
        targetOf[o] = t;
      }
    }
    for (final o in original.columns) {
      if (!targetOf.containsKey(o)) {
        final t = targByName[_nameKey(o.name)];
        if (t != null) targetOf[o] = t;
      }
    }
    final pureAdded =
        <DesignColumn>[for (final c in added) if (!usedAdded.contains(c)) c];
    final pureRemoved =
        <DesignColumn>[for (final o in removed) if (!usedRemoved.contains(o)) o];

    // 3. 索引 / 唯一键 / 外键 / 检查:先删后增(避开列被约束引用而删除失败)
    void diffByName<T>(
      List<T> olds,
      List<T> news,
      String Function(T) nameOf,
      String Function(T) sigOf,
      void Function(T) onDrop,
      void Function(T) onAdd,
    ) {
      final oldMap = <String, T>{for (final e in olds) nameOf(e).toLowerCase(): e};
      final newMap = <String, T>{for (final e in news) nameOf(e).toLowerCase(): e};
      for (final e in olds) {
        final m = newMap[nameOf(e).toLowerCase()];
        if (m == null || sigOf(m) != sigOf(e)) onDrop(e);
      }
      for (final e in news) {
        final m = oldMap[nameOf(e).toLowerCase()];
        if (m == null || sigOf(m) != sigOf(e)) onAdd(e);
      }
    }

    diffByName<DesignIndex>(original.indexes, target.indexes,
        (e) => _indexName(e, target), (e) => _indexSignature(e, typeId),
        (e) => drops.add(_dropIndexSql(target, typeId, _indexName(e, original))),
        (e) => adds.add(buildCreateIndex(target, typeId, e)));
    diffByName<DesignUniqueKey>(original.uniqueKeys, target.uniqueKeys,
        (e) => _uniqueName(e, target), _uniqueSignature, (e) {
      final name = _uniqueName(e, original);
      drops.add('ALTER TABLE $tbl DROP '
          '${isMysql ? 'INDEX' : 'CONSTRAINT'} ${ident(typeId, name)};');
    }, (e) {
      final clause = uniqueKeyClause(e, target, typeId);
      if (clause.isNotEmpty) adds.add('ALTER TABLE $tbl ADD $clause;');
    });
    diffByName<DesignForeignKey>(original.foreignKeys, target.foreignKeys,
        (e) => _fkName(e, target), _fkSignature, (e) {
      final name = _fkName(e, original);
      drops.add('ALTER TABLE $tbl '
          '${isMysql ? 'DROP FOREIGN KEY' : 'DROP CONSTRAINT'} ${ident(typeId, name)};');
    }, (e) {
      final clause = foreignKeyClause(e, target, typeId);
      if (clause.isNotEmpty) adds.add('ALTER TABLE $tbl ADD $clause;');
    });
    diffByName<DesignCheck>(original.checks, target.checks,
        (e) => _checkName(e, target), _checkSignature, (e) {
      final name = _checkName(e, original);
      drops.add('ALTER TABLE $tbl DROP CONSTRAINT ${ident(typeId, name)};');
    }, (e) {
      final clause = checkClause(e, target, typeId);
      if (clause.isNotEmpty) adds.add('ALTER TABLE $tbl ADD $clause;');
    });

    // 3a2. 排除约束:同为表约束,按「DROP CONSTRAINT + ADD CONSTRAINT」重建。
    // 注释随 DROP 一起消失,故重建路径必须把注释补回来(见 [_diffConstraintComments])。
    if (isPg) {
      _diffNamedObjects<DesignExclude>(
        original.excludes,
        target.excludes,
        (e) => _excludeName(e, target),
        _excludeSignature,
        (e) => e.comment,
        onDrop: (e) => drops.add('ALTER TABLE $tbl DROP CONSTRAINT '
            '${ident(typeId, _excludeName(e, original))};'),
        onRecreate: (e) {
          final clause = excludeClause(e, target, typeId);
          if (clause.isNotEmpty) adds.add('ALTER TABLE $tbl ADD $clause;');
        },
        onComment: (e) => comments.add('COMMENT ON CONSTRAINT '
            '${ident(typeId, _excludeName(e, target))} ON $tbl IS '
            '${_commentValue(e.comment)};'),
      );
      // 3a3. 规则:不是约束,ALTER 表达不了,整条 DROP RULE + CREATE RULE 重建。
      _diffNamedObjects<DesignRule>(
        original.rules,
        target.rules,
        (e) => _ruleName(e, target),
        _ruleSignature,
        (e) => e.comment,
        onDrop: (e) => drops.add(buildDropRule(original, typeId, _ruleName(e, original))),
        onRecreate: (e) => adds.add(buildCreateRule(target, typeId, e)),
        onComment: (e) => comments.add(buildRuleComment(target, typeId, e)),
      );
      // 规则的「启用」不在 CREATE RULE 文本里(见 [_ruleSignature]),PG 用
      // ALTER TABLE … ENABLE|DISABLE RULE 单独切换:只改勾选时补这一条,
      // 跟着重建走的规则则在 CREATE 之后补上开关。
      final origRuleByName = <String, DesignRule>{
        for (final o in original.rules) _nameKey(_ruleName(o, target)): o,
      };
      for (final t in target.rules) {
        final name = _ruleName(t, target);
        if (name.isEmpty) continue;
        final o = origRuleByName[_nameKey(name)];
        if (o == null) {
          if (!t.enable) {
            adds.add(buildSetRuleEnable(target, typeId, name, false));
          }
          continue;
        }
        if (o.enable == t.enable) continue;
        adds.add(buildSetRuleEnable(
            target, typeId, _ruleName(o, original), t.enable));
      }
    }

    // 3b. 触发器:先 DROP 再 CREATE(改主体 / 改事件都按「换一个」处理)。
    //
    // 触发器不是表约束,ALTER TABLE … ADD/DROP 表达不了,只能整条重建;
    // 也不能像索引那样在 CREATE 上用 OR REPLACE(三种方言都没有)。
    // 名字带 `_insert` 后缀的情况见 [_mysqlTriggerSql]:MySQL 一个触发器只能挂
    // 一个事件,原始设计里一条会展开成多条 CREATE,反查回来即多条独立触发器,
    // 因此这里按「触发器名」一一配对即可,不需要感知展开关系。
    diffByName<DesignTrigger>(original.triggers, target.triggers,
        (e) => e.name.trim(), _triggerSignature, (e) {
      final name = e.name.trim();
      if (name.isEmpty) return;
      drops.add(_dropTriggerSql(original, typeId, name));
    }, (e) {
      adds.addAll(buildCreateTriggers(target, typeId, e));
      // PG 的「启用」是触发器建好之后的独立动作(见 buildStatements 同款补语)
      if (isPg && !e.enable && e.name.trim().isNotEmpty) {
        adds.add('ALTER TABLE $tbl DISABLE TRIGGER ${ident(typeId, e.name.trim())};');
      }
    });

    // 4a. 列重命名(PG / SQL Server;MySQL 由 CHANGE COLUMN 一并完成)
    if (!isMysql) {
      for (final entry in renameOf.entries) {
        final o = entry.value;
        final t = entry.key;
        if (isPg) {
          colSqls.add('ALTER TABLE $tbl RENAME COLUMN '
              '${ident(typeId, o.name.trim())} TO ${ident(typeId, t.name.trim())};');
        } else {
          colSqls.add('EXEC sp_rename '
              '${lit('${_rawSqlServerTable(target)}.${o.name.trim()}')}, '
              '${lit(t.name.trim())}, ${lit('COLUMN')};');
        }
      }
    }

    // 4b. 删列(被重命名占用的列已在 renameOf 中配对,不会进入本分支)
    for (final o in pureRemoved) {
      final col = ident(typeId, o.name.trim());
      if (isMysql) {
        mysqlActions.add('DROP COLUMN $col');
      } else {
        colSqls.add('ALTER TABLE $tbl DROP COLUMN $col;');
      }
    }

    // 4c. 加列(列序位置仅 MySQL 能表达)
    for (var i = 0; i < target.columns.length; i++) {
      final t = target.columns[i];
      if (!pureAdded.contains(t)) continue;
      final def = columnDefSql(t, typeId);
      if (isMysql) {
        mysqlActions.add('ADD COLUMN $def${_mysqlPositionSql(target, i, typeId)}');
      } else {
        colSqls.add('ALTER TABLE $tbl ADD COLUMN $def;');
      }
    }

    // 4d. 改列
    for (var i = 0; i < target.columns.length; i++) {
      final t = target.columns[i];
      final o = renameOf[t] ?? origByName[_nameKey(t.name)];
      if (o == null) continue; // 新增列已由 4c 承担
      final changed = _columnSignature(o, typeId) != _columnSignature(t, typeId);
      final moved = original.columns.indexOf(o) != i;
      if (!changed && !(isMysql && moved)) continue;
      if (isMysql) {
        final def = columnDefSql(t, typeId);
        final pos = moved ? _mysqlPositionSql(target, i, typeId) : '';
        mysqlActions.add(renameOf.containsKey(t)
            ? 'CHANGE COLUMN ${ident(typeId, o.name.trim())} $def$pos'
            : 'MODIFY COLUMN $def$pos');
      } else {
        colSqls.addAll(_alterColumnDiffSqls(o, t, target, tbl, typeId));
      }
    }

    // 4e. 表存储参数 / 表空间(仅 PG 有对应 ALTER;SQL Server 改填充因子需重建表)
    if (isPg) {
      final tOpts = {for (final e in target.storageOptionPairs()) e.key: e.value};
      final oOpts = {for (final e in original.storageOptionPairs()) e.key: e.value};
      final setPairs = [
        for (final e in tOpts.entries) //
          if (!_sameText(oOpts[e.key] ?? '', e.value)) '${e.key} = ${e.value}',
      ];
      final resetKeys = [for (final k in oOpts.keys) if (!tOpts.containsKey(k)) k];
      if (setPairs.isNotEmpty) {
        colSqls.add('ALTER TABLE $tbl SET (${setPairs.join(', ')});');
      }
      if (resetKeys.isNotEmpty) {
        colSqls.add('ALTER TABLE $tbl RESET (${resetKeys.join(', ')});');
      }
      if (!_sameText(target.tablespace, original.tablespace)) {
        // 清空即回默认表空间(PG 没有 DROP TABLESPACE 这一说)
        final ts = target.tablespace.trim().isEmpty ? 'pg_default' : target.tablespace.trim();
        colSqls.add('ALTER TABLE $tbl SET TABLESPACE ${ident(typeId, ts)};');
      }
    }

    // 5. 主键重建(列序或集合变化时)
    final oldPk = <String>[
      for (final o in original.columns) //
        if (o.primaryKey && targetOf[o] != null) _nameKey(targetOf[o]!.name),
    ];
    final newPk = [for (final name in pkColumnNames(target)) _nameKey(name)];
    if (!_sameList(oldPk, newPk)) {
      final oldPkName = original.pkName.trim().isNotEmpty
          ? original.pkName.trim()
          : defaultPkName(typeId, original.name.trim());
      final newPkName = target.pkName.trim().isNotEmpty
          ? target.pkName.trim()
          : (isPg || isMysql
              ? defaultPkName(typeId, target.name.trim())
              : 'PK_${target.name.trim()}');
      if (oldPk.isNotEmpty) {
        if (isMysql) {
          mysqlActions.add('DROP PRIMARY KEY');
        } else if (oldPkName.isNotEmpty) {
          colSqls.add('ALTER TABLE $tbl DROP CONSTRAINT ${ident(typeId, oldPkName)};');
        }
      }
      if (newPk.isNotEmpty) {
        final cols = pkColumnNames(target).map((e) => ident(typeId, e)).join(', ');
        if (isMysql) {
          mysqlActions.add('ADD PRIMARY KEY ($cols)');
        } else {
          colSqls.add('ALTER TABLE $tbl ADD CONSTRAINT '
              '${ident(typeId, newPkName)} PRIMARY KEY ($cols);');
        }
      }
    }

    // 6. 注释(MySQL 表注释作为 ALTER 动作,需先于合并语句采集)
    if (isMysql) {
      if (!_sameText(original.tableComment, target.tableComment)) {
        mysqlActions.add('COMMENT = ${lit(target.tableComment.trim())}');
      }
    } else if (isPg) {
      if (!_sameText(original.tableComment, target.tableComment)) {
        comments.add('COMMENT ON TABLE $tbl IS '
            '${_commentValue(target.tableComment)};');
      }
      for (final t in target.columns) {
        // 新增列无原列可比:注释非空即需补 COMMENT(否则用户填的注释会静默丢失)
        final o = renameOf[t] ?? origByName[_nameKey(t.name)];
        if (_sameText(o?.comment ?? '', t.comment)) continue;
        comments.add('COMMENT ON COLUMN $tbl.${ident(typeId, t.name.trim())} IS '
            '${_commentValue(t.comment)};');
      }
      // 外键注释:按约束名配对(未动过的约束不会进入本循环,原注释也不会被覆写)
      final origFkByName = <String, DesignForeignKey>{
        for (final o in original.foreignKeys) _nameKey(_fkName(o, target)): o,
      };
      for (final t in target.foreignKeys) {
        final name = _fkName(t, target);
        final o = origFkByName[_nameKey(name)];
        if (_sameText(o?.comment ?? '', t.comment)) continue;
        comments.add('COMMENT ON CONSTRAINT ${ident(typeId, name)} ON $tbl IS '
            '${_commentValue(t.comment)};');
      }
      // 索引注释:同上按索引名配对(结构变更重建的索引也已覆盖注释)
      final origIdxByName = <String, DesignIndex>{
        for (final o in original.indexes) _nameKey(_indexName(o, target)): o,
      };
      for (final t in target.indexes) {
        final name = _indexName(t, target);
        final o = origIdxByName[_nameKey(name)];
        if (_sameText(o?.comment ?? '', t.comment)) continue;
        comments.add('COMMENT ON INDEX '
            '${qualified(typeId, target.schema, name)} IS '
            '${_commentValue(t.comment)};');
      }
      // 唯一键约束注释
      final origUqByName = <String, DesignUniqueKey>{
        for (final o in original.uniqueKeys) _nameKey(_uniqueName(o, target)): o,
      };
      for (final t in target.uniqueKeys) {
        final name = _uniqueName(t, target);
        final o = origUqByName[_nameKey(name)];
        if (_sameText(o?.comment ?? '', t.comment)) continue;
        comments.add('COMMENT ON CONSTRAINT ${ident(typeId, name)} ON $tbl IS '
            '${_commentValue(t.comment)};');
      }
      // 检查约束注释
      final origCkByName = <String, DesignCheck>{
        for (final o in original.checks) _nameKey(_checkName(o, target)): o,
      };
      for (final t in target.checks) {
        final name = _checkName(t, target);
        final o = origCkByName[_nameKey(name)];
        if (_sameText(o?.comment ?? '', t.comment)) continue;
        comments.add('COMMENT ON CONSTRAINT ${ident(typeId, name)} ON $tbl IS '
            '${_commentValue(t.comment)};');
      }
      // 排除约束的注释走 [_diffNamedObjects](结构变更要连注释一起重建)
    } else {
      void emitProp(String? oldVal, String newVal, String? column) {
        if (_sameText(oldVal ?? '', newVal)) return;
        // 属性原值决定用哪个存储过程:无 → add,有 → update,清空 → drop
        final fn = newVal.trim().isEmpty
            ? 'sp_dropextendedproperty'
            : ((oldVal ?? '').trim().isEmpty
                ? 'sp_addextendedproperty'
                : 'sp_updateextendedproperty');
        comments.add(sqlServerCommentSql(
            schema: target.schema?.trim(),
            table: target.name.trim(),
            column: column,
            value: newVal.trim(),
            proc: fn));
      }

      emitProp(original.tableComment, target.tableComment, null);
      for (final t in target.columns) {
        // 同上:新增列的注释走 sp_addextendedproperty(原无属性)
        final o = renameOf[t] ?? origByName[_nameKey(t.name)];
        emitProp(o?.comment, t.comment, t.name.trim());
      }
    }

    final stmts = <String>[...head, ...drops];
    if (isMysql) {
      if (mysqlActions.isNotEmpty) {
        stmts.add('ALTER TABLE $tbl ${mysqlActions.join(', ')};');
      }
    } else {
      stmts.addAll(colSqls);
    }
    stmts.addAll(adds);
    stmts.addAll(comments);
    // 表所有者变更:PG 无内联子句,补一条 ALTER … OWNER TO。
    // 「结构同步」的「比较所有者」勾选时 _forTarget 会保留源侧所有者,差异由此落地;
    // 设计表流程里两侧所有者都取自实表,相等 → 不产生语句。
    if (isPg &&
        target.owner.trim().isNotEmpty &&
        !_sameText(original.owner, target.owner)) {
      stmts
          .add('ALTER TABLE $tbl OWNER TO ${ident(typeId, target.owner.trim())};');
    }
    return stmts;
  }

  /// MySQL 列位置子句(重排列序靠它表达)
  static String _mysqlPositionSql(DesignTable d, int index, String typeId) {
    if (index == 0) return ' FIRST';
    final prev = d.columns[index - 1].name.trim();
    return prev.isEmpty ? '' : ' AFTER ${ident(typeId, prev)}';
  }

  /// PostgreSQL / SQL Server 的单列属性变更语句集。
  ///
  /// 注:SQL Server 的计算列(Computed)无法用 ALTER COLUMN 重写,由服务端
  /// 报错并原样回显;此处不做猜测性屏蔽。
  static List<String> _alterColumnDiffSqls(
      DesignColumn o, DesignColumn t, DesignTable target, String tbl, String typeId) {
    final out = <String>[];
    final col = ident(typeId, t.name.trim());
    final oldType = _columnTypeSql(o, typeId);
    final newType = _columnTypeSql(t, typeId);
    if (isPgLike(typeId)) {
      final oldColl = o.collation.trim();
      final newColl = t.collation.trim();
      if (oldType != newType || oldColl != newColl) {
        final collate = newColl.isEmpty ? '' : ' COLLATE ${ident(typeId, newColl)}';
        out.add('ALTER TABLE $tbl ALTER COLUMN $col TYPE $newType$collate '
            'USING $col::$newType;');
      }
      if (o.notNull != t.notNull) {
        out.add('ALTER TABLE $tbl ALTER COLUMN $col '
            '${t.notNull ? 'SET NOT NULL' : 'DROP NOT NULL'};');
      }
      if (!_sameText(o.defaultValue, t.defaultValue)) {
        final v = t.defaultValue.trim();
        out.add(v.isEmpty
            ? 'ALTER TABLE $tbl ALTER COLUMN $col DROP DEFAULT;'
            : 'ALTER TABLE $tbl ALTER COLUMN $col SET DEFAULT ${_defaultLiteral(v)};');
      }
      if (_identitySignature(o) != _identitySignature(t)) {
        // 序列选项无法定点 ALTER(需知道内部序列名),统一 DROP 后重建;
        // 代价是该列的当前序列值重置(与 Navicat 的重建策略一致)。
        out.add('ALTER TABLE $tbl ALTER COLUMN $col DROP IDENTITY IF EXISTS;');
        final clause = _identityClause(t, typeId);
        if (clause.isNotEmpty) {
          out.add('ALTER TABLE $tbl ALTER COLUMN $col ADD $clause;');
        }
      }
      return out;
    }
    // SQL Server
    if (oldType != newType || o.notNull != t.notNull) {
      // 排序规则必须随 ALTER COLUMN 一起重写:该语法不带 COLLATE 时回退到
      // 数据库默认排序规则,已有显式排序规则的列会被静默改掉
      final coll = t.collation.trim();
      out.add('ALTER TABLE $tbl ALTER COLUMN $col $newType'
          '${coll.isEmpty ? '' : ' COLLATE $coll'} '
          '${t.notNull ? 'NOT NULL' : 'NULL'};');
    }
    if (!_sameText(o.defaultValue, t.defaultValue)) {
      out.add(_sqlServerDropDefaultSql(target, t.name.trim()));
      final v = t.defaultValue.trim();
      if (v.isNotEmpty) {
        final name = 'DF_${target.name.trim()}_${t.name.trim()}';
        out.add('ALTER TABLE $tbl ADD CONSTRAINT ${ident(typeId, name)} '
            'DEFAULT ${_defaultLiteral(v)} FOR $col;');
      }
    }
    return out;
  }

  static String _identitySignature(DesignColumn c) => [
        c.identityMode.trim().toUpperCase(),
        c.identityIncrement.trim(),
        c.identityMinValue.trim(),
        c.identityMaxValue.trim(),
        c.identityStart.trim(),
        c.identityCache.trim(),
        c.identityCycle ? 'cycle' : '',
      ].join('|');

  /// 「SQL 预览」在编辑模式下的文本(无变更时输出单行注释)
  static String alterPreview(
          DesignTable target, DesignTable original, String typeId) =>
      buildAlterStatements(target, original, typeId).join('\n');

  /// 是否存在待应用的变更(用于禁用「保存」与提示)。
  /// 不单独实现比较,直接复用差异生成,避免两处判定口径不一致。
  static bool hasChanges(DesignTable target, DesignTable original, String typeId) =>
      buildAlterStatements(target, original, typeId).isNotEmpty;

  /// 新建表时「当前方言表达不了」的设计项;返回 null = 全部都能落进 DDL。
  ///
  /// 与 [alterUnsupported] 同一原则:**宁可拒于执行前,也不静默丢弃**。
  /// 改造前这些项在 [buildStatements] 里被 `if (isPgLike)` 一类的门直接跳过,
  /// 界面上却照样能填能勾 —— 用户在 MySQL 连接上写了三条规则点保存,库里一条
  /// 都没有,而界面告诉他「保存成功」。本方法是那个缺口的唯一拦口。
  static String? createUnsupported(DesignTable d, String typeId) {
    final pg = isPgLike(typeId);
    if (!pg && d.rules.isNotEmpty) {
      return '规则(RULE)仅 PostgreSQL 支持,当前类型无法生成:请先清空「规则」标签的内容';
    }
    if (!pg && d.excludes.isNotEmpty) {
      return '排除约束(EXCLUDE)仅 PostgreSQL 支持,当前类型无法生成:请先清空「排除」标签的内容';
    }
    for (final fk in d.foreignKeys) {
      if (!pg && fk.comment.trim().isNotEmpty) {
        return '外键注释仅 PostgreSQL 支持(需 COMMENT ON CONSTRAINT):'
            '请清空外键「${_nameOr(fk.name, '未命名')}」的注释';
      }
      if (fk.matchAll && !pg && !isMysqlLike(typeId)) {
        return '「符合全部」(MATCH FULL)仅 PostgreSQL / MySQL 支持:'
            '请取消外键「${_nameOr(fk.name, '未命名')}」的勾选';
      }
    }
    for (final uk in d.uniqueKeys) {
      if (!pg && uk.comment.trim().isNotEmpty) {
        return '唯一键注释仅 PostgreSQL 支持:请清空唯一键「${_nameOr(uk.name, '未命名')}」的注释';
      }
      if (!pg && uk.tablespace.trim().isNotEmpty) {
        return '唯一键表空间仅 PostgreSQL 支持:请清空唯一键「${_nameOr(uk.name, '未命名')}」的表空间';
      }
      if (!pg && uk.fillFactor.trim().isNotEmpty) {
        return '唯一键填充因子仅 PostgreSQL 支持:请清空唯一键「${_nameOr(uk.name, '未命名')}」的填充因子';
      }
      if (!pg && uk.columnItems.any((c) => c.hasOptions)) {
        return '唯一键字段的排序规则 / 运算符类别 / 排序顺序仅 PostgreSQL 支持:'
            '请在「${_nameOr(uk.name, '未命名')}」的字段弹窗里清空这些选项';
      }
    }
    for (final ck in d.checks) {
      if (!pg && ck.comment.trim().isNotEmpty) {
        return '检查约束注释仅 PostgreSQL 支持:请清空检查「${_nameOr(ck.name, '未命名')}」的注释';
      }
      if (!pg && ck.noInherit) {
        return '「无继承」(NO INHERIT)仅 PostgreSQL 支持:'
            '请取消检查「${_nameOr(ck.name, '未命名')}」的勾选';
      }
    }
    for (final ex in d.excludes) {
      if (!pg && ex.comment.trim().isNotEmpty) {
        return '排除约束注释仅 PostgreSQL 支持:请清空排除「${_nameOr(ex.name, '未命名')}」的注释';
      }
    }
    for (final idx in d.indexes) {
      final name = _nameOr(idx.name, '未命名');
      if (!pg && idx.concurrent) {
        return '并发创建索引(CONCURRENTLY)仅 PostgreSQL 支持:请取消索引「$name」的「并发」勾选';
      }
      if (!pg && idx.tablespace.trim().isNotEmpty) {
        return '索引表空间仅 PostgreSQL 支持:请清空索引「$name」的表空间';
      }
      if (idx.fillFactor.trim().isNotEmpty && !pg && !isSqlServerLike(typeId)) {
        return '索引填充因子仅 PostgreSQL / SQL Server 支持:请清空索引「$name」的填充因子';
      }
      if (!pg) {
        // 弹窗里逐字段的排序规则 / 运算符类别 / 排序顺序 / Nulls 排序全是 PG 语法
        final hasOpt = idx.columnItems.any((c) => c.hasOptions);
        if (hasOpt) {
          return '索引字段的排序规则 / 运算符类别 / 排序顺序仅 PostgreSQL 支持:'
              '请在「$name」的字段弹窗里清空这些选项';
        }
      }
    }
    if (!pg) {
      if (d.unlogged) return '「不记录」(UNLOGGED)仅 PostgreSQL 支持,请先取消勾选';
      if (d.owner.trim().isNotEmpty) return '表所有者仅 PostgreSQL 支持,请先清空「所有者」';
      if (d.inherits.trim().isNotEmpty) return '继承(INHERITS)仅 PostgreSQL 支持,请先清空「继承自」';
      if (d.cluster.trim().isNotEmpty) return '集群(CLUSTER)仅 PostgreSQL 支持,请先清空「集群」';
      // 表级 TABLESPACE 子句只有 PG 会输出(SQL Server 用的是 ON [PRIMARY]
      // 文件组,是另一套语法,设计器尚未支持)
      if (d.tablespace.trim().isNotEmpty) {
        return '表空间仅 PostgreSQL 支持,请先清空「表空间」';
      }
      // 存储参数:PG 全量可生成,SQL Server 只认填充因子,其它方言一项都不出
      for (final e in d.storageOptionPairs()) {
        if (isSqlServerLike(typeId) && e.key == kPgFillFactor) continue;
        return '表存储参数 ${e.key} 仅 PostgreSQL 支持'
            '${isSqlServerLike(typeId) ? '(SQL Server 只认填充因子)' : ''},'
            '请先在「选项」里清空它';
      }
    }
    return null;
  }

  /// 编辑模式下无法用 ALTER 表达的变更;返回 null = 可保存。
  ///
  /// 原则:宁可阻断保存并说明原因,也不静默丢弃用户的修改。
  static String? alterUnsupported(
      DesignTable target, DesignTable original, String typeId) {
    if (!isPgLike(typeId) && !isMysqlLike(typeId) && !isSqlServerLike(typeId)) {
      return '当前数据库类型不支持编辑已有表结构';
    }
    // 触发器已支持编辑:增删改都按「DROP + CREATE」重建(见 buildAlterStatements 3b)。
    // 先决条件是三个驱动都在 readTableDesign 里回填了 triggers —— 基线为空的话
    // 这里会把库里既有的触发器全当成「新增」重复建,那份守卫在各驱动的反查里。
    //
    // 排除约束 / 规则同一套路:排除走 ALTER TABLE DROP + ADD CONSTRAINT,规则走
    // DROP RULE + CREATE RULE(见 3a2 / 3a3)。两者都只有 PostgreSQL 有语法,
    // 其余方言连反查都读不回来(基线恒空),变更只能阻断。
    final pg = isPgLike(typeId);
    if (!pg &&
        (!_sameSigList(
            target.rules.map((e) => '${e.name}|${_ruleSignature(e)}').toList(),
            original.rules.map((e) => '${e.name}|${_ruleSignature(e)}').toList()) ||
            !_sameSigList(
                target.excludes.map((e) => '${e.name}|${_excludeSignature(e)}').toList(),
                original.excludes.map((e) => '${e.name}|${_excludeSignature(e)}').toList()))) {
      return '规则 / 排除约束仅 PostgreSQL 支持,当前数据库类型无法修改';
    }
    // 索引注释:PG 走 COMMENT ON INDEX、MySQL 内联进 CREATE INDEX(已计入索引
    // 签名),SQL Server 的扩展属性不覆盖索引层次 → 只能阻断提示
    if (isSqlServerLike(typeId) &&
        _hasCommentLoss(target.indexes, original.indexes,
            (e) => _indexName(e, target), (e) => e.comment)) {
      return '索引注释暂仅支持 PostgreSQL / MySQL 在「设计表」中修改';
    }
    // 唯一键 / 检查约束注释:除 PG 的 COMMENT ON CONSTRAINT 外其它方言无对应语法
    if (!isPgLike(typeId) &&
        (_hasCommentLoss(target.uniqueKeys, original.uniqueKeys,
            (e) => _uniqueName(e, target), (e) => e.comment) ||
            _hasCommentLoss(
                target.checks, original.checks, (e) => _checkName(e, target), (e) => e.comment))) {
      return '唯一键 / 检查约束注释暂仅支持 PostgreSQL 在「设计表」中修改';
    }
    // 存储参数 / 表空间:PG 有 ALTER TABLE SET (…) / SET TABLESPACE(见 4e),
    // SQL Server 改填充因子需重建表、MySQL 无表级 reloption → 只能阻断。
    if (!pg &&
        (!_sameText(target.tablespace, original.tablespace) ||
            _storageSignature(target) != _storageSignature(original))) {
      return '表空间 / 存储参数仅 PostgreSQL 支持在「设计表」中修改';
    }
    // 不记录 / 继承 / 集群:任何方言都没有对应的 ALTER(前两项需重建表),
    // 改造前用户的修改会被静默丢弃,这里补上拦口。
    if (target.unlogged != original.unlogged) {
      return '「不记录」(UNLOGGED)只能在「新建表」时设定,已有表无法用 ALTER 修改';
    }
    if (!_sameText(target.inherits, original.inherits)) {
      return '继承(INHERITS)只能在「新建表」时设定,已有表无法用 ALTER 修改';
    }
    if (!_sameText(target.cluster, original.cluster)) {
      return '「集群」是一条 CLUSTER 命令而非结构变更,暂不支持在「设计表」中修改';
    }
    if (!isMysqlLike(typeId)) {
      // 列序调整:PG / SQL Server 无对应语法(不做隐式表重建)
      final tIndex = <String, int>{
        for (var i = 0; i < target.columns.length; i++) //
          _nameKey(target.columns[i].name): i,
      };
      for (var i = 0; i < original.columns.length; i++) {
        final j = tIndex[_nameKey(original.columns[i].name)];
        if (j != null && j != i) {
          return '${isPgLike(typeId) ? 'PostgreSQL' : 'SQL Server'} 不支持调整已有列的顺序';
        }
      }
    }
    if (isSqlServerLike(typeId)) {
      final tMap = <String, DesignColumn>{
        for (final c in target.columns) _nameKey(c.name): c,
      };
      for (final o in original.columns) {
        final t = tMap[_nameKey(o.name)];
        if (t != null && _identitySignature(o) != _identitySignature(t)) {
          return 'SQL Server 不支持修改已有列的 IDENTITY 属性(请删除该列后重新添加)';
        }
      }
      final oldPk = [
        for (final o in original.columns) //
          if (o.primaryKey) _nameKey(o.name),
      ];
      final newPk = [for (final name in pkColumnNames(target)) _nameKey(name)];
      if (!_sameList(oldPk, newPk) && original.pkName.trim().isEmpty) {
        return '未能读取该表的主键约束名,无法重建主键';
      }
    }
    return null;
  }

  static bool _sameSigList(List<String> a, List<String> b) =>
      _sameList(a.toList()..sort(), b.toList()..sort());
}
