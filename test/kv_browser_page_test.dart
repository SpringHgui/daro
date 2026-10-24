import 'dart:async';
import 'dart:io';

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/kv_driver.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/kv_browser_page.dart';
import 'package:flutter/gestures.dart' show kSecondaryMouseButton;
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
    Map<String, KvValueRead> values = const {},
    this.fullText = const {},
    this.writableTypes = const {},
    this.writesAllowed = false,
  }) : store = Map<String, KvValueRead>.of(values);

  final List<KvKeyPage> pages;
  final Map<String, KvKeyMeta> meta;

  /// 值存储:写方法就地改它,于是「写回后再读」能看到新内容(与真服务端同构)
  final Map<String, KvValueRead> store;

  /// 大值的完整原文(只在按偏移续读时用):[store] 里放的是首段预览
  final Map<String, String> fullText;

  /// 能力表里的可写类型。页面读的是**在用驱动**的这张表,所以这里就是开关:
  /// 默认空集 = 「这台引擎不给写入口」
  final Set<KvKeyType> writableTypes;

  /// false = 写方法只记一笔随即抛错:只读用例若被页面误触发写,
  /// 表现成错误横幅而不是静默成功
  final bool writesAllowed;

  /// 页面发来的写命令(按发生顺序),供断言「哪条命令、对哪个键、什么值」
  final List<String> writes = [];

  int scanCalls = 0;
  final List<int> readOffsets = [];

  /// 非 null 时让 [readValue] 卡在闸上:复现「写后重取还在途,用户已经点了
  /// 下一格」这段真服务端往返里必然存在的窗口
  Completer<void>? valueReadGate;

  /// 回放位置:SCAN 的游标由测试脚本决定,页面只管往后翻
  int _cursorIndex = 0;

  void _record(String op) {
    writes.add(op);
    if (!writesAllowed) {
      throw UnsupportedError('本页用例默认只读,页面却发起了写操作:$op');
    }
  }

  @override
  KvCapabilities get capabilities => KvCapabilities(
        multipleNamespaces: true,
        cursorScan: true,
        keyTypes: {KvKeyType.string, KvKeyType.hash, ...writableTypes},
        ttl: true,
        internalEncoding: true,
        memoryUsage: true,
        valueOffsetRead: true,
        idleTime: false,
        commandConsole: false,
        writableTypes: writableTypes,
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
    final gate = valueReadGate;
    if (gate != null) await gate.future;
    final read = store[key.display];
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

  // 写方法:记一笔命令摘要,放行时才真的改内存值存储。
  // 不放行时随即抛错 —— 只读用例被页面误触发写,要炸出来而不是静默成功。
  @override
  Future<void> setStringValue(KvNamespace namespace, KvBytes key,
          KvBytes value,
          {int? ttlSeconds}) async =>
      _apply(key.display, 'SET ${key.display} ${value.display} ${ttlSeconds == null ? 'KEEPTTL' : 'EX $ttlSeconds'}',
          (read) => KvValueRead(
                key: read.key,
                type: KvKeyType.string,
                rows: [KvValueRow(value: value)],
                totalLength: value.byteLength,
                rangeStart: 0,
                ttlSeconds: read.ttlSeconds,
              ));

  @override
  Future<int> deleteKeys(KvNamespace namespace, List<KvBytes> keys) async {
    _record('DEL ${keys.map((k) => k.display).join(' ')}');
    for (final k in keys) {
      store.remove(k.display);
    }
    return keys.length;
  }

  @override
  Future<int> setTtl(
      KvNamespace namespace, List<KvBytes> keys, int? ttlSeconds) async {
    for (final k in keys) {
      _record(ttlSeconds == null
          ? 'PERSIST ${k.display}'
          : 'EXPIRE ${k.display} $ttlSeconds');
    }
    for (final k in keys) {
      final read = store[k.display];
      if (read != null) {
        store[k.display] = KvValueRead(
            key: read.key,
            type: read.type,
            rows: read.rows,
            totalLength: read.totalLength,
            totalCount: read.totalCount,
            truncated: read.truncated,
            rangeStart: read.rangeStart,
            ttlSeconds: ttlSeconds ?? -1);
      }
    }
    return keys.length;
  }

  @override
  Future<void> hashWrite(KvNamespace namespace, KvBytes key,
      {List<(KvBytes field, KvBytes value)> set = const [],
      List<KvBytes> removeFields = const []}) async {
    _record([
      for (final e in set) 'HSET ${key.display} ${e.$1.display}=${e.$2.display}',
      for (final f in removeFields) 'HDEL ${key.display} ${f.display}',
    ].join(' | '));
    _mutateRows(key.display, (rows) {
      for (final e in set) {
        final i = rows.indexWhere((r) => r.field == e.$1);
        if (i >= 0) {
          rows[i] = KvValueRow(field: e.$1, value: e.$2);
        } else {
          rows.add(KvValueRow(field: e.$1, value: e.$2));
        }
      }
      rows.removeWhere((r) => removeFields.contains(r.field));
    });
  }

  @override
  Future<void> listWrite(KvNamespace namespace, KvBytes key,
      {List<(int index, KvBytes value)> set = const [],
      List<int> removeIndexes = const [],
      KvBytes? prepend,
      KvBytes? append}) async {
    _record([
      for (final e in set) 'LSET ${key.display} ${e.$1} ${e.$2.display}',
      if (prepend != null) 'LPUSH ${key.display} ${prepend.display}',
      if (append != null) 'RPUSH ${key.display} ${append.display}',
      for (final i in removeIndexes) 'LREM ${key.display} $i',
    ].join(' | '));
    _mutateRows(key.display, (rows) {
      for (final e in set) {
        if (e.$1 >= 0 && e.$1 < rows.length) {
          rows[e.$1] = KvValueRow(index: e.$1, value: e.$2);
        }
      }
      if (prepend != null) {
        rows.insert(0, KvValueRow(value: prepend));
      } else if (append != null) {
        rows.add(KvValueRow(value: append));
      }
      for (final i in removeIndexes.reversed) {
        if (i >= 0 && i < rows.length) rows.removeAt(i);
      }
    });
  }

  @override
  Future<void> setWrite(KvNamespace namespace, KvBytes key,
      {List<KvBytes> add = const [],
      List<KvBytes> removeMembers = const []}) async {
    _record([
      for (final m in add) 'SADD ${key.display} ${m.display}',
      for (final m in removeMembers) 'SREM ${key.display} ${m.display}',
    ].join(' | '));
    _mutateRows(key.display, (rows) {
      rows.removeWhere((r) => removeMembers.contains(r.value));
      for (final m in add) {
        if (!rows.any((r) => r.value == m)) rows.add(KvValueRow(value: m));
      }
    });
  }

  @override
  Future<void> zsetWrite(KvNamespace namespace, KvBytes key,
      {List<(KvBytes member, double score)> add = const [],
      List<KvBytes> removeMembers = const []}) async {
    _record([
      for (final e in add) 'ZADD ${key.display} ${e.$2} ${e.$1.display}',
      for (final m in removeMembers) 'ZREM ${key.display} ${m.display}',
    ].join(' | '));
    _mutateRows(key.display, (rows) {
      rows.removeWhere((r) => removeMembers.contains(r.value));
      for (final e in add) {
        final i = rows.indexWhere((r) => r.value == e.$1);
        if (i >= 0) {
          rows[i] = KvValueRow(value: e.$1, score: e.$2);
        } else {
          rows.add(KvValueRow(value: e.$1, score: e.$2));
        }
      }
    });
  }

  /// 就地改存储里那个键的行,并同步元素数
  void _mutateRows(String keyName, void Function(List<KvValueRow> rows) body) {
    final read = store[keyName];
    if (read == null) return;
    final rows = [...read.rows];
    body(rows);
    store[keyName] = KvValueRead(
        key: read.key,
        type: read.type,
        rows: rows,
        totalLength: read.totalLength,
        totalCount: rows.length,
        ttlSeconds: read.ttlSeconds);
  }

  /// 字符串整段替换
  void _apply(String keyName, String op,
      KvValueRead Function(KvValueRead read) next) {
    _record(op);
    final read = store[keyName];
    if (read != null) store[keyName] = next(read);
  }

  @override
  Future<KvServerInfo?> readServerInfo() async => null;
}

