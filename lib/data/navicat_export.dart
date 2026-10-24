import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'db_data.dart';
import 'navicat_import.dart';

/// 把 daro 的连接导出成 Navicat 的 `.ncx`,使文件能直接进 Navicat「导入连接设置」。
///
/// ── 属性模板的来源 ──
/// 下面每种 ConnType 的属性**名称与顺序**,以及 daro 模型里没有的那些属性取值,
/// 全部取自本机 Navicat Premium 16/17 真实导出文件(connections.ncx,379 条)的
/// 实测结果:同一类型的属性顺序在文件里完全稳定(4 种类型各只有 1 种顺序),
/// 且除连接名 / 主机 / 端口 / 用户 / 密码 / 保存路径外,其余属性在样本里都是常量
/// (即 Navicat 出厂默认值)。属性序列被 test/navicat_export_test.dart 钉成快照,
/// 防止后续改动悄悄破坏与 Navicat 的兼容性。
///
/// ── 密码 ──
/// 用与导入完全相同的 libcc 常量做 AES-128-CBC + PKCS#7 加密,输出大写十六进制,
/// 所以 Navicat 侧能照常解密(空密码也会写出密文,与 Navicat 自身行为一致:
/// 样本里 Windows 身份验证的 SQL Server 连接就是 `Password="E191AF42..."`)。
///
/// ── 分组 ──
/// Navicat 的 `.ncx` 没有分组字段(实测本机 378 条连接、52 个属性里无任何
/// group/folder 字样),daro 用**私有属性** `Group="分组名"` 承载,追加在每条
/// 连接属性表末尾。Navicat 读该文件时忽略未知属性、连接本身照常导入,
/// 只是看不到分组;分组在 daro ↔ daro 之间完整往返。

/// 不可导出的 daro 连接类型 → 原因(Navicat 侧没有对应 ConnType)
const Map<String, String> kNavicatExportBlockers = {
  'access': 'Navicat 没有 Access 连接类型',
  // 实测:Navicat 导出 Redis 连接给的是 base64 JSON 的 .ano,不是 .ncx。
  // 没有真实 .ncx Redis 样本可对照属性表,就不凭猜模板写出去。
  'redis': 'Redis 在 Navicat 侧按 .ano 导出,.ncx 里没有它的属性模板',
};

/// 一次导出的结果。
class NavicatExportResult {
  const NavicatExportResult({
    required this.xml,
    required this.exported,
    required this.skipped,
  });

  /// 待写盘的 .ncx 文本(UTF-8,CRLF,与 Navicat 排版一致)
  final String xml;

  /// 实际写进文件的连接名
  final List<String> exported;

  /// 因 Navicat 无对应类型而跳过的连接(名称 + 原因)
  final List<(String name, String reason)> skipped;
}

/// daro typeId → Navicat ConnType + 其在 Navicat 设置目录里的子目录名。
///
/// mariadb 按 MYSQL 导出:Navicat 的 MariaDB 与 MySQL 共用同一套驱动与属性,
/// 而实测样本里没有出现独立的 MARIADB 类型,不写未验证的类型名。
const Map<String, (String connType, String folder)> kNavicatExportTypes = {
  'mysql': ('MYSQL', r'MySQL'),
  'mariadb': ('MYSQL', r'MySQL'),
  'postgresql': ('POSTGRESQL', r'PostgreSQL'),
  'sqlserver': ('SQLSERVER', r'SQL Server'),
  'sqlite': ('SQLITE', r'SQLite'),
};

/// 生成 .ncx 文本。
///
/// [settingsSaveRoot] 是 Navicat 存放单连接设置文件的根目录
/// (通常为「文档\Navicat」),仅用于回填 `SettingsSavePath` 属性;
/// Navicat 导入时会按自己的位置重算,不影响能否导入。
NavicatExportResult buildNavicatNcx(
  List<ConnectionInfo> connections, {
  required bool includePasswords,
  required String settingsSaveRoot,
  String formatVersion = '1.5',
}) {
  final exported = <String>[];
  final skipped = <(String, String)>[];
  final buffer = StringBuffer()
    ..write('<?xml version="1.0" encoding="UTF-8"?>$_crlf')
    ..write('<Connections Ver="$formatVersion">$_crlf');
  for (final conn in connections) {
    final type = kNavicatExportTypes[conn.typeId];
    if (type == null) {
      skipped.add((
        conn.name,
        kNavicatExportBlockers[conn.typeId] ?? 'Navicat 无 ${conn.typeId} 连接类型'
      ));
      continue;
    }
    final spec = _Entry(conn, type, includePasswords, settingsSaveRoot);
    buffer.write('\t${spec.toXml()}$_crlf');
    exported.add(conn.name);
  }
  buffer
    ..write('</Connections>$_crlf')
    // Navicat 的文件结尾还有一个空行,保持一致
    ..write(_crlf);
  return NavicatExportResult(
    xml: buffer.toString(),
    exported: exported,
    skipped: skipped,
  );
}

