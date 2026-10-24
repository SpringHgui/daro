// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get langFollowSystem => 'Follow System';

  @override
  String get optionsTitle => 'Options';

  @override
  String get optionsGeneral => 'General';

  @override
  String get optionsLanguage => 'Language';

  @override
  String get optionsLanguageHint =>
      'Takes effect immediately; open views switch with it.';

  @override
  String get themeMenuTitle => 'Theme';

  @override
  String get themeFollowSystem => 'Follow System';

  @override
  String get themeLight => 'Light';

  @override
  String get themeDark => 'Dark';

  @override
  String themeSwitchTooltip(String mode) {
    return 'Theme: $mode (click to switch)';
  }

  @override
  String get menuFile => 'File';

  @override
  String get menuNewConnection => 'New Connection...';

  @override
  String get menuNewQuery => 'New Query';

  @override
  String get menuImportConnections => 'Import Connections';

  @override
  String get menuExportConnections => 'Export Connections';

  @override
  String get menuExit => 'Exit';

  @override
  String get menuView => 'View';

  @override
  String get menuRefresh => 'Refresh';

  @override
  String get menuThemeCustomize => 'Customize Theme...';

  @override
  String get menuLargeIcons => 'Large Icons';

  @override
  String get menuSmallIcons => 'Small Icons';

  @override
  String get menuList => 'List';

  @override
  String get menuDetails => 'Details';

  @override
  String get menuTools => 'Tools';

  @override
  String get menuCommandLine => 'Command Line Interface...';

  @override
  String get menuDataTransfer => 'Data Transfer...';

  @override
  String get menuDataSync => 'Data Sync...';

  @override
  String get menuSchemaSync => 'Schema Sync...';

  @override
  String get menuBackup => 'Backup...';

  @override
  String get menuRestoreBackup => 'Restore Backup...';

  @override
  String get menuMcpService => 'MCP Server...';

  @override
  String get menuOptions => 'Options...';

  @override
  String get menuHelp => 'Help';

  @override
  String get menuIssueTracker => 'Report an Issue';

  @override
  String get menuAbout => 'About...';

  @override
  String get catTable => 'Table';

  @override
  String get catView => 'View';

  @override
  String get catMaterializedView => 'Materialized View';

  @override
  String get catFunction => 'Function';

  @override
  String get catProcedure => 'Procedure';

  @override
  String get catRole => 'Roles';

  @override
  String get catQuery => 'Query';

  @override
  String get catBackup => 'Backup';

  @override
  String get ctxNewRole => 'New Role';

  @override
  String get userTabGeneral => 'General';

  @override
  String get userTabAdvanced => 'Advanced';

  @override
  String get userTabMemberOf => 'Member Of';

  @override
  String get userTabMembers => 'Members';

  @override
  String get userTabServerPrivileges => 'Server Privileges';

  @override
  String get userTabPrivileges => 'Privileges';

  @override
  String get userTabSqlPreview => 'SQL Preview';

  @override
  String get userFieldUsername => 'User name:';

  @override
  String get userFieldHost => 'Host:';

  @override
  String get userFieldPlugin => 'Plugin:';

  @override
  String get userFieldPassword => 'Password:';

  @override
  String get userFieldPasswordConfirm => 'Confirm password:';

  @override
  String get userFieldExpirePolicy => 'Password expire policy:';

  @override
  String get userFieldExpireDays => 'Expire days:';

  @override
  String get userFieldComment => 'Comment:';

  @override
  String get userFieldPrincipalType => 'Principal type:';

  @override
  String get userFieldConnectionLimit => 'Connection limit:';

  @override
  String get userFieldValidUntil => 'Valid until:';

  @override
  String get userFieldNewPassword => 'New password:';

  @override
  String get userExpireDefault => 'DEFAULT';

  @override
  String get userExpireExpired => 'Expire now';

  @override
  String get userExpireNever => 'Never expire';

  @override
  String get userExpireInterval => 'Expire in days';

  @override
  String get userPgLogin => 'Can login (LOGIN)';

  @override
  String get userPgSuper => 'Superuser (SUPERUSER)';

  @override
  String get userPgCreateDb => 'Create databases (CREATEDB)';

  @override
  String get userPgCreateRole => 'Create roles (CREATEROLE)';

  @override
  String get userPgInherit => 'Inherit privileges (INHERIT)';

  @override
  String get userPgReplication => 'Replication (REPLICATION)';

  @override
  String get userPgBypassRls => 'Bypass row-level security (BYPASSRLS)';

  @override
  String get userPrincipalSql => 'SQL login';

  @override
  String get userPrincipalWindows => 'Windows user';

  @override
  String get userPrincipalWindowsGroup => 'Windows group';

  @override
  String get userPrincipalRole => 'Database role';

  @override
  String get userIsRole => 'This is a role, not a login-capable user';

  @override
  String get userColTarget => 'Object';

  @override
  String get userColPrivilege => 'Privilege';

  @override
  String get userColGrant => 'Grantable';

  @override
  String get userColRole => 'Role';

  @override
  String get userColMember => 'Member';

  @override
  String get userNoPrivileges => 'No privileges granted.';

  @override
  String get userNoMembers => 'No members.';

  @override
  String get userNoMemberOf => 'Not a member of any role.';

  @override
  String get userNoCandidates => 'No roles available.';

  @override
  String get userAddPrivilege => 'Add privilege';

  @override
  String get userRemovePrivilege => 'Remove';

  @override
  String get userPrivilegeTargetHint => 'database.table (empty = server level)';

  @override
  String userSaveOk(String name) {
    return 'Saved role $name';
  }

  @override
  String userSaveFailedAt(String index) {
    return 'Statement #$index failed:';
  }

  @override
  String userSavedButRefreshFailed(String error) {
    return 'Saved, but refreshing the object list failed: $error';
  }

  @override
  String get userNameRequired => 'Please enter a user name.';

  @override
  String get userPasswordMismatch => 'The two passwords do not match.';

  @override
  String get userPasswordRequired =>
      'Please enter a password (a new account needs one).';

  @override
  String userLoadFailed(String name, String error) {
    return 'Failed to load \"$name\": $error';
  }

  @override
  String get userNotSupported =>
      'This database type does not support account management.';

  @override
  String get userAdvancedHint =>
      'Options on this page rewrite the account DDL; availability differs per database type.';

  @override
  String get userPrivilegeHint =>
      'Checked privileges are granted on save; unchecking revokes all privileges on that object first.';

  @override
  String userDropConfirm(String name) {
    return 'Delete role \"$name\"?\nThis cannot be undone.';
  }

  @override
  String get userMembershipHint =>
      '\"Member Of\" lists the roles this account belongs to; \"Members\" lists the accounts that belong to this role.';

  @override
  String get userRoleOnlyHint =>
      'Role membership is available on MySQL 8 and later only.';

  @override
  String get userLoading => 'Reading account information ...';

  @override
  String get userRefresh => 'Refresh';

  @override
  String get userPrivilegeDatabase => 'Database';

  @override
  String get userPrivilegeTable => 'Table';

  @override
  String get userPrivilegeNames => 'Privileges (comma separated)';

  @override
  String get userSelectedRoles => 'Member of';

  @override
  String get userCandidateRoles => 'Available roles';

  @override
  String get importDoneTitle => 'Import complete';

  @override
  String importDoneMessage(String count) {
    return 'Imported $count connections. See them in the connection tree on the left.';
  }

  @override
  String importDoneManualPassword(String count) {
    return '$count of them carried no password (Navicat had none saved, or used an older encryption). Right-click such a connection and choose Edit Connection to fill it in.';
  }

  @override
  String importDoneNewGroups(String count, String names) {
    return '$count groups did not exist locally and were created: $names.';
  }

  @override
  String get exportDoneTitle => 'Export complete';

  @override
  String exportDoneMessage(String count, String path) {
    return 'Exported $count connections to\n$path\nIn Navicat, use File | Import Connection Settings and pick this file.';
  }

  @override
  String exportDoneSkipped(String names) {
    return 'Not exported, because Navicat has no matching connection type: $names.';
  }

  @override
  String listEllipsisMore(String count) {
    return ' and $count more';
  }

  @override
  String get listEllipsis => ' etc.';

  @override
  String get listSeparator => ', ';

  @override
  String get issueTrackerTitle => 'Report an issue';

  @override
  String issueTrackerOpenFailed(String url) {
    return 'Could not open your browser automatically. Please visit:\n$url';
  }

  @override
  String get ribbonConnection => 'Connection';

  @override
  String get ribbonNewQuery => 'New Query';

  @override
  String get tabObjects => 'Objects';

  @override
  String get tabDesignSuffix => ' (Design)';

  @override
  String get tabNewSuffix => ' (New)';

  @override
  String get tabCommandLine => 'Command Line';

  @override
  String get tabCtxClose => 'Close';

  @override
  String get tabCtxCloseOthers => 'Close Other Tabs';

  @override
  String get tabCtxCloseRight => 'Close Tabs to the Right';

  @override
  String get tabCtxCloseAll => 'Close All Tabs';

  @override
  String get statusNoDatabase => 'No database selected';

  @override
  String statusRecordPosition(String current, String total, String page) {
    return 'Record $current of $total, page $page';
  }

  @override
  String statusSelectedRows(String count, String total, String page) {
    return '$count of $total rows selected, page $page';
  }

  @override
  String statusObjectsSelected(String count) {
    return '$count selected';
  }

  @override
  String get tipDetailedLayout => 'Detailed layout';

  @override
  String get tipListLayout => 'List';

  @override
  String get tipLeftPanel => 'Left panel';

  @override
  String get tipRightPanel => 'Right panel';

  @override
  String sqlHistoryTitle(String count) {
    return 'SQL history ($count)';
  }

  @override
  String get sqlHistoryLatest => 'Latest';

  @override
  String get mcpTipDisabled =>
      'MCP server is disabled (click to open settings)';

  @override
  String get mcpTipCorrupted =>
      'Failed to load the MCP policy (click for details)';

  @override
  String get mcpTipIdle =>
      'MCP server is enabled but not listening (click to open settings)';

  @override
  String get mcpTipRunning => 'MCP server is running (click to open settings)';

  @override
  String mcpTipRunningCalls(String count) {
    return 'MCP server is running ($count calls)';
  }

  @override
  String get infoPickNode =>
      'Select a node in the connection tree to see its details';

  @override
  String get infoSectionDatabase => 'Database';

  @override
  String get infoSectionConnection => 'Connection';

  @override
  String get infoSectionSchema => 'Schema';

  @override
  String get infoSectionConnGroup => 'Connection group';

  @override
  String get infoSectionKvNamespace => 'Cache namespace';

  @override
  String get infoSectionTable => 'Table';

  @override
  String get fieldConnection => 'Connection';

  @override
  String get fieldType => 'Type';

  @override
  String get fieldKeyCount => 'Key count';

  @override
  String get fieldHost => 'Host';

  @override
  String get fieldUser => 'User';

  @override
  String get fieldDatabase => 'Database';

  @override
  String get fieldSchema => 'Schema';

  @override
  String get fieldConnCount => 'Connections';

  @override
  String get infoGroupEmpty =>
      'The group is empty. Right-click a connection and choose Move to Group, or delete this group.';

  @override
  String get infoTableDoubleClickHint =>
      'Double-click a table to browse its first 100 rows';

  @override
  String get fieldCharset => 'Character Set';

  @override
  String get fieldCollation => 'Collation';

  @override
  String get fieldRows => 'Rows';

  @override
  String get fieldEngine => 'Engine';

  @override
  String get fieldAutoIncrement => 'Auto Increment';

  @override
  String get fieldRowFormat => 'Row Format';

  @override
  String get fieldCreateTime => 'Created';

  @override
  String get fieldUpdateTime => 'Last Modified';

  @override
  String get fieldCheckTime => 'Last Checked';

  @override
  String get fieldDataLength => 'Data Length';

  @override
  String get fieldIndexLength => 'Index Length';

  @override
  String get fieldMaxDataLength => 'Max Data Length';

  @override
  String get fieldDataFree => 'Data Free';

  @override
  String get fieldCreateOptions => 'Create Options';

  @override
  String get fieldComment => 'Comment';

  @override
  String infoRowCountEstimate(String count) {
    return '$count (Est.)';
  }

  @override
  String get infoFetchRowCount => 'Get row count';

  @override
  String get infoFetchingRowCount => 'Counting...';

  @override
  String infoRowCountFailed(String error) {
    return 'Count failed: $error';
  }

  @override
  String get infoPageInfo => 'Information';

  @override
  String get infoPageDdl => 'DDL';

  @override
  String get fieldOid => 'OID';

  @override
  String get fieldOwner => 'Owner';

  @override
  String get fieldTablespace => 'Tablespace';

  @override
  String get fieldEncoding => 'Encoding';

  @override
  String get fieldLcCollate => 'LC_COLLATE';

  @override
  String get fieldConnectionLimit => 'Connection Limit';

  @override
  String get infoValueNoLimit => 'Unlimited';

  @override
  String get fieldTableType => 'Table Type';

  @override
  String get tableTypeRegular => 'Regular';

  @override
  String get tableTypePartitioned => 'Partitioned table';

  @override
  String get tableTypeView => 'View';

  @override
  String get tableTypeMatView => 'Materialized view';

  @override
  String get tableTypeForeign => 'Foreign table';

  @override
  String get fieldPartitionOf => 'Partition Of';

  @override
  String get fieldInheritsFrom => 'Inherits From';

  @override
  String get fieldHasOids => 'Has OIDs';

  @override
  String get fieldFillFactor => 'Fill Factor';

  @override
  String get fieldAcl => 'ACL';

  @override
  String get infoValueYes => 'Yes';

  @override
  String get infoValueNo => 'No';

  @override
  String get infoPageUses => 'Uses';

  @override
  String get infoPageUsedBy => 'Used By';

  @override
  String get infoPageUsesTooltip => 'Objects this table depends on';

  @override
  String get infoPageUsedByTooltip => 'Objects referencing this table';

  @override
  String get infoDepsLoading => 'Reading dependencies...';

  @override
  String get infoDepsEmpty => 'No dependent objects';

  @override
  String infoDepsFailed(String error) {
    return 'Failed to read dependencies: $error';
  }

  @override
  String get infoMaximizePanel => 'Widen the details panel';

  @override
  String get infoRestorePanel => 'Restore panel width';

  @override
  String infoDetailFailed(String error) {
    return 'Could not read details: $error';
  }

  @override
  String get infoDdlLoading => 'Reading definition...';

  @override
  String get infoDdlUnsupported =>
      'This database type cannot show its CREATE statement.';

  @override
  String infoDdlFailed(String error) {
    return 'Could not read DDL: $error';
  }

  @override
  String get infoCopyDdl => 'Copy CREATE statement';

  @override
  String actionOpen(String label) {
    return 'Open $label';
  }

  @override
  String actionDesign(String label) {
    return 'Design $label';
  }

  @override
  String actionNew(String label) {
    return 'New $label';
  }

  @override
  String actionDelete(String label) {
    return 'Delete $label';
  }

  @override
  String get tableKindRegular => 'Regular';

  @override
  String get tableKindExternal => 'External';

  @override
  String get tableKindPartition => 'Partition';

  @override
  String get importWizard => 'Import Wizard';

  @override
  String get exportWizard => 'Export Wizard';

  @override
  String actionClear(String label) {
    return 'Truncate $label';
  }

  @override
  String clearConfirmOne(String label, String name) {
    return 'Truncate $label \"$name\"?\nThis deletes all rows but keeps the structure, and cannot be undone.';
  }

  @override
  String clearFailedDetail(String error) {
    return 'Could not truncate:\n$error';
  }

  @override
  String get btnClear => 'Truncate';

  @override
  String get ctxCopyRename => 'Copy / Rename';

  @override
  String get btnGotIt => 'OK';

  @override
  String get btnDelete => 'Delete';

  @override
  String get btnPaste => 'Paste';

  @override
  String get btnRetry => 'Retry';

  @override
  String stubWip(String action) {
    return '$action is not implemented yet';
  }

  @override
  String get deleteQueryTitle => 'Delete Query';

  @override
  String deleteQueryConfirmOne(String name) {
    return 'Delete the query \"$name\"?\nYou can save it again later; open query tabs are not affected.';
  }

  @override
  String deleteQueryConfirmMany(String count) {
    return 'Delete the $count selected queries?';
  }

  @override
  String deleteObjectConfirmOne(String label, String name) {
    return 'Delete $label \"$name\"?\nThis permanently removes the object and cannot be undone.';
  }

  @override
  String deleteObjectConfirmMany(String count, String label) {
    return 'Delete the $count selected $label?\nThis permanently removes them and cannot be undone.';
  }

  @override
  String deleteFailedNames(String names) {
    return 'Could not delete: $names\nCheck the connection, or whether the objects still exist.';
  }

  @override
  String loadingObjectsTitle(String database) {
    return 'Loading objects of $database ...';
  }

  @override
  String openDatabaseFailedTitle(String database) {
    return 'Could not open $database';
  }

  @override
  String categoryListFailedTitle(String label) {
    return 'Could not read the $label list';
  }

  @override
  String get colName => 'Name';

  @override
  String get colRowsEstimated => 'Rows (est.)';

  @override
  String get colComment => 'Comment';

  @override
  String rowsApprox(String value, String unit) {
    return '~$value$unit';
  }

  @override
  String get rowsUnitSmall => 'K';

  @override
  String get rowsUnitLarge => 'M';

  @override
  String get renameTableTitle => 'Rename Table';

  @override
  String renameFailedDetail(String error) {
    return 'Rename failed:\n$error';
  }

  @override
  String copiedTables(String count) {
    return 'Copied $count table(s) - press Ctrl+V to paste copies';
  }

  @override
  String pasteNeedOpenConnection(String connection) {
    return 'Open the connection \"$connection\" before pasting.';
  }

  @override
  String pasteWrongContext(String context) {
    return 'Tables can only be pasted into the connection / database / schema they were copied from:\n$context';
  }

  @override
  String get pasteTableTitle => 'Paste Tables';

  @override
  String pasteConfirmDetail(String count, String plan) {
    return 'Create $count table(s) (structure + data):\n$plan';
  }

  @override
  String pastedTables(String count) {
    return 'Pasted $count table(s)';
  }

  @override
  String pasteFailedDetail(String detail) {
    return 'Paste failed:\n$detail';
  }

  @override
  String get catTablePlural => 'Tables';

  @override
  String get catViewPlural => 'Views';

  @override
  String get catMaterializedViewPlural => 'Materialized Views';

  @override
  String get catFunctionPlural => 'Functions';

  @override
  String get catProcedurePlural => 'Procedures';

  @override
  String get newExternalTable => 'New External Table';

  @override
  String get newPartitionTable => 'New Partitioned Table';

  @override
  String openedConnection(String name) {
    return 'Opened connection \"$name\"';
  }

  @override
  String openedDatabase(String name) {
    return 'Opened database \"$name\"';
  }

  @override
  String openedSchema(String name) {
    return 'Opened schema \"$name\"';
  }

  @override
  String get openedBare => 'Opened';

  @override
  String get closedBare => 'Closed';

  @override
  String closedConnection(String name) {
    return 'Closed connection \"$name\"';
  }

  @override
  String closedSchema(String name) {
    return 'Closed schema \"$name\"';
  }

  @override
  String closedDatabase(String name) {
    return 'Closed database \"$name\"';
  }

  @override
  String openedNamed(String name) {
    return 'Opened \"$name\"';
  }

  @override
  String closedNamed(String name) {
    return 'Closed \"$name\"';
  }

  @override
  String renamedConnection(String newName, String oldName) {
    return 'Renamed connection \"$oldName\" to \"$newName\"';
  }

  @override
  String renamedTable(String newName, String oldName) {
    return 'Renamed table \"$oldName\" to \"$newName\"';
  }

  @override
  String movedConnectionToUngrouped(String name) {
    return 'Moved connection \"$name\" out of any group';
  }

  @override
  String movedConnectionToGroup(String group, String name) {
    return 'Moved connection \"$name\" into group \"$group\"';
  }

  @override
  String get unnamedGroup => 'Unnamed Group';

  @override
  String unnamedGroupNumbered(String index) {
    return 'Unnamed Group $index';
  }

  @override
  String get renameGroupTitle => 'Rename Group';

  @override
  String groupAlreadyExists(String name) {
    return 'A group named \"$name\" already exists (names are case-insensitive).';
  }

  @override
  String get ctxOpenConnection => 'Open Connection';

  @override
  String get ctxCloseConnection => 'Close Connection';

  @override
  String get ctxOpen => 'Open';

  @override
  String get ctxClose => 'Close';

  @override
  String get ctxRefresh => 'Refresh';

  @override
  String get ctxNewDatabase => 'New Database';

  @override
  String get ctxEditConnection => 'Edit Connection';

  @override
  String get ctxCopyConnection => 'Copy Connection';

  @override
  String get ctxMoveToGroup => 'Move to Group';

  @override
  String get ctxUngrouped => 'Ungrouped';

  @override
  String get ctxNewGroup => 'New Group';

  @override
  String get ctxDeleteConnection => 'Delete Connection';

  @override
  String get ctxNewConnectionEllipsis => 'New Connection…';

  @override
  String get ctxRenameGroup => 'Rename Group';

  @override
  String get ctxDeleteGroup => 'Delete Group';

  @override
  String ctxDeleteGroupWith(String count) {
    return 'Delete Group ($count connections)';
  }

  @override
  String deleteGroupConfirm(String count, String group) {
    return 'Deleting the group \"$group\" keeps its $count connection(s); they move back to Ungrouped. Continue?';
  }

  @override
  String get btnCancel => 'Cancel';

  @override
  String get btnSave => 'Save';

  @override
  String get ctxNewSchema => 'New Schema';

  @override
  String get ctxDelete => 'Delete';

  @override
  String get ctxEditDatabase => 'Edit Database';

  @override
  String get ctxNewQuery => 'New Query';

  @override
  String get ctxDumpSql => 'Dump SQL File';

  @override
  String get ctxStructureOnly => 'Structure Only';

  @override
  String get ctxRunSql => 'Run SQL File';

  @override
  String get ctxCloseSchema => 'Close Schema';

  @override
  String get ctxOpenSchema => 'Open Schema';

  @override
  String get ctxEditSchema => 'Edit Schema';

  @override
  String get ctxDeleteSchema => 'Delete Schema';

  @override
  String deleteSchemaConfirm(String name) {
    return 'Delete schema \"$name\"?\nThis permanently removes the schema and every object in it, and cannot be undone.';
  }

  @override
  String deleteFailedDetail(String error) {
    return 'Delete failed:\n$error';
  }

  @override
  String get ctxNewTable => 'New Table';

  @override
  String get ctxNewView => 'New View';

  @override
  String get ctxNewFunction => 'New Function';

  @override
  String get ctxNewProcedure => 'New Procedure';

  @override
  String get deleteDatabaseTitle => 'Delete Database';

  @override
  String deleteDatabaseConfirm(String name) {
    return 'Delete database \"$name\"?\nThis permanently removes the database and all of its data, and cannot be undone.';
  }

  @override
  String get sqlFileTypeLabel => 'SQL files';

  @override
  String get dumpStructureReadFailed =>
      'Could not read the structure:\nThe database is unavailable or the connection is closed. Open the connection and try again.';

  @override
  String dumpWriteFailed(String error) {
    return 'Could not write the file:\n$error';
  }

  @override
  String dumpDatabaseDone(String name, String path) {
    return 'Exported the structure of \"$name\" (structure only, no data) to:\n$path';
  }

  @override
  String dumpSchemaDone(String name, String path) {
    return 'Exported the structure of schema \"$name\" (structure only, no data) to:\n$path';
  }

  @override
  String openConnectionUnsupported(String name, String type) {
    return 'The database type ($type) of connection \"$name\" is not supported yet, so it cannot be opened.';
  }

  @override
  String openConnectionFailed(String error, String name) {
    return 'Could not connect to \"$name\":\n$error';
  }

  @override
  String deleteConnectionConfirm(String name) {
    return 'Delete connection \"$name\"?\nTabs opened from it stay, but can no longer be used.';
  }

  @override
  String get noMatchingConnections => 'No matching connections';

  @override
  String get noConnectionsYet => 'No connections yet';

  @override
  String get clickToolbarNewConnection =>
      'Use the Connection button on the toolbar to add one';

  @override
  String get dropToUngroup => 'Release to move to Ungrouped';

  @override
  String get driverNotImplemented => 'This type has no driver yet';

  @override
  String loadFailedDetail(String error) {
    return 'Could not load: $error';
  }

  @override
  String get readFailed => 'Could not read';

  @override
  String get clickToRetry => 'Click to retry';

  @override
  String get searchConnectionsHint => 'Search connections...';

  @override
  String get dbTypeFilter => 'Filter by database type';

  @override
  String get notImplemented => 'Not implemented';

  @override
  String get clearAllFilters => 'Clear all';

  @override
  String get collapseAll => 'Collapse all';

  @override
  String get cellEditorPickCell => 'Select a cell first';

  @override
  String get btnCommitChanges => 'Commit Changes';

  @override
  String rowsCopiedToClipboard(String count) {
    return 'Copied $count row(s) to clipboard';
  }

  @override
  String cellMenuSetNull(String count) {
    return 'Set $count cell(s) to NULL';
  }

  @override
  String cellMenuCopy(String count) {
    return 'Copy $count cell(s)';
  }

  @override
  String cellsCopiedToClipboard(String count) {
    return 'Copied $count cell(s) to clipboard';
  }

  @override
  String cellsClearedToNull(String count, String action) {
    return 'Set $count cell(s) to NULL — $action or Ctrl+S to save';
  }

  @override
  String get filterSourceBuilder => 'Builder';

  @override
  String get filterSourceText => 'Text';

  @override
  String get toolPanelFilter => 'Filter & Sort';

  @override
  String get toolPanelColumns => 'Columns';

  @override
  String get toolPanelCellEditor => 'Cell Editor';

  @override
  String get btnOk => 'OK';

  @override
  String get dtpSelectTime => 'Select Time';

  @override
  String get dtpToday => 'Today';

  @override
  String get dtpMonth1 => 'Jan';

  @override
  String get dtpMonth2 => 'Feb';

  @override
  String get dtpMonth3 => 'Mar';

  @override
  String get dtpMonth4 => 'Apr';

  @override
  String get dtpMonth5 => 'May';

  @override
  String get dtpMonth6 => 'Jun';

  @override
  String get dtpMonth7 => 'Jul';

  @override
  String get dtpMonth8 => 'Aug';

  @override
  String get dtpMonth9 => 'Sep';

  @override
  String get dtpMonth10 => 'Oct';

  @override
  String get dtpMonth11 => 'Nov';

  @override
  String get dtpMonth12 => 'Dec';

  @override
  String get dtpMonthTitle => '[monthName] [year]';

  @override
  String get dtpYearTitle => '[year]';

  @override
  String get dtpYearRangeTitle => '[from] - [to]';

  @override
  String get dtpWeekdayMon => 'Mon';

  @override
  String get dtpWeekdayTue => 'Tue';

  @override
  String get dtpWeekdayWed => 'Wed';

  @override
  String get dtpWeekdayThu => 'Thu';

  @override
  String get dtpWeekdayFri => 'Fri';

  @override
  String get dtpWeekdaySat => 'Sat';

  @override
  String get dtpWeekdaySun => 'Sun';

  @override
  String get catRecord => 'record';

  @override
  String gridPagingFailed(String error) {
    return 'Failed to load page: $error';
  }

  @override
  String get gridAlreadyLastPage => 'Already on the last page';

  @override
  String gridPageMissing(String page) {
    return 'Page $page does not exist';
  }

  @override
  String get gridDiscardTitle => 'Discard Unsaved Changes';

  @override
  String get gridDiscardConfirm =>
      'You have unsaved changes. Continuing will discard them.\nContinue?';

  @override
  String gridDeleteRowConfirm(String row) {
    return 'Delete row $row?\n';
  }

  @override
  String gridDeleteRowsConfirm(String count, String preview) {
    return 'Delete the $count selected rows? ($preview)\n';
  }

  @override
  String get gridDeletePendingHint =>
      'Nothing is written until you click Commit Changes or press Ctrl+S.';

  @override
  String get gridNothingToSave => 'Nothing to save';

  @override
  String gridConnectionMissing(String connection) {
    return 'Connection \"$connection\" no longer exists';
  }

  @override
  String gridDeleteRowError(String row, String error) {
    return 'Delete row $row: $error';
  }

  @override
  String gridInsertRowError(String error) {
    return 'Insert row: $error';
  }

  @override
  String gridUpdateRowError(String row, String error) {
    return 'Update row $row: $error';
  }

  @override
  String gridSavedRows(String count) {
    return 'Saved $count row(s)';
  }

  @override
  String gridSaveFailed(String errors) {
    return 'Save failed: $errors';
  }

  @override
  String get gridSaveFailedTitle => 'Save Failed';

  @override
  String get gridSortMethod => 'Sort order';

  @override
  String get gridAddSortCriterion => 'Add sort criterion';

  @override
  String get gridSortEmptyHint => 'Click + to add a sort criterion';

  @override
  String get gridReadingColumns => 'Reading column info …';

  @override
  String get gridFilterEmptyHint =>
      'Click + to add a filter. Select a row to append a sibling criterion (+) or a parenthesised group (O+) at its end.';

  @override
  String get gridFilter => 'Filter';

  @override
  String get gridAddFilterCriterion => 'Add filter criterion';

  @override
  String get gridMoveCriterionUp => 'Move selected criterion up';

  @override
  String get gridMoveCriterionDown => 'Move selected criterion down';

  @override
  String get gridNoValueNeeded => '(no value needed)';

  @override
  String get gridAddSibling => 'Add a sibling criterion after this one';

  @override
  String get gridAddGroup => 'Add a parenthesised group after this one';

  @override
  String get gridDeleteFilterGroup => 'Delete group';

  @override
  String get gridDeleteCriterion => 'Delete criterion';

  @override
  String get gridWhereHint =>
      'Without the WHERE keyword, e.g. id > 100 AND name LIKE \'Acme%\'';

  @override
  String get gridApplyFilterSort => 'Apply Filter & Sort';

  @override
  String get gridCriterionEdited => 'Criterion edited';

  @override
  String get gridAsc => 'Ascending';

  @override
  String get gridDesc => 'Descending';

  @override
  String get gridDeleteSortCriterion => 'Delete sort criterion';

  @override
  String gridColumnsCount(String visible, String total) {
    return 'Columns ($visible/$total)';
  }

  @override
  String get gridShowAllColumns => 'Show all columns';

  @override
  String get gridKeepFirstColumnOnly => 'Keep first column only';

  @override
  String get gridLoadingColumns => 'Loading column info ...';

  @override
  String get gridSearch => 'Search';

  @override
  String get gridColumnName => 'Column name';

  @override
  String get cellNoneSelected => 'No cell selected';

  @override
  String cellColumnIndex(String col) {
    return 'Column $col';
  }

  @override
  String cellRowIndex(String row) {
    return '  ·  row $row';
  }

  @override
  String cellEditorTitle(String title) {
    return 'Cell Editor · $title';
  }

  @override
  String get btnApply => 'Apply';

  @override
  String get btnUndo => 'Revert';

  @override
  String get cellEmptyValue => 'Cell is empty';

  @override
  String get cellNotBase64Image =>
      'This cell is not recognised base64 image data';

  @override
  String get cellNotHtml => 'This cell does not contain HTML source';

  @override
  String cellWrittenBack(String action) {
    return 'Written back to the cell — $action or Ctrl+S to save';
  }

  @override
  String get cellMenuSetBlank => 'Set to empty string';

  @override
  String get cellMenuSetNullCell => 'Set to NULL';

  @override
  String get gridSort => 'Sort';

  @override
  String gridSortAscBy(String column) {
    return 'Ascending ($column)';
  }

  @override
  String gridSortDescBy(String column) {
    return 'Descending ($column)';
  }

  @override
  String get gridClearSort => 'Clear sort';

  @override
  String get gridMoreSorting => 'More sorting...';

  @override
  String gridHideColumn(String column) {
    return 'Hide \"$column\"';
  }

  @override
  String get gridColumnsPanel => 'Columns panel...';

  @override
  String get gridMoreFilters => 'More filters...';

  @override
  String get gridClearFilter => 'Clear filter';

  @override
  String get gridRemoveAllSortFilter => 'Remove all sorting and filters';

  @override
  String get gridShow => 'Show';

  @override
  String get gridShowAll => 'All records';

  @override
  String get gridShowNullOnly => 'Records with NULL only';

  @override
  String get gridShowNotNullOnly => 'Records without NULL';

  @override
  String get gridClipboardEmpty => 'Clipboard is empty';

  @override
  String get gridClipboardNoRecords => 'No pasteable records on the clipboard';

  @override
  String gridPastedRows(String count) {
    return 'Pasted $count row(s) as new records (not saved)';
  }

  @override
  String get gridRowCountUnknown => '  ·  row count unknown';

  @override
  String gridRowCount(String total) {
    return '  ·  $total rows';
  }

  @override
  String gridLoadingTable(String table) {
    return 'Loading $table ...';
  }

  @override
  String gridReadTableFailed(String table) {
    return 'Failed to read $table';
  }

  @override
  String get gridFilterDirty => 'Filter / sort has unapplied changes';

  @override
  String get gridFilterApplied => 'Filter / sort applied';

  @override
  String get gridAddRecord => 'Add Record';

  @override
  String get gridDeleteSelectedRecords => 'Delete Selected Records';

  @override
  String get gridSaving => 'Saving...';

  @override
  String get gridRevertChanges => 'Revert';

  @override
  String get gridStop => 'Stop';

  @override
  String get gridReadOnlyBrowse =>
      'This connection is read-only; data cannot be edited or saved in the grid.';

  @override
  String gridPagerRange(String from, String to, String total) {
    return '$from-$to of $total';
  }

  @override
  String get gridPageSize => 'Page Size';

  @override
  String gridRowsPerPage(String size) {
    return '$size rows/page';
  }

  @override
  String gridRowsPerPageCurrent(String size) {
    return '$size rows/page ✓';
  }

  @override
  String gridConnectionGone(String connection) {
    return 'Connection \"$connection\" no longer exists; open it first.';
  }

  @override
  String get gridDeleteRecordTitle => 'Delete Record';

  @override
  String gridDeleteRecordPending(String action) {
    return 'Nothing is written until you click \"$action\" or press Ctrl+S.';
  }

  @override
  String gridDeleteRowDetail(String row, String tail) {
    return 'Delete row $row?\n$tail';
  }

  @override
  String gridDeleteRowsDetail(String count, String preview, String tail) {
    return 'Delete the $count selected rows? ($preview)\n$tail';
  }

  @override
  String gridUpdateRowErrorAt(String row, String error) {
    return 'Update row $row: $error';
  }

  @override
  String get gridSortDirection => 'Sort Direction';

  @override
  String get gridSortAsc => 'Ascending';

  @override
  String get gridSortDesc => 'Descending';

  @override
  String get gridJoinAnd => 'And';

  @override
  String get gridJoinOr => 'Or';

  @override
  String get gridFilterValuePlaceholder => '<?>';

  @override
  String get gridSortTitle => 'Sorting';

  @override
  String get gridAddSortHint => 'Click + to add a sort criterion';

  @override
  String get gridAddSortCriterionTitle => 'Add Sort Criterion';

  @override
  String get gridDeleteSortCriterionTitle => 'Delete Sort Criterion';

  @override
  String get gridFilterTitle => 'Filter';

  @override
  String get gridAddFilterCriterionTitle => 'Add Filter Criterion';

  @override
  String get gridMoveCriterionUpTitle => 'Move Selected Criterion Up';

  @override
  String get gridMoveCriterionDownTitle => 'Move Selected Criterion Down';

  @override
  String get gridFilterTextHint =>
      'Exclude the WHERE keyword, e.g. id > 100 AND name LIKE \'Acme%\'';

  @override
  String get gridApplyFilterSortTitle => 'Apply Filter & Sort';

  @override
  String get gridCriterionEditedTitle => 'Criterion Edited';

  @override
  String get gridColumnsTitle => 'Columns';

  @override
  String get gridColumnsNoInfo => 'Columns';

  @override
  String get gridSearchColumnsHint => 'Search';

  @override
  String get gridLoadingColumnsEllipsis => 'Loading column info ...';

  @override
  String get gridReadingColumnsEllipsis => 'Reading column info …';

  @override
  String get gridCellEditorTabText => 'Text';

  @override
  String get gridCellEditorTabHex => 'Hex';

  @override
  String get gridCellEditorTabImage => 'Image';

  @override
  String get gridCellEditorTabWeb => 'Web';

  @override
  String get gridSelectNone => 'No cell selected';

  @override
  String gridColumnNumber(String col) {
    return 'Column $col';
  }

  @override
  String gridDisplayRow(String row) {
    return '  ·  Row $row';
  }

  @override
  String gridCellEditorHeader(String title) {
    return 'Cell Editor · $title';
  }

  @override
  String get gridCellEditorPanelTitle => 'Cell Editor';

  @override
  String get gridCellCopyOne => 'Copy Cell';

  @override
  String get gridCellCopyAs => 'Copy As';

  @override
  String get gridCellCopyTsv => 'Record (tab-delimited)';

  @override
  String get gridCellCopyCsv => 'Record (CSV)';

  @override
  String get gridCellCopyCsvWithHeader => 'Record + Column Names (CSV)';

  @override
  String get gridCellPasteAppend => 'Paste Rows (Append as New)';

  @override
  String get gridCellPasteToCell => 'Paste into Cell';

  @override
  String get gridCellSaveAs => 'Save Data As...';

  @override
  String get gridCellSetNullTitle => 'Set to NULL';

  @override
  String gridRowsCopy(String count) {
    return 'Copy $count Rows';
  }

  @override
  String get gridRowsCopyOne => 'Copy Row';

  @override
  String gridRowsDelete(String count) {
    return 'Delete $count Records';
  }

  @override
  String get gridRowsDeleteOne => 'Delete Record';

  @override
  String gridCellSortAscBy(String column) {
    return 'Ascending ($column)';
  }

  @override
  String gridCellSortDescBy(String column) {
    return 'Descending ($column)';
  }

  @override
  String gridCellHideColumn(String column) {
    return 'Hide \"$column\"';
  }

  @override
  String get dtpOk => 'OK';

  @override
  String get dtpCancel => 'Cancel';

  @override
  String get btnApplyTitle => 'Apply';

  @override
  String get btnUndoTitle => 'Undo';

  @override
  String get opEquals => 'Equals';

  @override
  String get opNotEquals => 'Not equals';

  @override
  String get opGreaterThan => 'Greater than';

  @override
  String get opGreaterOrEqual => 'Greater or equal';

  @override
  String get opLessThan => 'Less than';

  @override
  String get opLessOrEqual => 'Less or equal';

  @override
  String get opContains => 'Contains';

  @override
  String get opNotContains => 'Not contains';

  @override
  String get opStartsWith => 'Starts with';

  @override
  String get opEndsWith => 'Ends with';

  @override
  String get opIsNull => 'Is empty';

  @override
  String get opIsNotNull => 'Is not empty';

  @override
  String get gridRefresh => 'Refresh';

  @override
  String gridDeleteRowErrorAt(String row, String error) {
    return 'Delete row $row: $error';
  }

  @override
  String get cliEmptyHint =>
      'Type a SQL statement and press Enter. Statements end with a semicolon; keep typing to continue an unfinished one, use ↑ / ↓ for history.';

  @override
  String get cliInputHint => 'Enter a SQL statement';

  @override
  String get ctxNewMatView => 'New Materialized View';

  @override
  String actionRefresh(String label) {
    return 'Refresh $label';
  }

  @override
  String actionRefreshConcurrently(String label) {
    return 'Refresh $label Concurrently';
  }

  @override
  String get matvTabDefinition => 'Definition';

  @override
  String get matvTabIndexes => 'Indexes';

  @override
  String get matvTabAdvanced => 'Advanced';

  @override
  String get matvTabComment => 'Comments';

  @override
  String get matvTabSqlPreview => 'SQL Preview';

  @override
  String get matvPanelMessages => 'Messages';

  @override
  String get matvPreview => 'Preview';

  @override
  String get matvExplain => 'Explain';

  @override
  String get matvBeautifySql => 'Beautify SQL';

  @override
  String get matvTabChange => 'Changes';

  @override
  String get matvCopy => 'Copy';

  @override
  String get matvFullscreen => 'Fullscreen';

  @override
  String get matvExitFullscreen => 'Exit fullscreen';

  @override
  String get matvCommentHint =>
      'Comment for this materialized view (written as COMMENT ON on save)';

  @override
  String get matvAddIndex => 'Add Index';

  @override
  String get matvDropIndex => 'Drop Index';

  @override
  String get matvColDefinition => 'Definition';

  @override
  String get colUnique => 'Unique';

  @override
  String get matvFieldIndexName => 'Index name:';

  @override
  String get matvFieldIndexColumns => 'Columns:';

  @override
  String get matvFieldOwner => 'Owner:';

  @override
  String get matvFieldTablespace => 'Tablespace:';

  @override
  String get matvFieldAccessMethod => 'Access method:';

  @override
  String get matvFieldStorageParams => 'Storage parameters:';

  @override
  String get matvWithData => 'Populate data (WITH DATA)';

  @override
  String get matvDefault => 'Default';

  @override
  String matvColumnsHint(String columns) {
    return 'Available columns: $columns';
  }

  @override
  String get matvSaveTitle => 'Save Materialized View';

  @override
  String get matvDefinitionEmpty =>
      'The definition is empty. Enter a query on the Definition tab.';

  @override
  String matvDefinitionReadFailed(String name, String error) {
    return 'Failed to read the definition of materialized view \"$name\":\n$error';
  }

  @override
  String matvRebuildConfirm(String name) {
    return 'Saving rebuilds materialized view \"$name\" (DROP, then CREATE).\nIts indexes are recreated automatically, but grants on it are lost and the data is repopulated.\nContinue?';
  }

  @override
  String matvSaveFailed(String error) {
    return 'Save failed:\n$error';
  }

  @override
  String matvSaved(String name) {
    return 'Saved materialized view \"$name\"';
  }

  @override
  String get matvPreviewEmpty =>
      'The definition is empty; there is nothing to preview.';

  @override
  String get matvExplainEmpty =>
      'The definition is empty; it cannot be explained.';

  @override
  String matvPreviewResult(String rows, String ms) {
    return 'Returned $rows rows in $ms ms';
  }

  @override
  String get matvRefreshRunning => 'Refreshing ...';

  @override
  String get matvToolbarRefreshConcurrently => 'Concurrent Refresh';

  @override
  String matvRefreshed(String name, String ms) {
    return 'Refreshed materialized view \"$name\" in $ms ms';
  }

  @override
  String matvRefreshFailed(String error) {
    return 'Refresh failed:\n$error';
  }

  @override
  String matvPreviewFailed(String error) {
    return 'Preview failed:\n$error';
  }

  @override
  String matvExplainFailed(String error) {
    return 'Explain failed:\n$error';
  }

  @override
  String get matvConcurrentlyNeedUnique =>
      'A concurrent refresh requires a unique index that covers every column and has no WHERE clause';

  @override
  String get matvConcurrentlyOk => 'Concurrent refresh available';

  @override
  String get matvConcurrentlyAsk =>
      'No qualifying unique index was found. Try a concurrent refresh anyway?';

  @override
  String matvNotPopulated(String name) {
    return 'Materialized view \"$name\" has no data (WITH NO DATA). Refresh it before querying.';
  }

  @override
  String get matvIndexIncomplete =>
      'Enter an index name and at least one column.';

  @override
  String get matvIndexNoSelection => 'Select an index in the list first.';

  @override
  String matvIndexDropConfirm(String name) {
    return 'Drop index \"$name\"?\nThis runs DROP INDEX and cannot be undone.';
  }

  @override
  String get sqlRunAtCursor => 'Run Current Statement';

  @override
  String get sqlRunSelected => 'Run Selected';

  @override
  String get sqlRunAll => 'Run All';

  @override
  String get sqlMenuCopy => 'Copy';

  @override
  String get sqlMenuPaste => 'Paste';

  @override
  String get sqlMenuSelectAll => 'Select All';

  @override
  String get kvTabCategory => 'Key browser';

  @override
  String get kvPatternHint =>
      'Search key names (substring by default; glob, e.g. scan:*)';

  @override
  String get kvTypeAll => 'All types';

  @override
  String get kvActionRefresh => 'Refresh';

  @override
  String get kvActionReadMore => 'Read rest';

  @override
  String get kvColKeyName => 'Key';

  @override
  String get kvColType => 'Type';

  @override
  String get kvColTtl => 'TTL';

  @override
  String get kvColEncoding => 'Encoding';

  @override
  String get kvColBytes => 'Bytes';

  @override
  String get kvColField => 'Field';

  @override
  String get kvColIndex => 'Index';

  @override
  String get kvColEntryId => 'Entry ID';

  @override
  String get kvColScore => 'Score';

  @override
  String get kvColMember => 'Member';

  @override
  String get kvColValue => 'Value';

  @override
  String get kvTypeString => 'String';

  @override
  String get kvTypeList => 'List';

  @override
  String get kvTypeSet => 'Set';

  @override
  String get kvTypeZset => 'Sorted set';

  @override
  String get kvTypeHash => 'Hash';

  @override
  String get kvTypeStream => 'Stream';

  @override
  String get kvTypeNone => 'Missing';

  @override
  String get kvTypeUnknown => 'Unknown';

  @override
  String get kvTtlNone => 'No expiry';

  @override
  String get kvTtlDay => 'd';

  @override
  String get kvScanning => 'Scanning…';

  @override
  String get kvScanComplete => 'Fully scanned';

  @override
  String get kvScanContinue => 'Scan not finished — click to keep scanning';

  @override
  String get kvRoundCapHint => 'No hit in this round, keep scanning';

  @override
  String kvLoadedCount(String loaded) {
    return '$loaded keys loaded';
  }

  @override
  String kvLoadedOfTotal(String loaded, String total) {
    return '$loaded keys loaded / about $total';
  }

  @override
  String get kvNoMatch => 'No keys match the filter';

  @override
  String get kvNoMatchYet =>
      'No matching key found yet — click to keep scanning';

  @override
  String kvLoadFailed(String error) {
    return 'Failed to load keys: $error';
  }

  @override
  String kvValueFailed(String error) {
    return 'Failed to read value: $error';
  }

  @override
  String get kvPickKey => 'Select a key on the left to view its value';

  @override
  String get kvKeyGone => 'This key no longer exists (expired or deleted)';

  @override
  String kvValueBytes(String total) {
    return '$total bytes';
  }

  @override
  String kvValueCount(String total) {
    return '$total items';
  }

  @override
  String kvValueTruncated(String shown, String total) {
    return 'Large value — first $shown of $total bytes shown';
  }

  @override
  String get kvFormatText => 'Text';

  @override
  String get kvFormatJson => 'JSON';

  @override
  String get kvFormatHex => 'Hex';

  @override
  String get kvFormatNotJson => 'Not valid JSON — showing as-is';

  @override
  String get kvFormatBinary => 'Binary value — cannot be shown as JSON';

  @override
  String kvFormatTooBig(String count) {
    return 'Formatted view renders only the first $count bytes';
  }

  @override
  String get kvFormatReadOnly =>
      'Formatted view is read-only — switch back to Text to edit';

  @override
  String get kvConnectionGone => 'This connection no longer exists';

  @override
  String get kvScanUnsupported => 'This engine cannot enumerate keys';

  @override
  String get kvActionNewKey => 'New Key';

  @override
  String get kvActionAdd => 'Add';

  @override
  String get kvActionSetTtl => 'Set Expiration...';

  @override
  String get kvActionClearTtl => 'Clear Expiration';

  @override
  String get kvMenuCopyKeyName => 'Copy Key Name';

  @override
  String get kvMenuDeleteKeyOne => 'Delete Key';

  @override
  String kvMenuDeleteKeys(String count) {
    return 'Delete $count Keys';
  }

  @override
  String get kvMenuEdit => 'Edit';

  @override
  String get kvMenuCopyValue => 'Copy Value';

  @override
  String get kvMenuDeleteItemOne => 'Delete Item';

  @override
  String kvMenuDeleteItems(String count) {
    return 'Delete $count Items';
  }

  @override
  String get kvNotWritable => 'This engine does not support writes';

  @override
  String get kvStreamReadOnly =>
      'Streams only accept appended entries, so they cannot be edited here';

  @override
  String get kvBigStringReadOnly =>
      'The value is not fully loaded and cannot be edited here (load the rest first)';

  @override
  String kvDeleteConfirmOne(String name) {
    return 'Delete key $name? This cannot be undone.';
  }

  @override
  String kvDeleteConfirmMany(String count) {
    return 'Delete the $count selected keys? This cannot be undone.';
  }

  @override
  String get kvDeleteDoneOne => 'Deleted 1 key';

  @override
  String kvDeleteDone(String count) {
    return 'Deleted $count keys';
  }

  @override
  String get kvTtlSetDoneOne => 'Set expiration for 1 key';

  @override
  String kvTtlSetDone(String count) {
    return 'Set expiration for $count keys';
  }

  @override
  String get kvTtlClearedDoneOne => 'Cleared expiration for 1 key';

  @override
  String kvTtlClearedDone(String count) {
    return 'Cleared expiration for $count keys';
  }

  @override
  String kvWritePartial(String ok, String total) {
    return '$ok of $total keys updated; the rest no longer exist';
  }

  @override
  String kvExpiresAt(String time) {
    return 'Expires at $time';
  }

  @override
  String kvWriteFailed(String error) {
    return 'Write failed: $error';
  }

  @override
  String get kvSaveDone => 'Saved';

  @override
  String get kvNewKeyTitle => 'New Key';

  @override
  String get kvKeyNameEmpty => 'Key name cannot be empty';

  @override
  String get kvKeyNameTaken =>
      'This key already exists. Use another name, or use Add to append to it';

  @override
  String get kvFieldTtl => 'Expiration';

  @override
  String get kvTtlHint => 'Empty = no expiration';

  @override
  String get kvTtlSetTitle => 'Set Expiration';

  @override
  String get kvTtlInvalid => 'Expiration must be a number greater than 0';

  @override
  String get kvTtlUnitSeconds => 'Seconds';

  @override
  String get kvTtlUnitMinutes => 'Minutes';

  @override
  String get kvTtlUnitHours => 'Hours';

  @override
  String get kvTtlUnitDays => 'Days';

  @override
  String get kvAddFieldTitle => 'Add Field';

  @override
  String get kvAddElementTitle => 'Add Element';

  @override
  String get kvAddMemberTitle => 'Add Member';

  @override
  String get kvPosition => 'Insert position';

  @override
  String get kvPositionHead => 'Head';

  @override
  String get kvPositionTail => 'Tail';

  @override
  String get kvScoreInvalid => 'Score must be a number';

  @override
  String get kvHexHint => 'Start with HEX: to write raw bytes as hex';

  @override
  String get kvHexInvalid =>
      'Invalid hex: needs an even number of 0-9A-F characters';

  @override
  String get kvValueRequired => 'Value cannot be empty';

  @override
  String kvCopiedNames(String count) {
    return 'Copied $count key name(s) to clipboard';
  }

  @override
  String get kvTtlAlreadyNone => 'These keys already had no expiry';

  @override
  String kvDeletePartial(String ok, String total) {
    return 'Deleted $ok of $total item(s); the rest are outside the loaded window or already gone';
  }

  @override
  String kvAddWindowOnly(String total, String shown) {
    return 'This value has $total item(s) but only $shown are loaded, so the new entry lands outside that range';
  }

  @override
  String get kvDeleteItemConfirmOne =>
      'Delete this item?\nIt cannot be recovered.';

  @override
  String kvDeleteItemConfirm(String count) {
    return 'Delete the $count selected items?\nThey cannot be recovered.';
  }

  @override
  String get kvItemsDeletedOne => 'Deleted 1 item';

  @override
  String kvItemsDeleted(String count) {
    return 'Deleted $count item(s)';
  }

  @override
  String get kvIndexReadOnly =>
      'The index marks an element\'s position, so it cannot be edited; edit the value on the right instead';

  @override
  String get findHint => 'Find';

  @override
  String get findReplaceHint => 'Replace';

  @override
  String get findNoMatches => 'No matches';

  @override
  String findMatchPosition(String current, String total) {
    return '$current of $total';
  }

  @override
  String get findSearching => 'Searching…';

  @override
  String get findPrev => 'Previous';

  @override
  String get findNext => 'Next';

  @override
  String get findCaseSensitive => 'Match case';

  @override
  String get findRegex => 'Use regular expression';

  @override
  String get findBtnReplace => 'Replace';

  @override
  String get findBtnReplaceAll => 'Replace All';

  @override
  String get findClose => 'Close';
}
