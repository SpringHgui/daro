// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Japanese (`ja`).
class AppLocalizationsJa extends AppLocalizations {
  AppLocalizationsJa([String locale = 'ja']) : super(locale);

  @override
  String get langFollowSystem => 'システムに従う';

  @override
  String get optionsTitle => 'オプション';

  @override
  String get optionsGeneral => '一般';

  @override
  String get optionsLanguage => '言語';

  @override
  String get optionsLanguageHint => '切り替えは即時反映され、表示中の画面も一緒に切り替わります。';

  @override
  String get themeMenuTitle => 'テーマ';

  @override
  String get themeFollowSystem => 'システムに従う';

  @override
  String get themeLight => 'ライト';

  @override
  String get themeDark => 'ダーク';

  @override
  String themeSwitchTooltip(String mode) {
    return 'テーマ：$mode（クリックで切替）';
  }

  @override
  String get menuFile => 'ファイル';

  @override
  String get menuNewConnection => '新しい接続...';

  @override
  String get menuNewQuery => '新しいクエリ';

  @override
  String get menuImportConnections => '接続のインポート';

  @override
  String get menuExportConnections => '接続のエクスポート';

  @override
  String get menuExit => '終了';

  @override
  String get menuView => '表示';

  @override
  String get menuRefresh => '更新';

  @override
  String get menuThemeCustomize => 'テーマのカスタマイズ...';

  @override
  String get menuLargeIcons => '大きいアイコン';

  @override
  String get menuSmallIcons => '小さいアイコン';

  @override
  String get menuList => '一覧';

  @override
  String get menuDetails => '詳細情報';

  @override
  String get menuTools => 'ツール';

  @override
  String get menuCommandLine => 'コマンドライン...';

  @override
  String get menuDataTransfer => 'データ転送...';

  @override
  String get menuDataSync => 'データ同期...';

  @override
  String get menuSchemaSync => '構造同期...';

  @override
  String get menuBackup => 'バックアップ...';

  @override
  String get menuRestoreBackup => 'バックアップの復元...';

  @override
  String get menuMcpService => 'MCP サービス...';

  @override
  String get menuOptions => 'オプション...';

  @override
  String get menuHelp => 'ヘルプ';

  @override
  String get menuIssueTracker => '問題の報告';

  @override
  String get menuAbout => '情報表示...';

  @override
  String get catTable => 'テーブル';

  @override
  String get catView => 'ビュー';

  @override
  String get catMaterializedView => 'マテリアライズドビュー';

  @override
  String get catFunction => '関数';

  @override
  String get catProcedure => 'ストアドプロシージャ';

  @override
  String get catRole => 'ロール';

  @override
  String get catQuery => 'クエリ';

  @override
  String get catBackup => 'バックアップ';

  @override
  String get ctxNewRole => '新規ロール';

  @override
  String get userTabGeneral => '一般';

  @override
  String get userTabAdvanced => '詳細';

  @override
  String get userTabMemberOf => '所属ロール';

  @override
  String get userTabMembers => 'メンバー';

  @override
  String get userTabServerPrivileges => 'サーバー権限';

  @override
  String get userTabPrivileges => '権限';

  @override
  String get userTabSqlPreview => 'SQL プレビュー';

  @override
  String get userFieldUsername => 'ユーザー名:';

  @override
  String get userFieldHost => 'ホスト:';

  @override
  String get userFieldPlugin => 'プラグイン:';

  @override
  String get userFieldPassword => 'パスワード:';

  @override
  String get userFieldPasswordConfirm => 'パスワードの確認:';

  @override
  String get userFieldExpirePolicy => 'パスワード有効期限:';

  @override
  String get userFieldExpireDays => '有効日数:';

  @override
  String get userFieldComment => 'コメント:';

  @override
  String get userFieldPrincipalType => 'プリンシパル種別:';

  @override
  String get userFieldConnectionLimit => '接続数制限:';

  @override
  String get userFieldValidUntil => '有効期限:';

  @override
  String get userFieldNewPassword => '新しいパスワード:';

  @override
  String get userExpireDefault => 'DEFAULT';

  @override
  String get userExpireExpired => '即時失効';

  @override
  String get userExpireNever => '無期限';

  @override
  String get userExpireInterval => '日数で失効';

  @override
  String get userPgLogin => 'ログイン可(LOGIN)';

  @override
  String get userPgSuper => 'スーパーユーザー(SUPERUSER)';

  @override
  String get userPgCreateDb => 'データベース作成(CREATEDB)';

  @override
  String get userPgCreateRole => 'ロール作成(CREATEROLE)';

  @override
  String get userPgInherit => '権限を継承(INHERIT)';

  @override
  String get userPgReplication => 'レプリケーション(REPLICATION)';

  @override
  String get userPgBypassRls => '行レベルセキュリティを回避(BYPASSRLS)';

  @override
  String get userPrincipalSql => 'SQL ログイン';

  @override
  String get userPrincipalWindows => 'Windows ユーザー';

  @override
  String get userPrincipalWindowsGroup => 'Windows グループ';

  @override
  String get userPrincipalRole => 'データベースロール';

  @override
  String get userIsRole => 'これはロールです(ログイン可能なユーザーではありません)';

  @override
  String get userColTarget => '対象';

  @override
  String get userColPrivilege => '権限';

  @override
  String get userColGrant => '付与可能';

  @override
  String get userColRole => 'ロール';

  @override
  String get userColMember => 'メンバー';

  @override
  String get userNoPrivileges => '付与された権限はありません。';

  @override
  String get userNoMembers => 'メンバーはいません。';

  @override
  String get userNoMemberOf => 'どのロールにも所属していません。';

  @override
  String get userNoCandidates => '選択できるロールがありません。';

  @override
  String get userAddPrivilege => '権限を追加';

  @override
  String get userRemovePrivilege => '削除';

  @override
  String get userPrivilegeTargetHint => 'データベース.テーブル(空 = サーバーレベル)';

