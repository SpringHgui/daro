import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:re_editor/re_editor.dart';
import 'package:re_highlight/languages/sql.dart';

import '../app/app_state.dart';
import '../data/user_sql.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';
import 'routine_design_page.dart' show sqlDarkTheme, sqlLightTheme;

/// 用户 / 角色(User & Role)设计页。
///
/// 与 [RoutineDesignPage] / [ViewDesignPage] 同构:页面只持有"表单字段",
/// DDL 一律由 [UserSql] 拼装,**「SQL 预览」标签与「保存」动作复用同一份拼装结果**,
/// 保证"看到的 = 保存时执行的"。
///
/// 布局(对齐 Navicat 的「用户」编辑器):
/// - 工具栏:保存 / 刷新;
/// - 标题行:`设计角色: name @ 连接.库`;
/// - 标签页:常规 / 高级 / 成员属于 / 成员 / 服务器权限 / 权限 / SQL 预览;
/// - 状态栏:保存进度与错误。
///
/// 七个标签**按当前数据库类型的能力裁剪**(见 [UserSql] 的 `supports*` 判定):
/// 不适用的标签仍然保留(保持布局稳定、避免标签数随连接跳变),
/// 内容换成一条说明 —— 让用户知道"为什么这里没东西",而不是看到一个空页。
class UserDesignPage extends StatefulWidget {
  const UserDesignPage({
    super.key,
    required this.name,
    required this.connection,
    required this.database,
    this.schema,
    this.isNew = false,
  });

  /// 对象名。[listUsers] 返回的标识:MySQL 系是 `user@host`,其它类型是纯名称。
  final String name;

  /// 所属连接名
  final String connection;

  /// 所属数据库(账号是服务器级对象,库名只用于「权限」页的默认上下文)
  final String database;

  /// 所属模式(PostgreSQL / SQL Server;无模式层为 null)
  final String? schema;

  /// 是否为新建(新建 = 从空白表单开始,不回读服务端)
  final bool isNew;

  @override
  State<UserDesignPage> createState() => _UserDesignPageState();
}

class _UserDesignPageState extends State<UserDesignPage> {
  /// 表单状态(唯一数据源;所有标签都直接读写它)
  late UserSpec _spec;

  /// 「常规 / 高级」页各输入框的 controller(文本类字段需要它才能回填与取值)
  final _usernameCtrl = TextEditingController();
  final _hostCtrl = TextEditingController();
  final _passwordCtrl = TextEditingController();
  final _passwordConfirmCtrl = TextEditingController();
  final _expireDaysCtrl = TextEditingController();
  final _newPasswordCtrl = TextEditingController();
  final _commentCtrl = TextEditingController();
  final _connLimitCtrl = TextEditingController();
  final _validUntilCtrl = TextEditingController();

  /// 「权限」页新增行的输入
  final _newPrivDbCtrl = TextEditingController();
  final _newPrivTableCtrl = TextEditingController();

  /// 「成员属于」页待选角色(异步拉取)
  List<String> _grantableRoles = const [];
  bool _rolesLoading = false;

  /// 保存 / 加载进行中
  bool _saving = false;
  bool _loading = false;

  String? _error;
  String? _message;

  /// SQL 预览的只读 controller(文本在 build 中同步)
  final CodeLineEditingController _previewCtrl =
      CodeLineEditingController.fromText('');

  /// didChangeDependencies 中缓存的 AppState(dispose 时安全引用)
  AppState? _appState;

