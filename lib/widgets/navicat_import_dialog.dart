import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../app/app_state.dart';
import '../data/db_data.dart';
import '../data/db_types.dart';
import '../data/navicat_import.dart';
import '../theme/app_theme.dart';

/// 导入完成后的汇总(由调用方弹 MessageBox 告知结果)。
class NavicatImportOutcome {
  const NavicatImportOutcome({
    required this.imported,
    required this.needsManualPassword,
    this.newGroups = const [],
  });

  /// 已写入连接树的连接配置
  final List<ConnectionInfo> imported;

  /// 其中密码没能带过来(Navicat 端未保存 / 旧版加密未能解密)的条数
  final int needsManualPassword;

  /// 文件里出现、但本地原本没有,因而本次新建的连接分组名
  final List<String> newGroups;
}

/// 打开「从 Navicat 导入连接」向导。
///
/// 取消返回 null;导入成功返回 [NavicatImportOutcome](返回时连接已落盘、树已刷新)。
Future<NavicatImportOutcome?> showNavicatImportDialog(
  BuildContext context, {
  required AppState app,
}) {
  return showDialog<NavicatImportOutcome>(
    context: context,
    builder: (_) => NavicatImportDialog(app: app),
  );
}

/// Navicat `.ncx` 导入向导:选文件 → 解析并解密保存的密码 → 勾选 → 写入连接树。
///
/// 列表按文件原序展示全部条目(实测一个真实导出文件含 379 条),固定行高 +
/// `ListView.builder` 懒建,一次构建只造可见的十几行。
class NavicatImportDialog extends StatefulWidget {
  const NavicatImportDialog({super.key, required this.app});

  final AppState app;

  @override
  State<NavicatImportDialog> createState() => _NavicatImportDialogState();
}

class _NavicatImportDialogState extends State<NavicatImportDialog> {
  /// 行高固定:几百行的列表里避免逐行测量,滚动条长度也才准确
  static const double _rowHeight = 34;

  final TextEditingController _pathController = TextEditingController();

  /// 路径输入框焦点:离开输入框就自动解析(粘贴路径后不必再按回车)
  final FocusNode _pathFocus = FocusNode();

  /// 成功/警告色:随主题在 build 中刷新(与「运行 SQL 文件」对话框同一做法)
  Color _ok = AppColors.light.iconSuccess;
  Color _warn = AppColors.light.iconWarning;

  /// 当前输入 / 选中的 .ncx 路径
  String _filePath = '';

  /// 最近一次解析过的路径(含解析失败):同一路径不重复解析,以免清掉用户勾选
  String? _parsedPath;

  /// 解析结果;null 表示尚未成功解析
  NavicatNcx? _ncx;

  /// 读取或解析失败的原因,原样展示给用户(不静默吞掉)
  String? _error;

  /// 勾选中的条目下标
  final Set<int> _selected = {};

  /// 与现有连接(或文件内更早条目)重名的下标:默认不勾选,勾选后由连接树自动改名
  final Set<int> _duplicates = {};

  @override
  void initState() {
    super.initState();
    _pathFocus.addListener(_parseOnBlur);
  }

  @override
  void dispose() {
    _pathFocus.removeListener(_parseOnBlur);
    _pathFocus.dispose();
    _pathController.dispose();
    super.dispose();
  }

  /// 输入框失去焦点 = 路径写完:自动读取。仍在输入中就敲坏的路径不该报错。
  void _parseOnBlur() {
    if (_pathFocus.hasFocus) return;
    final path = _filePath.trim();
    if (path.isEmpty || path == _parsedPath) return;
    _parse();
  }

  List<NavicatConnection> get _entries => _ncx?.connections ?? const [];

  bool get _parsedOk => _ncx != null;

  bool get _hasPath => _filePath.trim().isNotEmpty;

