import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:provider/provider.dart';
import 'package:url_launcher/url_launcher.dart';
import 'package:window_manager/window_manager.dart';
import '../app/app_state.dart';
import '../data/db_data.dart';
import '../pages/connection_dialog_page.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';
import 'about_dialog.dart';
import 'mcp_settings_dialog.dart';
import 'navicat_export_dialog.dart';
import 'navicat_import_dialog.dart';
import 'options_dialog.dart';
import 'schema_sync_dialog.dart';
import 'theme_customize_dialog.dart';

/// 与 windows/runner/flutter_window.cpp 通信,接收窗口按钮 hover 状态。
/// 鼠标进入最大化按钮区域后,系统返回 HTMAXBUTTON 接管事件,Flutter 端
/// MouseRegion 失效,必须由 native 端主动推送 hover 状态才能重绘按钮背景。
const _titlebarChannel = MethodChannel('daro/titlebar');

// 顶部菜单栏兼应用标题栏:文件/编辑/查看/收藏夹/工具/窗口/帮助。
// 菜单本身由 base_ui_flutter 的 MenuStrip 提供(点击展开 + 悬停自动切换);
// 窗口控制按钮(最小化/最大化/关闭)复用 window_manager 的 WindowCaptionButton。
//
// 因 Flutter Windows 引擎把内容固定在客户区内,要让菜单画到标题栏区域,
// 只能隐藏系统标题栏(TitleBarStyle.hidden)再自绘按钮。
// 副作用:Win11 的 Snap Layouts hover 菜单不会弹出——系统不再识别最大化按钮
// 为原生按钮(WM_NCHITTEST 不返回 HTMAXBUTTON)。修复需改 windows/runner 的
// WM_NCHITTEST,见 https://github.com/luoluoqixi/flutter_windows11_snap_layouts_examples。
class TopMenu extends StatefulWidget {
  const TopMenu({super.key});

  @override
  State<TopMenu> createState() => _TopMenuState();
}

