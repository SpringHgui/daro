import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../data/connection_defaults.dart';
import '../data/db_data.dart';
import '../data/db_types.dart';
import '../theme/app_theme.dart';

/// 分组下拉里代表「不属于任何分组」的哨兵值(不是合法分组名)
const String kGroupNone = '<未分组>';

/// 分组下拉里代表「去新建一个分组」的哨兵值(选中后由宿主弹窗收名字)
const String kGroupNew = '<新建分组…>';

/// "新建连接"配置页(连接向导第二步)。
///
/// 风格:顶部品牌区(固定) → Tab 栏(常规/高级/数据库/SSL/SSH/HTTP/备注)
/// → 表单内容 → 底部按钮(测试连接/URI.../上一步/确定/取消)。
class ConnectionFormPage extends StatefulWidget {
  const ConnectionFormPage({
    super.key,
    required this.type,
    this.onBack,
    required this.onCancel,
    required this.onConfirm,
    required this.onTestConnection,
    this.initial,
    this.groupOptions = const [],
    this.onCreateGroup,
  });

  /// 已选中的数据库类型(决定默认端口/用户名/标题)
  final DbType type;

  /// 点击「上一步」时回调(切换回类型选择步,不会关闭弹窗)。
  /// 编辑模式传 null,隐藏该按钮(无类型选择步可回退)
  final VoidCallback? onBack;

  /// 点击「取消」时回调(直接关闭整个弹窗)
  final VoidCallback onCancel;

  /// 点击「确定」时回调,携带表单收集到的连接信息
  final ValueChanged<ConnectionInfo> onConfirm;

  /// 点击「测试连接」时由宿主执行:表单不自己取 AppState,连库交给弹窗宿主
  final Future<(bool, String)> Function(ConnectionInfo) onTestConnection;

  /// 编辑已有连接时传入的初始配置(新建时为空,表单使用默认值)
  final ConnectionInfo? initial;

  /// 可选的既有分组名(下拉候选)。由宿主从 AppState 传入,表单不自己取状态。
  final List<String> groupOptions;

  /// 请求新建分组:宿主弹窗收名字并登记,返回最终分组名(null = 取消)
  final Future<String?> Function()? onCreateGroup;

  @override
  State<ConnectionFormPage> createState() => _ConnectionFormPageState();
}

class _ConnectionFormPageState extends State<ConnectionFormPage> {
  // 常规 Tab 字段
  late final TextEditingController _nameController;
  late final TextEditingController _hostController;
  late final TextEditingController _portController;
  late final TextEditingController _userController;
  late final TextEditingController _passController;
  late final TextEditingController _dbFileController;
  bool _savePassword = true;

  // 备注 Tab 字段
  late final TextEditingController _notesController;

  /// footer 上方状态提示条的消息(为空时不显示)
  String? _statusMessage;

  /// 消息结果:null = 普通提示;true = 成功;false = 失败(决定图标与颜色)
  bool? _statusIsSuccess;

  /// 在弹窗内部的状态提示条上显示一条消息(替代主界面的 SnackBar)
  void _setStatus(String message, {bool? isSuccess}) {
    setState(() {
      _statusMessage = message;
      _statusIsSuccess = isSuccess;
    });
  }

  /// 取类型 label 的第一行(去除 \n 第二行),用于连接名称为空时回退
  String get _shortLabel => widget.type.label.split('\n').first;

  /// 当前选中类型是否为 SQLite(文件型数据库,表单字段不同于 C/S 架构)
  bool get _isSqlite => widget.type.id == 'sqlite';

  /// 当前选中类型是否为 Access(同为文件型数据库)
  bool get _isAccess => widget.type.id == 'access';

  /// 文件型数据库(SQLite / Access):仅显示连接名称 + 文件路径
  bool get _isFileBased => _isSqlite || _isAccess;

  /// 当前选中类型是否为 SQL Server(隐藏端口、增加验证方式)
  bool get _isSqlServer =>
      widget.type.id == 'sqlserver' || widget.type.id == 'aliyun-rds-sqlserver';

  /// SQL Server 验证方式:'sql' = SQL Server 身份验证,'windows' = Windows 身份验证
  String _authMethod = 'sql';

  /// 所属连接分组;空串 = 未分组。下拉里选中项由 [kGroupNone] 表示
  String _group = '';

  /// 分组候选(initState 里并入 initial.group,避免编辑模式下拉缺项)
  late final List<String> _groupOptions;

