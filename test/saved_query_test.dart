import 'package:daro/app/app_state.dart';
import 'package:daro/data/saved_query_store.dart';
import 'package:flutter_test/flutter_test.dart';

/// 已保存查询(SavedQuery 模型 + AppState 查询管理)单元测试。
/// 均为同步逻辑断言:AppState 构造触发的异步加载(load 全部 catch,
/// 不会产生未处理异常)在测试结束前不会执行到清空/覆盖断言数据。
void main() {
  group('SavedQuery 模型', () {
    test('json 序列化往返一致', () {
      const query = SavedQuery(
        name: '日报',
        connection: 'c1',
        database: 'db1',
        sql: 'SELECT 1',
      );
      final restored = SavedQuery.fromJson(query.toJson());
      expect(restored.name, '日报');
      expect(restored.connection, 'c1');
      expect(restored.database, 'db1');
      expect(restored.sql, 'SELECT 1');
      expect(restored.key, query.key);
    });

    test('fromJson 缺字段时回退默认值; key 按连接|库|名称拼接', () {
      final q = SavedQuery.fromJson({});
      expect(q.name, '');
      expect(q.connection, isNull);
      expect(q.database, isNull);
      expect(q.sql, '');
      expect(q.key, 'null|null|');
      expect(const SavedQuery(name: 'r', connection: 'c1', database: 'db1', sql: '').key,
          'c1|db1|r');
    });
  });

  group('AppState 查询管理', () {
    test('保存/覆盖/按上下文过滤/删除', () {
      final app = AppState();
      app.saveQuery(name: 'a', sql: 'SELECT a', connection: 'c1', database: 'db1');
      app.saveQuery(name: 'b', sql: 'SELECT b', connection: 'c1', database: 'db2');
      app.saveQuery(name: 'c', sql: 'SELECT c', connection: null, database: null);

      expect(app.savedQueriesOf('c1', 'db1').map((q) => q.name), ['a']);
      expect(app.savedQueriesOf('c1', 'db2').map((q) => q.name), ['b']);
      expect(app.savedQueriesOf(null, null).map((q) => q.name), ['c']);
      expect(app.savedQueriesOf('c1', 'db3'), isEmpty);

      // 同 连接|库|名称 覆盖,不同归属互不影响
      app.saveQuery(name: 'a', sql: 'SELECT a2', connection: 'c1', database: 'db1');
      expect(app.savedQueriesOf('c1', 'db1').single.sql, 'SELECT a2');
      expect(app.savedQueryOf('a', connection: 'c1', database: 'db1')!.sql, 'SELECT a2');
      expect(app.savedQueryOf('a', connection: 'c1', database: 'db2'), isNull);

      app.deleteSavedQuery('a', connection: 'c1', database: 'db1');
      expect(app.savedQueriesOf('c1', 'db1'), isEmpty);
      expect(app.savedQueryOf('a', connection: 'c1', database: 'db1'), isNull);
    });

    test('openSavedQuery 新建 tab 并预填 SQL;再次打开仅激活不覆盖编辑内容', () {
      final app = AppState();
      const query = SavedQuery(
        name: '报表',
        connection: 'c1',
        database: 'db1',
        sql: 'SELECT 1',
      );

      app.openSavedQuery(query);
      expect(app.tabs.single.type, TabType.query);
      expect(app.tabs.single.title, '报表');
      expect(app.tabs.single.connection, 'c1');
      expect(app.tabs.single.database, 'db1');
      expect(app.activeTab, app.tabs.single.key);
      final key = AppState.queryTabKey('c1', 'db1', '报表');
      expect(app.queryTextFor(key), 'SELECT 1');

      // 打开后编辑文本,再次打开同名查询:不重建 tab,不覆盖已编辑内容
      app.updateQueryText(key, 'SELECT 2');
      app.openSavedQuery(query);
      expect(app.tabs.length, 1);
      expect(app.queryTextFor(key), 'SELECT 2');
    });

    test('saveQuery 命名保存后重命名无标题 tab 并迁移编辑文本', () {
      final app = AppState();
      app.newQuery(); // '无标题-查询 1',无浏览上下文 → connection/database 为 null
      final tab = app.tabs.single;
      expect(tab.connection, isNull);
      expect(tab.database, isNull);

      final oldKey = AppState.queryTabKey(null, null, '无标题-查询 1');
      app.updateQueryText(oldKey, 'SELECT x');
      app.recordSql(oldKey, 'SELECT x');

      app.saveQuery(
        name: '我的查询',
        sql: 'SELECT x',
        connection: null,
        database: null,
        tabTitle: '无标题-查询 1',
      );

      expect(app.tabs.single.title, '我的查询');
      expect(app.activeTab, app.tabs.single.key);
      final newKey = AppState.queryTabKey(null, null, '我的查询');
      expect(app.queryTextFor(newKey), 'SELECT x');
      expect(app.queryTextFor(oldKey), '');
      expect(app.sqlHistoryFor(newKey), ['SELECT x']);
      expect(app.savedQueryOf('我的查询')!.sql, 'SELECT x');
    });

    test('closeTab 按 queryTabKey 清理查询文本(修复 OpenTab.key 格式不一致)', () {
      final app = AppState();
      const query = SavedQuery(
        name: '报表',
        connection: 'c1',
        database: 'db1',
        sql: 'SELECT 1',
      );
      app.openSavedQuery(query);
      final key = AppState.queryTabKey('c1', 'db1', '报表');
      app.updateQueryText(key, 'SELECT 9');
      app.recordSql(key, 'SELECT 9');

      app.closeTab(app.tabs.single.key);
      expect(app.tabs, isEmpty);
      expect(app.activeTab, AppState.objectsTabKey);
      expect(app.queryTextFor(key), '');
      expect(app.sqlHistoryFor(key), isEmpty);
    });
  });
}
