/// SQL / DDL 语法高亮的共享层:配色表 + 只读文本的同步 TextSpan 渲染。
///
/// 三条约定:
/// - 配色是中调色、与 [AppPalette] 无关的固定值(明亮近似 SSMS / VS Code Light+,
///   暗黑近似 VS Code Dark+),因此**不进主题定制弹窗**,也不走 token 取色;
///   以前它在三个文件里各抄了一份(query_page / table_designer_page /
///   routine_design_page),改一处漏两处,现在只有这里一份。
/// - **只读**展示位(详情面板 DDL 页、建库 / 修改库的脚本预览、结构同步脚本)
///   用 [highlightedSqlSpan] + `SelectableText.rich`:同步出帧,首帧就是彩色,
///   不存在「先裸文本再等 isolate 回帧」的慢半拍。可编辑处仍走 re_editor 的
///   `CodeEditor`(它把解析放 isolate,但带行号槽 / 查找栏 / 编辑器外壳,
///   塞进窄面板过重),两边共用本文件的配色表。
/// - 解析失败、或解析结果**掉了字符**,一律退回裸文本:高亮是锦上添花,
///   不能反过来把 DDL 弄没了一截。
library;

import 'package:flutter/painting.dart';
import 'package:re_highlight/languages/sql.dart';
import 'package:re_highlight/re_highlight.dart';

import 'app_theme.dart';

/// 明亮主题配色(键 = re_highlight 的 token scope)
const Map<String, TextStyle> sqlLightTheme = {
  'keyword': TextStyle(color: Color(0xff0000ff)),
  'literal': TextStyle(color: Color(0xff001080)),
  'name': TextStyle(color: Color(0xff001080)),
  'attr': TextStyle(color: Color(0xff001080)),
  'variable': TextStyle(color: Color(0xff001080)),
  'meta': TextStyle(color: Color(0xff001080)),
  'string': TextStyle(color: Color(0xffa31515)),
  'quote': TextStyle(color: Color(0xffa31515)),
  'regexp': TextStyle(color: Color(0xffa31515)),
  'comment': TextStyle(color: Color(0xff008000), fontStyle: FontStyle.italic),
  'doctag': TextStyle(color: Color(0xff800000)),
  'section': TextStyle(color: Color(0xff800000)),
  'number': TextStyle(color: Color(0xff098658)),
  'symbol': TextStyle(color: Color(0xff098658)),
  'type': TextStyle(color: Color(0xff267f99)),
  'class-title': TextStyle(color: Color(0xff267f99)),
  'built_in': TextStyle(color: Color(0xff795e26)),
  'title': TextStyle(color: Color(0xff795e26)),
};

/// 暗黑主题配色
const Map<String, TextStyle> sqlDarkTheme = {
  'keyword': TextStyle(color: Color(0xff569cd6)),
  'literal': TextStyle(color: Color(0xff569cd6)),
  'name': TextStyle(color: Color(0xff9cdcfe)),
  'attr': TextStyle(color: Color(0xff9cdcfe)),
  'variable': TextStyle(color: Color(0xff9cdcfe)),
  'meta': TextStyle(color: Color(0xff9cdcfe)),
  'string': TextStyle(color: Color(0xffce9178)),
  'quote': TextStyle(color: Color(0xffce9178)),
  'regexp': TextStyle(color: Color(0xffd16969)),
  'comment': TextStyle(color: Color(0xff6a9955), fontStyle: FontStyle.italic),
  'doctag': TextStyle(color: Color(0xff6a9955)),
  'section': TextStyle(color: Color(0xff6a9955)),
  'number': TextStyle(color: Color(0xffb5cea8)),
  'symbol': TextStyle(color: Color(0xffb5cea8)),
  'type': TextStyle(color: Color(0xff4ec9b0)),
  'class-title': TextStyle(color: Color(0xff4ec9b0)),
  'built_in': TextStyle(color: Color(0xffdcdcaa)),
  'title': TextStyle(color: Color(0xffdcdcaa)),
};