  /// 分组下拉的选中处理:两个哨兵值各自翻译,[kGroupNew] 交宿主弹窗收名字,
  /// 取消则保持原值不动(不能把用户已选的分组清空)。
  Future<void> _selectGroup(String picked) async {
    if (picked == kGroupNone) {
      setState(() => _group = '');
      return;
    }
    if (picked != kGroupNew) {
      setState(() => _group = picked);
      return;
    }
    final created = await widget.onCreateGroup?.call();
    if (created != null && mounted) setState(() => _group = created);
  }

  /// 测试连接是否进行中(防止重复点击)
  bool _testing = false;

  /// 「测试连接」:把当前值整理成配置,交宿主真连一次(连上即断)
  Future<void> _testConnection() async {
    if (_testing) return;
    setState(() => _testing = true);
    _setStatus('正在连接 ${_hostController.text.trim()}...');

    final (isSuccess, message) =
        await widget.onTestConnection(ConnectionInfo(
      name: '',
      typeId: widget.type.id,
      host: _isFileBased
          ? _dbFileController.text.trim()
          : _hostController.text.trim(),
      port: (_isFileBased || _isSqlServer) ? '' : _portController.text.trim(),
      username: _isFileBased ? '' : _userController.text.trim(),
      // Access 的文件级数据库密码走 password;SQLite 无密码概念恒为空
      password: (_isAccess || !_isFileBased) ? _passController.text : '',
      database: _isFileBased ? _dbFileController.text.trim() : '',
      authMethod: _isSqlServer ? _authMethod : '',
    ));
    if (!mounted) return;
    setState(() => _testing = false);
    _setStatus(message, isSuccess: isSuccess);
  }

  /// 弹出文件选择对话框,让用户选择数据库文件
  Future<void> _browseFile() async {
    final XTypeGroup typeGroup;
    if (_isAccess) {
      typeGroup = const XTypeGroup(
        label: 'Access 数据库',
        extensions: ['accdb', 'mdb'],
      );
    } else {
      typeGroup = const XTypeGroup(
        label: 'SQLite 数据库',
        extensions: ['db', 'sqlite', 'sqlite3'],
      );
    }
    final file = await openFile(
      acceptedTypeGroups: [typeGroup, const XTypeGroup(label: '所有文件')],
      initialDirectory: _dbFileController.text.trim().isNotEmpty
          ? _dbFileController.text.trim()
          : null,
    );
    if (file != null) {
      _dbFileController.text = file.path;
    }
  }

  @override
  void initState() {
    super.initState();
    final initial = widget.initial;
    _nameController = TextEditingController(text: initial?.name ?? '');
    _hostController = TextEditingController(
        text: initial?.host ?? 'localhost');
    _portController = TextEditingController(
        text: (initial != null && initial.port.isNotEmpty)
            ? initial.port
            : defaultPortFor(widget.type.id));
    _userController = TextEditingController(
        text: initial?.username ?? defaultUsernameFor(widget.type.id));
    _passController = TextEditingController(text: initial?.password ?? '');
    _dbFileController = TextEditingController(text: _filePathOf(initial));
    _notesController = TextEditingController();
    if (_isSqlServer && initial != null) {
      _authMethod = initial.authMethod.isEmpty ? 'sql' : initial.authMethod;
    }
    if (initial != null) {
      _savePassword = initial.password.isNotEmpty;
      _group = initial.group;
    }
    // 分组候选要含它自己当前所在分组,否则下拉会显示成「未分组」
    _groupOptions = [
      for (final g in widget.groupOptions)
        if (g != kGroupNone && g != kGroupNew) g,
      if (initial != null && initial.group.isNotEmpty) initial.group,
    ];
  }

  /// 文件型数据库(SQLite / Access)的路径:表单把路径同时写入 host 与 database,
  /// 读取时优先取 database,为空时回退 host
  String _filePathOf(ConnectionInfo? initial) {
    if (initial == null) return '';
    return initial.database.isNotEmpty ? initial.database : initial.host;
  }

