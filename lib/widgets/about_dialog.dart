import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:url_launcher/url_launcher.dart';

import '../app/version.g.dart';
import '../theme/app_theme.dart';

/// 项目主页:同一仓库在 GitHub / Gitee 的双平台镜像,两个入口都摆在「关于」里。
const String kGithubHomeUrl = 'https://github.com/SpringHgui/daro';
const String kGiteeHomeUrl = 'https://gitee.com/SpringHgui/daro';

/// 问题反馈:走 GitHub Issues(与 top_menu「帮助 → 问题反馈」指向同一地址)。
const String kIssuesUrl = '$kGithubHomeUrl/issues';

/// 打开「帮助 → 关于…」弹窗。
Future<void> showDaroAboutDialog(BuildContext context) {
  return showDialog<void>(
    context: context,
    builder: (_) => const DaroAboutDialog(),
  );
}

/// 「关于」弹窗:启动画面式版面(居中 Logo + 名称 + 副标题 + 版本 + 版权),
/// 面板底色与文字均取当前主题中性色板,明暗双主题自适应。
///
/// 版本号取自 pubspec.yaml 生成的 [kAppVersion] / [kAppBuild],勿在此硬编码。
class DaroAboutDialog extends StatelessWidget {
  const DaroAboutDialog({super.key});

  static const double _kWidth = 400;

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final dt = t.desktopTokensFor(context);

    return DialogBox(
      title: '关于 daro',
      width: _kWidth,
      onClose: () => Navigator.of(context).maybePop(),
      child: Container(
        color: t.background,
        padding: const EdgeInsets.fromLTRB(28, 34, 28, 22),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _buildLogo(t),
            const SizedBox(height: 22),
            _buildTitle(dt, t),
            const SizedBox(height: 34),
            _buildLinks(context, dt, t),
            const SizedBox(height: 26),
            _buildFooter(dt, t),
          ],
        ),
      ),
    );
  }

  Widget _buildLogo(AppPalette t) {
    return Center(
      child: Container(
        width: 108,
        height: 108,
        decoration: BoxDecoration(
          color: t.muted,
          borderRadius: BorderRadius.circular(24),
          border: Border.all(color: t.border),
        ),
        padding: const EdgeInsets.all(16),
        child: SvgPicture.asset('assets/icons/app_logo.svg'),
      ),
    );
  }

  Widget _buildTitle(DesktopTokens dt, AppPalette t) {
    return Column(
      children: [
        Text(
          'daro',
          textAlign: TextAlign.center,
          style: TextStyle(
            fontFamily: dt.fontFamily,
            fontSize: 38,
            height: 1.1,
            fontWeight: FontWeight.w600,
            letterSpacing: 1.5,
            color: t.foreground,
            decoration: TextDecoration.none,
          ),
        ),
        const SizedBox(height: 4),
        Text(
          '桌面数据库管理工具',
          textAlign: TextAlign.center,
          style: TextStyle(
            fontFamily: dt.fontFamily,
            fontSize: 14,
            fontWeight: FontWeight.w400,
            letterSpacing: 3,
            color: t.mutedForeground,
            decoration: TextDecoration.none,
          ),
        ),
        const SizedBox(height: 14),
        Text(
          '版本 $kAppVersion（构建 $kAppBuild）',
          textAlign: TextAlign.center,
          style: TextStyle(
            fontFamily: dt.fontFamily,
            fontSize: dt.fontSize,
            fontWeight: FontWeight.w400,
            color: t.mutedForeground,
            decoration: TextDecoration.none,
          ),
        ),
      ],
    );
  }

  /// 项目主页(双平台)+ 问题反馈:平台按钮带各自的品牌图标。
  Widget _buildLinks(BuildContext context, DesktopTokens dt, AppPalette t) {
    // 品牌原色是按亮底挑的,暗色主题下对比度不够(GitHub 的黑几乎看不见),
    // 取每个平台备好的提亮档 —— 判据跟查询页 / 对象面板一致,跟主题色板走。
    final dark = t.isDark;
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        for (final p in _kPlatformLinks) ...[
          Button(
            // text 保留作无障碍标签,实际显示的是带图标的 child
            text: p.label,
            child: _platformContent(p, dark),
            tokens: dt,
            onPressed: () => _openUrl(context, p.url),
          ),
          const SizedBox(width: 10),
        ],
        Button(
          text: '问题反馈',
          tokens: dt,
          onPressed: () => _openUrl(context, kIssuesUrl),
        ),
      ],
    );
  }

  /// 平台按钮内容:品牌图标 + 平台名(字号与文字色由 Button 内部的
  /// DefaultTextStyle 下发,这里不覆盖,免得跟按钮三态配色脱节)。
  Widget _platformContent(_PlatformLink p, bool dark) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        SvgPicture.asset(
          p.icon,
          width: 16,
          height: 16,
          colorFilter: ColorFilter.mode(
            dark ? p.colorDark : p.colorLight,
            BlendMode.srcIn,
          ),
        ),
        const SizedBox(width: 7),
        Text(p.label),
      ],
    );
  }

  Widget _buildFooter(DesktopTokens dt, AppPalette t) {
    return Text(
      'Copyright © ${DateTime.now().year} daro. 保留所有权利.',
      textAlign: TextAlign.center,
      style: TextStyle(
        fontFamily: dt.fontFamily,
        fontSize: 12,
        fontWeight: FontWeight.w400,
        color: t.mutedForeground,
        decoration: TextDecoration.none,
      ),
    );
  }

  /// 用系统默认浏览器打开链接;失败时退化为弹窗展示 URL,避免点击无反馈。
  Future<void> _openUrl(BuildContext context, String url) async {
    var opened = false;
    try {
      opened = await launchUrl(Uri.parse(url),
          mode: LaunchMode.externalApplication);
    } catch (e) {
      debugPrint('[about_dialog] launchUrl failed: $e');
      opened = false;
    }
    if (opened || !context.mounted) return;
    MessageBox.show(
      context,
      title: '无法打开浏览器',
      message: '请在浏览器中访问:\n$url',
      buttons: MessageBoxButtons.ok,
      tokens: Tokens.read(context).desktopTokensFor(context),
    );
  }
}