/// 把生成好的 .ncx 写到 [path](UTF-8、flush)。
///
/// 向导「确定」与测试走同一条落盘路径,保证写出的文件就是所见文本。
Future<File> writeNavicatNcxFile(String path, String xml) =>
    File(path).writeAsString(xml, encoding: utf8, flush: true);

const String _crlf = '\r\n';

/// 单条连接的属性装配:按 ConnType 走各自的实测模板顺序。
class _Entry {
  _Entry(
      this.conn, this.type, this.includePasswords, this.settingsSaveRoot);

  final ConnectionInfo conn;
  final (String connType, String folder) type;
  final bool includePasswords;
  final String settingsSaveRoot;

  String get connType => type.$1;

  /// 文件型(SQLite)在 daro 里把路径同时存进 host 与 database
  bool get _isFileBased => connType == 'SQLITE';

  /// Navicat 形如 `C:\Users\<你>\Documents\Navicat\MySQL\Servers\<连接名>`;
  /// 导入时 Navicat 会按本机位置重算,所以这里只需形制正确。
  String get _settingsSavePath {
    final root = settingsSaveRoot.endsWith(r'\')
        ? settingsSaveRoot.substring(0, settingsSaveRoot.length - 1)
        : settingsSaveRoot;
    return '$root\\${type.$2}\\Servers\\${conn.name}';
  }

  String get _host {
    if (_isFileBased) {
      return conn.database.isNotEmpty ? conn.database : conn.host;
    }
    return conn.host;
  }

  String get _port {
    if (conn.port.isNotEmpty) return conn.port;
    // 缺省端口按 Navicat 的出厂值补,留空会让 Navicat 导入时拿到空端口
    return switch (connType) {
      'MYSQL' => '3306',
      'POSTGRESQL' => '5432',
      'SQLSERVER' => '1433',
      _ => '',
    };
  }

  /// 口令密文。
  ///
  /// daro 的连接模型里只有 `password` 一个字段,分不清「没保存口令」与「保存了
  /// 空口令」;而 Navicat 真实导出文件里 65 条空口令连接都是
  /// `Password="" SavePassword="false"`(只有 SQLite 例外,它写的是空串的密文)。
  /// 故非 SQLite 类型遇到空口令还原成「未保存」形态,避免凭空造出一条
  /// 「保存了空口令」的连接;唯一例外是 SQL Server 的 Windows 身份验证
  /// (本就不需要口令,Navicat 实测仍写空串密文且 SavePassword=true)。
  String get _passwordCipher {
    if (!includePasswords) return '';
    if (connType == 'SQLITE') return ''; // SQLite 的口令走 SQLiteEncryptPassword
    // Windows 身份验证不需要口令,Navicat 仍写出空串的密文(实测样本 SQLEXPRESS 行)
    if (conn.password.isEmpty && !_isWindowsAuth) return '';
    return encryptNavicatPassword(conn.password);
  }

  /// SQL Server 的 Windows 身份验证(不需要口令)
  bool get _isWindowsAuth =>
      connType == 'SQLSERVER' && conn.authMethod == 'windows';

  String get _savePassword => includePasswords &&
          (connType == 'SQLITE' || conn.password.isNotEmpty || _isWindowsAuth)
      ? 'true'
      : 'false';

  Map<String, String> get _attrs {
    final base = <String, String>{
      'ConnectionName': conn.name,
      'ProjectUUID': '',
      'ConnType': connType,
      'ServiceProvider': 'Default',
    };
    return switch (connType) {
      'MYSQL' => base
        ..addAll(<String, String>{
            'Host': _host,
            'Port': _port,
            'UserName': conn.username,
            'Password': _passwordCipher,
            'SavePassword': _savePassword,
            'SettingsSavePath': _settingsSavePath,
            'SessionLimit': '0',
            'InitialSessionQueries': '',
            'ClientDriverVersion': 'Default',
            'ClientCharacterSet': '',
            'ClientEncoding': '65001',
            'Keepalive': 'false',
            'UseConnectionTimeout': 'true',
            'ConnectionTimeoutSeconds': '30',
            'UseReadTimeout': 'false',
            'UseWriteTimeout': 'true',
            'WriteTimeoutSeconds': '30',
            'Encoding': '65001',
            'MySQLCharacterSet': 'true',
            'Compression': 'false',
            'AutoConnect': 'false',
            'NamedPipe': 'false',
            'UseAdvanced': 'false',
            'SSL': 'false',
            'SSH': 'false',
            'HTTP': 'false',
            'Compatibility': 'false',
            'Remarks': '',
          }),
      'POSTGRESQL' => base
        ..addAll(<String, String>{
            'Host': _host,
            'Port': _port,
            'Database': conn.database,
            'UserName': conn.username,
            'Password': _passwordCipher,
            'SavePassword': _savePassword,
            'HostTypePreference': 'Default',
            'SettingsSavePath': _settingsSavePath,
            'ConnectionParameters': '',
            'SessionLimit': '0',
            'InitialSessionQueries': '',
            'Keepalive': 'false',
            'AutoConnect': 'false',
            'ClientDriverEncoding': '',
            'ClientEncoding': '65001',
            'UseAdvanced': 'false',
            'SSL': 'false',
            'SSH': 'false',
            'HTTP': 'false',
            'Remarks': '',
          }),
      'SQLSERVER' => base
        ..addAll(<String, String>{
            'Host': _host,
            'Port': _port,
            // 命名实例(形如 host\instance)由 Navicat 按 host 寻址,端口未指定
            'PortSpecified': 'false',
            'Database': conn.database,
            'MSSQLAuthenMode':
                conn.authMethod == 'windows' ? 'WINDOWS' : 'SQLSERVER',
            'MSSQLAuthenWindowsDomain': '',
            'UserName': conn.username,
            'Password': _passwordCipher,
            'SavePassword': _savePassword,
            'SettingsSavePath': _settingsSavePath,
            'ConnectionParameters': '',
            'SessionLimit': '0',
            'InitialSessionQueries': '',
            'TimeoutReconnection': 'false',
            'UseConnectionTimeout': 'false',
            'UseExecutionTimeout': 'false',
            'TrustServerCertificate': 'false',
            'AutoConnect': 'false',
            'UseAdvanced': 'false',
            'UseEncryption': 'false',
            'NativeClientDriver': '',
            'SSH': 'false',
            'Remarks': '',
          }),
      'SQLITE' => base
        ..addAll(<String, String>{
            'DatabaseFileName': _host,
            'UserName': conn.username,
            'Password': '',
            'SavePassword': _savePassword,
            'SettingsSavePath': _settingsSavePath,
            'SessionLimit': '0',
            'InitialSessionQueries': '',
            'AutoConnect': 'false',
            'SQLiteEncrypt': conn.password.isEmpty ? 'false' : 'true',
            'SQLiteEncryptPassword':
                includePasswords ? encryptNavicatPassword(conn.password) : '',
            'SQLiteSaveEncryptPassword': includePasswords ? 'true' : 'false',
            'CipherName': '',
            'CipherLegacyPageSize': '0',
            'HTTP': 'false',
            'Remarks': '',
          }),
      _ => base,
    };
  }

  String toXml() {
    final sb = StringBuffer('<Connection');
    final attrs = Map<String, String>.of(_attrs);
    // daro 的连接分组:Navicat 原生没有这个概念(实测本机 378 条导出的 .ncx 里
    // 无任何 group/folder 属性,SettingsSavePath 也恒为 <根>\<类型>\Servers\<连接名>),
    // 故只能自带私有属性。追加在实测模板之后,不改动既有属性的相对顺序;
    // 未分组的连接不写该属性,导出的文件与不带分组时逐字节一致。
    final group = conn.group.trim();
    if (group.isNotEmpty) attrs['Group'] = group;
    attrs.forEach((key, value) {
      sb.write(' $key="${_escape(value)}"');
    });
    sb.write('/>');
    return sb.toString();
  }
}

/// XML 属性值转义。反斜杠不转义(Navicat 的 Windows 路径原样写出)。
String _escape(String value) {
  final sb = StringBuffer();
  for (final unit in value.runes) {
    switch (unit) {
      case 0x26: // &
        sb.write('&amp;');
      case 0x3c: // <
        sb.write('&lt;');
      case 0x3e: // >
        sb.write('&gt;');
      case 0x22: // "
        sb.write('&quot;');
      default:
        // XML 1.0 不允许的控制字符直接丢掉,否则整个文件无法被解析
        if (unit < 0x20 && unit != 0x09 && unit != 0x0a && unit != 0x0d) {
          continue;
        }
        sb.writeCharCode(unit);
    }
  }
  return sb.toString();
}

/// 用 Navicat 的 libcc 常量加密明文口令,输出大写十六进制密文。
///
/// 与 [decryptNavicatPassword] 严格互逆(test 里做双向对拍)。
String encryptNavicatPassword(String plain) {
  final key = Uint8List.fromList(utf8.encode(kNavicatLibccKey));
  final iv = Uint8List.fromList(utf8.encode(kNavicatLibccIv));
  final cipher = aesCbcEncrypt(
      key, iv, Uint8List.fromList(utf8.encode(plain)));
  final sb = StringBuffer();
  for (final b in cipher) {
    sb.write(b.toRadixString(16).toUpperCase().padLeft(2, '0'));
  }
  return sb.toString();
}
