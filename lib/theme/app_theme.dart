import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../l10n/locale_config.dart';

/// 通用 UI 色板:与 base-ui-flutter 的 [DesktopTokens] 命名对齐,
/// 各 widget 不硬编码颜色,统一通过 [Tokens.of] 取色。
///
/// 设计原则:按抽象语义层级命名(底色 / 前景 / 交互 / 线条),
/// 不绑定具体组件(如“工具栏”“状态栏”),仅包含通用 UI 结构色。
/// 业务定制色(图标 / 头像 等)由 [AppColors] 独立管理,
/// 不出现在主题定制对话框中。
class AppPalette {
  const AppPalette({
    required this.background,
    required this.surface,
    required this.control,
    required this.secondary,
    required this.statusBar,
    required this.popover,
    required this.foreground,
    required this.mutedForeground,
    required this.disabledForeground,
    required this.accentForeground,
    required this.accent,
    required this.highlight,
    required this.border,
    required this.divider,
    required this.gridLine,
    required this.muted,
  });

  /// 窗口 / 主面板底色 (→ backgroundColor)
  final Color background;

  /// 面板 / 次级区域底色 (→ surfaceColor)
  final Color surface;

  /// 控件 / 交互元素底色 (→ controlColor)
  final Color control;

  /// 次级 / 内嵌区域底色 (→ secondaryColor)
  final Color secondary;

  /// 条带区域底色
  final Color statusBar;

  /// 浮动层 / 弹出内容底色 (→ popoverColor)
  final Color popover;

  /// 主要前景色 (→ foregroundColor)
  final Color foreground;

  /// 次要前景色 (→ mutedForegroundColor)
  final Color mutedForeground;

  /// 禁用前景色 (→ disabledForegroundColor)
  final Color disabledForeground;

  /// 强调背景上的前景色 (→ accentForegroundColor)
  final Color accentForeground;

  /// 强调 / 选中态底色 (→ accentColor / primaryColor)
  final Color accent;

  /// 高亮态底色
  final Color highlight;

  /// 主边框线 (→ borderColor)
  final Color border;

  /// 次级分隔线
  final Color divider;

  /// 网格线
  final Color gridLine;

  /// 柔和底色 / 槽位背景 (→ mutedColor)
  final Color muted;

  /// 连接树节点选中背景:[accent] 低透明度混合 [background],
  /// 明暗自适应地得到淡蓝(文字仍用 [foreground],不再反白)
  Color get treeSelectedBg =>
      Color.alphaBlend(accent.withValues(alpha: 0.15), background);

  /// 文本划选背景:同上做法但更实一档(15% 太淡,划选看不出范围)。
  /// 不设 `textSelectionTheme` 时会落到 Material 3 基线的**紫色**(#6750A4),
  /// 与本应用的蓝色强调色不是一家 —— 见 `buildAppTheme`。
  Color get textSelectionBg =>
      Color.alphaBlend(accent.withValues(alpha: 0.30), background);

  /// 淡蓝档(20%):比 [treeSelectedBg] 实一档、比 [textSelectionBg] 淡一档。
  ///
  /// 给「同一屏里要同时出现三种选中强度」的场合用 —— 表设计器的焦点列表头与
  /// 行首当前行槽都是这一档(参考工具实测 #CCE4F6,即 #0078D4 按 20% 压白),
  /// 再淡就成了「看不出哪一列是当前列」。
  Color get accentSoftBg =>
      Color.alphaBlend(accent.withValues(alpha: 0.20), background);