  @override
  String userSaveOk(String name) {
    return 'ロール $name を保存しました';
  }

  @override
  String userSaveFailedAt(String index) {
    return '$index 番目のステートメントが失敗しました:';
  }

  @override
  String userSavedButRefreshFailed(String error) {
    return '保存しましたが、オブジェクト一覧の更新に失敗しました:$error';
  }

  @override
  String get userNameRequired => 'ユーザー名を入力してください。';

  @override
  String get userPasswordMismatch => '入力したパスワードが一致しません。';

  @override
  String get userPasswordRequired => 'パスワードを入力してください(新規アカウントには必要です)。';

  @override
  String userLoadFailed(String name, String error) {
    return '「$name」の読み込みに失敗しました:$error';
  }

  @override
  String get userNotSupported => 'このデータベース種別はアカウント管理に対応していません。';

  @override
  String get userAdvancedHint =>
      'このページの項目はアカウントの DDL を書き換えます。データベース種別により利用できる項目が異なります。';

  @override
  String get userPrivilegeHint =>
      'チェックした権限は保存時に付与されます。外すとその対象の権限をすべて取り消してから付与し直します。';

  @override
  String userDropConfirm(String name) {
    return 'ロール「$name」を削除しますか?\nこの操作は取り消せません。';
  }

  @override
  String get userMembershipHint =>
      '「所属ロール」はこのアカウントが所属するロール、「メンバー」はこのロールに所属するアカウントを表示します。';

  @override
  String get userRoleOnlyHint => 'ロールのメンバー関係は MySQL 8 以降でのみ利用できます。';

  @override
  String get userLoading => 'アカウント情報を読み込んでいます ...';

  @override
  String get userRefresh => '更新';

  @override
  String get userPrivilegeDatabase => 'データベース';

  @override
  String get userPrivilegeTable => 'テーブル';

  @override
  String get userPrivilegeNames => '権限(カンマ区切り)';

  @override
  String get userSelectedRoles => '所属ロール';

  @override
  String get userCandidateRoles => '選択可能なロール';

  @override
  String get importDoneTitle => 'インポート完了';

  @override
  String importDoneMessage(String count) {
    return '$count 件の接続をインポートしました。左の接続ツリーで確認できます。';
  }

  @override
  String importDoneManualPassword(String count) {
    return 'そのうち $count 件はパスワードを引き継げませんでした（Navicat 側に保存されていないか、古い暗号化方式です）。対象の接続を右クリック →「接続の編集」で補入力してください。';
  }

  @override
  String importDoneNewGroups(String count, String names) {
    return 'ローカルに存在しないグループ $count 件を作成しました：$names。';
  }

  @override
  String get exportDoneTitle => 'エクスポート完了';

  @override
  String exportDoneMessage(String count, String path) {
    return '$count 件の接続を\n$path\nにエクスポートしました。Navicat で「ファイル → 接続設定のインポート…」からこのファイルを選んでください。';
  }

  @override
  String exportDoneSkipped(String names) {
    return '対応する種類の接続がないためエクスポートされませんでした：$names。';
  }

  @override
  String listEllipsisMore(String count) {
    return ' など $count 件';
  }

  @override
  String get listEllipsis => ' など';

  @override
  String get listSeparator => '、';

  @override
  String get issueTrackerTitle => '問題の報告';

  @override
  String issueTrackerOpenFailed(String url) {
    return 'ブラウザを自動的に開けませんでした。次に手動でアクセスしてください：\n$url';
  }

  @override
  String get ribbonConnection => '接続';

  @override
  String get ribbonNewQuery => '新しいクエリ';

  @override
  String get tabObjects => 'オブジェクト';

  @override
  String get tabDesignSuffix => '（設計）';

  @override
  String get tabNewSuffix => '（新規）';

  @override
  String get tabCommandLine => 'コマンドライン';

  @override
  String get tabCtxClose => '閉じる';

  @override
  String get tabCtxCloseOthers => '他のタブを閉じる';

  @override
  String get tabCtxCloseRight => '右側のタブを閉じる';

  @override
  String get tabCtxCloseAll => 'すべて閉じる';

  @override
  String get statusNoDatabase => 'データベースが選択されていません';

  @override
  String statusRecordPosition(String current, String total, String page) {
    return 'レコード $current / 全 $total 件、$page ページ目';
  }

  @override
  String statusSelectedRows(String count, String total, String page) {
    return '$count 行を選択中（全 $total 件、$page ページ目）';
  }

  @override
  String statusObjectsSelected(String count) {
    return '$count 項目を選択中';
  }

  @override
  String get tipDetailedLayout => '詳細レイアウト';

  @override
  String get tipListLayout => '一覧';

  @override
  String get tipLeftPanel => '左パネル';

  @override
  String get tipRightPanel => '右パネル';

  @override
  String sqlHistoryTitle(String count) {
    return 'SQL 実行履歴（$count）';
  }

  @override
  String get sqlHistoryLatest => '最新';

  @override
  String get mcpTipDisabled => 'MCP サービスは無効です（クリックして設定を開く）';

  @override
  String get mcpTipCorrupted => 'MCP ポリシーの読み込みに失敗しました（クリックして詳細を表示）';

  @override
  String get mcpTipIdle => 'MCP サービスは有効ですが待機していません（クリックして設定を開く）';

  @override
  String get mcpTipRunning => 'MCP サービスは実行中です（クリックして設定を開く）';

  @override
  String mcpTipRunningCalls(String count) {
    return 'MCP 実行中（$count 件の呼び出し）';
  }

  @override
  String get infoPickNode => '左の接続ツリーでノードを選択すると詳細が表示されます';

  @override
  String get infoSectionDatabase => 'データベース';

  @override
  String get infoSectionConnection => '接続';

  @override
  String get infoSectionSchema => 'スキーマ';

  @override
  String get infoSectionConnGroup => '接続グループ';

