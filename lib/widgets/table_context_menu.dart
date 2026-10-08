import 'dart:io';

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

import '../app/app_state.dart';
import '../data/db_data.dart';
import '../data/matview_sql.dart';
import '../data/user_sql.dart';
import '../l10n/locale_config.dart';
import 'data_export_wizard.dart';
import 'data_import_wizard.dart';
import 'table_copy_rename_dialog.dart';

// 表实例右键菜单:打开 / 删除 / 清空 / 设计 / 转储SQL / 复制重命名。
// 连接树(database_tree)与对象面板(object_panel)共用本函数,保证两处交互一致。
// 需驱动已建立连接的动作(删除 / 清空 / 设计 / 转储 / 复制重命名)在未连接时禁用;
// 「打开表」与双击行为一致,不强制要求连接(标签页打开后数据页自行加载)。

/// 弹出表实例上下文菜单。
/// [position] 为全局屏幕坐标(直接来自 PointerDownEvent.position)。
void showTableContextMenu({
  required BuildContext context,
  required AppState app,
  required ConnectionInfo conn,
  required String database,
  required String table,
  String? schema,
  required Offset position,
}) {
  final isOpen = app.connectionManager.isConnected(conn.name);
  final l = context.l10n;
  final tableLabel = l.catTable;
  showContextMenu(
    context,
    position: position,
    items: [
      MenuItem(
        text: l.actionOpen(tableLabel),
        onPressed: () => app.openTable(
          table,
          connection: conn.name,
          database: database,
          schema: schema,
        ),
      ),
      MenuItem(
        text: l.actionDelete(tableLabel),
        enabled: isOpen,
        onPressed: () => _deleteTable(context, app, conn, database, table, schema),
      ),
      MenuItem(
        text: l.actionClear(tableLabel),
        enabled: isOpen,
        onPressed: () => _clearTable(context, app, conn, database, table, schema),
      ),
      MenuItem(
        text: l.actionDesign(tableLabel),
        enabled: isOpen,
        onPressed: () => app.designTable(
          table,
          connection: conn.name,
          database: database,
          schema: schema,
        ),
      ),
      MenuSeparator(),
      MenuItem(
        text: l.ctxDumpSql,
        enabled: isOpen,
        onPressed: () => _dumpTable(context, app, conn, database, table, schema),
      ),
      MenuItem(
        text: l.importWizard,
        enabled: isOpen,
        onPressed: () => showDataImportWizard(
          context,
          app: app,
          conn: conn,
          database: database,
          table: table,
          schema: schema,
        ),
      ),
      MenuItem(
        text: l.exportWizard,
        enabled: isOpen,
        onPressed: () => showDataExportWizard(
          context,
          app: app,
          conn: conn,
          database: database,
          table: table,
          schema: schema,
        ),
      ),
      MenuItem(
        text: l.ctxCopyRename,
        enabled: isOpen,
        onPressed: () => _copyRenameTable(context, app, conn, database, table, schema),
      ),
    ],
  );
}

/// 「删除表」:确认后 DROP TABLE 并刷新对象列表
Future<void> _deleteTable(
  BuildContext context,
  AppState app,
  ConnectionInfo conn,
  String database,
  String table,
  String? schema,
) async {
  final l = context.l10n;
  final title = l.actionDelete(l.catTable);
  final result = await MessageBox.show(
    context,
    title: title,
    message: l.deleteObjectConfirmOne(l.catTable, table),
    type: MessageBoxType.warning,
    buttons: MessageBoxButtons.okCancel,
    okText: l.btnDelete,
  );
  if (result != MessageBoxResult.ok || !context.mounted) return;
  final outcome = await app.dropTable(conn, database, table, schema: schema);
  if (!context.mounted) return;
  if (!outcome.ok) {
    MessageBox.show(
      context,
      title: title,
      message: l.deleteFailedDetail('${outcome.error}'),
      type: MessageBoxType.error,
      okText: l.btnGotIt,
    );
  }
}

