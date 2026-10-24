import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:daro/data/navicat_import.dart';
import 'package:flutter_test/flutter_test.dart';

/// Navicat 密码解密的基准向量。
///
/// 密文由 Navicat 内置常量(AES-128-CBC,key=`libcckeylibcckey`,
/// iv=`libcciv libcciv `)对**测试专用假口令**加密得到,
/// 不含任何真实环境口令;空串密文则与本机真实导出文件里的空密码连接一致。
const _libccVectors = <String, String>{
  '': 'E191AF42327478CC5F143EF279EC4D81',
  'Test@1234': 'ACCD060DF11B61DB7730FA7791F5B178',
  'p@ssw0rd-mysql': 'D079CE5CDFD2DBB3DE1C9AD758B6989F',
  'A-str Very Long Passw0rd!':
      '07FBC0BFBCADE8812E5A741E5395F0EFBE5CE18EB291B0EF02FBFBFD1EFDE98D',
  '密@码abc': 'E1A665FFE48BC7F17E732A86D452AA44',
};

const _ncxFixture = '''
<?xml version="1.0" encoding="UTF-8"?>
<Connections Ver="1.5">
	<Connection ConnectionName="demo_mysql" ProjectUUID="" ConnType="MYSQL" ServiceProvider="Default" Host="db.example.com" Port="3306" UserName="root" Password="D079CE5CDFD2DBB3DE1C9AD758B6989F" SavePassword="true" Encoding="65001" SSL="false" SSH="false" Remarks=""/>
	<Connection ConnectionName="demo_pg" ConnType="POSTGRESQL" Host="pg.example.com" Port="5432" Database="appdb" UserName="postgres" Password="ACCD060DF11B61DB7730FA7791F5B178" SavePassword="true"/>
	<Connection ConnectionName="demo_mssql_win" ConnType="SQLSERVER" Host="localhost\\SQLEXPRESS" Port="1433" Database="master" MSSQLAuthenMode="WINDOWS" UserName="" Password="E191AF42327478CC5F143EF279EC4D81" SavePassword="true"/>
	<Connection ConnectionName="demo_lite" ConnType="SQLITE" DatabaseFileName="C:\\tmp\\orderdb.db" UserName="" Password="" SavePassword="true" SQLiteEncrypt="false"/>
	<Connection ConnectionName="demo_oracle" ConnType="ORACLE" Host="ora.example.com" Port="1521" Database="ORCL" UserName="scott" Password="ACCD060DF11B61DB7730FA7791F5B178" SavePassword="true"/>
	<Connection ConnectionName="demo_no_save" ConnType="MYSQL" Host="legacy.example.com" Port="3306" UserName="app" Password="" SavePassword="false"/>
	<Connection ConnectionName="demo_old_fmt" ConnType="MYSQL" Host="old.example.com" Port="3306" UserName="app" Password="414243444546474" SavePassword="true"/>
	<Connection ConnectionName="a&amp;b" ConnType="MYSQL" Host="ent.example.com" Port="3306" UserName="u" Password="" SavePassword="false"/>
</Connections>
''';