  /// 可导入(引擎已实现)的下标集合
  Set<int> get _importable {
    final out = <int>{};
    for (var i = 0; i < _entries.length; i++) {
      if (_entries[i].isSupported) out.add(i);
    }
    return out;
  }

  Future<void> _pickFile() async {
    final file = await openFile(
      acceptedTypeGroups: const [
        // .ncx = XML 导出(Navicat);.ano = base64 JSON 导出(Another Redis
        // Desktop Manager)。解析按内容认格式,后缀只影响这里能不能选中文件。
        XTypeGroup(label: '连接导出文件', extensions: ['ncx', 'ano', 'xml']),
        XTypeGroup(label: '所有文件'),
      ],
      confirmButtonText: '打开',
    );
    final path = file?.path;
    if (path == null || !mounted) return;
    _pathController.text = path;
    setState(() => _filePath = path);
    _parse();
  }

  /// 读取并解析当前路径。失败时清空旧结果、把原因显示出来。
  void _parse() {
    final path = _filePath.trim();
    setState(() {
      _parsedPath = path.isEmpty ? null : path;
      _ncx = null;
      _selected.clear();
      _duplicates.clear();
      _error = null;
    });
    if (path.isEmpty) return;
    final NavicatNcx parsed;
    try {
      parsed = NavicatNcx.readFileSync(path);
    } on FormatException catch (e) {
      setState(() => _error = e.message);
      return;
    } catch (e) {
      // 文件被占用 / 无权限 / 只是后缀叫 .ncx 的情况,一律如实报出
      setState(() => _error = '读取文件失败:$e');
      return;
    }
    final taken = widget.app.connections.map((c) => c.name).toSet();
    final selected = <int>{};
    final duplicates = <int>{};
    for (var i = 0; i < parsed.connections.length; i++) {
      final e = parsed.connections[i];
      if (!e.isSupported) continue;
      // Navicat 允许不同引擎下同名;daro 按名称索引连接,重名默认不勾选
      if (taken.contains(e.name)) {
        duplicates.add(i);
        continue;
      }
      taken.add(e.name);
      selected.add(i);
    }
    setState(() {
      _ncx = parsed;
      _selected.addAll(selected);
      _duplicates.addAll(duplicates);
    });
  }

  void _toggle(int index) {
    setState(() {
      if (!_selected.remove(index)) _selected.add(index);
    });
  }

  void _toggleAll() {
    final importable = _importable;
    setState(() {
      if (_selected.length == importable.length) {
        _selected.clear();
      } else {
        _selected
          ..clear()
          ..addAll(importable);
      }
    });
  }