  @override
  void dispose() {
    _nameController.dispose();
    _hostController.dispose();
    _portController.dispose();
    _userController.dispose();
    _passController.dispose();
    _dbFileController.dispose();
    _notesController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    // 高度由外层 Flexible 提供(自适应),宽度由外层 SizedBox(960) 提供。
    // FocusTraversalGroup:让 Tab / 方向键能在弹窗内的输入框与按钮之间移动焦点
    // (WidgetOrderTraversalPolicy 支持方向键遍历)。
    return FocusTraversalGroup(
      policy: WidgetOrderTraversalPolicy(),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _header(t),
          Expanded(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: TabControl(
                initialIndex: 0,
                tabs: _buildTabs(t),
              ),
            ),
          ),
          // 状态提示条:测试连接 / URI 等操作的结果显示在这里(弹窗内部,footer 上方)
          _statusBar(t),
          Separator(thickness: 1, color: t.border),
          const SizedBox(height: 6),
          _footer(t),
        ],
      ),
    );
  }

  /// footer 上方的状态提示条。无消息时保持占位,避免高度跳变。
  /// 失败消息:点击可查看完整错误(省略号截断部分),右侧复制按钮一键复制。
  Widget _statusBar(AppPalette t) {
    // 功能强调色:与 QueryPage 运行/停止一致,主题无关的中调色(明/暗下均清晰)
    const successColor = Color(0xff2e9e4f);
    const errorColor = Color(0xffd93025);

    final (iconData, iconColor) = switch (_statusIsSuccess) {
      true => (Icons.check_circle_outline, successColor),
      false => (Icons.error_outline, errorColor),
      _ => (Icons.info_outline, t.mutedForeground),
    };
    final msg = _statusMessage;
    // 仅失败且非空时提供 查看全文 / 复制 交互
    final isError = _statusIsSuccess == false && msg != null;

    return Container(
      height: 26,
      padding: const EdgeInsets.symmetric(horizontal: 16),
      alignment: Alignment.centerLeft,
      child: Row(
        children: [
          if (msg != null) ...[
            Icon(iconData, size: 13, color: iconColor),
            const SizedBox(width: 6),
          ],
          Expanded(
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: isError ? _showStatusDetail : null,
              child: Text(
                msg ?? '',
                style: TextStyle(color: t.mutedForeground, fontSize: 12),
                overflow: TextOverflow.ellipsis,
              ),
            ),
          ),
          if (isError)
            GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: _copyStatusMessage,
              child: Padding(
                padding: const EdgeInsets.only(left: 8),
                child: Icon(Icons.copy, size: 13, color: t.mutedForeground),
              ),
            ),
        ],
      ),
    );
  }

  /// 复制完整失败信息到剪贴板(含被省略号截断的部分)
  Future<void> _copyStatusMessage() async {
    final msg = _statusMessage;
    if (msg == null || msg.isEmpty) return;
    await Clipboard.setData(ClipboardData(text: msg));
  }

  /// 失败消息点击:弹窗展示完整错误文本,便于阅读长错误
  void _showStatusDetail() {
    final msg = _statusMessage;
    if (msg == null || msg.isEmpty) return;
    MessageBox.show(
      context,
      title: '连接失败详情',
      message: msg,
      type: MessageBoxType.error,
      okText: '知道了',
    );
  }

  /// 顶部:仅保留当前数据库类型的官方图标(右对齐)
  Widget _header(AppPalette t) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 16),
      child: Align(
        alignment: Alignment.centerRight,
        child: DbTypeIcon(type: widget.type, size: 48),
      ),
    );
  }

  /// Tab 列表:每个 Tab 包含 label + 内容 widget。
  /// 目前只开放「常规」与「备注」,其余 Tab 暂时注释(待后续实现)。
  List<TabItem> _buildTabs(AppPalette t) {
    return [
      TabItem(
        label: '常规',
        child: _GeneralForm(
          name: _nameController,
          host: _hostController,
          port: _portController,
          user: _userController,
          pass: _passController,
          dbFile: _dbFileController,
          savePassword: _savePassword,
          onSavePasswordChanged: (v) => setState(() => _savePassword = v),
          isSqlite: _isFileBased,
          isAccess: _isAccess,
          isSqlServer: _isSqlServer,
          authMethod: _authMethod,
          onAuthMethodChanged: (v) => setState(() => _authMethod = v),
          group: _group,
          groupOptions: _groupOptions,
          onGroupSelected: _selectGroup,
          onBrowseFile: _browseFile,
        ),
      ),
      // const TabItem(
      //   label: '高级',
      //   child: _Placeholder(text: '高级选项(编码、超时等)'),
      // ),
      // const TabItem(
      //   label: '数据库',
      //   child: _Placeholder(text: '默认数据库 / 模式'),
      // ),
      // const TabItem(
      //   label: 'SSL',
      //   child: _Placeholder(text: 'SSL 证书与加密'),
      // ),
      // const TabItem(
      //   label: 'SSH',
      //   child: _Placeholder(text: 'SSH 隧道配置'),
      // ),
      // const TabItem(
      //   label: 'HTTP',
      //   child: _Placeholder(text: 'HTTP 隧道配置'),
      // ),
      TabItem(label: '备注', child: _NotesForm(controller: _notesController)),
    ];
  }

  /// 底部按钮
  Widget _footer(AppPalette t) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 10, 20, 12),
      child: Row(
        children: [
          Button(
            text: _testing ? '测试中...' : '测试连接',
            onPressed: _testing ? null : _testConnection,
          ),
          const SizedBox(width: 4),
          Button(
            text: 'URI...',
            onPressed: () => _setStatus('URI 导入功能尚未实现'),
          ),
          const Spacer(),
          if (widget.onBack != null) ...[
            Button(
              text: '上一步',
              onPressed: widget.onBack,
            ),
            const SizedBox(width: 8),
          ],
          Button(
            text: '确定',
            onPressed: () {
              final info = ConnectionInfo(
                name: _nameController.text.trim().isEmpty
                    ? _shortLabel
                    : _nameController.text.trim(),
                typeId: widget.type.id,
                host: _isFileBased
                    ? _dbFileController.text.trim()
                    : _hostController.text.trim(),
                port: (_isFileBased || _isSqlServer)
                    ? ''
                    : _portController.text.trim(),
                username: _isFileBased ? '' : _userController.text.trim(),
                // SQLite(文件型但非 Access)无密码;Access 数据库密码按「保存密码」勾选落盘
                password: (_isFileBased && !_isAccess)
                    ? ''
                    : (_savePassword ? _passController.text : ''),
                database: _isFileBased ? _dbFileController.text.trim() : '',
                authMethod: _isSqlServer ? _authMethod : '',
                group: _group,
                isLive: true,
              );
              if (!hasAnyDriver(info)) {
                _setStatus('${_shortLabel} 驱动尚未实现,连接后无法加载元数据');
                return;
              }
              widget.onConfirm(info);
            },
          ),
          const SizedBox(width: 8),
          Button(
            text: '取消',
            onPressed: widget.onCancel,
          ),
        ],
      ),
    );
  }
}