class _TopMenuState extends State<TopMenu> with WindowListener {
  bool _isMaximized = false;
  // native 端推送的 hover 按钮(0=无;2=最大化)。仅最大化按钮需要此处维护,
  // 因为它被 HTMAXBUTTON 抢走 MouseRegion 事件。最小化/关闭按钮由各自
  // WindowCaptionButton 内部 MouseRegion 自行处理。
  int _nativeHoveredButton = 0;
  // 标题栏上次 pointer down 时间,用于自检双击(零延迟,不依赖 GestureDetector
  // 的双击判定窗口,避免单击被 hold 300ms 影响菜单首次展开)。
  // 阈值用 500ms(Windows 系统默认双击间隔),300ms 太严、用户稍慢就识别不到。
  DateTime? _lastTitleBarDown;
  // 拖拽自检测状态。不用 GestureDetector 的 pan 手势:Flutter 对鼠标
  // (precise pointer) 的 kPrecisePointerPanSlop 只有 2 逻辑像素,双击时手的
  // 轻微抖动(≥2px)就会在第一次按下后触发 onPanStart → startDragging 进入
  // Windows 模态移动循环 → 第二次 down 被吞 → 双击永远失败。改为 Listener
  // 自测移动距离,阈值放宽到 6px(> 系统双击容差 ~4px,正常双击不误触)。
  Offset? _dragStart;
  bool _dragging = false;
  // 双击后置 true,抑制本次按压期间启动拖拽(否则拖拽 modal loop 会打断
  // maximize 调用)。下次 pointer down 时重置。
  bool _suppressDrag = false;

  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
    // 异步读取初始最大化状态(窗口刚启动时为 false,但仍查询以避免竞态)
    windowManager.isMaximized().then((v) {
      if (mounted) setState(() => _isMaximized = v);
    });
    _titlebarChannel.setMethodCallHandler((call) async {
      if (call.method == 'onHoverChange' && call.arguments is int) {
        final next = call.arguments as int;
        if (next != _nativeHoveredButton && mounted) {
          setState(() => _nativeHoveredButton = next);
        }
      }
      return null;
    });
  }

  @override
  void dispose() {
    _titlebarChannel.setMethodCallHandler(null);
    windowManager.removeListener(this);
    super.dispose();
  }

  @override
  void onWindowMaximize() {
    debugPrint('[titlebar] onWindowMaximize()');
    setState(() => _isMaximized = true);
  }

  @override
  void onWindowUnmaximize() {
    debugPrint('[titlebar] onWindowUnmaximize()');
    setState(() => _isMaximized = false);
  }

  @override
  Widget build(BuildContext context) {
    final appPalette = Tokens.of(context);
    final app = context.watch<AppState>();
    final brightness = Theme.of(context).brightness;
    // 将应用语义色板桥接到 DesktopTokens,使 MenuStrip 跟随明 / 暗主题。
    // controlColor/borderColor 设为 surface:让菜单栏背景与整体一致,
    // 且 MenuStrip 自带底边框与背景同色而隐藏,整体无可见下边框。
    final dt = appPalette.desktopTokensFor(context).copyWith(
      controlColor: appPalette.surface,
      borderColor: appPalette.surface,
      // hover / 打开态背景基于 surface 派生:暗色主题提亮、亮色主题加深,
      // 避免默认黑色混合在暗色下 hover 变暗不可见、或亮色默认值突兀
      controlHoverColor: Color.alphaBlend(
        brightness == Brightness.dark
            ? Colors.white.withValues(alpha: 0.08)
            : Colors.black.withValues(alpha: 0.08),
        appPalette.surface,
      ),
      controlPressedColor: Color.alphaBlend(
        brightness == Brightness.dark
            ? Colors.white.withValues(alpha: 0.14)
            : Colors.black.withValues(alpha: 0.14),
        appPalette.surface,
      ),
    );
    return Container(
      height: kWindowCaptionHeight,
      color: appPalette.surface,
      child: Row(
        children: [
          // 左上角应用 Logo(矢量,按 DPI 光栅化)
          Padding(
            padding: const EdgeInsets.only(left: 8, right: 4),
            child: SvgPicture.asset(
              'assets/icons/app_logo.svg',
              width: 20,
              height: 20,
            ),
          ),
          // 标题栏空白区域可拖拽移动窗口 + 双击切换最大化。
          // 注意:不用 window_manager 的 DragToMoveArea——它自带 onDoubleTap
          // (双击最大化),会让菜单项的点击被 double-tap 判定窗口 hold 约
          // 300ms,表现为"首次点击展开菜单有明显延迟"。
          // 方案:Listener 自检双击(500ms 内两次 pointer down) + 自测拖拽
          // (移动 >6px 才 startDragging),单击立即穿透到 MenuStrip 零延迟。
          //
          // 关键:Listener 必须铺满整个标题栏区域(横向填满 Expanded、纵向
          // 32px)。若直接把 Listener 包在 MenuStrip 外,Listener 的 hit test
          // 区域会跟随 MenuStrip 固有尺寸(仅菜单项宽度 x 28px),导致"帮助
          // 菜单右侧到窗口按钮左侧"的大片空白无命中目标,双击/拖拽都无效。
          // 因此用 Stack:底层 MenuStrip 左对齐,上层 Positioned.fill 铺满
          // 透明 Listener(translucent 不拦截菜单的 hover/tap)。
          Expanded(
            child: Stack(
              children: [
                // 底层:菜单栏,仅占菜单项宽度、左对齐。
                Align(
                  alignment: Alignment.centerLeft,
                  child: MenuStrip(items: _buildMenuItems(context, app), tokens: dt),
                ),
                // 上层:透明命中层,铺满整个标题栏,负责双击/拖拽。
                Positioned.fill(
                  child: Listener(
                    behavior: HitTestBehavior.translucent,
                    onPointerDown: (event) {
                      final now = DateTime.now();
                      final last = _lastTitleBarDown;
                      final isDouble = last != null &&
                          now.difference(last).inMilliseconds < 500;
                      debugPrint('[titlebar] down pos=${event.position} '
                          'isDouble=$isDouble '
                          'gap=${last == null ? '-' : '${now.difference(last).inMilliseconds}ms'} '
                          'isMax=$_isMaximized');
                      _lastTitleBarDown = isDouble ? null : now;
                      _suppressDrag = isDouble;
                      _dragStart = event.position;
                      _dragging = false;
                      if (isDouble) {
                        // 走 native 端 toggleMaximize(复用最大化按钮的
                        // IsZoomed + PostMessage SC_MAXIMIZE/SC_RESTORE 路径)。
                        // 不用 windowManager.maximize():它内部 GetWindowPlacement
                        // 未初始化 length 会失败,导致 maximize 不生效。
                        debugPrint('[titlebar] -> toggleMaximize()');
                        _titlebarChannel
                            .invokeMethod('toggleMaximize')
                            .then((r) {
                          debugPrint(
                              '[titlebar] toggleMaximize returned: $r');
                        }).timeout(const Duration(seconds: 2), onTimeout: () {
                          debugPrint('[titlebar] toggleMaximize TIMEOUT '
                              '- native handler NOT registered');
                        });
                      }
                    },
                    onPointerMove: (event) {
                      final start = _dragStart;
                      if (_suppressDrag || _dragging || start == null) return;
                      // 移动超过 6px 才启动拖拽,避免双击时轻微抖动误触发。
                      if ((event.position - start).distance > 6.0) {
                        _dragging = true;
                        debugPrint('[titlebar] -> startDragging() '
                            'dist=${(event.position - start).distance}');
                        windowManager.startDragging();
                      }
                    },
                    onPointerUp: (_) {
                      _dragStart = null;
                      _dragging = false;
                    },
                    onPointerCancel: (_) {
                      _dragStart = null;
                      _dragging = false;
                    },
                    child: const SizedBox.expand(),
                  ),
                ),
              ],
            ),
          ),
          _themeToggle(context, app, dt),
          WindowCaptionButton.minimize(
            brightness: brightness,
            onPressed: windowManager.minimize,
          ),
          // 最大化按钮:hover 状态由 native 端通过 method channel 推送
          // (因为 HTMAXBUTTON 抢走了 MouseRegion 事件),在按钮上叠加一层
          // 半透明背景模拟 hover 效果。点击仍由 native 端转发 SC_MAXIMIZE
          // 到主窗口触发,不依赖此处的 onPressed。
          _MaximizeButton(
            isMaximized: _isMaximized,
            isHovered: _nativeHoveredButton == 2,
            brightness: brightness,
          ),
          WindowCaptionButton.close(
            brightness: brightness,
            onPressed: windowManager.close,
          ),
        ],
      ),
    );
  }

  /// 主题切换按钮:在 跟随系统 -> 明亮 -> 暗黑 之间循环
  Widget _themeToggle(BuildContext context, AppState app, DesktopTokens dt) {
    final t = Tokens.of(context);
    final l = context.l10n;
    final modeLabel = switch (app.themeMode) {
      ThemeMode.system => l.themeFollowSystem,
      ThemeMode.light => l.themeLight,
      ThemeMode.dark => l.themeDark,
    };
    final IconData icon = switch (app.themeMode) {
      ThemeMode.system => Icons.brightness_auto,
      ThemeMode.light => Icons.light_mode,
      ThemeMode.dark => Icons.dark_mode,
    };
    final String tooltip = l.themeSwitchTooltip(modeLabel);
    return IconBtn(
      icon: icon,
      iconSize: 15,
      color: t.mutedForeground,
      tooltip: tooltip,
      onTap: () => app.cycleThemeMode(),
    );
  }

  /// 组装菜单栏数据:已实现的功能接上回调,未实现的置灰禁用。
  List<MenuItem> _buildMenuItems(BuildContext context, AppState app) {
    final l = context.l10n;
    return [
      MenuItem(text: l.menuFile, children: [
        MenuItem(
            text: l.menuNewConnection,
            onPressed: () => _openConnectionWindow(context)),
        MenuItem(
            text: l.menuNewQuery,
            shortcut: 'Ctrl+N',
            onPressed: app.newQuery),
        const MenuSeparator(),
        MenuItem(
            text: l.menuImportConnections,
            onPressed: () => _importFromNavicat(context)),
        MenuItem(
            text: l.menuExportConnections,
            onPressed: () => _exportToNavicat(context)),
        // const MenuItem(text: '打开文件...', enabled: false),
        const MenuSeparator(),
        MenuItem(text: l.menuExit, onPressed: windowManager.close),
      ]),
      // TODO(菜单): 以下菜单待功能实现后再启用,暂时注释
      // const MenuItem(text: '编辑', children: [
      //   MenuItem(text: '撤销', enabled: false),
      //   MenuItem(text: '重做', enabled: false),
      //   MenuSeparator(),
      //   MenuItem(text: '剪切', enabled: false),
      //   MenuItem(text: '复制', enabled: false),
      //   MenuItem(text: '粘贴', enabled: false),
      //   MenuItem(text: '删除', enabled: false),
      // ]),
      MenuItem(text: l.menuView, children: [
        MenuItem(text: l.menuRefresh, onPressed: () => app.reloadConnections()),
        const MenuSeparator(),
        MenuItem(
            text: l.menuThemeCustomize,
            onPressed: () => _openThemeCustomize(context)),
        const MenuSeparator(),
        // 大型图标 = 多列网格布局,列表 = 单列列表布局(objectGridLayout)
        MenuItem(
            text: l.menuLargeIcons, onPressed: () => app.setObjectLayout(true)),
        MenuItem(text: l.menuSmallIcons, enabled: false),
        MenuItem(text: l.menuList, onPressed: () => app.setObjectLayout(false)),
        MenuItem(text: l.menuDetails, enabled: false),
      ]),
      // const MenuItem(text: '收藏夹', children: [
      //   MenuItem(text: '添加到收藏夹', enabled: false),
      //   MenuItem(text: '整理收藏夹...', enabled: false),
      // ]),
      MenuItem(text: l.menuTools, children: [
        MenuItem(
            text: l.menuCommandLine,
            // 命令列会话必须绑定一个库:未选中数据库时不可用
            enabled: app.objectConnection != null && app.objectDatabase != null,
            onPressed: () => app.openCommandLine(
              connection: app.objectConnection!,
              database: app.objectDatabase!,
            )),
        MenuItem(text: l.menuDataTransfer, enabled: false),
        MenuItem(text: l.menuDataSync, enabled: false),
        MenuItem(
            text: l.menuSchemaSync, onPressed: () => _openSchemaSync(context)),
        const MenuSeparator(),
        MenuItem(text: l.menuBackup, enabled: false),
        MenuItem(text: l.menuRestoreBackup, enabled: false),
        MenuSeparator(),
        // MCP 设置改动即时落盘生效,所以只有关闭按钮,不需要「选项...」那种确定/取消。
        MenuItem(
            text: l.menuMcpService, onPressed: () => _openMcpSettings(context)),
        MenuItem(
            text: l.menuOptions, onPressed: () => _openOptions(context)),
      ]),
      // const MenuItem(text: '窗口', children: [
      //   MenuItem(text: '新建窗口', enabled: false),
      //   MenuItem(text: '关闭窗口', enabled: false),
      //   MenuSeparator(),
      //   MenuItem(text: '下一个窗口', enabled: false),
      //   MenuItem(text: '上一个窗口', enabled: false),
      // ]),
      MenuItem(text: l.menuHelp, children: [
        MenuItem(
            text: l.menuIssueTracker,
            onPressed: () => _openIssueTracker(context)),
        const MenuSeparator(),
        MenuItem(text: l.menuAbout, onPressed: () => _showAbout(context)),
      ]),
    ];
  }

  /// 点击「结构同步」:弹出比对 / 部署大弹窗,读取当前连接树的连接与元数据缓存。
  Future<void> _openSchemaSync(BuildContext context) async {
    await showSchemaSyncDialog(context, app: context.read<AppState>());
  }

  /// 点击"新建连接":以模态弹窗(base-ui `DialogBox`)弹出"选择一个连接类型"向导。
  /// 与 Ribbon 的"连接"按钮共用同一向导,完成后把新连接加入连接树。
  Future<void> _openConnectionWindow(BuildContext context) async {
    final result = await showDialog<ConnectionInfo>(
      context: context,
      builder: (_) => const ConnectionDialogPage(),
    );
    if (result != null) {
      context.read<AppState>().addConnection(result);
    }
  }

  /// 点击「导入连接」:解析 .ncx、解密保存的密码、勾选后写入连接树
  /// 都在向导内完成,这里只把结果(含需要补填密码的条数)汇总告知。
  Future<void> _importFromNavicat(BuildContext context) async {
    final result = await showNavicatImportDialog(
        context, app: context.read<AppState>());
    if (result == null || !context.mounted) return;
    final l = context.l10n;
    final manual = result.needsManualPassword;
    final groups = result.newGroups;
    final imported = l.importDoneMessage('${result.imported.length}');
    final extra = manual == 0 ? '' : '\n${l.importDoneManualPassword('$manual')}';
    final groupNote = groups.isEmpty ? '' : '\n${_groupNamesNote(l, groups)}';
    MessageBox.show(
      context,
      title: l.importDoneTitle,
      message: '$imported$extra$groupNote',
      buttons: MessageBoxButtons.ok,
      tokens: Tokens.read(context).desktopTokensFor(context),
    );
  }

  /// 新建分组提示:名称最多列 5 个,超出追加省略号
  String _groupNamesNote(AppLocalizations l, List<String> groups) {
    final shown = groups.take(5).join(l.listSeparator);
    final names = groups.length > 5 ? '$shown${l.listEllipsis}' : shown;
    return l.importDoneNewGroups('${groups.length}', names);
  }

  /// 点击「导出连接」:勾选连接、选目标文件、写出 Navicat 可直接导入的
  /// .ncx 都在向导内完成,这里只汇总结果与被跳过的连接。
  Future<void> _exportToNavicat(BuildContext context) async {
    final result = await showNavicatExportDialog(
        context, app: context.read<AppState>());
    if (result == null || !context.mounted) return;
    final l = context.l10n;
    var extra = '';
    if (result.skipped.isNotEmpty) {
      final names = result.skipped
          .take(3)
          .map((s) => '${s.$1}(${s.$2})')
          .join(l.listSeparator);
      final more = result.skipped.length > 3
          ? l.listEllipsisMore('${result.skipped.length}')
          : '';
      extra = '\n${l.exportDoneSkipped('$names$more')}';
    }
    final done = l.exportDoneMessage('${result.exported}', result.path.path);
    MessageBox.show(
      context,
      title: l.exportDoneTitle,
      message: '$done$extra',
      buttons: MessageBoxButtons.ok,
      tokens: Tokens.read(context).desktopTokensFor(context),
    );
  }

  /// 点击"视图 → 主题定制...":以模态弹窗(base-ui `DialogBox`)弹出主题定制窗口,
  /// 确认后由 [AppState.setCustomPalette] 整窗重建并落盘。
  Future<void> _openThemeCustomize(BuildContext context) async {
    await showDialog<void>(
      context: context,
      builder: (_) => const ThemeCustomizeDialog(),
    );
  }

  /// 点击"工具 → MCP 服务...":弹出 MCP 设置对话框(策略、连接授权、客户端配置)。
  Future<void> _openMcpSettings(BuildContext context) async {
    await showDialog<void>(
      context: context,
      builder: (_) => const McpSettingsDialog(),
    );
  }

  /// 点击"工具 → 选项…":弹出选项对话框(左侧分类 + 右侧设置页,当前是「常规」)。
  Future<void> _openOptions(BuildContext context) async {
    await showOptionsDialog(context);
  }

  /// 点击"问题反馈":用系统默认浏览器打开 daro 的 GitHub Issues 页面。
  /// 启动失败时(无默认浏览器 / http 协议未注册)退化为弹窗展示链接,
  /// 让用户至少能手动复制,而不是点击后毫无反馈。
  Future<void> _openIssueTracker(BuildContext context) async {
    const url = 'https://github.com/SpringHgui/daro/issues';
    var opened = false;
    try {
      opened = await launchUrl(Uri.parse(url),
          mode: LaunchMode.externalApplication);
    } catch (e) {
      debugPrint('[top_menu] launchUrl failed: $e');
      opened = false;
    }
    if (opened || !context.mounted) return;
    final l = context.l10n;
    MessageBox.show(
      context,
      title: l.issueTrackerTitle,
      message: l.issueTrackerOpenFailed(url),
      buttons: MessageBoxButtons.ok,
      tokens: Tokens.read(context).desktopTokensFor(context),
    );
  }

  /// "关于"对话框:启动画面式弹窗(见 about_dialog.dart)
  void _showAbout(BuildContext context) => showDaroAboutDialog(context);
}

