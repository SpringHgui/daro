import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../app/app_state.dart';
import '../data/db_data.dart';
import '../data/db_metadata.dart' show formatDetailTimestamp;
import '../data/drivers/kv_driver.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';
import 'kv_write_dialogs.dart';

/// 行高 / 单元格留白沿用数据网格的密度口径(AGENTS.md「表格密度」):
/// 22 / 6,不是 base-ui 控件默认的 28 / 12。
const double _rowHeight = 22.0;
const double _cellPaddingX = 6.0;
const double _numberColWidth = 22.0;

/// 工具条高度:base-ui 控件高 28,容器比控件矮会在 debug 下报 overflow
const double _toolbarHeight = 32.0;

/// 页脚状态条高度
const double _footerHeight = 24.0;

/// 写入失败条的红:与主题定制弹窗 / 查询页同一中调色(色板里没有语义错误色)
const Color _kErrorColor = Color(0xffd93025);

/// 一次用户动作里最多翻多少轮 SCAN。
///
/// 为什么要有上限:带 MATCH / TYPE 时命中稀疏,单轮回空页 + 非零游标
/// (见 [KvKeyPage] 注释),要一直翻才有结果。不设上限就等于在用户按下
/// 「刷新」后静默做一次全键空间遍历 —— 大实例上是几十万次往返。
/// 到顶就停手,游标留着等用户续扫:滚到底自动触发,列表短到滚不动时点兜底行。
const int _maxRoundsPerAction = 40;

/// 距底部不足这么多行就提前续扫,不等用户真滚到底(滚轮一格也能触发)
const int _autoLoadLeadRows = 8;

/// 键网格表头的高度估值(base-ui 按 tokens 算,这里只用来判「撑没撑满视口」)。
/// 估小了只会让兜底行早出现一格,不会和自动续扫撞车。
const double _gridHeaderGuess = 26.0;

/// 每轮 SCAN 的 COUNT 提示值(服务端可以返回更少或略多)
const int _scanCountHint = 200;

/// 目录里取不到属性时的占位符:纯标点,三语通用,不进 ARB
const String _missing = '--';

/// 键列表的一种列。列集合随驱动能力表变(不支持 TTL 就没有 TTL 列),
/// 故单元格取数按「第几列是什么」而不是按固定下标。
enum _KeyCol { name, type, ttl, encoding, bytes }

/// Delete / Ctrl+C 归哪一侧:两个网格各自有选区,不能凭猜。
enum _Pane { keys, value }

/// 过滤词输入 → SCAN 的 MATCH 模式;空输入返回 null(= 不过滤)。
///
/// 界面口径是**包含匹配**:SCAN MATCH 吃的是 glob,裸 `user` 只会命中键名恰好
/// 等于 `user` 的那一个,而用户敲它想要的是「名字里有 user」。没写通配符就自动
/// 包成 `*user*`。
/// 已含 `*` / `?` / 反斜杠转义时按原样发,让想要精确前缀(`scan:*`)或
/// 搜字面星号(`foo\*bar`)的用户仍能表达。
KvBytes? kvKeyPatternFromInput(String raw) {
  final text = raw.trim();
  if (text.isEmpty) return null;
  final isGlob = text.contains(RegExp(r'[*?\\]'));
  return KvBytes.text(isGlob ? text : '*$text*');
}

