import 'dart:io';

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/kv_browser_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

/// 实连回归:用真 Redis 验证**键浏览页**这条 UI 链路(而不是驱动抽象)。
///
/// 默认**不跑**:`DARO_LIVE_DB=1 flutter test test/kv_browser_page_live_test.dart`
/// 密码只从环境变量读(`DARO_REDIS_PWD`),仓库里不留凭据。
///
/// 与 `redis_kv_live_test.dart` 的分工:那边验 [KvStoreDriver] 的取数口径,
/// 这里验「页面把驱动调用起来了」—— glob 过滤真的下推到 MATCH、单击键行真的
/// 取值、TTL / 编码 / 字节列真的有数、200KB 大值真的按窗口续读。
/// 目标库是本地探针容器(`.redis_probe/seed*.sh` 种的数据)。
///
/// 两个实连专有的写法,少一条都会假失败:
/// 1. **整条用例包在 `tester.runAsync` 里**——页面在 `initState` 就发起 SCAN,
///    那条 socket 往返一旦建在 fake 时钟区域就永远等不到网络回调,页面会一直停在
///    「扫描中」;只有在 runAsync 内 `pumpWidget`,整条 future 链才落在真实区域。
/// 2. **等待以页面自己的在途标志为准**(见 [scanIdle]),不是「多给几百毫秒」——
///    一轮动作内部要跑几十次 SCAN 往返,墙钟窗口既慢又不稳。
void main() {
  final enabled = Platform.environment['DARO_LIVE_DB'] == '1';
  final pwd = Platform.environment['DARO_REDIS_PWD'] ?? '';
  final skip = enabled ? null : '未开启实连(DARO_LIVE_DB=1 时才跑)';

  if (skip != null) {
    // ignore: avoid_print
    print('实连测试跳过:$skip');
    test('实连:键浏览页(跳过)', () {}, skip: skip);
    return;
  }

  late AppState app;

  /// 让真实时钟走 [ms] 并推一帧:fake 时钟区域里 socket 回调不会被投递
  Future<void> tick(WidgetTester tester, [int ms = 50]) async {
    await Future<void>.delayed(Duration(milliseconds: ms));
    await tester.pump(Duration(milliseconds: ms));
  }

  /// 等页脚不再有「扫描中」= 键列表这一路的在途动作跑完。
  /// 上限只是兜底:一空闲就立刻返回,不会每次都等满。
  /// 先推一帧再判——`initState` 里发起扫描,在途标志要下一帧才渲染出来,
  /// 立刻判会读成「没在忙」,后面全成空转。
  Future<void> scanIdle(WidgetTester tester) async {
    await tick(tester);
    for (var i = 0;
        i < 200 && find.textContaining('扫描中').evaluate().isNotEmpty;
        i++) {
      await tick(tester);
    }
  }

  /// 等某个目标出现(值面板没有在途标志,只能盯目标本身)
  Future<void> untilFound(WidgetTester tester, Finder target) async {
    for (var i = 0; i < 200 && target.evaluate().isEmpty; i++) {
      await tick(tester);
    }
  }

  /// 开页 + 等首轮扫描落地
  Future<void> open(WidgetTester tester,
      {String namespaceId = '0', String namespaceLabel = 'db0'}) async {
    await tester.pumpWidget(
      ChangeNotifierProvider<AppState>.value(
        value: app,
        child: MaterialApp(
          locale: const Locale('zh'),
          localizationsDelegates: kAppLocalizationsDelegates,
          supportedLocales: kSupportedLocales,
          home: Material(
            child: SizedBox(
              width: 1100,
              height: 760,
              child: KvBrowserPage(
                  connection: 'Redis 探针',
                  namespaceId: namespaceId,
                  namespaceLabel: namespaceLabel),
            ),
          ),
        ),
      ),
    );
    await scanIdle(tester);
  }

  /// 在过滤框输入 glob 并回车重扫
  Future<void> filter(WidgetTester tester, String pattern) async {
    await tester.enterText(find.byType(Input), pattern);
    await tester.pump();
    await tester.testTextInput.receiveAction(TextInputAction.search);
    await scanIdle(tester);
  }

  /// 键名列在表格里;过滤输入框里留着同一段文本,`find.text` 会同时命中两者,
  /// 所以点击前必须把范围收到 DataGridView 之内(键网格是 SplitContainer 的第一段)
  Finder keyRow(String name) => find.descendant(
      of: find.byType(DataGridView).first, matching: find.text(name));

  /// 点一次「加载更多」按游标续扫;按钮已禁用(游标扫完)时返回 false
  Future<bool> loadMoreOnce(WidgetTester tester) async {
    final more = find.widgetWithText(Button, '加载更多');
    if (more.evaluate().isEmpty) return false;
    if (tester.widget<Button>(more).onPressed == null) return false;
    await tester.tap(more);
    await scanIdle(tester);
    return true;
  }

  /// 命中稀少的 glob:一轮动作只取到游标上第一段,必要时续点「加载更多」
  Future<void> untilKey(WidgetTester tester, String name) async {
    for (var i = 0; i < 12; i++) {
      await scanIdle(tester);
      if (keyRow(name).evaluate().isNotEmpty) return;
      if (!await loadMoreOnce(tester)) return;
    }
  }

  setUp(() async {
    // AppState 会异步落盘:指向临时目录
    final dir = await Directory.systemTemp.createTemp('daro_kv_page_live');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => dir.path,
    );
    app = AppState();
    app.addConnections([
      ConnectionInfo(
        name: 'Redis 探针',
        typeId: 'redis',
        host: '127.0.0.1',
        port: '6379',
        username: '',
        password: pwd,
        isLive: true,
      ),
    ]);
  });

  testWidgets('打开即连上服务端:未过滤也能扫出键', (tester) async {
    await tester.runAsync(() async {
      await open(tester);
      expect(find.textContaining('键列表读取失败'), findsNothing);
      // 探针 db0 有 2000+ 键:页脚报已加载条数,且首屏一定不是 0
      expect(find.textContaining('已加载'), findsOneWidget);
      expect(find.textContaining('已加载 0 个键'), findsNothing);
      expect(find.byType(DataGridView), findsOneWidget);
      // 一轮动作跑完就该停下,不能残留「扫描中」
      expect(find.textContaining('扫描中'), findsNothing);
    });
  });

  testWidgets('glob 过滤下推 MATCH,命中稀少时用「加载更多」续扫补齐',
      (tester) async {
    const hits = ['app:user:1001', 'app:config:version', 'app:session:s1'];
    bool gotAll() => hits.every((n) => keyRow(n).evaluate().isNotEmpty);

    await tester.runAsync(() async {
      await open(tester);
      await filter(tester, 'app:*');
      await untilKey(tester, hits.first);
      for (var i = 0; i < 12 && !gotAll(); i++) {
        if (!await loadMoreOnce(tester)) break;
      }
      expect(gotAll(), isTrue, reason: '游标续扫应凑齐 app:* 的 3 个键');

      // 只有 app:user:1001 带 TTL(seed 时设 7 天,跑用例时已走到 6 天多):
      // 按「N天 hh:mm:ss」的形状断言,不锁具体数字——用例跑得越晚数值越小。
      expect(
          find.descendant(
              of: find.byType(DataGridView).first,
              matching: find.textContaining(RegExp(r'^\d+天 \d\d:\d\d:\d\d$'))),
          findsOneWidget);
      // 另两个键永不过期,元数据列也不能空着
      expect(keyRow('不过期'), findsNWidgets(2));

      await tester.tap(keyRow('app:session:s1'));
      await untilFound(tester, find.text('字段'));
      expect(find.text('user'), findsWidgets);
      expect(find.text('10.0.0.1'), findsOneWidget);
      expect(find.textContaining('共 4 项'), findsOneWidget);
    });
  });

  testWidgets('非 UTF-8 值按 HEX 显示,不解成乱码也不炸', (tester) async {
    await tester.runAsync(() async {
      await open(tester);
      await filter(tester, 'bin:blob2');
      await untilKey(tester, 'bin:blob2');
      expect(keyRow('bin:blob2'), findsOneWidget);

      await tester.tap(keyRow('bin:blob2'));
      await untilFound(tester, find.text('HEX:00FF80C3281A04'));
      // seed4.sh 种的是 00 FF 80 C3 28 1A 04
      expect(find.text('HEX:00FF80C3281A04'), findsOneWidget);
    });
  });

  testWidgets('200KB 大值先给一段,「读取后续内容」按 GETRANGE 偏移续读',
      (tester) async {
    await tester.runAsync(() async {
      await open(tester);
      await filter(tester, 'big:huge');
      await untilKey(tester, 'big:huge');
      await tester.tap(keyRow('big:huge'));
      await untilFound(tester, find.textContaining('已显示前 65536'));

      // 预览上限 64KB:首屏只显示前段,不整段灌进内存
      expect(find.textContaining('已显示前 65536 / 200000 字节'), findsOneWidget);
      await tester.tap(find.text('读取后续内容'));
      await untilFound(tester, find.textContaining('已显示前 131072'));
      expect(find.textContaining('已显示前 131072 / 200000 字节'), findsOneWidget);
    });
  });

  testWidgets('切到空库:如实说没有,而不是报错', (tester) async {
    await tester.runAsync(() async {
      await open(tester, namespaceId: '1', namespaceLabel: 'db1');

      expect(find.textContaining('键列表读取失败'), findsNothing);
      // 全库扫完、一条没命中:页脚是「已加载 0 个键」,「已遍历完」只在有命中时才说
      expect(find.textContaining('已加载 0 个键'), findsOneWidget);
    });
  });
}
