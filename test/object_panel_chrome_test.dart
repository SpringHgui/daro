import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/theme/app_theme.dart';
import 'package:daro/widgets/object_panel.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:daro/l10n/locale_config.dart';

/// 对象面板的铬件层级守护。
///
/// 对象面板工具条底色取**次级底** (`AppPalette.secondary`,明亮 #F1F1F1),
/// 与 Navicat 对象面板工具条实测的 #F0F0F0 对齐 —— 形成"灰工具条 + 白领区"
/// 的标准桌面布局。曾有一版为规避窄面板"整条带子发灰"改用 `background`(与正文
/// 同白),结果与参考图差异更明显(参考图的工具条本就是灰底),故改回灰底。
/// 面板里参与分层的只有二级灰(`secondary` 与 `surface` / `control`),不得再
/// 为这条带子单独加深一档(见 `AppTheme.light` 的注释与 theme_palette_test)。
void main() {
  Future<AppState> pumpObjectPanel(
    WidgetTester tester, {
    ThemeMode mode = ThemeMode.light,
  }) async {
    final app = AppState();
    app.setThemeMode(mode);
    final palette = mode == ThemeMode.dark ? AppTheme.dark : AppTheme.light;
    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<AppState>.value(value: app),
          Provider<AppPalette>.value(value: palette),
          Provider<AppColors>.value(
              value: mode == ThemeMode.dark
                  ? AppColors.dark
                  : AppColors.light),
        ],
        child: TokenScope(
          tokens: palette.toDesktopTokens(),
          child: MaterialApp(
            locale: const Locale('zh'),
            localizationsDelegates: kAppLocalizationsDelegates,
            supportedLocales: kSupportedLocales,
            theme: ThemeData(brightness: Brightness.light),
            home: const Material(
              type: MaterialType.transparency,
              child: ObjectPanel(),
            ),
          ),
        ),
      ),
    );
    await tester.pump();
    return app;
  }

  DesktopTokens stripTokens(WidgetTester tester) {
    final strip = tester.widget<ToolStrip>(find.byType(ToolStrip));
    return strip.tokens!;
  }

  testWidgets('明亮主题:工具条底色 = 次级灰 secondary(不是内容白)', (tester) async {
    await pumpObjectPanel(tester);
    final tokens = stripTokens(tester);
    expect(tokens.controlColor, AppTheme.light.secondary);
    expect(tokens.controlColor, isNot(AppTheme.light.background),
        reason: '对齐 Navicat:工具条是灰底,列表才是白底');
    expect(tokens.controlColor, isNot(AppTheme.light.control),
        reason: '铬件底 #F8F8F8 比参考图浅一档,灰工具条 + 灰表头会连不成一块');
  });

  testWidgets('暗色主题:工具条用次级底(比正文提亮一档)', (tester) async {
    await pumpObjectPanel(tester, mode: ThemeMode.dark);
    final tokens = stripTokens(tester);
    expect(tokens.controlColor, AppTheme.dark.secondary);
    expect(tokens.controlColor.computeLuminance(),
        greaterThan(AppTheme.dark.background.computeLuminance()),
        reason: '暗色下铬件必须比正文亮,否则整块糊成一片');
  });

  testWidgets('hover / pressed 由工具条底色派生,亮色下更深', (tester) async {
    await pumpObjectPanel(tester);
    final tokens = stripTokens(tester);
    final base = AppTheme.light.secondary.computeLuminance();
    expect(tokens.controlHoverColor.computeLuminance(), lessThan(base));
    expect(tokens.controlPressedColor.computeLuminance(),
        lessThan(tokens.controlHoverColor.computeLuminance()),
        reason: '按下应比悬浮更明显');
  });
}