  /// 拷贝并覆盖指定字段,用于主题定制时逐项修改。
  AppPalette copyWith({
    Color? background,
    Color? surface,
    Color? control,
    Color? secondary,
    Color? statusBar,
    Color? popover,
    Color? foreground,
    Color? mutedForeground,
    Color? disabledForeground,
    Color? accentForeground,
    Color? accent,
    Color? highlight,
    Color? border,
    Color? divider,
    Color? gridLine,
    Color? muted,
  }) =>
      AppPalette(
        background: background ?? this.background,
        surface: surface ?? this.surface,
        control: control ?? this.control,
        secondary: secondary ?? this.secondary,
        statusBar: statusBar ?? this.statusBar,
        popover: popover ?? this.popover,
        foreground: foreground ?? this.foreground,
        mutedForeground: mutedForeground ?? this.mutedForeground,
        disabledForeground: disabledForeground ?? this.disabledForeground,
        accentForeground: accentForeground ?? this.accentForeground,
        accent: accent ?? this.accent,
        highlight: highlight ?? this.highlight,
        border: border ?? this.border,
        divider: divider ?? this.divider,
        gridLine: gridLine ?? this.gridLine,
        muted: muted ?? this.muted,
      );

  /// 值相等:全部字段逐一比较。
  ///
  /// 用于识别"这份色板其实等于内置默认值"([AppTheme.isBuiltInDefault]),
  /// 因此必须是深比较而非引用比较。
  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is AppPalette &&
          other.background == background &&
          other.surface == surface &&
          other.control == control &&
          other.secondary == secondary &&
          other.statusBar == statusBar &&
          other.popover == popover &&
          other.foreground == foreground &&
          other.mutedForeground == mutedForeground &&
          other.disabledForeground == disabledForeground &&
          other.accentForeground == accentForeground &&
          other.accent == accent &&
          other.highlight == highlight &&
          other.border == border &&
          other.divider == divider &&
          other.gridLine == gridLine &&
          other.muted == muted;

  @override
  int get hashCode => Object.hash(
        background,
        surface,
        control,
        secondary,
        statusBar,
        popover,
        foreground,
        mutedForeground,
        disabledForeground,
        accentForeground,
        accent,
        highlight,
        border,
        divider,
        gridLine,
        muted,
      );

  /// 序列化为 字段名 -> #RRGGBB 的映射,用于本地持久化。
  Map<String, String> toJson() => {
        'background': _hex(background),
        'surface': _hex(surface),
        'control': _hex(control),
        'secondary': _hex(secondary),
        'statusBar': _hex(statusBar),
        'popover': _hex(popover),
        'foreground': _hex(foreground),
        'mutedForeground': _hex(mutedForeground),
        'disabledForeground': _hex(disabledForeground),
        'accentForeground': _hex(accentForeground),
        'accent': _hex(accent),
        'highlight': _hex(highlight),
        'border': _hex(border),
        'divider': _hex(divider),
        'gridLine': _hex(gridLine),
        'muted': _hex(muted),
      };

  /// 从持久化的字段映射还原色板;缺失 / 非法字段回退到 [AppTheme.light] 默认值。
  ///
  /// 兼容旧版键名:检测到旧格式(含 menuBar / textPrimary 等已废弃键)时
  /// 先映射到新键名再解析,确保用户历史定制不丢失。
  static AppPalette fromJson(Map<String, dynamic> json) {
    // 旧格式检测:只要含任一旧键名就走迁移分支
    const oldKeys = {
      'menuBar', 'ribbonBar', 'tabBar', 'popupBg',
      'textPrimary', 'textSecondary', 'textMuted', 'textOnSelected',
      'selectedBg', 'highlightBg', 'gutterBg',
    };
    final isLegacy = oldKeys.any(json.containsKey);
    if (isLegacy) {
      return AppTheme.light.copyWith(
        background: _colOrNull(json['surface']),
        surface: _colOrNull(json['menuBar']),
        control: _colOrNull(json['ribbonBar']),
        secondary: _colOrNull(json['tabBar']),
        statusBar: _colOrNull(json['statusBar']),
        popover: _colOrNull(json['popupBg']),
        foreground: _colOrNull(json['textPrimary']),
        mutedForeground: _colOrNull(json['textSecondary']),
        disabledForeground: _colOrNull(json['textMuted']),
        accentForeground: _colOrNull(json['textOnSelected']),
        accent: _colOrNull(json['selectedBg']),
        highlight: _colOrNull(json['highlightBg']),
        border: _colOrNull(json['border']),
        divider: _colOrNull(json['divider']),
        gridLine: _colOrNull(json['gridLine']),
        muted: _colOrNull(json['gutterBg']),
      );
    }
    return AppTheme.light.copyWith(
      background: _colOrNull(json['background']),
      surface: _colOrNull(json['surface']),
      control: _colOrNull(json['control']),
      secondary: _colOrNull(json['secondary']),
      statusBar: _colOrNull(json['statusBar']),
      popover: _colOrNull(json['popover']),
      foreground: _colOrNull(json['foreground']),
      mutedForeground: _colOrNull(json['mutedForeground']),
      disabledForeground: _colOrNull(json['disabledForeground']),
      accentForeground: _colOrNull(json['accentForeground']),
      accent: _colOrNull(json['accent']),
      highlight: _colOrNull(json['highlight']),
      border: _colOrNull(json['border']),
      divider: _colOrNull(json['divider']),
      gridLine: _colOrNull(json['gridLine']),
      muted: _colOrNull(json['muted']),
    );
  }

