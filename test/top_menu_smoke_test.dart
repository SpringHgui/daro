import 'package:daro/app/app_state.dart';
import 'package:daro/widgets/navicat_import_dialog.dart';
import 'package:daro/widgets/options_dialog.dart';
import 'package:daro/widgets/top_menu.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:url_launcher_platform_interface/link.dart';
import 'package:url_launcher_platform_interface/url_launcher_platform_interface.dart';
import 'package:daro/l10n/locale_config.dart';

/// 记录 launch 调用的假实现:测试进程里没有 url_launcher 的原生插件,
/// 直接调用会 MissingPluginException,必须替换平台实例才能验证接线。
class _RecordingUrlLauncher extends UrlLauncherPlatform {
  String? lastUrl;
  LaunchOptions? lastOptions;

  @override
  LinkDelegate? get linkDelegate => null;

  @override
  Future<bool> launchUrl(String url, LaunchOptions options) async {
    lastUrl = url;
    lastOptions = options;
    return true;
  }

  @override
  Future<bool> supportsMode(PreferredLaunchMode mode) async => true;
}

/// Provider 必须在 MaterialApp 之上(与 main.dart 一致):showDialog 的弹层路由
/// 挂在 Navigator 上,拿不到 home 里的 Provider,取色时会直接抛异常。
Widget harness(AppState app) => ChangeNotifierProvider<AppState>.value(
      value: app,
      child: MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: kAppLocalizationsDelegates,
        supportedLocales: kSupportedLocales,
        theme: ThemeData(brightness: Brightness.dark),
        home: const TopMenu(),
      ),
    );

void main() {
  testWidgets('TopMenu lays out without throwing', (tester) async {
    await tester.pumpWidget(harness(AppState()));
    await tester.pumpAndSettle();
    expect(find.byType(TopMenu), findsOneWidget);
  });

  testWidgets('文件菜单里的「导入连接」可打开导入向导',
      (tester) async {
    await tester.pumpWidget(harness(AppState()));
    await tester.pumpAndSettle();
    await tester.tap(find.text('文件'));
    await tester.pumpAndSettle();
    expect(find.text('导入连接'), findsOneWidget);

    await tester.tap(find.text('导入连接'));
    await tester.pumpAndSettle();
    expect(find.byType(NavicatImportDialog), findsOneWidget);
    // 只认「请先选择导出文件」这句前缀:引导文案里的后缀列表
    // (.ncx / .ano)会随支持的格式变,不该牵动这条冒烟线
    expect(find.textContaining('请先选择导出文件'), findsOneWidget);
    // 来源工具卡片:菜单入口进来的同一块区域,厂商标识要真的在
    expect(find.text('Another Redis Desktop Manager'), findsOneWidget);
  });

  testWidgets('工具菜单里的「选项…」打开选项对话框', (tester) async {
    await tester.pumpWidget(harness(AppState()));
    await tester.pumpAndSettle();
    await tester.tap(find.text('工具'));
    await tester.pumpAndSettle();

    await tester.tap(find.text('选项...'));
    await tester.pumpAndSettle();
    expect(find.byType(OptionsDialog), findsOneWidget);
    // 语言入口已从顶部菜单的「语言」子菜单搬进「常规」页
    expect(find.text('语言'), findsOneWidget);
  });

  testWidgets('帮助菜单里的「问题反馈」打开 GitHub Issues',
      (tester) async {
    final fake = _RecordingUrlLauncher();
    final original = UrlLauncherPlatform.instance;
    UrlLauncherPlatform.instance = fake;
    addTearDown(() => UrlLauncherPlatform.instance = original);

    await tester.pumpWidget(harness(AppState()));
    await tester.pumpAndSettle();
    await tester.tap(find.text('帮助'));
    await tester.pumpAndSettle();
    expect(find.text('问题反馈'), findsOneWidget);

    await tester.tap(find.text('问题反馈'));
    await tester.pumpAndSettle();
    expect(fake.lastUrl, 'https://github.com/SpringHgui/daro/issues');
    // 外部应用模式:交给系统默认浏览器,而不是应用内 webview。
    expect(fake.lastOptions?.mode, PreferredLaunchMode.externalApplication);
  });
}
