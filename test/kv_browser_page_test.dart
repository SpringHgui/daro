import 'dart:io';

import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/kv_driver.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/kv_browser_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

// Redis 键浏览页:游标扫描的分页语义、空页不等于无结果、扫描轮数上限,
// 以及「单击键行即取值」与「大字符串续读」。
// 全程挂假驱动,不碰真实 Redis。

const _conn = ConnectionInfo(
  name: 'c',
  typeId: 'redis',
  host: '127.0.0.1',
  port: '6379',
  username: '',
  isLive: true,
);

/// 脚本化假驱动:[pages] 按顺序回放 SCAN 结果,取完就是「已扫完的空页」。
class FakeKvDriver implements KvStoreDriver {
  FakeKvDriver({
    required this.pages,
    this.meta = const {},
    this.values = const {},
    this.fullText = const {},
  });

  final List<KvKeyPage> pages;
  final Map<String, KvKeyMeta> meta;
  final Map<String, KvValueRead> values;

  /// 大值的完整原文(只在按偏移续读时用):[values] 里放的是首段预览
  final Map<String, String> fullText;

  int scanCalls = 0;
  final List<int> readOffsets = [];

  /// 回放位置:SCAN 的游标由测试脚本决定,页面只管往后翻
  int _cursorIndex = 0;

  @override
  KvCapabilities get capabilities => const KvCapabilities(
        multipleNamespaces: true,
        cursorScan: true,
        keyTypes: {KvKeyType.string, KvKeyType.hash},
        ttl: true,
        internalEncoding: true,
        memoryUsage: true,
        valueOffsetRead: true,
        idleTime: false,
        commandConsole: false,
        readOnlyFirstVersion: true,
        maxValuePreviewBytes: 64 * 1024,
      );

  @override
  bool get isConnected => true;

  @override
  Future<void> connect() async {}

  @override
  Future<void> close() async {}

  @override
  Future<List<KvNamespace>> listNamespaces() async =>
      const [KvNamespace(id: '0', label: 'db0', keyCount: 3)];

  @override
  Future<int?> keyCount(KvNamespace namespace) async => 3;

  @override
  Future<KvKeyPage> scanKeys(KvKeyScan request) async {
    scanCalls++;
    if (_cursorIndex >= pages.length) {
      return const KvKeyPage(entries: [], nextCursor: null, totalEstimate: 3);
    }
    return pages[_cursorIndex++];
  }

  @override
  Future<List<KvKeyMeta>> readKeyMeta(
          KvNamespace namespace, List<KvBytes> keys) async =>
      [for (final k in keys) meta[k.display] ?? const KvKeyMeta(type: KvKeyType.string)];

  @override
  Future<KvValueRead> readValue(KvNamespace namespace, KvBytes key,
      {int offset = 0, int? limit}) async {
    readOffsets.add(offset);
    final read = values[key.display];
    if (read == null) {
      throw KvServerException('WRONGTYPE Operation against a key');
    }
    // 续读:按 offset 截掉已展示的前段,模拟 GETRANGE 窗口
    if (read.type == KvKeyType.string && offset > 0) {
      final text = fullText[key.display] ?? read.rows.first.value.text ?? '';
      final rest = offset >= text.length ? '' : text.substring(offset);
      return KvValueRead(
        key: read.key,
        type: read.type,
        rows: rest.isEmpty
            ? const []
            : [KvValueRow(value: KvBytes.text(rest))],
        totalLength: read.totalLength,
        truncated: false,
        rangeStart: offset,
      );
    }
    return read;
  }

  @override
  Future<KvReply> runCommand(KvNamespace namespace, List<String> arguments) async =>
      KvReply.simple('OK');

  @override
  Future<KvServerInfo?> readServerInfo() async => null;
}

Future<FakeKvDriver> _pumpPage(
  WidgetTester tester, {
  required List<KvKeyPage> pages,
  Map<String, KvKeyMeta> meta = const {},
  Map<String, KvValueRead> values = const {},
  Map<String, String> fullText = const {},
}) async {
  final driver =
      FakeKvDriver(pages: pages, meta: meta, values: values, fullText: fullText);
  final app = AppState();
  app.addConnections([_conn]);
  app.kvManager.attachDriverForTest('c', driver);
  await tester.pumpWidget(
    ChangeNotifierProvider<AppState>.value(
      value: app,
      child: MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: kAppLocalizationsDelegates,
        supportedLocales: kSupportedLocales,
        // 与 main.dart 同构:Input 内的 TextField 需要 Material 祖先
        home: const Material(
          child: SizedBox(
            width: 1000,
            height: 700,
            child: KvBrowserPage(
                connection: 'c', namespaceId: '0', namespaceLabel: 'db0'),
          ),
        ),
      ),
    ),
  );
  await _settle(tester);
  return driver;
}

/// 跑固定几帧代替 pumpAndSettle:Spinner 是持续动画,后者会一直等到超时
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 8; i++) {
    await tester.pump(const Duration(milliseconds: 60));
  }
}

