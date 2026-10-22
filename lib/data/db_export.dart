/// 数据导出的格式定义与**流式**写出引擎。
///
/// 引擎不直接依赖 AppState / ConnectionManager:分页取数由调用方以
/// [ExportPageLoader] 注入,写出目标由 [File] 提供,因此可脱离 UI 单测。
/// 每一页取到后立即编码并写入 [IOSink],内存占用与表大小无关。
library;

import 'dart:convert';
import 'dart:io';

import 'csv_codec.dart';
import 'drivers/db_driver.dart';
import 'table_design.dart';

/// 导出格式(全部为纯文本类:无需额外二进制编码依赖)
enum DbExportFormat { txt, csv, json, xml, html, sql }

extension DbExportFormatX on DbExportFormat {
  /// 分隔文本类:txt 与 csv 共用同一套编码(仅默认分隔符不同)
  bool get delimited => this == DbExportFormat.txt || this == DbExportFormat.csv;
}

/// 导出格式的展示名(向导选项)
String dbExportFormatLabel(DbExportFormat f) => switch (f) {
      DbExportFormat.txt => '文本文件 (*.txt)',
      DbExportFormat.csv => 'CSV 文件 (*.csv)',
      DbExportFormat.json => 'JSON 文件 (*.json)',
      DbExportFormat.xml => 'XML 文件 (*.xml)',
      DbExportFormat.html => 'HTML 文件 (*.htm;*.html)',
      DbExportFormat.sql => 'SQL 脚本文件 (*.sql)',
    };

/// 导出格式的默认文件扩展名
String dbExportFormatExt(DbExportFormat f) => switch (f) {
      DbExportFormat.txt => 'txt',
      DbExportFormat.csv => 'csv',
      DbExportFormat.json => 'json',
      DbExportFormat.xml => 'xml',
      DbExportFormat.html => 'html',
      DbExportFormat.sql => 'sql',
    };

/// 导出格式的一句话说明(向导第 1 步与「导出结果」对话框共用)
String dbExportFormatHint(DbExportFormat f) => switch (f) {
      DbExportFormat.txt => '制表符 / 逗号 / 分号分隔的纯文本,分隔符可在附加选项里改。',
      DbExportFormat.csv => '逗号 / 分号 / 制表符分隔,可用 Excel 直接打开(建议保留 UTF-8 BOM)。',
      DbExportFormat.json => '对象数组,每条记录一个对象,NULL 输出为 JSON null。',
      DbExportFormat.xml => 'table / row / field 结构,NULL 输出为 null="true" 空元素。',
      DbExportFormat.html => '自包含的静态表格页面,可直接在浏览器中查看。',
      DbExportFormat.sql => '生成 INSERT 语句,可跨库回放;可选附带建表语句。',
    };

/// SQL 导出选项
class SqlExportOptions {
  const SqlExportOptions({
    this.dropStatement = false,
    this.createStatement = false,
    this.insertBatchSize = 50,
  });

  /// 是否在结构前输出 DROP TABLE IF EXISTS
  final bool dropStatement;

  /// 是否包含建表语句(CREATE TABLE)
  final bool createStatement;

  /// 每条 INSERT 合并的行数(多值 VALUES);1 表示逐行 INSERT
  final int insertBatchSize;
}

/// JSON 导出选项
class JsonExportOptions {
  const JsonExportOptions({this.pretty = false});

  /// 是否缩进美化(大表下文件显著变大)
  final bool pretty;
}

/// 一次导出请求的完整配置
class DbExportRequest {
  const DbExportRequest({
    required this.format,
    this.csv = const DelimitedStyle(),
    this.csvQuoteAll = false,
    this.csvWithHeader = true,
    this.csvBom = false,
    this.sql = const SqlExportOptions(),
    this.json = const JsonExportOptions(),
    this.columns,
    this.append = false,
    this.pageSize = 2000,
    this.createDdl,
  });

  final DbExportFormat format;

  /// CSV 分隔风格(含 NULL 输出文本)
  final DelimitedStyle csv;

  /// CSV 是否给所有字段加引号
  final bool csvQuoteAll;

  /// CSV 首行是否输出列名
  final bool csvWithHeader;

  /// CSV 是否写入 UTF-8 BOM(便于 Excel 直接双击识别中文)
  final bool csvBom;

  final SqlExportOptions sql;
  final JsonExportOptions json;

  /// 只导出这些列(按此顺序);null 或空表示全部列。
  /// 在写出前对结果集做投影,因此驱动仍按 `SELECT *` 取数——
  /// 省下为每个驱动加列投影 SQL 的改动,代价是宽表多读几列。
  final List<String>? columns;

