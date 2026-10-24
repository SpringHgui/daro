import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter_test/flutter_test.dart';

/// 「导入连接」向导里两枚来源工具标识(assets/icons/tools/*.svg)的守护。
///
/// 这两块瓦片与引擎瓦片不同:厂商只发布位图标(Navicat 的官方件是 PNG lockup、
/// ARDM 是 512x512 应用图标),所以 `tool/compose_tool_icons.py` 把裁好的图形按
/// base64 内嵌进 SVG。**flutter_svg 解析不了 `<image>` 时不会报错,只是什么都不画**
/// —— 界面会退化成两块空白底板。因此这里不看资源名,直接数像素。
const _tools = {
  'navicat': 'assets/icons/tools/navicat.svg',
  'another-redis-desktop-manager':
      'assets/icons/tools/another-redis-desktop-manager.svg',
};

void main() {
  testWidgets('两块瓦片都在 pubspec 的 assets 清单里(漏声明时 rootBundle 取不到)',
      (tester) async {
    for (final asset in _tools.values) {
      final data = await rootBundle.load(asset);
      expect(data.lengthInBytes, greaterThan(1000), reason: asset);
    }
  });

  for (final entry in _tools.entries) {
    testWidgets('${entry.key}: 官方标真的画出来了(不是只剩底板)', (tester) async {
      final key = UniqueKey();
      // 位图解码是真异步:整段包进 runAsync,否则等不到解码完成
      await tester.runAsync(() async {
        await tester.pumpWidget(MaterialApp(
          home: Scaffold(
            body: Center(
              child: RepaintBoundary(
                key: key,
                child: SvgPicture.asset(entry.value, width: 128, height: 128),
              ),
            ),
          ),
        ));
        await Future<void>.delayed(const Duration(milliseconds: 500));
        await tester.pump();

        final image = await tester
            .renderObject<RenderRepaintBoundary>(find.byKey(key))
            .toImageSync(pixelRatio: 1);
        final bytes = (await image.toByteData(
                format: ui.ImageByteFormat.rawRgba))!
            .buffer
            .asUint8List();
        // 底板是白→浅蓝灰(高亮度、低饱和),官方标各自有强色:
        // Navicat 的金色与 ARDM 的红。红蓝差超过 60 的像素只可能来自标本身。
        final saturated = _saturatedPixels(bytes);
        expect(saturated, greaterThan(200),
            reason: '${entry.key} 只剩圆角底板:内嵌位图没有被绘制');
      });
    });
  }
}

int _saturatedPixels(Uint8List bytes) {
  var n = 0;
  for (var i = 0; i < bytes.length; i += 4) {
    final r = bytes[i], b = bytes[i + 2], a = bytes[i + 3];
    if (a > 200 && (r - b).abs() > 60) n++;
  }
  return n;
}