  @override
  String get infoSectionTable => 'テーブル';

  @override
  String get fieldConnection => '接続';

  @override
  String get fieldType => 'タイプ';

  @override
  String get fieldHost => 'ホスト';

  @override
  String get fieldUser => 'ユーザー';

  @override
  String get fieldDatabase => 'データベース';

  @override
  String get fieldSchema => 'スキーマ';

  @override
  String get fieldConnCount => '接続数';

  @override
  String get infoGroupEmpty =>
      'グループは空です。接続を右クリック →「グループへ移動」で追加するか、このグループを削除してください。';

  @override
  String get infoTableDoubleClickHint => 'テーブルをダブルクリックすると先頭 100 行を表示します';

  @override
  String get fieldCharset => '文字セット';

  @override
  String get fieldCollation => '照合順序';

  @override
  String get fieldRows => '行数';

  @override
  String get fieldEngine => 'エンジン';

  @override
  String get fieldAutoIncrement => '自動採番';

  @override
  String get fieldRowFormat => '行フォーマット';

  @override
  String get fieldCreateTime => '作成日';

  @override
  String get fieldUpdateTime => '更新日';

  @override
  String get fieldCheckTime => 'チェック日時';

  @override
  String get fieldDataLength => 'データ長';

  @override
  String get fieldIndexLength => 'インデックス長';

  @override
  String get fieldMaxDataLength => '最大データ長';

  @override
  String get fieldDataFree => '空き領域';

  @override
  String get fieldCreateOptions => '作成オプション';

  @override
  String get fieldComment => 'コメント';

  @override
  String infoRowCountEstimate(String count) {
    return '$count (概算)';
  }

  @override
  String get infoFetchRowCount => '行数を取得';

  @override
  String get infoFetchingRowCount => '集計中...';

  @override
  String infoRowCountFailed(String error) {
    return '集計に失敗しました: $error';
  }

  @override
  String get infoPageInfo => '情報';

  @override
  String get infoPageDdl => 'DDL';

  @override
  String get fieldOid => 'OID';

  @override
  String get fieldOwner => 'オーナー';

  @override
  String get fieldTablespace => 'テーブル空間';

  @override
  String get fieldEncoding => 'エンコーディング';

  @override
  String get fieldLcCollate => '照合順序';

  @override
  String get fieldConnectionLimit => '接続制限';

  @override
  String get infoValueNoLimit => '無制限';

  @override
  String get fieldTableType => 'Table Type';

  @override
  String get tableTypeRegular => '通常';

  @override
  String get tableTypePartitioned => 'パーティションテーブル';

  @override
  String get tableTypeView => 'ビュー';

  @override
  String get tableTypeMatView => 'マテリアライズドビュー';

  @override
  String get tableTypeForeign => '外部テーブル';

  @override
  String get fieldPartitionOf => 'パーティション所属';

  @override
  String get fieldInheritsFrom => 'Inherits From';

  @override
  String get fieldHasOids => 'Has OIDs';

  @override
  String get fieldFillFactor => 'フィルファクタ';

  @override
  String get fieldAcl => 'ACL';

  @override
  String get infoValueYes => 'はい';

  @override
  String get infoValueNo => 'いいえ';

  @override
  String get infoPageUses => '使用';

  @override
  String get infoPageUsedBy => '使用元';

  @override
  String get infoPageUsesTooltip => 'このテーブルが参照するオブジェクト';

  @override
  String get infoPageUsedByTooltip => 'このテーブルを参照するオブジェクト';

  @override
  String get infoDepsLoading => '依存関係を読み込み中…';

  @override
  String get infoDepsEmpty => '依存オブジェクトはありません';

  @override
  String infoDepsFailed(String error) {
    return '依存関係の読み取りに失敗:$error';
  }

  @override
  String get infoMaximizePanel => '詳細パネルを広げる';

  @override
  String get infoRestorePanel => 'パネル幅に戻す';

  @override
  String infoDetailFailed(String error) {
    return '詳細を取得できません: $error';
  }

  @override
  String get infoDdlLoading => '定義を読み込み中...';

  @override
  String get infoDdlUnsupported => 'このデータベース種別は CREATE 文を表示できません。';

  @override
  String infoDdlFailed(String error) {
    return 'DDL を取得できません: $error';
  }

  @override
  String get infoCopyDdl => 'CREATE 文をコピー';

  @override
  String actionOpen(String label) {
    return '$labelを開く';
  }

  @override
  String actionDesign(String label) {
    return '$labelを設計';
  }

  @override
  String actionNew(String label) {
    return '$labelを新規作成';
  }

  @override
  String actionDelete(String label) {
    return '$labelを削除';
  }

  @override
  String get tableKindRegular => '通常';

  @override
  String get tableKindExternal => '外部';

  @override
  String get tableKindPartition => 'パーティション';

  @override
  String get importWizard => 'インポートウィザード';

  @override
  String get exportWizard => 'エクスポートウィザード';

  @override
  String actionClear(String label) {
    return '$labelをクリア';
  }

  @override
  String clearConfirmOne(String label, String name) {
    return '$label「$name」をクリアしますか?\nすべての行が削除されます(構造は保持され)、元に戻せません。';
  }

  @override
  String clearFailedDetail(String error) {
    return 'クリアできませんでした:\n$error';
  }

  @override
  String get btnClear => 'クリア';

  @override
  String get ctxCopyRename => 'コピー / 名前変更';

  @override
  String get btnGotIt => 'OK';

  @override
  String get btnDelete => '削除';

  @override
  String get btnPaste => '貼り付け';

  @override
  String get btnRetry => '再試行';

  @override
  String stubWip(String action) {
    return '$actionは現在開発中です';
  }

  @override
  String get deleteQueryTitle => 'クエリを削除';

  @override
  String deleteQueryConfirmOne(String name) {
    return 'クエリ「$name」を削除しますか?\n後に再保存できますが開いているクエリページには影響しません。';
  }

