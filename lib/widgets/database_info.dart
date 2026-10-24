import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:provider/provider.dart';
import '../app/app_state.dart';
import '../data/db_data.dart';
import '../data/db_metadata.dart';
import '../data/db_types.dart';
import '../data/drivers/db_driver.dart';
import '../data/drivers/kv_driver.dart';
import '../data/table_design.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';
import 'object_category_icon.dart';

// 右侧对象详情面板:根据当前选中节点展示不同信息。
// - 选中表节点   -> 表详情(引擎 / 行格式 / 大小 / 时间戳 / 估算行数 + DDL 页)
// - 选中库节点   -> 库详情(默认字符集 / 排序规则 + DDL 页)
// - 选中连接节点 -> 连接信息
// - 未选中       -> 跟随对象页浏览上下文(objectContext)展示库信息,无则空态
//
// 库 / 表两页的排版对齐 Navicat 的详情面板:顶部 ⓘ / DDL 切换,头部
// 「大图标 + 名称 + 类型」,其下缩进对齐地列出所属连接与库,再是属性表。
// 属性一律来自系统目录的真实取值,取不到显示占位符而不猜值。
class DatabaseInfo extends StatelessWidget {
  const DatabaseInfo({super.key});

  @override
  Widget build(BuildContext context) {
    final app = context.read<AppState>();

    return ValueListenableBuilder<SelectedNode?>(
      valueListenable: app.detailSelection,
      builder: (context, node, _) {
        if (node != null) {
          switch (node.kind) {
            case NodeKind.table:
              return _TableDetailView(
                app: app,
                table: node.name,
                connection: node.connection,
                database: node.database,
                schema: node.schema,
              );
            case NodeKind.database:
              return _DatabaseDetailView(
                app: app,
                database: node.name,
                connection: node.connection,
              );
            case NodeKind.connection:
              return _ConnectionInfoView(connection: node.name);
            case NodeKind.schema:
              return _SchemaInfoView(
                schema: node.name,
                connection: node.connection,
                database: node.database,
              );
            case NodeKind.tableGroup:
              break;
            case NodeKind.connGroup:
              return _ConnGroupInfoView(group: node.name);
            case NodeKind.kvNamespace:
              return _KvNamespaceInfoView(
                app: app,
                namespace: node.name,
                connection: node.connection,
                namespaceId: node.database,
              );
          }
        }
        // 未选中节点:跟随对象页上下文展示库信息
        final conn = app.objectConnection;
        final db = app.objectDatabase;
        if (conn != null && db != null) {
          return _DatabaseDetailView(
              app: app, database: db, connection: conn);
        }
        return Container(
          color: Tokens.of(context).background,
          child: Empty(
            icon: const Icon(Icons.info_outline),
            title: context.l10n.infoPickNode,
            compact: true,
          ),
        );
      },
    );
  }

  /// 按连接名取连接配置,不存在返回 null
  static ConnectionInfo? _find(AppState app, String? name) {
    if (name == null) return null;
    for (final c in app.connections) {
      if (c.name == name) return c;
    }
    return null;
  }

  /// 按类型 id 查 DbType,不存在返回 null
  static DbType? _dbTypeOf(String? typeId) {
    if (typeId == null) return null;
    for (final e in kAllDbTypes) {
      if (e.id == typeId) return e;
    }
    return null;
  }
}

/// 大图标(默认 46px),与连接树同源的自绘 SVG
Widget _navIcon(String asset, {double size = 46}) => UiIcon(asset, size: size);

/// 头部图标列宽与「图标 / 文字」两列的间距:上下文行按此缩进,
/// 使其与名称文字左对齐(参考 Navicat 详情面板的排布)。
const double _headerIconSize = 46;
const double _headerGap = 14;
const double _panePadding = 16;

/// 目录里取不到属性时的占位符。纯标点、三语通用,故不进 ARB。
const String _missing = '--';

// ───────────────────────────────────────────────────────────── 库详情

/// 库详情视图(选中库 / 对象页浏览库时展示)。
///
/// MySQL / MariaDB 走系统目录取默认字符集与排序规则;其余类型驱动不提供
/// 这类信息,回退到连接基础字段(类型 / 主机 / 用户)。
class _DatabaseDetailView extends StatefulWidget {
  const _DatabaseDetailView({
    required this.app,
    required this.database,
    this.connection,
  });

  final AppState app;
  final String database;
  final String? connection;

  @override
  State<_DatabaseDetailView> createState() => _DatabaseDetailViewState();
}

class _DatabaseDetailViewState extends State<_DatabaseDetailView> {
  DatabaseDetail? _detail;
  String? _error;
  bool _loading = false;

  String? _ddl;
  String? _ddlError;
  bool _ddlLoading = false;

  /// 本次取数对应的「连接|库」,用于判断选中项是否真的换了
  String _cacheKey = '';

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(_DatabaseDetailView old) {
    super.didUpdateWidget(old);
    if (old.connection != widget.connection ||
        old.database != widget.database) {
      _detail = null;
      _error = null;
      _ddl = null;
      _ddlError = null;
      _load();
    }
  }

  ConnectionInfo? get _conn =>
      DatabaseInfo._find(widget.app, widget.connection);

  /// 只有真实连接且已打开才值得发目录查询(假连接与未连接时不猜值)
  bool get _queryable {
    final c = _conn;
    return c != null &&
        c.isLive &&
        widget.app.connectionManager.isConnected(c.name);
  }