/// 键值 / 缓存型引擎(Redis 等)的键浏览页。
///
/// 上半是键列表(游标分页 + glob 过滤 + 类型过滤),下半是选中键的值网格。
/// 与 [TableDataPage] 的根本差别:这里没有「一条 SQL 拿全表」——
/// 键只能按游标一页页扫,值只能按容器类型分形状读,大字符串还要按窗口取。
///
/// 写入口一律按驱动 [KvCapabilities.writableTypes] 显隐:不支持的类型连按钮都不出现,
/// 而**拒绝**一次编辑必定说明原因(流只读 / 引擎只读 / 值没读全 / 下标是位置),
/// 不给「点了没反应」的门。流是追加型日志,改历史条目不是常规操作,保持只读。
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
  final FocusNode _pageFocus = FocusNode();

  /// 过滤框的焦点锚点:Ctrl+F 要把焦点送进去、Esc 要把它交还给列表,
  /// 而 Input 内部自建的焦点节点宿主取不到(见 [_focusInTextField] 的注释),
  /// 只能由页面自己持有一个传进去
  final FocusNode _patternFocus = FocusNode();

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

  /// 值读出的时刻:页脚要按「当时的剩余秒数」换算「过期于几点」,
  /// 拿扫描时的旧 TTL 算会显示成一个已经过去的时刻
  DateTime? _valueReadAt;

  /// 字符串值的多行视图。网格一行只有 22px 且末尾省略号,缩进后的 JSON 与
  /// 带空格的十六进制根本放不下,所以 string 类型不走 DataGridView,
  /// 改用这个编辑框承载「文本 / JSON / 十六进制」三种格式。
  final TextEditingController _stringCtrl = TextEditingController();

  /// 当前视图渲染出来的文本与「格式套不上」的原因(页脚要说清)
  KvValueView? _stringView;

  /// 视图里的文本与读出来的值不一致(只有「文本」格式可编辑,见 [_stringEditable])
  bool _stringDirty = false;

  /// 在途动作的代数:改过滤 / 续扫 / 换选中键都会自增,
  /// 晚到的回包据此丢弃 —— 否则旧请求的响应会把新结果盖掉
  int _scanGen = 0;
  int _valueGen = 0;

  // ────────────────────────────────────────────── 选区 / 编辑态

  /// 键网格的单元格选区(框选 / Ctrl / Shift 由组件算好,页面只存集合)。
  /// 用**单元格**而不是行:右键落在选区内时组件保持整个集合,
  /// 「删除 N 个键 / 设置过期时间」才有的是一整批而不是一行。
  Set<(int, int)> _keyCells = {};
  (int, int)? _keyAnchor;

  Set<(int, int)> _valueCells = {};
  (int, int)? _valueAnchor;

  /// 值网格里正在就地编辑的格(行, 网格列)
  (int, int)? _editing;

  /// Delete / Ctrl+C 落在哪一侧(最近一次选中动作归属的网格)
  _Pane _activePane = _Pane.keys;

  // ────────────────────────────────────────────── 写入状态

  /// 一次写只在途一份:连点确定会发两遍命令
  bool _writing = false;

  /// 服务端错误原文(可拖选复制)与上一次动作的回执
  String? _actionError;
  String? _actionNote;

  /// 「重试」要重放的**同一次**写:参数在发起时就解析成了字节 / 键名,
  /// 不重新读选区 —— 删除之后行号会位移,按行号重放等于操作另一个对象
  Future<void> Function()? _lastWrite;

  @override
  void initState() {
    super.initState();
    HardwareKeyboard.instance.addHandler(_onKey);
    _startScan();
  }

  @override
  void dispose() {
    HardwareKeyboard.instance.removeHandler(_onKey);
    _pattern.dispose();
    _stringCtrl.dispose();
    _keysScroll.dispose();
    _pageFocus.dispose();
    _patternFocus.dispose();
    super.dispose();
  }

  KvNamespace get _namespace =>
      KvNamespace(id: widget.namespaceId, label: widget.namespaceLabel);

  /// 取连接配置;连接已被删除时返回 null(界面按「不可用」渲染)
  ConnectionInfo? _connInfo(AppState app) =>
      app.connectionByName(widget.connection);

  /// 本连接引擎的能力表:优先读**在用**的驱动,连接尚未建立时才按
  /// 连接类型查静态表(能力表是引擎的固有属性,造一个驱动对象不联网)。
  /// 写入口、菜单可用性、拒绝编辑的理由都由它门控
  KvCapabilities? _caps() {
    final app = context.read<AppState>();
    final conn = _connInfo(app);
    if (conn == null) return null;
    return app.kvManager.capabilitiesOf(conn.name) ??
        createKvDriver(conn)?.capabilities;
  }

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
    _keyCells = {};
    _keyAnchor = null;
    _clearValueSelection();
    _actionError = null;
    _actionNote = null;
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
  KvBytes? _patternBytes() => kvKeyPatternFromInput(_pattern.text);

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
      _editing = null;
      _clearValueSelection();
    });
    try {
      final driver = await app.kvManager.driverFor(conn);
      final read = await driver.readValue(_namespace, key);
      if (gen != _valueGen || !mounted) return;
      setState(() {
        _value = read;
        _valueReadAt = DateTime.now();
        _syncStringView(read);
      });
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
        final next = KvValueRead(
          key: read.key,
          type: read.type,
          rows: [...current.rows, ...read.rows],
          totalLength: read.totalLength ?? current.totalLength,
          totalCount: read.totalCount ?? current.totalCount,
          truncated: read.truncated,
          // 已展示到的右缘:下一次续读从这里开始
          rangeStart: shown,
          ttlSeconds: read.ttlSeconds,
        );
        _value = next;
        _valueReadAt = DateTime.now();
        _syncStringView(next);
      });
    } on Object catch (e) {
      if (gen != _valueGen || !mounted) return;
      setState(() => _valueError = '$e');
    } finally {
      if (gen == _valueGen && mounted) setState(() => _loadingValue = false);
    }
  }

  /// 写之后重取当前键的值(不闪 spinner,不改动选中的键)。
  /// 返回新读出的值供调用方比对新旧计数。
  Future<KvValueRead?> _refreshValue() async {
    final current = _value;
    if (current == null || _loadingValue) return null;
    final app = context.read<AppState>();
    final conn = _connInfo(app);
    if (conn == null) return null;
    final gen = ++_valueGen;
    try {
      final driver = await app.kvManager.driverFor(conn);
      final read = await driver.readValue(_namespace, current.key);
      if (gen != _valueGen || !mounted) return null;
      // 保住「身份没变」的选区:重取在途时用户点了另一格,这次选中不该被
      // 一次后台刷新抹掉(表现是「删除此项」刚出现就消失)。
      // 行位移或换了类型仍然全丢 —— 按行号重放写命令等于动另一个对象。
      final keep = <(int, int)>{};
      if (current.type == read.type) {
        for (final c in _valueCells) {
          if (c.$1 < read.rows.length &&
              _rowIdentity(current, c.$1) == _rowIdentity(read, c.$1)) {
            keep.add(c);
          }
        }
      }
      setState(() {
        _value = read;
        _valueReadAt = DateTime.now();
        _syncStringView(read);
        _editing = null;
        _valueCells = keep;
        if (keep.isEmpty) {
          _valueAnchor = null;
        } else if (!keep.contains(_valueAnchor)) {
          _valueAnchor = keep.first;
        }
      });
      return read;
    } on Object catch (e) {
      if (gen != _valueGen || !mounted) return null;
      setState(() => _valueError = '$e');
      return null;
    }
  }

  /// 一行的身份:位置或成员名,**不含值**(值正是用户刚改掉的那一格)。
  /// 写后重取用它判断选区是否还指着同一个对象,见 [_refreshValue]。
  static String? _rowIdentity(KvValueRead read, int row) {
    final r = read.rows[row];
    return switch (read.type) {
      KvKeyType.list => '${r.index ?? row}',
      KvKeyType.stream => r.entryId,
      KvKeyType.set || KvKeyType.zset => r.value.display,
      _ => r.field?.display,
    };
  }

  // ────────────────────────────────────────────── 字符串值的格式视图

  /// 当前格式。走 `read` 而不是 `select`:本页面刻意不订阅 AppState
  /// (它在扫描 / 选键期间高频通知,整页重画不划算),而格式变化总是由本页
  /// 自己发起,发起时紧跟一次 setState 就够了。
  KvValueFormat get _format => context.read<AppState>().kvValueFormat;

  /// 字符串值的完整字节:续读会把后段作为**新行**追加,所以整段是各行拼接,
  /// 不是第一行(网格按行显示看不出来,视图一拼接就露馅)
  KvBytes _stringValueBytes(KvValueRead read) {
    if (read.rows.length <= 1) {
      return read.rows.isEmpty ? KvBytes(const []) : read.rows.first.value;
    }
    final out = <int>[];
    for (final r in read.rows) {
      out.addAll(r.value.bytes);
    }
    return KvBytes(out);
  }

  /// 按当前格式重渲染并灌回编辑框:视图文本永远等于「这次的渲染结果」,
  /// 用户的改动只体现在 [_stringDirty],写回后回读会再走一次这里
  void _syncStringView(KvValueRead read) {
    final view =
        kvRenderValue(_stringValueBytes(read), context.read<AppState>().kvValueFormat);
    _stringView = view;
    _stringCtrl.text = view.text;
    _stringDirty = false;
  }

  /// 只有「文本」格式可编辑:JSON 的缩进、十六进制的空格都是派生内容,
  /// 原样写回等于改了存的字节;超出渲染上限的视图更是只含前一段
  bool _stringEditable(KvValueRead read) =>
      _format == KvValueFormat.text && _editRefusal(read, 0) == null;

  /// 不可编辑的原因:格式原因(只读视图)优先,其次是能力表与截断
  String? _stringEditNote(KvValueRead read) {
    if (_format != KvValueFormat.text) return context.l10n.kvFormatReadOnly;
    return _editRefusal(read, 0);
  }

  String? _formatNoteText(AppLocalizations l, KvFormatNote note) =>
      switch (note) {
        KvFormatNote.none => null,
        KvFormatNote.notJson => l.kvFormatNotJson,
        KvFormatNote.binary => l.kvFormatBinary,
        KvFormatNote.tooBig => l.kvFormatTooBig('$kKvFormatViewBytes'),
      };

  Future<void> _saveStringView() async {
    final read = _value;
    if (read == null ||
        read.type != KvKeyType.string ||
        !_stringEditable(read) ||
        !_stringDirty ||
        _writing) {
      return;
    }
    final bytes = kvBytesFromInput(_stringCtrl.text);
    if (bytes == null) {
      setState(() => _actionError = context.l10n.kvHexInvalid);
      return;
    }
    _activePane = _Pane.value;
    await _runWrite(_cmd(
        (d) => d.setStringValue(_namespace, read.key, bytes)));
  }

  // ────────────────────────────────────────────── 选区取值

  /// 键网格的目标行:有框选就用框选涉及的行,否则退回当前选中行
  List<int> get _keyTargetRows {
    final rows = {for (final (r, _) in _keyCells) r}.toList()..sort();
    if (rows.isNotEmpty) return rows;
    final selected = _selectedKey;
    return selected == null || selected >= _keys.length ? const [] : [selected];
  }

  /// 值网格的目标行(只有单元格选区,没有「当前行」兜底)
  List<int> get _valueTargetRows {
    final rows = {for (final (r, _) in _valueCells) r}.toList()..sort();
    return rows;
  }

  List<(int, int)> get _sortedValueCells => _valueCells.toList()
    ..sort((a, b) => a.$1 != b.$1 ? a.$1.compareTo(b.$1) : a.$2.compareTo(b.$2));

  void _clearValueSelection() {
    _valueCells = {};
    _valueAnchor = null;
  }

  // ────────────────────────────────────────────── 写入:公共通道

  /// 执行一次写:在途门控、错误原文、成功回执,以及默认重载值网格。
  /// [body] 返回 null = 用通用回执「已保存」;返回文本 = 原样作为回执。
  ///
  /// 服务端错误包成文本上抛([KvServerException.toString] 就是原文),
  /// 因为 WRONGTYPE / value is not an integer 之类本身就是最有用的提示。
  Future<bool> _runWrite(
    Future<String?> Function(KvStoreDriver driver) body, {
    bool reloadValue = true,
  }) async {
    if (_writing) return false;
    final l = context.l10n;
    final app = context.read<AppState>();
    final conn = _connInfo(app);
    if (conn == null) return false;
    setState(() {
      _writing = true;
      _actionError = null;
      _actionNote = null;
    });
    try {
      final driver = await app.kvManager.driverFor(conn);
      final note = await body(driver);
      if (!mounted) return false;
      setState(() {
        _writing = false;
        _actionNote = note ?? l.kvSaveDone;
      });
      if (reloadValue) await _refreshValue();
      return true;
    } on Object catch (e) {
      if (!mounted) return false;
      setState(() {
        _writing = false;
        _actionError = '$e';
      });
      return false;
    }
  }

  /// 把「只发命令、回执用通用『已保存』」的写包成 [_runWrite] 要的形态
  Future<String?> Function(KvStoreDriver) _cmd(
          Future<void> Function(KvStoreDriver) body) =>
      (driver) async {
        await body(driver);
        return null;
      };

  Future<void> _retryLastWrite() async {
    final again = _lastWrite;
    if (again == null || _writing) return;
    await again();
  }

  /// 重新取回这些键的元数据(写之后 TTL / 字节数就变了)。
  /// 只按当前列表里的位置回写;取不到就保留旧值,不打断已经成功的写。
  Future<void> _patchKeyMeta(List<KvBytes> keys) async {
    if (keys.isEmpty) return;
    final app = context.read<AppState>();
    final conn = _connInfo(app);
    if (conn == null) return;
    final where = <KvBytes, int>{
      for (var i = 0; i < _keys.length; i++) _keys[i].key: i,
    };
    final rows = [for (final k in keys) where[k]]
        .whereType<int>()
        .where((i) => i < _keys.length)
        .toList();
    if (rows.isEmpty) return;
    try {
      final driver = await app.kvManager.driverFor(conn);
      final metas = await driver.readKeyMeta(_namespace, [
        for (final i in rows) _keys[i].key,
      ]);
      if (!mounted) return;
      setState(() {
        final next = List<KvKeyEntry>.of(_keys);
        for (var i = 0; i < rows.length; i++) {
          next[rows[i]] = KvKeyEntry(key: next[rows[i]].key, meta: metas[i]);
        }
        _keys = next;
      });
    } on Object {
      // 元数据只是锦上添花:静默保留旧值
    }
  }

  // ────────────────────────────────────────────── 写入:键

  /// 新建键。键名查重在对话框里做:服务端对已存在的键不会拒绝
  /// HSET / RPUSH,而是往里面追加 —— 不查就是「新建」偷偷改了旧键。
  Future<void> _newKey() async {
    final app = context.read<AppState>();
    final conn = _connInfo(app);
    if (conn == null) return;
    final l = context.l10n;
    final driver = await app.kvManager.driverFor(conn);
    final result = await showKvNewKeyDialog(
      context,
      initialType: KvKeyType.string,
      checkName: (name) async {
        try {
          final metas = await driver.readKeyMeta(_namespace, [
            KvBytes.text(name),
          ]);
          if (metas.isEmpty) return null;
          return metas.first.type == KvKeyType.none ? null : l.kvKeyNameTaken;
        } on Object catch (e) {
          // 查不了就不写:把原因原样画在键名行下面,让用户自己决定
          return '$e';
        }
      },
    );
    if (result == null || !mounted) return;
    await _createKey(result);
  }

  Future<void> _createKey(KvFormResult r) async {
    _lastWrite = () => _createKey(r);
    final l = context.l10n;
    final key = KvBytes.text(r.keyName ?? '');
    final value = kvBytesFromInput(r.text) ?? KvBytes.text(r.text);
    final type = r.type ?? KvKeyType.string;
    final ttl = r.ttlSeconds;
    final done = await _runWrite((d) async {
      switch (type) {
        case KvKeyType.string:
          await d.setStringValue(_namespace, key, value, ttlSeconds: ttl);
        case KvKeyType.hash:
          await d.hashWrite(_namespace, key,
              set: [(KvBytes.text(r.fieldText ?? ''), value)]);
        case KvKeyType.list:
          await d.listWrite(_namespace, key,
              prepend: r.atHead ? value : null,
              append: r.atHead ? null : value);
        case KvKeyType.set:
          await d.setWrite(_namespace, key, add: [value]);
        case KvKeyType.zset:
          await d.zsetWrite(_namespace, key, add: [(value, r.score ?? 0)]);
        default:
          return l.kvNotWritable;
      }
      // 容器类型只能先落一项再补过期时间:键在那条写之前根本不存在
      if (type != KvKeyType.string && ttl != null) {
        await d.setTtl(_namespace, [key], ttl);
      }
      return null;
    }, reloadValue: false);
    if (!done || !mounted) return;
    setState(() {
      _keys = [KvKeyEntry(key: key), ..._keys];
      _keyCells = {};
      _keyAnchor = null;
      _selectedKey = 0;
      _value = null;
      _editing = null;
      _clearValueSelection();
    });
    await _patchKeyMeta([key]);
    if (mounted) await _loadValue(0);
  }

  Future<void> _deleteKeys(List<int> rows) async {
    if (rows.isEmpty) return;
    final l = context.l10n;
    final keys = [for (final r in rows) _keys[r].key];
    final answer = await MessageBox.show(
      context,
      title: l.btnDelete,
      message: rows.length == 1
          ? l.kvDeleteConfirmOne(keys.first.display)
          : l.kvDeleteConfirmMany('${rows.length}'),
      type: MessageBoxType.question,
      buttons: MessageBoxButtons.okCancel,
      // 不给 okText 就会露出 base-ui 的英文兜底「OK / Cancel」:三语文案在这
      // 条链路上断掉,而且确认键说的是「确定」而不是要发生的动作。
      okText: l.btnDelete,
      cancelText: l.btnCancel,
    );
    if (answer != MessageBoxResult.ok || !mounted) return;
    await _removeKeys(rows, keys);
  }

  /// 删完在本地列表里去掉这些行:服务端已经回包,没必要为确认删除再扫一遍
  Future<void> _removeKeys(List<int> rows, List<KvBytes> keys) async {
    _lastWrite = () => _removeKeys(rows, keys);
    final l = context.l10n;
    final valid = rows.where((r) => r < _keys.length).toList();
    final done = await _runWrite((d) async {
      final n = await d.deleteKeys(_namespace, keys);
      return n == keys.length ? null : l.kvWritePartial('$n', '${keys.length}');
    }, reloadValue: false);
    if (!done || !mounted) return;
    final removed = valid.toSet();
    final current = _selectedKey;
    final keepValue = current != null && !removed.contains(current);
    setState(() {
      _keys = [
        for (var i = 0; i < _keys.length; i++)
          if (!removed.contains(i)) _keys[i],
      ];
      _keyCells = {};
      _keyAnchor = null;
      if (keepValue) {
        // 删掉的是别的行:当前键没动,行号往前位移
        _selectedKey = current - removed.where((r) => r < current).length;
      } else {
        _selectedKey = null;
        _value = null;
        _valueError = null;
        _editing = null;
        _clearValueSelection();
      }
    });
  }

  Future<void> _askTtl(List<int> rows) async {
    if (rows.isEmpty) return;
    final first = _keys[rows.first].meta?.ttlSeconds;
    final seconds = await showKvTtlDialog(
      context,
      initialSeconds: (first != null && first > 0) ? first : null,
    );
    if (seconds == null || !mounted) return;
    await _setTtl([for (final r in rows) _keys[r].key], seconds);
  }

  Future<void> _clearTtl(List<int> rows) async {
    if (rows.isEmpty) return;
    await _setTtl([for (final r in rows) _keys[r].key], null);
  }

  Future<void> _setTtl(List<KvBytes> keys, int? seconds) async {
    _lastWrite = () => _setTtl(keys, seconds);
    final l = context.l10n;
    final done = await _runWrite((d) async {
      final n = await d.setTtl(_namespace, keys, seconds);
      if (n == keys.length) {
        if (seconds == null) {
          return n == 1 ? l.kvTtlClearedDoneOne : l.kvTtlClearedDone('$n');
        }
        return n == 1 ? l.kvTtlSetDoneOne : l.kvTtlSetDone('$n');
      }
      // PERSIST 对「本来就没过期时间」和「键已不存在」都回 0,不能说键没了
      if (n == 0 && seconds == null) return l.kvTtlAlreadyNone;
      return l.kvWritePartial('$n', '${keys.length}');
    });
    if (!done || !mounted) return;
    await _patchKeyMeta(keys);
  }

  // ────────────────────────────────────────────── 写入:值

  /// 值网格有没有「左边那一列」:hash 字段名 / list 下标 / stream 条目 ID /
  /// zset 分数;string 与 set 只有值一列
  bool _hasFirstCol(KvKeyType type) =>
      type == KvKeyType.hash ||
      type == KvKeyType.list ||
      type == KvKeyType.stream ||
      type == KvKeyType.zset;

  /// 能否往这个类型「添加一项」:字符串只有一个值,流只读
  bool _addable(KvKeyType type) =>
      type == KvKeyType.hash ||
      type == KvKeyType.list ||
      type == KvKeyType.set ||
      type == KvKeyType.zset;

  /// 拒绝就地编辑的原因;null = 可以编辑。**每一句都说清为什么**,
  /// 静默不响应等于让用户怀疑自己点错了
  String? _editRefusal(KvValueRead read, int col) {
    final l = context.l10n;
    if (read.type == KvKeyType.stream) return l.kvStreamReadOnly;
    final caps = _caps();
    if (caps == null || !caps.writable(read.type)) return l.kvNotWritable;
    if (read.type == KvKeyType.list && col == 0) return l.kvIndexReadOnly;
    if (read.type == KvKeyType.string && _isTruncatedString(read)) {
      return l.kvBigStringReadOnly;
    }
    return null;
  }

  void _startValueEdit(int row, int col) {
    final read = _value;
    if (read == null) return;
    final kb = HardwareKeyboard.instance;
    if (kb.isControlPressed || kb.isShiftPressed) return;
    // 已在编辑这格:本次点击只在编辑器里挪光标,不重建
    if (_editing == (row, col)) return;
    final refusal = _editRefusal(read, col);
    if (refusal != null) {
      setState(() {
        _actionNote = refusal;
        _actionError = null;
      });
      return;
    }
    setState(() => _editing = (row, col));
  }

  void _cancelValueEdit() {
    if (_editing == null) return;
    setState(() => _editing = null);
  }

  Future<void> _commitValueCell(int row, int col, String text) async {
    final read = _value;
    if (read == null) return;
    setState(() => _editing = null);
    final target = read.rows[row];
    final bytes = kvBytesFromInput(text);
    if (bytes == null) {
      setState(() => _actionError = context.l10n.kvHexInvalid);
      return;
    }
    double? score;
    if (read.type == KvKeyType.zset && col == 0) {
      score = double.tryParse(text.trim());
      if (score == null) {
        setState(() => _actionError = context.l10n.kvScoreInvalid);
        return;
      }
    }
    await _writeCell(read, row, col, target, bytes, score);
  }

  Future<void> _writeCell(KvValueRead read, int row, int col, KvValueRow target,
      KvBytes bytes, double? score) async {
    _lastWrite = () => _writeCell(read, row, col, target, bytes, score);
    final firstCol = _hasFirstCol(read.type) && col == 0;
    switch (read.type) {
      case KvKeyType.string:
        await _runWrite(_cmd(
            (d) => d.setStringValue(_namespace, read.key, bytes)));
      case KvKeyType.hash:
        if (firstCol) {
          if (bytes == target.field) return; // 没改名:不发命令,也不报「已保存」
          // 字段名改名 = 先按新名写入同一份值,再删旧字段
          await _runWrite(_cmd((d) => d.hashWrite(_namespace, read.key,
              set: [(bytes, target.value)], removeFields: [target.field!])));
        } else {
          await _runWrite(_cmd((d) => d.hashWrite(_namespace, read.key,
              set: [(target.field!, bytes)])));
        }
      case KvKeyType.list:
        if (firstCol) return; // 下标列不可编辑(见 [_editRefusal])
        await _runWrite(
            _cmd((d) => d.listWrite(_namespace, read.key, set: [(row, bytes)])));
      case KvKeyType.set:
        if (bytes == target.value) return;
        await _runWrite(_cmd((d) => d.setWrite(_namespace, read.key,
            add: [bytes], removeMembers: [target.value])));
      case KvKeyType.zset:
        if (firstCol) {
          await _runWrite(_cmd((d) => d.zsetWrite(_namespace, read.key, add: [
            (target.value, score ?? target.score ?? 0)
          ])));
        } else {
          if (bytes == target.value) return;
          await _runWrite(_cmd((d) => d.zsetWrite(_namespace, read.key, add: [
            (bytes, target.score ?? 0)
          ], removeMembers: [target.value])));
        }
      default:
        setState(() => _actionNote = context.l10n.kvNotWritable);
    }
    if (!mounted) return;
    await _patchKeyMeta([read.key]);
  }

  Future<void> _addValueItem() async {
    final read = _value;
    if (read == null) return;
    final result = await showKvAddItemDialog(
      context,
      type: read.type,
      keyName: read.key.display,
    );
    if (result == null || !mounted) return;
    await _addValueItemNow(read, result);
  }

  Future<void> _addValueItemNow(KvValueRead read, KvFormResult r) async {
    _lastWrite = () => _addValueItemNow(read, r);
    final l = context.l10n;
    final bytes = kvBytesFromInput(r.text) ?? KvBytes.text(r.text);
    // 容器值只读出一个窗口时,追加的那一项多半落在窗口外:
    // 说「已保存」而网格没变化等于骗人,把计数与可见范围一并写清楚
    final outside =
        read.truncated && !(read.type == KvKeyType.list && r.atHead);
    final done = await _runWrite((d) async {
      switch (read.type) {
        case KvKeyType.hash:
          await d.hashWrite(_namespace, read.key,
              set: [(KvBytes.text(r.fieldText ?? ''), bytes)]);
        case KvKeyType.list:
          await d.listWrite(_namespace, read.key,
              prepend: r.atHead ? bytes : null,
              append: r.atHead ? null : bytes);
        case KvKeyType.set:
          await d.setWrite(_namespace, read.key, add: [bytes]);
        case KvKeyType.zset:
          await d.zsetWrite(_namespace, read.key, add: [(bytes, r.score ?? 0)]);
        default:
          return l.kvNotWritable;
      }
      return outside
          ? l.kvAddWindowOnly(
              '${read.totalCount ?? read.rows.length}',
              '${read.rows.length}')
          : null;
    });
    if (!done || !mounted) return;
    await _patchKeyMeta([read.key]);
  }

  Future<void> _deleteValueItems(List<int> rows) async {
    final read = _value;
    if (read == null || rows.isEmpty) return;
    final l = context.l10n;
    final answer = await MessageBox.show(
      context,
      title: l.btnDelete,
      message: rows.length == 1
          ? l.kvDeleteItemConfirmOne
          : l.kvDeleteItemConfirm('${rows.length}'),
      type: MessageBoxType.question,
      buttons: MessageBoxButtons.okCancel,
      okText: l.btnDelete,
      cancelText: l.btnCancel,
    );
    if (answer != MessageBoxResult.ok || !mounted) return;
    await _removeValueItems(
        read, rows, [for (final r in rows) read.rows[r]]);
  }

  Future<void> _removeValueItems(
      KvValueRead read, List<int> rows, List<KvValueRow> targets) async {
    _lastWrite = () => _removeValueItems(read, rows, targets);
    final l = context.l10n;
    final before = read.totalCount;
    final done = await _runWrite((d) async {
      switch (read.type) {
        case KvKeyType.hash:
          await d.hashWrite(_namespace, read.key,
              removeFields: [for (final t in targets) t.field!]);
        case KvKeyType.list:
          await d.listWrite(_namespace, read.key, removeIndexes: rows);
        case KvKeyType.set:
          await d.setWrite(_namespace, read.key,
              removeMembers: [for (final t in targets) t.value]);
        case KvKeyType.zset:
          await d.zsetWrite(_namespace, read.key,
              removeMembers: [for (final t in targets) t.value]);
        default:
          return l.kvNotWritable;
      }
      return rows.length == 1 ? l.kvItemsDeletedOne : l.kvItemsDeleted('${rows.length}');
    }, reloadValue: false);
    if (!done || !mounted) return;
    final fresh = await _refreshValue();
    final after = fresh?.totalCount;
    if (after != null && before != null && before - after < rows.length) {
      if (mounted) {
        setState(() => _actionNote =
            l.kvDeletePartial('${before - after}', '${rows.length}'));
      }
    }
    if (mounted) await _patchKeyMeta([read.key]);
  }

  // ────────────────────────────────────────────── 复制

  Future<void> _copyText(String text) => Clipboard.setData(
        ClipboardData(text: text),
      );

  Future<void> _copyKeyNames(List<int> rows) async {
    if (rows.isEmpty) return;
    await _copyText([for (final r in rows) _keys[r].key.display].join('\n'));
    if (!mounted) return;
    setState(() {
      _actionNote = context.l10n.kvCopiedNames('${rows.length}');
      _actionError = null;
    });
  }

  Future<void> _copyValueCells() async {
    final read = _value;
    if (read == null || _valueCells.isEmpty) return;
    final texts = [
      for (final (r, c) in _sortedValueCells) _cellText(read, r, c),
    ];
    await _copyText(texts.join('\n'));
    if (!mounted) return;
    setState(() {
      _actionNote = context.l10n.cellsCopiedToClipboard('${texts.length}');
      _actionError = null;
    });
  }

  // ────────────────────────────────────────────── 键盘

  /// Del / Ctrl+C / Ctrl+S 全局监听:焦点必须落在本页,且不在就地编辑器里
  /// (编辑器内的 Del 删字符、Ctrl+C 复制选中文本,都不能抢)。
  /// Ctrl+F / Esc 是进出过滤框,判定在那道「焦点在文本框就不抢键」的闸门之前
  bool _onKey(KeyEvent event) {
    if (event is! KeyDownEvent) return false;
    final ctrl = HardwareKeyboard.instance.isControlPressed;
    // Ctrl+F 的落点就是过滤框本身,所以必须走在 _focusInTextField 之前,
    // 否则焦点已经在框里时再按一次等于没按;仍要求焦点在本页,
    // 免得抢走左侧树 / 其他标签页输入框的 Ctrl+F
    if (ctrl && event.logicalKey == LogicalKeyboardKey.keyF) {
      // 就地编辑器优先:它自带焦点,打断输入不如把键让出去
      if (_editing != null || !_focusInPage) return false;
      // 桌面端 Input 只在「聚焦那一刻」全选,焦点本来就在框里再按不会重选,
      // 这里补一次:已有过滤词时整句选中,直接敲新的就能改
      if (_pattern.text.isNotEmpty) {
        _pattern.selection =
            TextSelection(baseOffset: 0, extentOffset: _pattern.text.length);
      }
      _patternFocus.requestFocus();
      return true;
    }
    // Esc 离开过滤框:焦点还给列表,Del / Ctrl+C 与方向键恢复可用
    if (event.logicalKey == LogicalKeyboardKey.escape &&
        _patternFocus.hasFocus) {
      _pageFocus.requestFocus();
      return true;
    }
    if (_editing != null) return false;
    // 过滤框这类输入框拿着焦点时不抢键:Ctrl+C 该复制框里选中的文本,Del 该删字符
    if (_focusInTextField) return false;
    if (!_focusInPage) return false;
    // Ctrl+S 保存字符串视图。放在 _focusInTextField 之后、Ctrl+C 之前:
    // 焦点在值编辑框里(它是 Textarea,不是 Input,不会被那道闸门挡掉),
    // 但焦点在键名过滤框里时不抢 —— 那里 Ctrl+S 没有归属,留给页面自己。
    if (ctrl && event.logicalKey == LogicalKeyboardKey.keyS) {
      final read = _value;
      if (read == null || read.type != KvKeyType.string) return false;
      if (!_stringEditable(read) || !_stringDirty || _writing) return false;
      _saveStringView();
      return true;
    }
    if (ctrl && event.logicalKey == LogicalKeyboardKey.keyC) {
      if (_activePane == _Pane.value) {
        if (_valueCells.isEmpty) return false;
        _copyValueCells();
      } else {
        final rows = _keyTargetRows;
        if (rows.isEmpty) return false;
        _copyKeyNames(rows);
      }
      return true;
    }
    if (event.logicalKey == LogicalKeyboardKey.delete) {
      if (_writing) return true;
      if (_activePane == _Pane.value) {
        final rows = _valueTargetRows;
        if (rows.isEmpty) return false;
        _deleteValueItems(rows);
      } else {
        final rows = _keyTargetRows;
        if (rows.isEmpty) return false;
        _deleteKeys(rows);
      }
      return true;
    }
    return false;
  }

  bool get _focusInPage {
    var node = FocusManager.instance.primaryFocus;
    while (node != null) {
      if (node == _pageFocus) return true;
      node = node.parent;
    }
    return false;
  }

  /// 焦点是否落在文本输入框(过滤框)里。判定挂在 widget 而不是 FocusNode 上:
  /// base-ui 的 Input 内部自建 TextField 的焦点节点,宿主拿不到它,只能往上找祖先。
  bool get _focusInTextField {
    final ctx = FocusManager.instance.primaryFocus?.context;
    if (ctx is! Element) return false;
    return ctx.findAncestorWidgetOfExactType<Input>() != null;
  }

  /// 点中网格就把键盘焦点收到本页。
  ///
  /// `DataGridView` 自己不 requestFocus,而 Ctrl+C / Del 挂在 `HardwareKeyboard` 全局
  /// 处理器上、以 `_focusInPage` 为门槛 —— 没人请求焦点时该门槛恒假,两个快捷键在这页
  /// 从来就没通过(桌面惯例里「点中即焦点」由宿主补齐)。
  void _focusPage() {
    if (_editing != null) return; // 就地编辑器自带焦点,抢过来会打断输入
    _pageFocus.requestFocus();
  }

  // ────────────────────────────────────────────── build

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    final l = context.l10n;
    final caps = _caps();
    // 连接已删除,或引擎不支持枚举键(如 Memcached 没有 SCAN):
    // 只给一句说明,不给「点了没反应」的控件
    if (caps == null) {
      return _notice(t, l.kvConnectionGone);
    }
    if (!caps.cursorScan) return _notice(t, l.kvScanUnsupported);

    return Focus(
      focusNode: _pageFocus,
      child: ColoredBox(
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
      ),
    );
  }

  // ────────────────────────────────────────────── 上半:键列表

  Widget _keyPane(AppPalette t, KvCapabilities caps) {
    final l = context.l10n;
    final band = _actionBand(t, _Pane.keys);
    // 引擎一个类型都写不动时不给「新建键」入口(否则点了就是报错)
    final writable = caps.writableTypes.isNotEmpty;
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
                    focusNode: _patternFocus,
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
                    itemToString: (v) => v.isEmpty
                        ? l.kvTypeAll
                        : kvTypeLabel(l, kvKeyTypeOf(v)),
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
                if (writable) ...[
                  const SizedBox(width: 6),
                  Button(
                    text: l.kvActionNewKey,
                    onPressed: _writing ? null : _newKey,
                  ),
                ],
              ],
            ),
          ),
        ),
        Expanded(child: _keyList(t, caps)),
        if (band != null) band,
        _footer(t, _keySummary(l)),
      ],
    );
  }

  /// 键列表主体:滚到接近底部就自动续扫一批(取代原「加载更多」按钮)。
  ///
  /// 只在**向下的**纵向滚动通知上触发,且要求 `maxScrollExtent > 0`:
  /// 内容撑不满视口时根本滚不动,交给下面的兜底行,否则每次布局通知都会
  /// 再发起一轮扫描 —— 那正是 [_maxRoundsPerAction] 要挡住的静默全库遍历。
  Widget _keyList(AppPalette t, KvCapabilities caps) {
    final l = context.l10n;
    return LayoutBuilder(
      builder: (context, box) => Column(
        children: [
          Expanded(
            child: NotificationListener<ScrollUpdateNotification>(
              onNotification: (n) {
                final m = n.metrics;
                if (m.axis == Axis.vertical &&
                    (n.scrollDelta ?? 0) > 0 &&
                    m.maxScrollExtent > 0 &&
                    m.pixels >= m.maxScrollExtent - _autoLoadLeadRows * _rowHeight) {
                  _loadMore();
                }
                return false;
              },
              child: _keyGrid(t, caps),
            ),
          ),
          if (_canContinueManually(box.maxHeight))
            _continueRow(t, l.kvScanContinue),
        ],
      ),
    );
  }

  /// 游标没扫完、列表又短到滚不动:此时只有兜底行能续扫
  bool _canContinueManually(double viewportHeight) =>
      !_loadingKeys &&
      !_scanDone &&
      _keys.isNotEmpty &&
      _keys.length * _rowHeight < viewportHeight - _gridHeaderGuess;

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
      // 错误原文完整给出(截断的报错没法排查),重扫是旁边一个明确的按钮
      return _errorBand(t, l.kvLoadFailed(error), _startScan);
    }
    if (_keys.isEmpty) {
      final label = Text(
        _scanDone ? l.kvNoMatch : l.kvNoMatchYet,
        style: TextStyle(fontSize: 12, color: t.mutedForeground),
      );
      // 空列表滚不动,自动续扫无从触发 —— 这句说明本身就是续扫入口
      if (_scanDone) return Center(child: label);
      return GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: _loadMore,
        child: Center(child: label),
      );
    }
    final cols = _keyCols(caps);
    final rows = {for (final (r, _) in _keyCells) r};
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
        // 选区用**单元格**而不是行:组件对右键落在选区内时保持整个集合,
        // 「删除 N 个键」才有批量可言(见 [_keyCells])
        selectedCells: _keyCells.isEmpty ? null : _keyCells,
        selectedRows: rows.isEmpty ? null : rows,
        anchorCell: _keyAnchor,
        onCellsSelected: _onKeyCellsSelected,
        // 行号槽点整行:取值 + 选区退回单行
        onRowSelected: _onKeyRowSelected,
        // 框选模式下单击由组件在抬起时补发(拖出的选区不算点击)
        onCellTap: _onKeyCellTap,
        onCellContext: _onKeyContext,
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
            _KeyCol.type => Text(kvTypeLabel(l, meta?.type), style: style),
            _KeyCol.ttl => Text(_ttlLabel(l, meta), style: style),
            _KeyCol.encoding => Text(meta?.encoding ?? _missing, style: style),
            _KeyCol.bytes =>
              Text(meta?.byteSize?.toString() ?? _missing, style: style),
          };
        },
      ),
    );
  }

  void _onKeyCellsSelected(Set<(int, int)> cells) {
    _activePane = _Pane.keys;
    _focusPage();
    final kb = HardwareKeyboard.instance;
    final anchor = kb.isShiftPressed
        ? (_keyAnchor ?? (cells.isEmpty ? null : cells.first))
        : (cells.length == 1
            ? cells.first
            : _changedCell(_keyCells, cells) ?? _keyAnchor ??
                (cells.isEmpty ? null : cells.first));
    if (cells.isEmpty) {
      if (_keyCells.isEmpty && _keyAnchor == null) return;
      setState(() {
        _keyCells = {};
        _keyAnchor = null;
      });
      return;
    }
    setState(() {
      _keyCells = cells;
      _keyAnchor = anchor;
      if (cells.length > 1) _editing = null;
    });
  }

  void _onKeyRowSelected(int row) {
    _activePane = _Pane.keys;
    _focusPage();
    final kb = HardwareKeyboard.instance;
    if (kb.isControlPressed || kb.isShiftPressed) return;
    setState(() {
      _keyCells = {};
      _keyAnchor = null;
    });
    _loadValue(row);
  }

  void _onKeyCellTap(int row, int col) {
    _activePane = _Pane.keys;
    _focusPage();
    final kb = HardwareKeyboard.instance;
    if (kb.isControlPressed || kb.isShiftPressed) return;
    if (_selectedKey == row && _value != null) return;
    _loadValue(row);
  }

  /// Ctrl 加选 / 去选时从新旧集合之差里取出刚点的那格;变化超过一格(框选)返回 null
  (int, int)? _changedCell(Set<(int, int)> before, Set<(int, int)> next) {
    final added = next.difference(before);
    if (added.length == 1) return added.first;
    final removed = before.difference(next);
    if (removed.length == 1) return removed.first;
    return null;
  }

  void _onKeyContext(int row, int col, Offset position) {
    final rows = _keyTargetRows;
    showContextMenu(context, items: _keyMenu(rows), position: position);
  }

  List<MenuModel> _keyMenu(List<int> rows) {
    final l = context.l10n;
    final n = rows.length;
    final hasTtl = rows.any((r) => _keys[r].meta?.hasTtl ?? false);
    return [
      MenuItem(
          text: l.kvActionNewKey, enabled: !_writing, onPressed: _newKey),
      MenuItem(
          text: l.kvMenuCopyKeyName,
          shortcut: 'Ctrl+C',
          onPressed: () => _copyKeyNames(rows)),
      const MenuSeparator(),
      MenuItem(
          text: l.kvActionSetTtl,
          enabled: !_writing,
          onPressed: () => _askTtl(rows)),
      // 一个有过期时间的键都没有:「清除过期时间」就没有可说的,不给入口
      if (hasTtl)
        MenuItem(
            text: l.kvActionClearTtl,
            enabled: !_writing,
            onPressed: () => _clearTtl(rows)),
      MenuItem(
          text: n == 1 ? l.kvMenuDeleteKeyOne : l.kvMenuDeleteKeys('$n'),
          shortcut: 'Del',
          enabled: !_writing,
          onPressed: () => _deleteKeys(rows)),
    ];
  }

  // ────────────────────────────────────────────── 下半:值

  Widget _valuePane(AppPalette t) {
    final l = context.l10n;
    final band = _actionBand(t, _Pane.value);
    if (_loadingValue) return const Center(child: Spinner(size: 18));
    final error = _valueError;
    if (error != null) {
      final selected = _selectedKey;
      return Column(
        children: [
          Expanded(
            child: _errorBand(
              t,
              l.kvValueFailed(error),
              () {
                if (selected != null) _loadValue(selected);
              },
            ),
          ),
          if (band != null) band,
        ],
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
    final toolbar = _valueToolbar(t, read);
    return Column(
      children: [
        if (toolbar != null) toolbar,
        Expanded(
          child: read.type == KvKeyType.string
              ? _stringViewer(t, read)
              : _valueGrid(t, read),
        ),
        if (band != null) band,
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

  /// 值工具条:字符串给「格式 + 保存」,容器给「添加 / 删除」。容器那条要求
  /// 能力表允许写且类型能追加(字符串没有「加一项」这回事,流与只读引擎
  /// 整条都不出现 —— 能力表不允许的类型不给入口)
  Widget? _valueToolbar(AppPalette t, KvValueRead read) {
    if (read.type == KvKeyType.string) return _stringToolbar(t, read);
    final l = context.l10n;
    final caps = _caps();
    if (caps == null || !caps.writable(read.type) || !_addable(read.type)) {
      return null;
    }
    final rows = _valueTargetRows;
    return SizedBox(
      height: _toolbarHeight,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
        child: Row(
          children: [
            Button(
              text: l.kvActionAdd,
              onPressed: _writing ? null : _addValueItem,
            ),
            const SizedBox(width: 6),
            if (rows.isNotEmpty)
              Button(
                // 文案随选区变化:让用户按下去之前就知道这次动的是几项
                text: rows.length == 1
                    ? l.kvMenuDeleteItemOne
                    : l.kvMenuDeleteItems('${rows.length}'),
                onPressed: _writing ? null : () => _deleteValueItems(rows),
              ),
          ],
        ),
      ),
    );
  }

  /// 字符串值的工具条:格式下拉 + 保存。
  /// 保存只在「文本」格式且内容有改动时可点 —— 格式化视图不可编辑,
  /// 给一个永远灰着的按钮比给一个「点了不生效」的入口好(AGENTS.md 方言门控同源)
  Widget _stringToolbar(AppPalette t, KvValueRead read) {
    final l = context.l10n;
    final app = context.read<AppState>();
    return SizedBox(
      height: _toolbarHeight,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
        child: Row(
          children: [
            SizedBox(
              width: 112,
              child: ComboBox<KvValueFormat>(
                items: const [
                  KvValueFormat.text,
                  KvValueFormat.json,
                  KvValueFormat.hex,
                ],
                value: _format,
                itemToString: (v) => switch (v) {
                  KvValueFormat.text => l.kvFormatText,
                  KvValueFormat.json => l.kvFormatJson,
                  KvValueFormat.hex => l.kvFormatHex,
                },
                onChanged: (v) {
                  if (v == null) return;
                  app.setKvValueFormat(v);
                  final current = _value;
                  if (current != null) setState(() => _syncStringView(current));
                },
              ),
            ),
            const SizedBox(width: 6),
            Button(
              text: l.btnSave,
              onPressed:
                  _stringEditable(read) && _stringDirty && !_writing
                      ? _saveStringView
                      : null,
            ),
          ],
        ),
      ),
    );
  }

  /// 多行视图:格式渲染出的文本直接进编辑框,滚不动了自然滚动
  /// (与网格同一套等宽字号,切换格式时字不跳)
  Widget _stringViewer(AppPalette t, KvValueRead read) {
    final editable = _stringEditable(read);
    final tokens = t.desktopTokensFor(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(6, 2, 6, 6),
      child: Textarea(
        controller: _stringCtrl,
        // 不可编辑用 readOnly 而不是 enabled: false —— 后者会整块忽略指针,
        // 格式化出来的 JSON 就成了「看得见却选不中」(与报错原文同一要求)
        readOnly: !editable,
        expands: true,
        showBorder: true,
        tokens: tokens,
        style: TextStyle(
          fontFamily: 'Consolas',
          fontFamilyFallback: const ['monospace'],
          fontSize: 12.5,
          height: 1.45,
          decoration: TextDecoration.none,
          fontWeight: FontWeight.w400,
          color: tokens.foregroundColor,
        ),
        onChanged: editable
            ? (v) => setState(() => _stringDirty = v != _stringView?.text)
            : null,
      ),
    );
  }

  Widget _valueGrid(AppPalette t, KvValueRead read) {
    final l = context.l10n;
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
        selectedCells: _valueCells.isEmpty ? null : _valueCells,
        anchorCell: _valueAnchor,
        onCellsSelected: _onValueCellsSelected,
        onCellTap: _startValueEdit,
        onCellContext: _onValueContext,
        editingCell: _editing,
        headerColor: t.secondary,
        headerFontSize: 12,
        gridLineColor: t.gridLine,
        rowHoverColor: Color.alphaBlend(
          t.foreground.withValues(alpha: 0.06),
          t.background,
        ),
        tokens: t.desktopTokensFor(context),
        cellBuilder: (row, col) {
          // 就地编辑器:Enter / 失焦提交,Esc 取消(与表数据页同一交互)
          if (_editing == (row, col)) {
            final numeric = read.type == KvKeyType.list ||
                (read.type == KvKeyType.zset && col == 0);
            return InlineEditor(
              // 单元格元素会被列表回收复用,初值只在建 State 时读一次:
              // 不带 key 会从别的格子带着上一格的文本过来
              key: ValueKey('kv-cell:$row:$col'),
              initialValue: _cellText(read, row, col),
              onCommit: (value) => _commitValueCell(row, col, value),
              onCancel: _cancelValueEdit,
              height: _rowHeight,
              // 编辑态与显示态同一水平留白、同一对齐方向,进编辑不跳位
              contentPadding:
                  const EdgeInsets.symmetric(horizontal: _cellPaddingX),
              textAlign: numeric ? TextAlign.end : TextAlign.start,
            );
          }
          return Text(
            _cellText(read, row, col),
            style: _cellStyle(),
            overflow: TextOverflow.ellipsis,
            maxLines: 1,
          );
        },
      ),
    );
  }

  /// 一格的显示 / 编辑文本:与网格取数同一处口径,
  /// 复制、编辑器初值、显示三处才不会各说各话
  String _cellText(KvValueRead read, int row, int col) {
    final r = read.rows[row];
    if (!_hasFirstCol(read.type) || col > 0) return r.value.display;
    return switch (read.type) {
      KvKeyType.zset => r.score?.toString() ?? _missing,
      KvKeyType.list => '${r.index ?? _missing}',
      _ => r.entryId ?? r.field?.display ?? _missing,
    };
  }

  void _onValueCellsSelected(Set<(int, int)> cells) {
    _activePane = _Pane.value;
    _focusPage();
    final kb = HardwareKeyboard.instance;
    final anchor = kb.isShiftPressed
        ? (_valueAnchor ?? (cells.isEmpty ? null : cells.first))
        : (cells.length == 1
            ? cells.first
            : _changedCell(_valueCells, cells) ?? _valueAnchor ??
                (cells.isEmpty ? null : cells.first));
    if (cells.isEmpty) {
      if (_valueCells.isEmpty && _valueAnchor == null) return;
      setState(() {
        _valueCells = {};
        _valueAnchor = null;
      });
      return;
    }
    setState(() {
      _valueCells = cells;
      _valueAnchor = anchor;
      // 就地编辑器与框选互斥:留着它,拖出的选区会和抢焦点的输入框打架
      if (cells.length > 1) _editing = null;
    });
  }

  void _onValueContext(int row, int col, Offset position) {
    // 编辑态:右键交给编辑器自己的文本菜单(撤销 / 剪切 / 粘贴),
    // 再弹单元格菜单会两层叠在一起
    if (_editing == (row, col)) return;
    showContextMenu(context,
        items: _valueMenu(row, col), position: position);
  }

  List<MenuModel> _valueMenu(int row, int col) {
    final read = _value;
    if (read == null) return const [];
    final l = context.l10n;
    final caps = _caps();
    final writable = caps != null && caps.writable(read.type);
    final rows = _valueTargetRows;
    final batch = _valueCells.length > 1 && _valueCells.contains((row, col));
    final n = batch ? rows.length : 1;
    return [
      MenuItem(
          text: l.kvMenuEdit,
          enabled: !_writing && writable,
          onPressed: () => _startValueEdit(row, col)),
      MenuItem(
          text: l.kvMenuCopyValue,
          shortcut: 'Ctrl+C',
          onPressed: () {
            if (batch) {
              _copyValueCells();
            } else {
              _copyText(_cellText(read, row, col));
            }
          }),
      if (_addable(read.type)) ...[
        const MenuSeparator(),
        MenuItem(
            text: l.kvActionAdd,
            enabled: !_writing && writable,
            onPressed: _addValueItem),
        MenuItem(
            text: n == 1 ? l.kvMenuDeleteItemOne : l.kvMenuDeleteItems('$n'),
            shortcut: 'Del',
            enabled: !_writing && writable,
            onPressed: () => _deleteValueItems(batch ? rows : [row])),
      ],
    ];
  }

  String _valueSummary(AppLocalizations l, KvValueRead read) {
    final label = kvTypeLabel(l, read.type);
    final size = switch (read.type) {
      KvKeyType.string => l.kvValueBytes('${read.totalLength ?? 0}'),
      _ => l.kvValueCount('${read.totalCount ?? read.rows.length}'),
    };
    final parts = [label, size];
    if (_isTruncatedString(read)) {
      parts.add(l.kvValueTruncated('${_previewBytes(read)}',
          '${read.totalLength ?? 0}'));
    }
    // 字符串走格式视图:套不上的原因与「为什么不能打字」都只在这里说,
    // 只给一个灰框不解释,用户只会以为页面坏了
    if (read.type == KvKeyType.string) {
      final note = _formatNoteText(l, _stringView?.note ?? KvFormatNote.none);
      if (note != null) parts.add(note);
      if (!_stringEditable(read)) {
        final why = _stringEditNote(read);
        if (why != null) parts.add(why);
      }
    }
    // 键的过期时间只在值面板按「过期于几点」说一次:列表那列 TTL 是扫页时
    // 批量读的,隔几分钟再看就成了过去的时刻
    final expires = _expiresAt(l, read);
    if (expires != null) parts.add(expires);
    return parts.join(' · ');
  }

  /// 「过期于 …」:用取值当时的剩余秒数换算,而不是列表里的旧 TTL
  String? _expiresAt(AppLocalizations l, KvValueRead read) {
    final ttl = read.ttlSeconds;
    final at = _valueReadAt;
    if (ttl == null || ttl <= 0 || at == null) return null;
    final text = formatDetailTimestamp(at.add(Duration(seconds: ttl)));
    return text == null ? null : l.kvExpiresAt(text);
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

  /// 上一次动作的回执条:失败给原文 + 重试,成功 / 说明给一行可拖选复制的文本。
  ///
  /// 只在**发起动作那一侧**([_activePane])出现:这条上下两个 pane 是同一段文本的
  /// 两个渲染点,不门控就会一屏刷两遍同样的「已保存」。
  /// 两者都没有时返回 null(不占高度)
  Widget? _actionBand(AppPalette t, _Pane pane) {
    if (pane != _activePane) return null;
    final error = _actionError;
    if (error != null) {
      return _errorBand(t, context.l10n.kvWriteFailed(error), _retryLastWrite);
    }
    final note = _actionNote;
    if (note == null) return null;
    return Padding(
      padding: const EdgeInsets.fromLTRB(6, 3, 6, 3),
      child: Align(
        alignment: Alignment.centerLeft,
        child: SelectableText(
          note,
          style: TextStyle(
              fontSize: 11.5,
              color: t.mutedForeground,
              decoration: TextDecoration.none,
              fontWeight: FontWeight.w400),
        ),
      ),
    );
  }

  /// 错误条:原文整段可拖选复制(截断的报错没法排查),重试是旁边一个明确的按钮
  Widget _errorBand(AppPalette t, String text, VoidCallback onRetry) {
    final l = context.l10n;
    return Padding(
      padding: const EdgeInsets.fromLTRB(6, 4, 6, 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: SelectableText(
              text,
              style: TextStyle(
                  fontSize: 11.5,
                  color: _kErrorColor,
                  decoration: TextDecoration.none,
                  fontWeight: FontWeight.w400),
            ),
          ),
          const SizedBox(width: 6),
          Button(
            text: l.btnRetry,
            onPressed: _writing ? null : onRetry,
          ),
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

  /// 兜底续扫行:整行可点,只在列表短到滚不动时出现(见 [_canContinueManually])
  Widget _continueRow(AppPalette t, String text) {
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: _loadMore,
      child: SizedBox(
        height: _footerHeight,
        child: Center(
          child: Text(text,
              style: TextStyle(fontSize: 11.5, color: t.mutedForeground,
                  decoration: TextDecoration.none)),
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