  @override
  String deleteQueryConfirmMany(String count) {
    return '$count 個の選択したクエリを削除しますか?';
  }

  @override
  String deleteObjectConfirmOne(String label, String name) {
    return '$label「$name」を削除しますか?\nこの操作は取り消せず、オブジェクトは完全に削除されます。';
  }

  @override
  String deleteObjectConfirmMany(String count, String label) {
    return '選択した $count 個の$labelを削除しますか?\nこの操作は取り消せず、オブジェクトは完全に削除されます。';
  }

  @override
  String deleteFailedNames(String names) {
    return '削除できません:$names\n接続状態、またはオブジェクトが存在するかを確認してください。';
  }

  @override
  String loadingObjectsTitle(String database) {
    return '$database のオブジェクト一覧を読み込んでいます ...';
  }

  @override
  String openDatabaseFailedTitle(String database) {
    return '$database を開けません';
  }

  @override
  String categoryListFailedTitle(String label) {
    return '$label一覧を取得できません';
  }

  @override
  String get colName => '名前';

  @override
  String get colRowsEstimated => '行数（推定）';

  @override
  String get colComment => 'コメント';

  @override
  String rowsApprox(String value, String unit) {
    return '約$value$unit';
  }

  @override
  String get rowsUnitSmall => '万';

  @override
  String get rowsUnitLarge => '億';

  @override
  String get renameTableTitle => 'テーブルの名前変更';

  @override
  String renameFailedDetail(String error) {
    return '名前を変更できません:\n$error';
  }

  @override
  String copiedTables(String count) {
    return '$count 個のテーブルをコピーしました。Ctrl+V でコピーを貼り付け';
  }

  @override
  String pasteNeedOpenConnection(String connection) {
    return '貼り付けの前に接続「$connection」を開いてください。';
  }

  @override
  String pasteWrongContext(String context) {
    return '貼り付けはコピー元と同じ接続 / データベース / スキーマにのみ可能です:\n$context';
  }

  @override
  String get pasteTableTitle => 'テーブルの貼り付け';

  @override
  String pasteConfirmDetail(String count, String plan) {
    return '$count 個のテーブル(構造 + データ)を作成します:\n$plan';
  }

  @override
  String pastedTables(String count) {
    return '$count 個のテーブルを貼り付けました';
  }

  @override
  String pasteFailedDetail(String detail) {
    return '貼り付けに失敗しました:\n$detail';
  }

  @override
  String get catTablePlural => 'テーブル';

  @override
  String get catViewPlural => 'ビュー';

  @override
  String get catMaterializedViewPlural => 'マテリアライズドビュー';

  @override
  String get catFunctionPlural => '関数';

  @override
  String get catProcedurePlural => 'プロシージャ';

  @override
  String get newExternalTable => '外部テーブルを新規作成';

  @override
  String get newPartitionTable => 'パーティションテーブルを新規作成';

  @override
  String openedConnection(String name) {
    return '接続「$name」を開きました';
  }

  @override
  String openedDatabase(String name) {
    return 'データベース「$name」を開きました';
  }

  @override
  String openedSchema(String name) {
    return 'スキーマ「$name」を開きました';
  }

  @override
  String get openedBare => '開きました';

  @override
  String get closedBare => '閉じました';

  @override
  String closedConnection(String name) {
    return '接続「$name」を閉じました';
  }

  @override
  String closedSchema(String name) {
    return 'スキーマ「$name」を閉じました';
  }

  @override
  String closedDatabase(String name) {
    return 'データベース「$name」を閉じました';
  }

  @override
  String openedNamed(String name) {
    return '「$name」を開きました';
  }

  @override
  String closedNamed(String name) {
    return '「$name」を閉じました';
  }

  @override
  String renamedConnection(String newName, String oldName) {
    return '接続「$oldName」を「$newName」に名前変更しました';
  }

  @override
  String renamedTable(String newName, String oldName) {
    return 'テーブル「$oldName」を「$newName」に名前変更しました';
  }

  @override
  String movedConnectionToUngrouped(String name) {
    return '接続「$name」を無グループへ移動しました';
  }

  @override
  String movedConnectionToGroup(String group, String name) {
    return '接続「$name」をグループ「$group」に入れました';
  }

  @override
  String get unnamedGroup => '無題グループ';

  @override
  String unnamedGroupNumbered(String index) {
    return '無題グループ $index';
  }

  @override
  String get renameGroupTitle => 'グループ名の変更';

  @override
  String groupAlreadyExists(String name) {
    return '「$name」というグループは既にあります(大文字小文字は区別しません)。';
  }

  @override
  String get ctxOpenConnection => '接続を開く';

  @override
  String get ctxCloseConnection => '接続を閉じる';

  @override
  String get ctxOpen => '開く';

  @override
  String get ctxClose => '閉じる';

  @override
  String get ctxRefresh => '更新';

  @override
  String get ctxNewDatabase => 'データベースを新規作成';

  @override
  String get ctxEditConnection => '接続を編集';

  @override
  String get ctxCopyConnection => '接続をコピー';

  @override
  String get ctxMoveToGroup => 'グループへ移動';

  @override
  String get ctxUngrouped => '無グループ';

  @override
  String get ctxNewGroup => 'グループを新規作成';

  @override
  String get ctxDeleteConnection => '接続を削除';

  @override
  String get ctxNewConnectionEllipsis => '新規接続…';

  @override
  String get ctxRenameGroup => 'グループ名の変更';

  @override
  String get ctxDeleteGroup => 'グループを削除';

  @override
  String ctxDeleteGroupWith(String count) {
    return 'グループを削除（接続 $count 件）';
  }

  @override
  String deleteGroupConfirm(String count, String group) {
    return 'グループ「$group」を削除しても接続 $count 件は削除されず、無グループに戻ります。続行しますか?';
  }

  @override
  String get btnCancel => 'キャンセル';

