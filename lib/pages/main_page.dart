import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:base_ui_flutter/base_ui_flutter.dart' show Splitter;
import '../app/app_state.dart';
import '../data/routine_sql.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';
import '../widgets/top_menu.dart';
import '../widgets/ribbon.dart';
import '../widgets/database_tree.dart';
import '../widgets/object_panel.dart';
import '../widgets/database_info.dart';
import '../widgets/status_bar.dart';
import '../widgets/view_tabs.dart';
import '../widgets/table_data_page.dart';
import '../widgets/query_page.dart';
import '../widgets/command_line_page.dart';
import '../widgets/kv_browser_page.dart';
import '../widgets/table_designer_page.dart';
import '../widgets/routine_design_page.dart';
import '../widgets/user_design_page.dart';
import '../widgets/view_design_page.dart';
import '../widgets/matview_design_page.dart';

class MainPage extends StatefulWidget {
  const MainPage({super.key});

  @override
  State<MainPage> createState() => _MainPageState();
}

class _MainPageState extends State<MainPage> {
  /// 拖动分隔条中:此时关闭面板宽度的隐式动画,让宽度严格跟随鼠标
  bool _resizing = false;

  /// 应用外壳的焦点作用域:三个区域与标签页里的控件都挂在它下面。
  /// 用它区分「焦点在页面内」和「焦点在弹出的对话框里」(见 [_onAppKey])。
  final FocusScopeNode _chromeScope = FocusScopeNode(debugLabel: 'MainPage');

  @override
  void initState() {
    super.initState();
    // 应用级快捷键挂在焦点树的**根节点**上,而不是在外面包一层 Focus。
    // 根节点是按键链的最后一站:焦点落在页面任何角落都能冒泡到它;
    // 「刚启动、还没点过任何东西」时 primaryFocus 就是根节点本身,照样收得到。
    // 包一层的 Focus 则两头不讨好 —— canRequestFocus: false 时它在链上之外
    // (不是任何焦点节点的祖先,空窗时收不到键);autofocus: true 时它又会
    // 抢在 SQL 编辑器的 autofocus 之前拿到焦点,新标签一打开光标就不在编辑器里。
    FocusManager.instance.rootScope.onKeyEvent = _onAppKey;
  }

  @override
  void dispose() {
    // 只撤自己挂上去的那个(热重载后可能已被新 State 覆盖)
    if (FocusManager.instance.rootScope.onKeyEvent == _onAppKey) {
      FocusManager.instance.rootScope.onKeyEvent = null;
    }
    _chromeScope.dispose();
    super.dispose();
  }

  /// 应用级快捷键:Ctrl+N 新建查询 / Ctrl+W 关闭当前标签 /
  /// Ctrl+Tab、Ctrl+Shift+Tab 在标签条上前后循环。
  ///
  /// 按键先由获得焦点的下层处理(SQL 编辑器自绑 Ctrl+S/F/H、对象面板自绑
  /// Ctrl+C/V),冒泡到这里时只剩没人消费的键,所以不必做冲突让位。
  KeyEventResult _onAppKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final kb = HardwareKeyboard.instance;
    if (!kb.isControlPressed) return KeyEventResult.ignored;
    // 焦点在对话框 / 浮层里时不抢键:它们同样会冒泡到焦点树根节点,但既不在
    // 本页作用域之内、也不是本页的祖先(弹窗里的 Ctrl+W 是想删一个字符,不是关标签)。
    final primary = FocusManager.instance.primaryFocus;
    if (primary != null &&
        !_chromeScope.hasFocus &&
        !_chromeScope.ancestors.contains(primary)) {
      return KeyEventResult.ignored;
    }
    final app = context.read<AppState>();
    if (event.logicalKey == LogicalKeyboardKey.keyN) {
      app.newQuery();
    } else if (event.logicalKey == LogicalKeyboardKey.keyW) {
      // 「对象」是固定的首页标签,不可关闭
      if (app.activeTab == AppState.objectsTabKey) {
        return KeyEventResult.ignored;
      }
      app.closeTab(app.activeTab);
    } else if (event.logicalKey == LogicalKeyboardKey.tab) {
      app.activateAdjacentTab(backward: kb.isShiftPressed);
    } else {
      return KeyEventResult.ignored;
    }
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    final app = context.read<AppState>();
    final leftVisible = context.select((AppState a) => a.leftPanelVisible);
    final rightVisible = context.select((AppState a) => a.rightPanelVisible);
    // 侧栏与中间面板拼缝上的发丝边框色(左栏画右边框、右栏画左边框)
    final borderColor = Tokens.of(context).border;