/// 「清空表」:确认后删除全部行(保留结构)
Future<void> _clearTable(
  BuildContext context,
  AppState app,
  ConnectionInfo conn,
  String database,
  String table,
  String? schema,
) async {
  final l = context.l10n;
  final title = l.actionClear(l.catTable);
  final result = await MessageBox.show(
    context,
    title: title,
    message: l.clearConfirmOne(l.catTable, table),
    type: MessageBoxType.warning,
    buttons: MessageBoxButtons.okCancel,
    okText: l.btnClear,
  );
  if (result != MessageBoxResult.ok || !context.mounted) return;
  final outcome =
      await app.truncateTable(conn, database, table, schema: schema);
  if (!context.mounted) return;
  if (!outcome.ok) {
    MessageBox.show(
      context,
      title: title,
      message: l.clearFailedDetail('${outcome.error}'),
      type: MessageBoxType.error,
      okText: l.btnGotIt,
    );
  }
}

/// 「转储SQL」:选择保存位置,生成该表 CREATE TABLE DDL 并落盘
Future<void> _dumpTable(
  BuildContext context,
  AppState app,
  ConnectionInfo conn,
  String database,
  String table,
  String? schema,
) async {
  final l = context.l10n;
  final location = await getSaveLocation(
    acceptedTypeGroups: [XTypeGroup(label: l.sqlFileTypeLabel, extensions: ['sql'])],
    suggestedName: '${table}_structure.sql',
    confirmButtonText: l.btnSave,
  );
  if (location == null || !context.mounted) return;

  final sql = await app.dumpTableSql(conn, database, table, schema: schema);
  if (sql == null || !context.mounted) {
    MessageBox.show(
      context,
      title: l.ctxDumpSql,
      message: l.dumpStructureReadFailed,
      type: MessageBoxType.error,
      okText: l.btnGotIt,
    );
    return;
  }

  try {
    final file = File(location.path);
    await file.writeAsString(sql);
  } catch (e) {
    if (!context.mounted) return;
    MessageBox.show(
      context,
      title: l.ctxDumpSql,
      message: l.dumpWriteFailed('$e'),
      type: MessageBoxType.error,
      okText: l.btnGotIt,
    );
    return;
  }
  if (!context.mounted) return;
  MessageBox.show(
    context,
    title: l.ctxDumpSql,
    message: l.dumpDatabaseDone(table, location.path),
    type: MessageBoxType.info,
    okText: l.btnGotIt,
  );
}

/// 「复制重命名」:打开对话框,提供「复制」与「重命名」两个动作
Future<void> _copyRenameTable(
  BuildContext context,
  AppState app,
  ConnectionInfo conn,
  String database,
  String table,
  String? schema,
) async {
  await showDialog<bool>(
    context: context,
    builder: (_) => TableCopyRenameDialog(
      connection: conn,
      database: database,
      tableName: table,
      schema: schema,
    ),
  );
}