  @override
  String get btnSave => '保存';

  @override
  String get ctxNewSchema => 'スキーマを新規作成';

  @override
  String get ctxDelete => '削除';

  @override
  String get ctxEditDatabase => 'データベースを編集';

  @override
  String get ctxNewQuery => 'クエリを新規作成';

  @override
  String get ctxDumpSql => 'SQL ファイルにエクスポート';

  @override
  String get ctxStructureOnly => '構造のみ';

  @override
  String get ctxRunSql => 'SQL ファイルを実行';

  @override
  String get ctxCloseSchema => 'スキーマを閉じる';

  @override
  String get ctxOpenSchema => 'スキーマを開く';

  @override
  String get ctxEditSchema => 'スキーマを編集';

  @override
  String get ctxDeleteSchema => 'スキーマを削除';

  @override
  String deleteSchemaConfirm(String name) {
    return 'スキーマ「$name」を削除しますか?\nこのスキーマとその全オブジェクトが完全に削除され、元に戻せません。';
  }

  @override
  String deleteFailedDetail(String error) {
    return '削除できません:\n$error';
  }

  @override
  String get ctxNewTable => 'テーブルを新規作成';

  @override
  String get ctxNewView => 'ビューを新規作成';

  @override
  String get ctxNewFunction => '関数を新規作成';

  @override
  String get ctxNewProcedure => 'プロシージャを新規作成';

  @override
  String get deleteDatabaseTitle => 'データベースを削除';

  @override
  String deleteDatabaseConfirm(String name) {
    return 'データベース「$name」を削除しますか?\nこのデータベースとその全データが完全に削除され、元に戻せません。';
  }

  @override
  String get sqlFileTypeLabel => 'SQL ファイル';

  @override
  String get dumpStructureReadFailed =>
      '構造の読み取りに失敗しました:\nデータベースが使用できないか接続が閉じられています。接続を開いて再試行してください。';

  @override
  String dumpWriteFailed(String error) {
    return 'ファイルの書き込みに失敗しました:\n$error';
  }

  @override
  String dumpDatabaseDone(String name, String path) {
    return '「$name」の構造（構造のみ、データなし）を出力しました:\n$path';
  }

  @override
  String dumpSchemaDone(String name, String path) {
    return 'スキーマ「$name」の構造（構造のみ、データなし）を出力しました:\n$path';
  }

  @override
  String openConnectionUnsupported(String name, String type) {
    return '接続「$name」のデータベースタイプ（$type）には未対応のため開けません。';
  }

  @override
  String openConnectionFailed(String error, String name) {
    return '接続「$name」に失敗しました:\n$error';
  }

  @override
  String deleteConnectionConfirm(String name) {
    return '接続「$name」を削除しますか?\nこの接続のタブは残りますが、引き続き利用できません。';
  }

  @override
  String get noMatchingConnections => '一致する接続がありません';

  @override
  String get noConnectionsYet => '接続はまだありません';

  @override
  String get clickToolbarNewConnection => 'ツールバーの「接続」ボタンで新規作成できます';

  @override
  String get dropToUngroup => '離すと無グループに移動';

  @override
  String get driverNotImplemented => 'このタイプは未対応です(実装待ち)';

  @override
  String loadFailedDetail(String error) {
    return '読み込みに失敗しました: $error';
  }

  @override
  String get readFailed => '読み取り失敗';

  @override
  String get clickToRetry => 'クリックで再試行';

  @override
  String get searchConnectionsHint => '接続を検索...';

  @override
  String get dbTypeFilter => 'データベースタイプで絞り込み';

  @override
  String get notImplemented => '未実装';

  @override
  String get clearAllFilters => 'すべてクリア';

  @override
  String get collapseAll => 'すべて折りたたむ';

  @override
  String get cellEditorPickCell => 'まずセルを選択してください';

  @override
  String get btnCommitChanges => '変更を確定';

  @override
  String rowsCopiedToClipboard(String count) {
    return '$count 行をクリップボードにコピーしました';
  }

  @override
  String cellMenuSetNull(String count) {
    return '$count 個のセルを NULL に設定';
  }

  @override
  String cellMenuCopy(String count) {
    return '$count 個のセルをコピー';
  }

  @override
  String cellsCopiedToClipboard(String count) {
    return '$count 個のセルをクリップボードにコピーしました';
  }

  @override
  String cellsClearedToNull(String count, String action) {
    return '$count 個のセルを NULL に設定しました。「$action」または Ctrl+S で保存';
  }

  @override
  String get filterSourceBuilder => 'ビルダー';

  @override
  String get filterSourceText => 'テキスト';

  @override
  String get toolPanelFilter => '絞り込みと並び替え';

  @override
  String get toolPanelColumns => '列';

  @override
  String get toolPanelCellEditor => 'セルエディター';

  @override
  String get btnOk => 'OK';

  @override
  String get dtpSelectTime => '時刻を選択';

  @override
  String get dtpToday => '今日';

  @override
  String get dtpMonth1 => '1月';

  @override
  String get dtpMonth2 => '2月';

  @override
  String get dtpMonth3 => '3月';

  @override
  String get dtpMonth4 => '4月';

  @override
  String get dtpMonth5 => '5月';

  @override
  String get dtpMonth6 => '6月';

  @override
  String get dtpMonth7 => '7月';

  @override
  String get dtpMonth8 => '8月';

  @override
  String get dtpMonth9 => '9月';

  @override
  String get dtpMonth10 => '10月';

  @override
  String get dtpMonth11 => '11月';

  @override
  String get dtpMonth12 => '12月';

  @override
  String get dtpMonthTitle => '[year]年[month]月';

  @override
  String get dtpYearTitle => '[year]年';

  @override
  String get dtpYearRangeTitle => '[from] - [to]';

  @override
  String get dtpWeekdayMon => '月';

  @override
  String get dtpWeekdayTue => '火';

