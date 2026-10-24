import 'package:flutter/foundation.dart';

import '../data/db_data.dart';
import '../data/drivers/kv_driver.dart';
import 'connection_manager.dart' show LoadStatus;

/// 一个 KV 连接下「命名空间列表」的加载状态(Redis = db0..db15)。
class KvNamespaceListState {
  KvNamespaceListState({
    this.status = LoadStatus.idle,
    this.namespaces = const [],
    this.error,
  });

  LoadStatus status;
  List<KvNamespace> namespaces;
  String? error;

  KvNamespace? namespace(String id) =>
      namespaces.where((n) => n.id == id).firstOrNull;
}

/// 缓存 / 键值型引擎(Redis 等)的连接与元数据管理。
///
/// 为什么与 SQL 侧的 [ConnectionManager] 分成两个类而不是加几个方法:
/// 那边 40 多个方法全都围绕「库 → 模式 → 表 → 列」这套关系型层级,
/// 缓存型引擎只有「命名空间 → 键」两层,套进去会得到一堆恒抛或恒空的桩。
/// 两个类各持一份驱动字典,连接名相同也不冲突(一条连接只可能属于一侧,
/// 判据见 kKvStoreTypes 与 kSupportedDriverTypes 互斥,由 kv_driver_test 守护)。
class KvConnectionManager extends ChangeNotifier {
  /// 连接名 → 活跃 KV 驱动(长连接;驱动内部再按库号持有会话)
  final _drivers = <String, KvStoreDriver>{};

  /// 连接名 → 命名空间列表加载状态
  final _nsStates = <String, KvNamespaceListState>{};

  KvNamespaceListState namespacesOf(String connection) =>
      _nsStates[connection] ??= KvNamespaceListState();

  bool isConnected(String connection) =>
      _drivers[connection]?.isConnected ?? false;

  /// 取(或建立)连接的活跃驱动。驱动内部按库号惰性开会话,
  /// 因此这里返回成功不代表已经握过手 —— 第一条命令才是。
  Future<KvStoreDriver> driverFor(ConnectionInfo conn) async {
    final driver = _drivers[conn.name];
    if (driver != null && driver.isConnected) return driver;
    await driver?.close();
    final fresh = createKvDriver(conn);
    if (fresh == null) {
      throw UnsupportedError('暂不支持 ${conn.typeId} 类型的连接');
    }
    await fresh.connect();
    _drivers[conn.name] = fresh;
    return fresh;
  }

  /// 测试注入:预先挂一个「已连接」的驱动,使 [driverFor] 直接复用
  @visibleForTesting
  void attachDriverForTest(String connection, KvStoreDriver driver) {
    _drivers[connection] = driver;
  }

  /// 连接向导「测试连接」:连上即断,不保留长连接
  Future<(bool, String)> testConnection(ConnectionInfo conn) async {
    final driver = createKvDriver(conn);
    if (driver == null) {
      return (false, '暂不支持 ${conn.typeId} 类型的连接');
    }
    try {
      await driver.connect();
      final ns = await driver.listNamespaces();
      return (true, '连接成功(${ns.length} 个库)');
    } on KvServerException catch (e) {
      return (false, '服务端拒绝: ${e.message}');
    } on Object catch (e) {
      return (false, '连接失败: $e');
    } finally {
      await driver.close();
    }
  }

  /// 展开连接节点:确保连接存活并拉取命名空间列表。
  /// 已加载 / 加载中直接返回(与 SQL 侧 expandConnection 同一语义)。
  Future<void> expandConnection(ConnectionInfo conn) async {
    final state = namespacesOf(conn.name);
    if (state.status == LoadStatus.loading ||
        state.status == LoadStatus.loaded) {
      return;
    }
    await _loadNamespaces(conn, state);
  }

  /// 右键「打开连接」:即使已缓存也强制重新通信
  Future<(bool, String)> forceExpandConnection(ConnectionInfo conn) async {
    final state = namespacesOf(conn.name);
    state.status = LoadStatus.idle;
    await _loadNamespaces(conn, state);
    if (state.status == LoadStatus.error) {
      return (false, state.error ?? '未知错误');
    }
    return (true, '连接成功');
  }

  Future<void> retryExpandConnection(ConnectionInfo conn) async {
    namespacesOf(conn.name).status = LoadStatus.idle;
    await expandConnection(conn);
  }

  Future<void> _loadNamespaces(
      ConnectionInfo conn, KvNamespaceListState state) async {
    state.status = LoadStatus.loading;
    state.error = null;
    notifyListeners();
    try {
      final driver = await driverFor(conn);
      state.namespaces = await driver.listNamespaces();
      state.status = LoadStatus.loaded;
    } on KvServerException catch (e) {
      // 服务端错误:连接仍可用,只把这一页标成失败
      state.error = e.message;
      state.status = LoadStatus.error;
    } on Object catch (e) {
      state.error = '$e';
      state.status = LoadStatus.error;
      // 连接状态不可信(见 KvConnectionException 的说明):作废,下次调用自然重连
      await _dropDriver(conn.name);
    }
    notifyListeners();
  }

  /// 刷新命名空间列表(重开库 / 服务端改了 databases 数)
  Future<void> refreshNamespaces(ConnectionInfo conn) async {
    namespacesOf(conn.name).status = LoadStatus.idle;
    await expandConnection(conn);
  }

  /// 断开一条连接:驱动内部会关掉它持有的全部库会话
  Future<void> disconnect(String connection) async {
    final driver = _drivers.remove(connection);
    _nsStates.remove(connection);
    await driver?.close();
    notifyListeners();
  }

  Future<void> _dropDriver(String connection) async {
    final driver = _drivers.remove(connection);
    await driver?.close();
  }

  @override
  void dispose() {
    for (final driver in _drivers.values) {
      driver.close();
    }
    _drivers.clear();
    super.dispose();
  }
}
