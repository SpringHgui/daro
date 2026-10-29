import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

import 'package:daro/app/app_state.dart';
import 'package:daro/data/db_data.dart';
import 'package:daro/theme/app_theme.dart';
import 'package:daro/widgets/create_database_dialog.dart';
import 'package:daro/l10n/locale_config.dart';

/// 渲染守护:新建数据库对话框的标签页结构 (常规 / 扩展 / 注释 / SQL 预览)、
/// 各类型的字段集合、以及"切页 + 输入后 SQL 预览跟随"这条链路。
///
/// 只 pump 到 `ConnectionManager.isConnected == false` 的分支,所以
/// [AppState.loadCreateDatabaseCatalog] 走内置兜底值,不碰真实连接。

ConnectionInfo _conn(String typeId, {String user = 'postgres'}) => ConnectionInfo(
      name: 'local',
      typeId: typeId,
      host: '127.0.0.1',
      port: typeId == 'postgresql' ? '5432' : '3306',
      username: user,
    );

Future<void> _open(WidgetTester tester, ConnectionInfo conn) async {
  tester.view.physicalSize = const Size(1600, 1000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  await tester.pumpWidget(ChangeNotifierProvider<AppState>(
    create: (_) => AppState(),
    child: MaterialApp(
      locale: const Locale('zh'),
      localizationsDelegates: kAppLocalizationsDelegates,
      supportedLocales: kSupportedLocales,
      theme: ThemeData(brightness: Brightness.light),
      home: TokenScope(
        tokens: AppTheme.light.toDesktopTokens(),
        child: Scaffold(
          body: Builder(
            builder: (context) => Center(
              child: GestureDetector(
                onTap: () => showDialog<void>(
                  context: context,
                  builder: (_) => CreateDatabaseDialog(connection: conn),
                ),
                child: const Text('open'),
              ),
            ),
          ),
        ),
      ),
    ),
  ));
  await tester.tap(find.text('open'));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('PostgreSQL:四个标签页 + 常规页字段齐全,无布局异常', (tester) async {
    await _open(tester, _conn('postgresql'));

    // 标签页
    for (final label in ['常规', '扩展', '注释', 'SQL 预览']) {
      expect(find.text(label), findsOneWidget, reason: '缺少标签页 $label');
    }

    // 常规页:与 Navicat 参考窗口同序的字段
    for (final label in [
      '数据库名称:',
      '所有者:',
      '模板:',
      '编码:',
      '排序规则:',
      '字符分类:',
      '表空间:',
      '连接限制:',
    ]) {
      expect(find.text(label), findsOneWidget, reason: '缺少字段 $label');
    }
    expect(find.text('允许连接'), findsOneWidget);
    expect(find.text('是否模板'), findsOneWidget);

    // 弹窗宽度按 daro 尺寸,不是 Navicat 的 1060
    final size = (tester.renderObject(find.descendant(
      of: find.byType(DialogBox),
      matching: find.byType(IntrinsicWidth),
    ).first) as RenderBox)
        .size;
    expect(size.width, 700);

    expect(tester.takeException(), isNull, reason: '不应有溢出 / 断言异常');
  });

  testWidgets('PostgreSQL:名称与选项变化后 SQL 预览跟随更新', (tester) async {
    await _open(tester, _conn('postgresql'));

    await tester.tap(find.text('SQL 预览'));
    await tester.pumpAndSettle();
    // 名称为空时给出提示
    expect(find.textContaining('请先填写数据库名称'), findsOneWidget);

    await tester.tap(find.text('常规'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(Input).first, 'demo');
    await tester.pumpAndSettle();

    await tester.tap(find.text('SQL 预览'));
    await tester.pumpAndSettle();
    // 预览区现在是带语法高亮的富文本,finder 必须开 findRichText 才看得见。
    expect(find.textContaining('CREATE DATABASE "demo"', findRichText: true),
        findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('MySQL:只展示字符集与排序规则,不出现 PostgreSQL 专有字段',
      (tester) async {
    await _open(tester, _conn('mysql', user: 'root'));

    expect(find.text('字符集:'), findsOneWidget);
    expect(find.text('排序规则:'), findsOneWidget);
    expect(find.text('所有者:'), findsNothing);
    expect(find.text('表空间:'), findsNothing);
    expect(find.text('允许连接'), findsNothing);

    // 扩展 / 注释页对 MySQL 不可用
    await tester.tap(find.text('扩展'));
    await tester.pumpAndSettle();
    expect(find.textContaining('仅 PostgreSQL 支持扩展'), findsOneWidget);

    expect(tester.takeException(), isNull);
  });
}