  @override
  String get dtpWeekdayWed => '水';

  @override
  String get dtpWeekdayThu => '木';

  @override
  String get dtpWeekdayFri => '金';

  @override
  String get dtpWeekdaySat => '土';

  @override
  String get dtpWeekdaySun => '日';

  @override
  String get catRecord => 'レコード';

  @override
  String gridPagingFailed(String error) {
    return 'ページの読み込みに失敗しました: $error';
  }

  @override
  String get gridAlreadyLastPage => 'すでに最終ページです';

  @override
  String gridPageMissing(String page) {
    return '$page ページは存在しません';
  }

  @override
  String get gridDiscardTitle => '未保存の変更を破棄';

  @override
  String get gridDiscardConfirm => '未保存の変更があります。続行すると破棄されます。\n続行しますか?';

  @override
  String gridDeleteRowConfirm(String row) {
    return '$row 行目のレコードを削除しますか?\n';
  }

  @override
  String gridDeleteRowsConfirm(String count, String preview) {
    return '選択した $count 行を削除しますか?($preview)\n';
  }

  @override
  String get gridDeletePendingHint => '「変更を確定」または Ctrl+S で初めてデータベースに書き込まれます。';

  @override
  String get gridNothingToSave => '保存する変更はありません';

  @override
  String gridConnectionMissing(String connection) {
    return '接続 \"$connection\" が存在しません';
  }

  @override
  String gridDeleteRowError(String row, String error) {
    return '$row 行目の削除: $error';
  }

  @override
  String gridInsertRowError(String error) {
    return '行の追加: $error';
  }

  @override
  String gridUpdateRowError(String row, String error) {
    return '$row 行目の更新: $error';
  }

  @override
  String gridSavedRows(String count) {
    return '$count 行を保存しました';
  }

  @override
  String gridSaveFailed(String errors) {
    return '保存失敗: $errors';
  }

  @override
  String get gridSaveFailedTitle => '保存失敗';

  @override
  String get gridSortMethod => '並び替え方式';

  @override
  String get gridAddSortCriterion => '並び替え条件を追加';

  @override
  String get gridSortEmptyHint => '+ をクリックして並び替え条件を追加';

  @override
  String get gridReadingColumns => '列情報を読み込んでいます…';

  @override
  String get gridFilterEmptyHint =>
      '+ をクリックして絞り込み条件を追加。行を選択すると、行末に同級の条件(+)または括弧グループ(O+)を追加できます。';

  @override
  String get gridFilter => '絞り込み';

  @override
  String get gridAddFilterCriterion => '絞り込み条件を追加';

  @override
  String get gridMoveCriterionUp => '選択した条件を上に移動';

  @override
  String get gridMoveCriterionDown => '選択した条件を下に移動';

  @override
  String get gridNoValueNeeded => '(値不要)';

  @override
  String get gridAddSibling => 'この条件の後に同級の条件を追加';

  @override
  String get gridAddGroup => 'この条件の後に括弧グループを追加';

  @override
  String get gridDeleteFilterGroup => 'グループを削除';

  @override
  String get gridDeleteCriterion => '条件を削除';

  @override
  String get gridWhereHint =>
      'WHERE 句を除いて入力。例:id > 100 AND name LIKE \'Acme%\'';

  @override
  String get gridApplyFilterSort => '絞り込みと並び替えを適用';

  @override
  String get gridCriterionEdited => '条件を編集しました';

  @override
  String get gridAsc => '昇順';

  @override
  String get gridDesc => '降順';

  @override
  String get gridDeleteSortCriterion => '並び替え条件を削除';

  @override
  String gridColumnsCount(String visible, String total) {
    return '列 ($visible/$total)';
  }

  @override
  String get gridShowAllColumns => 'すべての列を表示';

  @override
  String get gridKeepFirstColumnOnly => '最初の列のみ保持';

  @override
  String get gridLoadingColumns => '列情報を読み込んでいます...';

  @override
  String get gridSearch => '検索';

  @override
  String get gridColumnName => '列名';

  @override
  String get cellNoneSelected => 'セルが選択されていません';

  @override
  String cellColumnIndex(String col) {
    return '$col 列目';
  }

  @override
  String cellRowIndex(String row) {
    return '  ·  $row 行目';
  }

  @override
  String cellEditorTitle(String title) {
    return 'セルエディター · $title';
  }

  @override
  String get btnApply => '適用';

  @override
  String get btnUndo => '元に戻す';

  @override
  String get cellEmptyValue => 'セルは空です';

  @override
  String get cellNotBase64Image => 'このセルは認識できる base64 画像データではありません';

  @override
  String get cellNotHtml => 'このセルの内容は HTML ソースコードではありません';

  @override
  String cellWrittenBack(String action) {
    return 'セルに書き戻しました。「$action」または Ctrl+S で保存';
  }

  @override
  String get cellMenuSetBlank => '空白文字列に設定';

  @override
  String get cellMenuSetNullCell => 'NULL に設定';

  @override
  String get gridSort => '並び替え';

  @override
  String gridSortAscBy(String column) {
    return '昇順($column)';
  }

  @override
  String gridSortDescBy(String column) {
    return '降順($column)';
  }

  @override
  String get gridClearSort => '並び替えを解除';

  @override
  String get gridMoreSorting => 'その他の並び替え...';

  @override
  String gridHideColumn(String column) {
    return '「$column」を非表示';
  }

  @override
  String get gridColumnsPanel => '列パネル...';

  @override
  String get gridMoreFilters => 'その他の絞り込み...';

  @override
  String get gridClearFilter => '絞り込みをクリア';

  @override
  String get gridRemoveAllSortFilter => 'すべての並び替えと絞り込みを削除';

  @override
  String get gridShow => '表示';

  @override
  String get gridShowAll => 'すべてのレコード';

  @override
  String get gridShowNullOnly => 'NULL の値のみ';

  @override
  String get gridShowNotNullOnly => 'NULL 以外の値のみ';

