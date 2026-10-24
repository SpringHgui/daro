import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_ja.dart';
import 'app_localizations_zh.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
      : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
    delegate,
    GlobalMaterialLocalizations.delegate,
    GlobalCupertinoLocalizations.delegate,
    GlobalWidgetsLocalizations.delegate,
  ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('en'),
    Locale('ja'),
    Locale('zh')
  ];

  /// 选项 → 常规:语言下拉的「跟随系统」项
  ///
  /// In zh, this message translates to:
  /// **'跟随系统'**
  String get langFollowSystem;

  /// 「工具 → 选项...」对话框标题
  ///
  /// In zh, this message translates to:
  /// **'选项'**
  String get optionsTitle;

  /// 选项对话框左侧分类:常规
  ///
  /// In zh, this message translates to:
  /// **'常规'**
  String get optionsGeneral;

  /// 选项 → 常规:语言字段的标签
  ///
  /// In zh, this message translates to:
  /// **'语言'**
  String get optionsLanguage;

  /// 选项 → 常规:语言字段下方的说明
  ///
  /// In zh, this message translates to:
  /// **'切换后立即生效,已打开的界面文案一起切换。'**
  String get optionsLanguageHint;

  /// No description provided for @themeMenuTitle.
  ///
  /// In zh, this message translates to:
  /// **'主题'**
  String get themeMenuTitle;

  /// No description provided for @themeFollowSystem.
  ///
  /// In zh, this message translates to:
  /// **'跟随系统'**
  String get themeFollowSystem;

  /// No description provided for @themeLight.
  ///
  /// In zh, this message translates to:
  /// **'明亮'**
  String get themeLight;

  /// No description provided for @themeDark.
  ///
  /// In zh, this message translates to:
  /// **'暗黑'**
  String get themeDark;

  /// 标题栏主题按钮的悬停提示
  ///
  /// In zh, this message translates to:
  /// **'主题:{mode}（点击切换）'**
  String themeSwitchTooltip(String mode);

  /// No description provided for @menuFile.
  ///
  /// In zh, this message translates to:
  /// **'文件'**
  String get menuFile;

  /// No description provided for @menuNewConnection.
  ///
  /// In zh, this message translates to:
  /// **'新建连接...'**
  String get menuNewConnection;

  /// No description provided for @menuNewQuery.
  ///
  /// In zh, this message translates to:
  /// **'新建查询'**
  String get menuNewQuery;

  /// No description provided for @menuImportConnections.
  ///
  /// In zh, this message translates to:
  /// **'导入连接'**
  String get menuImportConnections;

  /// No description provided for @menuExportConnections.
  ///
  /// In zh, this message translates to:
  /// **'导出连接'**
  String get menuExportConnections;

  /// No description provided for @menuExit.
  ///
  /// In zh, this message translates to:
  /// **'退出'**
  String get menuExit;

  /// No description provided for @menuView.
  ///
  /// In zh, this message translates to:
  /// **'视图'**
  String get menuView;

  /// No description provided for @menuRefresh.
  ///
  /// In zh, this message translates to:
  /// **'刷新'**
  String get menuRefresh;

  /// No description provided for @menuThemeCustomize.
  ///
  /// In zh, this message translates to:
  /// **'主题定制...'**
  String get menuThemeCustomize;

  /// No description provided for @menuLargeIcons.
  ///
  /// In zh, this message translates to:
  /// **'大型图标'**
  String get menuLargeIcons;

  /// No description provided for @menuSmallIcons.
  ///
  /// In zh, this message translates to:
  /// **'小图标'**
  String get menuSmallIcons;

  /// No description provided for @menuList.
  ///
  /// In zh, this message translates to:
  /// **'列表'**
  String get menuList;

  /// No description provided for @menuDetails.
  ///
  /// In zh, this message translates to:
  /// **'详细信息'**
  String get menuDetails;

  /// No description provided for @menuTools.
  ///
  /// In zh, this message translates to:
  /// **'工具'**
  String get menuTools;

  /// No description provided for @menuCommandLine.
  ///
  /// In zh, this message translates to:
  /// **'命令列界面...'**
  String get menuCommandLine;

  /// No description provided for @menuDataTransfer.
  ///
  /// In zh, this message translates to:
  /// **'数据传输...'**
  String get menuDataTransfer;

  /// No description provided for @menuDataSync.
  ///
  /// In zh, this message translates to:
  /// **'数据同步...'**
  String get menuDataSync;

  /// No description provided for @menuSchemaSync.
  ///
  /// In zh, this message translates to:
  /// **'结构同步...'**
  String get menuSchemaSync;

  /// No description provided for @menuBackup.
  ///
  /// In zh, this message translates to:
  /// **'备份...'**
  String get menuBackup;

  /// No description provided for @menuRestoreBackup.
  ///
  /// In zh, this message translates to:
  /// **'还原备份...'**
  String get menuRestoreBackup;

  /// No description provided for @menuMcpService.
  ///
  /// In zh, this message translates to:
  /// **'MCP 服务...'**
  String get menuMcpService;

  /// No description provided for @menuOptions.
  ///
  /// In zh, this message translates to:
  /// **'选项...'**
  String get menuOptions;

  /// No description provided for @menuHelp.
  ///
  /// In zh, this message translates to:
  /// **'帮助'**
  String get menuHelp;

  /// No description provided for @menuIssueTracker.
  ///
  /// In zh, this message translates to:
  /// **'问题反馈'**
  String get menuIssueTracker;

  /// No description provided for @menuAbout.
  ///
  /// In zh, this message translates to:
  /// **'关于...'**
  String get menuAbout;

  /// No description provided for @catTable.
  ///
  /// In zh, this message translates to:
  /// **'表'**
  String get catTable;

  /// No description provided for @catView.
  ///
  /// In zh, this message translates to:
  /// **'视图'**
  String get catView;

  /// No description provided for @catMaterializedView.
  ///
  /// In zh, this message translates to:
  /// **'实体化视图'**
  String get catMaterializedView;

  /// No description provided for @catFunction.
  ///
  /// In zh, this message translates to:
  /// **'函数'**
  String get catFunction;

  /// No description provided for @catProcedure.
  ///
  /// In zh, this message translates to:
  /// **'过程'**
  String get catProcedure;

  /// No description provided for @catRole.
  ///
  /// In zh, this message translates to:
  /// **'角色'**
  String get catRole;

  /// No description provided for @catQuery.
  ///
  /// In zh, this message translates to:
  /// **'查询'**
  String get catQuery;

  /// No description provided for @catBackup.
  ///
  /// In zh, this message translates to:
  /// **'备份'**
  String get catBackup;

  /// 连接树 / 对象面板「角色」分组右键菜单的新建入口
  ///
  /// In zh, this message translates to:
  /// **'新建角色'**
  String get ctxNewRole;

  /// No description provided for @userTabGeneral.
  ///
  /// In zh, this message translates to:
  /// **'常规'**
  String get userTabGeneral;

  /// No description provided for @userTabAdvanced.
  ///
  /// In zh, this message translates to:
  /// **'高级'**
  String get userTabAdvanced;

  /// No description provided for @userTabMemberOf.
  ///
  /// In zh, this message translates to:
  /// **'成员属于'**
  String get userTabMemberOf;

  /// No description provided for @userTabMembers.
  ///
  /// In zh, this message translates to:
  /// **'成员'**
  String get userTabMembers;

  /// No description provided for @userTabServerPrivileges.
  ///
  /// In zh, this message translates to:
  /// **'服务器权限'**
  String get userTabServerPrivileges;

  /// No description provided for @userTabPrivileges.
  ///
  /// In zh, this message translates to:
  /// **'权限'**
  String get userTabPrivileges;

  /// No description provided for @userTabSqlPreview.
  ///
  /// In zh, this message translates to:
  /// **'SQL 预览'**
  String get userTabSqlPreview;

  /// No description provided for @userFieldUsername.
  ///
  /// In zh, this message translates to:
  /// **'用户名:'**
  String get userFieldUsername;

  /// No description provided for @userFieldHost.
  ///
  /// In zh, this message translates to:
  /// **'主机:'**
  String get userFieldHost;

  /// No description provided for @userFieldPlugin.
  ///
  /// In zh, this message translates to:
  /// **'插件:'**
  String get userFieldPlugin;

  /// No description provided for @userFieldPassword.
  ///
  /// In zh, this message translates to:
  /// **'密码:'**
  String get userFieldPassword;

  /// No description provided for @userFieldPasswordConfirm.
  ///
  /// In zh, this message translates to:
  /// **'确认密码:'**
  String get userFieldPasswordConfirm;

  /// No description provided for @userFieldExpirePolicy.
  ///
  /// In zh, this message translates to:
  /// **'密码过期策略:'**
  String get userFieldExpirePolicy;

  /// No description provided for @userFieldExpireDays.
  ///
  /// In zh, this message translates to:
  /// **'过期天数:'**
  String get userFieldExpireDays;

  /// No description provided for @userFieldComment.
  ///
  /// In zh, this message translates to:
  /// **'注释:'**
  String get userFieldComment;

  /// No description provided for @userFieldPrincipalType.
  ///
  /// In zh, this message translates to:
  /// **'主体类型:'**
  String get userFieldPrincipalType;

  /// No description provided for @userFieldConnectionLimit.
  ///
  /// In zh, this message translates to:
  /// **'连接数限制:'**
  String get userFieldConnectionLimit;

  /// No description provided for @userFieldValidUntil.
  ///
  /// In zh, this message translates to:
  /// **'口令失效时间:'**
  String get userFieldValidUntil;

  /// No description provided for @userFieldNewPassword.
  ///
  /// In zh, this message translates to:
  /// **'新密码:'**
  String get userFieldNewPassword;

  /// No description provided for @userExpireDefault.
  ///
  /// In zh, this message translates to:
  /// **'DEFAULT'**
  String get userExpireDefault;

  /// No description provided for @userExpireExpired.
  ///
  /// In zh, this message translates to:
  /// **'立即过期'**
  String get userExpireExpired;

  /// No description provided for @userExpireNever.
  ///
  /// In zh, this message translates to:
  /// **'永不过期'**
  String get userExpireNever;

  /// No description provided for @userExpireInterval.
  ///
  /// In zh, this message translates to:
  /// **'按天过期'**
  String get userExpireInterval;

  /// No description provided for @userPgLogin.
  ///
  /// In zh, this message translates to:
  /// **'可登录(LOGIN)'**
  String get userPgLogin;

  /// No description provided for @userPgSuper.
  ///
  /// In zh, this message translates to:
  /// **'超级用户(SUPERUSER)'**
  String get userPgSuper;

  /// No description provided for @userPgCreateDb.
  ///
  /// In zh, this message translates to:
  /// **'创建数据库(CREATEDB)'**
  String get userPgCreateDb;

  /// No description provided for @userPgCreateRole.
  ///
  /// In zh, this message translates to:
  /// **'创建角色(CREATEROLE)'**
  String get userPgCreateRole;

  /// No description provided for @userPgInherit.
  ///
  /// In zh, this message translates to:
  /// **'继承权限(INHERIT)'**
  String get userPgInherit;

  /// No description provided for @userPgReplication.
  ///
  /// In zh, this message translates to:
  /// **'复制(REPLICATION)'**
  String get userPgReplication;

  /// No description provided for @userPgBypassRls.
  ///
  /// In zh, this message translates to:
  /// **'绕过行级安全(BYPASSRLS)'**
  String get userPgBypassRls;

  /// No description provided for @userPrincipalSql.
  ///
  /// In zh, this message translates to:
  /// **'SQL 登录名'**
  String get userPrincipalSql;

  /// No description provided for @userPrincipalWindows.
  ///
  /// In zh, this message translates to:
  /// **'Windows 用户'**
  String get userPrincipalWindows;

  /// No description provided for @userPrincipalWindowsGroup.
  ///
  /// In zh, this message translates to:
  /// **'Windows 组'**
  String get userPrincipalWindowsGroup;

  /// No description provided for @userPrincipalRole.
  ///
  /// In zh, this message translates to:
  /// **'数据库角色'**
  String get userPrincipalRole;

  /// No description provided for @userIsRole.
  ///
  /// In zh, this message translates to:
  /// **'这是一个角色(非可登录用户)'**
  String get userIsRole;

  /// No description provided for @userColTarget.
  ///
  /// In zh, this message translates to:
  /// **'对象'**
  String get userColTarget;

  /// No description provided for @userColPrivilege.
  ///
  /// In zh, this message translates to:
  /// **'权限'**
  String get userColPrivilege;

  /// No description provided for @userColGrant.
  ///
  /// In zh, this message translates to:
  /// **'可转授'**
  String get userColGrant;

  /// No description provided for @userColRole.
  ///
  /// In zh, this message translates to:
  /// **'角色'**
  String get userColRole;

  /// No description provided for @userColMember.
  ///
  /// In zh, this message translates to:
  /// **'成员'**
  String get userColMember;

  /// No description provided for @userNoPrivileges.
  ///
  /// In zh, this message translates to:
  /// **'没有已授予的权限。'**
  String get userNoPrivileges;

  /// No description provided for @userNoMembers.
  ///
  /// In zh, this message translates to:
  /// **'没有成员。'**
  String get userNoMembers;

  /// No description provided for @userNoMemberOf.
  ///
  /// In zh, this message translates to:
  /// **'不属于任何角色。'**
  String get userNoMemberOf;

  /// No description provided for @userNoCandidates.
  ///
  /// In zh, this message translates to:
  /// **'没有可选的角色。'**
  String get userNoCandidates;

  /// No description provided for @userAddPrivilege.
  ///
  /// In zh, this message translates to:
  /// **'添加权限'**
  String get userAddPrivilege;

  /// No description provided for @userRemovePrivilege.
  ///
  /// In zh, this message translates to:
  /// **'移除'**
  String get userRemovePrivilege;

  /// No description provided for @userPrivilegeTargetHint.
  ///
  /// In zh, this message translates to:
  /// **'库.表(留空 = 服务器级)'**
  String get userPrivilegeTargetHint;

  /// No description provided for @userSaveOk.
  ///
  /// In zh, this message translates to:
  /// **'已保存角色 {name}'**
  String userSaveOk(String name);

  /// No description provided for @userSaveFailedAt.
  ///
  /// In zh, this message translates to:
  /// **'第 {index} 条语句执行失败:'**
  String userSaveFailedAt(String index);

  /// No description provided for @userSavedButRefreshFailed.
  ///
  /// In zh, this message translates to:
  /// **'已保存,但刷新对象列表失败:{error}'**
  String userSavedButRefreshFailed(String error);

  /// No description provided for @userNameRequired.
  ///
  /// In zh, this message translates to:
  /// **'请填写用户名。'**
  String get userNameRequired;

  /// No description provided for @userPasswordMismatch.
  ///
  /// In zh, this message translates to:
  /// **'两次输入的密码不一致。'**
  String get userPasswordMismatch;

  /// No description provided for @userPasswordRequired.
  ///
  /// In zh, this message translates to:
  /// **'请填写密码(新建账号需要设置密码)。'**
  String get userPasswordRequired;

  /// No description provided for @userLoadFailed.
  ///
  /// In zh, this message translates to:
  /// **'读取「{name}」详情失败:{error}'**
  String userLoadFailed(String name, String error);

  /// No description provided for @userNotSupported.
  ///
  /// In zh, this message translates to:
  /// **'当前数据库类型不支持账号管理。'**
  String get userNotSupported;

  /// No description provided for @userAdvancedHint.
  ///
  /// In zh, this message translates to:
  /// **'本页选项会改写建号语句;不同数据库类型的可用项不同。'**
  String get userAdvancedHint;

  /// No description provided for @userPrivilegeHint.
  ///
  /// In zh, this message translates to:
  /// **'勾选后保存会重新授予权限;取消勾选会先撤销该对象的全部权限再授予。'**
  String get userPrivilegeHint;

  /// No description provided for @userDropConfirm.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除角色「{name}」吗?\n此操作不可恢复。'**
  String userDropConfirm(String name);

  /// No description provided for @userMembershipHint.
  ///
  /// In zh, this message translates to:
  /// **'「成员属于」列出本账号加入了哪些角色;「成员」列出哪些账号加入了本角色。'**
  String get userMembershipHint;

  /// No description provided for @userRoleOnlyHint.
  ///
  /// In zh, this message translates to:
  /// **'角色成员关系仅在 MySQL 8 及以上版本可用。'**
  String get userRoleOnlyHint;

  /// No description provided for @userLoading.
  ///
  /// In zh, this message translates to:
  /// **'正在读取账号信息 …'**
  String get userLoading;

  /// No description provided for @userRefresh.
  ///
  /// In zh, this message translates to:
  /// **'刷新'**
  String get userRefresh;

  /// No description provided for @userPrivilegeDatabase.
  ///
  /// In zh, this message translates to:
  /// **'库名'**
  String get userPrivilegeDatabase;

  /// No description provided for @userPrivilegeTable.
  ///
  /// In zh, this message translates to:
  /// **'表名'**
  String get userPrivilegeTable;

  /// No description provided for @userPrivilegeNames.
  ///
  /// In zh, this message translates to:
  /// **'权限名(逗号分隔)'**
  String get userPrivilegeNames;

  /// No description provided for @userSelectedRoles.
  ///
  /// In zh, this message translates to:
  /// **'已加入'**
  String get userSelectedRoles;

  /// No description provided for @userCandidateRoles.
  ///
  /// In zh, this message translates to:
  /// **'可选角色'**
  String get userCandidateRoles;

  /// No description provided for @importDoneTitle.
  ///
  /// In zh, this message translates to:
  /// **'导入完成'**
  String get importDoneTitle;

  /// No description provided for @importDoneMessage.
  ///
  /// In zh, this message translates to:
  /// **'已导入 {count} 条连接,可在左侧连接树查看。'**
  String importDoneMessage(String count);

  /// No description provided for @importDoneManualPassword.
  ///
  /// In zh, this message translates to:
  /// **'其中 {count} 条没能带过密码(Navicat 端未保存,或用了旧版加密方式),右键该连接 →「编辑连接」补填后即可正常连接。'**
  String importDoneManualPassword(String count);

  /// No description provided for @importDoneNewGroups.
  ///
  /// In zh, this message translates to:
  /// **'文件里的分组本地不存在,已新建 {count} 个:{names}。'**
  String importDoneNewGroups(String count, String names);

  /// No description provided for @exportDoneTitle.
  ///
  /// In zh, this message translates to:
  /// **'导出完成'**
  String get exportDoneTitle;

  /// No description provided for @exportDoneMessage.
  ///
  /// In zh, this message translates to:
  /// **'已把 {count} 条连接导出到\n{path}\n在 Navicat 里用「文件 → 导入连接设置…」选择该文件即可。'**
  String exportDoneMessage(String count, String path);

  /// No description provided for @exportDoneSkipped.
  ///
  /// In zh, this message translates to:
  /// **'Navicat 没有对应类型的连接未导出:{names}。'**
  String exportDoneSkipped(String names);

  /// 列表截断后的「等 N 条」尾巴,自带前导空格
  ///
  /// In zh, this message translates to:
  /// **' 等 {count} 条'**
  String listEllipsisMore(String count);

  /// No description provided for @listEllipsis.
  ///
  /// In zh, this message translates to:
  /// **' 等'**
  String get listEllipsis;

  /// 顿号 / 逗号等列表分隔符
  ///
  /// In zh, this message translates to:
  /// **'、'**
  String get listSeparator;

  /// No description provided for @issueTrackerTitle.
  ///
  /// In zh, this message translates to:
  /// **'问题反馈'**
  String get issueTrackerTitle;

  /// No description provided for @issueTrackerOpenFailed.
  ///
  /// In zh, this message translates to:
  /// **'无法自动打开浏览器,请在浏览器中访问:\n{url}'**
  String issueTrackerOpenFailed(String url);

  /// No description provided for @ribbonConnection.
  ///
  /// In zh, this message translates to:
  /// **'连接'**
  String get ribbonConnection;

  /// No description provided for @ribbonNewQuery.
  ///
  /// In zh, this message translates to:
  /// **'新建查询'**
  String get ribbonNewQuery;

  /// 固定的对象浏览页标签,同时是它的身份标识
  ///
  /// In zh, this message translates to:
  /// **'对象'**
  String get tabObjects;

  /// 设计页标签标题后缀,自带前导空格
  ///
  /// In zh, this message translates to:
  /// **' (设计)'**
  String get tabDesignSuffix;

  /// 新建对象标签标题后缀,自带前导空格
  ///
  /// In zh, this message translates to:
  /// **' (新建)'**
  String get tabNewSuffix;

  /// 命令列界面标签的显示名(不含省略号,标签拼成「连接名 - 命令列界面」)
  ///
  /// In zh, this message translates to:
  /// **'命令列界面'**
  String get tabCommandLine;

  /// No description provided for @tabCtxClose.
  ///
  /// In zh, this message translates to:
  /// **'关闭'**
  String get tabCtxClose;

  /// No description provided for @tabCtxCloseOthers.
  ///
  /// In zh, this message translates to:
  /// **'关闭其他选项卡'**
  String get tabCtxCloseOthers;

  /// No description provided for @tabCtxCloseRight.
  ///
  /// In zh, this message translates to:
  /// **'关闭右侧的选项卡'**
  String get tabCtxCloseRight;

  /// No description provided for @tabCtxCloseAll.
  ///
  /// In zh, this message translates to:
  /// **'全部关闭'**
  String get tabCtxCloseAll;

  /// No description provided for @statusNoDatabase.
  ///
  /// In zh, this message translates to:
  /// **'未选择数据库'**
  String get statusNoDatabase;

  /// No description provided for @statusRecordPosition.
  ///
  /// In zh, this message translates to:
  /// **'第 {current} 条记录（共 {total} 条）于第 {page} 页'**
  String statusRecordPosition(String current, String total, String page);

  /// No description provided for @statusSelectedRows.
  ///
  /// In zh, this message translates to:
  /// **'已选 {count} 行（共 {total} 条）于第 {page} 页'**
  String statusSelectedRows(String count, String total, String page);

  /// No description provided for @statusObjectsSelected.
  ///
  /// In zh, this message translates to:
  /// **'已选择 {count} 项'**
  String statusObjectsSelected(String count);

  /// No description provided for @tipDetailedLayout.
  ///
  /// In zh, this message translates to:
  /// **'详细布局'**
  String get tipDetailedLayout;

  /// No description provided for @tipListLayout.
  ///
  /// In zh, this message translates to:
  /// **'列表'**
  String get tipListLayout;

  /// No description provided for @tipLeftPanel.
  ///
  /// In zh, this message translates to:
  /// **'左侧栏'**
  String get tipLeftPanel;

  /// No description provided for @tipRightPanel.
  ///
  /// In zh, this message translates to:
  /// **'右侧栏'**
  String get tipRightPanel;

  /// No description provided for @sqlHistoryTitle.
  ///
  /// In zh, this message translates to:
  /// **'SQL 执行历史 ({count})'**
  String sqlHistoryTitle(String count);

  /// No description provided for @sqlHistoryLatest.
  ///
  /// In zh, this message translates to:
  /// **'最新'**
  String get sqlHistoryLatest;

  /// No description provided for @mcpTipDisabled.
  ///
  /// In zh, this message translates to:
  /// **'MCP 服务已禁用(点击打开设置)'**
  String get mcpTipDisabled;

  /// No description provided for @mcpTipCorrupted.
  ///
  /// In zh, this message translates to:
  /// **'MCP 策略加载失败(点击查看详情)'**
  String get mcpTipCorrupted;

  /// No description provided for @mcpTipIdle.
  ///
  /// In zh, this message translates to:
  /// **'MCP 服务已启用但未监听(点击打开设置)'**
  String get mcpTipIdle;

  /// No description provided for @mcpTipRunning.
  ///
  /// In zh, this message translates to:
  /// **'MCP 运行中(点击打开设置)'**
  String get mcpTipRunning;

  /// No description provided for @mcpTipRunningCalls.
  ///
  /// In zh, this message translates to:
  /// **'MCP 运行中 ({count} 个调用)'**
  String mcpTipRunningCalls(String count);

  /// No description provided for @infoPickNode.
  ///
  /// In zh, this message translates to:
  /// **'在左侧连接树中选择节点查看详情'**
  String get infoPickNode;

  /// No description provided for @infoSectionDatabase.
  ///
  /// In zh, this message translates to:
  /// **'数据库'**
  String get infoSectionDatabase;

  /// No description provided for @infoSectionConnection.
  ///
  /// In zh, this message translates to:
  /// **'连接'**
  String get infoSectionConnection;

  /// No description provided for @infoSectionSchema.
  ///
  /// In zh, this message translates to:
  /// **'模式'**
  String get infoSectionSchema;

  /// No description provided for @infoSectionConnGroup.
  ///
  /// In zh, this message translates to:
  /// **'连接分组'**
  String get infoSectionConnGroup;

  /// No description provided for @infoSectionKvNamespace.
  ///
  /// In zh, this message translates to:
  /// **'缓存命名空间'**
  String get infoSectionKvNamespace;

  /// No description provided for @infoSectionTable.
  ///
  /// In zh, this message translates to:
  /// **'表'**
  String get infoSectionTable;

  /// No description provided for @fieldConnection.
  ///
  /// In zh, this message translates to:
  /// **'连接'**
  String get fieldConnection;

  /// No description provided for @fieldType.
  ///
  /// In zh, this message translates to:
  /// **'类型'**
  String get fieldType;

  /// No description provided for @fieldKeyCount.
  ///
  /// In zh, this message translates to:
  /// **'键数量'**
  String get fieldKeyCount;

  /// No description provided for @fieldHost.
  ///
  /// In zh, this message translates to:
  /// **'主机'**
  String get fieldHost;

  /// No description provided for @fieldUser.
  ///
  /// In zh, this message translates to:
  /// **'用户'**
  String get fieldUser;

  /// No description provided for @fieldDatabase.
  ///
  /// In zh, this message translates to:
  /// **'数据库'**
  String get fieldDatabase;

  /// No description provided for @fieldSchema.
  ///
  /// In zh, this message translates to:
  /// **'模式'**
  String get fieldSchema;

  /// No description provided for @fieldConnCount.
  ///
  /// In zh, this message translates to:
  /// **'连接数'**
  String get fieldConnCount;

  /// No description provided for @infoGroupEmpty.
  ///
  /// In zh, this message translates to:
  /// **'分组为空:把连接右键 →「移动到分组」放进来,或删除该分组。'**
  String get infoGroupEmpty;

  /// No description provided for @infoTableDoubleClickHint.
  ///
  /// In zh, this message translates to:
  /// **'双击表可查看前 100 行数据'**
  String get infoTableDoubleClickHint;

  /// No description provided for @fieldCharset.
  ///
  /// In zh, this message translates to:
  /// **'字符集'**
  String get fieldCharset;

  /// No description provided for @fieldCollation.
  ///
  /// In zh, this message translates to:
  /// **'排序规则'**
  String get fieldCollation;

  /// No description provided for @fieldRows.
  ///
  /// In zh, this message translates to:
  /// **'行'**
  String get fieldRows;

  /// No description provided for @fieldEngine.
  ///
  /// In zh, this message translates to:
  /// **'引擎'**
  String get fieldEngine;

  /// No description provided for @fieldAutoIncrement.
  ///
  /// In zh, this message translates to:
  /// **'自动递增'**
  String get fieldAutoIncrement;

  /// No description provided for @fieldRowFormat.
  ///
  /// In zh, this message translates to:
  /// **'行格式'**
  String get fieldRowFormat;

  /// No description provided for @fieldCreateTime.
  ///
  /// In zh, this message translates to:
  /// **'创建日期'**
  String get fieldCreateTime;

  /// No description provided for @fieldUpdateTime.
  ///
  /// In zh, this message translates to:
  /// **'修改日期'**
  String get fieldUpdateTime;

  /// No description provided for @fieldCheckTime.
  ///
  /// In zh, this message translates to:
  /// **'检查时间'**
  String get fieldCheckTime;

  /// No description provided for @fieldDataLength.
  ///
  /// In zh, this message translates to:
  /// **'数据长度'**
  String get fieldDataLength;

  /// No description provided for @fieldIndexLength.
  ///
  /// In zh, this message translates to:
  /// **'索引长度'**
  String get fieldIndexLength;

  /// No description provided for @fieldMaxDataLength.
  ///
  /// In zh, this message translates to:
  /// **'最大数据长度'**
  String get fieldMaxDataLength;

  /// No description provided for @fieldDataFree.
  ///
  /// In zh, this message translates to:
  /// **'数据可用空间'**
  String get fieldDataFree;

  /// No description provided for @fieldCreateOptions.
  ///
  /// In zh, this message translates to:
  /// **'创建选项'**
  String get fieldCreateOptions;

  /// No description provided for @fieldComment.
  ///
  /// In zh, this message translates to:
  /// **'注释'**
  String get fieldComment;

  /// 详情面板「行」的估算值展示;精确值需点「获取行数」
  ///
  /// In zh, this message translates to:
  /// **'{count} (估算)'**
  String infoRowCountEstimate(String count);

  /// No description provided for @infoFetchRowCount.
  ///
  /// In zh, this message translates to:
  /// **'获取行数'**
  String get infoFetchRowCount;

  /// No description provided for @infoFetchingRowCount.
  ///
  /// In zh, this message translates to:
  /// **'正在统计…'**
  String get infoFetchingRowCount;

  /// No description provided for @infoRowCountFailed.
  ///
  /// In zh, this message translates to:
  /// **'统计失败:{error}'**
  String infoRowCountFailed(String error);

  /// No description provided for @infoPageInfo.
  ///
  /// In zh, this message translates to:
  /// **'信息'**
  String get infoPageInfo;

  /// No description provided for @infoPageDdl.
  ///
  /// In zh, this message translates to:
  /// **'DDL'**
  String get infoPageDdl;

  /// No description provided for @fieldOid.
  ///
  /// In zh, this message translates to:
  /// **'OID'**
  String get fieldOid;

  /// No description provided for @fieldOwner.
  ///
  /// In zh, this message translates to:
  /// **'所有者'**
  String get fieldOwner;

  /// No description provided for @fieldTablespace.
  ///
  /// In zh, this message translates to:
  /// **'表空间'**
  String get fieldTablespace;

  /// No description provided for @fieldEncoding.
  ///
  /// In zh, this message translates to:
  /// **'编码'**
  String get fieldEncoding;

  /// No description provided for @fieldLcCollate.
  ///
  /// In zh, this message translates to:
  /// **'排序规则排序'**
  String get fieldLcCollate;

  /// No description provided for @fieldConnectionLimit.
  ///
  /// In zh, this message translates to:
  /// **'连接限制'**
  String get fieldConnectionLimit;

  /// No description provided for @infoValueNoLimit.
  ///
  /// In zh, this message translates to:
  /// **'无'**
  String get infoValueNoLimit;

  /// No description provided for @fieldTableType.
  ///
  /// In zh, this message translates to:
  /// **'Table Type'**
  String get fieldTableType;

  /// No description provided for @tableTypeRegular.
  ///
  /// In zh, this message translates to:
  /// **'常规'**
  String get tableTypeRegular;

  /// No description provided for @tableTypePartitioned.
  ///
  /// In zh, this message translates to:
  /// **'分区表'**
  String get tableTypePartitioned;

  /// No description provided for @tableTypeView.
  ///
  /// In zh, this message translates to:
  /// **'视图'**
  String get tableTypeView;

  /// No description provided for @tableTypeMatView.
  ///
  /// In zh, this message translates to:
  /// **'物化视图'**
  String get tableTypeMatView;

  /// No description provided for @tableTypeForeign.
  ///
  /// In zh, this message translates to:
  /// **'外部表'**
  String get tableTypeForeign;

  /// No description provided for @fieldPartitionOf.
  ///
  /// In zh, this message translates to:
  /// **'分区属于'**
  String get fieldPartitionOf;

  /// No description provided for @fieldInheritsFrom.
  ///
  /// In zh, this message translates to:
  /// **'Inherits From'**
  String get fieldInheritsFrom;

  /// No description provided for @fieldHasOids.
  ///
  /// In zh, this message translates to:
  /// **'Has OIDs'**
  String get fieldHasOids;

  /// No description provided for @fieldFillFactor.
  ///
  /// In zh, this message translates to:
  /// **'填充因子'**
  String get fieldFillFactor;

  /// No description provided for @fieldAcl.
  ///
  /// In zh, this message translates to:
  /// **'ACL'**
  String get fieldAcl;

  /// No description provided for @infoValueYes.
  ///
  /// In zh, this message translates to:
  /// **'是'**
  String get infoValueYes;

  /// No description provided for @infoValueNo.
  ///
  /// In zh, this message translates to:
  /// **'否'**
  String get infoValueNo;

  /// No description provided for @infoPageUses.
  ///
  /// In zh, this message translates to:
  /// **'使用'**
  String get infoPageUses;

  /// No description provided for @infoPageUsedBy.
  ///
  /// In zh, this message translates to:
  /// **'被使用'**
  String get infoPageUsedBy;

  /// No description provided for @infoPageUsesTooltip.
  ///
  /// In zh, this message translates to:
  /// **'本表所引用的对象'**
  String get infoPageUsesTooltip;

  /// No description provided for @infoPageUsedByTooltip.
  ///
  /// In zh, this message translates to:
  /// **'引用本表的对象'**
  String get infoPageUsedByTooltip;

  /// No description provided for @infoDepsLoading.
  ///
  /// In zh, this message translates to:
  /// **'正在读取依赖关系…'**
  String get infoDepsLoading;

  /// No description provided for @infoDepsEmpty.
  ///
  /// In zh, this message translates to:
  /// **'没有依赖对象'**
  String get infoDepsEmpty;

  /// No description provided for @infoDepsFailed.
  ///
  /// In zh, this message translates to:
  /// **'依赖读取失败:{error}'**
  String infoDepsFailed(String error);

  /// No description provided for @infoMaximizePanel.
  ///
  /// In zh, this message translates to:
  /// **'加宽详情面板'**
  String get infoMaximizePanel;

  /// No description provided for @infoRestorePanel.
  ///
  /// In zh, this message translates to:
  /// **'恢复面板宽度'**
  String get infoRestorePanel;

  /// No description provided for @infoDetailFailed.
  ///
  /// In zh, this message translates to:
  /// **'详情读取失败:{error}'**
  String infoDetailFailed(String error);

  /// No description provided for @infoDdlLoading.
  ///
  /// In zh, this message translates to:
  /// **'正在读取定义…'**
  String get infoDdlLoading;

  /// No description provided for @infoDdlUnsupported.
  ///
  /// In zh, this message translates to:
  /// **'该数据库类型暂不支持显示建表语句。'**
  String get infoDdlUnsupported;

  /// No description provided for @infoDdlFailed.
  ///
  /// In zh, this message translates to:
  /// **'DDL 读取失败:{error}'**
  String infoDdlFailed(String error);

  /// No description provided for @infoCopyDdl.
  ///
  /// In zh, this message translates to:
  /// **'复制建表语句'**
  String get infoCopyDdl;

  /// No description provided for @actionOpen.
  ///
  /// In zh, this message translates to:
  /// **'打开{label}'**
  String actionOpen(String label);

  /// No description provided for @actionDesign.
  ///
  /// In zh, this message translates to:
  /// **'设计{label}'**
  String actionDesign(String label);

  /// No description provided for @actionNew.
  ///
  /// In zh, this message translates to:
  /// **'新建{label}'**
  String actionNew(String label);

  /// No description provided for @actionDelete.
  ///
  /// In zh, this message translates to:
  /// **'删除{label}'**
  String actionDelete(String label);

  /// No description provided for @tableKindRegular.
  ///
  /// In zh, this message translates to:
  /// **'常规'**
  String get tableKindRegular;

  /// No description provided for @tableKindExternal.
  ///
  /// In zh, this message translates to:
  /// **'外部'**
  String get tableKindExternal;

  /// No description provided for @tableKindPartition.
  ///
  /// In zh, this message translates to:
  /// **'分区'**
  String get tableKindPartition;

  /// No description provided for @importWizard.
  ///
  /// In zh, this message translates to:
  /// **'导入向导'**
  String get importWizard;

  /// No description provided for @exportWizard.
  ///
  /// In zh, this message translates to:
  /// **'导出向导'**
  String get exportWizard;

  /// No description provided for @actionClear.
  ///
  /// In zh, this message translates to:
  /// **'清空{label}'**
  String actionClear(String label);

  /// No description provided for @clearConfirmOne.
  ///
  /// In zh, this message translates to:
  /// **'确定要清空{label}「{name}」吗?\n此操作会删除其中全部数据(保留结构),且不可恢复。'**
  String clearConfirmOne(String label, String name);

  /// No description provided for @clearFailedDetail.
  ///
  /// In zh, this message translates to:
  /// **'清空失败:\n{error}'**
  String clearFailedDetail(String error);

  /// No description provided for @btnClear.
  ///
  /// In zh, this message translates to:
  /// **'清空'**
  String get btnClear;

  /// No description provided for @ctxCopyRename.
  ///
  /// In zh, this message translates to:
  /// **'复制重命名'**
  String get ctxCopyRename;

  /// No description provided for @btnGotIt.
  ///
  /// In zh, this message translates to:
  /// **'知道了'**
  String get btnGotIt;

  /// No description provided for @btnDelete.
  ///
  /// In zh, this message translates to:
  /// **'删除'**
  String get btnDelete;

  /// No description provided for @btnPaste.
  ///
  /// In zh, this message translates to:
  /// **'粘贴'**
  String get btnPaste;

  /// No description provided for @btnRetry.
  ///
  /// In zh, this message translates to:
  /// **'重试'**
  String get btnRetry;

  /// No description provided for @stubWip.
  ///
  /// In zh, this message translates to:
  /// **'{action} 功能开发中,敬请期待'**
  String stubWip(String action);

  /// No description provided for @deleteQueryTitle.
  ///
  /// In zh, this message translates to:
  /// **'删除查询'**
  String get deleteQueryTitle;

  /// No description provided for @deleteQueryConfirmOne.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除查询「{name}」吗?\n删除后可重新保存,打开着的查询页不受影响。'**
  String deleteQueryConfirmOne(String name);

  /// No description provided for @deleteQueryConfirmMany.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除选中的 {count} 个查询吗?'**
  String deleteQueryConfirmMany(String count);

  /// No description provided for @deleteObjectConfirmOne.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除{label}「{name}」吗?\n此操作会永久删除该对象,且不可恢复。'**
  String deleteObjectConfirmOne(String label, String name);

  /// No description provided for @deleteObjectConfirmMany.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除选中的 {count} 个{label}吗?\n此操作会永久删除这些对象,且不可恢复。'**
  String deleteObjectConfirmMany(String count, String label);

  /// No description provided for @deleteFailedNames.
  ///
  /// In zh, this message translates to:
  /// **'删除失败:{names}\n请检查连接状态或对象是否存在。'**
  String deleteFailedNames(String names);

  /// No description provided for @loadingObjectsTitle.
  ///
  /// In zh, this message translates to:
  /// **'正在加载 {database} 的对象列表 ...'**
  String loadingObjectsTitle(String database);

  /// No description provided for @openDatabaseFailedTitle.
  ///
  /// In zh, this message translates to:
  /// **'打开 {database} 失败'**
  String openDatabaseFailedTitle(String database);

  /// No description provided for @categoryListFailedTitle.
  ///
  /// In zh, this message translates to:
  /// **'{label}列表读取失败'**
  String categoryListFailedTitle(String label);

  /// No description provided for @colName.
  ///
  /// In zh, this message translates to:
  /// **'名称'**
  String get colName;

  /// No description provided for @colRowsEstimated.
  ///
  /// In zh, this message translates to:
  /// **'行(估算)'**
  String get colRowsEstimated;

  /// No description provided for @colComment.
  ///
  /// In zh, this message translates to:
  /// **'注释'**
  String get colComment;

  /// No description provided for @rowsApprox.
  ///
  /// In zh, this message translates to:
  /// **'约{value}{unit}'**
  String rowsApprox(String value, String unit);

  /// No description provided for @rowsUnitSmall.
  ///
  /// In zh, this message translates to:
  /// **'万'**
  String get rowsUnitSmall;

  /// No description provided for @rowsUnitLarge.
  ///
  /// In zh, this message translates to:
  /// **'亿'**
  String get rowsUnitLarge;

  /// No description provided for @renameTableTitle.
  ///
  /// In zh, this message translates to:
  /// **'重命名表'**
  String get renameTableTitle;

  /// No description provided for @renameFailedDetail.
  ///
  /// In zh, this message translates to:
  /// **'重命名失败:\n{error}'**
  String renameFailedDetail(String error);

  /// No description provided for @copiedTables.
  ///
  /// In zh, this message translates to:
  /// **'已复制 {count} 张表,Ctrl+V 粘贴为副本'**
  String copiedTables(String count);

  /// No description provided for @pasteNeedOpenConnection.
  ///
  /// In zh, this message translates to:
  /// **'请先打开连接「{connection}」再粘贴。'**
  String pasteNeedOpenConnection(String connection);

  /// No description provided for @pasteWrongContext.
  ///
  /// In zh, this message translates to:
  /// **'粘贴只能回到复制时的连接 / 数据库 / 模式:\n{context}'**
  String pasteWrongContext(String context);

  /// No description provided for @pasteTableTitle.
  ///
  /// In zh, this message translates to:
  /// **'粘贴表'**
  String get pasteTableTitle;

  /// No description provided for @pasteConfirmDetail.
  ///
  /// In zh, this message translates to:
  /// **'将粘贴创建 {count} 张表(结构 + 数据):\n{plan}'**
  String pasteConfirmDetail(String count, String plan);

  /// No description provided for @pastedTables.
  ///
  /// In zh, this message translates to:
  /// **'已粘贴创建 {count} 张表'**
  String pastedTables(String count);

  /// No description provided for @pasteFailedDetail.
  ///
  /// In zh, this message translates to:
  /// **'粘贴失败:\n{detail}'**
  String pasteFailedDetail(String detail);

  /// No description provided for @catTablePlural.
  ///
  /// In zh, this message translates to:
  /// **'表'**
  String get catTablePlural;

  /// No description provided for @catViewPlural.
  ///
  /// In zh, this message translates to:
  /// **'视图'**
  String get catViewPlural;

  /// No description provided for @catMaterializedViewPlural.
  ///
  /// In zh, this message translates to:
  /// **'实体化视图'**
  String get catMaterializedViewPlural;

  /// No description provided for @catFunctionPlural.
  ///
  /// In zh, this message translates to:
  /// **'函数'**
  String get catFunctionPlural;

  /// No description provided for @catProcedurePlural.
  ///
  /// In zh, this message translates to:
  /// **'过程'**
  String get catProcedurePlural;

  /// No description provided for @newExternalTable.
  ///
  /// In zh, this message translates to:
  /// **'新建外部表'**
  String get newExternalTable;

  /// No description provided for @newPartitionTable.
  ///
  /// In zh, this message translates to:
  /// **'新建分区表'**
  String get newPartitionTable;

  /// No description provided for @openedConnection.
  ///
  /// In zh, this message translates to:
  /// **'已打开连接「{name}」'**
  String openedConnection(String name);

  /// No description provided for @openedDatabase.
  ///
  /// In zh, this message translates to:
  /// **'已打开数据库「{name}」'**
  String openedDatabase(String name);

  /// No description provided for @openedSchema.
  ///
  /// In zh, this message translates to:
  /// **'已打开模式「{name}」'**
  String openedSchema(String name);

  /// No description provided for @openedBare.
  ///
  /// In zh, this message translates to:
  /// **'已打开'**
  String get openedBare;

  /// No description provided for @closedBare.
  ///
  /// In zh, this message translates to:
  /// **'已关闭'**
  String get closedBare;

  /// No description provided for @closedConnection.
  ///
  /// In zh, this message translates to:
  /// **'已关闭连接「{name}」'**
  String closedConnection(String name);

  /// No description provided for @closedSchema.
  ///
  /// In zh, this message translates to:
  /// **'已关闭模式「{name}」'**
  String closedSchema(String name);

  /// No description provided for @closedDatabase.
  ///
  /// In zh, this message translates to:
  /// **'已关闭数据库「{name}」'**
  String closedDatabase(String name);

  /// No description provided for @openedNamed.
  ///
  /// In zh, this message translates to:
  /// **'已打开「{name}」'**
  String openedNamed(String name);

  /// No description provided for @closedNamed.
  ///
  /// In zh, this message translates to:
  /// **'已关闭「{name}」'**
  String closedNamed(String name);

  /// No description provided for @renamedConnection.
  ///
  /// In zh, this message translates to:
  /// **'已重命名连接「{oldName}」为「{newName}」'**
  String renamedConnection(String newName, String oldName);

  /// No description provided for @renamedTable.
  ///
  /// In zh, this message translates to:
  /// **'已重命名表「{oldName}」为「{newName}」'**
  String renamedTable(String newName, String oldName);

  /// No description provided for @movedConnectionToUngrouped.
  ///
  /// In zh, this message translates to:
  /// **'已把连接「{name}」移到未分组'**
  String movedConnectionToUngrouped(String name);

  /// No description provided for @movedConnectionToGroup.
  ///
  /// In zh, this message translates to:
  /// **'已把连接「{name}」移入分组「{group}」'**
  String movedConnectionToGroup(String group, String name);

  /// No description provided for @unnamedGroup.
  ///
  /// In zh, this message translates to:
  /// **'未命名分组'**
  String get unnamedGroup;

  /// No description provided for @unnamedGroupNumbered.
  ///
  /// In zh, this message translates to:
  /// **'未命名分组 {index}'**
  String unnamedGroupNumbered(String index);

  /// No description provided for @renameGroupTitle.
  ///
  /// In zh, this message translates to:
  /// **'重命名分组'**
  String get renameGroupTitle;

  /// No description provided for @groupAlreadyExists.
  ///
  /// In zh, this message translates to:
  /// **'已存在同名分组「{name}」(不区分大小写)。'**
  String groupAlreadyExists(String name);

  /// No description provided for @ctxOpenConnection.
  ///
  /// In zh, this message translates to:
  /// **'打开连接'**
  String get ctxOpenConnection;

  /// No description provided for @ctxCloseConnection.
  ///
  /// In zh, this message translates to:
  /// **'关闭连接'**
  String get ctxCloseConnection;

  /// No description provided for @ctxOpen.
  ///
  /// In zh, this message translates to:
  /// **'打开'**
  String get ctxOpen;

  /// No description provided for @ctxClose.
  ///
  /// In zh, this message translates to:
  /// **'关闭'**
  String get ctxClose;

  /// No description provided for @ctxRefresh.
  ///
  /// In zh, this message translates to:
  /// **'刷新'**
  String get ctxRefresh;

  /// No description provided for @ctxNewDatabase.
  ///
  /// In zh, this message translates to:
  /// **'新建数据库'**
  String get ctxNewDatabase;

  /// No description provided for @ctxEditConnection.
  ///
  /// In zh, this message translates to:
  /// **'编辑连接'**
  String get ctxEditConnection;

  /// No description provided for @ctxCopyConnection.
  ///
  /// In zh, this message translates to:
  /// **'复制连接'**
  String get ctxCopyConnection;

  /// No description provided for @ctxMoveToGroup.
  ///
  /// In zh, this message translates to:
  /// **'移动到分组'**
  String get ctxMoveToGroup;

  /// No description provided for @ctxUngrouped.
  ///
  /// In zh, this message translates to:
  /// **'未分组'**
  String get ctxUngrouped;

  /// No description provided for @ctxNewGroup.
  ///
  /// In zh, this message translates to:
  /// **'新建分组'**
  String get ctxNewGroup;

  /// No description provided for @ctxDeleteConnection.
  ///
  /// In zh, this message translates to:
  /// **'删除连接'**
  String get ctxDeleteConnection;

  /// No description provided for @ctxNewConnectionEllipsis.
  ///
  /// In zh, this message translates to:
  /// **'新建连接…'**
  String get ctxNewConnectionEllipsis;

  /// No description provided for @ctxRenameGroup.
  ///
  /// In zh, this message translates to:
  /// **'重命名分组'**
  String get ctxRenameGroup;

  /// No description provided for @ctxDeleteGroup.
  ///
  /// In zh, this message translates to:
  /// **'删除分组'**
  String get ctxDeleteGroup;

  /// No description provided for @ctxDeleteGroupWith.
  ///
  /// In zh, this message translates to:
  /// **'删除分组(含 {count} 条连接)'**
  String ctxDeleteGroupWith(String count);

  /// No description provided for @deleteGroupConfirm.
  ///
  /// In zh, this message translates to:
  /// **'删除分组「{group}」不会删除其中的 {count} 条连接,它们会回落到未分组。继续?'**
  String deleteGroupConfirm(String count, String group);

  /// No description provided for @btnCancel.
  ///
  /// In zh, this message translates to:
  /// **'取消'**
  String get btnCancel;

  /// No description provided for @btnSave.
  ///
  /// In zh, this message translates to:
  /// **'保存'**
  String get btnSave;

  /// No description provided for @ctxNewSchema.
  ///
  /// In zh, this message translates to:
  /// **'新建模式'**
  String get ctxNewSchema;

  /// No description provided for @ctxDelete.
  ///
  /// In zh, this message translates to:
  /// **'删除'**
  String get ctxDelete;

  /// No description provided for @ctxEditDatabase.
  ///
  /// In zh, this message translates to:
  /// **'编辑数据库'**
  String get ctxEditDatabase;

  /// No description provided for @ctxNewQuery.
  ///
  /// In zh, this message translates to:
  /// **'新建查询'**
  String get ctxNewQuery;

  /// No description provided for @ctxDumpSql.
  ///
  /// In zh, this message translates to:
  /// **'转储SQL文件'**
  String get ctxDumpSql;

  /// No description provided for @ctxStructureOnly.
  ///
  /// In zh, this message translates to:
  /// **'仅结构'**
  String get ctxStructureOnly;

  /// No description provided for @ctxRunSql.
  ///
  /// In zh, this message translates to:
  /// **'运行SQL文件'**
  String get ctxRunSql;

  /// No description provided for @ctxCloseSchema.
  ///
  /// In zh, this message translates to:
  /// **'关闭模式'**
  String get ctxCloseSchema;

  /// No description provided for @ctxOpenSchema.
  ///
  /// In zh, this message translates to:
  /// **'打开模式'**
  String get ctxOpenSchema;

  /// No description provided for @ctxEditSchema.
  ///
  /// In zh, this message translates to:
  /// **'编辑模式'**
  String get ctxEditSchema;

  /// No description provided for @ctxDeleteSchema.
  ///
  /// In zh, this message translates to:
  /// **'删除模式'**
  String get ctxDeleteSchema;

  /// No description provided for @deleteSchemaConfirm.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除模式「{name}」吗?\n此操作会永久删除该模式及其全部对象,且不可恢复。'**
  String deleteSchemaConfirm(String name);

  /// No description provided for @deleteFailedDetail.
  ///
  /// In zh, this message translates to:
  /// **'删除失败:\n{error}'**
  String deleteFailedDetail(String error);

  /// No description provided for @ctxNewTable.
  ///
  /// In zh, this message translates to:
  /// **'新建表'**
  String get ctxNewTable;

  /// No description provided for @ctxNewView.
  ///
  /// In zh, this message translates to:
  /// **'新建视图'**
  String get ctxNewView;

  /// No description provided for @ctxNewFunction.
  ///
  /// In zh, this message translates to:
  /// **'新建函数'**
  String get ctxNewFunction;

  /// No description provided for @ctxNewProcedure.
  ///
  /// In zh, this message translates to:
  /// **'新建过程'**
  String get ctxNewProcedure;

  /// No description provided for @deleteDatabaseTitle.
  ///
  /// In zh, this message translates to:
  /// **'删除数据库'**
  String get deleteDatabaseTitle;

  /// No description provided for @deleteDatabaseConfirm.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除数据库「{name}」吗?\n此操作会永久删除该数据库及其所有数据,且不可恢复。'**
  String deleteDatabaseConfirm(String name);

  /// No description provided for @sqlFileTypeLabel.
  ///
  /// In zh, this message translates to:
  /// **'SQL 文件'**
  String get sqlFileTypeLabel;

  /// No description provided for @dumpStructureReadFailed.
  ///
  /// In zh, this message translates to:
  /// **'结构读取失败:\n数据库不可用或连接已断开,请先打开连接重试。'**
  String get dumpStructureReadFailed;

  /// No description provided for @dumpWriteFailed.
  ///
  /// In zh, this message translates to:
  /// **'文件写入失败:\n{error}'**
  String dumpWriteFailed(String error);

  /// No description provided for @dumpDatabaseDone.
  ///
  /// In zh, this message translates to:
  /// **'已导出「{name}」结构(仅结构,不含数据)到:\n{path}'**
  String dumpDatabaseDone(String name, String path);

  /// No description provided for @dumpSchemaDone.
  ///
  /// In zh, this message translates to:
  /// **'已导出「{name}」模式结构(仅结构,不含数据)到:\n{path}'**
  String dumpSchemaDone(String name, String path);

  /// No description provided for @openConnectionUnsupported.
  ///
  /// In zh, this message translates to:
  /// **'连接「{name}」的数据库类型({type})暂不支持,无法打开。'**
  String openConnectionUnsupported(String name, String type);

  /// No description provided for @openConnectionFailed.
  ///
  /// In zh, this message translates to:
  /// **'连接「{name}」失败:\n{error}'**
  String openConnectionFailed(String error, String name);

  /// No description provided for @deleteConnectionConfirm.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除连接「{name}」吗?\n已打开的该连接标签页仍会保留,但将无法继续访问。'**
  String deleteConnectionConfirm(String name);

  /// No description provided for @noMatchingConnections.
  ///
  /// In zh, this message translates to:
  /// **'没有匹配的连接'**
  String get noMatchingConnections;

  /// No description provided for @noConnectionsYet.
  ///
  /// In zh, this message translates to:
  /// **'暂无连接'**
  String get noConnectionsYet;

  /// No description provided for @clickToolbarNewConnection.
  ///
  /// In zh, this message translates to:
  /// **'点击工具栏「连接」按钮新建连接'**
  String get clickToolbarNewConnection;

  /// No description provided for @dropToUngroup.
  ///
  /// In zh, this message translates to:
  /// **'释放以移到「未分组」'**
  String get dropToUngroup;

  /// No description provided for @driverNotImplemented.
  ///
  /// In zh, this message translates to:
  /// **'暂不支持该类型,待实现驱动'**
  String get driverNotImplemented;

  /// No description provided for @loadFailedDetail.
  ///
  /// In zh, this message translates to:
  /// **'加载失败: {error}'**
  String loadFailedDetail(String error);

  /// No description provided for @readFailed.
  ///
  /// In zh, this message translates to:
  /// **'读取失败'**
  String get readFailed;

  /// No description provided for @clickToRetry.
  ///
  /// In zh, this message translates to:
  /// **'点击重试'**
  String get clickToRetry;

  /// No description provided for @searchConnectionsHint.
  ///
  /// In zh, this message translates to:
  /// **'搜索连接...'**
  String get searchConnectionsHint;

  /// No description provided for @dbTypeFilter.
  ///
  /// In zh, this message translates to:
  /// **'数据库类型筛选'**
  String get dbTypeFilter;

  /// No description provided for @notImplemented.
  ///
  /// In zh, this message translates to:
  /// **'未实现'**
  String get notImplemented;

  /// No description provided for @clearAllFilters.
  ///
  /// In zh, this message translates to:
  /// **'全部清除'**
  String get clearAllFilters;

  /// No description provided for @collapseAll.
  ///
  /// In zh, this message translates to:
  /// **'折叠全部'**
  String get collapseAll;

  /// 表数据页:单元格编辑器 / 值面板未选中任何格时的占位提示
  ///
  /// In zh, this message translates to:
  /// **'请先选中一个单元格'**
  String get cellEditorPickCell;

  /// 表数据页:把本地暂存的增删改写回数据库的按钮
  ///
  /// In zh, this message translates to:
  /// **'确认修改'**
  String get btnCommitChanges;

  /// No description provided for @rowsCopiedToClipboard.
  ///
  /// In zh, this message translates to:
  /// **'已复制 {count} 行到剪贴板'**
  String rowsCopiedToClipboard(String count);

  /// No description provided for @cellMenuSetNull.
  ///
  /// In zh, this message translates to:
  /// **'将 {count} 个单元格置为 NULL'**
  String cellMenuSetNull(String count);

  /// No description provided for @cellMenuCopy.
  ///
  /// In zh, this message translates to:
  /// **'复制 {count} 个单元格'**
  String cellMenuCopy(String count);

  /// No description provided for @cellsCopiedToClipboard.
  ///
  /// In zh, this message translates to:
  /// **'已复制 {count} 个单元格到剪贴板'**
  String cellsCopiedToClipboard(String count);

  /// No description provided for @cellsClearedToNull.
  ///
  /// In zh, this message translates to:
  /// **'已将 {count} 个单元格置为 NULL（点「{action}」或 Ctrl+S 落库）'**
  String cellsClearedToNull(String count, String action);

  /// No description provided for @filterSourceBuilder.
  ///
  /// In zh, this message translates to:
  /// **'创建工具'**
  String get filterSourceBuilder;

  /// No description provided for @filterSourceText.
  ///
  /// In zh, this message translates to:
  /// **'文本'**
  String get filterSourceText;

  /// No description provided for @toolPanelFilter.
  ///
  /// In zh, this message translates to:
  /// **'筛选 & 排序'**
  String get toolPanelFilter;

  /// No description provided for @toolPanelColumns.
  ///
  /// In zh, this message translates to:
  /// **'列'**
  String get toolPanelColumns;

  /// No description provided for @toolPanelCellEditor.
  ///
  /// In zh, this message translates to:
  /// **'单元格编辑器'**
  String get toolPanelCellEditor;

  /// No description provided for @btnOk.
  ///
  /// In zh, this message translates to:
  /// **'确定'**
  String get btnOk;

  /// No description provided for @dtpSelectTime.
  ///
  /// In zh, this message translates to:
  /// **'选择时间'**
  String get dtpSelectTime;

  /// No description provided for @dtpToday.
  ///
  /// In zh, this message translates to:
  /// **'今天'**
  String get dtpToday;

  /// No description provided for @dtpMonth1.
  ///
  /// In zh, this message translates to:
  /// **'1月'**
  String get dtpMonth1;

  /// No description provided for @dtpMonth2.
  ///
  /// In zh, this message translates to:
  /// **'2月'**
  String get dtpMonth2;

  /// No description provided for @dtpMonth3.
  ///
  /// In zh, this message translates to:
  /// **'3月'**
  String get dtpMonth3;

  /// No description provided for @dtpMonth4.
  ///
  /// In zh, this message translates to:
  /// **'4月'**
  String get dtpMonth4;

  /// No description provided for @dtpMonth5.
  ///
  /// In zh, this message translates to:
  /// **'5月'**
  String get dtpMonth5;

  /// No description provided for @dtpMonth6.
  ///
  /// In zh, this message translates to:
  /// **'6月'**
  String get dtpMonth6;

  /// No description provided for @dtpMonth7.
  ///
  /// In zh, this message translates to:
  /// **'7月'**
  String get dtpMonth7;

  /// No description provided for @dtpMonth8.
  ///
  /// In zh, this message translates to:
  /// **'8月'**
  String get dtpMonth8;

  /// No description provided for @dtpMonth9.
  ///
  /// In zh, this message translates to:
  /// **'9月'**
  String get dtpMonth9;

  /// No description provided for @dtpMonth10.
  ///
  /// In zh, this message translates to:
  /// **'10月'**
  String get dtpMonth10;

  /// No description provided for @dtpMonth11.
  ///
  /// In zh, this message translates to:
  /// **'11月'**
  String get dtpMonth11;

  /// No description provided for @dtpMonth12.
  ///
  /// In zh, this message translates to:
  /// **'12月'**
  String get dtpMonth12;

  /// 日历日视图顶栏标题模板。[year] / [month] / [monthName] 是必须原样保留的标记，由宿主换成花括号后交给 base-ui 在渲染时替换
  ///
  /// In zh, this message translates to:
  /// **'[year]年[month]月'**
  String get dtpMonthTitle;

  /// No description provided for @dtpYearTitle.
  ///
  /// In zh, this message translates to:
  /// **'[year]年'**
  String get dtpYearTitle;

  /// No description provided for @dtpYearRangeTitle.
  ///
  /// In zh, this message translates to:
  /// **'[from] - [to]'**
  String get dtpYearRangeTitle;

  /// No description provided for @dtpWeekdayMon.
  ///
  /// In zh, this message translates to:
  /// **'周一'**
  String get dtpWeekdayMon;

  /// No description provided for @dtpWeekdayTue.
  ///
  /// In zh, this message translates to:
  /// **'周二'**
  String get dtpWeekdayTue;

  /// No description provided for @dtpWeekdayWed.
  ///
  /// In zh, this message translates to:
  /// **'周三'**
  String get dtpWeekdayWed;

  /// No description provided for @dtpWeekdayThu.
  ///
  /// In zh, this message translates to:
  /// **'周四'**
  String get dtpWeekdayThu;

  /// No description provided for @dtpWeekdayFri.
  ///
  /// In zh, this message translates to:
  /// **'周五'**
  String get dtpWeekdayFri;

  /// No description provided for @dtpWeekdaySat.
  ///
  /// In zh, this message translates to:
  /// **'周六'**
  String get dtpWeekdaySat;

  /// No description provided for @dtpWeekdaySun.
  ///
  /// In zh, this message translates to:
  /// **'周日'**
  String get dtpWeekdaySun;

  /// No description provided for @catRecord.
  ///
  /// In zh, this message translates to:
  /// **'记录'**
  String get catRecord;

  /// No description provided for @gridPagingFailed.
  ///
  /// In zh, this message translates to:
  /// **'分页加载失败: {error}'**
  String gridPagingFailed(String error);

  /// No description provided for @gridAlreadyLastPage.
  ///
  /// In zh, this message translates to:
  /// **'已是最后一页'**
  String get gridAlreadyLastPage;

  /// No description provided for @gridPageMissing.
  ///
  /// In zh, this message translates to:
  /// **'第 {page} 页不存在'**
  String gridPageMissing(String page);

  /// No description provided for @gridDiscardTitle.
  ///
  /// In zh, this message translates to:
  /// **'放弃未保存的修改'**
  String get gridDiscardTitle;

  /// No description provided for @gridDiscardConfirm.
  ///
  /// In zh, this message translates to:
  /// **'当前有未保存的修改，继续将丢弃这些修改。\n是否继续？'**
  String get gridDiscardConfirm;

  /// No description provided for @gridDeleteRowConfirm.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除第 {row} 行记录吗?\n'**
  String gridDeleteRowConfirm(String row);

  /// No description provided for @gridDeleteRowsConfirm.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除选中的 {count} 行记录吗?({preview})\n'**
  String gridDeleteRowsConfirm(String count, String preview);

  /// No description provided for @gridDeletePendingHint.
  ///
  /// In zh, this message translates to:
  /// **'删除后点击「确认修改」或 Ctrl+S 才会写入数据库。'**
  String get gridDeletePendingHint;

  /// No description provided for @gridNothingToSave.
  ///
  /// In zh, this message translates to:
  /// **'没有需要保存的修改'**
  String get gridNothingToSave;

  /// No description provided for @gridConnectionMissing.
  ///
  /// In zh, this message translates to:
  /// **'连接 \"{connection}\" 不存在'**
  String gridConnectionMissing(String connection);

  /// No description provided for @gridDeleteRowError.
  ///
  /// In zh, this message translates to:
  /// **'删除第 {row} 行: {error}'**
  String gridDeleteRowError(String row, String error);

  /// No description provided for @gridInsertRowError.
  ///
  /// In zh, this message translates to:
  /// **'新增行: {error}'**
  String gridInsertRowError(String error);

  /// No description provided for @gridUpdateRowError.
  ///
  /// In zh, this message translates to:
  /// **'更新第 {row} 行: {error}'**
  String gridUpdateRowError(String row, String error);

  /// No description provided for @gridSavedRows.
  ///
  /// In zh, this message translates to:
  /// **'已保存 {count} 行修改'**
  String gridSavedRows(String count);

  /// No description provided for @gridSaveFailed.
  ///
  /// In zh, this message translates to:
  /// **'保存失败: {errors}'**
  String gridSaveFailed(String errors);

  /// No description provided for @gridSaveFailedTitle.
  ///
  /// In zh, this message translates to:
  /// **'保存失败'**
  String get gridSaveFailedTitle;

  /// No description provided for @gridSortMethod.
  ///
  /// In zh, this message translates to:
  /// **'排序方式'**
  String get gridSortMethod;

  /// No description provided for @gridAddSortCriterion.
  ///
  /// In zh, this message translates to:
  /// **'添加排序准则'**
  String get gridAddSortCriterion;

  /// No description provided for @gridSortEmptyHint.
  ///
  /// In zh, this message translates to:
  /// **'点击 + 以添加排序准则'**
  String get gridSortEmptyHint;

  /// No description provided for @gridReadingColumns.
  ///
  /// In zh, this message translates to:
  /// **'正在读取列信息 …'**
  String get gridReadingColumns;

  /// No description provided for @gridFilterEmptyHint.
  ///
  /// In zh, this message translates to:
  /// **'点击 + 添加筛选条件；选中一行后可在其行尾追加同级条件（+）或括号分组（O+）'**
  String get gridFilterEmptyHint;

  /// No description provided for @gridFilter.
  ///
  /// In zh, this message translates to:
  /// **'筛选'**
  String get gridFilter;

  /// No description provided for @gridAddFilterCriterion.
  ///
  /// In zh, this message translates to:
  /// **'添加筛选条件'**
  String get gridAddFilterCriterion;

  /// No description provided for @gridMoveCriterionUp.
  ///
  /// In zh, this message translates to:
  /// **'上移选中条件'**
  String get gridMoveCriterionUp;

  /// No description provided for @gridMoveCriterionDown.
  ///
  /// In zh, this message translates to:
  /// **'下移选中条件'**
  String get gridMoveCriterionDown;

  /// No description provided for @gridNoValueNeeded.
  ///
  /// In zh, this message translates to:
  /// **'（无需值）'**
  String get gridNoValueNeeded;

  /// No description provided for @gridAddSibling.
  ///
  /// In zh, this message translates to:
  /// **'在此条件后添加同级条件'**
  String get gridAddSibling;

  /// No description provided for @gridAddGroup.
  ///
  /// In zh, this message translates to:
  /// **'在此条件后添加括号分组'**
  String get gridAddGroup;

  /// No description provided for @gridDeleteFilterGroup.
  ///
  /// In zh, this message translates to:
  /// **'删除分组'**
  String get gridDeleteFilterGroup;

  /// No description provided for @gridDeleteCriterion.
  ///
  /// In zh, this message translates to:
  /// **'删除条件'**
  String get gridDeleteCriterion;

  /// No description provided for @gridWhereHint.
  ///
  /// In zh, this message translates to:
  /// **'不含 WHERE 关键字，例如：id > 100 AND name LIKE \'集团%\''**
  String get gridWhereHint;

  /// No description provided for @gridApplyFilterSort.
  ///
  /// In zh, this message translates to:
  /// **'应用筛选 & 排序'**
  String get gridApplyFilterSort;

  /// No description provided for @gridCriterionEdited.
  ///
  /// In zh, this message translates to:
  /// **'已编辑准则'**
  String get gridCriterionEdited;

  /// No description provided for @gridAsc.
  ///
  /// In zh, this message translates to:
  /// **'升序'**
  String get gridAsc;

  /// No description provided for @gridDesc.
  ///
  /// In zh, this message translates to:
  /// **'降序'**
  String get gridDesc;

  /// No description provided for @gridDeleteSortCriterion.
  ///
  /// In zh, this message translates to:
  /// **'删除排序准则'**
  String get gridDeleteSortCriterion;

  /// No description provided for @gridColumnsCount.
  ///
  /// In zh, this message translates to:
  /// **'列 ({visible}/{total})'**
  String gridColumnsCount(String visible, String total);

  /// No description provided for @gridShowAllColumns.
  ///
  /// In zh, this message translates to:
  /// **'显示所有列'**
  String get gridShowAllColumns;

  /// No description provided for @gridKeepFirstColumnOnly.
  ///
  /// In zh, this message translates to:
  /// **'只保留第一列'**
  String get gridKeepFirstColumnOnly;

  /// No description provided for @gridLoadingColumns.
  ///
  /// In zh, this message translates to:
  /// **'正在加载列信息 ...'**
  String get gridLoadingColumns;

  /// No description provided for @gridSearch.
  ///
  /// In zh, this message translates to:
  /// **'搜索'**
  String get gridSearch;

  /// No description provided for @gridColumnName.
  ///
  /// In zh, this message translates to:
  /// **'列名'**
  String get gridColumnName;

  /// No description provided for @cellNoneSelected.
  ///
  /// In zh, this message translates to:
  /// **'未选中单元格'**
  String get cellNoneSelected;

  /// No description provided for @cellColumnIndex.
  ///
  /// In zh, this message translates to:
  /// **'列 {col}'**
  String cellColumnIndex(String col);

  /// No description provided for @cellRowIndex.
  ///
  /// In zh, this message translates to:
  /// **'  ·  第 {row} 行'**
  String cellRowIndex(String row);

  /// No description provided for @cellEditorTitle.
  ///
  /// In zh, this message translates to:
  /// **'单元格编辑器 · {title}'**
  String cellEditorTitle(String title);

  /// No description provided for @btnApply.
  ///
  /// In zh, this message translates to:
  /// **'应用'**
  String get btnApply;

  /// No description provided for @btnUndo.
  ///
  /// In zh, this message translates to:
  /// **'撤销'**
  String get btnUndo;

  /// No description provided for @cellEmptyValue.
  ///
  /// In zh, this message translates to:
  /// **'当前单元格为空'**
  String get cellEmptyValue;

  /// No description provided for @cellNotBase64Image.
  ///
  /// In zh, this message translates to:
  /// **'当前单元格不是可识别的 base64 图片数据'**
  String get cellNotBase64Image;

  /// No description provided for @cellNotHtml.
  ///
  /// In zh, this message translates to:
  /// **'当前单元格内容不是 HTML 网页源码'**
  String get cellNotHtml;

  /// No description provided for @cellWrittenBack.
  ///
  /// In zh, this message translates to:
  /// **'已写回单元格（点「{action}」或 Ctrl+S 落库）'**
  String cellWrittenBack(String action);

  /// No description provided for @cellMenuSetBlank.
  ///
  /// In zh, this message translates to:
  /// **'设置为空白字符串'**
  String get cellMenuSetBlank;

  /// No description provided for @cellMenuSetNullCell.
  ///
  /// In zh, this message translates to:
  /// **'设置为 NULL'**
  String get cellMenuSetNullCell;

  /// No description provided for @gridSort.
  ///
  /// In zh, this message translates to:
  /// **'排序'**
  String get gridSort;

  /// No description provided for @gridSortAscBy.
  ///
  /// In zh, this message translates to:
  /// **'升序（{column}）'**
  String gridSortAscBy(String column);

  /// No description provided for @gridSortDescBy.
  ///
  /// In zh, this message translates to:
  /// **'降序（{column}）'**
  String gridSortDescBy(String column);

  /// No description provided for @gridClearSort.
  ///
  /// In zh, this message translates to:
  /// **'取消排序'**
  String get gridClearSort;

  /// No description provided for @gridMoreSorting.
  ///
  /// In zh, this message translates to:
  /// **'更多排序...'**
  String get gridMoreSorting;

  /// No description provided for @gridHideColumn.
  ///
  /// In zh, this message translates to:
  /// **'隐藏「{column}」'**
  String gridHideColumn(String column);

  /// No description provided for @gridColumnsPanel.
  ///
  /// In zh, this message translates to:
  /// **'列面板...'**
  String get gridColumnsPanel;

  /// No description provided for @gridMoreFilters.
  ///
  /// In zh, this message translates to:
  /// **'更多筛选...'**
  String get gridMoreFilters;

  /// No description provided for @gridClearFilter.
  ///
  /// In zh, this message translates to:
  /// **'清除筛选'**
  String get gridClearFilter;

  /// No description provided for @gridRemoveAllSortFilter.
  ///
  /// In zh, this message translates to:
  /// **'移除所有排序及筛选'**
  String get gridRemoveAllSortFilter;

  /// No description provided for @gridShow.
  ///
  /// In zh, this message translates to:
  /// **'显示'**
  String get gridShow;

  /// No description provided for @gridShowAll.
  ///
  /// In zh, this message translates to:
  /// **'全部记录'**
  String get gridShowAll;

  /// No description provided for @gridShowNullOnly.
  ///
  /// In zh, this message translates to:
  /// **'仅含 NULL 值的记录'**
  String get gridShowNullOnly;

  /// No description provided for @gridShowNotNullOnly.
  ///
  /// In zh, this message translates to:
  /// **'仅不含 NULL 值的记录'**
  String get gridShowNotNullOnly;

  /// No description provided for @gridClipboardEmpty.
  ///
  /// In zh, this message translates to:
  /// **'剪贴板为空'**
  String get gridClipboardEmpty;

  /// No description provided for @gridClipboardNoRecords.
  ///
  /// In zh, this message translates to:
  /// **'剪贴板里没有可粘贴的记录'**
  String get gridClipboardNoRecords;

  /// No description provided for @gridPastedRows.
  ///
  /// In zh, this message translates to:
  /// **'已粘贴 {count} 行为新增记录(未保存)'**
  String gridPastedRows(String count);

  /// No description provided for @gridRowCountUnknown.
  ///
  /// In zh, this message translates to:
  /// **'  ·  行数未知'**
  String get gridRowCountUnknown;

  /// No description provided for @gridRowCount.
  ///
  /// In zh, this message translates to:
  /// **'  ·  {total} 行'**
  String gridRowCount(String total);

  /// No description provided for @gridLoadingTable.
  ///
  /// In zh, this message translates to:
  /// **'正在加载 {table} ...'**
  String gridLoadingTable(String table);

  /// No description provided for @gridReadTableFailed.
  ///
  /// In zh, this message translates to:
  /// **'读取 {table} 失败'**
  String gridReadTableFailed(String table);

  /// No description provided for @gridFilterDirty.
  ///
  /// In zh, this message translates to:
  /// **'筛选 / 排序有未应用的更改'**
  String get gridFilterDirty;

  /// No description provided for @gridFilterApplied.
  ///
  /// In zh, this message translates to:
  /// **'已应用筛选 / 排序'**
  String get gridFilterApplied;

  /// No description provided for @gridAddRecord.
  ///
  /// In zh, this message translates to:
  /// **'添加记录'**
  String get gridAddRecord;

  /// No description provided for @gridDeleteSelectedRecords.
  ///
  /// In zh, this message translates to:
  /// **'删除选中记录'**
  String get gridDeleteSelectedRecords;

  /// No description provided for @gridSaving.
  ///
  /// In zh, this message translates to:
  /// **'保存中...'**
  String get gridSaving;

  /// No description provided for @gridRevertChanges.
  ///
  /// In zh, this message translates to:
  /// **'取消修改'**
  String get gridRevertChanges;

  /// No description provided for @gridStop.
  ///
  /// In zh, this message translates to:
  /// **'停止'**
  String get gridStop;

  /// No description provided for @gridReadOnlyBrowse.
  ///
  /// In zh, this message translates to:
  /// **'该连接为只读浏览,数据不可在网格中编辑或保存。'**
  String get gridReadOnlyBrowse;

  /// No description provided for @gridPagerRange.
  ///
  /// In zh, this message translates to:
  /// **'第 {from}-{to} 条 / 共 {total} 条'**
  String gridPagerRange(String from, String to, String total);

  /// No description provided for @gridPageSize.
  ///
  /// In zh, this message translates to:
  /// **'页大小设置'**
  String get gridPageSize;

  /// No description provided for @gridRowsPerPage.
  ///
  /// In zh, this message translates to:
  /// **'{size} 条/页'**
  String gridRowsPerPage(String size);

  /// No description provided for @gridRowsPerPageCurrent.
  ///
  /// In zh, this message translates to:
  /// **'{size} 条/页 ✓'**
  String gridRowsPerPageCurrent(String size);

  /// No description provided for @gridConnectionGone.
  ///
  /// In zh, this message translates to:
  /// **'连接「{connection}」已不存在,请先打开连接。'**
  String gridConnectionGone(String connection);

  /// No description provided for @gridDeleteRecordTitle.
  ///
  /// In zh, this message translates to:
  /// **'删除记录'**
  String get gridDeleteRecordTitle;

  /// No description provided for @gridDeleteRecordPending.
  ///
  /// In zh, this message translates to:
  /// **'删除后点击「{action}」或 Ctrl+S 才会写入数据库。'**
  String gridDeleteRecordPending(String action);

  /// No description provided for @gridDeleteRowDetail.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除第 {row} 行记录吗?\n{tail}'**
  String gridDeleteRowDetail(String row, String tail);

  /// No description provided for @gridDeleteRowsDetail.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除选中的 {count} 行记录吗?({preview})\n{tail}'**
  String gridDeleteRowsDetail(String count, String preview, String tail);

  /// No description provided for @gridUpdateRowErrorAt.
  ///
  /// In zh, this message translates to:
  /// **'更新第 {row} 行: {error}'**
  String gridUpdateRowErrorAt(String row, String error);

  /// No description provided for @gridSortDirection.
  ///
  /// In zh, this message translates to:
  /// **'排序方向'**
  String get gridSortDirection;

  /// No description provided for @gridSortAsc.
  ///
  /// In zh, this message translates to:
  /// **'升序'**
  String get gridSortAsc;

  /// No description provided for @gridSortDesc.
  ///
  /// In zh, this message translates to:
  /// **'降序'**
  String get gridSortDesc;

  /// No description provided for @gridJoinAnd.
  ///
  /// In zh, this message translates to:
  /// **'且'**
  String get gridJoinAnd;

  /// No description provided for @gridJoinOr.
  ///
  /// In zh, this message translates to:
  /// **'或'**
  String get gridJoinOr;

  /// No description provided for @gridFilterValuePlaceholder.
  ///
  /// In zh, this message translates to:
  /// **'<?>'**
  String get gridFilterValuePlaceholder;

  /// No description provided for @gridSortTitle.
  ///
  /// In zh, this message translates to:
  /// **'排序方式'**
  String get gridSortTitle;

  /// No description provided for @gridAddSortHint.
  ///
  /// In zh, this message translates to:
  /// **'点击 + 以添加排序准则'**
  String get gridAddSortHint;

  /// No description provided for @gridAddSortCriterionTitle.
  ///
  /// In zh, this message translates to:
  /// **'添加排序准则'**
  String get gridAddSortCriterionTitle;

  /// No description provided for @gridDeleteSortCriterionTitle.
  ///
  /// In zh, this message translates to:
  /// **'删除排序准则'**
  String get gridDeleteSortCriterionTitle;

  /// No description provided for @gridFilterTitle.
  ///
  /// In zh, this message translates to:
  /// **'筛选'**
  String get gridFilterTitle;

  /// No description provided for @gridAddFilterCriterionTitle.
  ///
  /// In zh, this message translates to:
  /// **'添加筛选条件'**
  String get gridAddFilterCriterionTitle;

  /// No description provided for @gridMoveCriterionUpTitle.
  ///
  /// In zh, this message translates to:
  /// **'上移选中条件'**
  String get gridMoveCriterionUpTitle;

  /// No description provided for @gridMoveCriterionDownTitle.
  ///
  /// In zh, this message translates to:
  /// **'下移选中条件'**
  String get gridMoveCriterionDownTitle;

  /// No description provided for @gridFilterTextHint.
  ///
  /// In zh, this message translates to:
  /// **'不含 WHERE 关键字，例如：id > 100 AND name LIKE \'集团%\''**
  String get gridFilterTextHint;

  /// No description provided for @gridApplyFilterSortTitle.
  ///
  /// In zh, this message translates to:
  /// **'应用筛选 & 排序'**
  String get gridApplyFilterSortTitle;

  /// No description provided for @gridCriterionEditedTitle.
  ///
  /// In zh, this message translates to:
  /// **'已编辑准则'**
  String get gridCriterionEditedTitle;

  /// No description provided for @gridColumnsTitle.
  ///
  /// In zh, this message translates to:
  /// **'列'**
  String get gridColumnsTitle;

  /// No description provided for @gridColumnsNoInfo.
  ///
  /// In zh, this message translates to:
  /// **'列'**
  String get gridColumnsNoInfo;

  /// No description provided for @gridSearchColumnsHint.
  ///
  /// In zh, this message translates to:
  /// **'搜索'**
  String get gridSearchColumnsHint;

  /// No description provided for @gridLoadingColumnsEllipsis.
  ///
  /// In zh, this message translates to:
  /// **'正在加载列信息 ...'**
  String get gridLoadingColumnsEllipsis;

  /// No description provided for @gridReadingColumnsEllipsis.
  ///
  /// In zh, this message translates to:
  /// **'正在读取列信息 …'**
  String get gridReadingColumnsEllipsis;

  /// No description provided for @gridCellEditorTabText.
  ///
  /// In zh, this message translates to:
  /// **'文本'**
  String get gridCellEditorTabText;

  /// No description provided for @gridCellEditorTabHex.
  ///
  /// In zh, this message translates to:
  /// **'十六进制'**
  String get gridCellEditorTabHex;

  /// No description provided for @gridCellEditorTabImage.
  ///
  /// In zh, this message translates to:
  /// **'图像'**
  String get gridCellEditorTabImage;

  /// No description provided for @gridCellEditorTabWeb.
  ///
  /// In zh, this message translates to:
  /// **'网页'**
  String get gridCellEditorTabWeb;

  /// No description provided for @gridSelectNone.
  ///
  /// In zh, this message translates to:
  /// **'未选中单元格'**
  String get gridSelectNone;

  /// No description provided for @gridColumnNumber.
  ///
  /// In zh, this message translates to:
  /// **'列 {col}'**
  String gridColumnNumber(String col);

  /// No description provided for @gridDisplayRow.
  ///
  /// In zh, this message translates to:
  /// **'  ·  第 {row} 行'**
  String gridDisplayRow(String row);

  /// No description provided for @gridCellEditorHeader.
  ///
  /// In zh, this message translates to:
  /// **'单元格编辑器 · {title}'**
  String gridCellEditorHeader(String title);

  /// No description provided for @gridCellEditorPanelTitle.
  ///
  /// In zh, this message translates to:
  /// **'单元格编辑器'**
  String get gridCellEditorPanelTitle;

  /// No description provided for @gridCellCopyOne.
  ///
  /// In zh, this message translates to:
  /// **'复制单元格'**
  String get gridCellCopyOne;

  /// No description provided for @gridCellCopyAs.
  ///
  /// In zh, this message translates to:
  /// **'复制为'**
  String get gridCellCopyAs;

  /// No description provided for @gridCellCopyTsv.
  ///
  /// In zh, this message translates to:
  /// **'记录（制表符分隔）'**
  String get gridCellCopyTsv;

  /// No description provided for @gridCellCopyCsv.
  ///
  /// In zh, this message translates to:
  /// **'记录（CSV）'**
  String get gridCellCopyCsv;

  /// No description provided for @gridCellCopyCsvWithHeader.
  ///
  /// In zh, this message translates to:
  /// **'记录 + 栏位名（CSV）'**
  String get gridCellCopyCsvWithHeader;

  /// No description provided for @gridCellPasteAppend.
  ///
  /// In zh, this message translates to:
  /// **'粘贴行(追加为新增)'**
  String get gridCellPasteAppend;

  /// No description provided for @gridCellPasteToCell.
  ///
  /// In zh, this message translates to:
  /// **'粘贴到单元格'**
  String get gridCellPasteToCell;

  /// No description provided for @gridCellSaveAs.
  ///
  /// In zh, this message translates to:
  /// **'保存数据为...'**
  String get gridCellSaveAs;

  /// No description provided for @gridCellSetNullTitle.
  ///
  /// In zh, this message translates to:
  /// **'设置为 NULL'**
  String get gridCellSetNullTitle;

  /// No description provided for @gridRowsCopy.
  ///
  /// In zh, this message translates to:
  /// **'复制 {count} 行'**
  String gridRowsCopy(String count);

  /// No description provided for @gridRowsCopyOne.
  ///
  /// In zh, this message translates to:
  /// **'复制行'**
  String get gridRowsCopyOne;

  /// No description provided for @gridRowsDelete.
  ///
  /// In zh, this message translates to:
  /// **'删除 {count} 行记录'**
  String gridRowsDelete(String count);

  /// No description provided for @gridRowsDeleteOne.
  ///
  /// In zh, this message translates to:
  /// **'删除 记录'**
  String get gridRowsDeleteOne;

  /// No description provided for @gridCellSortAscBy.
  ///
  /// In zh, this message translates to:
  /// **'升序（{column}）'**
  String gridCellSortAscBy(String column);

  /// No description provided for @gridCellSortDescBy.
  ///
  /// In zh, this message translates to:
  /// **'降序（{column}）'**
  String gridCellSortDescBy(String column);

  /// No description provided for @gridCellHideColumn.
  ///
  /// In zh, this message translates to:
  /// **'隐藏「{column}」'**
  String gridCellHideColumn(String column);

  /// No description provided for @dtpOk.
  ///
  /// In zh, this message translates to:
  /// **'确定'**
  String get dtpOk;

  /// No description provided for @dtpCancel.
  ///
  /// In zh, this message translates to:
  /// **'取消'**
  String get dtpCancel;

  /// No description provided for @btnApplyTitle.
  ///
  /// In zh, this message translates to:
  /// **'应用'**
  String get btnApplyTitle;

  /// No description provided for @btnUndoTitle.
  ///
  /// In zh, this message translates to:
  /// **'撤销'**
  String get btnUndoTitle;

  /// No description provided for @opEquals.
  ///
  /// In zh, this message translates to:
  /// **'等于'**
  String get opEquals;

  /// No description provided for @opNotEquals.
  ///
  /// In zh, this message translates to:
  /// **'不等于'**
  String get opNotEquals;

  /// No description provided for @opGreaterThan.
  ///
  /// In zh, this message translates to:
  /// **'大于'**
  String get opGreaterThan;

  /// No description provided for @opGreaterOrEqual.
  ///
  /// In zh, this message translates to:
  /// **'大于等于'**
  String get opGreaterOrEqual;

  /// No description provided for @opLessThan.
  ///
  /// In zh, this message translates to:
  /// **'小于'**
  String get opLessThan;

  /// No description provided for @opLessOrEqual.
  ///
  /// In zh, this message translates to:
  /// **'小于等于'**
  String get opLessOrEqual;

  /// No description provided for @opContains.
  ///
  /// In zh, this message translates to:
  /// **'包含'**
  String get opContains;

  /// No description provided for @opNotContains.
  ///
  /// In zh, this message translates to:
  /// **'不包含'**
  String get opNotContains;

  /// No description provided for @opStartsWith.
  ///
  /// In zh, this message translates to:
  /// **'开头是'**
  String get opStartsWith;

  /// No description provided for @opEndsWith.
  ///
  /// In zh, this message translates to:
  /// **'结尾是'**
  String get opEndsWith;

  /// No description provided for @opIsNull.
  ///
  /// In zh, this message translates to:
  /// **'为空'**
  String get opIsNull;

  /// No description provided for @opIsNotNull.
  ///
  /// In zh, this message translates to:
  /// **'不为空'**
  String get opIsNotNull;

  /// No description provided for @gridRefresh.
  ///
  /// In zh, this message translates to:
  /// **'刷新'**
  String get gridRefresh;

  /// No description provided for @gridDeleteRowErrorAt.
  ///
  /// In zh, this message translates to:
  /// **'删除第 {row} 行: {error}'**
  String gridDeleteRowErrorAt(String row, String error);

  /// 命令列界面:输出区为空时的用法说明
  ///
  /// In zh, this message translates to:
  /// **'输入 SQL 语句后按回车执行。语句以分号结尾;未写完可继续输入下一行,↑ / ↓ 翻历史。'**
  String get cliEmptyHint;

  /// 命令列界面:输入行占位文案
  ///
  /// In zh, this message translates to:
  /// **'输入 SQL 语句'**
  String get cliInputHint;

  /// 连接树分组头 / 对象面板工具栏:新建实体化视图(仅 PostgreSQL 家族)
  ///
  /// In zh, this message translates to:
  /// **'新建实体化视图'**
  String get ctxNewMatView;

  /// No description provided for @actionRefresh.
  ///
  /// In zh, this message translates to:
  /// **'刷新{label}'**
  String actionRefresh(String label);

  /// No description provided for @actionRefreshConcurrently.
  ///
  /// In zh, this message translates to:
  /// **'并发刷新{label}'**
  String actionRefreshConcurrently(String label);

  /// No description provided for @matvTabDefinition.
  ///
  /// In zh, this message translates to:
  /// **'定义'**
  String get matvTabDefinition;

  /// No description provided for @matvTabIndexes.
  ///
  /// In zh, this message translates to:
  /// **'索引'**
  String get matvTabIndexes;

  /// No description provided for @matvTabAdvanced.
  ///
  /// In zh, this message translates to:
  /// **'高级'**
  String get matvTabAdvanced;

  /// No description provided for @matvTabComment.
  ///
  /// In zh, this message translates to:
  /// **'注释'**
  String get matvTabComment;

  /// No description provided for @matvTabSqlPreview.
  ///
  /// In zh, this message translates to:
  /// **'SQL 预览'**
  String get matvTabSqlPreview;

  /// No description provided for @matvPanelMessages.
  ///
  /// In zh, this message translates to:
  /// **'消息'**
  String get matvPanelMessages;

  /// No description provided for @matvPreview.
  ///
  /// In zh, this message translates to:
  /// **'预览'**
  String get matvPreview;

  /// No description provided for @matvExplain.
  ///
  /// In zh, this message translates to:
  /// **'解释'**
  String get matvExplain;

  /// No description provided for @matvBeautifySql.
  ///
  /// In zh, this message translates to:
  /// **'美化 SQL'**
  String get matvBeautifySql;

  /// No description provided for @matvTabChange.
  ///
  /// In zh, this message translates to:
  /// **'更改'**
  String get matvTabChange;

  /// No description provided for @matvCopy.
  ///
  /// In zh, this message translates to:
  /// **'复制'**
  String get matvCopy;

  /// No description provided for @matvFullscreen.
  ///
  /// In zh, this message translates to:
  /// **'全屏'**
  String get matvFullscreen;

  /// No description provided for @matvExitFullscreen.
  ///
  /// In zh, this message translates to:
  /// **'退出全屏'**
  String get matvExitFullscreen;

  /// No description provided for @matvCommentHint.
  ///
  /// In zh, this message translates to:
  /// **'输入实体化视图注释(保存时写入 COMMENT ON)'**
  String get matvCommentHint;

  /// No description provided for @matvAddIndex.
  ///
  /// In zh, this message translates to:
  /// **'添加索引'**
  String get matvAddIndex;

  /// No description provided for @matvDropIndex.
  ///
  /// In zh, this message translates to:
  /// **'删除索引'**
  String get matvDropIndex;

  /// No description provided for @matvColDefinition.
  ///
  /// In zh, this message translates to:
  /// **'定义'**
  String get matvColDefinition;

  /// No description provided for @colUnique.
  ///
  /// In zh, this message translates to:
  /// **'唯一'**
  String get colUnique;

  /// No description provided for @matvFieldIndexName.
  ///
  /// In zh, this message translates to:
  /// **'索引名称:'**
  String get matvFieldIndexName;

  /// No description provided for @matvFieldIndexColumns.
  ///
  /// In zh, this message translates to:
  /// **'索引列:'**
  String get matvFieldIndexColumns;

  /// No description provided for @matvFieldOwner.
  ///
  /// In zh, this message translates to:
  /// **'所有者:'**
  String get matvFieldOwner;

  /// No description provided for @matvFieldTablespace.
  ///
  /// In zh, this message translates to:
  /// **'表空间:'**
  String get matvFieldTablespace;

  /// No description provided for @matvFieldAccessMethod.
  ///
  /// In zh, this message translates to:
  /// **'访问方法:'**
  String get matvFieldAccessMethod;

  /// No description provided for @matvFieldStorageParams.
  ///
  /// In zh, this message translates to:
  /// **'存储参数:'**
  String get matvFieldStorageParams;

  /// No description provided for @matvWithData.
  ///
  /// In zh, this message translates to:
  /// **'填充数据(WITH DATA)'**
  String get matvWithData;

  /// No description provided for @matvDefault.
  ///
  /// In zh, this message translates to:
  /// **'默认'**
  String get matvDefault;

  /// No description provided for @matvColumnsHint.
  ///
  /// In zh, this message translates to:
  /// **'可用列:{columns}'**
  String matvColumnsHint(String columns);

  /// No description provided for @matvSaveTitle.
  ///
  /// In zh, this message translates to:
  /// **'保存实体化视图'**
  String get matvSaveTitle;

  /// No description provided for @matvDefinitionEmpty.
  ///
  /// In zh, this message translates to:
  /// **'实体化视图定义不能为空,请在「定义」页输入查询 SQL。'**
  String get matvDefinitionEmpty;

  /// No description provided for @matvDefinitionReadFailed.
  ///
  /// In zh, this message translates to:
  /// **'读取实体化视图「{name}」定义失败:\n{error}'**
  String matvDefinitionReadFailed(String name, String error);

  /// No description provided for @matvRebuildConfirm.
  ///
  /// In zh, this message translates to:
  /// **'保存会重建实体化视图「{name}」(先 DROP 再 CREATE)。\n已有索引会自动重建,但对其授予的权限(GRANT)会丢失,数据也会重新填充。\n是否继续?'**
  String matvRebuildConfirm(String name);

  /// No description provided for @matvSaveFailed.
  ///
  /// In zh, this message translates to:
  /// **'保存失败:\n{error}'**
  String matvSaveFailed(String error);

  /// No description provided for @matvSaved.
  ///
  /// In zh, this message translates to:
  /// **'已保存实体化视图「{name}」'**
  String matvSaved(String name);

  /// No description provided for @matvPreviewEmpty.
  ///
  /// In zh, this message translates to:
  /// **'实体化视图定义为空,无可预览的数据。'**
  String get matvPreviewEmpty;

  /// No description provided for @matvExplainEmpty.
  ///
  /// In zh, this message translates to:
  /// **'实体化视图定义为空,无法解释。'**
  String get matvExplainEmpty;

  /// No description provided for @matvPreviewResult.
  ///
  /// In zh, this message translates to:
  /// **'已返回 {rows} 行,耗时 {ms} ms'**
  String matvPreviewResult(String rows, String ms);

  /// No description provided for @matvRefreshRunning.
  ///
  /// In zh, this message translates to:
  /// **'正在刷新 ...'**
  String get matvRefreshRunning;

  /// No description provided for @matvToolbarRefreshConcurrently.
  ///
  /// In zh, this message translates to:
  /// **'并发刷新'**
  String get matvToolbarRefreshConcurrently;

  /// No description provided for @matvRefreshed.
  ///
  /// In zh, this message translates to:
  /// **'已刷新实体化视图「{name}」,耗时 {ms} ms'**
  String matvRefreshed(String name, String ms);

  /// No description provided for @matvRefreshFailed.
  ///
  /// In zh, this message translates to:
  /// **'刷新失败:\n{error}'**
  String matvRefreshFailed(String error);

  /// No description provided for @matvPreviewFailed.
  ///
  /// In zh, this message translates to:
  /// **'预览失败:\n{error}'**
  String matvPreviewFailed(String error);

  /// No description provided for @matvExplainFailed.
  ///
  /// In zh, this message translates to:
  /// **'解释失败:\n{error}'**
  String matvExplainFailed(String error);

  /// No description provided for @matvConcurrentlyNeedUnique.
  ///
  /// In zh, this message translates to:
  /// **'并发刷新需要一个覆盖全部列、且不带 WHERE 条件的唯一索引'**
  String get matvConcurrentlyNeedUnique;

  /// No description provided for @matvConcurrentlyOk.
  ///
  /// In zh, this message translates to:
  /// **'并发刷新可用'**
  String get matvConcurrentlyOk;

  /// No description provided for @matvConcurrentlyAsk.
  ///
  /// In zh, this message translates to:
  /// **'未检测到满足条件的唯一索引,仍要尝试并发刷新吗?'**
  String get matvConcurrentlyAsk;

  /// No description provided for @matvNotPopulated.
  ///
  /// In zh, this message translates to:
  /// **'实体化视图「{name}」尚未填充数据(WITH NO DATA),需先刷新才能查询。'**
  String matvNotPopulated(String name);

  /// No description provided for @matvIndexIncomplete.
  ///
  /// In zh, this message translates to:
  /// **'请先填写索引名称与索引列。'**
  String get matvIndexIncomplete;

  /// No description provided for @matvIndexNoSelection.
  ///
  /// In zh, this message translates to:
  /// **'请先在索引列表中选择要删除的索引。'**
  String get matvIndexNoSelection;

  /// No description provided for @matvIndexDropConfirm.
  ///
  /// In zh, this message translates to:
  /// **'确定删除索引「{name}」吗?\n将执行 DROP INDEX,不可恢复。'**
  String matvIndexDropConfirm(String name);

  /// No description provided for @sqlRunAtCursor.
  ///
  /// In zh, this message translates to:
  /// **'运行当前语句'**
  String get sqlRunAtCursor;

  /// No description provided for @sqlRunSelected.
  ///
  /// In zh, this message translates to:
  /// **'运行已选择的'**
  String get sqlRunSelected;

  /// No description provided for @sqlRunAll.
  ///
  /// In zh, this message translates to:
  /// **'运行全部'**
  String get sqlRunAll;

  /// No description provided for @sqlMenuCopy.
  ///
  /// In zh, this message translates to:
  /// **'复制'**
  String get sqlMenuCopy;

  /// No description provided for @sqlMenuPaste.
  ///
  /// In zh, this message translates to:
  /// **'粘贴'**
  String get sqlMenuPaste;

  /// No description provided for @sqlMenuSelectAll.
  ///
  /// In zh, this message translates to:
  /// **'全选'**
  String get sqlMenuSelectAll;

  /// No description provided for @kvTabCategory.
  ///
  /// In zh, this message translates to:
  /// **'键浏览'**
  String get kvTabCategory;

  /// No description provided for @kvPatternHint.
  ///
  /// In zh, this message translates to:
  /// **'搜索键名(默认包含匹配,glob 如 scan:*)'**
  String get kvPatternHint;

  /// No description provided for @kvTypeAll.
  ///
  /// In zh, this message translates to:
  /// **'全部类型'**
  String get kvTypeAll;

  /// No description provided for @kvActionRefresh.
  ///
  /// In zh, this message translates to:
  /// **'刷新'**
  String get kvActionRefresh;

  /// No description provided for @kvActionReadMore.
  ///
  /// In zh, this message translates to:
  /// **'读取后续内容'**
  String get kvActionReadMore;

  /// No description provided for @kvColKeyName.
  ///
  /// In zh, this message translates to:
  /// **'键名'**
  String get kvColKeyName;

  /// No description provided for @kvColType.
  ///
  /// In zh, this message translates to:
  /// **'类型'**
  String get kvColType;

  /// No description provided for @kvColTtl.
  ///
  /// In zh, this message translates to:
  /// **'TTL'**
  String get kvColTtl;

  /// No description provided for @kvColEncoding.
  ///
  /// In zh, this message translates to:
  /// **'编码'**
  String get kvColEncoding;

  /// No description provided for @kvColBytes.
  ///
  /// In zh, this message translates to:
  /// **'字节'**
  String get kvColBytes;

  /// No description provided for @kvColField.
  ///
  /// In zh, this message translates to:
  /// **'字段'**
  String get kvColField;

  /// No description provided for @kvColIndex.
  ///
  /// In zh, this message translates to:
  /// **'下标'**
  String get kvColIndex;

  /// No description provided for @kvColEntryId.
  ///
  /// In zh, this message translates to:
  /// **'条目 ID'**
  String get kvColEntryId;

  /// No description provided for @kvColScore.
  ///
  /// In zh, this message translates to:
  /// **'分数'**
  String get kvColScore;

  /// No description provided for @kvColMember.
  ///
  /// In zh, this message translates to:
  /// **'成员'**
  String get kvColMember;

  /// No description provided for @kvColValue.
  ///
  /// In zh, this message translates to:
  /// **'值'**
  String get kvColValue;

  /// No description provided for @kvTypeString.
  ///
  /// In zh, this message translates to:
  /// **'字符串'**
  String get kvTypeString;

  /// No description provided for @kvTypeList.
  ///
  /// In zh, this message translates to:
  /// **'列表'**
  String get kvTypeList;

  /// No description provided for @kvTypeSet.
  ///
  /// In zh, this message translates to:
  /// **'集合'**
  String get kvTypeSet;

  /// No description provided for @kvTypeZset.
  ///
  /// In zh, this message translates to:
  /// **'有序集合'**
  String get kvTypeZset;

  /// No description provided for @kvTypeHash.
  ///
  /// In zh, this message translates to:
  /// **'哈希'**
  String get kvTypeHash;

  /// No description provided for @kvTypeStream.
  ///
  /// In zh, this message translates to:
  /// **'流'**
  String get kvTypeStream;

  /// No description provided for @kvTypeNone.
  ///
  /// In zh, this message translates to:
  /// **'不存在'**
  String get kvTypeNone;

  /// No description provided for @kvTypeUnknown.
  ///
  /// In zh, this message translates to:
  /// **'未知'**
  String get kvTypeUnknown;

  /// No description provided for @kvTtlNone.
  ///
  /// In zh, this message translates to:
  /// **'不过期'**
  String get kvTtlNone;

  /// No description provided for @kvTtlDay.
  ///
  /// In zh, this message translates to:
  /// **'天'**
  String get kvTtlDay;

  /// No description provided for @kvScanning.
  ///
  /// In zh, this message translates to:
  /// **'扫描中…'**
  String get kvScanning;

  /// No description provided for @kvScanComplete.
  ///
  /// In zh, this message translates to:
  /// **'已遍历完'**
  String get kvScanComplete;

  /// No description provided for @kvScanContinue.
  ///
  /// In zh, this message translates to:
  /// **'未扫完,点此继续扫描'**
  String get kvScanContinue;

  /// No description provided for @kvRoundCapHint.
  ///
  /// In zh, this message translates to:
  /// **'本轮未命中,可继续扫描'**
  String get kvRoundCapHint;

  /// No description provided for @kvLoadedCount.
  ///
  /// In zh, this message translates to:
  /// **'已加载 {loaded} 个键'**
  String kvLoadedCount(String loaded);

  /// No description provided for @kvLoadedOfTotal.
  ///
  /// In zh, this message translates to:
  /// **'已加载 {loaded} 个键 / 约 {total}'**
  String kvLoadedOfTotal(String loaded, String total);

  /// No description provided for @kvNoMatch.
  ///
  /// In zh, this message translates to:
  /// **'没有符合条件的键'**
  String get kvNoMatch;

  /// No description provided for @kvNoMatchYet.
  ///
  /// In zh, this message translates to:
  /// **'尚未扫到符合条件的键,点此继续扫描'**
  String get kvNoMatchYet;

  /// No description provided for @kvLoadFailed.
  ///
  /// In zh, this message translates to:
  /// **'键列表读取失败:{error}'**
  String kvLoadFailed(String error);

  /// No description provided for @kvValueFailed.
  ///
  /// In zh, this message translates to:
  /// **'值读取失败:{error}'**
  String kvValueFailed(String error);

  /// No description provided for @kvPickKey.
  ///
  /// In zh, this message translates to:
  /// **'选择左侧的键查看值'**
  String get kvPickKey;

  /// No description provided for @kvKeyGone.
  ///
  /// In zh, this message translates to:
  /// **'该键已不存在(可能刚过期或被删除)'**
  String get kvKeyGone;

  /// No description provided for @kvValueBytes.
  ///
  /// In zh, this message translates to:
  /// **'共 {total} 字节'**
  String kvValueBytes(String total);

  /// No description provided for @kvValueCount.
  ///
  /// In zh, this message translates to:
  /// **'共 {total} 项'**
  String kvValueCount(String total);

  /// No description provided for @kvValueTruncated.
  ///
  /// In zh, this message translates to:
  /// **'值较大,已显示前 {shown} / {total} 字节'**
  String kvValueTruncated(String shown, String total);

  /// No description provided for @kvFormatText.
  ///
  /// In zh, this message translates to:
  /// **'文本'**
  String get kvFormatText;

  /// No description provided for @kvFormatJson.
  ///
  /// In zh, this message translates to:
  /// **'JSON'**
  String get kvFormatJson;

  /// No description provided for @kvFormatHex.
  ///
  /// In zh, this message translates to:
  /// **'十六进制'**
  String get kvFormatHex;

  /// No description provided for @kvFormatNotJson.
  ///
  /// In zh, this message translates to:
  /// **'不是合法 JSON,已按原文显示'**
  String get kvFormatNotJson;

  /// No description provided for @kvFormatBinary.
  ///
  /// In zh, this message translates to:
  /// **'值是二进制,无法按 JSON 显示'**
  String get kvFormatBinary;

  /// No description provided for @kvFormatTooBig.
  ///
  /// In zh, this message translates to:
  /// **'格式视图只渲染前 {count} 字节'**
  String kvFormatTooBig(String count);

  /// No description provided for @kvFormatReadOnly.
  ///
  /// In zh, this message translates to:
  /// **'格式化视图只读,编辑请切回「文本」'**
  String get kvFormatReadOnly;

  /// No description provided for @kvConnectionGone.
  ///
  /// In zh, this message translates to:
  /// **'该连接已不存在,无法读取键'**
  String get kvConnectionGone;

  /// No description provided for @kvScanUnsupported.
  ///
  /// In zh, this message translates to:
  /// **'该引擎不支持枚举键'**
  String get kvScanUnsupported;

  /// No description provided for @kvActionNewKey.
  ///
  /// In zh, this message translates to:
  /// **'新建键'**
  String get kvActionNewKey;

  /// No description provided for @kvActionAdd.
  ///
  /// In zh, this message translates to:
  /// **'添加'**
  String get kvActionAdd;

  /// No description provided for @kvActionSetTtl.
  ///
  /// In zh, this message translates to:
  /// **'设置过期时间…'**
  String get kvActionSetTtl;

  /// No description provided for @kvActionClearTtl.
  ///
  /// In zh, this message translates to:
  /// **'清除过期时间'**
  String get kvActionClearTtl;

  /// No description provided for @kvMenuCopyKeyName.
  ///
  /// In zh, this message translates to:
  /// **'复制键名'**
  String get kvMenuCopyKeyName;

  /// No description provided for @kvMenuDeleteKeyOne.
  ///
  /// In zh, this message translates to:
  /// **'删除键'**
  String get kvMenuDeleteKeyOne;

  /// No description provided for @kvMenuDeleteKeys.
  ///
  /// In zh, this message translates to:
  /// **'删除 {count} 个键'**
  String kvMenuDeleteKeys(String count);

  /// No description provided for @kvMenuEdit.
  ///
  /// In zh, this message translates to:
  /// **'编辑'**
  String get kvMenuEdit;

  /// No description provided for @kvMenuCopyValue.
  ///
  /// In zh, this message translates to:
  /// **'复制值'**
  String get kvMenuCopyValue;

  /// No description provided for @kvMenuDeleteItemOne.
  ///
  /// In zh, this message translates to:
  /// **'删除此项'**
  String get kvMenuDeleteItemOne;

  /// No description provided for @kvMenuDeleteItems.
  ///
  /// In zh, this message translates to:
  /// **'删除 {count} 项'**
  String kvMenuDeleteItems(String count);

  /// No description provided for @kvNotWritable.
  ///
  /// In zh, this message translates to:
  /// **'该引擎不支持写操作'**
  String get kvNotWritable;

  /// No description provided for @kvStreamReadOnly.
  ///
  /// In zh, this message translates to:
  /// **'流只能追加条目,不支持在此编辑'**
  String get kvStreamReadOnly;

  /// No description provided for @kvBigStringReadOnly.
  ///
  /// In zh, this message translates to:
  /// **'值未完整读出,不能在此编辑(先点「读取后续内容」读全)'**
  String get kvBigStringReadOnly;

  /// No description provided for @kvDeleteConfirmOne.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除键「{name}」吗?\n删除后不可恢复。'**
  String kvDeleteConfirmOne(String name);

  /// No description provided for @kvDeleteConfirmMany.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除选中的 {count} 个键吗?\n删除后不可恢复。'**
  String kvDeleteConfirmMany(String count);

  /// No description provided for @kvDeleteDoneOne.
  ///
  /// In zh, this message translates to:
  /// **'已删除 1 个键'**
  String get kvDeleteDoneOne;

  /// No description provided for @kvDeleteDone.
  ///
  /// In zh, this message translates to:
  /// **'已删除 {count} 个键'**
  String kvDeleteDone(String count);

  /// No description provided for @kvTtlSetDoneOne.
  ///
  /// In zh, this message translates to:
  /// **'已设置 1 个键的过期时间'**
  String get kvTtlSetDoneOne;

  /// No description provided for @kvTtlSetDone.
  ///
  /// In zh, this message translates to:
  /// **'已设置 {count} 个键的过期时间'**
  String kvTtlSetDone(String count);

  /// No description provided for @kvTtlClearedDoneOne.
  ///
  /// In zh, this message translates to:
  /// **'已清除 1 个键的过期时间'**
  String get kvTtlClearedDoneOne;

  /// No description provided for @kvTtlClearedDone.
  ///
  /// In zh, this message translates to:
  /// **'已清除 {count} 个键的过期时间'**
  String kvTtlClearedDone(String count);

  /// No description provided for @kvWritePartial.
  ///
  /// In zh, this message translates to:
  /// **'{ok} / {total} 个键已生效,其余键已不存在'**
  String kvWritePartial(String ok, String total);

  /// No description provided for @kvExpiresAt.
  ///
  /// In zh, this message translates to:
  /// **'过期于 {time}'**
  String kvExpiresAt(String time);

  /// No description provided for @kvWriteFailed.
  ///
  /// In zh, this message translates to:
  /// **'写入失败:{error}'**
  String kvWriteFailed(String error);

  /// No description provided for @kvSaveDone.
  ///
  /// In zh, this message translates to:
  /// **'已保存'**
  String get kvSaveDone;

  /// No description provided for @kvNewKeyTitle.
  ///
  /// In zh, this message translates to:
  /// **'新建键'**
  String get kvNewKeyTitle;

  /// No description provided for @kvKeyNameEmpty.
  ///
  /// In zh, this message translates to:
  /// **'键名不能为空'**
  String get kvKeyNameEmpty;

  /// No description provided for @kvKeyNameTaken.
  ///
  /// In zh, this message translates to:
  /// **'该键已存在,请换键名,或用「添加」向它追加内容'**
  String get kvKeyNameTaken;

  /// No description provided for @kvFieldTtl.
  ///
  /// In zh, this message translates to:
  /// **'过期时间'**
  String get kvFieldTtl;

  /// No description provided for @kvTtlHint.
  ///
  /// In zh, this message translates to:
  /// **'留空 = 不过期'**
  String get kvTtlHint;

  /// No description provided for @kvTtlSetTitle.
  ///
  /// In zh, this message translates to:
  /// **'设置过期时间'**
  String get kvTtlSetTitle;

  /// No description provided for @kvTtlInvalid.
  ///
  /// In zh, this message translates to:
  /// **'过期时长要填大于 0 的数字'**
  String get kvTtlInvalid;

  /// No description provided for @kvTtlUnitSeconds.
  ///
  /// In zh, this message translates to:
  /// **'秒'**
  String get kvTtlUnitSeconds;

  /// No description provided for @kvTtlUnitMinutes.
  ///
  /// In zh, this message translates to:
  /// **'分钟'**
  String get kvTtlUnitMinutes;

  /// No description provided for @kvTtlUnitHours.
  ///
  /// In zh, this message translates to:
  /// **'小时'**
  String get kvTtlUnitHours;

  /// No description provided for @kvTtlUnitDays.
  ///
  /// In zh, this message translates to:
  /// **'天'**
  String get kvTtlUnitDays;

  /// No description provided for @kvAddFieldTitle.
  ///
  /// In zh, this message translates to:
  /// **'添加字段'**
  String get kvAddFieldTitle;

  /// No description provided for @kvAddElementTitle.
  ///
  /// In zh, this message translates to:
  /// **'添加元素'**
  String get kvAddElementTitle;

  /// No description provided for @kvAddMemberTitle.
  ///
  /// In zh, this message translates to:
  /// **'添加成员'**
  String get kvAddMemberTitle;

  /// No description provided for @kvPosition.
  ///
  /// In zh, this message translates to:
  /// **'插入位置'**
  String get kvPosition;

  /// No description provided for @kvPositionHead.
  ///
  /// In zh, this message translates to:
  /// **'头部'**
  String get kvPositionHead;

  /// No description provided for @kvPositionTail.
  ///
  /// In zh, this message translates to:
  /// **'尾部'**
  String get kvPositionTail;

  /// No description provided for @kvScoreInvalid.
  ///
  /// In zh, this message translates to:
  /// **'分数要填数字'**
  String get kvScoreInvalid;

  /// No description provided for @kvHexHint.
  ///
  /// In zh, this message translates to:
  /// **'以 HEX: 开头即按十六进制写入裸字节'**
  String get kvHexHint;

  /// No description provided for @kvHexInvalid.
  ///
  /// In zh, this message translates to:
  /// **'十六进制格式不对:需要偶数个 0-9A-F 字符'**
  String get kvHexInvalid;

  /// No description provided for @kvValueRequired.
  ///
  /// In zh, this message translates to:
  /// **'值不能为空'**
  String get kvValueRequired;

  /// No description provided for @kvCopiedNames.
  ///
  /// In zh, this message translates to:
  /// **'已复制 {count} 个键名到剪贴板'**
  String kvCopiedNames(String count);

  /// No description provided for @kvTtlAlreadyNone.
  ///
  /// In zh, this message translates to:
  /// **'这些键本来就没有过期时间'**
  String get kvTtlAlreadyNone;

  /// No description provided for @kvDeletePartial.
  ///
  /// In zh, this message translates to:
  /// **'{ok} / {total} 项已删除,其余项不在已读出的窗口里或已不存在'**
  String kvDeletePartial(String ok, String total);

  /// No description provided for @kvAddWindowOnly.
  ///
  /// In zh, this message translates to:
  /// **'该值共 {total} 项,当前只读出 {shown} 项,新内容落在已读出范围之外'**
  String kvAddWindowOnly(String total, String shown);

  /// No description provided for @kvDeleteItemConfirmOne.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除这一项吗?\n删除后不可恢复。'**
  String get kvDeleteItemConfirmOne;

  /// No description provided for @kvDeleteItemConfirm.
  ///
  /// In zh, this message translates to:
  /// **'确定要删除选中的 {count} 项吗?\n删除后不可恢复。'**
  String kvDeleteItemConfirm(String count);

  /// No description provided for @kvItemsDeletedOne.
  ///
  /// In zh, this message translates to:
  /// **'已删除 1 项'**
  String get kvItemsDeletedOne;

  /// No description provided for @kvItemsDeleted.
  ///
  /// In zh, this message translates to:
  /// **'已删除 {count} 项'**
  String kvItemsDeleted(String count);

  /// No description provided for @kvIndexReadOnly.
  ///
  /// In zh, this message translates to:
  /// **'下标表示元素的位置,不能编辑;要改内容请编辑右侧的值'**
  String get kvIndexReadOnly;

  /// No description provided for @findHint.
  ///
  /// In zh, this message translates to:
  /// **'查找'**
  String get findHint;

  /// No description provided for @findReplaceHint.
  ///
  /// In zh, this message translates to:
  /// **'替换'**
  String get findReplaceHint;

  /// No description provided for @findNoMatches.
  ///
  /// In zh, this message translates to:
  /// **'无匹配'**
  String get findNoMatches;

  /// No description provided for @findMatchPosition.
  ///
  /// In zh, this message translates to:
  /// **'第 {current}/{total} 处'**
  String findMatchPosition(String current, String total);

  /// No description provided for @findSearching.
  ///
  /// In zh, this message translates to:
  /// **'搜索中…'**
  String get findSearching;

  /// No description provided for @findPrev.
  ///
  /// In zh, this message translates to:
  /// **'上一个'**
  String get findPrev;

  /// No description provided for @findNext.
  ///
  /// In zh, this message translates to:
  /// **'下一个'**
  String get findNext;

  /// No description provided for @findCaseSensitive.
  ///
  /// In zh, this message translates to:
  /// **'区分大小写'**
  String get findCaseSensitive;

  /// No description provided for @findRegex.
  ///
  /// In zh, this message translates to:
  /// **'使用正则表达式'**
  String get findRegex;

  /// No description provided for @findBtnReplace.
  ///
  /// In zh, this message translates to:
  /// **'替换'**
  String get findBtnReplace;

  /// No description provided for @findBtnReplaceAll.
  ///
  /// In zh, this message translates to:
  /// **'全部替换'**
  String get findBtnReplaceAll;

  /// No description provided for @findClose.
  ///
  /// In zh, this message translates to:
  /// **'关闭'**
  String get findClose;
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'ja', 'zh'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
    case 'ja':
      return AppLocalizationsJa();
    case 'zh':
      return AppLocalizationsZh();
  }

  throw FlutterError(
      'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
      'an issue with the localizations generation tool. Please file an issue '
      'on GitHub with a reproducible sample app and the gen-l10n configuration '
      'that was used.');
}