  @override
  void initState() {
    super.initState();
    _appState = context.read<AppState>();
    final acc = UserAccount.parse(widget.name);
    _spec = UserSpec(
      originalName: widget.isNew ? '' : acc.name,
      originalHost: widget.isNew ? '' : acc.host,
      username: acc.name,
      host: acc.host,
      // 新建默认造角色(连接树里本分组就叫「角色」);可取消勾选变成用户
      isRole: widget.isNew,
    );
    _syncControllers();
    if (!widget.isNew) {
      // 首帧后加载详情(initState 期间不可 setState)
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _load();
      });
    }
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _appState = context.read<AppState>();
  }

  @override
  void didUpdateWidget(UserDesignPage oldWidget) {
    super.didUpdateWidget(oldWidget);
    final changed = oldWidget.name != widget.name ||
        oldWidget.connection != widget.connection ||
        oldWidget.database != widget.database ||
        oldWidget.schema != widget.schema ||
        oldWidget.isNew != widget.isNew;
    if (!changed) return;
    // 同类设计 tab 间切换:重置为新对象(表单不跨对象保留,避免误改)
    final acc = UserAccount.parse(widget.name);
    _spec = UserSpec(
      originalName: widget.isNew ? '' : acc.name,
      originalHost: widget.isNew ? '' : acc.host,
      username: acc.name,
      host: acc.host,
      isRole: widget.isNew,
    );
    _grantableRoles = const [];
    _syncControllers();
    setState(() {
      _error = null;
      _message = null;
    });
    if (!widget.isNew) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _load();
      });
    }
  }

  @override
  void dispose() {
    for (final c in [
      _usernameCtrl,
      _hostCtrl,
      _passwordCtrl,
      _passwordConfirmCtrl,
      _expireDaysCtrl,
      _newPasswordCtrl,
      _commentCtrl,
      _connLimitCtrl,
      _validUntilCtrl,
      _newPrivDbCtrl,
      _newPrivTableCtrl,
    ]) {
      c.dispose();
    }
    _previewCtrl.dispose();
    super.dispose();
  }

  // ── 基本信息 ────────────────────────────────────────────

  /// 当前连接类型(拿不到时为空串 → 一切能力判定都退化为"不支持")
  String get _typeId =>
      _appState?.connectionByName(widget.connection)?.typeId ?? '';

  /// 账号模型
  UserObjectKind get _kind => UserSql.kindOf(_typeId);

  /// 把 [UserSpec] 的值灌进各 controller(回读 / 切换对象后调用)
  void _syncControllers() {
    _usernameCtrl.text = _spec.username;
    _hostCtrl.text = _spec.host;
    _passwordCtrl.text = _spec.password;
    _passwordConfirmCtrl.text = _spec.passwordConfirm;
    _expireDaysCtrl.text = '${_spec.expireDays}';
    _newPasswordCtrl.text = _spec.newPassword;
    _commentCtrl.text = _spec.comment;
    _connLimitCtrl.text = '${_spec.pg?.connectionLimit ?? -1}';
    _validUntilCtrl.text = _spec.pg?.validUntil ?? '';
    _newPrivDbCtrl.text = '';
    _newPrivTableCtrl.text = '';
  }

  /// 从 controller 回写 [UserSpec](每次构建 / 保存前调用,保证两侧一致)
  void _pullFromControllers() {
    _spec.username = _usernameCtrl.text.trim();
    _spec.host = _hostCtrl.text.trim();
    _spec.password = _passwordCtrl.text;
    _spec.passwordConfirm = _passwordConfirmCtrl.text;
    _spec.newPassword = _newPasswordCtrl.text;
    _spec.comment = _commentCtrl.text;
    _spec.expireDays = int.tryParse(_expireDaysCtrl.text.trim()) ?? 0;
    final pg = _spec.pg;
    if (pg != null) {
      pg.connectionLimit = int.tryParse(_connLimitCtrl.text.trim()) ?? -1;
      pg.validUntil = _validUntilCtrl.text.trim();
    }
  }

  // ── 加载与保存 ──────────────────────────────────────────

  /// 读取账号详情与权限(编辑模式)
  Future<void> _load() async {
    final app = _appState ?? context.read<AppState>();
    final l = context.l10n;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final spec = await app.readUser(
        widget.name,
        connection: widget.connection,
        database: widget.database,
      );
      if (!mounted) return;
      if (spec == null) {
        // 读不到:保留名称兜底,让用户仍能改密码 / 删号,并给出提示
        setState(() {
          _loading = false;
          _message = l.userNotSupported;
        });
        return;
      }
      setState(() {
        _spec = spec;
        _loading = false;
        _message = null;
        _syncControllers();
      });
      _loadRoles();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _loading = false;
        _error = l.userLoadFailed(widget.name, '$e');
      });
    }
  }

  /// 拉取「成员属于」的候选角色列表(失败静默 → 列表为空 + 提示文案)
  Future<void> _loadRoles() async {
    if (!UserSql.supportsMembership(_typeId)) return;
    final app = _appState ?? context.read<AppState>();
    setState(() => _rolesLoading = true);
    final roles = await app.listGrantableRoles(
      connection: widget.connection,
      database: widget.database,
    );
    if (!mounted) return;
    setState(() {
      _rolesLoading = false;
      _grantableRoles = roles;
    });
  }

  /// 保存:把表单铺开成语句逐条执行(与 SQL 预览同源)
  Future<void> _onSave() async {
    _pullFromControllers();
    final l = context.l10n;

    // 前置校验:用户名必填、两次密码一致
    if (!_spec.account.isValid) {
      setState(() {
        _error = l.userNameRequired;
        _message = null;
      });
      return;
    }
    if (_spec.password.isNotEmpty && _spec.password != _spec.passwordConfirm) {
      setState(() {
        _error = l.userPasswordMismatch;
        _message = null;
      });
      return;
    }

    final app = _appState ?? context.read<AppState>();
    setState(() {
      _saving = true;
      _error = null;
      _message = null;
    });
    final outcome = await app.saveUser(
      _spec,
      connection: widget.connection,
      database: widget.database,
      schema: widget.schema,
    );
    if (!mounted) return;
    if (!outcome.ok) {
      setState(() {
        _saving = false;
        // failedAt 是 1 基下标,0 表示非批量失败(拼接 / 连接层就挂了)
        _error = outcome.failedAt <= 0
            ? '${outcome.error}'
            : '${l.userSaveFailedAt('${outcome.failedAt}')} ${outcome.error}';
      });
      return;
    }
    setState(() {
      _saving = false;
      // 保存后本页转为"编辑态":原账号名更新为当前值,
      // 否则再次保存会重复生成 RENAME
      _spec.originalName = _spec.username;
      _spec.originalHost = _spec.host;
      _spec.password = '';
      _spec.passwordConfirm = '';
      _spec.newPassword = '';
      _syncControllers();
      _message = l.userSaveOk(_spec.account.displayName);
    });
    _loadRoles();
  }

  // ── 构建 ────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final l = context.l10n;
    if (!UserSql.supportsUsers(_typeId)) {
      return Container(
        color: t.background,
        child: Empty(
          icon: const Icon(Icons.person_off_outlined),
          title: l.userNotSupported,
          description: _typeId.isEmpty ? null : '${widget.connection} · $_typeId',
          compact: true,
          maxWidth: 520,
        ),
      );
    }
    // 每次构建前把 controller 的值同步进 spec,
    // 保证 SQL 预览 / 保存看到的是最新输入
    _pullFromControllers();
    return Container(
      color: t.background,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _toolbar(t),
          _titleBar(t),
          Expanded(child: _tabs(t)),
          _statusBar(t),
        ],
      ),
    );
  }

  /// 工具栏:保存 / 刷新
  ///
  /// 底色取 secondary(栏底灰),与文档标签条里**选中标签**的底色一致 ——
  /// 本行紧贴标签条、选中的标签底边开放,同色才能连成一整块,不留接缝。
  Widget _toolbar(AppPalette t) {
    final l = context.l10n;
    return Container(
      height: 34,
      color: t.secondary,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        children: [
          ToolbarButton(
            icon: Icons.save_outlined,
            text: l.btnSave,
            enabled: !_saving && !_loading,
            onTap: _onSave,
          ),
          _divider(t),
          ToolbarButton(
            icon: Icons.refresh,
            text: l.userRefresh,
            enabled: !_saving && !_loading && !widget.isNew,
            onTap: _load,
          ),
        ],
      ),
    );
  }

  /// 标题行:设计角色: name @ 连接.库
  Widget _titleBar(AppPalette t) {
    final l = context.l10n;
    final verb = widget.isNew
        ? l.actionNew(l.catRole)
        : l.actionDesign(l.catRole);
    return Container(
      height: 28,
      alignment: Alignment.centerLeft,
      padding: const EdgeInsets.only(left: 12),
      color: t.secondary,
      child: Text(
        '$verb: ${widget.name} @ ${widget.connection}.${widget.database}',
        style: TextStyle(
          fontSize: 12.5,
          color: t.mutedForeground,
          decoration: TextDecoration.none,
          fontWeight: FontWeight.w400,
        ),
        overflow: TextOverflow.ellipsis,
      ),
    );
  }

  /// 七个标签(对齐 Navicat 的用户编辑器)
  Widget _tabs(AppPalette t) {
    final l = context.l10n;
    return TabControl(
      contentPadding: EdgeInsets.zero,
      tabBarColor: t.secondary,
      selectedTabColor: t.surface,
      tabs: [
        TabItem(label: l.userTabGeneral, child: _generalTab(t)),
        TabItem(label: l.userTabAdvanced, child: _advancedTab(t)),
        TabItem(label: l.userTabMemberOf, child: _memberOfTab(t)),
        TabItem(label: l.userTabMembers, child: _membersTab(t)),
        TabItem(
            label: l.userTabServerPrivileges, child: _serverPrivilegesTab(t)),
        TabItem(label: l.userTabPrivileges, child: _privilegesTab(t)),
        TabItem(label: l.userTabSqlPreview, child: _previewTab(t)),
      ],
    );
  }

  /// ── 常规 ── 用户名 / 主机 / 插件 / 密码 / 确认密码 / 过期策略
  Widget _generalTab(AppPalette t) {
    final l = context.l10n;
    final plugins = AuthPluginCatalog.of(_typeId);
    final hasHost = UserSql.hasHost(_typeId);
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          FieldRow(
            label: l.userFieldUsername,
            labelWidth: 110,
            child: _wide(260, _usernameCtrl, enabled: !_saving),
          ),
          const SizedBox(height: 10),
          if (hasHost) ...[
            FieldRow(
              label: l.userFieldHost,
              labelWidth: 110,
              child: _wide(260, _hostCtrl, enabled: !_saving, hint: '%'),
            ),
            const SizedBox(height: 10),
          ],
          if (plugins.isNotEmpty) ...[
            FieldRow(
              label: l.userFieldPlugin,
              labelWidth: 110,
              child: SizedBox(
                width: 260,
                child: ComboBox<String>(
                  items: plugins,
                  value: plugins.contains(_spec.plugin) ? _spec.plugin : null,
                  itemToString: (v) => v,
                  hint: l.userExpireDefault,
                  enabled: !_saving,
                  onChanged: (v) => setState(() => _spec.plugin = v ?? ''),
                ),
              ),
            ),
            const SizedBox(height: 10),
          ],
          FieldRow(
            label: l.userFieldPassword,
            labelWidth: 110,
            child: SizedBox(
              width: 260,
              child: Input(
                controller: _passwordCtrl,
                enabled: !_saving,
                obscureText: true,
                obscureToggle: true,
                hint: widget.isNew ? null : '••••••',
              ),
            ),
          ),
          const SizedBox(height: 10),
          FieldRow(
            label: l.userFieldPasswordConfirm,
            labelWidth: 110,
            child: SizedBox(
              width: 260,
              child: Input(
                controller: _passwordConfirmCtrl,
                enabled: !_saving,
                obscureText: true,
                obscureToggle: true,
                hint: widget.isNew ? null : '••••••',
              ),
            ),
          ),
          const SizedBox(height: 10),
          FieldRow(
            label: l.userFieldExpirePolicy,
            labelWidth: 110,
            child: SizedBox(
              width: 260,
              child: ComboBox<PasswordExpirePolicy>(
                items: PasswordExpirePolicy.values,
                value: _spec.expirePolicy,
                itemToString: (v) => _expireLabel(l, v),
                enabled: !_saving,
                onChanged: (v) => setState(() => _spec.expirePolicy =
                    v ?? PasswordExpirePolicy.defaultPolicy),
              ),
            ),
          ),
          if (_spec.expirePolicy == PasswordExpirePolicy.interval) ...[
            const SizedBox(height: 10),
            FieldRow(
              label: l.userFieldExpireDays,
              labelWidth: 110,
              child: _wide(120, _expireDaysCtrl, enabled: !_saving),
            ),
          ],
          const SizedBox(height: 10),
          FieldRow(
            label: l.userFieldComment,
            labelWidth: 110,
            child: _wide(320, _commentCtrl, enabled: !_saving),
          ),
          // 「这是角色」只在新建时有意义:已有账号的用户 / 角色属性不能互改
          if (_kind == UserObjectKind.mysqlAccount && widget.isNew) ...[
            const SizedBox(height: 18),
            CheckBox(
              value: _spec.isRole,
              enabled: !_saving,
              label: l.userIsRole,
              onChanged: (v) => setState(() => _spec.isRole = v ?? false),
            ),
          ],
        ],
      ),
    );
  }

  /// 定宽输入框(FieldRow 的 child 需要显式宽度,否则 TextField 会断言无界宽)
  Widget _wide(
    double width,
    TextEditingController c, {
    required bool enabled,
    String? hint,
  }) =>
      SizedBox(
        width: width,
        child: Input(controller: c, enabled: enabled, hint: hint),
      );

  /// 过期策略的显示名(DEFAULT 保持英文原样,与 Navicat 一致)
  String _expireLabel(AppLocalizations l, PasswordExpirePolicy p) => switch (p) {
        PasswordExpirePolicy.defaultPolicy => l.userExpireDefault,
        PasswordExpirePolicy.expired => l.userExpireExpired,
        PasswordExpirePolicy.never => l.userExpireNever,
        PasswordExpirePolicy.interval => l.userExpireInterval,
      };

  /// ── 高级 ── 按类型切换:PG 角色属性 / SQL Server 主体类型 / MySQL 新密码
  Widget _advancedTab(AppPalette t) {
    final l = context.l10n;
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            l.userAdvancedHint,
            style: TextStyle(fontSize: 12, color: t.mutedForeground),
          ),
          const SizedBox(height: 16),
          ..._advancedFields(t, l),
        ],
      ),
    );
  }

  List<Widget> _advancedFields(AppPalette t, AppLocalizations l) {
    switch (_kind) {
      case UserObjectKind.pgRole:
        final pg = _spec.pg ??= PgRoleAttributes();
        return [
          _checkRow(l.userPgLogin, pg.canLogin,
              (v) => setState(() => pg.canLogin = v)),
          _checkRow(l.userPgSuper, pg.superUser,
              (v) => setState(() => pg.superUser = v)),
          _checkRow(l.userPgCreateDb, pg.createDb,
              (v) => setState(() => pg.createDb = v)),
          _checkRow(l.userPgCreateRole, pg.createRole,
              (v) => setState(() => pg.createRole = v)),
          _checkRow(l.userPgInherit, pg.inherit,
              (v) => setState(() => pg.inherit = v)),
          _checkRow(l.userPgReplication, pg.replication,
              (v) => setState(() => pg.replication = v)),
          _checkRow(l.userPgBypassRls, pg.bypassRls,
              (v) => setState(() => pg.bypassRls = v)),
          const SizedBox(height: 14),
          FieldRow(
            label: l.userFieldConnectionLimit,
            labelWidth: 130,
            child: _wide(120, _connLimitCtrl, enabled: !_saving),
          ),
          const SizedBox(height: 10),
          FieldRow(
            label: l.userFieldValidUntil,
            labelWidth: 130,
            child: _wide(260, _validUntilCtrl,
                enabled: !_saving, hint: 'infinity / 2026-12-31'),
          ),
          const SizedBox(height: 14),
          _newPasswordRow(l, 130),
        ];
      case UserObjectKind.sqlPrincipal:
        return [
          FieldRow(
            label: l.userFieldPrincipalType,
            labelWidth: 130,
            child: SizedBox(
              width: 220,
              child: ComboBox<SqlPrincipalType>(
                items: SqlPrincipalType.values,
                value: _spec.sqlPrincipalType,
                itemToString: (v) => _principalLabel(l, v),
                // 主体类型决定 CREATE 的形状,只能在新建时选
                enabled: !_saving && widget.isNew,
                onChanged: (v) => setState(() => _spec.sqlPrincipalType =
                    v ?? SqlPrincipalType.sqlUser),
              ),
            ),
          ),
          const SizedBox(height: 14),
          _newPasswordRow(l, 130),
        ];
      case UserObjectKind.mysqlAccount:
        return [
          _newPasswordRow(l, 130),
          const SizedBox(height: 10),
          Text(
            'MySQL 8 起可用 ALTER USER … IDENTIFIED BY 单独改密码;'
            '留空表示不改动。',
            style: TextStyle(fontSize: 11.5, color: t.mutedForeground),
          ),
        ];
      case UserObjectKind.unsupported:
        return [Text(l.userNotSupported)];
    }
  }

  Widget _newPasswordRow(AppLocalizations l, double labelWidth) => FieldRow(
        label: l.userFieldNewPassword,
        labelWidth: labelWidth,
        child: SizedBox(
          width: 260,
          child: Input(
            controller: _newPasswordCtrl,
            enabled: !_saving,
            obscureText: true,
            obscureToggle: true,
          ),
        ),
      );

  String _principalLabel(AppLocalizations l, SqlPrincipalType v) => switch (v) {
        SqlPrincipalType.sqlUser => l.userPrincipalSql,
        SqlPrincipalType.windowsUser => l.userPrincipalWindows,
        SqlPrincipalType.windowsGroup => l.userPrincipalWindowsGroup,
        SqlPrincipalType.databaseRole => l.userPrincipalRole,
      };

  Widget _checkRow(String label, bool value, ValueChanged<bool> onChanged) =>
      Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: CheckBox(
          value: value,
          label: label,
          enabled: !_saving,
          onChanged: (v) => onChanged(v ?? false),
        ),
      );

  /// ── 成员属于 ── 本账号加入了哪些角色。
  ///
  /// 两栏:左 = 已加入(取消勾选即退出),右 = 可选角色(勾选即加入)。
  /// 不用"选中 + 中间箭头"那套:勾选本身就是状态,中间按钮只会变成死 UI。
  Widget _memberOfTab(AppPalette t) {
    final l = context.l10n;
    if (!UserSql.supportsMembership(_typeId)) {
      return _unsupported(l.userRoleOnlyHint);
    }
    final selected = _spec.memberOf.toList()..sort();
    // 已加入但不在候选表里的(历史角色 / 权限不足读不到)也要能取消
    final candidates = <String>{
      ..._grantableRoles,
      ..._spec.memberOf,
    }.toList()
      ..sort();
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 14, 20, 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            l.userMembershipHint,
            style: TextStyle(fontSize: 11.5, color: t.mutedForeground),
          ),
          const SizedBox(height: 10),
          Expanded(
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Expanded(
                  child: _rolePane(
                    t: t,
                    title: l.userSelectedRoles,
                    items: selected,
                    // 左栏本来就全是"已加入",勾选态恒为 true,取消即退出
                    checked: (_) => true,
                    empty: l.userNoMemberOf,
                    onToggle: (role, checked) => setState(() {
                      if (!checked) _spec.memberOf.remove(role);
                    }),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: _rolePane(
                    t: t,
                    title: l.userCandidateRoles,
                    items: candidates,
                    checked: (r) => _spec.memberOf.contains(r),
                    empty: _rolesLoading ? l.userLoading : l.userNoCandidates,
                    onToggle: (role, checked) => setState(() {
                      if (checked) {
                        _spec.memberOf.add(role);
                      } else {
                        _spec.memberOf.remove(role);
                      }
                    }),
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  /// 一栏带标题的勾选列表(角色可能上百,一律懒建)
  Widget _rolePane({
    required AppPalette t,
    required String title,
    required List<String> items,
    required bool Function(String) checked,
    required String empty,
    required void Function(String, bool) onToggle,
  }) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: 24,
          color: t.secondary,
          padding: const EdgeInsets.symmetric(horizontal: 8),
          alignment: Alignment.centerLeft,
          child: Text(
            title,
            style: TextStyle(fontSize: 12, color: t.mutedForeground),
          ),
        ),
        Expanded(
          child: Container(
            decoration: BoxDecoration(
              color: t.surface,
              border: Border.all(color: t.border),
            ),
            child: items.isEmpty
                ? Padding(
                    padding: const EdgeInsets.all(8),
                    child: Text(
                      empty,
                      style: TextStyle(fontSize: 12, color: t.mutedForeground),
                    ),
                  )
                : ListView.builder(
                    itemCount: items.length,
                    itemExtent: 26,
                    itemBuilder: (context, i) {
                      final name = items[i];
                      return SizedBox(
                        height: 26,
                        child: CheckBox(
                          value: checked(name),
                          label: name,
                          enabled: !_saving,
                          onChanged: (v) => onToggle(name, v ?? false),
                        ),
                      );
                    },
                  ),
          ),
        ),
      ],
    );
  }

  /// ── 成员 ── 哪些账号加入了本角色(只读:反向关系由对方账号维护)
  Widget _membersTab(AppPalette t) {
    final l = context.l10n;
    if (!UserSql.supportsMembership(_typeId)) {
      return _unsupported(l.userRoleOnlyHint);
    }
    final members = _spec.members.toList()..sort();
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            l.userMembershipHint,
            style: TextStyle(fontSize: 12, color: t.mutedForeground),
          ),
          const SizedBox(height: 14),
          if (members.isEmpty)
            Text(
              l.userNoMembers,
              style: TextStyle(fontSize: 12.5, color: t.mutedForeground),
            )
          else
            // 大列表懒建(项目约定:不用 Column 一次性 mount 几百行)
            SizedBox(
              height: (members.length * 26.0).clamp(26.0, 260.0),
              width: 320,
              child: Container(
                decoration: BoxDecoration(
                  color: t.surface,
                  border: Border.all(color: t.border),
                ),
                child: ListView.builder(
                  itemCount: members.length,
                  itemExtent: 26,
                  itemBuilder: (context, i) => Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 8),
                    child: Align(
                      alignment: Alignment.centerLeft,
                      child: Text(
                        members[i],
                        style: TextStyle(fontSize: 12.5, color: t.foreground),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }

  /// ── 服务器权限 ── 全局权限(库名为空 = `*.*`)
  Widget _serverPrivilegesTab(AppPalette t) {
    final l = context.l10n;
    if (!UserSql.supportsPrivileges(_typeId)) {
      return _unsupported(l.userNotSupported);
    }
    return _privilegeTable(
      t: t,
      entries: _spec.serverPrivileges,
      serverLevel: true,
      hint: l.userPrivilegeHint,
      onAdd: () => setState(() => _spec.serverPrivileges
          .add(UserPrivilegeEntry(privileges: {'SELECT'}))),
    );
  }

  /// ── 权限 ── 库 / 表级权限(顶部一行「库.表」+ 添加)
  Widget _privilegesTab(AppPalette t) {
    final l = context.l10n;
    if (!UserSql.supportsPrivileges(_typeId)) {
      return _unsupported(l.userNotSupported);
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(20, 14, 20, 6),
          child: Row(
            children: [
              SizedBox(
                width: 170,
                child: Input(
                  controller: _newPrivDbCtrl,
                  enabled: !_saving,
                  hint: l.userPrivilegeDatabase,
                ),
              ),
              const SizedBox(width: 6),
              SizedBox(
                width: 170,
                child: Input(
                  controller: _newPrivTableCtrl,
                  enabled: !_saving,
                  hint: l.userPrivilegeTable,
                ),
              ),
              const SizedBox(width: 12),
              Button(
                text: l.userAddPrivilege,
                onPressed: _saving
                    ? null
                    : () => setState(() {
                          _spec.privileges.add(UserPrivilegeEntry(
                            database: _newPrivDbCtrl.text.trim(),
                            table: _newPrivTableCtrl.text.trim(),
                            privileges: {'SELECT'},
                          ));
                          _newPrivDbCtrl.text = '';
                          _newPrivTableCtrl.text = '';
                        }),
              ),
            ],
          ),
        ),
        Expanded(
          child: _privilegeTable(
            t: t,
            entries: _spec.privileges,
            serverLevel: false,
            hint: l.userPrivilegeHint,
            onAdd: null,
          ),
        ),
      ],
    );
  }

  /// 权限表格(服务器权限与库权限共用):
  /// 每行 = 一个对象(`*.*` / `db.*` / `db.tbl`)+ 权限名 + 可转授 + 移除。
  ///
  /// 权限名用自由文本输入而不是下拉穷举:GRANT 里的权限常是复合项
  /// (`SELECT, INSERT`),下拉反而挡路;空 = 该行不授权。
  Widget _privilegeTable({
    required AppPalette t,
    required List<UserPrivilegeEntry> entries,
    required bool serverLevel,
    required String hint,
    required VoidCallback? onAdd,
  }) {
    final l = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(20, 12, 20, 8),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  hint,
                  style: TextStyle(fontSize: 11.5, color: t.mutedForeground),
                ),
              ),
              if (onAdd != null)
                Button(
                  text: l.userAddPrivilege,
                  onPressed: _saving ? null : onAdd,
                ),
            ],
          ),
        ),
        Expanded(
          child: entries.isEmpty
              ? Empty(
                  icon: const Icon(Icons.lock_outline),
                  title: l.userNoPrivileges,
                  compact: true,
                )
              : Container(
                  margin: const EdgeInsets.fromLTRB(20, 0, 20, 16),
                  decoration: BoxDecoration(
                    color: t.surface,
                    border: Border.all(color: t.border),
                  ),
                  // 懒建:100+ 行权限是常态,不用 Column 一次性 mount
                  child: ListView.builder(
                    itemCount: entries.length,
                    itemBuilder: (context, i) => _PrivilegeRow(
                      // 稳定 key:按 (db, table, column) 而非下标,
                      // 增删中间行时不会把输入框的编辑状态错配到别的行
                      key: ValueKey('${entries[i].database}.'
                          '${entries[i].table}.${entries[i].column}'),
                      entry: entries[i],
                      serverLevel: serverLevel,
                      enabled: !_saving,
                      onRemoved: () => setState(() => entries.removeAt(i)),
                    ),
                  ),
                ),
        ),
      ],
    );
  }

  /// ── SQL 预览 ── 与保存同源的语句列表(只读)
  Widget _previewTab(AppPalette t) {
    _pullFromControllers();
    final text = UserSql.buildScriptText(_typeId, _spec);
    if (_previewCtrl.text != text) _previewCtrl.text = text;
    final isDark = t.background.computeLuminance() < 0.5;
    return CodeEditor(
      controller: _previewCtrl,
      readOnly: true,
      wordWrap: true,
      chunkAnalyzer: const NonCodeChunkAnalyzer(),
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
      style: CodeEditorStyle(
        fontFamily: 'monospace',
        fontSize: 13.5,
        fontHeight: 20 / 13.5,
        textColor: t.foreground,
        backgroundColor: t.surface,
        codeTheme: CodeHighlightTheme(
          languages: {
            'sql': CodeHighlightThemeMode(mode: langSql),
          },
          theme: {
            ...(isDark ? sqlDarkTheme : sqlLightTheme),
            'root': TextStyle(color: t.foreground),
          },
        ),
      ),
      indicatorBuilder:
          (context, editingController, chunkController, notifier) =>
              DefaultCodeLineNumber(
        controller: editingController,
        notifier: notifier,
        textStyle: TextStyle(fontSize: 12.5, color: t.disabledForeground),
        focusedTextStyle: TextStyle(fontSize: 12.5, color: t.mutedForeground),
        minNumberCount: 4,
      ),
    );
  }

  /// 该标签在当前数据库类型下不适用时的占位(保留标签,内容说明原因)
  Widget _unsupported(String reason) {
    final t = Tokens.of(context);
    return Container(
      color: t.surface,
      child: Empty(
        icon: const Icon(Icons.info_outline),
        title: context.l10n.userNotSupported,
        description: reason,
        compact: true,
        maxWidth: 460,
      ),
    );
  }

  /// 状态栏:保存 / 加载进度、结果、错误
  Widget _statusBar(AppPalette t) {
    final l = context.l10n;
    final isError = _error != null;
    final busy = _saving || _loading;
    final text = _error ?? (busy ? l.userLoading : null) ?? _message;
    if (text == null) return const SizedBox.shrink();
    return Container(
      height: 26,
      color: t.secondary,
      padding: const EdgeInsets.only(left: 12, right: 8),
      child: Row(
        children: [
          if (busy) ...[
            const Spinner(size: 13),
            const SizedBox(width: 6),
          ] else if (isError) ...[
            const Icon(Icons.error_outline, size: 14, color: Color(0xffd93025)),
            const SizedBox(width: 6),
          ],
          Expanded(
            child: Text(
              text,
              style: TextStyle(
                fontSize: 12,
                color: isError ? const Color(0xffd93025) : t.mutedForeground,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      ),
    );
  }

  Widget _divider(AppPalette t) => SizedBox(
        width: 6,
        height: 16,
        child: Center(
          child: Separator(
            orientation: Axis.vertical,
            thickness: 1,
            color: t.divider,
          ),
        ),
      );
}

/// 一行权限:对象 + 权限名 + 可转授 + 移除。
///
/// 单独提成 StatefulWidget 是为了**让输入框拥有自己的 controller**:
/// 若在 build 里现造 controller,每次重建都会丢掉光标与正在输入的文本。
/// [key] 由宿主按 (db, table, column) 给定,增删中间行时状态不会错配。
class _PrivilegeRow extends StatefulWidget {
  const _PrivilegeRow({
    super.key,
    required this.entry,
    required this.serverLevel,
    required this.enabled,
    required this.onRemoved,
  });

  final UserPrivilegeEntry entry;
  final bool serverLevel;
  final bool enabled;
  final VoidCallback onRemoved;

  @override
  State<_PrivilegeRow> createState() => _PrivilegeRowState();
}

class _PrivilegeRowState extends State<_PrivilegeRow> {
  late final TextEditingController _targetCtrl;
  late final TextEditingController _privCtrl;

  @override
  void initState() {
    super.initState();
    _targetCtrl = TextEditingController(text: widget.entry.target);
    _privCtrl = TextEditingController(text: _privilegeText(widget.entry));
  }

  @override
  void dispose() {
    _targetCtrl.dispose();
    _privCtrl.dispose();
    super.dispose();
  }

  static String _privilegeText(UserPrivilegeEntry e) {
    final list = e.privileges.toList()..sort();
    return list.join(', ');
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final l = context.l10n;
    return Container(
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: t.gridLine)),
      ),
      padding: const EdgeInsets.fromLTRB(8, 4, 4, 4),
      child: Row(
        children: [
          // 对象列:服务器级固定 `*.*`;库级可编辑(`db` / `db.tbl`)
          SizedBox(
            width: 200,
            child: widget.serverLevel
                ? Align(
                    alignment: Alignment.centerLeft,
                    child: Text(
                      widget.entry.target,
                      style: TextStyle(fontSize: 12, color: t.mutedForeground),
                    ),
                  )
                : Input(
                    controller: _targetCtrl,
                    enabled: widget.enabled,
                    hint: l.userPrivilegeTargetHint,
                    onChanged: (v) {
                      final parts = v.split('.');
                      widget.entry.database =
                          parts.isNotEmpty ? parts[0].trim() : '';
                      widget.entry.table = parts.length > 1 ? parts[1].trim() : '';
                      widget.entry.column =
                          parts.length > 2 ? parts[2].trim() : '';
                    },
                  ),
          ),
          const SizedBox(width: 12),
          // 权限名:自由文本(`SELECT, INSERT`);空 = 该行不授权
          Expanded(
            child: Input(
              controller: _privCtrl,
              enabled: widget.enabled,
              hint: l.userPrivilegeNames,
              onChanged: (v) {
                widget.entry.privileges
                  ..clear()
                  ..addAll(v
                      .split(',')
                      .map((s) => s.trim().toUpperCase())
                      .where((s) => s.isNotEmpty));
              },
            ),
          ),
          const SizedBox(width: 12),
          // 可转授
          SizedBox(
            width: 86,
            child: CheckBox(
              value: widget.entry.grantOption,
              enabled: widget.enabled,
              label: l.userColGrant,
              onChanged: (v) => setState(() {
                widget.entry.grantOption = v ?? false;
              }),
            ),
          ),
          ToolbarButton(
            icon: Icons.close,
            tooltip: l.userRemovePrivilege,
            onTap: widget.enabled ? widget.onRemoved : null,
          ),
        ],
      ),
    );
  }
}
