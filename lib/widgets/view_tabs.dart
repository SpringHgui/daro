import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../app/app_state.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';
import 'object_category_icon.dart';

// 中部面板顶部的视图标签栏:默认展示"对象"页,表数据页/查询页追加为标签。
// 复用 base-ui 的 TabControl:自适应宽度、closable、滚动箭头、
// 右键菜单、键盘方向键切换均由组件内置,本文件只负责装配标签数据。

/// 文档标签条高度:比对话框内的标签(约 21)略高,好容纳 16px 图标。
const double _kTabBarHeight = 26;

class ViewTabs extends StatelessWidget {
  const ViewTabs({super.key});

  @override
  Widget build(BuildContext context) {
    // 标签列表与活动标签变化时重建(选中表由子项独立通知,不级联到这里)
    final tabCount = context.select<AppState, int>((a) => a.tabs.length);
    final activeTab = context.select<AppState, String>((a) => a.activeTab);
    final app = context.read<AppState>();
    final t = Tokens.of(context);
    final l = context.l10n;

    // 活动标签在完整标签列表(含固定"对象"页)中的位置;未找到时回到 0
    final activeIndex = activeTab == AppState.objectsTabKey
        ? 0
        : app.tabs.indexWhere((tab) => tab.title == activeTab) + 1;

    // 文档标签条:条高固定 26(比对话框标签略高,好容纳 16px 图标),
    // 标签宽度交由 TabControl 按标题自适应 —— 不再平分撑满整条,
    // 标签多到超出可视宽度时由组件内部弹出滚动箭头。
    return Container(
      height: _kTabBarHeight,
      color: t.background,
      child: TabControl(
        initialIndex: activeIndex.clamp(0, tabCount),
        // 延迟到下一帧再同步,避免鼠标事件处理期间触发 setState
        onChanged: (index) => WidgetsBinding.instance
            .addPostFrameCallback((_) => _activate(context, index)),
        // 选中标签用内容底色 background(纯白),条底用 surface(面板灰),
        // 让选中的白标签在灰条上明显"浮起";未选中标签回落到条底灰,
        // 对比清晰。旧写法选中=surface、条底=background,在 surface==control
        // 的纸白 / 暗色主题下对比几乎消失(仅差约 5 级),看起来像没切换。
        tabBarColor: t.surface,
        selectedTabColor: t.background,
        hoverTabColor: t.secondary,
        barHeight: _kTabBarHeight,
        // 纯标签条场景:不需要内容区
        contentPadding: EdgeInsets.zero,
        tabs: [
          // 固定的"对象"标签:不可关闭,无右键菜单。显示名走词条,
          // 身份另用 AppState.objectsTabKey 哨兵,切换语言不影响活动标签
          TabItem(label: l.tabObjects),
          for (final tab in app.tabs)
            TabItem(
              label: _tabLabel(context, tab),
              // 与连接树分组同一套图标,保证标签图标与分组一致:
              // 查询 → 查询图;表数据 / 新建表 → 表图;设计页按对象分类取图;
              // 命令列界面 → 自绘终端图(它不是对象分类,不走 ObjectCategoryIcon)
              icon: tab.type == TabType.commandLine
                  ? const UiIcon(kConsoleIcon, size: 16)
                  : ObjectCategoryIcon(
                      category: _tabCategory(tab),
                      size: 16,
                    ),
              onClose: () => context.read<AppState>().closeTab(tab.title),
              contextMenuItems: _tabMenuItems(context, tab.title),
            ),
        ],
      ),
    );
  }

  /// 标签显示名:标题里的「 (设计) / (新建)」是与语言无关的身份令牌
  /// (见 locale_config 的 splitTabTitle),显示时换成本地词条,
  /// 于是切换语言后已打开的标签显示也跟着变,而身份与迁移逻辑不受影响。
  String _tabLabel(BuildContext context, OpenTab tab) {
    final l = context.l10n;
    // 命令列标签的标题整体是内部身份(连接名|库名 + 令牌后缀),按类型另拼显示名
    if (tab.type == TabType.commandLine) {
      return '${tab.connection} - ${l.tabCommandLine}';
    }
    final split = splitTabTitle(tab.title);
    if (split.name == tab.title) return tab.title; // 无令牌后缀,原样显示
    return split.isNew
        ? '${split.name}${l.tabNewSuffix}'
        : '${split.name}${l.tabDesignSuffix}';
  }

  /// 标签图标分类:与连接树分组图标同源。
  /// 查询 → 查询;表数据页 / 新建表 → 表;设计页按
  /// [OpenTab.routineCategory] 区分视图 / 函数等(routineCategory 为空即表设计)。
  ObjectCategory _tabCategory(OpenTab tab) => switch (tab.type) {
        TabType.query => ObjectCategory.query,
        TabType.table || TabType.createTable => ObjectCategory.table,
        TabType.design => tab.routineCategory ?? ObjectCategory.table,
        _ => ObjectCategory.table,
      };

  /// 点击标签:0 = "对象"页,其余映射到打开标签列表
  void _activate(BuildContext context, int index) {
    final app = context.read<AppState>();
    if (index == 0) {
      app.activateTab(AppState.objectsTabKey);
    } else if (index - 1 < app.tabs.length) {
      app.activateTab(app.tabs[index - 1].title);
    }
  }

  /// 标签右键菜单:关闭 / 关闭其他 / 关闭右侧 / 全部关闭(仿 DBeaver)
  List<MenuModel> _tabMenuItems(BuildContext context, String title) {
    final app = context.read<AppState>();
    final l = context.l10n;
    final index = app.tabs.indexWhere((tab) => tab.title == title);
    return [
      MenuItem(
        text: l.tabCtxClose,
        shortcut: 'Ctrl+W',
        onPressed: () => app.closeTab(title),
      ),
      MenuItem(
        text: l.tabCtxCloseOthers,
        enabled: app.tabs.length > 1,
        onPressed: () => app.closeOtherTabs(title),
      ),
      MenuItem(
        text: l.tabCtxCloseRight,
        enabled: index >= 0 && index < app.tabs.length - 1,
        onPressed: () => app.closeTabsToRight(title),
      ),
      const MenuSeparator(),
      MenuItem(
        text: l.tabCtxCloseAll,
        shortcut: 'Ctrl+Shift+W',
        onPressed: () => app.closeAllTabs(),
      ),
    ];
  }
}