  void _import() {
    if (_selected.isEmpty) return;
    final indices = _selected.toList()..sort();
    final conns = [for (final i in indices) _entries[i].toConnection()];
    // 分组不单独走一次落盘:先记下已有分组名,addConnections 会把连接指向的
    // 未知分组一并登记(分组不存在则重建),差集即本次新建的分组。
    final before = widget.app.groupNames.toSet();
    widget.app.addConnections(conns);
    final newGroups = [
      for (final g in widget.app.groupNames)
        if (!before.contains(g)) g,
    ];
    Navigator.of(context).pop(NavicatImportOutcome(
      imported: conns,
      needsManualPassword:
          indices.where((i) => _entries[i].needsManualPassword).length,
      newGroups: newGroups,
    ));
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final colors = AppColors.of(context);
    _ok = colors.iconSuccess;
    _warn = colors.iconWarning;
    return DialogBox(
      title: '从 Navicat 导入连接',
      width: 860,
      height: 560,
      onClose: () => Navigator.of(context).pop(),
      footer: Row(
        children: [
          Button(
            text: '导入选中',
            onPressed: (_parsedOk && _selected.isNotEmpty) ? _import : null,
          ),
          const SizedBox(width: 8),
          Button(text: '取消', onPressed: () => Navigator.of(context).pop()),
          const Spacer(),
          Text(
            _parsedOk
                ? '已勾选 ${_selected.length} / 可导入 ${_importable.length}'
                : '尚未解析文件',
            style: _style(t, color: t.mutedForeground),
          ),
          const SizedBox(width: 10),
        ],
      ),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(18, 12, 18, 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            FieldRow(
              label: '导出文件:',
              labelWidth: 62,
              child: Row(
                children: [
                  Expanded(
                    child: Input(
                      controller: _pathController,
                      focusNode: _pathFocus,
                      hint: r'选择 Navicat / Another Redis Desktop Manager 导出的连接文件'
                          r'(路径也可直接粘贴),'
                          r'如 C:\Users\你\Desktop\connections.ncx 或 connections.ano',
                      onChanged: (v) => setState(() => _filePath = v),
                      onSubmitted: (_) => _parse(),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Button(text: '浏览...', onPressed: _pickFile),
                ],
              ),
            ),
            const SizedBox(height: 6),
            _statusLine(t),
            const SizedBox(height: 10),
            if (_parsedOk) ...[
              Row(
                children: [
                  CheckBox(
                    value: _selected.isNotEmpty &&
                        _selected.length == _importable.length,
                    onChanged: (_) => _toggleAll(),
                    label: '全选可导入',
                  ),
                  const Spacer(),
                  Text(_summaryText(),
                      style: _style(t, color: t.mutedForeground)),
                ],
              ),
              const SizedBox(height: 6),
              _listHeader(t),
              const SizedBox(height: 2),
              Expanded(child: _list(t)),
            ] else
              Expanded(child: _placeholder(t)),
          ],
        ),
      ),
    );
  }

  /// 文件与解析状态一行:未选 / 解析失败 / 解析成功三种情形各自明确
  Widget _statusLine(AppPalette t) {
    final err = _error;
    if (err != null) {
      return Text('解析失败:$err',
          style: _style(t, color: _warn),
          maxLines: 2,
          overflow: TextOverflow.ellipsis);
    }
    if (!_parsedOk) {
      return Text(
        _hasPath ? '路径已填:按回车或点到别处即读取该文件。' : '请先选择导出文件(.ncx / .ano)。',
        style: _style(t, color: t.mutedForeground),
      );
    }
    final ver = _ncx!.formatVersion;
    return Text(
      '已解析 ${_entries.length} 条连接${ver.isEmpty ? '' : '(Navicat 导出格式 $ver)'}。',
      style: _style(t, color: _ok),
    );
  }

  /// 右侧统计:不支持 / 重名 / 需要手填密码的数量,没有异常时说明密码都带过来了
  String _summaryText() {
    int count(bool Function(NavicatConnection) test) =>
        _entries.where(test).length;
    final unsupported = count((e) => !e.isSupported);
    final manual = count((e) => e.needsManualPassword);
    // 文件里出现的分组去重计数(未分组不计),与「是否已存在」无关,纯说清量级
    final groups = {for (final e in _entries) e.group}..remove('');
    final parts = <String>[
      if (unsupported > 0) '不可导入 $unsupported',
      if (_duplicates.isNotEmpty) '重名 ${_duplicates.length}',
      if (manual > 0) '需手填密码 $manual',
      if (groups.isNotEmpty) '分组 ${groups.length}',
    ];
    return parts.isEmpty ? '全部可导入,密码均已带过来' : parts.join(' · ');
  }