  Future<void> _load({bool refresh = false}) async {
    final c = _conn;
    if (c == null || !_queryable) {
      _cacheKey = '${widget.connection}|${widget.database}';
      return;
    }
    final key = '${c.name}|${widget.database}';
    if (!refresh && _cacheKey == key && (_loading || _detail != null)) return;
    _cacheKey = key;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final d = await widget.app.connectionManager
          .databaseDetail(c, widget.database, refresh: refresh);
      if (!mounted || _cacheKey != key) return;
      setState(() {
        _detail = d;
        _loading = false;
      });
    } catch (e) {
      if (!mounted || _cacheKey != key) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _loadDdl() async {
    final c = _conn;
    if (c == null || !_queryable || _ddlLoading || _ddl != null) return;
    setState(() {
      _ddlLoading = true;
      _ddlError = null;
    });
    try {
      final sql = await widget.app.connectionManager
          .getDefinition(c, widget.database, widget.database, 'database');
      if (!mounted) return;
      setState(() {
        _ddl = sql;
        _ddlLoading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _ddlError = '$e';
        _ddlLoading = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final l = context.l10n;
    final c = _conn;
    final dbType = DatabaseInfo._dbTypeOf(c?.typeId);
    final mysqlLike = c != null && const {'mysql', 'mariadb'}.contains(c.typeId);
    final pgLike = c != null && DdlBuilder.isPgLike(c.typeId);

    final rows = <Widget>[];
    if (mysqlLike) {
      rows.add(_field(t, l.fieldCharset, _detail?.charset));
      rows.add(_field(t, l.fieldCollation, _detail?.collation));
    } else if (pgLike) {
      final d = _detail;
      rows.add(_field(t, l.fieldOid, d?.oid));
      rows.add(_field(t, l.fieldOwner, d?.owner));
      rows.add(_field(t, l.fieldTablespace, d?.tablespace));
      // 编码 / 排序规则排序:MySQL 那两行在同名概念上的 PG 侧写法,标签各自取
      rows.add(_field(t, l.fieldEncoding, d?.charset));
      rows.add(_field(t, l.fieldLcCollate, d?.collation));
      rows.add(_field(
          t,
          l.fieldConnectionLimit,
          d == null
              ? null
              : d.connectionLimit == '-1'
                  ? l.infoValueNoLimit
                  : d.connectionLimit));
      rows.add(_field(t, l.fieldComment, d?.comment));
    } else if (c != null) {
      // 非 MySQL 家族:驱动不提供这类目录属性,退回连接基础事实。
      // 主机与连接名已在上下文行,不重复列。
      rows.add(_field(t, l.fieldType, _typeName(c.typeId)));
      rows.add(_field(t, l.fieldUser, c.username));
    }
    if (_error != null) rows.add(_errorLine(t, l.infoDetailFailed(_error!)));

    return _DetailScaffold(
      app: widget.app,
      icon: _navIcon(kDatabaseIcon, size: _headerIconSize),
      title: widget.database,
      kindLabel: l.infoSectionDatabase,
      showDdl: mysqlLike || pgLike,
      contextRows: [
        if (c != null)
          _contextRow(
            context,
            dbType == null
                ? Icon(Icons.dns, size: 16, color: t.mutedForeground)
                : DbTypeIcon(type: dbType, size: 16),
            '${c.host}  ${c.name}',
          ),
      ],
      infoBody: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: rows,
      ),
      onLoadDdl: _loadDdl,
      ddl: _ddlLoading
          ? _DdlLoading()
          : _ddlError != null
              ? _DdlMessage(text: l.infoDdlFailed(_ddlError!))
              : _ddl == null
                  ? _DdlMessage(text: l.infoDdlUnsupported)
                  : _DdlText(sql: _ddl!),
    );
  }
}

// ───────────────────────────────────────────────────────────── 表详情

/// 表详情视图(选中表时展示)。
///
/// 属性全部来自一条系统目录查询(引擎统计,不扫描数据);精确行数由
/// 「获取行数」按钮显式触发 [ConnectionManager.countTable] 全表 COUNT。
class _TableDetailView extends StatefulWidget {
  const _TableDetailView({
    required this.app,
    required this.table,
    this.connection,
    this.database,
    this.schema,
  });

  final AppState app;
  final String table;
  final String? connection;
  final String? database;
  final String? schema;

  @override
  State<_TableDetailView> createState() => _TableDetailViewState();
}

class _TableDetailViewState extends State<_TableDetailView> {
  TableDetail? _detail;
  String? _error;
  bool _loading = false;

  /// 「获取行数」得到的精确值(null = 尚未取);切换选中表时清空
  int? _exactRows;
  bool _counting = false;
  String? _countError;

  String? _ddl;
  String? _ddlError;
  bool _ddlLoading = false;

  /// 「使用 / 被使用」两页的取数状态(PostgreSQL 表详情专有)。
  /// 与 DDL 页同样按需加载:切到那一页才发一次目录查询。
  final _uses = _DepSlot();
  final _usedBy = _DepSlot();

  String _cacheKey = '';

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(_TableDetailView old) {
    super.didUpdateWidget(old);
    if (old.connection != widget.connection ||
        old.database != widget.database ||
        old.schema != widget.schema ||
        old.table != widget.table) {
      _detail = null;
      _error = null;
      _exactRows = null;
      _counting = false;
      _countError = null;
      _ddl = null;
      _ddlError = null;
      _uses.reset();
      _usedBy.reset();
      _load();
    }
  }

  ConnectionInfo? get _conn =>
      DatabaseInfo._find(widget.app, widget.connection);

  bool get _queryable {
    final c = _conn;
    return c != null &&
        c.isLive &&
        widget.database != null &&
        widget.app.connectionManager.isConnected(c.name);
  }

  Future<void> _load({bool refresh = false}) async {
    final c = _conn;
    final db = widget.database;
    if (c == null || db == null || !_queryable) {
      _cacheKey = _key(c, db);
      return;
    }
    final key = _key(c, db);
    if (!refresh && _cacheKey == key && (_loading || _detail != null)) return;
    _cacheKey = key;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final d = await widget.app.connectionManager.tableDetail(
        c,
        db,
        widget.table,
        schema: widget.schema,
        refresh: refresh,
      );
      if (!mounted || _cacheKey != key) return;
      setState(() {
        _detail = d;
        _loading = false;
      });
    } catch (e) {
      if (!mounted || _cacheKey != key) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  String _key(ConnectionInfo? c, String? db) =>
      '${c?.name ?? widget.connection}|${db ?? ''}|${widget.schema ?? ''}'
      '|${widget.table}';

  Future<void> _fetchExactRowCount() async {
    final c = _conn;
    final db = widget.database;
    if (c == null || db == null || _counting) return;
    setState(() {
      _counting = true;
      _countError = null;
    });
    try {
      final n = await widget.app.connectionManager
          .countTable(c, db, widget.table, schema: widget.schema);
      if (!mounted) return;
      setState(() {
        _exactRows = n;
        _counting = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _countError = '$e';
        _counting = false;
      });
    }
  }

  Future<void> _loadDdl() async {
    final c = _conn;
    final db = widget.database;
    if (c == null || db == null || !_queryable || _ddlLoading || _ddl != null) {
      return;
    }
    setState(() {
      _ddlLoading = true;
      _ddlError = null;
    });
    try {
      final sql = await widget.app.connectionManager
          .getDefinition(c, db, widget.table, 'table', schema: widget.schema);
      if (!mounted) return;
      setState(() {
        _ddl = sql;
        _ddlLoading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _ddlError = '$e';
        _ddlLoading = false;
      });
    }
  }

  Future<void> _loadDeps(bool usedBy) async {
    final c = _conn;
    final db = widget.database;
    final slot = usedBy ? _usedBy : _uses;
    if (c == null || db == null || !_queryable || slot.loading || slot.loaded) {
      return;
    }
    slot.loading = true;
    slot.error = null;
    setState(() {});
    try {
      final list = await widget.app.connectionManager.tableDependencies(
        c,
        db,
        widget.table,
        schema: widget.schema,
        usedBy: usedBy,
      );
      if (!mounted) return;
      slot
        ..list = list
        ..loaded = true
        ..loading = false;
      setState(() {});
    } catch (e) {
      if (!mounted) return;
      slot.error = '$e';
      slot.loading = false;
      setState(() {});
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final l = context.l10n;
    final c = _conn;
    final dbType = DatabaseInfo._dbTypeOf(c?.typeId);
    final mysqlLike = c != null && const {'mysql', 'mariadb'}.contains(c.typeId);
    final pgLike = c != null && DdlBuilder.isPgLike(c.typeId);
    final d = _detail;

    final rows = <Widget>[
      if (mysqlLike) ...[
        _rowsField(t, l, d),
        _field(t, l.fieldEngine, d?.engine),
        _field(t, l.fieldAutoIncrement, d?.autoIncrement == null
            ? null
            : formatThousands(d!.autoIncrement!)),
        _field(t, l.fieldRowFormat, d?.rowFormat),
        _field(t, l.fieldUpdateTime, formatDetailTimestamp(d?.updateTime)),
        _field(t, l.fieldCreateTime, formatDetailTimestamp(d?.createTime)),
        _field(t, l.fieldCheckTime, formatDetailTimestamp(d?.checkTime)),
        _field(t, l.fieldIndexLength, formatByteSize(d?.indexLength)),
        _field(t, l.fieldDataLength, formatByteSize(d?.dataLength)),
        _field(t, l.fieldMaxDataLength, formatByteSize(d?.maxDataLength)),
        _field(t, l.fieldDataFree, formatByteSize(d?.dataFree)),
        _field(t, l.fieldCollation, d?.collation),
        _field(t, l.fieldCreateOptions, d?.createOptions),
        _field(t, l.fieldComment, d?.comment),
      ] else if (pgLike) ...[
        // 顺序照 Navicat 的 PG 表信息页:标识 → 归属 → 体量 → 继承关系 → 存储 → 权限
        _field(t, l.fieldOid, d?.oid),
        _field(t, l.fieldOwner, d?.owner),
        _rowsField(t, l, d),
        _field(t, l.fieldTableType, _pgTableTypeLabel(l, d?.tableType)),
        _field(t, l.fieldPartitionOf, d?.partitionOf),
        _field(t, l.fieldTablespace, d?.tablespace),
        _field(t, l.fieldInheritsFrom, d?.inheritsFrom),
        _field(
            t,
            l.fieldHasOids,
            d?.hasOids == null
                ? null
                : d!.hasOids!
                    ? l.infoValueYes
                    : l.infoValueNo),
        _field(t, l.fieldFillFactor, d?.fillFactor),
        _field(t, l.fieldAcl, d?.acl),
        _field(t, l.fieldComment, d?.comment),
      ] else ...[
        // 主机 / 连接名 / 库名已由上方上下文行给出,这里只补它没有的两项
        if (c != null) _field(t, l.fieldType, _typeName(c.typeId)),
        if (c != null) _field(t, l.fieldUser, c.username),
        if (widget.schema != null) _field(t, l.fieldSchema, widget.schema!),
      ],
    ];
    if (_error != null) rows.add(_errorLine(t, l.infoDetailFailed(_error!)));

    return _DetailScaffold(
      app: widget.app,
      icon: ObjectCategoryIcon(category: ObjectCategory.table, size: _headerIconSize),
      title: widget.table,
      kindLabel: l.infoSectionTable,
      showDdl: mysqlLike || pgLike,
      depPages: [
        if (pgLike) ...[
          _DepPage(
            icon: Icons.call_made,
            tooltip: l.infoPageUsesTooltip,
            onLoad: () => _loadDeps(false),
            body: _DependencyView(slot: _uses, emptyText: l.infoDepsEmpty),
          ),
          _DepPage(
            icon: Icons.call_received,
            tooltip: l.infoPageUsedByTooltip,
            onLoad: () => _loadDeps(true),
            body: _DependencyView(slot: _usedBy, emptyText: l.infoDepsEmpty),
          ),
        ],
      ],
      contextRows: [
        if (c != null)
          _contextRow(
            context,
            dbType == null
                ? Icon(Icons.dns, size: 16, color: t.mutedForeground)
                : DbTypeIcon(type: dbType, size: 16),
            '${c.host}  ${c.name}',
          ),
        if (widget.database != null)
          _contextRow(
            context,
            UiIcon(kDatabaseIcon, size: 16),
            widget.database!,
          ),
      ],
      infoBody: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: rows,
      ),
      onLoadDdl: _loadDdl,
      ddl: _ddlLoading
          ? _DdlLoading()
          : _ddlError != null
              ? _DdlMessage(text: l.infoDdlFailed(_ddlError!))
              : _ddl == null
                  ? _DdlMessage(text: l.infoDdlUnsupported)
                  : _DdlText(sql: _ddl!),
      footer: mysqlLike
          ? Row(
              children: [
                Icon(Icons.open_in_new, size: 13, color: t.disabledForeground),
                const SizedBox(width: 5),
                Expanded(
                  child: Text(
                    l.infoTableDoubleClickHint,
                    style: TextStyle(fontSize: 12, color: t.disabledForeground),
                  ),
                ),
              ],
            )
          : null,
    );
  }

  /// 「行」字段:估算值 + 可选的精确值,右侧挂「获取行数」链接。
  ///
  /// 估算值来自引擎统计,可能滞后;精确值只有点了按钮才 COUNT(*),
  /// 避免在浏览元数据时悄悄扫全表。
  Widget _rowsField(AppPalette t, AppLocalizations l, TableDetail? d) {
    final String value;
    if (_exactRows != null) {
      value = formatThousands(_exactRows!);
    } else if (d?.rowEstimate != null) {
      value = l.infoRowCountEstimate(formatThousands(d!.rowEstimate!));
    } else {
      value = _missing;
    }
    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(l.fieldRows,
              style: TextStyle(fontSize: 12, color: t.mutedForeground)),
          const SizedBox(height: 3),
          Row(
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              Flexible(
                child: Text(value,
                    style: TextStyle(fontSize: 13, color: t.foreground)),
              ),
              const SizedBox(width: 8),
              LinkLabel(
                text: _counting
                    ? l.infoFetchingRowCount
                    : l.infoFetchRowCount,
                underline: false,
                enabled: !_counting,
                onLinkTap: _fetchExactRowCount,
              ),
            ],
          ),
          if (_countError != null)
            Padding(
              padding: const EdgeInsets.only(top: 3),
              child: Text(
                l.infoRowCountFailed(_countError!),
                style: TextStyle(fontSize: 12, color: t.disabledForeground),
              ),
            ),
        ],
      ),
    );
  }
}

