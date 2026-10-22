import 'dart:convert';
import 'dart:io';

import 'package:daro/data/db_data.dart';
import 'package:daro/data/drivers/db_driver.dart';
import 'package:flutter_test/flutter_test.dart';

/// **实连**回归:含裸字节列(BLOB / VARBINARY / 几何 / bit)的表在真服务端上打得开。
///
/// 离线单测只能验投影函数([mysqlSafeProjectionOf] 的纯逻辑),验不了这条链的
/// 前提:服务端真的会发裸字节、而 mysql_client 真的会在包内 UTF-8 解码时抛
/// `FormatException`(实测连接「本部 11」的 `bx11.bx_wxcoupon_mch`,
/// `cert_file blob` 里存的是 PKCS#12 证书)。
///
/// 全程只读(只 SELECT,绝不写、绝不 DDL)。
/// 默认**不跑**:`DARO_LIVE_DB=1 flutter test test/mysql_binary_column_live_test.dart`
const _preferConnName = '本部 11';
const _preferDatabase = 'bx11';
const _preferTable = 'bx_wxcoupon_mch';

/// 与 [ConnectionStore] 相同落点(path_provider 在测试宿主里不可用,直读文件)
File? _storeFile() {
  final env = Platform.environment;
  final sep = Platform.pathSeparator;
  for (final root in [env['APPDATA'], env['LOCALAPPDATA']]) {
    if (root == null || root.isEmpty) continue;
    for (final appId in ['daro', 'db_lite']) {
      final f =
          File('$root$sep' 'com.example$sep$appId$sep' 'connections.json');
      if (f.existsSync()) return f;
    }
  }
  return null;
}

/// 只认**指定那条**连接:别的都是生产实例,不在本机测试里挨个连一遍
ConnectionInfo? _mysqlConnection() {
  final file = _storeFile();
  if (file == null) return null;
  try {
    final json = jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
    for (final raw in (json['connections'] as List)) {
      final c = ConnectionInfo.fromJson(raw as Map<String, dynamic>);
      if (c.isLive && (c.typeId == 'mysql' || c.typeId == 'mariadb') &&
          c.name == _preferConnName) {
        return c;
      }
    }
    return null;
  } catch (_) {
    return null;
  }
}

/// 找一张「`SELECT *` 真的会炸」的表:`(库, 表, 裸字节列)`;找不到返回 null。
///
/// 有裸字节列还不够——值恰好是合法 UTF-8 时(如 `bit(1)` 的 0/1、存 ASCII 的
/// BLOB)解码不会失败,走的是原来的 `SELECT *` 路径,`hexColumns` 自然为空。
/// 所以必须实测一次 `SELECT *` 抛不抛,才验得到恢复投影那条分支。
Future<(String, String, String)?> _pickCrashingTable(DatabaseDriver driver) async {
  const binaryTypes =
      "'binary','varbinary','tinyblob','blob','mediumblob','longblob',"
      "'geometry','point','linestring','polygon'";
  final rs = await driver.executeQuery(
    'SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS '
    'WHERE DATA_TYPE IN ($binaryTypes) '
    "AND TABLE_SCHEMA NOT IN ('mysql','information_schema','performance_schema','sys') "
    'LIMIT 500',
    limit: 500,
  );
  final candidates = <List<String>>[];
  for (final r in rs.rows) {
    if (candidates.any((c) => c[0] == r[0] && c[1] == r[1])) continue;
    candidates.add([r[0], r[1], r[2]]);
    if (candidates.length >= 12) break;
  }
  // 报告里那张证书表优先,其次首选库,最后任意
  candidates.sort((a, b) {
    int rank(List<String> r) => r[1] == _preferTable
        ? 0
        : (r[0] == _preferDatabase ? 1 : 2);
    return rank(a).compareTo(rank(b));
  });
  for (final c in candidates) {
    final db = c[0];
    final table = c[1];
    var crashes = false;
    try {
      await driver.executeQuery('SELECT * FROM `${db}`.`${table}` LIMIT 1');
    } on Exception catch (e) {
      crashes = '$e'.contains('UTF-8');
    }
    if (crashes) return (db, table, c[2]);
  }
  return null;
}

void main() {
  final enabled = Platform.environment['DARO_LIVE_DB'] == '1';
  final conn = enabled ? _mysqlConnection() : null;
  final skip = !enabled
      ? '未开启实连(DARO_LIVE_DB=1 时才跑)'
      : conn == null
          ? '未找到「$_preferConnName」这条 MySQL 系连接(需本机 connections.json)'
          : null;
  if (skip != null) {
    // ignore: avoid_print
    print('实连测试跳过:$skip');
    test('实连:含裸字节列的表(跳过)', () {}, skip: skip);
    return;
  }

  late DatabaseDriver driver;
  String? db;
  String? table;
  String? column;

  setUpAll(() async {
    driver = createDriver(conn!)!;
    await driver.connect();
    final picked = await _pickCrashingTable(driver);
    db = picked?.$1;
    table = picked?.$2;
    column = picked?.$3;
    // ignore: avoid_print
    print('  样本:${db ?? '(没有 SELECT * 会炸的表)'}.$table.$column');
  });

  tearDownAll(() async => driver.close());

  test('实连:SELECT * 打不开的表,预览能自动换安全投影取回', () async {
    if (db == null) return; // 该实例上没有裸字节表,无从验证
    final preview = await driver.previewTable(db!, table!, limit: 5);
    expect(preview.columns, contains(column));
    // 二进制列必须被登记为十六进制列,否则回写会按文本存(静默改错数据)
    expect(preview.hexColumns, contains(column),
        reason: '驱动没换投影,或换了没回报');
    final idx = preview.columns.indexOf(column!);
    for (final row in preview.rows) {
      final v = row[idx];
      expect(
          v == 'NULL' || RegExp(r'^([0-9a-fA-F]{2})*$').hasMatch(v), isTrue,
          reason: '取回的应该是十六进制转储,实际是「$v」');
    }
  });

  test('实连:同一张表重复预览结果稳定(剥壳幂等,不会越读越长)', () async {
    if (db == null) return;
    final a = await driver.previewTable(db!, table!, limit: 5);
    final b = await driver.previewTable(db!, table!, limit: 5);
    expect(b.rows, a.rows);
    expect(b.hexColumns, a.hexColumns);
  });
}