  /// 颜色 -> #RRGGBB(应用主题色均为不透明,丢弃 alpha 段)
  static String _hex(Color c) =>
      '#${c.toARGB32().toRadixString(16).padLeft(8, '0').substring(2).toUpperCase()}';

  /// #RRGGBB / #AARRGGBB -> Color;格式非法返回 null(调用方据此回退默认)
  static Color? _colOrNull(dynamic s) {
    if (s is! String) return null;
    var h = s.replaceAll('#', '').replaceAll('0x', '').trim();
    if (h.length == 6) h = 'FF$h';
    if (h.length != 8) return null;
    final v = int.tryParse(h, radix: 16);
    return v == null ? null : Color(v);
  }

  /// 将本应用语义色板桥接到 [DesktopTokens],
  /// 使 base_ui_flutter 组件自动跟随当前明 / 暗主题。
  ///
  /// 只做颜色映射,**不含显示缩放** —— 渲染组件请用 [desktopTokensFor]。
  DesktopTokens toDesktopTokens() {
    final light = background.computeLuminance() > 0.5;

    // ── 按钮(对齐 Windows 桌面按钮)────────────────────────────────────
    // 尺寸(24 高 / 最小 73 宽)由 base-ui 的 winForm 预设给(VCL/WinForms
    // 按钮的档位:24~25 高、75 宽,比编辑框高一档),这里只管配色。
    //
    // 面色朝"纸白"走:亮色主题直接用内容白(配 #F1F1F1 底条 = 参考图的
    // #FDFDFD 压在 #F0F0F0 上),暗色主题在铬件色上提亮一档,按钮才浮得起来。
    final buttonFace =
        light ? background : _blend(control, const Color(0x14FFFFFF));
    // 边线比通用控件边线深一档,补回面色变白后丢掉的轮廓感
    final buttonBorder = light
        ? _blend(border, const Color(0x19000000)) // #E7E7E7 → #D0D0D0
        : _blend(border, const Color(0x33FFFFFF));

    // 热态 / 按下态:桌面按钮的反馈是"面色染上强调色、边线换强调色",
    // 而不是把中性面色压暗 —— 后者只能叠出灰,越悬停越脏(参考图实测
    // 悬浮面 #E0EEF9 + 边 #0078D4,按下再深一档)。
    //
    // 亮色用参考图实测值钉死,刻意**不跟随**可定制的 accent:这是"桌面按钮
    // 的样式",不是"当前强调色"(实测值同时等于"面色 + accent 12%",想让它
    // 跟着 accent 走的用户把两条换成按 accent 混合即可)。
    // 暗色没有参考图,按同一条规则(染 accent、按下再深一档)现算。
    final hoverFace = light
        ? const Color(0xFFE0EEF9)
        : _blend(buttonFace, accent.withValues(alpha: 0.28));
    final hoverBorder = light ? const Color(0xFF0078D4) : accent;
    final pressedFace = light
        ? const Color(0xFFB3D6F2)
        : _blend(buttonFace, accent.withValues(alpha: 0.45));
    final pressedBorder = light
        ? const Color(0xFF006BBE)
        : _blend(accent, const Color(0x40000000));

    return DesktopTokens.winForm.copyWith(
      primaryColor: accent,
      backgroundColor: background,
      foregroundColor: foreground,
      borderColor: border,
      surfaceColor: background,
      controlColor: control,
      controlHoverColor: _blend(control, const Color(0x14000000)),
      controlPressedColor: _blend(control, const Color(0x1F000000)),
      controlDisabledColor: surface,
      disabledForegroundColor: disabledForeground,
      buttonFaceColor: buttonFace,
      buttonBorderColor: buttonBorder,
      buttonHoverFaceColor: hoverFace,
      buttonHoverBorderColor: hoverBorder,
      buttonPressedFaceColor: pressedFace,
      buttonPressedBorderColor: pressedBorder,
      mutedColor: muted,
      mutedForegroundColor: mutedForeground,
      secondaryColor: secondary,
      secondaryForegroundColor: foreground,
      accentColor: accent,
      accentForegroundColor: accentForeground,
      cardColor: background,
      cardForegroundColor: foreground,
      popoverColor: popover,
      popoverForegroundColor: foreground,
      ringColor: accent,
      // hover / pressed 叠加色必须明暗自适应:暗色提亮、亮色加深,
      // 否则 base-ui 组件(按钮 / 列表行等)在暗色主题下 hover 不可见
      hoverOverlayColor:
          light ? const Color(0x0F000000) : const Color(0x14FFFFFF),
      pressedOverlayColor:
          light ? const Color(0x1F000000) : const Color(0x24FFFFFF),
    );
  }