// ───────────────────────────────────────────────────────────── 详情外壳

/// 库 / 表详情共用的外壳:顶部页签条 + 头部(图标 / 名称 / 类型)
/// + 上下文行 + 正文。正文在「信息」与「DDL」两页间切换,头部保持不动。
class _DetailScaffold extends StatefulWidget {
  const _DetailScaffold({
    required this.app,
    required this.icon,
    required this.title,
    required this.kindLabel,
    required this.infoBody,
    required this.ddl,
    required this.contextRows,
    required this.onLoadDdl,
    this.showDdl = true,
    this.depPages = const [],
    this.footer,
  });

  final AppState app;
  final Widget icon;
  final String title;
  final String kindLabel;
  final Widget infoBody;
  final Widget ddl;
  final List<Widget> contextRows;
  final Future<void> Function() onLoadDdl;
  final bool showDdl;

  /// 「使用 / 被使用」两页(PostgreSQL 表详情);页签顺序即列表顺序。
  /// 空列表 = 该对象没有依赖页,顶部也不出现这两个图标。
  final List<_DepPage> depPages;
  final Widget? footer;

  /// 依赖页在页号里的起点:0 信息、1 DDL(若可见),其后依次是各依赖页。
  int get depBase => 1 + (showDdl ? 1 : 0);

