/// SQL 脚本拆分:按语句结束符切成独立语句,供查询编辑页逐条执行、
/// 逐条出结果(每条语句一个结果 tab,同 pgAdmin / DBeaver 行为),
/// 也供「运行 SQL 文件」流式消费整个转储文件。
///
/// 词法感知:单引号字符串、双引号标识符、反引号标识符、SQL Server
/// [方括号] 标识符(以上均支持双写转义)、`--` 行注释、`/* */` 块注释
/// (支持 PostgreSQL 嵌套)、PostgreSQL 美元引号($$ / $tag$)——
/// 这些结构内部出现的结束符不作为语句分隔符。
///
/// 两种用法共用同一套词法状态机:
/// - [splitSqlStatements]:整段脚本一次切分(查询编辑页);
/// - [SqlStatementSplitter]:按块 [SqlStatementSplitter.feed] 增量喂入,
///   再大的文件在内存里也只留下「当前语句 + 当前数据块」,并且跨块保留
///   未闭合的注释 / 字符串 / 美元引号状态。任意切块下的产出与整段切分**严格一致**。
///
/// 已知局限:不解析 T-SQL BEGIN...END 块与非美元引号包裹的函数体,
/// 其中的结束符会被误拆(与多数客户端工具一致);转义按 SQL 标准处理
/// (双写引号),不处理 MySQL 反斜杠转义。
library;

/// 美元引号起始定界符:$$ 或 $tag$(tag 为标识符,不能数字开头,
/// 因此 $1 之类的占位参数不会被误判)
final RegExp _dollarQuoteStart = RegExp(r'\$([A-Za-z_][A-Za-z0-9_]*)?\$');

/// 干净状态下的「断点字符」集合:换行、各类引号与方括号、注释起始、美元
/// 引号与默认结束符。运行扫描一次跳到下一个断点,避免大文件上逐字符写缓冲。
final RegExp _specialChars = RegExp('[\n\'"`\\[\\]\$/*;-]');

/// 块注释扫描的下一个候选字符(可能是 /* 起始或 */ 收尾)
final RegExp _commentChars = RegExp(r'[*/]');

/// MySQL 客户端的 `DELIMITER xxx` 指令行:整行只有指令与新的结束符
final RegExp _delimiterDirective =
    RegExp(r'^\s*[Dd][Ee][Ll][Ii][Mm][Ii][Tt][Ee][Rr]\s+(\S+)\s*$');

/// SQL Server 客户端(SSMS / sqlcmd)的批处理分隔行:独占一行的 GO,
/// 可带重复次数。它不是 SQL 语法,只是把缓冲区里的批提交执行,
/// 故在此按「结束当前语句」处理;重复次数一律忽略——按次数重放转储
/// 会把数据写进去好几遍,不是客户端该替用户做的决定
final RegExp _goBatch = RegExp(r'^\s*[Gg][Oo]\s*\d*\s*$');

/// `DELIMITER xxx` 指令行中关键字**之后**的余文(定界符长度封顶与
/// [_lookahead] 口径一致)
final RegExp _delimiterTail = RegExp(r'^\s*\S{0,64}\s*$');

/// `GO [n]` 批分隔行中关键字**之后**的余文
final RegExp _goTail = RegExp(r'^\s*\d{0,64}\s*$');

/// 词法记号最长需要向前看的字符数:`/*` `--` `*/` 双写引号是 2,
/// `$tag$` 最长。增量喂入时数据块可能在记号中间切断,故每次 [feed]
/// 都保留这么长的尾部留待下一块,保证增量结果与整段切分一致。
/// 超过该长度的美元引号标签(转储里没见过)会被当作普通文本。
const int _lookahead = 70;

/// 试探缓冲的上限:超出即放弃「这可能是指令行」的判断回到普通扫描,
/// 避免极端输入(整行只有空白)下的重复试探放大成 O(n²)
const int _maxProbe = 4096;

/// [re] 在 [s] 的 [from] 之后第一个匹配的位置;无匹配返回 [s] 长度。
/// RegExp.firstMatch 不接受起始位置,故借 allMatches 的惰性迭代取首个。
int _nextMatch(RegExp re, String s, int from) {
  final it = re.allMatches(s, from).iterator;
  return it.moveNext() ? it.current.start : s.length;
}

