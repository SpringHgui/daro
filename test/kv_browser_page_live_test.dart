import 'dart:io';

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/kv_driver.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/kv_browser_page.dart';
import 'package:flutter/gestures.dart';
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
  late ConnectionInfo connInfo;

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

  /// 等某个目标消失:用在提交就地编辑之后(见 [commitCell])
  Future<void> untilGone(WidgetTester tester, Finder target) async {
    for (var i = 0; i < 200 && target.evaluate().isNotEmpty; i++) {
      await tick(tester);
    }
  }

  /// 就地编辑:改文本 → Enter → **等编辑器关掉** → 等刷出来的新值。
  /// 只等「那段文本出现」不算提交完成:编辑框里本来就写着它,会在写还在途时
  /// 就返回,下一个动作因此打在过期布局上(实测:紧接着点另一格,刚出现的
  /// 「删除此项」被随后落地的重取清掉)。
  Future<void> commitCell(WidgetTester tester, String value) async {
    await untilFound(tester, find.byType(InlineEditor));
    await tester.enterText(find.byType(InlineEditor), value);
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await untilGone(tester, find.byType(InlineEditor));
    await untilFound(tester, find.text(value));
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

  /// 键名列在表格里;过滤输入框里留着同一段文本,所以范围收到 DataGridView 之内。
  /// **不要**加 `.first`:空库时页面还没有任何网格,`.first` 在 `evaluate` 里
  /// 直接抛 `Bad state: No element`,而不是回「没找到」。
  Finder keyRow(String name) => find.descendant(
      of: find.byType(DataGridView), matching: find.text(name));

  /// 触发一次续扫:列表滚得动就滚到底让页面自动续扫,滚不动就点兜底行。
  /// 返回 false = 没有游标可续了(页脚报「已遍历完」或「没有符合条件的键」)
  Future<bool> loadMoreOnce(WidgetTester tester) async {
    if (find.textContaining('已遍历完').evaluate().isNotEmpty ||
        find.text('没有符合条件的键').evaluate().isNotEmpty) {
      return false;
    }
    final fallback = find.textContaining('点此继续扫描').first;
    if (fallback.evaluate().isNotEmpty) {
      await tester.tap(fallback);
    } else {
      await tester.drag(
          find.byType(DataGridView).first, const Offset(0, -2000));
    }
    await scanIdle(tester);
    return true;
  }

  /// 命中稀少的 glob:一轮动作只取到游标上第一段,必要时接着续扫
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
    connInfo = ConnectionInfo(
      name: 'Redis 探针',
      typeId: 'redis',
      host: '127.0.0.1',
      port: '6379',
      username: '',
      password: pwd,
      isLive: true,
    );
    app.addConnections([connInfo]);
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

  testWidgets('glob 过滤下推 MATCH,命中稀少时靠续扫补齐',
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

  // ─────────────────────────────── 写链路实连(全部打在探针空库 db1)
  //
  // 分工:夹具与真值校验走**独立驱动**,页面只负责被点。让被测对象同时当裁判,
  // 「界面回已保存、库里一条没变」这类缺陷就永远测不出来。

  const probePrefix = 'daro:ui-live:';

  Future<KvStoreDriver> probeDriver() async {
    final d = createKvDriver(connInfo)!;
    await d.connect();
    return d;
  }

  Future<KvNamespace> probeDb1(KvStoreDriver d) async =>
      (await d.listNamespaces()).firstWhere((n) => n.id == '1');

  /// 把探针库里的 `daro:ui-live:*` 整批删掉。
  ///
  /// 只在 `runAsync` 内调用(用例首尾各一次):`addTearDown` 的回调跑在 fake
  /// 时钟区域,那条 socket 往返永远等不到回调,整条用例就挂死在那儿。
  Future<void> clearLiveKeys() async {
    final d = await probeDriver();
    final db1 = await probeDb1(d);
    final keys = <KvBytes>[];
    String? cursor;
    for (var i = 0; i < 50; i++) {
      final page = await d.scanKeys(KvKeyScan(
          namespace: db1,
          cursor: cursor,
          pattern: KvBytes.text('$probePrefix*'),
          count: 200));
      keys.addAll([for (final e in page.entries) e.key]);
      cursor = page.nextCursor;
      if (cursor == null) break;
    }
    if (keys.isNotEmpty) await d.deleteKeys(db1, keys);
    await d.close();
  }

  /// 弹窗里按字段标签定位输入框(`_row` 把冒号拼在标签上,所以匹配「值:」)
  Finder dialogInput(String label) => find.descendant(
        of: find.ancestor(
            of: find.descendant(
                of: find.byType(DialogBox), matching: find.text('$label:')),
            matching: find.byType(FieldRow)),
        matching: find.byType(Input),
      );

  Finder dialogButton(String text) => find.descendant(
      of: find.byType(DialogBox), matching: find.text(text));

  Finder menuitem(String text) => find.descendant(
      of: find.byType(ContextMenuPanel), matching: find.text(text));

  Future<void> rightClick(WidgetTester tester, Finder target) async {
    await tester.tap(target, buttons: kSecondaryMouseButton);
    await tick(tester);
  }

  /// 点确认框里的动作键。
  ///
  /// 两条实连专有的坑,都在这:
  /// 1. `MessageBox.show` 先插一层 entry(里面那个 `MessageBox` 的 build 只回
  ///    `SizedBox.shrink()`),`controller.open()` 之后 `ModalOverlay` 才插入
  ///    **第二层** entry 渲染面板 —— 一帧 pump 不够,而且面板**不是**
  ///    `find.byType(MessageBox)` 的后代,拿它当范围永远命中 0 个。
  /// 2. 面板弹出前那次点击可能被 `ContextMenuStrip` 的收手 barrier 再过一遍,
  ///    所以先推几帧再找按钮。
  Future<void> tapMessageBoxOk(WidgetTester tester, String action) async {
    final ok = find.widgetWithText(Button, action);
    for (var i = 0; i < 20 && ok.evaluate().isEmpty; i++) {
      await tick(tester);
    }
    expect(ok, findsOneWidget, reason: '确认框没弹出动作键「$action」');
    // 标题与动作键同词,所以必须按 Button 找;漏了 okText 会露出英文兜底
    expect(find.widgetWithText(Button, 'OK'), findsNothing,
        reason: '确认框按钮漏了本地化就会露出 base-ui 的英文兜底 OK / Cancel');
    await tester.tap(ok);
    await tick(tester);
  }

  /// 推 [n] 帧,每帧之间给真实时钟留 50ms。
  ///
  /// 浮层的进/出场都比「内容出现 / 消失」慢几帧:`tick(tester, 300)` 只是
  /// 一次 delay + **一**帧 pump,既走不到 `MessageBox.show` 插入的第二层 entry,
  /// 也等不到弹层的 ModalBarrier 消失 —— 这期间点到网格的那一下会被 barrier 吞掉。
  Future<void> frames(WidgetTester tester, [int n = 6]) async {
    for (var i = 0; i < n; i++) {
      await tick(tester);
    }
  }

  /// 关弹窗后等它真的收干净(表单弹窗 + 确认框两种载体都覆盖)
  Future<void> closeOverlays(WidgetTester tester) async {
    for (var i = 0;
        i < 20 &&
            (find.byType(DialogBox).evaluate().isNotEmpty ||
                find.widgetWithText(Button, '删除').evaluate().isNotEmpty);
        i++) {
      await tick(tester);
    }
    await frames(tester);
  }

  testWidgets('实连写链路 · 字符串:新建 → 改值 → 设过期 → 清除 → 删除',
      (tester) async {
    const name = '${probePrefix}str';
    late KvStoreDriver d;
    late KvNamespace db1;
    final probe = KvBytes.text(name);

    await tester.runAsync(() async {
      await clearLiveKeys();
      d = await probeDriver();
      db1 = await probeDb1(d);
      await open(tester, namespaceId: '1', namespaceLabel: 'db1');
      // 探针库此时应为空:用页脚计数确认,不用 `keyRow(...)` + `findsNothing`
      // —— 空网格上「descendant(of: DataGridView.first)」会让 findsNothing 的
      // 失配描述走到 `Finder.found`,直接抛「No results have been found yet」
      expect(find.textContaining('已加载 0 个键'), findsOneWidget);

      // 1) 新建键:表单收下后必须真的落到服务端
      await tester.tap(find.text('新建键').first);
      await untilFound(tester, find.byType(DialogBox));
      await tester.enterText(dialogInput('键名'), name);
      await tester.enterText(dialogInput('值'), 'hello');
      await tester.tap(dialogButton('保存'));
      await closeOverlays(tester);
      await untilFound(tester, keyRow(name));
      expect(await d.readKeyMeta(db1, [probe]), isNotEmpty);
      expect((await d.readValue(db1, probe)).rows.single.value.text, 'hello');

      // 2) 改值:单击值格就地编辑,提交后界面与服务端都得是新值
      await untilFound(tester, find.text('hello'));
      await tester.tap(find.text('hello'));
      await commitCell(tester, 'world');
      expect(find.textContaining('已保存'), findsWidgets);
      expect((await d.readValue(db1, probe)).rows.single.value.text, 'world');

      // 3) 设过期时间:键网格的 TTL 列必须从「不过期」变成倒计时
      expect(keyRow('不过期'), findsOneWidget);
      await rightClick(tester, keyRow(name));
      await tester.tap(menuitem('设置过期时间…'));
      await untilFound(tester, dialogInput('过期时间'));
      await tester.enterText(dialogInput('过期时间'), '300');
      // 确定键的可用性是 `_seconds != null`,而 `_seconds` 在 build 时算:
      // enterText 后必须推一帧,否则点到的还是上一帧的禁用态(tap 自己不重绘)
      await tick(tester);
      await tester.tap(dialogButton('确定'));
      await closeOverlays(tester);
      await untilFound(
          tester,
          find.descendant(
              of: find.byType(DataGridView).first,
              matching: find.textContaining(
                  RegExp(r'^\d+(天 )?\d\d:\d\d:\d\d$'))));
      expect((await d.readKeyMeta(db1, [probe])).single.hasTtl, isTrue);

      // 4) 改值不能把 TTL 洗掉(SET 必须带 KEEPTTL)
      await tester.tap(find.text('world'));
      await commitCell(tester, 'world2');
      expect((await d.readKeyMeta(db1, [probe])).single.hasTtl, isTrue,
          reason: '改值走 SET KEEPTTL:过期时间不能被清成永久');

      // 5) 清除过期时间
      await rightClick(tester, keyRow(name));
      await tester.tap(menuitem('清除过期时间'));
      await untilFound(tester, keyRow('不过期'));
      expect((await d.readKeyMeta(db1, [probe])).single.hasTtl, isFalse);

      // 6) 删除:确认框的按钮写的是动作本身(「删除」),不是 base-ui 的英文兜底
      await rightClick(tester, keyRow(name));
      await tester.tap(menuitem('删除键'));
      await tapMessageBoxOk(tester, '删除');
      await tick(tester, 200);
      expect(keyRow(name), findsNothing);
      expect(find.textContaining('已加载 0 个键'), findsOneWidget);
      expect((await d.readKeyMeta(db1, [probe])).single.type, KvKeyType.none);
      await d.close();
    });
  });

  testWidgets('实连写链路 · 哈希:加字段 / 改字段值 / 删字段都落库', (tester) async {
    const name = '${probePrefix}hash';
    final probe = KvBytes.text(name);

    await tester.runAsync(() async {
      await clearLiveKeys();
      final d = await probeDriver();
      final db1 = await probeDb1(d);
      await d.hashWrite(db1, probe, set: [
        (KvBytes.text('one'), KvBytes.text('1'))
      ]);

      await open(tester, namespaceId: '1', namespaceLabel: 'db1');
      await untilFound(tester, keyRow(name));
      await tester.tap(keyRow(name));
      await untilFound(tester, find.text('one'));

      // 加字段:「添加」开表单,字段名 + 值,保存即 HSET
      await tester.tap(find.text('添加'));
      await untilFound(tester, dialogInput('字段'));
      await tester.enterText(dialogInput('字段'), 'two');
      await tester.enterText(dialogInput('值'), '2');
      await tester.tap(dialogButton('保存'));
      await closeOverlays(tester);
      await untilFound(tester, find.text('two'));
      var read = await d.readValue(db1, probe);
      expect(read.rows.map((r) => r.field!.text).toList(), ['one', 'two']);

      // 改字段值:只动值列,字段名不能被写花
      await tester.tap(find.text('1'));
      await commitCell(tester, '11');
      read = await d.readValue(db1, probe);
      expect(read.rows.first.value.text, '11');
      expect(read.rows.first.field!.text, 'one');

      // 删字段:选中值行 → 工具栏「删除此项」→ 确认
      await tester.tap(find.text('two'));
      await frames(tester);
      await tester.tap(find.text('删除此项'));
      await tapMessageBoxOk(tester, '删除');
      await closeOverlays(tester);
      await untilFound(tester, find.textContaining('共 1 项'));
      read = await d.readValue(db1, probe);
      expect(read.rows.map((r) => r.field!.text).toList(), ['one']);
      await clearLiveKeys();
      await d.close();
    });
  });

  testWidgets('实连写链路 · 列表 / 集合 / 有序集:加一项、删一项各自落库',
      (tester) async {
    const listKey = '${probePrefix}list';
    const setKey = '${probePrefix}set';
    const zsetKey = '${probePrefix}zset';

    await tester.runAsync(() async {
      await clearLiveKeys();
      final d = await probeDriver();
      final db1 = await probeDb1(d);
      await d.listWrite(db1, KvBytes.text(listKey), append: KvBytes.text('a'));
      await d.setWrite(db1, KvBytes.text(setKey), add: [KvBytes.text('x')]);
      await d.zsetWrite(db1, KvBytes.text(zsetKey), add: [
        (KvBytes.text('m1'), 1)
      ]);

      await open(tester, namespaceId: '1', namespaceLabel: 'db1');
      await untilFound(tester, keyRow(listKey));

      // list:追加元素(RPUSH),再删掉刚加的那一行
      await tester.tap(keyRow(listKey));
      await untilFound(tester, find.text('a'));
      await tester.tap(find.text('添加'));
      await untilFound(tester, dialogInput('值'));
      await tester.enterText(dialogInput('值'), 'b');
      await tester.tap(dialogButton('保存'));
      await closeOverlays(tester);
      await untilFound(tester, find.text('b'));
      expect((await d.readValue(db1, KvBytes.text(listKey))).rows.length, 2);

      await tester.tap(find.text('b'));
      await frames(tester);
      await tester.tap(find.text('删除此项'));
      await tapMessageBoxOk(tester, '删除');
      await closeOverlays(tester);
      await untilFound(tester, find.textContaining('共 1 项'));
      expect((await d.readValue(db1, KvBytes.text(listKey))).rows.single.value.text,
          'a');

      // set:加成员再删成员
      await tester.tap(keyRow(setKey));
      await untilFound(tester, find.text('x'));
      await tester.tap(find.text('添加'));
      await untilFound(tester, dialogInput('值'));
      await tester.enterText(dialogInput('值'), 'y');
      await tester.tap(dialogButton('保存'));
      await closeOverlays(tester);
      await untilFound(tester, find.text('y'));
      expect((await d.readValue(db1, KvBytes.text(setKey))).totalCount, 2);

      // zset:加成员带分数,成员与分数都要真的在服务端
      await tester.tap(keyRow(zsetKey));
      await untilFound(tester, find.text('m1'));
      await tester.tap(find.text('添加'));
      await untilFound(tester, dialogInput('分数'));
      await tester.enterText(dialogInput('分数'), '2.5');
      await tester.enterText(dialogInput('值'), 'm2');
      await tester.tap(dialogButton('保存'));
      await closeOverlays(tester);
      await untilFound(tester, find.text('m2'));
      final z = await d.readValue(db1, KvBytes.text(zsetKey));
      expect(z.rows.map((r) => r.value.text).toList(), ['m1', 'm2']);
      expect(z.rows.last.score, 2.5);

      await clearLiveKeys();
      await d.close();
    });
  });
}