    // 整页包一层焦点作用域:让 [_onAppKey] 能判断焦点是在页面内还是在弹窗里
    return FocusScope(
      node: _chromeScope,
      child: ColoredBox(
        color: Tokens.of(context).background,
        child: Column(
          children: [
            const TopMenu(),
            const Ribbon(),
            Expanded(
              // 三栏贴合:侧栏与中间面板之间不留布局间隙,拖动分隔条改为浮层
              // 覆盖在拼缝上(见 _PaneDivider),中间面板两侧因此不再出现分隔线
              child: Stack(
                fit: StackFit.expand,
                children: [
                  Row(
                    children: [
                      ValueListenableBuilder<double>(
                        valueListenable: app.leftPanelWidth,
                        builder: (context, width, child) => _AnimatedPanel(
                          visible: leftVisible,
                          width: width,
                          animate: !_resizing,
                          border: Border(right: BorderSide(color: borderColor)),
                          child: child!,
                        ),
                        // child 由 ValueListenableBuilder 缓存,拖动改宽时不重建子树
                        child: const DatabaseTree(),
                      ),
                      Expanded(
                        child: Column(
                          children: [
                            const ViewTabs(),
                            Expanded(
                              // 根据活动标签切换页面:对象浏览 / 表数据 / 查询编辑
                              // 用 Selector 仅在当前标签变化时重建,选中表时由子项 context.select 独立更新
                              child: Selector<AppState, OpenTab?>(
                                selector: (_, app) => app.activeTabModel,
                                child: const ObjectPanel(),
                                builder: (context, tab, cachedChild) {
                                  if (tab == null) return cachedChild!;
                                  if (tab.type == TabType.table)
                                    return TableDataPage(
                                      table: tab.title,
                                      connection: tab.connection!,
                                      database: tab.database!,
                                      schema: tab.schema,
                                    );
                                  if (tab.type == TabType.design) {
                                    // 标题里的「(设计) / (新建)」是与语言无关的内部身份令牌,
                                    // 这里剥掉它还原真实对象名(见 splitTabTitle)
                                    final split = splitTabTitle(tab.title);
                                    final name = split.name;
                                    final isNew = split.isNew;
                                    final category = tab.routineCategory;
                                    if (category != null) {
                                      // 用户 / 角色设计页(七标签:常规 / 高级 /
                                      // 成员属于 / 成员 / 服务器权限 / 权限 / SQL 预览)
                                      if (category == ObjectCategory.user) {
                                        return UserDesignPage(
                                          name: name,
                                          connection: tab.connection!,
                                          database: tab.database!,
                                          schema: tab.schema,
                                          isNew: isNew,
                                        );
                                      }
                                      // 例程(过程 / 函数 / 视图)设计页
                                      if (category ==
                                          ObjectCategory.materializedView) {
                                        // 实体化视图设计页(定义 / 索引 / 高级 / 注释 / SQL 预览)。
                                        // 与视图不同:保存是 DROP + CREATE 重建,故单独成页
                                        return MatViewDesignPage(
                                          name: name,
                                          connection: tab.connection!,
                                          database: tab.database!,
                                          schema: tab.schema,
                                          isNew: isNew,
                                          comment: tab.routineComment ?? '',
                                        );
                                      }
                                      if (category == ObjectCategory.view) {
                                        // 视图设计页(Navicat 风格:定义 / 规则 / 高级 / 注释 / SQL 预览)
                                        return ViewDesignPage(
                                          name: name,
                                          connection: tab.connection!,
                                          database: tab.database!,
                                          category: category,
                                          schema: tab.schema,
                                          isNew: isNew,
                                          comment: tab.routineComment ?? '',
                                        );
                                      }
                                      return RoutineDesignPage(
                                        name: name,
                                        connection: tab.connection!,
                                        database: tab.database!,
                                        category: category,
                                        schema: tab.schema,
                                        isNew: isNew,
                                        params: tab.routineParams ?? '',
                                        options:
                                            tab.routineOptions ??
                                            const RoutineOptions(),
                                        returnType: tab.routineReturnType ?? '',
                                        comment: tab.routineComment ?? '',
                                      );
                                    }
                                    // 「设计表」与「新建表」共用同一设计器:
                                    // existingTable 非空 = 编辑模式(反查结构 + 保存 ALTER)
                                    return TableDesignerPage(
                                      title: tab.title,
                                      tabKey: tab.key,
                                      connection: tab.connection!,
                                      database: tab.database!,
                                      schema: tab.schema,
                                      existingTable: isNew ? null : name,
                                    );
                                  }
                                  if (tab.type == TabType.query)
                                    return QueryPage(
                                      title: tab.title,
                                      connection: tab.connection,
                                      database: tab.database,
                                      schema: tab.schema,
                                    );
                                  if (tab.type == TabType.commandLine)
                                    return CommandLinePage(
                                      connection: tab.connection!,
                                      database: tab.database!,
                                    );
                                  if (tab.type == TabType.kvKeys)
                                    return KvBrowserPage(
                                      // key 用标签身份:db0 与 db15 两个标签同为
                                      // KvBrowserPage,不给 key 会复用同一个 State,
                                      // 切过去仍显示上一个库的键列表
                                      key: ValueKey(tab.key),
                                      connection: tab.connection!,
                                      namespaceId: tab.database!,
                                      namespaceLabel: tab.title,
                                    );
                                  if (tab.type == TabType.createTable)
                                    return TableDesignerPage(
                                      title: tab.title,
                                      tabKey: tab.key,
                                      connection: tab.connection!,
                                      database: tab.database!,
                                      schema: tab.schema,
                                    );
                                  return cachedChild!;
                                },
                              ),
                            ),
                          ],
                        ),
                      ),
                      ValueListenableBuilder<double>(
                        valueListenable: app.rightPanelWidth,
                        builder: (context, width, child) => _AnimatedPanel(
                          visible: rightVisible,
                          width: width,
                          animate: !_resizing,
                          alignment: Alignment.centerRight,
                          border: Border(left: BorderSide(color: borderColor)),
                          child: child!,
                        ),
                        child: const DatabaseInfo(),
                      ),
                    ],
                  ),
                  if (leftVisible)
                    _PaneDivider(
                      anchor: app.leftPanelWidth,
                      onDragStart: () => setState(() => _resizing = true),
                      onDrag: app.resizeLeftPanel,
                      onDragEnd: () => setState(() => _resizing = false),
                    ),
                  if (rightVisible)
                    _PaneDivider(
                      leading: false,
                      anchor: app.rightPanelWidth,
                      onDragStart: () => setState(() => _resizing = true),
                      onDrag: app.resizeRightPanel,
                      onDragEnd: () => setState(() => _resizing = false),
                    ),
                ],
              ),
            ),
            const StatusBar(),
          ],
        ),
      ),
    );
  }
}