  /// 桥接后的 base-ui 令牌,并已按当前显示缩放把边框收成「1 设备像素」。
  ///
  /// 这是渲染 base-ui 组件时应该用的入口:参照的 Navicat 是原生窗口,边框恒为
  /// 1 **物理**像素,在 200% 缩放下就是 0.5 逻辑像素;令牌默认的 1.0 会画成
  /// 2 物理像素,看起来比原生窗口粗一倍。直接调 [toDesktopTokens] 会漏掉缩放,
  /// 由 `test/desktop_tokens_bridge_test.dart` 守护。
  DesktopTokens desktopTokensFor(BuildContext context) =>
      toDesktopTokens().withHairlineBorders(context);

  /// Alpha-blend [overlay] onto [base].
  static Color _blend(Color base, Color overlay) =>
      Color.alphaBlend(overlay, base);
}

/// 业务定制色:图标 / 头像等应用特有颜色,不暴露给主题定制对话框。
///
/// 命名采用语义泛称,不绑定具体业务实体:
/// - [iconPrimary]   — 主图标色(表 / 函数)
/// - [iconInfo]      — 信息图标色(连接)
/// - [iconSuccess]   — 成功图标色(数据库)
/// - [iconSecondary] — 次级图标色(模式 / 用户)
/// - [iconWarning]   — 警告图标色(文件夹)
/// - [avatarSurface] — 头像背景
/// - [avatarForeground] — 头像前景
class AppColors {
  const AppColors({
    required this.iconPrimary,
    required this.iconInfo,
    required this.iconSuccess,
    required this.iconSecondary,
    required this.iconWarning,
    required this.avatarSurface,
    required this.avatarForeground,
  });

  /// 主图标色(表 / 函数)
  final Color iconPrimary;

  /// 信息图标色(连接)
  final Color iconInfo;

  /// 成功图标色(数据库)
  final Color iconSuccess;

  /// 次级图标色(模式 / 用户)
  final Color iconSecondary;

  /// 警告图标色(文件夹)
  final Color iconWarning;

  /// 头像背景
  final Color avatarSurface;

  /// 头像前景
  final Color avatarForeground;

  /// 明亮主题业务色默认值
  static const light = AppColors(
    iconPrimary: Color(0xff1565c0),
    iconInfo: Color(0xff1f6feb),
    iconSuccess: Color(0xff2e9e4f),
    iconSecondary: Color(0xff1e8e3e),
    iconWarning: Color(0xffb58a00),
    avatarSurface: Color(0xffc9ccd1),
    avatarForeground: Color(0xff3c4043),
  );