  @override
  State<_DetailScaffold> createState() => _DetailScaffoldState();
}

/// 详情面板的一个依赖页:图标 + 提示 + 正文,首次切到该页时才取数。
class _DepPage {
  const _DepPage({
    required this.icon,
    required this.tooltip,
    required this.body,
    required this.onLoad,
  });

  final IconData icon;
  final String tooltip;
  final Widget body;
  final Future<void> Function() onLoad;
}

class _DetailScaffoldState extends State<_DetailScaffold> {
  /// 0 = 信息页,1 = DDL 页(可见时),其后是「使用 / 被使用」
  int _page = 0;

  @override
  void dispose() {
    super.dispose();
  }

  void _selectPage(int page) {
    if (_page == page) return;
    setState(() => _page = page);
    if (page == 1 && widget.showDdl) widget.onLoadDdl();
    final dep = page - widget.depBase;
    if (dep >= 0 && dep < widget.depPages.length) widget.depPages[dep].onLoad();
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);

    return Container(
      color: t.background,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _DetailTopBar(
            app: widget.app,
            page: _page,
            showDdl: widget.showDdl,
            depTabs: [
              for (final p in widget.depPages) (p.icon, p.tooltip),
            ],
            onSelectPage: _selectPage,
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(
                _panePadding, 10, _panePadding, 0),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                widget.icon,
                const SizedBox(width: _headerGap),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        widget.title,
                        style: TextStyle(
                            fontSize: 18,
                            fontWeight: FontWeight.w600,
                            color: t.foreground),
                        // 名称可能很长(库名/表名),超出一行截断而非撑破面板
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                      ),
                      const SizedBox(height: 3),
                      Text(widget.kindLabel,
                          style: TextStyle(
                              fontSize: 12, color: t.mutedForeground)),
                    ],
                  ),
                ),
              ],
            ),
          ),
          // 上下文行缩进到与名称文字同一条竖线上
          Padding(
            padding: EdgeInsets.fromLTRB(
                _panePadding + _headerIconSize + _headerGap, 14, _panePadding, 0),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: widget.contextRows,
            ),
          ),
          Expanded(
            child: () {
              final dep = _page - widget.depBase;
              if (dep >= 0 && dep < widget.depPages.length) {
                return widget.depPages[dep].body;
              }
              if (_page == 1 && widget.showDdl) return widget.ddl;
              return SingleChildScrollView(
                padding: const EdgeInsets.fromLTRB(
                    _panePadding, 18, _panePadding, _panePadding),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    widget.infoBody,
                    if (widget.footer != null) ...[
                      const SizedBox(height: 12),
                      widget.footer!,
                    ],
                  ],
                ),
              );
            }(),
          ),
        ],
      ),
    );
  }
}

