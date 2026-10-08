/// 多语言的单一配置点:支持哪些语言、怎么从系统语言选出一个、各自用什么字体。
///
/// 主窗口的语言偏好由 `AppState` 持有并落盘;`desktop_multi_window` 起的子窗口
/// 是**另一个 Flutter 引擎**,拿不到 AppState,只能靠入口参数把语言码带过去
/// (见 `sub_window.dart` 的 `openSubWindow` / `buildSubWindowApp`)。
library;

import 'package:flutter/widgets.dart';

import 'app_localizations.dart';

export 'app_localizations.dart' show AppLocalizations;

/// 支持的语言码。ARB 文件名必须与此一一对应。
const List<String> kSupportedLanguageCodes = ['zh', 'en', 'ja'];

/// 跟随系统且系统语言未命中时的兜底语言
const String kFallbackLanguageCode = 'en';

/// 语言的**自称名**(endonym)。刻意不进 ARB:语言选择器里的「English / 日本語 /
/// 简体中文」在任何界面语言下都按原文显示,译成当地语言反而让人找不到自己的语言。
const Map<String, String> kLanguageEndonyms = {
  'zh': '简体中文',
  'en': 'English',
  'ja': '日本語',
};

/// 本引擎当前生效的语言码。
///
/// 只给「拿不到 BuildContext 又必须与界面同语言」的角落用:子窗口入口先按参数
/// 赋值再建 App。主窗口取文案一律走 [AppLocalizations.of]。
String currentLanguageCode = kFallbackLanguageCode;

/// 应用文案的本地化 delegate(含 Material / Widgets / Cupertino 的系统文案)。
final List<LocalizationsDelegate<dynamic>> kAppLocalizationsDelegates =
    AppLocalizations.localizationsDelegates;

/// 应用支持的 locale 列表。
final List<Locale> kSupportedLocales =
    kSupportedLanguageCodes.map(Locale.new).toList();

/// 取文案的快捷入口:`context.l10n.menuFile`。
///
/// 与项目里 `Tokens.of(context)` 的取色手感对齐,也省掉每个 widget 文件
/// 重复 import 生成物。语言随 MaterialApp 的 locale 变化整树重建,
/// 因此不需要额外订阅。
extension L10nContext on BuildContext {
  /// 当前语言的文案(缺 delegate 时由生成代码抛出,不会静默回退英文)
  AppLocalizations get l10n => AppLocalizations.of(this);
}

/// 解析实际生效的 locale。
///
/// [override] 是用户显式选定的语言码(null = 跟随系统)。跟随系统时按语言码粗匹配:
/// `zh*` → 简体中文,`ja*` → 日语,其余(含 `en*`、`fr*`、拿不到)→ 英语。
Locale resolveAppLocale({String? override, Locale? system}) {
  if (override != null && kSupportedLanguageCodes.contains(override)) {
    return Locale(override);
  }
  return switch (system?.languageCode) {
    'zh' => const Locale('zh'),
    'ja' => const Locale('ja'),
    _ => const Locale(kFallbackLanguageCode),
  };
}

/// 设计页标签标题的后缀 —— **内部身份令牌,不参与翻译**。
///
/// 标签身份一直是标题字符串(`AppState.activeTab` 与 `OpenTab.title` 比对),
/// 而「设计 / 新建」页要和同名数据页区分开,所以后缀拼进了标题。拼进去的就得
/// 是常量:让它随语言变,切一次语言活动标签便认不出自己。
/// 显示时由 `view_tabs.dart` 剥掉它、再按当前语言接上词条
/// (`tabDesignSuffix` / `tabNewSuffix`)。
const String kTabDesignTitleSuffix = ' (设计)';

/// 新建对象标签标题的后缀 —— 内部身份令牌(见 [kTabDesignTitleSuffix])。
const String kTabNewTitleSuffix = ' (新建)';

/// 命令列界面标签标题的后缀 —— 内部身份令牌(见 [kTabDesignTitleSuffix])。
/// 与上面两个不同,它不由 [splitTabTitle] 解析:命令列标签的标题整体是
/// `连接名|库名` 加此后缀,显示名由 `view_tabs` 按标签类型直接拼词条。
const String kTabCliTitleSuffix = ' (命令列界面)';

/// 拆开标签标题:返回真实对象名,以及它是否「新建未保存」。
/// 没有已知后缀时原样返回、`isNew` 为 false。
({String name, bool isNew}) splitTabTitle(String title) {
  for (final suffix in [kTabNewTitleSuffix, kTabDesignTitleSuffix]) {
    if (title.endsWith(suffix) && title.length > suffix.length) {
      return (
        name: title.substring(0, title.length - suffix.length),
        isNew: suffix == kTabNewTitleSuffix,
      );
    }
  }
  return (name: title, isNew: false);
}

/// 中文字体回退列表(桌面端):Windows 用微软雅黑,macOS 用苹方。
///
/// 取代此前依赖 chinese_font_library 提供的 SystemChineseFont.fontFamilyFallback,
/// 仅保留本项目实际运行平台所需的回退项。
const List<String> chineseFontFamilyFallback = [
  // Windows:Skia 按家族名匹配字体,规范英文名比本地化名「微软雅黑」更稳,
  // 后者在个别系统上可能没命中而悄悄退到默认 sans;两个都留,命中即用。
  'Microsoft YaHei',
  '微软雅黑',
  'PingFang SC', // macOS / iOS
];

/// 日语字体回退:Windows 用 Yu Gothic UI(退 Meiryo,两者 Win10+ 自带),
/// macOS 用 Hiragino Sans。
///
/// 日语界面下假名必须有字形,且汉字要取**日式字形**(「直」「骨」「関」等与中式
/// 写法有别),所以这套排在中文回退之前;排在后面会让假名以外的汉字全用中式字形。
const List<String> japaneseFontFamilyFallback = [
  'Yu Gothic UI',
  'Yu Gothic',
  'Meiryo',
  'Hiragino Sans',
];

/// 按语言取字体回退。
///
/// 日语表尾仍接中文回退:库名 / 表注释这类用户数据可能是中文,切到日语也得显示。
/// [languageCode] 为空(尚未解析出语言)时按中文回退。
List<String> fontFamilyFallbackFor(String? languageCode) =>
    languageCode == 'ja'
    ? [...japaneseFontFamilyFallback, ...chineseFontFamilyFallback]
    : chineseFontFamilyFallback;

/// 取当前上下文语言的字体回退,供手绘 `TextStyle` 的调用点使用。
List<String> fontFamilyFallbackOf(BuildContext context) =>
    fontFamilyFallbackFor(Localizations.localeOf(context).languageCode);
