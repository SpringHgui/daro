import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../data/db_data.dart';
import '../data/drivers/kv_driver.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';

/// 行高 / 单元格留白沿用数据网格的密度口径(AGENTS.md「表格密度」):
/// 22 / 6,不是 base-ui 控件默认的 28 / 12。
const double _rowHeight = 22.0;
const double _cellPaddingX = 6.0;
const double _numberColWidth = 22.0;

/// 工具条高度:base-ui 控件高 28,容器比控件矮会在 debug 下报 overflow
const double _toolbarHeight = 32.0;

/// 页脚状态条高度
const double _footerHeight = 24.0;

/// 一次用户动作里最多翻多少轮 SCAN。
///
/// 为什么要有上限:带 MATCH / TYPE 时命中稀疏,单轮回空页 + 非零游标
/// (见 [KvKeyPage] 注释),要一直翻才有结果。不设上限就等于在用户按下
/// 「刷新」后静默做一次全键空间遍历 —— 大实例上是几十万次往返。
/// 到顶就停手,游标留着让用户再点「加载更多」继续。
const int _maxRoundsPerAction = 40;

/// 每轮 SCAN 的 COUNT 提示值(服务端可以返回更少或略多)
const int _scanCountHint = 200;

/// 目录里取不到属性时的占位符:纯标点,三语通用,不进 ARB
const String _missing = '--';

/// 键列表的一种列。列集合随驱动能力表变(不支持 TTL 就没有 TTL 列),
/// 故单元格取数按「第几列是什么」而不是按固定下标。
enum _KeyCol { name, type, ttl, encoding, bytes }

/// 键值 / 缓存型引擎(Redis 等)的键浏览页。
///
/// 上半是键列表(游标分页 + glob 过滤 + 类型过滤),下半是选中键的值网格。
/// 与 [TableDataPage] 的根本差别:这里没有「一条 SQL 拿全表」——
/// 键只能按游标一页页扫,值只能按容器类型分形状读,大字符串还要按窗口取。
/// 首版全程只读(驱动 [KvCapabilities.readOnlyFirstVersion]),界面不给写入口。
class KvBrowserPage extends StatefulWidget {
  const KvBrowserPage({
    super.key,
    required this.connection,
    required this.namespaceId,
    required this.namespaceLabel,
  });

  final String connection;

  /// 命名空间 id(Redis 的库号 '0'..'15'),来自标签的 database 槽位
  final String namespaceId;

  /// 界面显示的库名(Redis = "db0")
  final String namespaceLabel;

  @override
  State<KvBrowserPage> createState() => _KvBrowserPageState();
}

class _KvBrowserPageState extends State<KvBrowserPage> {
  final TextEditingController _pattern = TextEditingController();
  final ScrollController _keysScroll = ScrollController();

  /// 已加载的键(含批量元数据);顺序 = SCAN 返回顺序,不做本地排序
  List<KvKeyEntry> _keys = const [];
  String? _cursor;
  bool _scanDone = false;
  bool _loadingKeys = false;
  String? _keysError;

  /// 本轮动作因扫描轮数上限而提前停手(过滤命中稀疏时会出现)
  bool _hitRoundCap = false;

  /// 键总数估算:只取驱动的 DBSIZE / INFO 值,不靠累加页面条数得出
  int? _totalEstimate;

  String? _typeFilter;
  int? _selectedKey;

  KvValueRead? _value;
  bool _loadingValue = false;
  String? _valueError;

  /// 在途动作的代数:改过滤 / 加载更多 / 换选中键都会自增,
  /// 晚到的回包据此丢弃 —— 否则旧请求的响应会把新结果盖掉
  int _scanGen = 0;
  int _valueGen = 0;

  @override
  void initState() {
    super.initState();
    _startScan();
  }

  @override
  void dispose() {
    _pattern.dispose();
    _keysScroll.dispose();
    super.dispose();
  }

  KvNamespace get _namespace =>
      KvNamespace(id: widget.namespaceId, label: widget.namespaceLabel);