/// 判断一段行首文本是否还可能补全成一条指令行:
/// [upper] 不超过 [keyword] 长度时按前缀比较,更长则要求关键字完整出现
/// 且其后的余文符合 [tail]。
bool _directiveCandidate(String upper, String keyword, RegExp tail) {
  if (upper.length <= keyword.length) return keyword.startsWith(upper);
  if (!upper.startsWith(keyword)) return false;
  return tail.hasMatch(upper.substring(keyword.length));
}

/// 判断一段行首文本是否还可能补全成一条指令行(DELIMITER / GO)
bool _couldBeDirectiveLine(String text) {
  if (text.length > _maxProbe) return false;
  final upper = text.trimLeft().toUpperCase();
  if (upper.isEmpty) return true; // 纯缩进 / 空行:仍可能是指令行
  return _directiveCandidate(upper, 'DELIMITER', _delimiterTail) ||
      _directiveCandidate(upper, 'GO', _goTail);
}

/// 把 SQL 脚本按语句结束符拆成语句列表(trim 后去掉空语句)。
/// 末尾无结束符的剩余文本也作为最后一条语句返回
List<String> splitSqlStatements(String script) {
  final splitter = SqlStatementSplitter();
  final statements = splitter.feed(script);
  return [...statements, ...splitter.finish()];
}

/// 找出 [script] 中**包含 [offset]** 的那条语句(词法口径与
/// [splitSqlStatements] 完全一致),供「运行光标所在语句」使用。
///
/// 不改动状态机就能拿到区间:切分器只把原串的连续片段写进缓冲
/// (`DELIMITER` / `GO` 指令行整行吞掉、不并入语句),再 `trim` 掉首尾空白,
/// 所以每条语句都是 [script] 的一段**逐字子串**,且按出现先后排列 ——
/// 顺着 `indexOf` 游标就能还原出偏移。
///
/// 光标落在两条语句之间(紧跟结束符或其后的空白)时归给**后一条**;
/// 落在最后一条语句之后仍返回最后一条。空 / 纯空白脚本返回 null ——
/// 注意孤立注释按 [splitSqlStatements] 的口径本身就是一条语句。
String? sqlStatementAt(String script, int offset) {
  final statements = splitSqlStatements(script);
  if (statements.isEmpty) return null;
  var cursor = 0;
  for (final statement in statements) {
    final start = script.indexOf(statement, cursor);
    if (start < 0) break; // 理论上到不了(见上文逐字子串不变量)
    if (start + statement.length >= offset) return statement;
    cursor = start + statement.length;
  }
  return statements.last;
}

/// 增量 SQL 语句切分器:见文件头注释。
///
/// [feed] 接受任意切块(不要求按行、不要求边界对齐),返回**本次新完成**
/// 的语句;结尾用 [finish] 取出最后一条不含结束符的语句。
/// 为保证词法记号不被切块打断,[feed] 会刻意扣住末尾一小段文本不判定,
/// 所以产出不保证「即时」——小脚本可能整条语句都要到 [finish] 才出现。
///
/// 除 `;` 外还识别两类客户端指令行(它们本身不作为语句输出):
/// - MySQL `DELIMITER xxx`:把后续语句的结束符换成指定文本。mysqldump 及
///   同类图形客户端转储里的存储过程、触发器依赖它,否则函数体内的分号会被误拆;
/// - SQL Server `GO`:结束当前一批语句。
class SqlStatementSplitter {
  SqlStatementSplitter({String delimiter = ';'})
      : _delimiter = delimiter.isEmpty ? ';' : delimiter;

  /// 当前语句结束符(初始 `;`,可被 DELIMITER 指令改写)
  String get delimiter => _delimiter;
  String _delimiter;

  /// 正在累积的语句文本
  final StringBuffer _buf = StringBuffer();

  /// 待返回的语句(每次 feed 结尾清空)
  final List<String> _out = <String>[];

  /// 行首指令试探缓冲:仅在「行首 + 词法干净」时攒字符
  final StringBuffer _pending = StringBuffer();

  /// 因 [_lookahead] 而推迟处理的尾部文本(下一个数据块会拼在它后面重扫)
  String _carry = '';

  /// 块注释嵌套深度(>0 表示当前在 /* */ 内)
  int _blockDepth = 0;

  /// 未闭合的引号 / 方括号结构的闭合符
  String? _quoteClose;

  /// 未闭合的美元引号定界符($$ / $tag$)
  String? _dollarTag;

  /// 当前可否按「行首」试探指令行(结构闭合或本行已判定不是指令行时为假)
  bool _atLineStart = true;

