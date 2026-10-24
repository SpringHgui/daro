import 'dart:convert';

import '../db_data.dart';
import 'redis_kv_driver.dart';

/// 键值 / 缓存类引擎(Redis、Memcached 及同类中间件)的驱动抽象。
///
/// 为什么不复用 [DatabaseDriver]:那个接口是 52 个方法的 SQL 形状 ——
/// 表 / 列 / 主键 / DDL / 用户 / 例程 / 事务。实测 MongoDriver 只实现了其中
/// 36 个、其中 9 个是「未实现即抛」的桩;缓存引擎再套一层只会让桩比实现多。
/// 本抽象只按**命名空间 + 键 + 字节值**建模,一条 SQL 概念都不进来。
///
/// 三条贯穿全接口的前提(都来自真 Redis 实测,见 .redis_probe/SELECTION.md):
///
/// 1. **键和值都是字节,不是字符串。** 值可能是证书、图片、`bit` 打包字节,
///    键名同样可以是任意字节(`\xff\xfe…:binname` 实测可 SET 可 GET)。
///    因此 [KvBytes] 是接口里的唯一数据载体:能 UTF-8 解码就按文本展示,
///    解不动就 `isBinary` + HEX,口径与 AGENTS.md「裸字节列」一致。
/// 2. **枚举靠游标,不用 KEYS。** [scanKeys] 是唯一的列键入口,
///    且[displayCounts 禁令]要求计数走 [keyCount](DBSIZE / stats),不数键。
/// 3. **服务端错误 ≠ 连接坏了。** 前者([KvServerException])连接可继续复用
///    (实测 WRONGTYPE 之后 PING 仍 PONG),后者([KvConnectionException])
///    必须整条连接作废重连 —— 有的客户端一旦解码失败会把连接永久打爆。
abstract class KvStoreDriver {
  const KvStoreDriver();

  /// 本引擎的能力表。UI 按它决定控件显隐 / 置灰,不做「能点但没效果」的入口。
  KvCapabilities get capabilities;

  bool get isConnected;

  Future<void> connect();
  Future<void> close();

  /// 命名空间列表:Redis = db0..db(N-1);Memcached = 单槽位(见 [KvCapabilities.multipleNamespaces])。
  Future<List<KvNamespace>> listNamespaces();

  /// 键总数:**只允许走目录 / stats**(Redis 的 DBSIZE、Memcached 的 cached_keys)。
  /// 取不到返回 null,界面显示横杠,绝不为了显示数字去扫键空间。
  Future<int?> keyCount(KvNamespace namespace);

  /// 游标扫描。首版 UI 的键列表只调这一个方法。
  Future<KvKeyPage> scanKeys(KvKeyScan request);

  /// 批量读键元数据(类型 / TTL / 编码 / 字节数)。
  /// 实现应当一次往返取完(流水线),不要在循环里逐键 await。
  Future<List<KvKeyMeta>> readKeyMeta(KvNamespace namespace, List<KvBytes> keys);

  /// 读一个键的值。容器类型按 [KvValueRead] 的行返回,字符串按字节区间取,
  /// 大值不至于一次灌进内存。
  Future<KvValueRead> readValue(
    KvNamespace namespace,
    KvBytes key, {
    int offset = 0,
    int? limit,
  });

  /// 命令控制台:把用户输入的原生命令按参数表发出,回包以 [KvReply] 结构交给
  /// 上层渲染。[namespace] 决定这条命令落在哪个库(控制台标签本来就绑库)。
  /// 驱动**不在这里做只读判断** —— 门控在调用方(UI / 策略层),
  /// 因为「哪些命令算写」是产品口径,不是协议口径。
  Future<KvReply> runCommand(KvNamespace namespace, List<String> arguments);

  /// 服务端概览(INFO 一类),用于连接节点详情;没有则返回 null。
  Future<KvServerInfo?> readServerInfo();
}

/// 值 / 键名字节载体:UTF-8 可解 = 文本,否则按 HEX 展示。
class KvBytes {
  /// [bytes] 按引用保存,调用方不应再改动它(网格缓存依赖这一点)
  KvBytes(this.bytes);

  /// 便捷构造:文本键名 / 字段名(UI 传来的过滤词等)
  KvBytes.text(String s) : bytes = utf8.encode(s);

  final List<int> bytes;

  /// 能按 UTF-8 解码的原文;解不动返回 null(不是空串 —— 区分「空值」与「二进制值」)
  String? get text {
    if (!_textRead) {
      _textRead = true;
      try {
        _text = utf8.decode(bytes);
      } on FormatException {
        _text = null;
      }
    }
    return _text;
  }

  String? _text;
  bool _textRead = false;