  /// 追加到已存在文件末尾(仅 [DbExportFormat.delimited] 生效,
  /// 其余格式追加会破坏文档结构,一律重写文件)
  final bool append;

  /// 每次向数据库取数的行数
  final int pageSize;

  /// 建表语句文本(仅 [SqlExportOptions.createStatement] 为真时使用),
  /// 由调用方经 AppState 的转储逻辑生成
  final String? createDdl;

  /// 覆盖部分选项生成新请求(AppState 按表注入 createDdl 与列子集)
  DbExportRequest copyWith({String? createDdl, List<String>? columns}) =>
      DbExportRequest(
        format: format,
        csv: csv,
        csvQuoteAll: csvQuoteAll,
        csvWithHeader: csvWithHeader,
        csvBom: csvBom,
        sql: sql,
        json: json,
        columns: columns ?? this.columns,
        append: append,
        pageSize: pageSize,
        createDdl: createDdl ?? this.createDdl,
      );
}

/// 导出目标表标识
class ExportTarget {
  const ExportTarget({
    required this.typeId,
    required this.database,
    required this.table,
    this.schema,
  });

  final String typeId;
  final String database;
  final String table;
  final String? schema;

  /// 带模式限定的表标识符
  String get ident => DdlBuilder.qualified(typeId, schema, table);

  /// 建议文件名用的表名(去掉路径非法字符)
  String get safeName => table.replaceAll(RegExp(r'[\\/:*?"<>|]'), '_');
}

/// 分页取数回调:返回从 [offset] 开始的至多 [limit] 行
typedef ExportPageLoader = Future<TablePreview> Function(int limit, int offset);

/// 批量导出中的一张表:目标路径与列子集(空表示全部列)
class ExportJob {
  const ExportJob({
    required this.table,
    required this.filePath,
    this.columns,
  });

  final String table;
  final String filePath;
  final List<String>? columns;

  /// 建议文件名用的安全表名
  String get safeName => table.replaceAll(RegExp(r'[\\/:*?"<>|]'), '_');
}

/// 批量导出整批结果
class BatchExportResult {
  const BatchExportResult({
    required this.rowsWritten,
    required this.tablesDone,
    required this.tablesFailed,
    this.cancelled = false,
    this.error,
  });

  /// 累计写出行数
  final int rowsWritten;

  /// 成功的表数
  final int tablesDone;

  /// 失败的表数
  final int tablesFailed;

  /// 用户中途取消
  final bool cancelled;

  /// 首个失败原因(null 表示无失败)
  final String? error;

  bool get ok => tablesFailed == 0 && !cancelled;
}

/// 导出执行结果
class ExportResult {
  const ExportResult({
    required this.rowsWritten,
    this.error,
    this.cancelled = false,
  });

  /// 已写出的行数
  final int rowsWritten;

  /// 失败原因(null 表示成功)
  final String? error;

  /// 用户中途取消(已写出的内容保留)
  final bool cancelled;

  bool get ok => error == null;
}

/// 把一张表导出到 [filePath]。
///
/// [load] 负责按 (limit, offset) 取数;调用方须保证分页顺序稳定
/// (AppState 的封装按主键、无主键时按全部列 ORDER BY)。
/// [onProgress] 每页回调一次(已完成行数, 总行数或 -1 表示未知)。
Future<ExportResult> exportTable({
  required ExportTarget target,
  required DbExportRequest request,
  required ExportPageLoader load,
  required String filePath,
  void Function(int done, int total)? onProgress,
  bool Function()? isCancelled,
}) async {
  // 追加只对分隔文本类有意义:JSON / XML / HTML 追加会破坏文档结构
  final file = File(filePath);
  final appending = request.append &&
      request.format.delimited &&
      await file.exists() &&
      await file.length() > 0;
  final sink = file.openWrite(mode: appending ? FileMode.append : FileMode.write);
  final writer = _FormatWriter.create(
    target,
    request,
    // 追加到已有内容后不应再出现第二行表头
    withHeader: request.csvWithHeader && !appending,
  );
  var done = 0;
  try {
    if (request.format.delimited && request.csvBom && !appending) {
      sink.add(const [0xEF, 0xBB, 0xBF]);
    }
    var page = _project(await load(request.pageSize, 0), request.columns);
    writer.begin(sink, page.columns);
    while (page.columns.isNotEmpty && page.rows.isNotEmpty) {
      writer.writeRows(sink, page);
      done += page.rows.length;
      onProgress?.call(done, -1);
      if (page.rows.length < request.pageSize) break;
      if (isCancelled?.call() ?? false) {
        writer.end(sink);
        return ExportResult(rowsWritten: done, cancelled: true);
      }
      page = _project(await load(request.pageSize, done), request.columns);
    }
    writer.end(sink);
    return ExportResult(rowsWritten: done);
  } catch (e) {
    return ExportResult(rowsWritten: done, error: e.toString());
  } finally {
    await sink.close();
  }
}

