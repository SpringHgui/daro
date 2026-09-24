import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../app/app_state.dart';
import '../data/db_data.dart';
import '../data/drivers/db_driver.dart';
import '../l10n/locale_config.dart';
import '../pages/connection_dialog_page.dart';
import '../theme/app_theme.dart';
import 'object_category_icon.dart';

// 顶部工具栏:每个按钮使用独立的图标,可点击并有悬停提示。
// 使用 base_ui_flutter 的 Button(ghost 无边框变体)实现,相当于一个无边框按钮组,
// 按钮内容为「图标 + 文字」的 widget。
//
// 分割线右侧的分类按钮(表 / 视图 / 函数 / 角色 / 查询)根据当前选中连接的
// 数据库类型动态显隐:例如 SQLite / Access 不显示「函数」「角色」按钮。
// 未选中任何连接时仅显示所有类型都支持的基础分类(表 / 视图 / 查询)。
class Ribbon extends StatelessWidget {
  const Ribbon({super.key});

  /// 分类按钮定义:ObjectCategory 枚举。
  /// 文字取 category.labelOf、能力匹配取 category.name、图标查
  /// [ObjectCategoryIcon.assetOf](均与连接树分组同源,避免同一分类在不同入口显示不同名)。
  static const _categoryButtons = <({ObjectCategory category})>[
    (category: ObjectCategory.table),
    (category: ObjectCategory.view),
    (category: ObjectCategory.materializedView),
    (category: ObjectCategory.function),
    (category: ObjectCategory.procedure),
    (category: ObjectCategory.user),
    (category: ObjectCategory.query),
  ];

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final l = context.l10n;
    final app = context.watch<AppState>();
    final activeCategory = app.objectCategory;

    // 当前选中连接的数据库类型 id;无选中连接时为 null
    final connInfo = app.connectionByName(app.objectConnection);
    final typeId = connInfo?.typeId;

    // 按当前数据库类型过滤分类按钮:
    // - 有选中连接时:仅显示该类型支持的分类
    // - 无选中连接时:仅显示所有类型都支持的基础分类(表 / 视图 / 查询)
    final visibleButtons = _categoryButtons.where((b) {
      if (typeId != null) return isCategorySupportedForType(typeId, b.category.name);
      // 无连接选中:只保留全类型通用的分类
      return b.category == ObjectCategory.table ||
          b.category == ObjectCategory.view ||
          b.category == ObjectCategory.query;
    }).toList();