// ────────────────────────────────────────────────────────────
// Tab 内容
// ────────────────────────────────────────────────────────────

// 高级 / 数据库 / SSL / SSH / HTTP 等 Tab 的占位组件,待实现时随 Tab 一起恢复
// class _Placeholder extends StatelessWidget {
//   const _Placeholder({required this.text});
//   final String text;
//
//   @override
//   Widget build(BuildContext context) {
//     final t = Tokens.of(context);
//     return Padding(
//       padding: const EdgeInsets.all(24),
//       child: Center(
//         child: Text(
//           text,
//           style: TextStyle(color: t.mutedForeground, fontSize: 13),
//         ),
//       ),
//     );
//   }
// }

/// 「常规」Tab:连接名称/主机/端口/用户名/密码/保存密码
///
/// SQLite 为文件型数据库,仅展示「连接名称」与「数据库文件」两个字段,
/// 其余 C/S 字段(host/port/user/pass)隐藏。
///
/// Access 同为文件型,但额外支持「数据库密码」(在 Access 里加密过的文件),
/// 故在文件路径下再补密码 + 保存密码字段。
///
/// SQL Server 隐藏端口(固定 1433),增加验证方式下拉:
/// SQL Server 身份验证 → 显示用户名/密码;
/// Windows 身份验证 → 隐藏用户名/密码。
class _GeneralForm extends StatelessWidget {
  const _GeneralForm({
    required this.name,
    required this.host,
    required this.port,
    required this.user,
    required this.pass,
    required this.dbFile,
    required this.savePassword,
    required this.onSavePasswordChanged,
    required this.isSqlite,
    required this.isAccess,
    required this.isSqlServer,
    required this.authMethod,
    required this.onAuthMethodChanged,
    required this.group,
    required this.groupOptions,
    required this.onGroupSelected,
    required this.onBrowseFile,
  });

  final TextEditingController name;
  final TextEditingController host;
  final TextEditingController port;
  final TextEditingController user;
  final TextEditingController pass;
  final TextEditingController dbFile;
  final bool savePassword;
  final ValueChanged<bool> onSavePasswordChanged;
  final bool isSqlite;
  final bool isAccess;
  final bool isSqlServer;
  final String authMethod;
  final ValueChanged<String> onAuthMethodChanged;

