import 'package:daro/app/app_state.dart';
import 'package:flutter_test/flutter_test.dart';

/// 标签身份回归:同名表在不同库 / 模式下各自成标签,切换、关闭、重命名
/// 必须按业务唯一键([OpenTab.key])而非标题定位。
///
/// 历史 bug:活动标签身份存的是标题(表名),两个库里的同名 `organizations`
/// 标签标题相同 → [activeTabModel] 永远命中第一个,切换第二个时界面毫无变化、
/// 只显示第一个打开的数据。
void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  group('同名表标签身份(按 OpenTab.key)', () {
    test('不同库的同名表各自成标签,激活身份互不串台', () {
      final app = AppState();
      app.openTable('organizations', connection: 'c1', database: 'db1');
      app.openTable('organizations', connection: 'c1', database: 'db2');

      expect(app.tabs.length, 2);
      // 标题(表名)相同,但唯一键不同
      expect(app.tabs.first.title, app.tabs.last.title);
      expect(app.tabs.first.key, isNot(app.tabs.last.key));
      // 最后打开的即活动标签,且解析到的是第二个(db2)
      expect(app.activeTab, app.tabs.last.key);
      expect(app.activeTabModel?.database, 'db2');

      // 切回第一个:活动模型必须变成 db1(修复前会停在 db1 之后不动 / 或恒为第一个)
      app.activateTab(app.tabs.first.key);
      expect(app.activeTabModel?.database, 'db1');
      app.activateTab(app.tabs.last.key);
      expect(app.activeTabModel?.database, 'db2');
    });

    test('同库重复打开同一张表不产生第二个标签,仅激活', () {
      final app = AppState();
      app.openTable('organizations', connection: 'c1', database: 'db1');
      final key = app.tabs.single.key;
      app.openTable('organizations', connection: 'c1', database: 'db1');
      expect(app.tabs.length, 1);
      expect(app.activeTab, key);
    });

    test('closeTab / closeOtherTabs / closeTabsToRight 只作用于目标 key', () {
      final app = AppState();
      app.openTable('organizations', connection: 'c1', database: 'db1');
      app.openTable('organizations', connection: 'c1', database: 'db2');
      app.openTable('organizations', connection: 'c1', database: 'db3');
      final firstKey = app.tabs.first.key;
      final secondKey = app.tabs[1].key;
      final lastKey = app.tabs.last.key;

      // 关闭中间的标签:另外两个仍在
      app.closeTab(secondKey);
      expect(app.tabs.map((t) => t.key), [firstKey, lastKey]);

      // 关闭活动标签时回到"对象"页
      app.activateTab(lastKey);
      app.closeTab(lastKey);
      expect(app.activeTab, AppState.objectsTabKey);
      expect(app.tabs.length, 1);

      // 关闭其它:只保留指定 key
      app.openTable('organizations', connection: 'c1', database: 'db2');
      app.closeOtherTabs(app.tabs.last.key);
      expect(app.tabs.length, 1);
      expect(app.activeTab, app.tabs.single.key);

      // 关闭右侧:按 key 定位边界
      app.openTable('organizations', connection: 'c1', database: 'db3');
      app.openTable('organizations', connection: 'c1', database: 'db4');
      final keepKey = app.tabs.first.key;
      app.closeTabsToRight(keepKey);
      expect(app.tabs.map((t) => t.key), [keepKey]);
    });

    test('renameTab 按 key 精确改名并迁移活动身份', () {
      final app = AppState();
      app.openTable('organizations', connection: 'c1', database: 'db1');
      app.openTable('organizations', connection: 'c1', database: 'db2');
      // 活动标签是第二个,把它"改名"(模拟设计器保存后表名变化)
      final activeKey = app.tabs.last.key;
      app.renameTab(activeKey, 'organizations_renamed');
      expect(app.tabs.length, 2);
      expect(app.tabs.last.title, 'organizations_renamed');
      // 活动身份迁移到新 key,仍指向第二个标签
      expect(app.activeTab, app.tabs.last.key);
      expect(app.activeTabModel?.title, 'organizations_renamed');
    });
  });
}