  /// 非 UTF-8 文本 = 二进制:与 MySQL 裸字节列同一判据
  bool get isBinary => text == null && bytes.isNotEmpty;

  /// 十六进制大写(无分隔),回写 / 比对用
  String get hex => bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join().toUpperCase();

  /// 展示串:文本原样,二进制给 `HEX:...`
  String get display => isBinary ? 'HEX:$hex' : (text ?? '');

  int get byteLength => bytes.length;

  @override
  bool operator ==(Object other) {
    if (other is! KvBytes) return false;
    if (other.bytes.length != bytes.length) return false;
    for (var i = 0; i < bytes.length; i++) {
      if (other.bytes[i] != bytes[i]) return false;
    }
    return true;
  }

  @override
  int get hashCode => Object.hashAll(bytes);

  @override
  String toString() => 'KvBytes(${display.length > 40 ? '${display.substring(0, 40)}…' : display})';
}

/// 键的容器类型。[none] = 键不存在,[unknown] = 引擎返回了不认识的类型名。
enum KvKeyType { string, list, set, zset, hash, stream, none, unknown }

KvKeyType kvKeyTypeOf(String raw) => switch (raw) {
      'string' => KvKeyType.string,
      'list' => KvKeyType.list,
      'set' => KvKeyType.set,
      'zset' => KvKeyType.zset,
      'hash' => KvKeyType.hash,
      'stream' => KvKeyType.stream,
      'none' => KvKeyType.none,
      _ => KvKeyType.unknown,
    };

/// 命名空间:Redis 用库序号('0'..'15')作 [id],Memcached 用固定单槽。
class KvNamespace {
  const KvNamespace({required this.id, required this.label, this.keyCount});

  final String id;

  /// 树与标签上显示的名字(Redis = "db0",Memcached = "memcached")
  final String label;

  /// 打开时就已知的键数(可空,不为了显示而扫)
  final int? keyCount;

  @override
  String toString() => label;
}

/// 一次游标扫描的请求。
class KvKeyScan {
  const KvKeyScan({
    required this.namespace,
    this.cursor,
    this.pattern,
    this.type,
    this.count = 100,
  });

  final KvNamespace namespace;

  /// null = 从头开始;驱动的游标口径(字符串 / 整数)不外泄
  final String? cursor;

  /// glob 模式,如 `scan:bench:*`;null = 不过滤
  final KvBytes? pattern;

  /// 按容器类型过滤('string' / 'hash' / …);null = 全部
  final String? type;

  /// 每页期望条数(提示,不是上限)
  final int count;
}

/// 扫描结果一页:[entries] 数量可以少于请求的 [KvKeyScan.count],
/// 因为 SCAN 语义是「至少遍历,返回可能更少」。
///
/// **带 MATCH / TYPE 过滤时本页可能是空的而 [done] 仍是 false** —— 游标只表示
/// 「扫到哪儿了」,服务端会在这一轮里把不匹配的键直接跳过(实测:2025 键里只有
/// 1 个 hash,`SCAN 0 COUNT 200 TYPE hash` 回游标 744 + 空数组)。
/// 所以「空页」绝不等于「没有匹配项」:调用方必须一直翻到 [done] 才能下结论,
/// 界面若在空页上停手就会显示成「查无此键」。
class KvKeyPage {
  const KvKeyPage({
    required this.entries,
    required this.nextCursor,
    this.totalEstimate,
  });

  final List<KvKeyEntry> entries;

  /// null = 已遍历完(引擎的「游标归零」);非空 = 续扫用
  final String? nextCursor;

  /// 键总数估算(DBSIZE),与 [KvStoreDriver.keyCount] 同源;不靠累加本页得出
  final int? totalEstimate;

  bool get done => nextCursor == null;
}

/// 列表里的一个键(元数据可能尚未取全,故各字段可空)。
class KvKeyEntry {
  const KvKeyEntry({required this.key, this.meta});

  final KvBytes key;
  final KvKeyMeta? meta;
}

/// 键元数据。取不到的项一律 null,界面显示横杠而不是 0。
class KvKeyMeta {
  const KvKeyMeta({
    required this.type,
    this.ttlSeconds,
    this.ttlMillis,
    this.encoding,
    this.byteSize,
    this.idleSeconds,
  });

  final KvKeyType type;

  /// -1 = 永不过期,-2 = 键不存在(实测口径);null = 引擎不给
  final int? ttlSeconds;
  final int? ttlMillis;

  /// 服务端内部编码(listpack / quicklist / skiplist / embstr / raw …)
  final String? encoding;

  /// 该键占用的字节数(MEMORY USAGE);不支持时为 null
  final int? byteSize;

