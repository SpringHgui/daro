// Release 正文下载表格守卫:tool/gen_release_notes.py 的 DOWNLOAD_PLATFORMS 里
// 的产物名是「按约定拼」出 asset 下载链接的(不是从 artifacts 列出来的)。
// 打包脚本一旦改名而这里没同步,发出去的 Release 就是一串 404 链接——
// release.yml 的「校验正文里的下载链接」步骤虽然也能拦住,但那已经是打 tag 之后了。
// 本用例把「声明的文件名」与「打包脚本里真正写出的文件名」钉在一起,提交前就报错。
// 运行:flutter test test/release_notes_assets_test.dart
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

/// 声明的 asset 名(去掉 `daro-{ver}-` 前缀)→ (真正写出该名字的文件, 必须出现的关键片段)。
/// 每个片段都要能说明「这个名字是从哪来的」,而不是随手抄一遍全文。
const Map<String, (String, List<String>)> _sources = {
  'windows-x64.exe': (
    'installer/windows/installer.iss',
    ['OutputBaseFilename={#MyAppName}-{#MyAppVersion}-windows-{#ArchTarget}'],
  ),
  'windows-x64-portable.zip': (
    'installer/windows/build_installer.bat',
    ['daro-%APP_VERSION%-windows-%ARCH%', '-portable.zip'],
  ),
  'macos-universal.dmg': (
    'installer/macos/make_dmg.sh',
    ['daro-\${VER}-macos-universal.dmg'],
  ),
  'macos-universal.zip': (
    'installer/macos/make_dmg.sh',
    ['daro-\${VER}-macos-universal.zip'],
  ),
  'linux-amd64.deb': (
    'installer/linux/package_linux.sh',
    ['daro-\${VER}-linux-\${ARCH_DEB}.deb', 'ARCH_DEB="amd64"'],
  ),
  'linux-x64.tar.xz': (
    'installer/linux/package_linux.sh',
    ['daro-\${VER}-linux-\${ARCH_FLUTTER}.tar.xz', 'ARCH_FLUTTER="x64"'],
  ),
};

const _prefix = 'daro-{ver}-';

/// 平台名 → 该平台的打包脚本目录(供跨平台一致性检查)。
const Map<String, String> _platformScriptDirs = {
  'windows': 'installer/windows',
  'macos': 'installer/macos',
  'linux': 'installer/linux',
};

void main() {
  final py = File('tool/gen_release_notes.py').readAsStringSync();
  final yml = File('.github/workflows/release.yml').readAsStringSync();

  // 抓 DOWNLOAD_PLATFORMS 里 ("EXE", "daro-{ver}-windows-x64.exe") 这种二元组。
  final declared = RegExp(r'\("([A-Z][A-Z.]*)",\s*"(' +
          RegExp.escape(_prefix) +
          r'[^"]+)"\)')
      .allMatches(py)
      .map((m) => m.group(2)!)
      .toList()..sort();

  /// `daro-{ver}-windows-x64.exe` → `windows-x64.exe`
  String tail(String name) => name.substring(_prefix.length);

  test('脚本里确实声明了下载产物', () {
    expect(declared, isNotEmpty,
        reason: '没在 tool/gen_release_notes.py 里找到 DOWNLOAD_PLATFORMS 的产物声明');
    for (final name in declared) {
      expect(name, startsWith(_prefix), reason: '下载表格里的产物名必须带 $_prefix 前缀');
    }
  });

  test('声明的每个产物名都能在打包脚本里找到出处', () {
    final unknown = declared.where((n) => !_sources.containsKey(tail(n)));
    expect(unknown, isEmpty,
        reason: '新增/改名了下载产物,请同步更新本用例的 _sources:${unknown.join(', ')}');
  });

  test('本用例覆盖的产物一个都没漏(声明与期望集合一致)', () {
    final declaredTails = declared.map(tail).toSet();
    final expectedTails = _sources.keys.toSet();
    final extra = declaredTails.difference(expectedTails);
    expect(extra, isEmpty, reason: '声明了但本用例没有对应出处检查:${extra.join(', ')}');
    expect(expectedTails.difference(declaredTails), isEmpty,
        reason: '链接表格里少了产物:${expectedTails.difference(declaredTails).join(', ')}');
  });

  test('打包脚本里确实会写出这些文件名', () {
    final offenders = <String>[];
    _sources.forEach((name, spec) {
      final (path, needles) = spec;
      final file = File(path);
      if (!file.existsSync()) {
        offenders.add('$path 不存在');
        return;
      }
      final text = file.readAsStringSync();
      for (final needle in needles) {
        if (!text.contains(needle)) {
          offenders.add('$path 里找不到 `$needle`(对应 $name)');
        }
      }
    });
    expect(offenders, isEmpty, reason: offenders.join('\n'));
  });

  test('每个平台都是「安装包 + 便携版」两档,且平台目录存在', () {
    final byPlatform = <String, List<String>>{};
    for (final name in declared) {
      final platform = tail(name).split('-').first;
      byPlatform.putIfAbsent(platform, () => []).add(name);
    }
    expect(byPlatform.keys.toSet(), _platformScriptDirs.keys.toSet(),
        reason: '下载表格的平台列与 installer/ 下的平台目录对不上');
    byPlatform.forEach((platform, names) {
      expect(names.length, 2,
          reason: '$platform 应恰好有「安装包 + 便携版」两个产物,实际:${names.join(', ')}');
      expect(Directory(_platformScriptDirs[platform]!).existsSync(), isTrue,
          reason: '缺目录 ${_platformScriptDirs[platform]}');
    });
  });

  test('release job 会拿真实 artifacts 复核正文里的下载链接', () {
    // 正文链接是拼出来的,必须有一道拿实际产物比对的关卡,否则改名会静默发出 404。
    expect(yml, contains('校验正文里的下载链接'),
        reason: 'release.yml 里缺少产物一致性校验步骤');
    expect(yml, contains('pattern: daro-*'),
        reason: 'release job 需以 daro-* 通配把各平台产物汇总到 artifacts/ 再上传');
  });

  test('下载表格补全了源码包 / 校验和 / MSIX 说明这三块', () {
    // 红框里的完整下载区 = 三平台两档 + 源码归档 + 完整 SHA256 + 商店说明。
    expect(py, contains('SOURCE_ARCHIVES'),
        reason: 'gen_release_notes.py 缺少 SOURCE_ARCHIVES(源码归档)声明');
    expect(py, contains('### 🔐 校验和 (SHA256)'),
        reason: 'render_downloads 缺少完整 SHA256 校验和表');
    expect(py, contains('MSIX_NOTE'),
        reason: '缺少 Microsoft Store / MSIX 说明常量');
    // 源码用 archive/refs/tags 链接(永远可用、不进 artifacts、不参与 releases/download 校验)
    expect(py, contains('archive/refs/tags/'),
        reason: '源码归档链接应走 archive/refs/tags/ 而非 releases/download/');
  });

  test('release.yml 先下载产物、再生成正文(否则算不出大小/SHA256)', () {
    final downloadStep = yml.indexOf('下载全部平台的产物');
    final genStep = yml.indexOf('生成 Release 正文');
    expect(downloadStep, greaterThanOrEqualTo(0), reason: '找不到下载产物步骤');
    expect(genStep, greaterThanOrEqualTo(0), reason: '找不到生成正文步骤');
    expect(downloadStep, lessThan(genStep),
        reason: '必须先下载 artifacts,生成正文时才能读大小与 SHA256');
    expect(yml, contains('--artifacts-dir artifacts'),
        reason: '生成正文步骤需传 --artifacts-dir artifacts');
  });
}