/// 按 [want] 取列子集(保持 [want] 的顺序);未要求的列名忽略。
/// [want] 为空或已覆盖全部列时原样返回,避免大表无谓的行列拷贝。
TablePreview _project(TablePreview page, List<String>? want) {
  if (want == null || want.isEmpty) return page;
  final idx = <int>[];
  for (final name in want) {
    final i = page.columns.indexOf(name);
    if (i >= 0 && !idx.contains(i)) idx.add(i);
  }
  var identity = idx.length == page.columns.length;
  for (var i = 0; identity && i < idx.length; i++) {
    if (idx[i] != i) identity = false;
  }
  if (identity) return page;
  final mask = page.nullMask;
  final hex = page.hexColumns;
  return TablePreview(
    limit: page.limit,
    columns: [for (final i in idx) page.columns[i]],
    rows: [
      for (final row in page.rows)
        [for (final i in idx) i < row.length ? row[i] : '']
    ],
    nullMask: mask == null
        ? null
        : [
            for (final m in mask) [for (final i in idx) i < m.length && m[i]]
          ],
    // 十六进制化的列要跟着列子集走,否则 SQL 写器会把它当普通文本导出
    hexColumns: hex == null
        ? null
        : {
            for (final i in idx)
              if (hex.contains(page.columns[i])) page.columns[i],
          },
  );
}

/// 把已在内存中的结果集一次写出到 [filePath]。
///
/// 用于查询页结果网格:数据已经全部取回,不需要分页取数、进度与取消,
/// 因此直接复用 [exportTable] 的编码与写出逻辑 —— 分页回调只在第一页
/// 返回全部数据,其后返回空页让引擎正常收尾。
///
/// 注意 SQL(INSERT) 格式需要真实目标表,结果集导出只应提供 CSV / JSON。
Future<ExportResult> exportRows({
  required ExportTarget target,
  required DbExportRequest request,
  required TablePreview data,
  required String filePath,
}) =>
    exportTable(
      target: target,
      request: request,
      filePath: filePath,
      load: (limit, offset) async => offset == 0
          ? data
          : const TablePreview(columns: [], rows: []),
    );

/// 各格式的写出器:begin 写文件头,end 收尾(缓冲区落盘由 IOSink 负责)
abstract class _FormatWriter {
  static _FormatWriter create(ExportTarget target, DbExportRequest req,
      {bool withHeader = true}) {
    switch (req.format) {
      case DbExportFormat.csv:
      case DbExportFormat.txt:
        return _DelimitedWriter(req, withHeader);
      case DbExportFormat.sql:
        return _SqlWriter(target, req);
      case DbExportFormat.json:
        return _JsonWriter(req);
      case DbExportFormat.xml:
        return _XmlWriter(target, req);
      case DbExportFormat.html:
        return _HtmlWriter(target, req, withHeader);
    }
  }

  void begin(IOSink sink, List<String> columns);
  void writeRows(IOSink sink, TablePreview page);
  void end(IOSink sink);
}

class _DelimitedWriter implements _FormatWriter {
  _DelimitedWriter(this._req, this._withHeader);

  final DbExportRequest _req;
  final bool _withHeader;

  @override
  void begin(IOSink sink, List<String> columns) {
    if (_withHeader) {
      sink.write(csvEncodeRow(columns, _req.csv) + _req.csv.eol);
    }
  }

  @override
  void writeRows(IOSink sink, TablePreview page) {
    for (var r = 0; r < page.rows.length; r++) {
      final cells = page.rows[r];
      final nulls = [
        for (var c = 0; c < cells.length; c++) page.isNullAt(r, c)
      ];
      sink.write(
        csvEncodeRow(cells, _req.csv,
            quoteAll: _req.csvQuoteAll, nullFlags: nulls) +
            _req.csv.eol,
      );
    }
  }

  @override
  void end(IOSink sink) {}
}

class _SqlWriter implements _FormatWriter {
  _SqlWriter(this._target, this._req);

  final ExportTarget _target;
  final DbExportRequest _req;
  List<String> _columns = const [];

  /// 待合并进一条 INSERT 的 VALUES 元组
  final List<String> _tuples = [];

