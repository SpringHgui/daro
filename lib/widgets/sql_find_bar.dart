import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:re_editor/re_editor.dart';

import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';

/// SQL 编辑器的快捷键档:把 re_editor 默认的替换键 **Ctrl+Alt+F 改绑成 Ctrl+H**
/// (与其它桌面编辑器 / Navicat 一致),查找仍是自带的 Ctrl+F。
/// 其余键一律沿用平台默认,不做覆盖。
class SqlFindShortcuts extends DefaultCodeShortcutsActivatorsBuilder {
  const SqlFindShortcuts();

  @override
  List<ShortcutActivator>? build(CodeShortcutType type) {
    if (type == CodeShortcutType.replace) {
      return const [SingleActivator(LogicalKeyboardKey.keyH, control: true)];
    }
    return super.build(type);
  }
}

/// SQL 编辑器的查找 / 替换栏,交给 `CodeEditor.findBuilder` 渲染。
///
/// 高度固定一档([height]):`CodeEditor` 只用 `preferredSize.height` 给编辑区
/// 让出顶部空间,而它不会因 `replaceMode` 翻转而重新询问,所以两种模式必须同高。
/// 未进入查找模式(`controller.value == null`)时高度为 0,编辑器不留空白。
class SqlFindBar extends StatefulWidget implements PreferredSizeWidget {
  const SqlFindBar({
    super.key,
    required this.controller,
    this.readOnly = false,
    this.onClosed,
  });

  final CodeFindController controller;

  /// 只读编辑器不给替换入口(仍给查找)。
  final bool readOnly;

  /// 关闭后把焦点还给编辑器,免得接着打字要点一下。
  final VoidCallback? onClosed;

  static const double height = 30;

  @override
  Size get preferredSize =>
      controller.value == null ? Size.zero : const Size.fromHeight(height);

  @override
  State<SqlFindBar> createState() => _SqlFindBarState();
}

class _SqlFindBarState extends State<SqlFindBar> {
  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_onChanged);
  }

  @override
  void didUpdateWidget(SqlFindBar oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_onChanged);
      widget.controller.addListener(_onChanged);
    }
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onChanged);
    super.dispose();
  }

  void _onChanged() {
    if (mounted) setState(() {});
  }

  void _close() {
    widget.controller.close();
    widget.onClosed?.call();
  }

  /// 编辑器外的浮层不在 `CodeEditor` 的 Shortcuts 子树里,Esc 得自己接。
  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent || event.logicalKey != LogicalKeyboardKey.escape) {
      return KeyEventResult.ignored;
    }
    _close();
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    final value = widget.controller.value;
    if (value == null) return const SizedBox.shrink();
    final t = Tokens.of(context);
    final dt = t.desktopTokensFor(context);
    final l = context.l10n;
    final canPrevNext = value.result?.matches.isNotEmpty == true;
    final replaceMode = value.replaceMode && !widget.readOnly;
    final glyph = TextStyle(
      fontFamily: dt.fontFamily,
      fontSize: 11.5,
      fontWeight: FontWeight.w600,
      // 本栏可能被挂在 Material 作用域之外,兜底样式会把文字画上下划线/加粗
      decoration: TextDecoration.none,
      color: t.foreground,
    );

    return Focus(
      canRequestFocus: false,
      onKeyEvent: _onKey,
      child: Container(
        height: SqlFindBar.height,
        color: t.secondary,
        padding: const EdgeInsets.symmetric(horizontal: 6),
        child: Row(
          children: [
            Flexible(
              fit: FlexFit.loose,
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 190),
                child: Input(
                  controller: widget.controller.findInputController,
                  focusNode: widget.controller.findInputFocusNode,
                  hint: l.findHint,
                  height: 22,
                  textInputAction: TextInputAction.search,
                  onSubmitted: (_) => widget.controller.nextMatch(),
                  tokens: dt,
                ),
              ),
            ),
            const SizedBox(width: 6),
            SizedBox(
              width: 76,
              child: Text(
                _counterText(l, value),
                style: TextStyle(
                  fontFamily: dt.fontFamily,
                  fontSize: 11.5,
                  color: value.result == null && !value.searching
                      ? t.disabledForeground
                      : t.mutedForeground,
                  decoration: TextDecoration.none,
                ),
              ),
            ),
            IconBtn(
              icon: Icons.arrow_upward,
              iconSize: 14,
              size: const Size(22, 22),
              tooltip: l.findPrev,
              color: t.foreground,
              tokens: dt,
              onTap: canPrevNext ? widget.controller.previousMatch : null,
            ),
            IconBtn(
              icon: Icons.arrow_downward,
              iconSize: 14,
              size: const Size(22, 22),
              tooltip: l.findNext,
              color: t.foreground,
              tokens: dt,
              onTap: canPrevNext ? widget.controller.nextMatch : null,
            ),
            const SizedBox(width: 4),
            IconBtn(
              child: Text('Aa', style: glyph),
              size: const Size(24, 22),
              tooltip: l.findCaseSensitive,
              color: t.foreground,
              selected: value.option.caseSensitive,
              selectedColor: t.accent,
              tokens: dt,
              onTap: widget.controller.toggleCaseSensitive,
            ),
            IconBtn(
              child: Text('.*', style: glyph),
              size: const Size(26, 22),
              tooltip: l.findRegex,
              color: t.foreground,
              selected: value.option.regex,
              selectedColor: t.accent,
              tokens: dt,
              onTap: widget.controller.toggleRegex,
            ),
            if (replaceMode) ...[
              const SizedBox(width: 4),
              Flexible(
                fit: FlexFit.loose,
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 170),
                  child: Input(
                    controller: widget.controller.replaceInputController,
                    focusNode: widget.controller.replaceInputFocusNode,
                    hint: l.findReplaceHint,
                    height: 22,
                    onSubmitted: (_) => widget.controller.replaceMatch(),
                    tokens: dt,
                  ),
                ),
              ),
              const SizedBox(width: 4),
              ToolbarButton(
                text: l.findBtnReplace,
                height: 22,
                paddingX: 6,
                enabled: canPrevNext,
                tokens: dt,
                onTap: widget.controller.replaceMatch,
              ),
              ToolbarButton(
                text: l.findBtnReplaceAll,
                height: 22,
                paddingX: 6,
                enabled: canPrevNext,
                tokens: dt,
                onTap: widget.controller.replaceAllMatches,
              ),
            ],
            const Spacer(),
            IconBtn(
              icon: Icons.close,
              iconSize: 14,
              size: const Size(22, 22),
              tooltip: l.findClose,
              color: t.foreground,
              tokens: dt,
              onTap: _close,
            ),
          ],
        ),
      ),
    );
  }

  /// 命中计数:正则写错时 `result` 为 null 但 `searching` 已 false,与「没有匹配」
  /// 同样只能显示 0 匹配 —— 编辑器不返回更细的原因。
  String _counterText(AppLocalizations l, CodeFindValue value) {
    if (value.searching) return l.findSearching;
    final matches = value.result?.matches;
    if (matches == null || matches.isEmpty) return l.findNoMatches;
    final index = value.result!.index;
    if (index < 0) return '${matches.length}';
    return l.findMatchPosition('${index + 1}', '${matches.length}');
  }
}