  /// 暗黑主题业务色默认值
  static const dark = AppColors(
    iconPrimary: Color(0xff6cb6ff),
    iconInfo: Color(0xff7ec1ff),
    iconSuccess: Color(0xff8fd48f),
    iconSecondary: Color(0xff57c25a),
    iconWarning: Color(0xffe9c46a),
    avatarSurface: Color(0xff8e98a5),
    avatarForeground: Color(0xffe9edf2),
  );

  /// 从 Provider 取业务色;未注入时回退明 / 暗默认值
  static AppColors of(BuildContext context) {
    try {
      return Provider.of<AppColors>(context, listen: true);
    } catch (_) {
      final brightness = Theme.of(context).brightness;
      return brightness == Brightness.dark ? dark : light;
    }
  }
}

/// 明暗双主题色板定义
class AppTheme {
  /// 明亮主题:纸白配色(对齐 Navicat 等桌面数据库工具的通透观感)。
  ///
  /// 取色原则 —— 灰阶层级收紧为"白 + 两级极浅灰",不再各区域各自一档:
  ///
  /// | 层级 | 用途 | 值 |
  /// |---|---|---|
  /// | 内容 | 网格 / 编辑器 / 树 / 弹层 | `#FFFFFF` |
  /// | 铬件 | 菜单栏 / ribbon / 面板工具栏 / 状态栏 | `#F8F8F8` |
  /// | 次级 | 标签条 / 面板标题条 / 内嵌带 | `#F1F1F1` |
  /// | 线条 | 边框 / 分隔线 / 网格线 | `#E7E7E7` / `#EFEFEF` |
  ///
  /// 历史坑:0.5 及更早版本用 `#F3F3F3` 做铬件、`#E7E7E7` 做状态栏、
  /// `#E0E0E0` 做分割线,同一屏里叠了 5 档互不相同的灰,整窗观感发灰发脏。
  /// 调整时请保持"层级数少、档位浅"这两条,不要为单个组件单独加深。
  ///
  /// 强调色取 Windows 桌面蓝 `#0078D4`(参考工具实测的选中底色),
  /// [highlight] 取同族更亮一档的 `#4DA0E1`(实测的 2px 强调条 / 焦点列下划线)。
  static const light = AppPalette(
    background: Color(0xffffffff),
    surface: Color(0xfff8f8f8),
    control: Color(0xfff8f8f8),
    secondary: Color(0xfff1f1f1),
    statusBar: Color(0xfff4f4f4),
    popover: Color(0xffffffff),
    foreground: Color(0xff1f1f1f),
    mutedForeground: Color(0xff5f6368),
    disabledForeground: Color(0xff9aa0a6),
    accentForeground: Color(0xffffffff),
    accent: Color(0xff0078D4),
    highlight: Color(0xff4DA0E1),
    border: Color(0xffe7e7e7),
    divider: Color(0xffefefef),
    gridLine: Color(0xffefefef),
    muted: Color(0xfff4f4f4),
  );

  /// 明亮主题的历史默认值(0.5 及更早,即上表里被替换掉的那一版)。
  ///
  /// 不参与任何渲染,只给 [isBuiltInDefault] 做判定用:老版本在用户打开
  /// 主题定制弹窗并点过"应用"后,会把当时的**默认值**当成"用户定制"写进
  /// theme_custom.json;若不识别出来,升级默认配色后它会一直把新配色盖住
  /// (症状:改了 [light] 却看不到任何变化)。
  static const lightLegacy = AppPalette(
    background: Color(0xffffffff),
    surface: Color(0xfff3f3f3),
    control: Color(0xfff3f3f3),
    secondary: Color(0xffececec),
    statusBar: Color(0xffe7e7e7),
    popover: Color(0xffffffff),
    foreground: Color(0xff1f1f1f),
    mutedForeground: Color(0xff5f6368),
    disabledForeground: Color(0xff80868b),
    accentForeground: Color(0xffffffff),
    accent: Color(0xff2196F3),
    highlight: Color(0xff42A5F5),
    border: Color(0xffe0e0e0),
    divider: Color(0xffe0e0e0),
    gridLine: Color(0xffe3e3e3),
    muted: Color(0xfff3f3f3),
  );

