import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:daro/theme/app_theme.dart';

/// 明亮主题「纸白」层级的守护测试。
///
/// 明亮主题的观感问题(整窗发灰、发脏)来自同一屏里叠了太多档不同的浅灰,
/// 因此这里不锁死具体色值,只锁死三条设计约束 —— 换个色值也必须满足,
/// 但任何一条被破坏都说明层级又散开了。
void main() {
  group('明亮主题的纸白层级', () {
    test('内容区纯白,铬件 / 次级底 / 条带都保持极浅', () {
      final p = AppTheme.light;
      expect(p.background, const Color(0xFFFFFFFF));
      expect(p.popover, const Color(0xFFFFFFFF));

      final chrome = <String, Color>{
        'surface(面板 / 菜单栏)': p.surface,
        'control(控件 / ribbon)': p.control,
        'secondary(标签条 / 标题条)': p.secondary,
        'statusBar(状态栏)': p.statusBar,
        'muted(柔和底)': p.muted,
      };
      chrome.forEach((name, c) {
        expect(
          c.computeLuminance(),
          greaterThan(0.85),
          reason: '$name 应保持接近白,不能再退回 F3F3F3 / E7E7E7 那一档',
        );
      });
    });

    test('层级单调:内容底 ≥ 铬件 ≥ 次级底', () {
      final p = AppTheme.light;
      final content = p.background.computeLuminance();
      final control = p.control.computeLuminance();
      final secondary = p.secondary.computeLuminance();
      expect(content, greaterThanOrEqualTo(control));
      expect(control, greaterThanOrEqualTo(secondary));
      expect(p.surface, p.control, reason: '菜单栏与 ribbon 必须同色,否则顶部会出现色阶缝');
    });

    test('线条比所邻底色深,但仍是浅灰', () {
      final p = AppTheme.light;
      final surface = p.surface.computeLuminance();
      for (final c in [p.border, p.divider, p.gridLine]) {
        expect(c.computeLuminance(), lessThan(surface), reason: '线条要能看见');
        expect(c.computeLuminance(), greaterThan(0.7), reason: '线条不能变回深灰网格');
      }
      expect(p.border.computeLuminance(),
          lessThanOrEqualTo(p.gridLine.computeLuminance()),
          reason: '结构边框应比网格线更实,否则面板边界会糊掉');
    });
  });

  group('内置默认色板识别(AppPalette 深比较)', () {
    test('当前默认与历史默认都算「未定制」', () {
      expect(AppTheme.isBuiltInDefault(AppTheme.light), isTrue);
      expect(AppTheme.isBuiltInDefault(AppTheme.dark), isTrue);
      expect(AppTheme.isBuiltInDefault(AppTheme.lightLegacy), isTrue);
      // 0.9 把内置强调色从 #2196F3 换成桌面蓝 #0078D4:上一版的明 / 暗默认值
      // 必须继续被认成"伪定制",否则老用户永远看不到新强调色。
      expect(AppTheme.isBuiltInDefault(AppTheme.lightLegacy08), isTrue);
      expect(AppTheme.isBuiltInDefault(AppTheme.darkLegacy08), isTrue);
    });

    test('强调色确实换到了桌面蓝,且旧档只作为历史值存在', () {
      expect(AppTheme.light.accent, const Color(0xFF0078D4));
      expect(AppTheme.light.highlight, const Color(0xFF4DA0E1));
      expect(AppTheme.dark.accent, const Color(0xFF0078D4));
      // 淡蓝档 = accent 20% 压白,实测参考工具的焦点列表头 / 行首槽就是这个值
      // (比较用 toARGB32:alphaBlend 产出的是 float 分量色,与 8 位常量直接 == 不可靠)
      expect(AppTheme.light.accentSoftBg.toARGB32(), 0xFFCCE4F6);
      expect(AppTheme.lightLegacy08.accent, const Color(0xFF2196F3));
      expect(
        AppTheme.isBuiltInDefault(AppTheme.light.copyWith(accent: const Color(0xFF2196F3))),
        isFalse,
        reason: '只把新默认换成旧强调色 = 真定制,不能被当成历史默认丢掉',
      );
    });

    test('动过任何一个字段就不算内置默认', () {
      expect(
        AppTheme.isBuiltInDefault(
            AppTheme.light.copyWith(accent: const Color(0xFF0F6CBD))),
        isFalse,
      );
      // 只改一个底色字段也应被识别为真正的定制
      expect(
        AppTheme.isBuiltInDefault(
            AppTheme.light.copyWith(statusBar: const Color(0xFFE7E7E7))),
        isFalse,
      );
    });
  });

  group('theme_custom.json 迁移', () {
    // 0.5 及更早版本在主题定制弹窗里点过「应用」后落盘的内容 ——
    // 就是当时的内置默认值,属"伪定制",加载时必须丢弃。
    const legacyFile = '''
{"light":{"background":"#FFFFFF","surface":"#F3F3F3","control":"#F3F3F3",
"secondary":"#ECECEC","statusBar":"#E7E7E7","popover":"#FFFFFF","foreground":"#1F1F1F",
"mutedForeground":"#5F6368","disabledForeground":"#80868B","accentForeground":"#FFFFFF",
"accent":"#2196F3","highlight":"#42A5F5","border":"#E0E0E0","divider":"#E0E0E0",
"gridLine":"#E3E3E3","muted":"#F3F3F3"},"dark":{"background":"#252526",
"surface":"#383838","control":"#383838","secondary":"#2D2D2D","statusBar":"#303030",
"popover":"#2D2D2D","foreground":"#C8CDD4","mutedForeground":"#9AA0A6",
"disabledForeground":"#6F7479","accentForeground":"#FFFFFF","accent":"#2196F3",
"highlight":"#42A5F5","border":"#3C3C3C","divider":"#4A4A4A","gridLine":"#3C3C3C",
"muted":"#2A2A2A"}}
''';

    AppPalette parse(String key) =>
        AppPalette.fromJson(jsonDecode(legacyFile)[key] as Map<String, dynamic>);

    test('旧版整份默认值会被识别为伪定制', () {
      expect(AppTheme.isBuiltInDefault(parse('light')), isTrue);
      expect(AppTheme.isBuiltInDefault(parse('dark')), isTrue);
    });

    test('旧版默认值 + 用户改过某个字段 → 保留为定制', () {
      final custom = parse('light').copyWith(accent: const Color(0xFF0F6CBD));
      expect(AppTheme.isBuiltInDefault(custom), isFalse);
      expect(custom.accent, const Color(0xFF0F6CBD));
    });

    test('旧格式(menuBar/textPrimary 等废弃键)仍按映射表解析', () {
      final old = AppPalette.fromJson(const {
        'menuBar': '#F3F3F3',
        'ribbonBar': '#F3F3F3',
        'surface': '#FFFFFF',
        'tabBar': '#ECECEC',
        'statusBar': '#E7E7E7',
        'popupBg': '#FFFFFF',
        'textPrimary': '#1F1F1F',
        'textSecondary': '#5F6368',
        'textMuted': '#80868B',
        'textOnSelected': '#FFFFFF',
        'selectedBg': '#2196F3',
        'highlightBg': '#42A5F5',
        'border': '#E0E0E0',
        'divider': '#E0E0E0',
        'gridLine': '#E3E3E3',
        'gutterBg': '#F3F3F3',
      });
      // 旧键 menuBar → 新键 surface,旧键 surface → 新键 background(命名整体平移)
      expect(old.surface, const Color(0xFFF3F3F3));
      expect(old.background, const Color(0xFFFFFFFF));
      expect(old.control, const Color(0xFFF3F3F3));
      expect(old.secondary, const Color(0xFFECECEC));
      expect(old.gridLine, const Color(0xFFE3E3E3));
    });

    test('色板序列化 → 反序列化后值相等', () {
      expect(AppPalette.fromJson(AppTheme.light.toJson()), AppTheme.light);
      expect(AppPalette.fromJson(AppTheme.dark.toJson()), AppTheme.dark);
    });
  });
}
