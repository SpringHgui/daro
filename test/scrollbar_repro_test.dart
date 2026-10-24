import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// 表数据页 / 结果网格的滚动条结构守护:纵向条挂在横向滚动区**外面**时,
/// Material Scrollbar 默认的 depth==0 通知过滤会拒收内层纵向 ListView 的
/// 通知(depth 1),纵向条整条不绘制(即「红框位置没有滚动条」的根因)。
/// 修复 = ScrollBar.notificationPredicate 按轴过滤,见 base-ui scroll_bar.dart。
void main() {
  testWidgets('外层纵向 ScrollBar 按轴过滤通知后应渲染滑块', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    final v = ScrollController();
    final h = ScrollController();
    addTearDown(v.dispose);
    addTearDown(h.dispose);

    await tester.pumpWidget(
      MaterialApp(
        home: Center(
          child: SizedBox(
            width: 600,
            height: 300,
            child: Builder(
              builder: (context) => ScrollConfiguration(
                behavior:
                    ScrollConfiguration.of(context).copyWith(scrollbars: false),
                child: ScrollBar(
                  controller: v,
                  thumbVisibility: true,
                  notificationPredicate: (n) => n.metrics.axis == Axis.vertical,
                  child: ScrollBar(
                    controller: h,
                    orientation: ScrollBarOrientation.horizontal,
                    thumbVisibility: true,
                    child: SingleChildScrollView(
                      controller: h,
                      scrollDirection: Axis.horizontal,
                      child: SizedBox(
                        width: 1200,
                        child: DataGridView(
                          columns: const [
                            DataGridViewColumn(title: 'a', width: 600),
                            DataGridViewColumn(title: 'b', width: 600),
                          ],
                          rowCount: 100,
                          rowHeight: 24,
                          verticalScrollController: v,
                          cellBuilder: (r, c) => Text('$r-$c'),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    // 纵向条(600 宽宿主的右缘)必须画出轨道 / 滑块的窄矩形
    final verticalBar = find.byWidgetPredicate((w) =>
        w is CustomPaint &&
        w.foregroundPainter is ScrollbarPainter &&
        (w.foregroundPainter! as ScrollbarPainter).thickness <= 8);
    expect(verticalBar, findsNWidgets(2)); // 纵 + 横两条
    expect(
      verticalBar.first,
      paints
        ..something((method, args) {
          if (method != #drawRect) return false;
          final r = args.first as Rect;
          // Windows 主题下轨道 = 静止 thickness + 两侧 crossAxisMargin(2),
          // 轨道右缘 600、滑块右缘 598,均落在视口右缘窄带内
          return r.right >= 597.0 && r.width <= 12.0;
        }),
      reason: '纵向滚动条应贴在视口右缘绘制',
    );
    debugDefaultTargetPlatformOverride = null;
  });
}