/// 最大化按钮:复用 window_manager 的 WindowCaptionButton 渲染图标,
/// 在其上叠加 hover 背景。hover 状态由 native 端通过 method channel 推送,
/// 因为鼠标进入按钮区域后 HTMAXBUTTON 接管事件,Flutter 端 MouseRegion 失效。
class _MaximizeButton extends StatelessWidget {
  const _MaximizeButton({
    required this.isMaximized,
    required this.isHovered,
    required this.brightness,
  });

  final bool isMaximized;
  final bool isHovered;
  final Brightness brightness;

  @override
  Widget build(BuildContext context) {
    // 与 window_manager WindowCaptionButton 内部颜色方案对齐
    // (见 window_caption_button.dart:342-363)。
    final hoverBg = brightness == Brightness.dark
        ? Colors.white.withValues(alpha: 0.0605)
        : Colors.black.withValues(alpha: 0.0373);
    return Stack(
      children: [
        // 底层按钮:onPressed 在 hover 时不会被调用(NC 接管点击),但仍保留
        // 作为非 hover 状态下的兜底点击入口(理论上不会触发,因 hover 先发生)。
        isMaximized
            ? WindowCaptionButton.unmaximize(
                brightness: brightness,
                onPressed: windowManager.unmaximize,
              )
            : WindowCaptionButton.maximize(
                brightness: brightness,
                onPressed: windowManager.maximize,
              ),
        // hover 时叠加半透明背景(在 WindowCaptionButton 之上,但透过
        // IgnorePointer 让点击事件穿透到底层按钮 / NC 区域)。
        if (isHovered)
          Positioned.fill(
            child: IgnorePointer(
              child: Container(
                constraints: const BoxConstraints(minWidth: 46, minHeight: 32),
                color: hoverBg,
              ),
            ),
          ),
      ],
    );
  }
}