  @override
  String get gridClipboardEmpty => 'クリップボードは空です';

  @override
  String get gridClipboardNoRecords => 'クリップボードに貼り付け可能なレコードがありません';

  @override
  String gridPastedRows(String count) {
    return '$count 行を新規レコードとして貼り付けました(未保存)';
  }

  @override
  String get gridRowCountUnknown => '  ·  行数不明';

  @override
  String gridRowCount(String total) {
    return '  ·  $total 行';
  }

  @override
  String gridLoadingTable(String table) {
    return '$table を読み込んでいます...';
  }

  @override
  String gridReadTableFailed(String table) {
    return '$table の読み込みに失敗';
  }

  @override
  String get gridFilterDirty => '絞り込み / 並び替えに未適用の変更があります';

  @override
  String get gridFilterApplied => '絞り込み / 並び替えを適用しました';

  @override
  String get gridAddRecord => 'レコード追加';

  @override
  String get gridDeleteSelectedRecords => '選択したレコードを削除';

  @override
  String get gridSaving => '保存中...';

  @override
  String get gridRevertChanges => '変更取消';

  @override
  String get gridStop => '停止';

  @override
  String gridPagerRange(String from, String to, String total) {
    return '$from-$to / 全 $total 件';
  }

  @override
  String get gridPageSize => 'ページサイズ';

  @override
  String gridRowsPerPage(String size) {
    return '$size 行/ページ';
  }

  @override
  String gridRowsPerPageCurrent(String size) {
    return '$size 行/ページ ✓';
  }

  @override
  String gridConnectionGone(String connection) {
    return '接続 \"$connection\" は存在しません。先に接続を開いてください。';
  }

  @override
  String get gridDeleteRecordTitle => 'レコードを削除';

  @override
  String gridDeleteRecordPending(String action) {
    return '削除は「$action」または Ctrl+S で初めてデータベースに書き込まれます。';
  }

  @override
  String gridDeleteRowDetail(String row, String tail) {
    return '$row 行目のレコードを削除しますか?\n$tail';
  }

  @override
  String gridDeleteRowsDetail(String count, String preview, String tail) {
    return '選択中の $count 行のレコードを削除しますか?($preview)\n$tail';
  }

  @override
  String gridUpdateRowErrorAt(String row, String error) {
    return '$row 行目の更新: $error';
  }

  @override
  String get gridSortDirection => '並び順';

  @override
  String get gridSortAsc => '昇順';

  @override
  String get gridSortDesc => '降順';

  @override
  String get gridJoinAnd => 'かつ';

  @override
  String get gridJoinOr => 'または';

  @override
  String get gridFilterValuePlaceholder => '<?>';

  @override
  String get gridSortTitle => '並べ替え';

  @override
  String get gridAddSortHint => '+ をクリックして並べ替え条件を追加';

  @override
  String get gridAddSortCriterionTitle => '並べ替え条件を追加';

  @override
  String get gridDeleteSortCriterionTitle => '並べ替え条件を削除';

  @override
  String get gridFilterTitle => 'フィルター';

  @override
  String get gridAddFilterCriterionTitle => 'フィルター条件を追加';

  @override
  String get gridMoveCriterionUpTitle => '選択した条件を上へ移動';

  @override
  String get gridMoveCriterionDownTitle => '選択した条件を下へ移動';

  @override
  String get gridFilterTextHint =>
      'WHERE キーワードは含めません。例: id > 100 AND name LIKE \'Acme%\'';

  @override
  String get gridApplyFilterSortTitle => 'フィルターと並べ替えを適用';

  @override
  String get gridCriterionEditedTitle => '条件を編集済み';

  @override
  String get gridColumnsTitle => '列';

  @override
  String get gridColumnsNoInfo => '列';

  @override
  String get gridSearchColumnsHint => '検索';

  @override
  String get gridLoadingColumnsEllipsis => '列情報を読み込み中 ...';

  @override
  String get gridReadingColumnsEllipsis => '列情報を読み込み中 …';

  @override
  String get gridCellEditorTabText => 'テキスト';

  @override
  String get gridCellEditorTabHex => '16 進数';

  @override
  String get gridCellEditorTabImage => '画像';

  @override
  String get gridCellEditorTabWeb => 'Web';

  @override
  String get gridSelectNone => 'セルが選択されていません';

  @override
  String gridColumnNumber(String col) {
    return '列 $col';
  }

  @override
  String gridDisplayRow(String row) {
    return '  ·  $row 行目';
  }

  @override
  String gridCellEditorHeader(String title) {
    return 'セルエディター · $title';
  }

  @override
  String get gridCellEditorPanelTitle => 'セルエディター';

  @override
  String get gridCellCopyAs => '形式を指定してコピー';

  @override
  String get gridCellCopyTsv => 'レコード（タブ区切り）';

  @override
  String get gridCellCopyCsv => 'レコード（CSV）';

  @override
  String get gridCellCopyCsvWithHeader => 'レコード + 列名（CSV）';

  @override
  String get gridCellPasteAppend => '行を貼り付け（新規として追加）';

  @override
  String get gridCellPasteToCell => 'セルに貼り付け';

  @override
  String get gridCellSaveAs => 'データを保存...';

  @override
  String get gridCellSetNullTitle => 'NULL に設定';

  @override
  String gridRowsCopy(String count) {
    return '$count 行をコピー';
  }

  @override
  String get gridRowsCopyOne => '行をコピー';

  @override
  String gridRowsDelete(String count) {
    return '$count 件のレコードを削除';
  }

  @override
  String get gridRowsDeleteOne => 'レコードを削除';

  @override
  String gridCellSortAscBy(String column) {
    return '昇順（$column）';
  }

  @override
  String gridCellSortDescBy(String column) {
    return '降順（$column）';
  }

  @override
  String gridCellHideColumn(String column) {
    return '「$column」を非表示';
  }