  /// 明亮主题的历史默认值(0.6 ~ 0.8.1 的「纸白」那一版)。
  ///
  /// 不参与任何渲染,只给 [isBuiltInDefault] 做判定用:这一版内置的强调色还是
  /// `#2196F3`,而 0.9 起内置改成桌面蓝 `#0078D4`;老用户若在主题弹窗里点过
  /// "应用"(哪怕什么都没改),不识别出这份"伪定制"就永远看不到新强调色。
  static const lightLegacy08 = AppPalette(
    background: Color(0xffffffff),
    surface: Color(0xfff8f8f8),
    control: Color(0xfff8f8f8),
    secondary: Color(0xfff1f1f1),
    statusBar: Color(0xfff4f4f4),
    popover: Color(0xffffffff),
    foreground: Color(0xff1f1f1f),
    mutedForeground: Color(0xff5f6368),
    disabledForeground: Color(0xff9aa0a6),
    accentForeground: Color(0xffffffff),
    accent: Color(0xff2196F3),
    highlight: Color(0xff42A5F5),
    border: Color(0xffe7e7e7),
    divider: Color(0xffefefef),
    gridLine: Color(0xffefefef),
    muted: Color(0xfff4f4f4),
  );

  /// 暗黑主题的历史默认值(0.8.1 及更早,自 0.5 起一直没动过,直到强调色改档)。
  ///
  /// 同 [lightLegacy08]:只用于识别"伪定制",不参与渲染。
  static const darkLegacy08 = AppPalette(
    background: Color(0xff252526),
    surface: Color(0xff383838),
    control: Color(0xff383838),
    secondary: Color(0xff2d2d2d),
    statusBar: Color(0xff303030),
    popover: Color(0xff2d2d2d),
    foreground: Color(0xffc8cdd4),
    mutedForeground: Color(0xff9aa0a6),
    disabledForeground: Color(0xff6f7479),
    accentForeground: Color(0xffffffff),
    accent: Color(0xff2196F3),
    highlight: Color(0xff42A5F5),
    border: Color(0xff3c3c3c),
    divider: Color(0xff4a4a4a),
    gridLine: Color(0xff3c3c3c),
    muted: Color(0xff2a2a2a),
  );

  /// [p] 是否就是某一版内置默认色板(当前版或历史版)。
  ///
  /// 命中的含义是"用户其实没定制过",加载时应丢弃、跟随 [light] / [dark]
  /// 的内置值,否则内置配色升级将不会生效。
  static bool isBuiltInDefault(AppPalette p) =>
      p == light ||
      p == dark ||
      p == lightLegacy ||
      p == lightLegacy08 ||
      p == darkLegacy08;

  /// 暗黑主题:沿用原版深色配色,与设计稿一致
  static const dark = AppPalette(
    background: Color(0xff252526),
    // 与 control 同色:标题栏与下方工具栏背景统一,
    // 与明亮主题下 surface == control 的行为保持一致
    surface: Color(0xff383838),
    control: Color(0xff383838),
    secondary: Color(0xff2d2d2d),
    statusBar: Color(0xff303030),
    popover: Color(0xff2d2d2d),
    foreground: Color(0xffc8cdd4),
    mutedForeground: Color(0xff9aa0a6),
    disabledForeground: Color(0xff6f7479),
    accentForeground: Color(0xffffffff),
    accent: Color(0xff0078D4),
    highlight: Color(0xff4DA0E1),
    // 边框色需比背景(background 0xff252526)亮,否则暗色下控件边框不可见
    border: Color(0xff3c3c3c),
    divider: Color(0xff4a4a4a),
    gridLine: Color(0xff3c3c3c),
    muted: Color(0xff2a2a2a),
  );
}