  /// Jet(ODBC)不支持多值 VALUES,退化为逐行 INSERT
  int get _batchSize => _target.typeId == 'access'
      ? 1
      : (_req.sql.insertBatchSize < 1 ? 1 : _req.sql.insertBatchSize);

  /// 列清单前缀:`INSERT INTO t ("a", "b") VALUES `
  String get _insertPrefix {
    final cols = _columns
        .map((c) => DdlBuilder.ident(_target.typeId, c))
        .join(', ');
    return 'INSERT INTO ${_target.ident} ($cols) VALUES ';
  }

  @override
  void begin(IOSink sink, List<String> columns) {
    _columns = columns;
    final t = _target;
    final db = t.schema == null || t.schema!.isEmpty
        ? t.database
        : '${t.database}.${t.schema}';
    sink
      ..writeln('-- ============================================')
      ..writeln('-- daro 数据导出')
      ..writeln('-- 目标表: $db.${t.table}')
      ..writeln('-- 生成时间: ${DateTime.now().toIso8601String()}')
      ..writeln('-- ============================================')
      ..writeln();
    if (_req.sql.createStatement && _req.createDdl != null) {
      if (_req.sql.dropStatement) {
        sink.writeln('DROP TABLE IF EXISTS ${t.ident};');
        sink.writeln();
      }
      sink
        ..writeln(_req.createDdl!.trim())
        ..writeln();
    }
  }

  @override
  void writeRows(IOSink sink, TablePreview page) {
    for (var r = 0; r < page.rows.length; r++) {
      final cells = page.rows[r];
      final buf = StringBuffer('(');
      for (var c = 0; c < cells.length; c++) {
        if (c > 0) buf.write(', ');
        buf.write(_cellLiteral(page, r, c));
      }
      buf.write(')');
      _tuples.add(buf.toString());
      if (_tuples.length >= _batchSize) _flush(sink);
    }
  }

  /// 单元格 → VALUES 里的字面量。
  ///
  /// 被驱动换成十六进制文本取回的二进制列(源库是 MySQL / MariaDB 的 BLOB 等,
  /// 见 [TablePreview.hexColumns])必须还原成二进制字面量,否则导出的 SQL 会把
  /// 十六进制字符本身写进列里(5 字节变 10 字节)。
  String _cellLiteral(TablePreview page, int row, int col) {
    if (page.isNullAt(row, col)) return 'NULL';
    final value = page.rows[row][col];
    if (!page.isHexColumn(col)) return DdlBuilder.lit(value);
    return _binaryLiteralFor(_target.typeId, value) ?? DdlBuilder.lit(value);
  }

  void _flush(IOSink sink) {
    if (_tuples.isEmpty) return;
    sink.writeln('${_insertPrefix}${_tuples.join(', ')};');
    _tuples.clear();
  }

  @override
  void end(IOSink sink) => _flush(sink);
}

/// 十六进制转储 → 目标方言的二进制字面量。
///
/// 值不是纯十六进制(用户在源端网格里手改成了文本),或目标方言没有对应的
/// 二进制字面量写法时返回 null,调用方按普通文本字面量导出。
/// 十六进制转储只可能来自 MySQL / MariaDB 源(见 mysql_driver.dart 的
/// `mysqlSafeProjection`),同方言回迁是主路径。
String? _binaryLiteralFor(String typeId, String hex) {
  if (TablePreview.binaryLiteralFromHex(hex) == null) return null;
  if (DdlBuilder.isMysqlLike(typeId)) return "X'$hex'";
  if (DdlBuilder.isSqlServerLike(typeId)) return '0x$hex';
  if (DdlBuilder.isPgLike(typeId)) return "'\\x$hex'::bytea";
  return null;
}

class _JsonWriter implements _FormatWriter {
  _JsonWriter(this._req);

  final DbExportRequest _req;
  List<String> _columns = const [];
  bool _wroteAny = false;

  @override
  void begin(IOSink sink, List<String> columns) {
    _columns = columns;
    sink.write('[');
    if (_req.json.pretty) sink.write('\n');
  }

  @override
  void writeRows(IOSink sink, TablePreview page) {
    for (var r = 0; r < page.rows.length; r++) {
      final map = <String, Object?>{};
      final cells = page.rows[r];
      for (var c = 0; c < _columns.length; c++) {
        map[_columns[c]] =
            (c < cells.length && !page.isNullAt(r, c)) ? cells[c] : null;
      }
      if (_wroteAny) sink.write(',');
      sink.write(_req.json.pretty ? '\n  ' : '');
      sink.write(jsonEncode(map));
      _wroteAny = true;
    }
  }