/// `.ano` 的**裸数组形**(Another Redis Desktop Manager 旧版「导出连接」):
/// 整份文件是 base64 的 JSON 数组。
///
/// 结构照本机真实导出文件(`host / port / auth / username / name / separator /
/// cluster / connectionReadOnly / key / order / connectionName`)复刻,
/// 主机与口令**全是假值**,不含任何真实实例凭据。
/// 注意 `port` 在真实文件里数字与字符串混写,这里两种都留一条。
final String _anoFixture = base64Encode(utf8.encode(jsonEncode([
  {
    'host': 'r-fake-instance.redis.rds.example.com',
    'port': 6379,
    'auth': 'Test@1234',
    'username': '',
    'name': '一部',
    'separator': ':',
    'cluster': false,
    'connectionReadOnly': false,
    'key': '1680846449382_3kp4n',
    'order': 0,
    'connectionName': '一部',
  },
  {
    'host': 'hex.redis.example.com',
    'port': 6379,
    'auth': 'abcdef1234567890',
    'username': '',
    'name': 'hex_like_auth',
    'separator': ':',
    'cluster': false,
    'connectionReadOnly': false,
    'key': '1680846449382_aaaa1',
    'order': 1,
    'connectionName': 'hex_like_auth',
  },
  {
    'host': 'open.redis.example.com',
    'port': 6379,
    'auth': '',
    'username': '',
    'name': 'no_auth',
    'separator': ':',
    'cluster': false,
    'connectionReadOnly': false,
    'key': '1680846449382_bbbb2',
    'order': 2,
    'connectionName': 'no_auth',
  },
  {
    'host': 'named.redis.example.com',
    'port': '6380',
    'auth': 'Test@1234',
    'username': '',
    'name': 'only_name',
    'separator': ':',
    'cluster': false,
    'connectionReadOnly': false,
    'key': '1680846449382_cccc3',
    'order': 3,
  },
  {
    'host': 'cluster.redis.example.com',
    'port': '9099',
    'auth': 'Test@1234',
    'username': '',
    'name': '集群从节点',
    'separator': ':',
    'cluster': true,
    'connectionReadOnly': true,
    'key': '1680846449382_dddd4',
    'order': 4,
    'connectionName': 'cluster_node',
  },
])));

/// `.ano` 的**新版形**:ARDM 的 `storage.exportConnectionsBundle()` 写出
/// `{connections: [...], groups: [{id, name}]}`,连接靠 `groupId` 挂到分组上。
/// 分组名与连接名同样是假值。
final String _anoBundleFixture = base64Encode(utf8.encode(jsonEncode({
  'connections': [
    {
      'host': 'prod.redis.example.com',
      'port': 6379,
      'auth': 'Test@1234',
      'name': '生产主节点',
      'connectionName': '生产主节点',
      'groupId': 'g_prod',
      'key': '1680846449382_eeee5',
      'order': 0,
    },
    {
      'host': 'local.redis.example.com',
      'port': 6379,
      'auth': '',
      'name': '本机',
      'connectionName': '本机',
      'groupId': null,
      'key': '1680846449382_ffff6',
      'order': 1,
    },
    {
      'host': 'orphan.redis.example.com',
      'port': 6379,
      'auth': 'Test@1234',
      'name': '分组已被删',
      'connectionName': '分组已被删',
      'groupId': 'g_gone',
      'key': '1680846449382_gggg7',
      'order': 2,
    },
  ],
  'groups': [
    {'id': 'g_prod', 'name': '生产集群'},
    {'id': 'g_test', 'name': '测试'},
  ],
})));