/// 「项目主页」里的一个平台入口:品牌图标 + 平台名 + 目标地址。
///
/// 图标是单色矢量(`assets/icons/brands/*.svg`),源文件里填的是**亮色主题**下的
/// 品牌色;暗色主题按 [colorDark] 重新着色 —— GitHub 的品牌黑落在深底上直接
/// 消失,所以两套主题各备一档,由 `colorFilter` 覆盖源文件的 fill。
/// 出处与商标归属记录在同目录 `SOURCES.json`。
class _PlatformLink {
  const _PlatformLink({
    required this.label,
    required this.icon,
    required this.url,
    required this.colorLight,
    required this.colorDark,
  });

  /// 平台名,同时作为按钮文案。
  final String label;

  /// 品牌图标的资源路径。
  final String icon;

  /// 项目主页在该平台的地址。
  final String url;

  /// 亮色主题下的图标着色(品牌原色)。
  final Color colorLight;

  /// 暗色主题下的图标着色(把太暗的品牌色提亮一档)。
  final Color colorDark;
}

/// 项目主页的双平台入口,GitHub 在前(问题反馈也走它)。
const List<_PlatformLink> _kPlatformLinks = <_PlatformLink>[
  _PlatformLink(
    label: 'GitHub',
    icon: 'assets/icons/brands/github.svg',
    url: kGithubHomeUrl,
    colorLight: Color(0xFF24292F), // GitHub 品牌黑
    colorDark: Color(0xFFE6EDF3), // 其暗色主题下的前景色
  ),
  _PlatformLink(
    label: 'Gitee',
    icon: 'assets/icons/brands/gitee.svg',
    url: kGiteeHomeUrl,
    colorLight: Color(0xFFC71D23), // Gitee 品牌红
    colorDark: Color(0xFFE4636A), // 同色调提亮,保住"红"的辨识度
  ),
];