  @override
  void end(IOSink sink) {
    sink
      ..write(_wroteAny && _req.json.pretty ? '\n]' : ']')
      ..write(_req.json.pretty ? '\n' : '');
  }
}

class _XmlWriter implements _FormatWriter {
  _XmlWriter(this._target, this._req);

  final ExportTarget _target;
  final DbExportRequest _req;

  /// 记录分隔符(换行风格)同样作用于 XML 缩进,保持与向导选项一致
  String get _eol => _req.csv.eol;

  @override
  void begin(IOSink sink, List<String> columns) {
    final t = _target;
    sink
      ..write('<?xml version="1.0" encoding="UTF-8"?>$_eol')
      ..write('<!-- daro 数据导出: ${t.database}.${t.table} · ')
      ..write('${DateTime.now().toIso8601String()} -->$_eol')
      ..write('<table name="${_xmlEscape(t.table)}">$_eol');
  }

  @override
  void writeRows(IOSink sink, TablePreview page) {
    final cols = page.columns;
    for (var r = 0; r < page.rows.length; r++) {
      final cells = page.rows[r];
      final buf = StringBuffer('  <row>');
      for (var c = 0; c < cols.length; c++) {
        final name = _xmlEscape(cols[c]);
        if (page.isNullAt(r, c)) {
          buf.write('<field name="$name" null="true" />');
        } else {
          buf.write('<field name="$name">'
              '${_xmlEscape(c < cells.length ? cells[c] : '')}</field>');
        }
      }
      sink.write('$buf</row>$_eol');
    }
  }

  @override
  void end(IOSink sink) => sink.write('</table>$_eol');
}

class _HtmlWriter implements _FormatWriter {
  _HtmlWriter(this._target, this._req, this._withHeader);

  final ExportTarget _target;
  final DbExportRequest _req;
  final bool _withHeader;

  String get _eol => _req.csv.eol;

  @override
  void begin(IOSink sink, List<String> columns) {
    final title = _htmlEscape(
        _target.schema == null || _target.schema!.isEmpty
            ? '${_target.database}.${_target.table}'
            : '${_target.database}.${_target.schema}.${_target.table}');
    sink
      ..write('<!DOCTYPE html>$_eol')
      ..write('<html lang="zh-CN">$_eol<head>')
      ..write('<meta charset="UTF-8">$_eol')
      ..write('<title>$title</title>$_eol')
      ..write('<style>body{font-family:"Microsoft YaHei",sans-serif}')
      ..write('table{border-collapse:collapse}th,td{border:1px solid #bbb;')
      ..write('padding:2px 8px;font-weight:normal}th{background:#f2f2f2}')
      ..write('td.null{color:#999;font-style:italic}</style>$_eol')
      ..write('</head>$_eol<body>')
      ..write('<h3>$title</h3>$_eol')
      ..write('<table>$_eol');
    if (_withHeader) {
      sink
        ..write('<thead><tr>')
        ..write(columns.map((c) => '<th>${_htmlEscape(c)}</th>').join())
        ..write('</tr></thead>$_eol');
    }
    sink.write('<tbody>$_eol');
  }

  @override
  void writeRows(IOSink sink, TablePreview page) {
    final cols = page.columns;
    for (var r = 0; r < page.rows.length; r++) {
      final cells = page.rows[r];
      final buf = StringBuffer('<tr>');
      for (var c = 0; c < cols.length; c++) {
        final v = c < cells.length ? cells[c] : '';
        buf.write(page.isNullAt(r, c)
            ? '<td class="null"></td>'
            : '<td>${_htmlEscape(v)}</td>');
      }
      sink.write('$buf</tr>$_eol');
    }
  }

  @override
  void end(IOSink sink) {
    sink
      ..write('</tbody>$_eol</table>$_eol')
      ..write('</body>$_eol</html>$_eol');
  }
}

/// XML 文本转义:五个预定义实体 + 非法的裸控制字符丢弃
String _xmlEscape(String v) {
  final buf = StringBuffer();
  for (final r in v.runes) {
    switch (r) {
      case 0x26:
        buf.write('&amp;');
      case 0x3c:
        buf.write('&lt;');
      case 0x3e:
        buf.write('&gt;');
      case 0x22:
        buf.write('&quot;');
      case 0x27:
        buf.write('&apos;');
      default:
        // XML 1.0 不允许的低序控制字符(制表 / 换行 / 回车除外)直接跳过
        if (r < 0x20 && r != 0x09 && r != 0x0a && r != 0x0d) continue;
        buf.writeCharCode(r);
    }
  }
  return buf.toString();
}

/// HTML 文本转义
String _htmlEscape(String v) => v
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;');