  /// 当前分组(空串 = 未分组)与候选列表、选中回调(含两个哨兵值)
  final String group;
  final List<String> groupOptions;
  final ValueChanged<String> onGroupSelected;

  final VoidCallback onBrowseFile;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 24, 20, 24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // 连接名称(所有类型共有)
          FieldRow(
            label: '连接名称:',
            child: SizedBox(width: 600, child: Input(controller: name)),
          ),
          const SizedBox(height: 14),
          // 分组(所有类型共有):单层,选「新建分组…」当场起名字
          FieldRow(
            label: '分组:',
            child: SizedBox(
              width: 240,
              child: ComboBox<String>(
                items: [kGroupNone, ...groupOptions, kGroupNew],
                value: group.isEmpty ? kGroupNone : group,
                onChanged: (v) {
                  if (v != null) onGroupSelected(v);
                },
                itemToString: (v) => v == kGroupNew ? '新建分组…' : v,
              ),
            ),
          ),
          if (isSqlite) ...[
            const SizedBox(height: 14),
            FieldRow(
              label: '数据库文件:',
              child: Row(
                children: [
                  SizedBox(
                      width: 560,
                      child: Input(controller: dbFile, hint: '数据库文件路径')),
                  const SizedBox(width: 6),
                  Button(
                    text: '浏览...',
                    onPressed: onBrowseFile,
                  ),
                ],
              ),
            ),
            // Access 支持「数据库密码」(在 Access 里用密码加密过的文件);
            // SQLite 无此概念,故仅 Access 显示密码相关字段
            if (isAccess) ...[
              const SizedBox(height: 14),
              FieldRow(
                label: '数据库密码:',
                child: SizedBox(
                  width: 240,
                  child: Input(
                      controller: pass, obscureText: true, obscureToggle: true),
                ),
              ),
              const SizedBox(height: 14),
              FieldRow(
                child: CheckBox(
                  value: savePassword,
                  onChanged: (v) => onSavePasswordChanged(v ?? false),
                  label: '保存密码',
                ),
              ),
            ],
          ] else ...[
            const SizedBox(height: 14),
            // 主机
            FieldRow(
              label: '主机:',
              child: SizedBox(width: 600, child: Input(controller: host)),
            ),
            if (!isSqlServer) ...[
              const SizedBox(height: 14),
              // 端口(SQL Server 固定 1433,不显示)
              FieldRow(
                label: '端口:',
                child: SizedBox(
                  width: 100,
                  child: Input(
                      controller: port,
                      hint: '3306',
                      keyboardType: TextInputType.number),
                ),
              ),
            ],
            if (isSqlServer) ...[
              const SizedBox(height: 14),
              // 验证方式(仅 SQL Server)
              FieldRow(
                label: '验证方式:',
                child: SizedBox(
                  width: 240,
                  child: ComboBox<String>(
                    items: const ['sql', 'windows'],
                    value: authMethod,
                    onChanged: (v) {
                      if (v != null) onAuthMethodChanged(v);
                    },
                    itemToString: (v) => switch (v) {
                      'windows' => 'Windows 身份验证',
                      _ => 'SQL Server 身份验证',
                    },
                  ),
                ),
              ),
            ],
            if (authMethod != 'windows' || !isSqlServer) ...[
              const SizedBox(height: 14),
              // 用户名
              FieldRow(
                label: '用户名:',
                child: SizedBox(width: 200, child: Input(controller: user)),
              ),
              const SizedBox(height: 14),
              // 密码(obscureText 密码框,右缘眼睛按钮可切换明文查看)
              FieldRow(
                label: '密码:',
                child: SizedBox(
                  width: 240,
                  child: Input(controller: pass, obscureText: true, obscureToggle: true),
                ),
              ),
              const SizedBox(height: 14),
              // 保存密码(独立一行)
              FieldRow(
                child: CheckBox(
                  value: savePassword,
                  onChanged: (v) => onSavePasswordChanged(v ?? false),
                  label: '保存密码',
                ),
              ),
            ],
          ],
        ],
      ),
    );
  }
}

/// 「备注」Tab:多行文本
class _NotesForm extends StatelessWidget {
  const _NotesForm({required this.controller});
  final TextEditingController controller;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 16),
      child: Textarea(
        controller: controller,
        minLines: 8,
        maxLines: 8,
      ),
    );
  }
}

// ────────────────────────────────────────────────────────────