  @override
  String get dtpOk => 'OK';

  @override
  String get dtpCancel => 'キャンセル';

  @override
  String get btnApplyTitle => '適用';

  @override
  String get btnUndoTitle => '元に戻す';

  @override
  String get opEquals => '等しい';

  @override
  String get opNotEquals => '等しくない';

  @override
  String get opGreaterThan => 'より大きい';

  @override
  String get opGreaterOrEqual => '以上';

  @override
  String get opLessThan => 'より小さい';

  @override
  String get opLessOrEqual => '以下';

  @override
  String get opContains => '含む';

  @override
  String get opNotContains => '含まない';

  @override
  String get opStartsWith => 'で始まる';

  @override
  String get opEndsWith => 'で終わる';

  @override
  String get opIsNull => '空';

  @override
  String get opIsNotNull => '空でない';

  @override
  String get gridRefresh => '更新';

  @override
  String gridDeleteRowErrorAt(String row, String error) {
    return '行 $row を削除: $error';
  }

  @override
  String get cliEmptyHint =>
      'SQL を入力して Enter キーで実行します。文の終わりはセミコロン。未完成ならそのまま続きを入力、↑ / ↓ で履歴を参照します。';

  @override
  String get cliInputHint => 'SQL 文を入力';

  @override
  String get ctxNewMatView => 'マテリアライズドビューを新規作成';

  @override
  String actionRefresh(String label) {
    return '$labelを更新';
  }

  @override
  String actionRefreshConcurrently(String label) {
    return '$labelを並列更新';
  }

  @override
  String get matvTabDefinition => '定義';

  @override
  String get matvTabIndexes => 'インデックス';

  @override
  String get matvTabAdvanced => '詳細';

  @override
  String get matvTabComment => 'コメント';

  @override
  String get matvTabSqlPreview => 'SQL プレビュー';

  @override
  String get matvPanelMessages => 'メッセージ';

  @override
  String get matvPreview => 'プレビュー';

  @override
  String get matvExplain => '実行計画';

  @override
  String get matvBeautifySql => 'SQL を整形';

  @override
  String get matvTabChange => '変更';

  @override
  String get matvCopy => 'コピー';

  @override
  String get matvFullscreen => '全画面';

  @override
  String get matvExitFullscreen => '全画面を終了';

  @override
  String get matvCommentHint => 'このマテリアライズドビューのコメント (保存時に COMMENT ON)';

  @override
  String get matvAddIndex => 'インデックスを追加';

  @override
  String get matvDropIndex => 'インデックスを削除';

  @override
  String get matvColDefinition => '定義';

  @override
  String get colUnique => '一意';

  @override
  String get matvFieldIndexName => 'インデックス名:';

  @override
  String get matvFieldIndexColumns => '列:';

  @override
  String get matvFieldOwner => '所有者:';

  @override
  String get matvFieldTablespace => 'テーブルスペース:';

  @override
  String get matvFieldAccessMethod => 'アクセス方法:';

  @override
  String get matvFieldStorageParams => 'ストレージパラメータ:';

  @override
  String get matvWithData => 'データを格納 (WITH DATA)';

  @override
  String get matvDefault => '既定';

  @override
  String matvColumnsHint(String columns) {
    return '利用可能な列: $columns';
  }

  @override
  String get matvSaveTitle => 'マテリアライズドビューを保存';

  @override
  String get matvDefinitionEmpty => '定義が空です。「定義」タブでクエリを入力してください。';

  @override
  String matvDefinitionReadFailed(String name, String error) {
    return 'マテリアライズドビュー「$name」の定義を読み込めませんでした:\n$error';
  }

  @override
  String matvRebuildConfirm(String name) {
    return '保存するとマテリアライズドビュー「$name」を再作成します (DROP → CREATE)。\n既存のインデックスは自動的に再作成されますが、付与された権限 (GRANT) は失われ、データも再格納されます。\n続行しますか?';
  }

  @override
  String matvSaveFailed(String error) {
    return '保存に失敗しました:\n$error';
  }

  @override
  String matvSaved(String name) {
    return 'マテリアライズドビュー「$name」を保存しました';
  }

  @override
  String get matvPreviewEmpty => '定義が空のため、プレビューできません。';

  @override
  String get matvExplainEmpty => '定義が空のため、実行計画を取得できません。';

  @override
  String matvPreviewResult(String rows, String ms) {
    return '$rows 行を $ms ms で返しました';
  }

  @override
  String get matvRefreshRunning => '更新中 ...';

  @override
  String get matvToolbarRefreshConcurrently => '並行更新';

  @override
  String matvRefreshed(String name, String ms) {
    return 'マテリアライズドビュー「$name」を $ms ms で更新しました';
  }

  @override
  String matvRefreshFailed(String error) {
    return '更新に失敗しました:\n$error';
  }

  @override
  String matvPreviewFailed(String error) {
    return 'プレビューに失敗しました:\n$error';
  }

  @override
  String matvExplainFailed(String error) {
    return '実行計画の取得に失敗しました:\n$error';
  }

  @override
  String get matvConcurrentlyNeedUnique =>
      '並列更新には、全列をカバーし WHERE 条件を持たない一意インデックスが必要です';

  @override
  String get matvConcurrentlyOk => '並列更新可能';

  @override
  String get matvConcurrentlyAsk => '条件を満たす一意インデックスが見つかりません。並列更新を続けますか?';

  @override
  String matvNotPopulated(String name) {
    return 'マテリアライズドビュー「$name」はデータが未格納です (WITH NO DATA)。更新してから照会してください。';
  }

  @override
  String get matvIndexIncomplete => 'インデックス名と列を入力してください。';

  @override
  String get matvIndexNoSelection => '一覧から削除するインデックスを選択してください。';

  @override
  String matvIndexDropConfirm(String name) {
    return 'インデックス「$name」を削除しますか?\nDROP INDEX を実行し、元に戻せません。';
  }
}