  /// 空闲秒数;需要服务端开启相应 maxmemory-policy,拿不到就是 null
  final int? idleSeconds;

  /// 有 TTL(过期时间已知)
  bool get hasTtl => (ttlSeconds ?? -1) > 0;
}

/// 值读取结果。行结构按 [type] 变,但**永远是这张表**,UI 一套网格吃到底。
///
/// | type   | field(左列)   | value(右列)      | score | entryId |
/// | ------ | ------------- | ---------------- | ----- | ------- |
/// | string | null          | 整段值            | —     | —       |
/// | hash   | 字段名         | 字段值            | —     | —       |
/// | list   | 下标(字符串)  | 元素              | —     | —       |
/// | set    | null          | 成员              | —     | —       |
/// | zset   | null          | 成员              | 分数  | —       |
/// | stream | 条目 ID        | `f=v; f=v` 拼接   | —     | 条目 ID |
class KvValueRead {
  const KvValueRead({
    required this.key,
    required this.type,
    required this.rows,
    this.totalLength,
    this.totalCount,
    this.truncated = false,
    this.rangeStart,
  });

  final KvBytes key;
  final KvKeyType type;
  final List<KvValueRow> rows;

  /// 值的总字节数(字符串 STRLEN / 容器元素数);null = 未取
  final int? totalLength;

  /// 容器元素总数(HLEN/LLEN/SCARD/ZCARD/XLEN);字符串用 [totalLength]
  final int? totalCount;

  /// 本次只取了一部分(受 [KvStoreDriver.readValue] 的 limit 或长度上限影响)
  final bool truncated;

  /// 字符串值的起始偏移(非字符串为 null)
  final int? rangeStart;
}

class KvValueRow {
  const KvValueRow({
    this.field,
    required this.value,
    this.score,
    this.entryId,
    this.index,
  });

  /// hash 字段名 / list 下标文本;字符串、set、zset 为 null
  final KvBytes? field;

  final KvBytes value;

  /// zset 分数:服务端发的是 bulk 字符串('-3' / '92.5'),这里已转 double;
  /// 转不动(非数值)时保留 null 并按文本展示。
  final double? score;

  /// stream 条目 ID
  final String? entryId;

  /// list 下标
  final int? index;
}

/// 控制台回包:RESP 的类型树。渲染成文本行由上层决定,这里只保结构。
class KvReply {
  const KvReply._(this.kind, {this.text, this.integer, this.bytes, this.items, this.isError = false});

  factory KvReply.simple(String s) => KvReply._(KvReplyKind.simple, text: s);
  factory KvReply.bulk(KvBytes b) => KvReply._(KvReplyKind.bulk, bytes: b);
  factory KvReply.integer(int i) => KvReply._(KvReplyKind.integer, integer: i);
  factory KvReply.array(List<KvReply> items) => KvReply._(KvReplyKind.array, items: items);
  factory KvReply.nil() => KvReply._(KvReplyKind.nil);
  factory KvReply.error(String message) =>
      KvReply._(KvReplyKind.error, text: message, isError: true);

  final KvReplyKind kind;
  final String? text;
  final int? integer;
  final KvBytes? bytes;
  final List<KvReply>? items;
  final bool isError;

  /// 单行摘要:控制台日志与错误提示用(INFO 这类多行 bulk 用 [render] 展开)
  String get summary => switch (kind) {
        KvReplyKind.nil => '(nil)',
        KvReplyKind.integer => '$integer',
        KvReplyKind.simple => text ?? '',
        KvReplyKind.error => 'ERR ${text ?? ''}',
        KvReplyKind.bulk => bytes?.display ?? '(nil)',
        KvReplyKind.array =>
          '[${(items ?? []).map((e) => e.summary).join(', ')}]',
      };

  /// 展开为多行文本:数组按层级缩进 2 空格,bulk 里的 `\n` 原样分行。
  /// 深度上限纯粹是防环与防刷屏,不是功能开关。
  List<String> render({int maxDepth = 6}) {
    final out = <String>[];
    void walk(KvReply r, int depth) {
      if (depth > maxDepth) {
        out.add('${'  ' * depth}…(层级过深)');
        return;
      }
      switch (r.kind) {
        case KvReplyKind.array:
          final items = r.items ?? const [];
          if (items.isEmpty) {
            out.add('${'  ' * depth}(空数组)');
            return;
          }
          for (final e in items) {
            walk(e, depth + 1);
          }
        case KvReplyKind.bulk:
          final t = r.bytes;
          if (t == null) {
            out.add('${'  ' * depth}(nil)');
          } else if (t.isBinary) {
            out.add('${'  ' * depth}${t.display}');
          } else {
            for (final line in (t.text ?? '').split('\n')) {
              out.add('${'  ' * depth}$line');
            }
          }
        default:
          out.add('${'  ' * depth}${r.summary}');
      }
    }

    if (kind == KvReplyKind.array) {
      walk(this, 0);
    } else {
      for (final line in summary.split('\n')) {
        out.add(line);
      }
    }
    return out;
  }
}