/// 视图 / 实体化视图实例右键菜单:打开 / 设计 / 刷新 / 删除 / 转储SQL。
/// 连接树与对象面板共用,保证两处交互一致。
///
/// 菜单项按**引擎真实能力**裁剪,而不是一律摆出来再禁用:
/// - 「设计」与「转储SQL」都要先读出 CREATE 定义。视图走驱动
///   [DatabaseDriver.getDefinition],Access 的该方法恒返回 null(ODBC 无对应目录),
///   故对 Access 不提供这两项 —— 点开只会是空编辑器、转储只会是空文件。
///   实体化视图的定义由界面侧 [MatViewSql] 从 PG 目录读取,故只在 PG 家族出现。
/// - MySQL 的 CREATE OR REPLACE VIEW 不含 SQL SECURITY 子句,设计页保存会把它
///   重置回默认。因此对 MySQL 系隐藏「设计」,避免一次无意的保存悄悄改掉视图的
///   权限语义(见下方 [_mysqlLosesSqlSecurity] 说明)。
/// - 「刷新 / 并发刷新」是实体化视图专属(视图是虚的,没有可刷新的物化内容)。
void showViewContextMenu({
  required BuildContext context,
  required AppState app,
  required ObjectCategory category,
  required ConnectionInfo conn,
  required String database,
  required String name,
  String? schema,
  required Offset position,
}) {
  final isOpen = app.connectionManager.isConnected(conn.name);
  final l = context.l10n;
  final label = category.labelOf(l);
  final isMatView = category == ObjectCategory.materializedView;
  // 实体化视图只有 PG 家族有(与非 PG 连接上根本列不出该分类一致)
  final matViewCapable = isMatView && MatViewSql.supports(conn.typeId);
  final canReadDefinition = isMatView
      ? matViewCapable
      : _supportsReadableView(conn.typeId);
  final canDesign = canReadDefinition &&
      (isMatView || !_mysqlLosesSqlSecurity(conn.typeId));

  showContextMenu(
    context,
    position: position,
    items: [
      // 与双击行为一致,不强制要求连接(标签页打开后数据页自行加载)
      MenuItem(
        text: l.actionOpen(label),
        onPressed: () => app.openTable(
          name,
          connection: conn.name,
          database: database,
          schema: schema,
        ),
      ),
      // 需驱动存活才能落 DDL,未连接时禁用(与表菜单同口径)
      if (canDesign)
        MenuItem(
          text: l.actionDesign(label),
          enabled: isOpen,
          onPressed: () => app.designRoutine(
            name,
            connection: conn.name,
            database: database,
            category: category,
            schema: schema,
          ),
        ),
      if (isMatView) ...[
        MenuItem(
          text: l.actionRefresh(label),
          enabled: isOpen,
          onPressed: () =>
              _refreshMatView(context, app, conn, database, name, schema),
        ),
        MenuItem(
          text: l.actionRefreshConcurrently(label),
          enabled: isOpen,
          onPressed: () => _refreshMatView(context, app, conn, database, name,
              schema,
              concurrently: true),
        ),
      ],
      MenuItem(
        text: l.actionDelete(label),
        enabled: isOpen,
        onPressed: () =>
            _deleteRoutine(context, app, category, conn, database, name, schema),
      ),
      if (canReadDefinition) ...[
        MenuSeparator(),
        MenuItem(
          text: l.ctxDumpSql,
          enabled: isOpen,
          onPressed: () =>
              _dumpRoutine(context, app, category, conn, database, name, schema),
        ),
      ],
    ],
  );
}

/// 右键菜单里的刷新:整表重填,耗时不可预估,故不弹进度、只在结束后报结果。
/// [concurrently] 走 `REFRESH ... CONCURRENTLY`(不锁读,但需要覆盖全部列的唯一索引)。
Future<void> _refreshMatView(
  BuildContext context,
  AppState app,
  ConnectionInfo conn,
  String database,
  String name,
  String? schema, {
  bool concurrently = false,
}) async {
  final l = context.l10n;
  final title = concurrently
      ? l.actionRefreshConcurrently(l.catMaterializedView)
      : l.actionRefresh(l.catMaterializedView);
  if (concurrently) {
    // 条件不满足时先问一句:并发刷新会直接报错,而报错信息是一句目录术语,
    // 不如这里说清前提。探测读不出就放行,交给服务端判定。
    bool? ready;
    try {
      final r = await app.connectionManager.runQuery(
        conn,
        MatViewSql.concurrentlyReadySql(name, schema: schema),
        database: database,
        limit: 1,
      );
      ready = MatViewSql.parseExists(r.rows);
    } catch (_) {
      ready = null;
    }
    if (ready == false) {
      if (!context.mounted) return;
      final go = await MessageBox.show(
        context,
        title: title,
        message: '${l.matvConcurrentlyNeedUnique}\n${l.matvConcurrentlyAsk}',
        type: MessageBoxType.warning,
        buttons: MessageBoxButtons.okCancel,
        okText: l.btnOk,
        cancelText: l.btnCancel,
      );
      if (go != MessageBoxResult.ok) return;
    }
  }
  final sw = Stopwatch()..start();
  // 走 runDdl:成功后顺带刷新对象列表,估算行数才会跟着变
  final outcome = await app.runDdl(
      MatViewSql.refreshSql(name, schema: schema, concurrently: concurrently),
      connection: conn.name,
      database: database,
      schema: schema);
  if (!context.mounted) return;
  if (!outcome.ok) {
    MessageBox.show(
      context,
      title: title,
      message: l.matvRefreshFailed('${outcome.error}'),
      type: MessageBoxType.error,
      okText: l.btnGotIt,
    );
    return;
  }
  MessageBox.show(
    context,
    title: title,
    message: l.matvRefreshed(name, '${sw.elapsedMilliseconds}'),
    type: MessageBoxType.info,
    okText: l.btnGotIt,
  );
}