  /// 本行已判定不是指令行:放弃试探直到下一个换行(避免逐字符重判)
  bool _lineRejected = false;

  /// 喂入一段脚本,返回本次新完成的语句
  List<String> feed(String text) {
    if (text.isEmpty) return _drain();
    // 上一块保留的尾部与本轮询位置对齐,故拼接后从 0 开始即接着上次推进
    final s = _carry.isEmpty ? text : '$_carry$text';
    _carry = '';
    _run(s, s.length > _lookahead ? s.length - _lookahead : 0);
    return _drain();
  }

  /// 结束喂入:输出最后一条不带结束符的语句(以及推迟处理的尾部余文)
  List<String> finish() {
    if (_carry.isNotEmpty) {
      final tail = _carry;
      _carry = '';
      // 没有下一个数据块了:整段扫完,不再保留向前看的余量
      _run(tail, tail.length, eof: true);
      final rest = _pending.toString();
      _pending.clear();
      if (rest.isNotEmpty) _applyDirectiveOrScan(rest);
    }
    _flush();
    return _drain();
  }

  /// 推进状态机直到 [limit];未扫完的部分留到下次([feed])或收尾([finish])处理。
  /// [eof] 为真表示已是文件末尾(调用方是 [finish]):此时不再保留余量,
  /// 行首试探未定论时把 [_pending] 原样留给调用人判定。
  void _run(String s, int limit, {bool eof = false}) {
    final n = s.length;
    var i = 0;
    while (i < limit) {
      final clean =
          _blockDepth == 0 && _quoteClose == null && _dollarTag == null;
      if (!_lineRejected && _atLineStart && clean) {
        // 行首:先攒整行试探指令行(试探长度由 _couldBeDirectiveLine 封顶)
        var j = i;
        var decided = false;
        while (j < limit) {
          _pending.write(s[j]);
          j++;
          final candidate = _pending.toString();
          // 先判行尾:整行读齐才谈得上是不是指令行(_couldBeDirectiveLine
          // 只回答「半截文本还可能补全成指令行吗」,对 'GO\n' 会答否)
          if (candidate.endsWith('\n')) {
            _pending.clear();
            if (_delimiterDirective.hasMatch(candidate)) {
              _delimiter = _delimiterDirective.firstMatch(candidate)!.group(1)!;
              i = j;
            } else if (_goBatch.hasMatch(candidate)) {
              _flush();
              i = j;
            } else {
              i = _scan(s, i, j);
            }
            decided = true;
            break;
          }
          if (!_couldBeDirectiveLine(candidate)) {
            // 不是指令行:本行余下部分不再试探,已攒下的字符按普通 SQL 扫描
            _pending.clear();
            _lineRejected = true;
            _atLineStart = false;
            i = _scan(s, i, j);
            decided = true;
            break;
          }
        }
        if (decided) continue;
        if (eof) return;
        // 试探到可判定区末尾仍未定论:半行连同留白尾部一起等下一块
        // (_pending 的内容已包含在 _carry 里,故此处必须清空)
        _pending.clear();
        _carry = s.substring(i);
        return;
      }
      // 普通扫描:按行推进(换行正是下一行可否试探指令的分界)
      final nl = s.indexOf('\n', i);
      final stop = nl == -1 ? limit : (nl + 1 > limit ? limit : nl + 1);
      i = _scan(s, i, stop);
    }
    if (!eof && i < n) _carry = s.substring(i);
  }

  /// 一整行喂完:是指令行就生效,否则当作普通 SQL 扫描
  void _applyDirectiveOrScan(String line) {
    if (_delimiterDirective.hasMatch(line)) {
      _delimiter = _delimiterDirective.firstMatch(line)!.group(1)!;
      return;
    }
    if (_goBatch.hasMatch(line)) {
      _flush();
      return;
    }
    _scan(line, 0, line.length);
  }

  List<String> _drain() {
    if (_out.isEmpty) return const [];
    final result = List<String>.of(_out);
    _out.clear();
    return result;
  }