  /// 取连接配置;连接已被删除时返回 null(界面按「不可用」渲染)
  ConnectionInfo? _connInfo(AppState app) =>
      app.connectionByName(widget.connection);

  // ────────────────────────────────────────────── 键列表:游标扫描

  /// 从头重扫(刷新、改过滤词、改类型都走这里)
  void _startScan() {
    // 上一轮还在翻:此时清空会把在途结果一起丢掉,列表就永远空着
    if (_loadingKeys) return;
    _keys = const [];
    _cursor = null;
    _scanDone = false;
    _hitRoundCap = false;
    _totalEstimate = null;
    _selectedKey = null;
    _value = null;
    _keysError = null;
    _scanMore(reset: true);
  }

  /// 续扫一批。[reset] = 忽略当前游标从头开始。
  ///
  /// 一次调用内连着翻若干轮,直到「拿到至少一个匹配键」「游标归零」或
  /// 「翻满 [_maxRoundsPerAction] 轮」—— 空页绝不等于无结果,详见 [KvKeyPage]。
  Future<void> _scanMore({required bool reset}) async {
    if (_loadingKeys) return;
    final app = context.read<AppState>();
    final conn = _connInfo(app);
    if (conn == null) return;
    final gen = ++_scanGen;
    final startCursor = reset ? null : _cursor;
    setState(() {
      _loadingKeys = true;
      _keysError = null;
    });
    try {
      final driver = await app.kvManager.driverFor(conn);
      final fresh = <KvKeyEntry>[];
      String? cursor = startCursor;
      var done = false;
      var rounds = 0;
      while (rounds < _maxRoundsPerAction && !done && fresh.isEmpty) {
        rounds++;
        final page = await driver.scanKeys(KvKeyScan(
          namespace: _namespace,
          cursor: cursor,
          pattern: _patternBytes(),
          type: _typeFilter,
          count: _scanCountHint,
        ));
        cursor = page.nextCursor;
        done = page.done;
        _totalEstimate = page.totalEstimate ?? _totalEstimate;
        fresh.addAll(page.entries);
      }
      if (gen != _scanGen || !mounted) return;
      // 元数据一次连发取完(驱动内部流水线);逐键 await 实测慢 4 倍
      final metas = fresh.isEmpty
          ? const <KvKeyMeta>[]
          : await driver.readKeyMeta(
              _namespace, [for (final e in fresh) e.key]);
      if (gen != _scanGen || !mounted) return;
      setState(() {
        _keys = [
          ..._keys,
          for (var i = 0; i < fresh.length; i++)
            KvKeyEntry(key: fresh[i].key, meta: metas[i]),
        ];
        _cursor = cursor;
        _scanDone = done;
        // 翻满一轮都没命中、又没到游标归零:还能接着扫,但要先问用户
        _hitRoundCap = !done && fresh.isEmpty;
      });
    } on Object catch (e) {
      if (gen != _scanGen || !mounted) return;
      setState(() => _keysError = '$e');
    } finally {
      if (gen == _scanGen && mounted) setState(() => _loadingKeys = false);
    }
  }

  /// 过滤词按字节发:键名本身可以是裸字节,按文本发 MATCH 永远筛不到它们
  KvBytes? _patternBytes() {
    final text = _pattern.text.trim();
    return text.isEmpty ? null : KvBytes.text(text);
  }

  // ────────────────────────────────────────────── 值:按容器类型分形状读

  Future<void> _loadValue(int rowIndex) async {
    if (rowIndex < 0 || rowIndex >= _keys.length) return;
    final app = context.read<AppState>();
    final conn = _connInfo(app);
    if (conn == null) return;
    final key = _keys[rowIndex].key;
    final gen = ++_valueGen;
    setState(() {
      _selectedKey = rowIndex;
      _loadingValue = true;
      _valueError = null;
    });
    try {
      final driver = await app.kvManager.driverFor(conn);
      final read = await driver.readValue(_namespace, key);
      if (gen != _valueGen || !mounted) return;
      setState(() => _value = read);
    } on Object catch (e) {
      if (gen != _valueGen || !mounted) return;
      setState(() {
        _value = null;
        _valueError = '$e';
      });
    } finally {
      if (gen == _valueGen && mounted) setState(() => _loadingValue = false);
    }
  }