  Widget _listHeader(AppPalette t) {
    Widget cell(String text, int flex) => Expanded(
          flex: flex,
          child: Text(text,
              style: _style(t, color: t.mutedForeground, size: 11.5)),
        );
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
      child: Row(
        children: [
          const SizedBox(width: 46),
          cell('连接名称', 4),
          cell('分组', 3),
          cell('目标', 5),
          cell('用户', 3),
          cell('状态', 4),
        ],
      ),
    );
  }

  Widget _list(AppPalette t) {
    return Container(
      decoration: BoxDecoration(
        color: t.surface,
        border: Border.all(color: t.border),
      ),
      child: ListView.builder(
        itemExtent: _rowHeight,
        itemCount: _entries.length,
        itemBuilder: (_, i) => _row(t, i),
      ),
    );
  }

  Widget _row(AppPalette t, int index) {
    final e = _entries[index];
    final selected = _selected.contains(index);
    final enabled = e.isSupported;
    return Listener(
      // 按下即切换:零延迟,也不引入 Material 水波纹
      onPointerDown: enabled ? (_) => _toggle(index) : null,
      child: Container(
        decoration: BoxDecoration(
          color: selected ? t.highlight.withValues(alpha: 0.10) : null,
          border: Border(bottom: BorderSide(color: t.divider)),
        ),
        padding: const EdgeInsets.symmetric(horizontal: 6),
        child: Row(
          children: [
            // 勾选统一由整行的 Listener 处理,这里 onChanged 空实现:
            // 传 null 会被画成禁用态,传真实回调又会与整行回调重复触发
            CheckBox(value: selected, onChanged: (_) {}, enabled: enabled),
            const SizedBox(width: 6),
            _typeIcon(e),
            const SizedBox(width: 6),
            Expanded(
              flex: 4,
              child: Text(
                e.name.isEmpty ? '(未命名)' : e.name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: _style(t,
                    color: enabled ? t.foreground : t.disabledForeground,
                    size: 12.5),
              ),
            ),
            Expanded(
              flex: 3,
              child: _groupCell(t, e),
            ),
            Expanded(
              flex: 5,
              child: Text(e.targetLabel,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: _style(t, color: t.mutedForeground, size: 12)),
            ),
            Expanded(
              flex: 3,
              child: Text(e.userName.isEmpty ? '-' : e.userName,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: _style(t, color: t.mutedForeground, size: 12)),
            ),
            Expanded(flex: 4, child: _tags(t, index)),
          ],
        ),
      ),
    );
  }

  /// 分组列:本地已有同名分组只显示名字;文件里有、本地没有的标「新建」,
  /// 导入时会连分组一起重建(Navicat 原生导出的文件不带 Group,整列即 '-')。
  Widget _groupCell(AppPalette t, NavicatConnection e) {
    final group = e.group;
    if (group.isEmpty) {
      return Text('-',
          maxLines: 1,
          style: _style(t, color: t.disabledForeground, size: 12));
    }
    final isNew = !widget.app.groupNames.contains(group);
    return Text(
      isNew ? '$group ·新建' : group,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: _style(t, color: isNew ? _warn : t.mutedForeground, size: 12),
    );
  }

  /// 行尾标记:不可导入的原因 / 已存在同名 / 密码要手填 / 密码已带过来,各自说清
  Widget _tags(AppPalette t, int index) {
    final e = _entries[index];
    final (String, Color) tag;
    final blocked = e.unsupportedReason;
    if (blocked != null) {
      tag = (blocked, t.disabledForeground);
    } else if (_duplicates.contains(index)) {
      tag = ('已存在同名', t.mutedForeground);
    } else {
      tag = switch (e.passwordState) {
        NavicatPasswordState.notSaved => ('未保存密码', _warn),
        NavicatPasswordState.undecryptable => ('密码未能解密', _warn),
        // .ncx 的口令是解密得来,.ano 的 auth 本就是明文,标签只说「带过来了」
        NavicatPasswordState.decrypted => e.password.isEmpty
            ? ('无需密码', t.mutedForeground)
            : ('口令已带过来', _ok),
        NavicatPasswordState.noPassword => ('无需密码', t.mutedForeground),
      };
    }
    final (text, color) = tag;
    return Align(
      alignment: Alignment.centerLeft,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
        decoration: BoxDecoration(
          border: Border.all(color: color.withValues(alpha: 0.5)),
          borderRadius: BorderRadius.circular(3),
        ),
        child: Text(text, style: _style(t, color: color, size: 10.5)),
      ),
    );
  }

  /// 引擎图标:能对上 [kAllDbTypes] 的用官方 logo,对不上(Navicat 专有类型)留空位
  Widget _typeIcon(NavicatConnection e) {
    final type = kAllDbTypes.where((d) => d.id == e.typeId).firstOrNull;
    if (type == null) return const SizedBox(width: 18);
    return DbTypeIcon(type: type, size: 18);
  }

  /// 未解析成功时的引导区:先摆明「支持哪些工具的导出文件」,再给操作说明。
  /// 解析成功后整块让位给连接列表(见 build 的 `_parsedOk` 分支)。
  Widget _placeholder(AppPalette t) {
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 620),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text('支持的导出来源',
                textAlign: TextAlign.center,
                style: _style(t, color: t.mutedForeground, size: 11.5)),
            const SizedBox(height: 8),
            const Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                _SourceCard(
                  icon: 'assets/icons/tools/navicat.svg',
                  name: 'Navicat',
                  howto: '文件 → 导出连接设置…',
                  files: '.ncx / .xml',
                ),
                SizedBox(width: 12),
                _SourceCard(
                  icon: 'assets/icons/tools/another-redis-desktop-manager.svg',
                  name: 'Another Redis Desktop Manager',
                  howto: '设置 → 导出连接',
                  files: 'connections.ano',
                ),
              ],
            ),
            const SizedBox(height: 16),
            Text(
              '在上面的工具里导出连接配置,然后回到这里选择该文件。\n'
              '两种容器按内容识别,后缀不必手工对齐:'
              '.ncx 是 XML,.ano 是 base64 的 JSON。\n'
              '.ncx 里保存的密码按 Navicat 内置算法解密后带过来;'
              '.ano 里的 auth 本就是明文。\n'
              '分组:Navicat 原生导出没有分组信息,连接会落在「未分组」;'
              'ARDM 新版导出带的 groups 会一并还原,'
              'daro 自己导出的文件按 Group 属性恢复。',
              textAlign: TextAlign.center,
              style: _style(t, color: t.mutedForeground, size: 12.5),
            ),
          ],
        ),
      ),
    );
  }

  TextStyle _style(AppPalette t, {Color? color, double size = 12}) => TextStyle(
        fontSize: size,
        color: color ?? t.foreground,
        decoration: TextDecoration.none,
        fontWeight: FontWeight.w400,
      );
}