  /// 词法状态机:把 [s] 中 [from] 到 [limit] 之间的文本并入当前语句,
  /// 遇到结束符就产出一条语句;返回**实际消费到的位置**。
  /// 判定可以越过 [limit] 向前看(词法记号成对出现),多吃掉的部分
  /// 调用方直接以返回值续上即可。
  /// 所有跨调用状态(注释深度 / 未闭合引号 / 美元引号)都是实例字段。
  int _scan(String s, int from, int limit) {
    final n = s.length;
    var i = from;
    while (i < limit) {
      // 块注释内:直到配对的 */(PostgreSQL 支持嵌套,用深度计数)
      if (_blockDepth > 0) {
        final j = _nextMatch(_commentChars, s, i);
        if (j > i) _buf.write(s.substring(i, j));
        if (j == n) return n;
        final c = s[j];
        if (c == '/' && j + 1 < n && s[j + 1] == '*') {
          _blockDepth++;
          _buf.write('/*');
          i = j + 2;
        } else if (c == '*' && j + 1 < n && s[j + 1] == '/') {
          _blockDepth--;
          _buf.write('*/');
          i = j + 2;
          if (_blockDepth == 0) _afterStructure();
        } else {
          _buf.write(c);
          i = j + 1;
        }
        continue;
      }

      // 美元引号内:消费到配对的 $tag$
      if (_dollarTag != null) {
        final tag = _dollarTag!;
        final j = s.indexOf(r'$', i);
        if (j < 0) {
          _buf.write(s.substring(i));
          return n;
        }
        _buf.write(s.substring(i, j));
        if (s.startsWith(tag, j)) {
          _buf.write(tag);
          i = j + tag.length;
          _dollarTag = null;
          _afterStructure();
        } else {
          _buf.write(r'$');
          i = j + 1;
        }
        continue;
      }

      // 引号包裹的结构内:直到未转义的闭合符(双写定界符为转义)
      if (_quoteClose != null) {
        final close = _quoteClose!;
        final j = s.indexOf(close, i);
        if (j < 0) {
          _buf.write(s.substring(i));
          return n;
        }
        _buf.write(s.substring(i, j + 1));
        if (j + 1 < n && s[j + 1] == close) {
          // 双写是转义,结构仍未闭合
          _buf.write(close);
          i = j + 2;
          continue;
        }
        i = j + 1;
        _quoteClose = null;
        _afterStructure();
        continue;
      }

      // 干净状态:语句结束符优先判定。MySQL 转储常用 $$ 作结束符,而它
      // 同时是 PostgreSQL 美元引号的起始串,不先判定就会把整个过程体当字符串吞掉
      if (s.startsWith(_delimiter, i)) {
        _flush();
        i += _delimiter.length;
        continue;
      }

      // 整段跳到下一个断点字符(自定义结束符的首字符可能不在常规集合里)
      final k0 = _nextMatch(_specialChars, s, i);
      var k = k0;
      final head = _delimiter[0];
      if (head != ';') {
        final d = s.indexOf(head, i);
        if (d >= 0 && d < k) k = d;
      }
      if (k > i) {
        _buf.write(s.substring(i, k));
        i = k;
        if (i >= n) return n;
        // 停在断点上:回到循环开头重新判定(结束符优先于结构起始)
        continue;
      }
      final ch = s[i];

      if (ch == '-' && i + 1 < n && s[i + 1] == '-') {
        // 行注释:本行余下部分原样保留(换行交给下一轮循环)
        final nl = s.indexOf('\n', i);
        if (nl < 0) {
          _buf.write(s.substring(i));
          return n;
        }
        _buf.write(s.substring(i, nl));
        i = nl;
        continue;
      }

      if (ch == '/' && i + 1 < n && s[i + 1] == '*') {
        _blockDepth = 1;
        _buf.write('/*');
        i += 2;
        continue;
      }

      if (ch == r'$' && _dollarQuoteStart.matchAsPrefix(s, i) != null) {
        final tag = _dollarQuoteStart.matchAsPrefix(s, i)!.group(0)!;
        _dollarTag = tag;
        _buf.write(tag);
        i += tag.length;
        continue;
      }

      if (ch == "'" || ch == '"' || ch == '`' || ch == '[') {
        _quoteClose = ch == '[' ? ']' : ch;
        _buf.write(ch);
        i++;
        continue;
      }

      if (ch == '\n') {
        // 换行:下一行重新获得指令行试探资格
        _atLineStart = true;
        _lineRejected = false;
      }
      _buf.write(ch);
      i++;
    }
    return i;
  }

  /// 结构闭合后:本行必然不是指令行,且当前不处于行首
  void _afterStructure() {
    _atLineStart = false;
    _lineRejected = true;
  }

  void _flush() {
    final s = _buf.toString().trim();
    if (s.isNotEmpty) _out.add(s);
    _buf.clear();
  }
}