/// 顶部页签条:ⓘ / DDL 两个切换按钮居中,右侧是面板加宽按钮。
class _DetailTopBar extends StatefulWidget {
  const _DetailTopBar({
    required this.app,
    required this.page,
    required this.showDdl,
    required this.onSelectPage,
    this.depTabs = const [],
  });

  final AppState app;
  final int page;
  final bool showDdl;

  /// 「使用 / 被使用」图标(页号接在 ⓘ / DDL 之后)
  final List<(IconData, String)> depTabs;
  final ValueChanged<int> onSelectPage;

  @override
  State<_DetailTopBar> createState() => _DetailTopBarState();
}

class _DetailTopBarState extends State<_DetailTopBar> {
  /// 加宽前的面板宽度,用于还原
  double? _restoreWidth;

  static const double _maxWidth = 640;

  /// 依赖页签的起始页号,须与 [_DetailScaffold.depBase] 一致
  int get depBase => widget.showDdl ? 2 : 1;

  void _toggleMaximize() {
    final w = widget.app.rightPanelWidth;
    if (_restoreWidth == null) {
      _restoreWidth = w.value;
      w.value = _maxWidth;
    } else {
      w.value = _restoreWidth!;
      _restoreWidth = null;
    }
    setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return Padding(
      padding: const EdgeInsets.fromLTRB(_panePadding, 6, 6, 0),
      child: Stack(
        alignment: Alignment.center,
        children: [
          Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              IconBtn(
                icon: Icons.info_outline,
                iconSize: 15,
                size: const Size(28, 26),
                selected: widget.page == 0,
                tooltip: l.infoPageInfo,
                onTap: () => widget.onSelectPage(0),
              ),
              const SizedBox(width: 4),
              if (widget.showDdl)
                IconBtn(
                  child: Text(
                    l.infoPageDdl,
                    style: const TextStyle(
                        fontSize: 10, fontWeight: FontWeight.w600),
                  ),
                  outline: true,
                  size: const Size(38, 22),
                  selected: widget.page == 1,
                  tooltip: l.infoPageDdl,
                  onTap: () => widget.onSelectPage(1),
                ),
              for (var i = 0; i < widget.depTabs.length; i++) ...[
                const SizedBox(width: 4),
                IconBtn(
                  icon: widget.depTabs[i].$1,
                  iconSize: 15,
                  size: const Size(28, 26),
                  selected: widget.page == depBase + i,
                  tooltip: widget.depTabs[i].$2,
                  onTap: () => widget.onSelectPage(depBase + i),
                ),
              ],
            ],
          ),
          Align(
            alignment: Alignment.centerRight,
            child: IconBtn(
              icon: _restoreWidth == null
                  ? Icons.open_in_full
                  : Icons.close_fullscreen,
              iconSize: 14,
              size: const Size(26, 26),
              tooltip: _restoreWidth == null
                  ? l.infoMaximizePanel
                  : l.infoRestorePanel,
              onTap: _toggleMaximize,
            ),
          ),
        ],
      ),
    );
  }
}

/// 上下文行:小图标 + 一行文字(所属连接 / 所属库)
Widget _contextRow(BuildContext context, Widget glyph, String text) {
  final t = Tokens.of(context);
  return Padding(
    padding: const EdgeInsets.only(bottom: 6),
    child: Row(
      children: [
        glyph,
        const SizedBox(width: 8),
        Flexible(
          child: Text(text,
              style: TextStyle(fontSize: 13, color: t.foreground),
              overflow: TextOverflow.ellipsis),
        ),
      ],
    ),
  );
}

