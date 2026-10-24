import 'dart:convert';
import 'dart:io';

import 'package:daro/app/mcp_service.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/data/mcp_policy_store.dart';
import 'package:daro/mcp/mcp_policy.dart';
import 'package:daro/mcp/mcp_pool.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'mcp_fakes.dart';

/// 应用侧宿主([McpService])的行为契约:起停与原因上报、策略实时生效、
/// 审计落盘、改名迁移、活动计数接线。
///
/// 判定逻辑本身在 `mcp_policy_test.dart` / `mcp_tools_test.dart`,HTTP 报文细节在
/// `mcp_http_test.dart`;这里只管「接线是否正确」—— 也就是换一个端口会不会真的
/// 重听、改一次策略下一次调用会不会生效、审计有没有落下判定上下文。
void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  late Directory dir;

  setUp(() async {
    dir = await Directory.systemTemp.createTemp('daro_mcp_service');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
            const MethodChannel('plugins.flutter.io/path_provider'),
            (c) async => dir.path);
  });

  tearDown(() async {
    dir.deleteSync(recursive: true);
  });

  File policyFile() =>
      File('${dir.path}${Platform.pathSeparator}mcp_policy.json');
  File auditFile() =>
      File('${dir.path}${Platform.pathSeparator}mcp_audit.jsonl');

  /// 占住一个随机端口不还,用于「端口被占用」负例。
  Future<int> takenPort() async {
    final socket = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(socket.close);
    return socket.port;
  }

  /// 取一个刚释放的端口(bind 0 → close)。极小概率被别人抢走,重跑即可。
  Future<int> freePort() async {
    final socket = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
    final port = socket.port;
    await socket.close();
    return port;
  }

  /// 造一个 McpService。`healthGet` 默认注入假探测 —— 真实的 `HttpClient` 在
  /// `TestWidgetsFlutterBinding` 下永远返回 400(见 flutter_test 的 HttpOverrides),
  /// 所以探测的**判定**用注入验证,探测的**通路**由 mcp_http_test 覆盖。
  McpService newService({
    List<ConnectionInfo> Function()? connections,
    DriverFactorySpy? spy,
    McpPasswordAsker? askPassword,
    McpTableOpener? openTableBridge,
    McpHealthGet? healthGet,
  }) {
    final service = McpService(
      loadConnections: () async =>
          connections?.call() ?? [fakeConn('a', database: 'db_a')],
      askPassword: askPassword,
      openTableBridge: openTableBridge,
      healthGet: healthGet ?? (_) async => 200,
      pool: McpDriverPool(driverFactory: spy ?? DriverFactorySpy()),
    );
    // 收干净再删临时目录:Windows 上未 close 的审计文件句柄会让 deleteSync 报「占用」。
    addTearDown(() async {
      await service.shutdown();
      service.dispose();
    });
    return service;
  }

  /// 起宿主的策略:端口由测试现取,绝不碰默认 5225(开发机上可能有 daro 在跑)。
  McpPolicy serving({
    required int port,
    String host = '127.0.0.1',
    String token = '',
    McpMode defaultMode = McpMode.readonly,
    List<String> names = const ['a'],
    bool auditEnabled = true,
  }) =>
      enabledPolicy(defaultMode: defaultMode, names: names).copyWith(
        http: McpHttpSettings(host: host, port: port, token: token),
        auditEnabled: auditEnabled,
      );

  /// 走协议层入口的一次工具调用(等价于 HTTP 那侧的 tools/call,但不占端口)。
  Future<Map<String, dynamic>> callTool(McpService service, String toolName,
      Map<String, dynamic> args) async {
    final res = await service.protocolHandler.handle({
      'jsonrpc': '2.0',
      'id': 1,
      'method': 'tools/call',
      'params': {'name': toolName, 'arguments': args},
    });
    expect(res, isNotNull);
    return res!['result'] as Map<String, dynamic>;
  }

  Future<bool> portBindable(int port) async {
    try {
      final s = await ServerSocket.bind(InternetAddress.loopbackIPv4, port);
      await s.close();
      return true;
    } on SocketException {
      return false;
    }
  }

  /// 真实走一次 socket 上的 HTTP:只用来验接线(token 换了有没有重听、
  /// 并发计数有没有上报),报文细节交给 mcp_http_test。
  Future<({int status, String body})> rawPost(McpService service,
      Map<String, dynamic> message,
      {String? token}) async {
    final port = Uri.parse(service.endpoint).port;
    final payload = jsonEncode(message);
    final socket = await Socket.connect(InternetAddress.loopbackIPv4, port,
        timeout: const Duration(seconds: 3));
    try {
      final head = [
        'POST /mcp HTTP/1.1',
        'Host: 127.0.0.1:$port',
        'Content-Type: application/json',
        'Content-Length: ${utf8.encode(payload).length}',
        'Connection: close',
        if (token != null) 'Authorization: Bearer $token',
      ].join('\r\n');
      socket.write('$head\r\n\r\n$payload');
      await socket.flush();
      final bytes = <int>[];
      await for (final chunk in socket) {
        bytes.addAll(chunk);
      }
      final text = utf8.decode(bytes);
      final split = text.indexOf('\r\n\r\n');
      final statusLine = split < 0 ? text : text.substring(0, text.indexOf('\r\n'));
      final status = int.parse(RegExp(r'HTTP/1\.\d (\d{3})').firstMatch(statusLine)!.group(1)!);
      return (status: status, body: split < 0 ? '' : text.substring(split + 4));
    } finally {
      socket.destroy();
    }
  }

  /// 审计是异步单链落盘:轮询等够行数,别用固定 sleep 蒙。
  Future<List<String>> auditLines(int atLeast) async {
    final deadline = DateTime.now().add(const Duration(seconds: 5));
    while (DateTime.now().isBefore(deadline)) {
      if (await auditFile().exists()) {
        final lines = await auditFile().readAsLines();
        if (lines.length >= atLeast) return lines;
      }
      await Future<void>.delayed(const Duration(milliseconds: 20));
    }
    throw StateError('审计日志没在 5s 内写出 $atLeast 行');
  }

  group('启动与对账', () {
    test('没有策略文件 = 默认关闭,不绑任何端口', () async {
      // 先记下默认端口的**初始**归属:本机若正跑着一个开了 MCP 的 daro 实例
      // (开发者一边跑应用一边测是常态),端口本来就被那个进程占着,
      // 事后「仍不可绑」说明不了这个服务干了什么。只有开局是空的才判得准。
      final freeBefore = await portBindable(5225);
      final service = newService();
      await service.bootstrap();
      expect(service.policyStatus, McpPolicyLoadStatus.missing);
      expect(service.policy.enabled, isFalse);
      expect(service.isRunning, isFalse);
      if (freeBefore) {
        expect(await portBindable(5225), isTrue, reason: '默认端口不该被占');
      }
    });

    test('启用后端口在监听,关闭后端口释放', () async {
      final port = await freePort();
      final service = newService();
      await service.save(serving(port: port));
      expect(service.isRunning, isTrue);
      expect(service.startError, isNull);
      expect(await portBindable(port), isFalse);

      await service.save(service.policy.copyWith(enabled: false));
      expect(service.isRunning, isFalse);
      expect(await portBindable(port), isTrue);
    });

    test('端口被别的进程占了:不崩,并给出能自解释的原因', () async {
      final port = await takenPort();
      final service = newService();
      await service.save(serving(port: port));
      expect(service.isRunning, isFalse);
      expect(service.startError, contains('端口 $port 已被占用'));
      expect(service.startError, contains('换一个端口'));

      // 换端口后应当起得来(设置页改端口 → 保存)。
      final port2 = await freePort();
      await service.save(serving(port: port2));
      expect(service.isRunning, isTrue);
      expect(service.startError, isNull);
      expect(await portBindable(port2), isFalse);
    });

    test('绑非回环地址却没设 token:拒绝起服务并说明理由', () async {
      final service = newService();
      await service.save(serving(port: 5299, host: '192.0.2.1'));
      expect(service.isRunning, isFalse);
      expect(service.startError, contains('Token'));
      expect(await policyFile().exists(), isTrue, reason: '策略仍然落盘');
    });

    test('换 token 会重听:旧 token 401、新 token 200(客户端不必重启)', () async {
      final port = await freePort();
      final service = newService();
      await service.save(serving(port: port, token: 'tk-old'));
      final ping = {'jsonrpc': '2.0', 'id': 1, 'method': 'ping'};

      expect((await rawPost(service, ping, token: 'tk-old')).status, 200);
      await service.save(service.policy.copyWith(
          http:
              McpHttpSettings(host: '127.0.0.1', port: port, token: 'tk-new')));
      expect((await rawPost(service, ping, token: 'tk-old')).status, 401);
      expect((await rawPost(service, ping, token: 'tk-new')).status, 200);
    });

    test('bootstrap / reload 幂等:重复对账不会重起宿主', () async {
      final port = await freePort();
      final service = newService();
      await service.save(serving(port: port));
      await rawPost(service, {'jsonrpc': '2.0', 'id': 1, 'method': 'ping'});
      final seen = service.lastRequestAt;
      final before = service.endpoint;
      expect(seen, isNotNull, reason: '先来一次真请求,才测得出宿主有没有被换掉');

      await service.bootstrap();
      await service.reload();
      expect(service.isRunning, isTrue);
      expect(service.endpoint, before);
      expect(service.lastRequestAt, seen, reason: '宿主被重建了:请求历史丢了');
      expect(await portBindable(port), isFalse);
      final res = await service.protocolHandler
          .handle({'jsonrpc': '2.0', 'id': 1, 'method': 'ping'});
      expect(res!['result'], isNotNull);
    });
  });

  group('策略实时生效(核心语义)', () {
    test('档位从只读放宽到读写:同一宿主实例下一次调用即生效', () async {
      final spy = DriverFactorySpy();
      final service = newService(spy: spy);
      await service.save(serving(port: await freePort()));

      final denied = await callTool(service, 'daro_execute_query', {
        'connection': 'a',
        'database': 'db_a',
        'sql': 'DELETE FROM users WHERE id = 1'
      });
      expect(denied['isError'], isTrue);
      expect(((denied['structuredContent'] as Map)['error'] as Map)['code'],
          'MODE_DENIED');

      await service
          .save(service.policy.copyWith(defaultMode: McpMode.readWrite));
      final allowed = await callTool(service, 'daro_execute_query', {
        'connection': 'a',
        'database': 'db_a',
        'sql': 'DELETE FROM users WHERE id = 1'
      });
      expect(allowed['isError'], isFalse, reason: '$allowed');
      expect(spy.connectCountOf('a'), 1, reason: '同键复用池实例,不该二次握手');
    });

    test('收紧到不再授权该连接:下一次调用即被拒', () async {
      final service = newService();
      await service.save(serving(port: await freePort()));
      expect(
          (await callTool(service, 'daro_list_tables',
                  {'connection': 'a', 'database': 'db_a'}))['isError'],
          isFalse);

      await service.save(service.policy.copyWith(
          connectionScopeAll: false, connectionNames: const []));
      final second = await callTool(service, 'daro_list_tables',
          {'connection': 'a', 'database': 'db_a'});
      expect(second['isError'], isTrue);
      expect(((second['structuredContent'] as Map)['error'] as Map)['code'],
          'CONNECTION_NOT_VISIBLE');
    });

    test('外部改策略文件后 reload 生效(工具侧本就直接读盘)', () async {
      final service = newService();
      await service.save(serving(port: await freePort()));
      // 模拟用户手改文件 / 另一实例写入:盘上换了档位,内存快照仍旧。
      await policyFile().writeAsString(
          jsonEncode(service.policy.copyWith(defaultMode: McpMode.full).toJson()));
      final viaDisk = await callTool(service, 'daro_execute_query', {
        'connection': 'a',
        'database': 'db_a',
        'sql': 'DROP TABLE users'
      });
      expect(viaDisk['isError'], isFalse,
          reason: '工具侧不看内存快照:$viaDisk');

      await service.reload();
      expect(service.policy.defaultMode, McpMode.full);
    });

    test('关掉服务:端口立即释放,agent 是连不上而不是拿到空结果', () async {
      final port = await freePort();
      final service = newService();
      await service.save(serving(port: port));
      await service.save(service.policy.copyWith(enabled: false));
      expect(await portBindable(port), isTrue);
    });
  });

  group('审计日志', () {
    test('每次调用落一行 JSONL,被拒的调用也带判定上下文', () async {
      final service = newService();
      await service.save(serving(port: await freePort()));

      await callTool(service, 'daro_execute_query',
          {'connection': 'a', 'database': 'db_a', 'sql': 'SELECT 1'});
      await callTool(service, 'daro_execute_query',
          {'connection': 'ghost', 'database': 'db_a', 'sql': 'SELECT 1'});
      final lines = await auditLines(2);

      final ok = jsonDecode(lines[0]) as Map<String, dynamic>;
      expect(ok['tool'], 'daro_execute_query');
      expect(ok['ok'], isTrue);
      expect(ok['mode'], 'readonly');
      expect(ok['sqlClass'], 'readOnly');
      expect(ok['limit'], 100);
      expect(ok['sqlDigest'], 'SELECT 1');
      expect(ok['ms'], isA<int>());

      final bad = jsonDecode(lines[1]) as Map<String, dynamic>;
      expect(bad['ok'], isFalse);
      expect(bad['error'], 'CONNECTION_NOT_VISIBLE');
      // 被拒的调用也要留判定上下文,否则日志里只剩一串错误码。
      expect(bad['mode'], 'readonly');
      expect(bad['sqlClass'], 'readOnly');

      expect(
          await auditFile().readAsString(), isNot(contains('pw-saved')),
          reason: '审计里绝不能有密码');
    });

    test('关掉审计后不再落盘', () async {
      final service = newService();
      await service.save(serving(port: await freePort()));
      await callTool(service, 'daro_list_connections', {});
      await auditLines(1);

      await service.save(service.policy.copyWith(auditEnabled: false));
      await callTool(service, 'daro_list_connections', {});
      await Future<void>.delayed(const Duration(milliseconds: 150));
      expect(await auditFile().readAsLines(), hasLength(1));
    });
  });

  group('改名迁移(W12 的地基)', () {
    test('授权过的连接改名:allowlist 与规则同步迁移并落盘', () async {
      final service = newService();
      await service.save(enabledPolicy(
              names: ['a', 'b'],
              rules: [
                McpConnectionRule(
                    connection: 'a',
                    mode: McpMode.readWrite,
                    databaseScope: const McpDatabaseScope.only(['db_a'])),
              ])
          .copyWith(enabled: false));

      expect(service.authorizes('a'), isTrue);
      await service.renameConnection('a', 'a2');

      expect(service.policy.connectionNames, ['a2', 'b']);
      final rule = service.policy.connections.single;
      expect(rule.connection, 'a2');
      expect(rule.mode, McpMode.readWrite);
      expect(rule.databaseScope.include, ['db_a']);
      // 工具侧每次调用重新读盘,所以盘上必须同步。
      final onDisk = await McpPolicyStore().load();
      expect(onDisk.authorizesConnection('a'), isFalse);
      expect(onDisk.authorizesConnection('a2'), isTrue);
      expect(service.authorizes('a'), isFalse);
    });

    test('未授权的连接改名:不动策略文件,也不牵连别人的池实例', () async {
      final spy = DriverFactorySpy();
      final service = newService(spy: spy);
      await service.save(serving(port: await freePort()));
      // 让 'a' 在池里有一条活连接,再改一个**没授权过**的名字。
      await callTool(service, 'daro_list_tables',
          {'connection': 'a', 'database': 'db_a'});
      expect(service.pooledDrivers, 1);
      final before = await policyFile().readAsString();

      await service.renameConnection('never-authorized', 'x');

      expect(await policyFile().readAsString(), before);
      expect(service.pooledDrivers, 1, reason: '无关改名不该把在用连接踢掉');
      expect(spy.listOf('a').single.closeCount, 0);
    });

    test('改名后旧池实例被销毁,下家重新握手', () async {
      final spy = DriverFactorySpy();
      final service = newService(spy: spy);
      await service.save(serving(port: await freePort()));
      await callTool(service, 'daro_list_tables',
          {'connection': 'a', 'database': 'db_a'});
      expect(spy.instancesOf('a'), 1);
      expect(spy.listOf('a').single.closeCount, 0);
      expect(service.pooledDrivers, 1);

      await service.renameConnection('a', 'a2');
      expect(spy.listOf('a').single.closeCount, 1);
      expect(service.pooledDrivers, 0);
    });
  });

  group('健康检查与活动计数', () {
    test('未启用 / 起不来 / 正常 三种状态各给一句可执行的话', () async {
      final service = newService();
      expect(await service.probe(), contains('未启用'));

      final port = await takenPort();
      await service.save(serving(port: port));
      expect(await service.probe(), contains('端口 $port 已被占用'));

      await service.save(serving(port: await freePort()));
      expect(await service.probe(), isNull);
      expect(service.activeCalls, 0);
    });

    test('探测通路异常时报错带上端点与原因', () async {
      final service = newService(healthGet: (url) async => 500);
      await service.save(serving(port: await freePort()));
      expect(await service.probe(), contains('HTTP 500'));

      final broken = newService(
          healthGet: (_) async => throw const SocketException('boom'));
      await broken.save(serving(port: await freePort()));
      expect(await broken.probe(), contains('boom'));
    });

    test('调用进行中 activeCalls 上涨、结束后归零(状态栏接线)', () async {
      final port = await freePort();
      final spy = DriverFactorySpy()
        ..tune = (d) => d.executeDelay = const Duration(milliseconds: 500);
      final service = newService(spy: spy);
      await service.save(serving(port: port));

      final pending = rawPost(service, {
        'jsonrpc': '2.0',
        'id': 1,
        'method': 'tools/call',
        'params': {
          'name': 'daro_execute_query',
          'arguments': {
            'connection': 'a',
            'database': 'db_a',
            'sql': 'SELECT 1'
          }
        }
      }, token: '');
      await Future<void>.delayed(const Duration(milliseconds: 150));
      expect(service.activeCalls, 1, reason: '计数没接上:状态栏会永远显示 0');
      expect((await pending).status, 200);
      expect(service.activeCalls, 0);
      expect(service.lastRequestAt, isNotNull);
    });

    test('UI 桥:openTableBridge 收到工具给的 连接/库/表 三元组', () async {
      final opened = <List<String>>[];
      final service = newService(
          openTableBridge: (c, d, t) async => opened.add([c, d, t]));
      await service.save(serving(port: await freePort()));
      final out = await callTool(service, 'daro_open_table',
          {'connection': 'a', 'database': 'db_a', 'table': 'users'});
      expect(out['isError'], isFalse, reason: '$out');
      expect(opened, [
        ['a', 'db_a', 'users']
      ]);
    });
  });
}