/// 取色辅助:返回当前主题亮度对应的语义色板。
///
/// [of] 直接从 [AppState] 读取当前生效的色板(可能是用户定制后的),
/// 通过 context.watch 注册依赖,确保主题变化时依赖组件自动重建;
/// 仅允许在 build 期间调用。事件回调中取色请用 [read]。
class Tokens {
  const Tokens._();

  /// build 期间取色:注册对 [AppState] 的依赖,主题变化时自动重建。
  static AppPalette of(BuildContext context) {
    final app = _maybeApp(context, listen: true);
    return app == null ? paletteOf(context) : _paletteOf(app);
  }

  /// 事件回调 / build 之外取色:不注册依赖,不会因 watch 时机触发断言。
  static AppPalette read(BuildContext context) {
    final app = _maybeApp(context, listen: false);
    return app == null ? paletteOf(context) : _paletteOf(app);
  }

  /// 没有 [AppState] 时的取色:独立子窗口是同进程里的另一个 Flutter 引擎,
  /// 树里不挂 AppState,色板由子窗口根注入成 Provider(创建时即定稿)。
  static AppPalette paletteOf(BuildContext context) {
    try {
      return Provider.of<AppPalette>(context, listen: false);
    } catch (_) {
      return Theme.of(context).brightness == Brightness.dark
          ? AppTheme.dark
          : AppTheme.light;
    }
  }

  static AppState? _maybeApp(BuildContext context, {required bool listen}) {
    try {
      return listen ? context.watch<AppState>() : context.read<AppState>();
    } catch (_) {
      // ProviderNotFoundException:当前引擎没有主窗口的 AppState
      return null;
    }
  }

  static AppPalette _paletteOf(AppState app) {
    final mode = app.themeMode;
    final brightness = mode == ThemeMode.system
        ? WidgetsBinding.instance.platformDispatcher.platformBrightness
        : (mode == ThemeMode.dark ? Brightness.dark : Brightness.light);
    return brightness == Brightness.dark ? app.effectiveDark : app.effectiveLight;
  }
}

/// 正文文字色:明亮主题下取纯黑(区别于色板 token 的近黑 `0xff1f1f1f`),
/// 暗色主题沿用 token 前景色,避免深底不可读。
/// 用于 ribbon / 连接树等需要纯黑正文的区域。
/// 始终经 [Tokens.of] 取色以注册主题依赖,保证切换明暗后自动重建。
Color bodyTextColor(BuildContext context) {
  final foreground = Tokens.of(context).foreground;
  return Theme.of(context).brightness == Brightness.dark
      ? foreground
      : const Color(0xff000000);
}

/// 应用 [ThemeData]:主窗口与「连接密码」等独立子窗口共用,
/// 保证两处的中日文回退字体、滚动条与弹层底色一致。
///
/// [languageCode] 决定字体回退(日语界面要日式字形);省略时按中文回退,
/// 与多语言接入前的行为一致。
ThemeData buildAppTheme(Brightness brightness, AppPalette palette,
    {String? languageCode}) {
  return ThemeData(
    brightness: brightness,
    scaffoldBackgroundColor: palette.background,
    canvasColor: palette.background,
    popupMenuTheme: PopupMenuThemeData(color: palette.popover),
    // 文本选区 / 光标:不设就会落到 Material 3 基线的紫色(#6750A4),
    // 输入框、文本域、以及只读的 SelectableText(部署脚本 / 日志、SQL 预览)
    // 全都会显示成紫色选区,与应用蓝色强调色不是一家。
    textSelectionTheme: TextSelectionThemeData(
      selectionColor: palette.textSelectionBg,
      cursorColor: palette.accent,
      selectionHandleColor: palette.accent,
    ),
    // 全局滚动条：静止收窄，鼠标悬浮 / 拖动时恢复常规宽度
    scrollbarTheme: ScrollbarThemeData(
      thickness: scrollbarHoverThickness(),
    ),
    textTheme: (brightness == Brightness.dark
            ? Typography.material2021().white
            : Typography.material2021().black)
        .apply(fontFamilyFallback: fontFamilyFallbackFor(languageCode)),
  );
}