/// 侧栏容器:展开时显示当前宽度(可拖动分隔条调整),收起时宽度动画到 0,
/// 内部内容保持原宽并裁剪
class _AnimatedPanel extends StatelessWidget {
  final bool visible;
  final double width;

  /// false = 宽度变化立即生效(拖动分隔条时避免隐式动画拖慢跟随感)
  final bool animate;
  final Alignment alignment;

  /// 拼缝发丝边框(左栏传 right、右栏传 left)。仅在面板展开时绘制,
  /// 收起(宽度 0)时不画,避免贴窗口边缘出现孤立竖线。
  final BoxBorder? border;
  final Widget child;

  const _AnimatedPanel({
    required this.visible,
    required this.width,
    this.animate = true,
    this.alignment = Alignment.centerLeft,
    this.border,
    required this.child,
  });

  @override
  Widget build(BuildContext context) {
    return AnimatedContainer(
      duration:
          animate ? const Duration(milliseconds: 200) : Duration.zero,
      curve: Curves.easeInOut,
      width: visible ? width : 0,
      // decoration 仅为 clipBehavior 提供裁剪路径(透明占位)
      decoration: const BoxDecoration(),
      // 边框用 foregroundDecoration 画在内容之上:侧栏内容填满宽度,
      // 若走 decoration 会被自身背景盖住而看不见。收起(宽度 0)时不画。
      foregroundDecoration:
          visible && border != null ? BoxDecoration(border: border) : null,
      clipBehavior: Clip.hardEdge,
      child: OverflowBox(
        alignment: alignment,
        minWidth: width,
        maxWidth: width,
        child: SizedBox(width: width, child: child),
      ),
    );
  }
}

/// 侧栏与中间面板拼缝上的拖动条。
///
/// 不占布局宽度(浮层覆盖),因此三栏彼此贴合;[Splitter.showHairline] 关掉
/// → 静止态拼缝上不画线(中间面板两侧没有像边框的线),
/// [Splitter.showHoverHighlight] 也关掉 → 悬浮 / 拖动时同样不高亮。
/// 手柄只剩 `resizeLeftRight` 光标提示与 5px 命中区,照样能按住改宽。
class _PaneDivider extends StatelessWidget {
  const _PaneDivider({
    required this.anchor,
    this.leading = true,
    required this.onDragStart,
    required this.onDrag,
    required this.onDragEnd,
  });

  /// 对应侧栏的当前宽度:分隔条中心压在「侧栏 / 中间面板」拼缝上
  final ValueListenable<double> anchor;

  /// true = 左栏与中间面板之间;false = 中间面板与右栏之间
  final bool leading;

  final VoidCallback onDragStart;
  final ValueChanged<double> onDrag;
  final VoidCallback onDragEnd;

  /// 命中条宽度,与 base-ui [Splitter] 默认 thickness 一致
  static const double _hit = 5;

  @override
  Widget build(BuildContext context) {
    return ValueListenableBuilder<double>(
      valueListenable: anchor,
      builder: (context, width, child) {
        // 拼缝两侧各让出半个命中条;侧栏收起(宽度 0)时贴边,不越界
        final inset = (width - _hit / 2).clamp(0.0, double.infinity);
        return Positioned(
          left: leading ? inset : null,
          right: leading ? null : inset,
          top: 0,
          bottom: 0,
          width: _hit,
          child: child!,
        );
      },
      child: Splitter(
        showHairline: false,
        showHoverHighlight: false,
        thickness: _hit,
        onDragStart: onDragStart,
        onDrag: onDrag,
        onDragEnd: onDragEnd,
      ),
    );
  }
}