KvKeyPage _page(List<String> names, String? next) => KvKeyPage(
      entries: [for (final n in names) KvKeyEntry(key: KvBytes.text(n))],
      nextCursor: next,
      totalEstimate: 3,
    );

void main() {
  late Directory supportDir;

  setUp(() async {
    // AppState 会异步落盘:指向临时目录,避免污染用户目录也避免悬空 future
    supportDir = await Directory.systemTemp.createTemp('daro_kv_browser_test');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => supportDir.path,
    );
  });

  testWidgets('打开即扫出键并批量补元数据', (tester) async {
    final driver = await _pumpPage(
      tester,
      pages: [_page(['user:1', 'user:2'], null)],
      meta: const {
        'user:1': KvKeyMeta(
            type: KvKeyType.hash, encoding: 'listpack', byteSize: 128),
        'user:2': KvKeyMeta(type: KvKeyType.string, encoding: 'embstr'),
      },
    );

    expect(find.text('user:1'), findsOneWidget);
    expect(find.text('user:2'), findsOneWidget);
    expect(find.text('哈希'), findsOneWidget);
    expect(find.text('listpack'), findsOneWidget);
    expect(find.text('128'), findsOneWidget);
    // 拿不到 TTL 显示横杠,不猜 0(0 会被读成「马上过期」)
    expect(find.text('--'), findsWidgets);
    expect(find.textContaining('已加载 2 个键 / 约 3'), findsOneWidget);
    expect(driver.scanCalls, 1);
  });

  testWidgets('带过滤的空页继续翻,不被当成「没有匹配项」', (tester) async {
    // SCAN 语义:命中稀疏时服务端回空页 + 非零游标(实测 2025 键里 1 个 hash)
    final driver = await _pumpPage(
      tester,
      pages: [
        _page([], '1'),
        _page([], '2'),
        _page(['cfg:hash'], '3'),
      ],
    );

    expect(find.text('cfg:hash'), findsOneWidget);
    expect(find.text('没有符合条件的键'), findsNothing);
    // 一次动作内连翻三轮,而不是停第一个空页上
    expect(driver.scanCalls, 3);
  });

  testWidgets('一直扫不到命中会在轮数上限停手,不做静默全库遍历',
      (tester) async {
    final driver = await _pumpPage(
      tester,
      pages: [for (var i = 0; i < 200; i++) _page([], '$i')],
    );

    expect(driver.scanCalls, 40);
    expect(find.textContaining('本轮未命中,可继续加载'), findsOneWidget);
    // 「加载更多」仍可点:游标留着,下一轮动作接着翻
    expect(find.text('加载更多'), findsOneWidget);
  });

  testWidgets('单击键行即取值,列形状跟着类型换', (tester) async {
    await _pumpPage(
      tester,
      pages: [_page(['cfg:hash'], null)],
      values: {
        'cfg:hash': KvValueRead(
          key: KvBytes.text('cfg:hash'),
          type: KvKeyType.hash,
          rows: [
            KvValueRow(
                field: KvBytes.text('name'), value: KvBytes.text('daro')),
            KvValueRow(field: KvBytes.text('lang'), value: KvBytes.text('zh')),
          ],
          totalCount: 2,
        ),
      },
    );

    expect(find.text('选择左侧的键查看值'), findsOneWidget);
    await tester.tap(find.text('cfg:hash'));
    await _settle(tester);

    expect(find.text('字段'), findsOneWidget);
    expect(find.text('name'), findsOneWidget);
    expect(find.text('daro'), findsOneWidget);
    expect(find.textContaining('共 2 项'), findsOneWidget);
  });

  testWidgets('大字符串先给前段,「读取后续内容」按偏移续读', (tester) async {
    final driver = await _pumpPage(
      tester,
      pages: [_page(['big:huge'], null)],
      values: {
        'big:huge': KvValueRead(
          key: KvBytes.text('big:huge'),
          type: KvKeyType.string,
          rows: [KvValueRow(value: KvBytes.text('AAAA'))],
          totalLength: 8,
          truncated: true,
          rangeStart: 0,
        ),
      },
      fullText: const {'big:huge': 'AAAABBBB'},
    );

    await tester.tap(find.text('big:huge'));
    await _settle(tester);
    expect(find.textContaining('值较大,已显示前 4 / 8 字节'), findsOneWidget);
    // 前段展示期间不能允许第二次续读请求叠上来
    expect(find.text('BBBB'), findsNothing);

    await tester.tap(find.text('读取后续内容'));
    await _settle(tester);
    expect(find.text('BBBB'), findsOneWidget);
    expect(driver.readOffsets, [0, 4]);
    expect(find.textContaining('共 8 字节'), findsOneWidget);
  });

  testWidgets('值读取失败给出原因并可重点重试', (tester) async {
    await _pumpPage(
      tester,
      pages: [_page(['gone:key'], null)],
      values: const {},
    );

    await tester.tap(find.text('gone:key'));
    await _settle(tester);
    expect(find.textContaining('值读取失败'), findsOneWidget);
  });
}
