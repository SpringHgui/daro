import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:window_manager/window_manager.dart';
import 'app/app_state.dart';
import 'app/ime_suppressor.dart';
import 'app/mcp_service.dart';
import 'app/sub_window.dart';
import 'l10n/locale_config.dart';
import 'pages/main_page.dart';
import 'theme/app_theme.dart';

Future<void> main(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();

  // 密码框获焦关输入法(见 app/ime_suppressor.dart)。主窗口与子窗口都走这个
  // main(),各自引擎装一次即可;原生通道按视图 HWND 分窗口注册,互不干扰。
  installImeSuppression();

  // desktop_multi_window 起的子窗口(如「连接密码」)自带入口
  // 参数,走各自的 App,不加载主窗口与 AppState。
  final subWindow = buildSubWindowApp(args);
  if (subWindow != null) {
    runApp(subWindow);
    return;
  }

  // 桌面窗口管理:隐藏系统标题栏,菜单 + 三个窗口按钮半自绘到同一行
  // (见 lib/widgets/top_menu.dart)。Snap Layouts hover 最大化按钮的菜单
  // 在此模式下不会弹出——需修改 windows/runner 的 WM_NCHITTEST,见 README。
  await windowManager.ensureInitialized();
  const windowOptions = WindowOptions(
    minimumSize: Size(1000, 700),
    center: true,
    titleBarStyle: TitleBarStyle.hidden,
  );
  await windowManager.waitUntilReadyToShow(windowOptions, () async {
    // 用 edge-trimmed 的 PNG 转成的 ICO(内部嵌入 PNG 数据)设置任务栏图标。
    // setIcon 在 Windows 平台通过 WM_SETICON 替换窗口/任务栏图标,
    // 路径相对于 <EXE 目录>/data/flutter_assets/。
    await windowManager.setTitle('daro');
    await windowManager.setIcon('windows/runner/resources/app_icon.ico');
    await windowManager.show();
    await windowManager.focus();
  });

  runApp(
    ChangeNotifierProvider(
      create: (_) => AppState(),
      child: const DbApp(),
    ),
  );
}

/// 主窗口 App。
class DbApp extends StatelessWidget {
  const DbApp({super.key});

  @override
  Widget build(BuildContext context) {
    final app = context.watch<AppState>();

    // 生效语言:用户显式选择优先,否则跟随系统(未命中兜底英语)。
    final locale = app.resolveLocale();
    // 镜像到引擎级变量:子窗口是另一个引擎、按入口参数各自维护,而主窗口这里
    // 每次重建都同步一次,供拿不到 BuildContext 的角落(字体回退、子窗口参数)取用。
    currentLanguageCode = locale.languageCode;

    // 根据主题模式解析实际亮度:跟随系统时读平台亮度。
    // 这里不依赖 Theme.of,因为 TokenScope 需要放在 MaterialApp 之上,
    // 才能覆盖 Dialog / Overlay 等 Navigator 上的弹窗(它们拿不到 home 里的 TokenScope)。
    final brightness = _brightnessFor(app.themeMode);
    final palette = brightness == Brightness.dark
        ? app.effectiveDark
        : app.effectiveLight;

    // 业务定制色(图标 / 头像等)独立于通用色板,不参与主题定制对话框
    final colors = brightness == Brightness.dark
        ? AppColors.dark
        : AppColors.light;

    // 通过 Provider<AppPalette> 注入通用色板(可能是用户定制后的),
    // Provider<AppColors> 注入业务色;
    // TokenScope 把通用色板桥接为 DesktopTokens,使 base-ui-flutter 组件跟随定制。
    // McpService 也挂在 MaterialApp 之上:设置页与状态栏是 Dialog / Overlay,拿不到 home 下的 Provider。
    return Provider<AppPalette>.value(
      value: palette,
      child: Provider<AppColors>.value(
        value: colors,
        child: ChangeNotifierProvider<McpService>.value(
          value: app.mcp,
          child: TokenScope(
            // 边框统一收成「1 设备像素」(见 AppPalette.desktopTokensFor):
            // 参照的 Navicat 是原生窗口,边框恒为 1 物理像素,在 200% 缩放下
            // 就是 0.5 逻辑像素;令牌默认的 1.0 会画成 2 物理像素,粗一倍。
            tokens: palette.desktopTokensFor(context),
            child: MaterialApp(
              title: 'daro',
              debugShowCheckedModeBanner: false,
              home: Material(
                type: MaterialType.transparency,
                child: const MainPage(),
              ),
              theme: buildAppTheme(Brightness.light, app.effectiveLight,
                  languageCode: locale.languageCode),
              darkTheme: buildAppTheme(Brightness.dark, app.effectiveDark,
                  languageCode: locale.languageCode),
              themeMode: app.themeMode,
              localizationsDelegates: kAppLocalizationsDelegates,
              supportedLocales: kSupportedLocales,
              locale: locale,
            ),
          ),
        ),
      ),
    );
  }

  /// 把 [ThemeMode] 解析成 [Brightness]。
  Brightness _brightnessFor(ThemeMode mode) {
    switch (mode) {
      case ThemeMode.light:
        return Brightness.light;
      case ThemeMode.dark:
        return Brightness.dark;
      case ThemeMode.system:
        return WidgetsBinding.instance.platformDispatcher.platformBrightness;
    }
  }
}