    return Container(
      height: 56,
      color: t.control,
      padding: const EdgeInsets.symmetric(horizontal: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          // 按钮组:窗口过窄时横向滚动,保证所有按钮可达
          Expanded(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.center,
                children: [
                  // 「连接」「新建查询」:自绘 SVG 图标 + 右下角绿色「+」徽章
                  _button(context,
                      _badgedIcon('assets/icons/ui/connection.svg'),
                      l.ribbonConnection,
                      onTap: () => _openConnectionWindow(context)),
                  _button(context, _badgedIcon('assets/icons/ui/query.svg'),
                      l.ribbonNewQuery, onTap: () => app.newQuery()),
                  // 新建查询右侧分割线
                  SizedBox(
                    height: 40,
                    child: Separator(
                      orientation: Axis.vertical,
                      thickness: 1,
                      color: t.border,
                    ),
                  ),
                  // 分类按钮:按当前数据库类型动态显隐,
                  // active 状态与左侧连接树分组节点选中联动;
                  // 图标与连接树 / 对象面板同源(ObjectCategoryIcon),
                  // 文字取 category.labelOf(同样同源,避免同名不同称)
                  for (final b in visibleButtons)
                    _button(
                        context,
                        ObjectCategoryIcon(category: b.category, size: 26),
                        b.category.labelOf(context.l10n),
                        active: activeCategory == b.category,
                        onTap: () => app.showObjectCategory(b.category)),
                ],
              ),
            ),
          ),
          const SizedBox(width: 8),
        ],
      ),
    );
  }

  // 单个工具栏按钮:无边框(ghost)变体,内容为图标 + 文字。
  // 未传入 onTap 的按钮也保持可点击(启用),避免被禁用置灰。
  // active 为 true 时显示统一的浅蓝背景(主题 accent 混入 control,明暗自适应),
  // 与左侧连接树分组节点选中联动;图标与文字不随选中态换色。
  Widget _button(BuildContext context, Widget icon, String text,
      {VoidCallback? onTap, bool active = false}) {
    // ribbon 按钮为「图标 + 文字」竖排,横向留白由按钮 padding 提供;
    // 默认 controlPaddingX=12 会让整体偏宽,这里压到 4,
    // 使「实体化视图」等长标签在 66px 内容宽内单行放下、且外框比默认更窄。
    final t = Tokens.of(context);
    final dt = TokenScope.maybeOf(context) ?? DesktopTokens.winForm;
    // padding / decoration 必须**恒定非空**:Container 在 active 与否时要产出
    // 同一棵子树形状(DecoratedBox > Padding > Button)。否则 active 从 false→true
    // 时 Container.build 的返回值会从「直接就是 Button」变成「DecoratedBox 套
    // Padding 再套 Button」,子槽位的运行时类型一变,Flutter 只能销毁旧的 Button
    // element、重建一份新 State 与新 FocusNode;而「按下即 requestFocus」刚把焦点
    // 交给这个旧节点,旧节点一销毁焦点就回退到上一个聚焦过的按钮 —— 症状即
    // 「点了函数,却还是新建查询带着焦点圈」。恒定包裹后,按钮的 State / FocusNode
    // 跨 active 切换存活,焦点稳稳留在被点的那一个。视觉不变(未选中时零内边距、
    // 透明底色与原来直接返回 Button 等价)。
    return Container(
      padding: active ? const EdgeInsets.only(bottom: 2) : EdgeInsets.zero,
      decoration: BoxDecoration(
        color: active
            ? Color.alphaBlend(t.accent.withValues(alpha: 0.16), t.control)
            : Colors.transparent,
      ),
      child: Button(
        text: text,
        variant: ButtonVariant.ghost,
        tokens: dt.copyWith(controlPaddingX: 4),
        onPressed: onTap ?? () {},
        child: ConstrainedBox(
          // 最小 66 保住原设计(中文标签下按钮等宽),最大 120 让英文长标签
          // (Materialized View)有自己的宽度而不是溢出压到相邻按钮上。
          constraints: const BoxConstraints(minWidth: 66, maxWidth: 120),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              icon,
              const SizedBox(height: 1),
              Text(
                text,
                softWrap: false,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontSize: 12,
                  height: 1.2,
                  // 选中态不加粗、不换色,仅靠背景高亮区分
                  color: bodyTextColor(context),
                  fontWeight: FontWeight.w500,
                  // 与全局字体机制一致:Button 内部 DefaultTextStyle 用的是 Segoe UI
                  // 且无 fontVariations,中日文会退化成最细的 regular;
                  // 显式补上按语言的字体回退 + 可变字重轴,保证 ribbon 文字与其它区域同粗细
                  fontFamilyFallback: fontFamilyFallbackOf(context),
                  fontVariations: const [
                    FontVariation.weight(400),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// 点击"连接":以模态弹窗(base-ui `DialogBox`)弹出"选择一个连接类型"向导。
  /// 完成连接向导后,把新建的连接加入连接树。
  Future<void> _openConnectionWindow(BuildContext context) async {
    final result = await showDialog<ConnectionInfo>(
      context: context,
      builder: (_) => const ConnectionDialogPage(),
    );
    if (result != null) {
      context.read<AppState>().addConnection(result);
    }
  }

  /// 基础图标(26px)+ 右下角绿色「+」徽章,与新建按钮角标一致。
  Widget _badgedIcon(String asset) => SizedBox(
        width: 26,
        height: 26,
        child: Stack(
          children: [
            Positioned.fill(child: UiIcon(asset, size: 26)),
            Positioned(right: 0, bottom: 0, child: _PlusBadge(size: 11)),
          ],
        ),
      );
}

/// 绿色圆形「+」徽章:自绘无动画,直径约为图标的三分之一。
class _PlusBadge extends StatelessWidget {
  const _PlusBadge({this.size = 11});

  final double size;

  @override
  Widget build(BuildContext context) =>
      CustomPaint(size: Size.square(size), painter: const _PlusBadgePainter());
}

class _PlusBadgePainter extends CustomPainter {
  const _PlusBadgePainter();

  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width;
    final c = Offset(s / 2, s / 2);
    // 绿色圆底
    canvas.drawCircle(c, s / 2, Paint()..color = const Color(0xff4caf50));
    // 白色「+」
    final p = Paint()
      ..color = Colors.white
      ..strokeWidth = s * 0.16
      ..strokeCap = StrokeCap.round;
    final arm = s * 0.26;
    canvas.drawLine(c - Offset(arm, 0), c + Offset(arm, 0), p);
    canvas.drawLine(c - Offset(0, arm), c + Offset(0, arm), p);
  }

  @override
  bool shouldRepaint(covariant _PlusBadgePainter oldDelegate) => false;
}