  /// 大字符串续读:从已展示的右缘接着取下一段,而不是整段灌进内存
  Future<void> _loadValueTail() async {
    final current = _value;
    if (current == null || _selectedKey == null) return;
    final app = context.read<AppState>();
    final conn = _connInfo(app);
    if (conn == null) return;
    final shown = (current.rangeStart ?? 0) + _previewBytes(current);
    final gen = ++_valueGen;
    setState(() => _loadingValue = true);
    try {
      final driver = await app.kvManager.driverFor(conn);
      final read =
          await driver.readValue(_namespace, current.key, offset: shown);
      if (gen != _valueGen || !mounted) return;
      setState(() {
        _value = KvValueRead(
          key: read.key,
          type: read.type,
          rows: [...current.rows, ...read.rows],
          totalLength: read.totalLength ?? current.totalLength,
          totalCount: read.totalCount ?? current.totalCount,
          truncated: read.truncated,
          // 已展示到的右缘:下一次续读从这里开始
          rangeStart: shown,
        );
      });
    } on Object catch (e) {
      if (gen != _valueGen || !mounted) return;
      setState(() => _valueError = '$e');
    } finally {
      if (gen == _valueGen && mounted) setState(() => _loadingValue = false);
    }
  }

  // ────────────────────────────────────────────── build

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final l = context.l10n;
    final conn = _connInfo(context.read<AppState>());
    final caps = conn == null ? null : createKvDriver(conn)?.capabilities;
    // 连接已删除,或引擎不支持枚举键(如 Memcached 没有 SCAN):
    // 只给一句说明,不给「点了没反应」的控件
    if (caps == null) {
      return _notice(t, l.kvConnectionGone);
    }
    if (!caps.cursorScan) return _notice(t, l.kvScanUnsupported);