enum KvReplyKind { simple, bulk, integer, array, nil, error }

/// 服务端概览:一段段命名文本(如 INFO 的 `# Clients`),外加派生指标。
class KvServerInfo {
  const KvServerInfo({
    required this.engineLabel,
    required this.version,
    this.sections = const [],
    this.metrics = const {},
  });

  /// 引擎自称(Redis / Memcached),用于详情面板标题
  final String engineLabel;
  final String version;

  /// 原文分段:[KvInfoSection.name] = '# Keyspace' 之类
  final List<KvInfoSection> sections;

  /// 抽出来的关键指标(内存、连接数等),key 用稳定英文 ID,标签由 UI 翻译
  final Map<String, String> metrics;
}

class KvInfoSection {
  const KvInfoSection({required this.name, required this.entries});

  final String name;

  /// 保持服务端给出顺序,详情面板按行直出
  final List<MapEntry<String, String>> entries;
}

/// 引擎能力表。加新缓存引擎时,只有这张表需要写清楚「像不像 Redis」。
class KvCapabilities {
  const KvCapabilities({
    required this.multipleNamespaces,
    required this.cursorScan,
    required this.keyTypes,
    required this.ttl,
    required this.internalEncoding,
    required this.memoryUsage,
    required this.commandConsole,
    required this.readOnlyFirstVersion,
    this.valueOffsetRead = false,
    this.idleTime = false,
    this.maxValuePreviewBytes,
  });

  /// Redis: true(db0..15)。Memcached: false —— 没有命名空间概念,
  /// 树里就只挂一个固定槽位节点。
  final bool multipleNamespaces;

  /// 能否按游标枚举键。**Memcached 是 false**(它没有 SCAN,只能 stats 计数),
  /// false 时键列表页显示「该引擎不支持枚举」,不给搜索框。
  final bool cursorScan;

  /// 支持的容器类型集合
  final Set<KvKeyType> keyTypes;

  final bool ttl;

  /// 能否读服务端内部编码(OBJECT ENCODING)。没有就整列不出现。
  final bool internalEncoding;

  /// 能否读单键字节数(MEMORY USAGE)
  final bool memoryUsage;

  /// 值能否按偏移区间读(字符串 GETRANGE)
  final bool valueOffsetRead;

  /// 空闲时长(OBJECT IDLETIME)
  final bool idleTime;

  /// 是否有命令控制台标签
  final bool commandConsole;

  /// 首版只读:true 时写命令在门控层拒绝,不给「改了不生效」的入口
  final bool readOnlyFirstVersion;

  /// 值预览的字节上限(超出只取前一段并标 truncated);null = 不额外限
  final int? maxValuePreviewBytes;

  bool supports(KvKeyType type) => keyTypes.contains(type);
}

/// 服务端返回的错误(WRONGTYPE / NOAUTH / ERR unknown command …)。
/// **连接仍然可用**(实测:出错后同连接 PING 回 PONG),上层按「这条命令失败」处理即可。
class KvServerException implements Exception {
  const KvServerException(this.message);

  final String message;

  @override
  String toString() => message;
}

/// 传输 / 解码 / 已关闭 —— 这条连接的状态不可信,必须作废重连。
///
/// 为什么单独分这一类:实测某些 Dart Redis 客户端的解析器一旦对二进制值解码失败,
/// 内部串行 future 链会被打断,之后**每条命令都返回同一个解码异常**,
/// 表现为「连上了但什么都读不出来」。不分这一类就等不到自愈。
class KvConnectionException implements Exception {
  const KvConnectionException(this.message, {this.cause});

  final String message;
  final Object? cause;

  @override
  String toString() => cause == null ? message : '$message($cause)';
}

/// 已实现驱动的缓存 / KV 引擎 id(与 [kSupportedDriverTypes] 并列,互不包含)
const kKvStoreTypes = {'redis'};

/// 该连接是否走 KV 抽象(而不是 SQL 的 [DatabaseDriver])
bool isKvStoreConnection(ConnectionInfo conn) =>
    conn.isLive && kKvStoreTypes.contains(conn.typeId);

/// KV 驱动工厂:与 [createDriver] 分开,SQL 侧的驱动字典不认 KV 类型。
KvStoreDriver? createKvDriver(ConnectionInfo conn) {
  switch (conn.typeId) {
    case 'redis':
      return RedisKvDriver(conn);
    default:
      return null;
  }
}