/// 交给 `CodeHighlightTheme.theme` 的完整配色:明暗取表 + 覆盖 `root`。
///
/// `root` 是 re_editor 给「没命中任何 scope 的裸文本」查的键,不覆盖就会掉回
/// 它自带的默认色(亮色主题下是深色,反过来也一样),换肤时文字跟着变黑 / 变白。
Map<String, TextStyle> sqlEditorTheme({
  required bool isDark,
  required Color foreground,
}) =>
    {
      ...(isDark ? sqlDarkTheme : sqlLightTheme),
      'root': TextStyle(color: foreground),
    };

/// 只读代码文本的统一字体:Consolas 优先、回落 monospace
/// (与既有脚本预览一致;可编辑的 CodeEditor 用的是 'monospace',不在此列)。
///
/// [height] 传 null = 沿用 Flutter 默认行距(各展示位原有的行高不同,
/// 这里不替它们统一,免得顺手改排版)。
///
/// `decoration: none` 是硬性要求——高亮出来的子 span 只带 color,
/// 一旦哪天脱离 Material 的 DefaultTextStyle 兜底,就会冒出双黄线 / 加粗。
TextStyle sqlCodeStyle({
  required Color color,
  double fontSize = 12,
  double? height = 1.45,
}) =>
    TextStyle(
      fontFamily: 'Consolas',
      fontFamilyFallback: const ['monospace'],
      fontSize: fontSize,
      height: height,
      color: color,
      decoration: TextDecoration.none,
    );

Highlight? _highlighter;

/// 复用同一个 [Highlight]:langSql 整套语法在首次 `highlight()` 时才编译
/// (`_compileLanguage` 把结果缓存在 Mode 上),每次新建实例等于白付一遍编译。
Highlight get _sqlHighlighter =>
    _highlighter ??= (Highlight()..registerLanguage('sql', langSql));

/// 单条 memo:详情面板 / 脚本预览在 hover、换肤等重建里会反复传同一段文本,
/// 解析虽然只有毫秒级,但没必要每次 build 都重跑。换文本即失效。
({String code, bool isDark, TextStyle base})? _lastRequest;
TextSpan? _lastSpan;

/// 把一段 SQL / DDL 解析成带高亮的 [TextSpan],供 `Text.rich` /
/// `SelectableText.rich` 直接渲染。
///
/// 字体与明暗都从 [palette] 推导:明暗取 [AppPalette.isDark],不走 `Theme.of`
/// —— 定制色板下两者可能不一致。字号 / 行高各展示位不同,故仍留参数。
///
/// 返回的是**根 span**,兜底字体([sqlCodeStyle])挂在它身上,命中的 scope 子 span
/// 只带自己的颜色与斜体、字号与字体沿用它——所以调用方直接 `Text.rich(span)` 即可,
/// **不要再传 `style:`**(传了会被这里的根样式盖掉,反而看不出问题)。
/// 解析异常或语法不认识时返回纯文本 span,不抛。
TextSpan highlightedSqlSpan(
  String code, {
  required AppPalette palette,
  double fontSize = 12,
  double? height = 1.45,
}) {
  final isDark = palette.isDark;
  final base = sqlCodeStyle(
    color: palette.foreground,
    fontSize: fontSize,
    height: height,
  );
  if (code.isEmpty) return TextSpan(text: code, style: base);
  final request = (code: code, isDark: isDark, base: base);
  final cached = _lastSpan;
  if (cached != null && request == _lastRequest) return cached;

  TextSpan span;
  try {
    final renderer = TextSpanRenderer(
      base,
      isDark ? sqlDarkTheme : sqlLightTheme,
    );
    _sqlHighlighter.highlight(code: code, language: 'sql').render(renderer);
    final inner = renderer.span;
    span = inner == null
        ? TextSpan(text: code, style: base)
        : TextSpan(style: base, children: [inner]);
  } catch (_) {
    span = TextSpan(text: code, style: base);
  }
  // 自校验:scope 切分一旦掉了字符(撞上奇怪的字节序列时真会发生),
  // 宁可整段退回裸文本,也不能让界面上的 DDL 少一截——少的那截没人看得见。
  if (span.toPlainText() != code) span = TextSpan(text: code, style: base);
  _lastRequest = request;
  _lastSpan = span;
  return span;
}