// ───────────────────────────────────────────────────────────── DDL 页

class _DdlText extends StatelessWidget {
  const _DdlText({required this.sql});

  final String sql;

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final l = context.l10n;
    return Stack(
      children: [
        Positioned.fill(
          child: SingleChildScrollView(
            padding: const EdgeInsets.fromLTRB(
                _panePadding, 14, _panePadding, _panePadding),
            child: Text(
              sql,
              style: TextStyle(
                fontFamily: 'monospace',
                fontSize: 12,
                height: 1.45,
                color: t.foreground,
              ),
            ),
          ),
        ),
        Positioned(
          top: 8,
          right: 8,
          child: IconBtn(
            icon: Icons.copy_outlined,
            iconSize: 15,
            tooltip: l.infoCopyDdl,
            onTap: () => Clipboard.setData(ClipboardData(text: sql)),
          ),
        ),
      ],
    );
  }
}

class _DdlLoading extends StatelessWidget {
  const _DdlLoading({this.label});

  /// 覆盖默认文案(依赖页复用同一条加载行)
  final String? label;

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    return Padding(
      padding: const EdgeInsets.all(_panePadding),
      child: Row(
        children: [
          const Spinner(size: 14, strokeWidth: 1.6),
          const SizedBox(width: 8),
          Text(label ?? context.l10n.infoDdlLoading,
              style: TextStyle(fontSize: 12, color: t.mutedForeground)),
        ],
      ),
    );
  }
}

class _DdlMessage extends StatelessWidget {
  const _DdlMessage({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    return Padding(
      padding: const EdgeInsets.all(_panePadding),
      child: Text(text,
          style: TextStyle(fontSize: 12, color: t.mutedForeground)),
    );
  }
}

// ───────────────────────────────────────────────────────────── 依赖页

/// 一个依赖页(使用 / 被使用)的取数状态,与 DDL 页同样「进入才查」。
class _DepSlot {
  List<DependentObject>? list;
  bool loading = false;
  bool loaded = false;
  String? error;

  void reset() {
    list = null;
    loading = false;
    loaded = false;
    error = null;
  }
}

/// 依赖树:对象名 + 灰色「类型 (性质)」后缀,外键下挂 PG 自建的内部触发器。
class _DependencyView extends StatefulWidget {
  const _DependencyView({required this.slot, required this.emptyText});

  final _DepSlot slot;
  final String emptyText;

  @override
  State<_DependencyView> createState() => _DependencyViewState();
}

class _DependencyViewState extends State<_DependencyView> {
  List<TreeNode<DependentObject>> _nodes = const [];
  int? _selected;

  @override
  void initState() {
    super.initState();
    _rebuild();
  }

  @override
  void didUpdateWidget(_DependencyView old) {
    super.didUpdateWidget(old);
    // 换表时宿主会 reset() 槽位并重新取数:列表对象一变就重建树
    if (!identical(old.slot.list, widget.slot.list)) {
      _selected = null;
      _rebuild();
    }
  }

  void _rebuild() {
    _nodes = [
      for (final o in widget.slot.list ?? const <DependentObject>[]) _toNode(o),
    ];
  }

  TreeNode<DependentObject> _toNode(DependentObject o) => TreeNode(
        data: o,
        // 默认展开:折叠态看不出外键偷偷建了哪些触发器,而这正是这两页的重点
        expanded: o.children.isNotEmpty,
        children: [for (final c in o.children) _toNode(c)],
      );

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final slot = widget.slot;
    if (slot.loading) return _DdlLoading(label: context.l10n.infoDepsLoading);
    if (slot.error != null) {
      return _DdlMessage(text: context.l10n.infoDepsFailed(slot.error!));
    }
    if (_nodes.isEmpty) {
      return _DdlMessage(text: widget.emptyText);
    }
    return Padding(
      padding: const EdgeInsets.fromLTRB(_panePadding, 12, 0, 0),
      child: TreeView<DependentObject>(
        nodes: _nodes,
        framed: false,
        indent: 18,
        selectedKey: _selected,
        onSelectionChanged: (k) => setState(() => _selected = k),
        nodeToString: (o) => o.qualifiedName,
        rowBuilder: (ctx) => _row(context, t, ctx),
      ),
    );
  }