/// 该引擎能否读出视图的 CREATE 定义。
/// 与 [DatabaseDriver.getDefinition] 的实现对齐:目前只有 Access 恒返回 null。
bool _supportsReadableView(String typeId) => typeId != 'access';

/// MySQL / MariaDB 的 `SHOW CREATE VIEW` 输出不含 `SQL SECURITY` 子句
/// (该属性存在 `information_schema.VIEWS.SECURITY_TYPE`,不在 CREATE 文本里),
/// 而设计页只能从 CREATE 文本还原;保存时拼出的 `CREATE OR REPLACE VIEW` 会
/// 让视图回落到引擎默认的 DEFINER 语义。这属于「保存一次就悄悄改语义」的静默
/// 副作用,宁可不给这个入口。MariaDB 同源,一并适用。
bool _mysqlLosesSqlSecurity(String typeId) =>
    typeId == 'mysql' || typeId == 'mariadb';

/// 「转储SQL」:选择保存位置,把视图 / 实体化视图的 CREATE 定义落盘
Future<void> _dumpRoutine(
  BuildContext context,
  AppState app,
  ObjectCategory category,
  ConnectionInfo conn,
  String database,
  String name,
  String? schema,
) async {
  final l = context.l10n;
  final location = await getSaveLocation(
    acceptedTypeGroups: [
      XTypeGroup(label: l.sqlFileTypeLabel, extensions: ['sql'])
    ],
    suggestedName: '${name}_structure.sql',
    confirmButtonText: l.btnSave,
  );
  if (location == null || !context.mounted) return;

  final sql = await app.dumpViewSql(conn, database, name,
      category: category, schema: schema);
  if (sql == null || !context.mounted) {
    MessageBox.show(
      context,
      title: l.ctxDumpSql,
      message: l.dumpStructureReadFailed,
      type: MessageBoxType.error,
      okText: l.btnGotIt,
    );
    return;
  }

  try {
    final file = File(location.path);
    await file.writeAsString(sql);
  } catch (e) {
    if (!context.mounted) return;
    MessageBox.show(
      context,
      title: l.ctxDumpSql,
      message: l.dumpWriteFailed('$e'),
      type: MessageBoxType.error,
      okText: l.btnGotIt,
    );
    return;
  }
  if (!context.mounted) return;
  MessageBox.show(
    context,
    title: l.ctxDumpSql,
    // 「已导出「x」结构」的一句通吃库 / 模式 / 对象三种转储
    message: l.dumpDatabaseDone(name, location.path),
    type: MessageBoxType.info,
    okText: l.btnGotIt,
  );
}