Future<FakeKvDriver> _pumpPage(
  WidgetTester tester, {
  required List<KvKeyPage> pages,
  Map<String, KvKeyMeta> meta = const {},
  Map<String, KvValueRead> values = const {},
  Map<String, String> fullText = const {},
  Set<KvKeyType> writableTypes = const {},
  bool writesAllowed = false,
}) async {
  final driver = FakeKvDriver(
    pages: pages,
    meta: meta,
    values: values,
    fullText: fullText,
    writableTypes: writableTypes,
    writesAllowed: writesAllowed,
  );
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

/// 剪贴板截获:`Clipboard.setData` 走 SystemChannels.platform,
/// 不注册 mock 处理器就永远读不到写入内容(测试环境没有真剪贴板)
String? _clipText;

void _watchClipboard() {
  _clipText = null;
  TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
      .setMockMethodCallHandler(SystemChannels.platform, (call) async {
    if (call.method == 'Clipboard.setData') {
      _clipText = (call.arguments as Map<Object?, Object?>)['text'] as String?;
    }
    return null;
  });
}

/// 页脚/回执里的复制回执(整串以「已复制」开头,与查询页同一口径)
String? get _copyNote {
  for (final w in find
          .byType(SelectableText)
          .evaluate()
          .map((e) => e.widget as SelectableText)) {
    final text = w.data ?? w.textSpan?.toPlainText() ?? '';
    if (text.startsWith('已复制')) return text;
  }
  return null;
}

/// 哈希值读取:字段名 → 字段值
KvValueRead _hashRead(String key, Map<String, String> fields) => KvValueRead(
      key: KvBytes.text(key),
      type: KvKeyType.hash,
      rows: [
        for (final e in fields.entries)
          KvValueRow(field: KvBytes.text(e.key), value: KvBytes.text(e.value)),
      ],
      totalCount: fields.length,
    );

/// 字符串值读取(值面板对它用多行格式视图,不再是网格)
KvValueRead _stringRead(String key, String text) => KvValueRead(
      key: KvBytes.text(key),
      type: KvKeyType.string,
      rows: [KvValueRow(value: KvBytes.text(text))],
      totalLength: text.length,
    );

/// 值视图当前的文本(那个多行编辑框的内容)
String _viewerText(WidgetTester tester) => tester
    .widget<EditableText>(find.descendant(
        of: find.byType(Textarea), matching: find.byType(EditableText)))
    .controller
    .text;

/// 值工具条上的格式下拉 → 选中 [label](面板项与收起态同文字,取最后一个)
Future<void> _pickFormat(WidgetTester tester, String label) async {
  await tester.tap(find.byType(ComboBox<KvValueFormat>));
  await _settle(tester);
  await tester.tap(find.text(label).last);
  await _settle(tester);
}

Future<void> _rightClick(WidgetTester tester, Finder target) async {
  await tester.tap(target, buttons: kSecondaryMouseButton);
  await _settle(tester);
}

/// 值视图里那个多行编辑框(只在 KvBrowserPage 子树内,不与过滤框混)
final Finder _viewerField = find.descendant(
    of: find.descendant(
        of: find.byType(KvBrowserPage), matching: find.byType(Textarea)),
    matching: find.byType(EditableText));

/// 值工具条上的「保存」按钮(Button 的文案是子 Text,按祖先取控件本体)
final Finder _saveButton = find.ancestor(
    of: _toolbarText('保存'), matching: find.byType(Button));

/// 只找浮层菜单里的那一项:工具条上会出现同一句跟随选区的文案,不能混着验
Finder _menuItem(String text) => find.descendant(
    of: find.byType(ContextMenuPanel), matching: find.text(text));

/// 只找页面自己(非浮层)里的文案:菜单挂在 Navigator 的 Overlay 上,
/// 在 KvBrowserPage 子树之外
Finder _toolbarText(String text) =>
    find.descendant(of: find.byType(KvBrowserPage), matching: find.text(text));

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
    expect(find.textContaining('本轮未命中,可继续扫描'), findsOneWidget);
    // 工具条上的「加载更多」已经拿掉了。空列表滚不动,自动续扫无从触发,
    // 所以中央那句说明本身就是续扫入口
    expect(find.text('加载更多'), findsNothing);
    await tester.tap(find.textContaining('点此继续扫描'));
    await _settle(tester);
    // 游标留着:续扫从上一轮的位点往后翻,不是从头重扫
    expect(driver.scanCalls, 80);
  });

  testWidgets('滚到接近底部自动续扫一批', (tester) async {
    final driver = await _pumpPage(
      tester,
      pages: [
        _page([for (var i = 0; i < 40; i++) 'key:$i'], '1'),
        _page(['tail:key'], '2'),
      ],
    );
    expect(driver.scanCalls, 1);
    expect(find.textContaining('已加载 40 个键'), findsOneWidget);
    // 列表撑满视口了,兜底行不该和自动加载同时出现
    expect(find.text('未扫完,点此继续扫描'), findsNothing);

    await tester.drag(find.byType(DataGridView).first, const Offset(0, -600));
    await _settle(tester);

    expect(driver.scanCalls, 2);
    expect(find.textContaining('已加载 41 个键'), findsOneWidget);
  });

  testWidgets('命中稀少、列表滚不动时给兜底续扫行', (tester) async {
    final driver = await _pumpPage(
      tester,
      pages: [_page(['a:1', 'a:2'], '1'), _page(['a:3'], '2')],
    );

    expect(find.text('未扫完,点此继续扫描'), findsOneWidget);
    await tester.tap(find.text('未扫完,点此继续扫描'));
    await _settle(tester);

    expect(find.text('a:3'), findsOneWidget);
    expect(driver.scanCalls, 2);
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
      // 给写权限:这条要守的是「未读全」这道闸门,不能让能力表先把原因抢走
      writableTypes: const {KvKeyType.string},
    );

    await tester.tap(find.text('big:huge'));
    await _settle(tester);
    expect(find.textContaining('值较大,已显示前 4 / 8 字节'), findsOneWidget);
    // 未读全的视图必须说清「这里不能改」,否则改半截就写回整段
    expect(find.textContaining('值未完整读出'), findsOneWidget);
    // 前段展示期间不能允许第二次续读请求叠上来
    expect(_viewerText(tester), 'AAAA');

    await tester.tap(find.text('读取后续内容'));
    await _settle(tester);
    expect(_viewerText(tester), 'AAAABBBB');
    expect(driver.readOffsets, [0, 4]);
    expect(find.textContaining('共 8 字节'), findsOneWidget);
    expect(find.textContaining('值未完整读出'), findsNothing,
        reason: '读全了就不该再挂着「未完整读出」的说法');
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

  // ─────────────────────────────────── 选中 / 复制
  // 只读数据必须能一次拿出去:看得到键名却复制不了,等于逼用户手抄。

  Future<void> _copyWithCtrlC(WidgetTester tester) async {
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await _settle(tester);
  }

  testWidgets('键列表点一行再按 Ctrl+C:键名进剪贴板,页脚给出回执',
      (tester) async {
    _watchClipboard();
    await _pumpPage(tester, pages: [_page(['user:1', 'user:2'], null)]);

    await tester.tap(find.text('user:1'));
    await _settle(tester);
    await _copyWithCtrlC(tester);

    expect(_clipText, 'user:1');
    expect(_copyNote, contains('已复制 1 个键名'));
  });

  testWidgets('右键「复制键名」与 Ctrl+C 同一结果(菜单不能另是一套)',
      (tester) async {
    _watchClipboard();
    await _pumpPage(tester, pages: [_page(['user:1', 'user:2'], null)]);

    // 右键先让落点成为选中目标(base-ui 2588939 的契约),所以不必先左键点一次
    await tester.tap(find.text('user:2'), buttons: kSecondaryMouseButton);
    await _settle(tester);
    await tester.tap(find.descendant(
        of: find.byType(ContextMenuPanel), matching: find.text('复制键名')));
    await _settle(tester);

    expect(_clipText, 'user:2');
  });

  testWidgets('值网格选中单元格后 Ctrl+C 复制单元格文本', (tester) async {
    _watchClipboard();
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
          ],
          totalCount: 1,
        ),
      },
    );

    await tester.tap(find.text('cfg:hash'));
    await _settle(tester);
    // 键行也处于选中态:此时选中值网格的格后按 Ctrl+C,复制的必须是「值」那一侧
    await tester.tap(find.text('daro'));
    await _settle(tester);
    await _copyWithCtrlC(tester);

    expect(_clipText, 'daro', reason: '两个网格各有选区,复制不能凭猜');
    expect(_copyNote, contains('已复制 1 个单元格'));
  });

  testWidgets('焦点回到过滤框时 Ctrl+C 归输入框,不被键名盖掉', (tester) async {
    _watchClipboard();
    await _pumpPage(tester, pages: [_page(['user:1'], null)]);

    // 点行让本页拿到焦点(上一条用例验的就是这条路),再回到过滤框
    await tester.tap(find.text('user:1'));
    await _settle(tester);
    await tester.tap(find.byType(Input));
    await _settle(tester);
    await tester.enterText(find.byType(Input), 'scan:*');
    await _settle(tester);

    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await _settle(tester);

    expect(_clipText, 'scan:*', reason: '框里有选区时该复制选中的过滤词');
    // 剪贴板在测试里由两个写入方共同覆盖(测试绑定会把事件同时投给焦点系统),
    // 所以判据看本页自己的回执:它一旦接管就写「已复制 N 个键名」
    expect(_copyNote, isNull, reason: '焦点在过滤框时本页不该接管 Ctrl+C');
  });

  // ─────────────────────────────────── 写:入口门控 / 就地编辑 / 菜单文案
  // 页面的写入口读的是**在用驱动**的能力表(KvConnectionManager.capabilitiesOf),
  // 所以 FakeKvDriver.writableTypes 就是「这台引擎让不让写」的开关。

  testWidgets('能力表可写:文本视图改完点保存,只发一条 SET 并回读新值',
      (tester) async {
    final driver = await _pumpPage(
      tester,
      pages: [_page(['app:title'], null)],
      values: {
        'app:title': _stringRead('app:title', 'hello'),
      },
      writableTypes: {KvKeyType.string},
      writesAllowed: true,
    );

    await tester.tap(find.text('app:title'));
    await _settle(tester);
    expect(_viewerText(tester), 'hello');
    // 没改动之前保存键不可点:字符串整段替换不是「随手点一下」该发生的事
    expect(tester.widget<Button>(_saveButton).onPressed, isNull);

    await tester.enterText(_viewerField, 'world');
    await _settle(tester);
    await tester.tap(_saveButton);
    await _settle(tester);

    // 字符串整段替换必须保 TTL:SET 不带 KEEPTTL 会把已有的过期时间清掉
    expect(driver.writes, ['SET app:title world KEEPTTL']);
    expect(_viewerText(tester), 'world');
    expect(find.textContaining('已保存'), findsOneWidget);
  });

  testWidgets('字符串视图 Ctrl+S 与「保存」键同一动作', (tester) async {
    final driver = await _pumpPage(
      tester,
      pages: [_page(['app:title'], null)],
      values: {
        'app:title': _stringRead('app:title', 'hello'),
      },
      writableTypes: {KvKeyType.string},
      writesAllowed: true,
    );

    await tester.tap(find.text('app:title'));
    await _settle(tester);
    await tester.enterText(_viewerField, 'world');
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyS);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await _settle(tester);

    expect(driver.writes, ['SET app:title world KEEPTTL']);
  });

  testWidgets('字符串值可切 JSON / 十六进制视图,格式化一律只读',
      (tester) async {
    await _pumpPage(
      tester,
      pages: [_page(['cfg:json', 'cfg:other'], null)],
      values: {
        'cfg:json': _stringRead('cfg:json', '{"a":1,"b":[2,3]}'),
        'cfg:other': _stringRead('cfg:other', '{"c":true}'),
      },
      writableTypes: {KvKeyType.string},
      writesAllowed: true,
    );

    await tester.tap(find.text('cfg:json'));
    await _settle(tester);
    expect(_viewerText(tester), '{"a":1,"b":[2,3]}', reason: '默认按文本原样');

    await _pickFormat(tester, 'JSON');
    expect(_viewerText(tester), '{\n  "a": 1,\n  "b": [\n    2,\n    3\n  ]\n}');
    // 缩进是派生内容,写回就等于改了存的字节 → 保存键必须灰
    expect(tester.widget<Button>(_saveButton).onPressed, isNull);
    expect(find.textContaining('格式化视图只读'), findsOneWidget);

    // 换一个键格式仍是 JSON:格式是会话级状态,不跟着键重置
    await tester.tap(find.text('cfg:other'));
    await _settle(tester);
    expect(_viewerText(tester), '{\n  "c": true\n}');

    await _pickFormat(tester, '十六进制');
    expect(_viewerText(tester), startsWith('HEX:7B 22 63 22'));
  });

  testWidgets('不是合法 JSON:页脚说清原因,视图仍给原文而不是空白',
      (tester) async {
    await _pumpPage(
      tester,
      pages: [_page(['raw:text'], null)],
      values: {
        'raw:text': _stringRead('raw:text', 'hello world'),
      },
    );

    await tester.tap(find.text('raw:text'));
    await _settle(tester);
    await _pickFormat(tester, 'JSON');

    expect(_viewerText(tester), 'hello world');
    expect(find.textContaining('不是合法 JSON'), findsOneWidget);
  });

  testWidgets('哈希字段值就地改:只发这一条 HSET,字段名不动', (tester) async {
    final driver = await _pumpPage(
      tester,
      pages: [_page(['cfg:hash'], null)],
      values: {
        'cfg:hash': _hashRead('cfg:hash', const {'name': 'daro'}),
      },
      writableTypes: {KvKeyType.hash},
      writesAllowed: true,
    );

    await tester.tap(find.text('cfg:hash'));
    await _settle(tester);
    // 落在**值**列:改字段名是另一条路径(先按新名写、再删旧字段)
    await tester.tap(find.text('daro'));
    await _settle(tester);
    await tester.enterText(find.byType(InlineEditor), 'daro-desktop');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await _settle(tester);

    expect(driver.writes, ['HSET cfg:hash name=daro-desktop']);
    // 回读后字段名仍在原位,只有值换了
    expect(find.text('name'), findsOneWidget);
    expect(find.text('daro-desktop'), findsOneWidget);
    expect(find.text('daro'), findsNothing);
  });

  testWidgets('写后重取在途时的新选区不被抹掉;行位移后才丢', (tester) async {
    final driver = await _pumpPage(
      tester,
      pages: [_page(['cfg:hash'], null)],
      values: {
        'cfg:hash': _hashRead('cfg:hash', const {'name': 'daro', 'lang': 'zh_CN'}),
      },
      writableTypes: {KvKeyType.hash},
      writesAllowed: true,
    );
    await tester.tap(find.text('cfg:hash'));
    await _settle(tester);

    // 改 name 的值,并把「写后重取」卡在闸上:此刻页面还是旧行,而新值已在路上
    await tester.tap(find.text('daro'));
    await _settle(tester);
    await tester.enterText(find.byType(InlineEditor), 'daro-desktop');
    final gate = Completer<void>();
    driver.valueReadGate = gate;
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await _settle(tester);

    // 重取在途时点另一格:选中要留在界面上 —— 后台刷新落地就清选区,表现成
    // 「删除此项」刚出现就消失(用户视角是点了没反应)
    await tester.tap(find.text('lang'));
    await _settle(tester);
    expect(_toolbarText('删除此项'), findsOneWidget);

    gate.complete();
    await _settle(tester);
    // 新值刷出来了,而那一格的行号与身份都还在原位 → 选区必须留住
    expect(find.text('daro-desktop'), findsOneWidget);
    expect(_toolbarText('删除此项'), findsOneWidget);
    driver.valueReadGate = null;

    // 换字段名会让后面的行整体位移:同一行号已经指着另一个字段,
    // 按行号重放 HDEL 会删错对象,所以选区必须丢
    await tester.tap(find.text('name'));
    await _settle(tester);
    await tester.enterText(find.byType(InlineEditor), 'title');
    final shifted = Completer<void>();
    driver.valueReadGate = shifted;
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await _settle(tester);
    await tester.tap(find.text('zh_CN'));
    await _settle(tester);
    expect(_toolbarText('删除此项'), findsOneWidget);
    shifted.complete();
    await _settle(tester);
    expect(find.text('title'), findsOneWidget);
    expect(_toolbarText('删除此项'), findsNothing);

    expect(driver.writes, [
      'HSET cfg:hash name=daro-desktop',
      'HSET cfg:hash title=daro-desktop | HDEL cfg:hash name',
    ]);
  });

  testWidgets('引擎不给写:值格点不开编辑器,并且说清为什么', (tester) async {
    await _pumpPage(
      tester,
      pages: [_page(['cfg:hash'], null)],
      values: {
        'cfg:hash': _hashRead('cfg:hash', const {'name': 'daro'}),
      },
    );

    await tester.tap(find.text('cfg:hash'));
    await _settle(tester);
    // 写入口一个都不给:值工具条(添加 / 删除)与「新建键」都不出现
    expect(find.text('添加'), findsNothing);
    expect(find.text('新建键'), findsNothing);

    await tester.tap(find.text('daro'));
    await _settle(tester);
    expect(find.byType(InlineEditor), findsNothing);
    // 不能只是「点了没反应」:拒绝编辑要给原因
    expect(find.text('该引擎不支持写操作'), findsOneWidget);
  });

  testWidgets('删除文案跟着选区走:一格 / 三格 / 一个键 / 两个键',
      (tester) async {
    await _pumpPage(
      tester,
      pages: [_page(['k:1', 'k:2', 'k:3'], null)],
      values: {
        'k:1': _hashRead(
            'k:1', const {'name': 'daro', 'lang': 'zh_CN', 'ver': 'v3'}),
      },
      writableTypes: {KvKeyType.hash},
    );

    await tester.tap(find.text('k:1'));
    await _settle(tester);

    // 值网格:Ctrl 加选三行的值列(按下只完成选中,不该进编辑器)
    Future<void> ctrlTap(Finder target) async {
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.tap(target);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await _settle(tester);
    }

    await ctrlTap(find.text('daro'));
    await ctrlTap(find.text('zh_CN'));
    await ctrlTap(find.text('v3'));
    expect(find.byType(InlineEditor), findsNothing);

    // 右键落在选区内:三格选区整体保留,菜单与工具条都说「三项」
    await _rightClick(tester, find.text('zh_CN'));
    expect(_menuItem('删除 3 项'), findsOneWidget);
    expect(_toolbarText('删除 3 项'), findsOneWidget);

    // 右键落在选区外:按桌面惯例收起成落点那一格,文案跟着改口
    await _rightClick(tester, find.text('name'));
    expect(_menuItem('删除此项'), findsOneWidget);
    expect(_menuItem('删除 3 项'), findsNothing);
    expect(_toolbarText('删除此项'), findsOneWidget);

    // 键网格同理。先做单选:右键落在第三行,面板只往下铺,不挡住上两行
    await _rightClick(tester, find.text('k:3'));
    expect(_menuItem('删除键'), findsOneWidget);
    // Ctrl 加选第一行(这一按同时收起上一份菜单):两行 → 「删除 2 个键」
    await ctrlTap(find.text('k:1'));
    await _rightClick(tester, find.text('k:1'));
    expect(_menuItem('删除 2 个键'), findsOneWidget);
    expect(_menuItem('删除键'), findsNothing);
  });
}