  Widget _row(
      BuildContext context, AppPalette t, TreeRowContext<DependentObject> ctx) {
    final o = ctx.node.data;
    return Container(
      // 选中态:浅色染色 + 上下描边(实心强调底会把左侧的类型图标吞掉)
      decoration: ctx.selected
          ? BoxDecoration(
              color: t.accent.withValues(alpha: 0.12),
              border: Border.symmetric(
                horizontal: BorderSide(color: t.accent, width: 1),
              ),
            )
          : null,
      child: Padding(
        padding: EdgeInsets.only(right: _panePadding),
        child: Row(
          children: [
            SizedBox(
              width: 18,
              child: ctx.hasChildren
                  ? GestureDetector(
                      behavior: HitTestBehavior.opaque,
                      onTap: ctx.toggleExpand,
                      child: Icon(
                        ctx.expanded
                            ? Icons.arrow_drop_down
                            : Icons.arrow_right,
                        size: 16,
                        color: t.mutedForeground,
                      ),
                    )
                  : null,
            ),
            SizedBox(
              width: 22,
              child: Center(child: _depGlyph(context, o.kind)),
            ),
            const SizedBox(width: 6),
            Flexible(
              child: Text(
                o.qualifiedName,
                style: TextStyle(fontSize: 13, color: t.foreground),
                overflow: TextOverflow.ellipsis,
              ),
            ),
            const SizedBox(width: 6),
            Flexible(
              child: Text(
                '${o.kind} (${o.degree})',
                style: TextStyle(fontSize: 12, color: t.mutedForeground),
                overflow: TextOverflow.ellipsis,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// 依赖行的左侧徽标:Navicat 给序列 / 索引画的是「123」「A-Z」文字标记,
/// 其余对象类型给线框图标;颜色一律取业务图标色板(明暗自适应)。
Widget _depGlyph(BuildContext context, String kind) {
  final c = AppColors.of(context);
  Widget icon(IconData i, Color color) => Icon(i, size: 15, color: color);
  Widget badge(String text) => Text(
        text,
        style: TextStyle(
            fontSize: 10, fontWeight: FontWeight.w700, color: c.iconPrimary),
      );
  return switch (kind) {
    'SEQUENCE' => badge('123'),
    'INDEX' || 'COLLATION' => badge('A-Z'),
    'TRIGGER' => icon(Icons.bolt, c.iconWarning),
    'FOREIGN KEY' || 'UNIQUE' => icon(Icons.link, c.iconInfo),
    'PRIMARY KEY' => icon(Icons.vpn_key, c.iconWarning),
    'NOT NULL' || 'CONVERSION' => icon(Icons.autorenew, c.iconInfo),
    'CHECK' || 'EXCLUSION' => icon(Icons.rule, c.iconInfo),
    'TYPE' => icon(Icons.bubble_chart, c.iconSecondary),
    'TABLE' || 'PARTITIONED TABLE' || 'TOAST TABLE' =>
      icon(Icons.table_chart, c.iconPrimary),
    'VIEW' || 'MATERIALIZED VIEW' || 'FOREIGN TABLE' =>
      icon(Icons.visibility, c.iconPrimary),
    'FUNCTION' => icon(Icons.functions, c.iconPrimary),
    'ROLE' => icon(Icons.person, c.iconSecondary),
    'SCHEMA' => icon(Icons.folder_outlined, c.iconWarning),
    'DEFAULT' => icon(Icons.tune, c.iconSecondary),
    _ => icon(Icons.help_outline, c.iconSecondary),
  };
}

// ───────────────────────────────────────────────────────────── 其余节点

/// 模式信息视图(选中模式节点时展示)
class _SchemaInfoView extends StatelessWidget {
  const _SchemaInfoView(
      {required this.schema, this.connection, this.database});

  final String schema;
  final String? connection;
  final String? database;

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final conn = DatabaseInfo._find(context.read<AppState>(), connection);

    return Container(
      color: t.background,
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // 与连接树同源:模式绿色层级图图标
          _navIcon(kSchemaIcon),
          const SizedBox(height: 8),
          Text(
            schema,
            style: TextStyle(
                fontSize: 20, fontWeight: FontWeight.w600, color: t.foreground),
          ),
          const SizedBox(height: 4),
          Text(context.l10n.infoSectionSchema,
              style: TextStyle(fontSize: 12, color: t.mutedForeground)),
          const SizedBox(height: 18),
          if (conn != null) ...[
            _field(t, context.l10n.fieldConnection, conn.name),
            _field(t, context.l10n.fieldType, _typeName(conn.typeId)),
            _field(t, context.l10n.fieldHost, '${conn.host}:${conn.port}'),
          ],
          if (database != null)
            _field(t, context.l10n.fieldDatabase, database!),
        ],
      ),
    );
  }
}

/// 连接信息视图(选中连接节点时展示)
class _ConnectionInfoView extends StatelessWidget {
  const _ConnectionInfoView({required this.connection});

  final String connection;

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final app = context.read<AppState>();
    final conn = DatabaseInfo._find(app, connection);
    // 与连接树同源:品牌图标 + 右下角状态点,离线时品牌色去饱和。
    // 状态必须按驱动链分流取(Redis 等缓存型引擎的驱动在 kvManager 里),
    // 只读 SQL 侧会让它恒判「未连接」—— 开着库也显示成灰底 + 灰点。
    final dbType = DatabaseInfo._dbTypeOf(conn?.typeId);
    final connected = conn != null &&
        (isKvStoreConnection(conn)
            ? app.kvManager.isConnected(conn.name)
            : app.connectionManager.isConnected(conn.name));

    return AnimatedBuilder(
      // 打开 / 关闭连接都不改选中项,面板本身不会重建;不挂这两条驱动链的
      // 通知,状态点就会停在旧状态(连接树是 watch 的,两边会显示成两种事实)
      animation: Listenable.merge([app.connectionManager, app.kvManager]),
      builder: (context, _) => Container(
        color: t.background,
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (dbType != null)
              DbTypeIcon(type: dbType, size: 46, connected: connected)
            else
              Icon(Icons.dns, size: 46, color: t.mutedForeground),
            const SizedBox(height: 8),
            Text(
              connection,
              style: TextStyle(
                  fontSize: 20, fontWeight: FontWeight.w600, color: t.foreground),
            ),
            const SizedBox(height: 4),
            Text(context.l10n.infoSectionConnection,
                style: TextStyle(fontSize: 12, color: t.mutedForeground)),
            const SizedBox(height: 18),
            if (conn != null) ...[
              _field(t, context.l10n.fieldType, _typeName(conn.typeId)),
              _field(t, context.l10n.fieldHost, '${conn.host}:${conn.port}'),
              _field(t, context.l10n.fieldUser, conn.username),
            ],
          ],
        ),
      ),
    );
  }
}

/// 连接分组信息视图(选中左侧树的分组节点时展示)。
///
/// 只列能真实派生的事实:组内连接数与类型分布。分组是纯本地视图层概念,
/// 没有服务端对应物,故不编造任何"描述/大小"类字段。
class _ConnGroupInfoView extends StatelessWidget {
  const _ConnGroupInfoView({required this.group});

  final String group;

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final app = context.read<AppState>();
    final members = app.connections.where((c) => c.group == group).toList();
    // 类型分布按 DbType 定义顺序聚合,避免同一分组两次渲染顺序抖动
    final counts = <String, int>{};
    for (final c in members) {
      counts[c.typeId] = (counts[c.typeId] ?? 0) + 1;
    }
    final ordered = [
      for (final type in kAllDbTypes)
        if (counts[type.id] != null) (type.label.split('\n').first, counts[type.id]!),
    ];

    return Container(
      color: t.background,
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          UiIcon(kConnGroupIcon, size: 46),
          const SizedBox(height: 8),
          Text(
            group,
            style: TextStyle(
                fontSize: 20, fontWeight: FontWeight.w600, color: t.foreground),
          ),
          const SizedBox(height: 4),
          Text(context.l10n.infoSectionConnGroup,
              style: TextStyle(fontSize: 12, color: t.mutedForeground)),
          const SizedBox(height: 18),
          _field(t, context.l10n.fieldConnCount, '${members.length}'),
          for (final (label, count) in ordered) _field(t, label, '$count'),
          if (members.isEmpty)
            Text(context.l10n.infoGroupEmpty,
                style: TextStyle(fontSize: 12, color: t.mutedForeground)),
        ],
      ),
    );
  }
}