/// 例程(函数 / 过程)实例右键菜单:设计 / 删除。
/// 连接树与对象面板共用,保证两处交互一致。
void showRoutineContextMenu({
  required BuildContext context,
  required AppState app,
  required ObjectCategory category,
  required ConnectionInfo conn,
  required String database,
  required String name,
  String? schema,
  required Offset position,
}) {
  final l = context.l10n;
  final label = category.labelOf(l);
  final isOpen = app.connectionManager.isConnected(conn.name);
  showContextMenu(
    context,
    position: position,
    items: [
      MenuItem(
        text: l.actionDesign(label),
        enabled: isOpen,
        onPressed: () => app.designRoutine(
          name,
          connection: conn.name,
          database: database,
          category: category,
          schema: schema,
        ),
      ),
      MenuItem(
        text: l.actionDelete(label),
        enabled: isOpen,
        onPressed: () =>
            _deleteRoutine(context, app, category, conn, database, name, schema),
      ),
    ],
  );
}

/// 「删除例程」:确认后 DROP 并刷新对象列表
Future<void> _deleteRoutine(
  BuildContext context,
  AppState app,
  ObjectCategory category,
  ConnectionInfo conn,
  String database,
  String name,
  String? schema,
) async {
  final l = context.l10n;
  final label = category.labelOf(l);
  final title = l.actionDelete(label);
  final result = await MessageBox.show(
    context,
    title: title,
    message: l.deleteObjectConfirmOne(label, name),
    type: MessageBoxType.warning,
    buttons: MessageBoxButtons.okCancel,
    okText: l.btnDelete,
  );
  if (result != MessageBoxResult.ok || !context.mounted) return;
  final failed = await app.dropObjects(
    category,
    [name],
    connection: conn.name,
    database: database,
    schema: schema,
  );
  if (!context.mounted) return;
  if (failed.isNotEmpty) {
    MessageBox.show(
      context,
      title: title,
      message: l.deleteFailedNames(failed.join(', ')),
      type: MessageBoxType.error,
      okText: l.btnGotIt,
    );
  }
}

/// 角色(用户)实例右键菜单:设计 / 删除。
/// 连接树与对象面板共用,保证两处交互一致。
///
/// [name] 是 [listUsers] 返回的对象标识:MySQL 系为 `user@host`,
/// 由 [UserAccount.parse] 拆成两段再送进 DDL(用户名本身可能含 `@`)。
void showUserContextMenu({
  required BuildContext context,
  required AppState app,
  required ConnectionInfo conn,
  required String database,
  required String name,
  String? schema,
  required Offset position,
}) {
  final l = context.l10n;
  final label = l.catRole;
  final isOpen = app.connectionManager.isConnected(conn.name);
  showContextMenu(
    context,
    position: position,
    items: [
      // 与双击行为一致:打开设计页(未连接时页面仍可打开,只是读不到详情)
      MenuItem(
        text: l.actionDesign(label),
        onPressed: () => app.designUser(
          name,
          connection: conn.name,
          database: database,
          schema: schema,
        ),
      ),
      MenuItem(
        text: l.actionDelete(label),
        enabled: isOpen,
        onPressed: () => _deleteUser(context, app, conn, database, name, schema),
      ),
    ],
  );
}

/// 「删除角色」:确认后 DROP USER / DROP ROLE 并刷新对象列表
Future<void> _deleteUser(
  BuildContext context,
  AppState app,
  ConnectionInfo conn,
  String database,
  String name,
  String? schema,
) async {
  final l = context.l10n;
  final title = l.actionDelete(l.catRole);
  final account = UserAccount.parse(name);
  final result = await MessageBox.show(
    context,
    title: title,
    message: l.userDropConfirm(account.displayName),
    type: MessageBoxType.warning,
    buttons: MessageBoxButtons.okCancel,
    okText: l.btnDelete,
  );
  if (result != MessageBoxResult.ok || !context.mounted) return;
  final outcome = await app.dropUser(
    account,
    connection: conn.name,
    database: database,
    schema: schema,
  );
  if (!context.mounted) return;
  if (!outcome.ok) {
    MessageBox.show(
      context,
      title: title,
      message: l.deleteFailedDetail('${outcome.error}'),
      type: MessageBoxType.error,
      okText: l.btnGotIt,
    );
  }
}