/// 「支持的导出来源」卡片:厂商官方标识(统一瓦片,见 assets/icons/tools/)+
/// 软件名 + 导出入口与文件后缀。
///
/// 只做本对话框的排版,不是可复用控件;底色用 `surface`,在明暗两套主题下
/// 都比外层 `background` 深一档,把瓦片与背景分开。
class _SourceCard extends StatelessWidget {
  const _SourceCard({
    required this.icon,
    required this.name,
    required this.howto,
    required this.files,
  });

  final String icon;
  final String name;
  final String howto;
  final String files;

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    return Container(
      width: 286,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      decoration: BoxDecoration(
        color: t.surface,
        border: Border.all(color: t.border),
        borderRadius: BorderRadius.circular(4),
      ),
      child: Row(
        children: [
          SvgPicture.asset(icon, width: 38, height: 38),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 13,
                      color: t.foreground,
                      decoration: TextDecoration.none,
                      fontWeight: FontWeight.w600,
                    )),
                const SizedBox(height: 2),
                Text(howto,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 11.5,
                      color: t.mutedForeground,
                      decoration: TextDecoration.none,
                      fontWeight: FontWeight.w400,
                    )),
                Text(files,
                    style: TextStyle(
                      fontSize: 11.5,
                      color: t.mutedForeground,
                      decoration: TextDecoration.none,
                      fontWeight: FontWeight.w400,
                    )),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