void main() {
  group('AES-128-CBC 原语', () {    test('与 FIPS-197 官方向量一致(零 IV 单块即 ECB 解密)', () {
      final key = Uint8List(16);
      final iv = Uint8List(16);
      final ct = _hex('66e94bd4ef8a2c3b884cfa59ca342b2e');
      expect(aesCbcDecrypt(key, iv, ct), _hex('00000000000000000000000000000000'));
    });

    test('加密→解密往返保持任意长度明文(解密结果含 PKCS#7 填充)', () {
      final key = Uint8List.fromList(utf8.encode(kNavicatLibccKey));
      final iv = Uint8List.fromList(utf8.encode(kNavicatLibccIv));
      for (final len in [0, 1, 15, 16, 17, 32, 33]) {
        final plain = Uint8List.fromList(
            List<int>.generate(len, (i) => 0x21 + (i % 0x5d)));
        final back = aesCbcDecrypt(key, iv, aesCbcEncrypt(key, iv, plain));
        expect(back.length, len + (16 - len % 16), reason: 'len=$len');
        expect(back.sublist(0, len), plain, reason: 'len=$len');
      }
    });
  });

  group('Navicat Password 解密', () {
    test('基准向量逐个还原', () {
      _libccVectors.forEach((plain, cipher) {
        final r = decryptNavicatPassword(cipher);
        expect(r.state, NavicatPasswordState.decrypted, reason: cipher);
        expect(r.password, plain, reason: cipher);
      });
    });

    test('小写十六进制同样可解(Navicat 实际写大写)', () {
      final r = decryptNavicatPassword(
          _libccVectors['Test@1234']!.toLowerCase());
      expect(r.password, 'Test@1234');
    });

    test('未保存密码 / 本就不需要密码 两种情形区分开', () {
      expect(decryptNavicatPassword('', saved: false).state,
          NavicatPasswordState.notSaved);
      expect(decryptNavicatPassword('').state, NavicatPasswordState.noPassword);
    });

    test('旧版 Blowfish 等解不出的密文判为 undecryptable,不产出半成品明文', () {
      for (final bad in [
        '414243444546474', // 奇数长度
        'zz79CE5CDFD2DBB3DE1C9AD758B6989F', // 非十六进制
        '00112233445566778899aabbcc', // 14 字节:不是 16 的整数倍
        'D079CE5CDFD2DBB3DE1C9AD758B69890', // 改动末字节 → 填充非法
        'Ymxvd2Zpc2ggbGVnYWN5IHBhc3N3b3Jk', // 旧版 Base64 形态
      ]) {
        expect(decryptNavicatPassword(bad).state,
            NavicatPasswordState.undecryptable,
            reason: bad);
      }
    });
  });

  group('NCX 解析与映射', () {
    final ncx = NavicatNcx.parse(_ncxFixture);
    NavicatConnection entry(String name) =>
        ncx.connections.firstWhere((e) => e.name == name);

    test('读出全部连接并保留导出格式版本', () {
      expect(ncx.connections.length, 8);
      expect(ncx.formatVersion, '1.5');
    });

    test('MySQL:host:port + 用户名 + 解密后的密码', () {
      final e = entry('demo_mysql');
      final c = e.toConnection();
      expect(e.typeId, 'mysql');
      expect(e.isSupported, isTrue);
      expect(c.host, 'db.example.com');
      expect(c.port, '3306');
      expect(c.username, 'root');
      expect(e.passwordState, NavicatPasswordState.decrypted);
      expect(c.password, 'p@ssw0rd-mysql');
      // NCX 的 MySQL 连接不带默认库(与新建连接表单一致:空则连上后列全部库)
      expect(c.database, '');
    });

    test('PostgreSQL:带默认库', () {
      final e = entry('demo_pg');
      final c = e.toConnection();
      expect(c.host, 'pg.example.com');
      expect(c.port, '5432');
      expect(c.database, 'appdb');
      expect(c.password, 'Test@1234');
    });

    test('SQL Server Windows 身份验证:authMethod=windows 且不落端口', () {
      final e = entry('demo_mssql_win');
      final c = e.toConnection();
      expect(c.typeId, 'sqlserver');
      expect(c.authMethod, 'windows');
      // 表单约定:SQL Server 不写端口(含命名实例时按 host 寻址)
      expect(c.port, '');
      expect(c.host, r'localhost\SQLEXPRESS');
      // 空密码密文解出空串:Windows 验证本就不用密码
      expect(e.passwordState, NavicatPasswordState.decrypted);
      expect(c.password, '');
    });

    test('SQLite:文件路径同时落到 host 与 database(读取端约定)', () {
      final e = entry('demo_lite');
      final c = e.toConnection();
      expect(e.isFileBased, isTrue);
      expect(c.host, r'C:\tmp\orderdb.db');
      expect(c.database, r'C:\tmp\orderdb.db');
      expect(c.port, '');
      expect(e.passwordState, NavicatPasswordState.noPassword);
    });

    test('未实现的引擎标为不可导入', () {
      expect(entry('demo_oracle').isSupported, isFalse);
    });

    test('未保存密码与解不出的密文都提示需手填', () {
      final unsaved = entry('demo_no_save');
      final legacy = entry('demo_old_fmt');
      expect(unsaved.passwordState, NavicatPasswordState.notSaved);
      expect(unsaved.needsManualPassword, isTrue);
      expect(legacy.passwordState, NavicatPasswordState.undecryptable);
      expect(legacy.needsManualPassword, isTrue);
      // 不写假密码
      expect(legacy.toConnection().password, '');
    });

    test('XML 实体在属性值里正确还原', () {
      expect(ncx.connections.last.name, 'a&b');
    });

    test('所有连接标为真实连接(走驱动加载元数据)', () {
      for (final e in ncx.connections) {
        expect(e.toConnection().isLive, isTrue);
      }
    });

    test('格式不符时给出中文原因', () {
      expect(() => NavicatNcx.parse('<NotNavicat/>'),
          throwsA(isA<FormatException>()));
      expect(() => NavicatNcx.parse('not xml at all'),
          throwsA(isA<FormatException>()));
      expect(() => NavicatNcx.parse('<Connections></Connections>'),
          throwsA(isA<FormatException>()));
    });
  });

  group('.ano(Redis)解析与映射', () {
    final ano = NavicatNcx.parse(_anoFixture);
    NavicatConnection entry(String name) =>
        ano.connections.firstWhere((e) => e.name == name);

    test('读出全部连接,类型归一到 redis 且可导入', () {
      expect(ano.connections.length, 5);
      expect(ano.formatVersion, '', reason: '.ano 不声明导出格式版本');
      for (final e in ano.connections.take(4)) {
        expect(e.typeId, 'redis');
        // 集群版那条单独挡,见下
        expect(e.isSupported, isTrue, reason: e.name);
      }
    });

    test('auth 按明文直取:十六进制形状的口令也不会被当成 Navicat 密文', () {
      // 'abcdef1234567890' 是偶数长度纯 hex —— 若误走 AES 分支就会变成
      // undecryptable 或一串乱码,这里是回归守护
      final e = entry('hex_like_auth');
      expect(e.passwordState, NavicatPasswordState.decrypted);
      expect(e.password, 'abcdef1234567890');
      expect(e.toConnection().password, 'abcdef1234567890');
    });

    test('空 auth = 这个实例本就没设 requirepass,不报「需手填密码」', () {
      final e = entry('no_auth');
      expect(e.passwordState, NavicatPasswordState.noPassword);
      expect(e.needsManualPassword, isFalse);
      expect(e.toConnection().password, '');
    });

    test('port 数字与字符串两种写法都收成字符串', () {
      expect(entry('一部').port, '6379');
      expect(entry('cluster_node').port, '9099');
    });

    test('名称优先 connectionName,缺失时退回 name', () {
      expect(entry('一部').attr('ConnectionName'), '一部');
      expect(entry('only_name').name, 'only_name');
    });

    test('集群版标记挡下:给出原因、不可勾选,而不是导一条点开就报错的连接', () {
      final e = entry('cluster_node');
      expect(e.cluster, isTrue);
      expect(e.isSupported, isFalse);
      expect(e.unsupportedReason, contains('集群'));
      // 未实现的引擎同样走这条出口(旧 UI 文案是「不支持 MYSQL」那种)
      expect(entry('一部').unsupportedReason, isNull);
    });

    test('映射出的连接:无分组、无默认库、真实连接', () {
      final e = entry('一部');
      final conn = e.toConnection();
      expect(conn.typeId, 'redis');
      expect(conn.host, 'r-fake-instance.redis.rds.example.com');
      expect(conn.username, '');
      expect(conn.database, '', reason: 'Redis 的库号在连接树里选,不落默认库');
      expect(conn.group, '');
      expect(conn.isLive, isTrue);
      expect(e.targetLabel, 'r-fake-instance.redis.rds.example.com:6379');
    });

    test('base64 折行(带换行)也能解', () {
      final folded = _fold(_anoFixture);
      expect(NavicatNcx.parse(folded).connections.length, 5);
    });

    test('.ano 里塞进非数组内容时给中文原因,不静默产出空列表', () {
      expect(
        () => NavicatNcx.parse(base64Encode(utf8.encode('{"host":"x"}'))),
        throwsA(isA<FormatException>()),
      );
      expect(
        () => NavicatNcx.parse(base64Encode(utf8.encode('[]'))),
        throwsA(isA<FormatException>()),
      );
    });

    test('XML 与 .ano 由同一入口按内容识别,后缀不参与判断', () {
      // 把 XML 文件改名成 .ano 也照样按 XML 解
      expect(NavicatNcx.parse(_ncxFixture).connections.length, 8);
    });
  });

  group('.ano 新版根对象 {connections, groups}', () {
    final ano = NavicatNcx.parse(_anoBundleFixture);
    NavicatConnection entry(String name) =>
        ano.connections.firstWhere((e) => e.name == name);

    test('connections 列表照单全收,与裸数组形走同一套映射', () {
      expect(ano.connections.length, 3);
      final e = entry('生产主节点');
      expect(e.typeId, 'redis');
      expect(e.isSupported, isTrue);
      expect(e.passwordState, NavicatPasswordState.decrypted);
      expect(e.toConnection().host, 'prod.redis.example.com');
    });

    test('groupId 反查 groups 得到分组名,并随连接一起落到 ConnectionInfo.group', () {
      expect(entry('生产主节点').group, '生产集群');
      expect(entry('生产主节点').toConnection().group, '生产集群');
    });

    test('无 groupId / 分组已被删的连接按「未分组」处理,不编出分组名', () {
      expect(entry('本机').group, '');
      expect(entry('分组已被删').group, '');
    });

    test('groups 缺失或为空时仍能解(旧文件与空分组表同一条路径)', () {
      List<Object?> conns() =>
          (jsonDecode(utf8.decode(base64Decode(_anoBundleFixture)))
                  as Map<String, Object?>)['connections']!
              as List<Object?>;
      for (final body in [
        {'connections': conns()},
        {'connections': conns(), 'groups': []},
      ]) {
        expect(NavicatNcx.parse(base64Encode(utf8.encode(jsonEncode(body))))
            .connections
            .length, 3);
      }
    });

    test('JSON 根对象没有 connections 列表时给中文原因,不静默产出空列表', () {
      expect(
        () => NavicatNcx.parse(base64Encode(utf8.encode('{"groups":[]}'))),
        throwsA(isA<FormatException>()),
      );
    });
  });

  // 真实导出文件的抽样回归:开关口径与实连数据库测试一致 —— 只有显式给出路径
  // 才跑(默认跳过,不依赖任何人的个人目录)。
  // 断言只看条数 / 类型 / 端口 / 密码「可得性」这些形状,**不打印也不比对口令值**。
  group('真实 .ano 抽样(按需)', () {
    final path = Platform.environment['DARO_NAVICAT_ANO'] ?? '';
    final skip = path.isEmpty ? '未给出导出文件路径(DARO_NAVICAT_ANO)' : null;

    test('解析真实 Redis 导出:条数、类型、寻址与口令状态', () {
      final parsed = NavicatNcx.readFileSync(path);
      expect(parsed.connections, isNotEmpty);
      for (final e in parsed.connections) {
        expect(e.typeId, 'redis', reason: e.name);
        expect(e.isSupported, isTrue, reason: '${e.name}:${e.unsupportedReason}');
        expect(e.host, isNotEmpty, reason: e.name);
        expect(int.tryParse(e.port), greaterThan(0), reason: e.name);
        // 该文件是勾选「导出密码」导出的:每条都该带明文 auth
        expect(e.passwordState, NavicatPasswordState.decrypted, reason: e.name);
        expect(e.password, isNotEmpty, reason: e.name);
        expect(e.toConnection().isLive, isTrue);
      }
    }, skip: skip);
  });
}

Uint8List _hex(String s) => Uint8List.fromList(
    List<int>.generate(s.length ~/ 2, (i) => int.parse(s.substring(i * 2, i * 2 + 2), radix: 16)));

/// 按 76 列折行(Navicat 写出 base64 时的排版),用来验证解码前剔空白。
String _fold(String s) => [
      for (var i = 0; i < s.length; i += 76)
        i + 76 < s.length ? s.substring(i, i + 76) : s.substring(i)
    ].join('\r\n');