/// 缓存型引擎(Redis 等)的命名空间详情。
///
/// 只有「哪个库 + 多少键」两件事:SQL 侧的库详情(字符集 / 排序规则 / 所有者)
/// 对 Redis 没有对应概念,走那条路只会得到一次被驱动拒掉的目录查询。
/// 键数取自驱动的 INFO keyspace / DBSIZE,取不到显示占位符而不猜值。
class _KvNamespaceInfoView extends StatelessWidget {
  const _KvNamespaceInfoView({
    required this.app,
    required this.namespace,
    this.connection,
    this.namespaceId,
  });

  final AppState app;
  final String namespace;
  final String? connection;

  /// 命名空间 id(Redis 的库号),装在 [SelectedNode.database] 槽位里带过来
  final String? namespaceId;

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final l = context.l10n;
    final conn = DatabaseInfo._find(app, connection);
    final ns = conn == null || namespaceId == null
        ? null
        : app.kvManager.namespacesOf(conn.name).namespace(namespaceId!);

    return Container(
      color: t.background,
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          UiIcon(kDatabaseIcon, size: 46),
          const SizedBox(height: 8),
          Text(
            namespace,
            style: TextStyle(
                fontSize: 20, fontWeight: FontWeight.w600, color: t.foreground),
          ),
          const SizedBox(height: 4),
          Text(l.infoSectionKvNamespace,
              style: TextStyle(fontSize: 12, color: t.mutedForeground)),
          const SizedBox(height: 18),
          _field(t, l.fieldConnection, conn?.name),
          if (conn != null) _field(t, l.fieldType, _typeName(conn.typeId)),
          _field(t, l.fieldKeyCount, ns?.keyCount?.toString()),
        ],
      ),
    );
  }
}

// ───────────────────────────────────────────────────────────── 公共小件
/// 按类型 id 查显示名(取 label 首行)
String _typeName(String typeId) {
  for (final type in kAllDbTypes) {
    if (type.id == typeId) return type.label.split('\n').first;
  }
  return typeId;
}

/// PG `relkind` 字母码 → 「表类型」的界面文字。
///
/// Navicat 的中文界面在这里给的是「常规」这类词，而不是 SQL 关键字，所以翻译
/// 落在界面侧（「使用 / 被使用」两页仍显示未译的类型码，与它一致）。
/// 认不出的字母原样回显：宁可看见一个陌生码，也不要被标成「常规」。
String? _pgTableTypeLabel(AppLocalizations l, String? code) {
  if (code == null || code.isEmpty) return null;
  return switch (code) {
    'r' => l.tableTypeRegular,
    'p' => l.tableTypePartitioned,
    'v' => l.tableTypeView,
    'm' => l.tableTypeMatView,
    'f' => l.tableTypeForeign,
    _ => code,
  };
}

/// 属性行:标签在上、取值在下。[value] 为空表示目录里没有该值,
/// 显示占位符而不是猜一个(0 / 空串都会把「无数据」误呈现成有效值)。
Widget _field(AppPalette t, String label, String? value) {
  final missing = value == null || value.isEmpty;
  return Padding(
    padding: const EdgeInsets.only(bottom: 16),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label, style: TextStyle(fontSize: 12, color: t.mutedForeground)),
        const SizedBox(height: 3),
        Text(
          missing ? _missing : value,
          style: TextStyle(
              fontSize: 13,
              color: missing ? t.mutedForeground : t.foreground),
        ),
      ],
    ),
  );
}

/// 详情读取失败的一行提示(就地展示,不弹全局提示条)
Widget _errorLine(AppPalette t, String message) {
  return Padding(
    padding: const EdgeInsets.only(bottom: 16, top: 2),
    child: Text(
      message,
      style: TextStyle(fontSize: 12, color: t.disabledForeground),
    ),
  );
}