    return ColoredBox(
      color: t.background,
      child: SplitContainer(
        orientation: Axis.vertical,
        initialRatio: 0.45,
        minFirst: 120,
        minSecond: 120,
        tokens: t.desktopTokensFor(context),
        first: _keyPane(t, caps),
        second: _valuePane(t),
      ),
    );
  }

  // ────────────────────────────────────────────── 上半:键列表

  Widget _keyPane(AppPalette t, KvCapabilities caps) {
    final l = context.l10n;
    return Column(
      children: [
        SizedBox(
          height: _toolbarHeight,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
            child: Row(
              children: [
                Expanded(
                  child: Input(
                    controller: _pattern,
                    hint: l.kvPatternHint,
                    textInputAction: TextInputAction.search,
                    height: 26,
                    // 游标扫描无法「边打字边过滤」:改过滤词只能从头重扫
                    onSubmitted: (_) => _startScan(),
                  ),
                ),
                const SizedBox(width: 6),
                SizedBox(
                  width: 112,
                  child: ComboBox<String>(
                    // '' = 不按类型过滤;放在第一项,清空就是选回它
                    items: const ['', 'string', 'hash', 'list', 'set', 'zset', 'stream'],
                    value: _typeFilter ?? '',
                    itemToString: (v) =>
                        v.isEmpty ? l.kvTypeAll : _typeLabel(l, kvKeyTypeOf(v)),
                    onChanged: (v) {
                      setState(() => _typeFilter =
                          (v == null || v.isEmpty) ? null : v);
                      _startScan();
                    },
                  ),
                ),
                const SizedBox(width: 6),
                Button(
                  text: l.kvActionRefresh,
                  onPressed: _loadingKeys ? null : _startScan,
                ),
                const SizedBox(width: 6),
                Button(
                  text: l.kvActionLoadMore,
                  onPressed: (_loadingKeys || _scanDone) ? null : _loadMore,
                ),
              ],
            ),
          ),
        ),
        Expanded(child: _keyGrid(t, caps)),
        _footer(t, _keySummary(l)),      ],
    );
  }

  void _loadMore() {
    if (_scanDone) return;
    _scanMore(reset: false);
  }

  String _keySummary(AppLocalizations l) {
    final loaded = _keys.length;
    return [
      _totalEstimate == null
          ? l.kvLoadedCount('$loaded')
          : l.kvLoadedOfTotal('$loaded', '$_totalEstimate'),
      if (_loadingKeys) l.kvScanning,
      if (_hitRoundCap) l.kvRoundCapHint,
      if (_scanDone && loaded > 0) l.kvScanComplete,
    ].join(' · ');
  }

  /// 本类型的列集合(键名恒有,其余按能力表)
  List<_KeyCol> _keyCols(KvCapabilities caps) => [
        _KeyCol.name,
        _KeyCol.type,
        if (caps.ttl) _KeyCol.ttl,
        if (caps.internalEncoding) _KeyCol.encoding,
        if (caps.memoryUsage) _KeyCol.bytes,
      ];

  Widget _keyGrid(AppPalette t, KvCapabilities caps) {
    final l = context.l10n;
    if (_loadingKeys && _keys.isEmpty) {
      return const Center(child: Spinner(size: 18));
    }
    final error = _keysError;
    if (error != null) {
      // 出错给一行可点的提示(与连接树的失败提示行同一交互):点了重扫
      return _hintRow(text: l.kvLoadFailed(error), onTap: _startScan);
    }
    if (_keys.isEmpty) {
      return Center(
        child: Text(
          _scanDone ? l.kvNoMatch : l.kvNoMatchYet,
          style: TextStyle(fontSize: 12, color: t.mutedForeground),
        ),
      );
    }
    final cols = _keyCols(caps);
    return ScrollBar(
      controller: _keysScroll,
      thumbVisibility: true,
      child: DataGridView(
        columns: [
          for (final col in cols)
            DataGridViewColumn(
              // 键名列不设固定宽:长短差别极大,把剩余宽度全给它
              title: switch (col) {
                _KeyCol.name => l.kvColKeyName,
                _KeyCol.type => l.kvColType,
                _KeyCol.ttl => l.kvColTtl,
                _KeyCol.encoding => l.kvColEncoding,
                _KeyCol.bytes => l.kvColBytes,
              },
              flex: col == _KeyCol.name ? 1 : 0,
              width: switch (col) {
                _KeyCol.name => null,
                _KeyCol.type => 76,
                _KeyCol.ttl => 92,
                _KeyCol.encoding => 96,
                _KeyCol.bytes => 84,
              },
              alignment: col == _KeyCol.bytes
                  ? Alignment.centerRight
                  : Alignment.centerLeft,
            ),
        ],
        rowCount: _keys.length,
        rowHeight: _rowHeight,
        cellPaddingX: _cellPaddingX,
        showRowNumbers: true,
        rowNumberWidth: _numberColWidth,
        rowNumberBuilder: (row, rowSelected) => rowSelected
            ? Icon(Icons.play_arrow, size: 14, color: t.accentForeground)
            : const SizedBox.shrink(),
        selectedRow: _selectedKey,
        // 只给 onRowSelected:单击任意格即取值(组件内部回退到行选中,零延迟)
        onRowSelected: _loadValue,
        headerColor: t.secondary,
        headerFontSize: 12,
        gridLineColor: t.gridLine,
        rowHoverColor: Color.alphaBlend(
          t.foreground.withValues(alpha: 0.06),
          t.background,
        ),
        tokens: t.desktopTokensFor(context),
        verticalScrollController: _keysScroll,
        cellBuilder: (row, col) {
          final meta = _keys[row].meta;
          final style = _cellStyle();
          return switch (cols[col]) {
            _KeyCol.name => Text(_keys[row].key.display,
                style: style,
                overflow: TextOverflow.ellipsis,
                maxLines: 1),
            _KeyCol.type => Text(_typeLabel(l, meta?.type), style: style),
            _KeyCol.ttl => Text(_ttlLabel(l, meta), style: style),
            _KeyCol.encoding => Text(meta?.encoding ?? _missing, style: style),
            _KeyCol.bytes =>
              Text(meta?.byteSize?.toString() ?? _missing, style: style),
          };
        },
      ),
    );
  }

  // ────────────────────────────────────────────── 下半:值

  Widget _valuePane(AppPalette t) {
    final l = context.l10n;
    if (_loadingValue) return const Center(child: Spinner(size: 18));
    final error = _valueError;
    if (error != null) {
      return _hintRow(
        text: l.kvValueFailed(error),
        onTap: () {
          final selected = _selectedKey;
          if (selected != null) _loadValue(selected);
        },
      );
    }
    final read = _value;
    if (read == null) {
      return Center(
        child: Text(l.kvPickKey,
            style: TextStyle(fontSize: 12, color: t.mutedForeground)),
      );
    }
    // 键在两次点击之间过期或被删:服务端回 none,值网格没有行可显示
    if (read.type == KvKeyType.none) {
      return Center(
        child: Text(l.kvKeyGone,
            style: TextStyle(fontSize: 12, color: t.mutedForeground)),
      );
    }
    return Column(
      children: [
        Expanded(child: _valueGrid(t, read)),
        _footer(
          t,
          _valueSummary(l, read),
          // 大值只回了一段:续读按钮挂在页脚右侧,不做「滚到底自动取」
          trailing: _isTruncatedString(read)
              ? Button(text: l.kvActionReadMore, onPressed: _loadValueTail)
              : null,
        ),
      ],
    );
  }

  /// 值网格的列形状按 [KvValueRead.type] 切换(映射表见 kv_driver.dart),
  /// 但始终是同一张 DataGridView:换类型只换列定义与取数,不换控件
  Widget _valueGrid(AppPalette t, KvValueRead read) {
    final l = context.l10n;
    // 首列只在有「左边那个东西」时出现:hash 的字段名 / list 的下标 /
    // stream 的条目 ID;zset 的首列是分数;string 与 set 只有值一列
    final firstLabel = switch (read.type) {
      KvKeyType.hash => l.kvColField,
      KvKeyType.list => l.kvColIndex,
      KvKeyType.stream => l.kvColEntryId,
      KvKeyType.zset => l.kvColScore,
      _ => null,
    };
    final firstWidth = switch (read.type) {
      KvKeyType.list => 72.0,
      KvKeyType.stream => 180.0,
      _ => 92.0,
    };
    return ScrollConfiguration(
      behavior: ScrollConfiguration.of(context).copyWith(scrollbars: false),
      child: DataGridView(
        columns: [
          if (firstLabel != null)
            DataGridViewColumn(
              title: firstLabel,
              width: firstWidth,
              alignment: read.type == KvKeyType.list ||
                      read.type == KvKeyType.zset
                  ? Alignment.centerRight
                  : Alignment.centerLeft,
            ),
          DataGridViewColumn(
            title: switch (read.type) {
              KvKeyType.set || KvKeyType.zset => l.kvColMember,
              _ => l.kvColValue,
            },
            flex: 1,
          ),
        ],
        rowCount: read.rows.length,
        rowHeight: _rowHeight,
        cellPaddingX: _cellPaddingX,
        headerColor: t.secondary,
        headerFontSize: 12,
        gridLineColor: t.gridLine,
        rowHoverColor: Color.alphaBlend(
          t.foreground.withValues(alpha: 0.06),
          t.background,
        ),
        tokens: t.desktopTokensFor(context),
        cellBuilder: (row, col) {
          final r = read.rows[row];
          final style = _cellStyle();
          if (firstLabel == null) {
            return Text(r.value.display,
                style: style, overflow: TextOverflow.ellipsis, maxLines: 1);
          }
          if (col == 0) {
            // zset 的首列是分数(已转 double,转不动时按原文本显示)
            if (read.type == KvKeyType.zset) {
              return Text(r.score?.toString() ?? _missing, style: style);
            }
            return Text(
              r.entryId ??
                  r.field?.display ??
                  (r.index == null ? _missing : '${r.index}'),
              style: style,
              overflow: TextOverflow.ellipsis,
              maxLines: 1,
            );
          }
          return Text(r.value.display,
              style: style, overflow: TextOverflow.ellipsis, maxLines: 1);
        },
      ),
    );
  }

  String _valueSummary(AppLocalizations l, KvValueRead read) {
    final label = _typeLabel(l, read.type);
    final size = switch (read.type) {
      KvKeyType.string => l.kvValueBytes('${read.totalLength ?? 0}'),
      _ => l.kvValueCount('${read.totalCount ?? read.rows.length}'),
    };
    if (_isTruncatedString(read)) {
      return '$label · ${l.kvValueTruncated('${_previewBytes(read)}', '${read.totalLength ?? 0}')}';
    }
    return '$label · $size';
  }

  /// 字符串值只取回了一段(驱动的预览上限),还能继续往后读
  bool _isTruncatedString(KvValueRead read) =>
      read.type == KvKeyType.string &&
      read.truncated &&
      (read.totalLength ?? 0) > (read.rangeStart ?? 0) + _previewBytes(read);

  int _previewBytes(KvValueRead? read) =>
      read == null ? 0 : read.rows.fold(0, (a, r) => a + r.value.byteLength);

  // ────────────────────────────────────────────── 公共小件

  Widget _footer(AppPalette t, String text, {Widget? trailing}) {
    final tokens = t.desktopTokensFor(context);
    return Container(
      height: _footerHeight,
      padding: const EdgeInsets.symmetric(horizontal: 6),
      decoration: BoxDecoration(
        color: t.secondary,
        border:
            Border(top: BorderSide(color: t.border, width: tokens.borderWidth)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Text(text,
                style: TextStyle(fontSize: 11.5, color: t.mutedForeground),
                overflow: TextOverflow.ellipsis),
          ),
          if (trailing != null) trailing,
        ],
      ),
    );
  }

  Widget _notice(AppPalette t, String text) => ColoredBox(
        color: t.background,
        child: Center(
          child: Text(text,
              style: TextStyle(fontSize: 12, color: t.mutedForeground)),
        ),
      );

  Widget _hintRow({required String text, required VoidCallback onTap}) {
    return GestureDetector(
      behavior: HitTestBehavior.translucent,
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Text(
          text,
          style: const TextStyle(
              fontSize: 12, decoration: TextDecoration.none),
          maxLines: 6,
          overflow: TextOverflow.ellipsis,
        ),
      ),
    );
  }

  /// 单元格文字样式:脱离 Material 兜底,必须显式关掉下划线与加粗
  /// (AGENTS.md「菜单/浮层性能」第 5 条同源)
  TextStyle _cellStyle() => const TextStyle(
        fontSize: 12.5,
        decoration: TextDecoration.none,
        fontWeight: FontWeight.w400,
      );

  String _typeLabel(AppLocalizations l, KvKeyType? type) => switch (type) {
        null => _missing,
        KvKeyType.string => l.kvTypeString,
        KvKeyType.list => l.kvTypeList,
        KvKeyType.set => l.kvTypeSet,
        KvKeyType.zset => l.kvTypeZset,
        KvKeyType.hash => l.kvTypeHash,
        KvKeyType.stream => l.kvTypeStream,
        KvKeyType.none => l.kvTypeNone,
        KvKeyType.unknown => l.kvTypeUnknown,
      };

  /// TTL 列:-1 = 永不过期,-2 = 键已不存在,拿不到 = 横杠
  String _ttlLabel(AppLocalizations l, KvKeyMeta? meta) {
    final ttl = meta?.ttlSeconds;
    if (ttl == null) return _missing;
    if (ttl == -1) return l.kvTtlNone;
    if (ttl <= 0) return _missing;
    final d = ttl ~/ 86400;
    final h = (ttl % 86400) ~/ 3600;
    final m = (ttl % 3600) ~/ 60;
    final s = ttl % 60;
    final clock = '${h.toString().padLeft(2, '0')}:'
        '${m.toString().padLeft(2, '0')}:${s.toString().padLeft(2, '0')}';
    return d == 0 ? clock : '${d}${l.kvTtlDay} $clock';
  }
}
