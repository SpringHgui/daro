import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';

import 'package:daro/app/app_state.dart';
import 'package:daro/data/routine_sql.dart';
import 'package:daro/l10n/locale_config.dart';
import 'package:daro/widgets/function_wizard_dialog.dart';

// 函数向导三步(例程类型 → 参数网格 → 选项)的行为回归,按参考工具(Navicat)
// 的向导形状核对:切步不销毁已填内容、底部四键的可用态随步骤与名称变化、
// 网格「预置一行空参数」、方言用不上的列不给编辑入口、函数才有「返回类型」
// 一行、第 3 步 PG 的五个选项(窗口 / 特性 / 防漏 / STRICT / 安全性)真的
// 进到结果里(设计页据此生成子句,不是界面上的摆设),非 PG 只留安全性。
//
// 生成 / 解析 SQL 本身由 test/routine_sql_test.dart 覆盖。

/// 把对话框真走一遍 `showDialog`,并接住 pop 回来的结果
class _Opener extends StatefulWidget {
  const _Opener({
    required this.typeId,
    required this.initialCategory,
    required this.onResult,
  });

  final String typeId;
  final ObjectCategory initialCategory;
  final void Function(RoutineWizardResult?) onResult;

  @override
  State<_Opener> createState() => _OpenerState();
}

class _OpenerState extends State<_Opener> {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      showDialog<RoutineWizardResult>(
        context: context,
        builder: (_) => FunctionWizardDialog(
          connection: '本地',
          database: 'demo',
          typeId: widget.typeId,
          schema: RoutineSql.defaultSchema(widget.typeId),
          initialCategory: widget.initialCategory,
        ),
      ).then(widget.onResult);
    });
  }

  @override
  Widget build(BuildContext context) => const SizedBox.shrink();
}

RoutineWizardResult? _result;

Future<void> _open(
  WidgetTester tester, {
  String typeId = 'postgresql',
  ObjectCategory initial = ObjectCategory.function,
}) async {
  // 向导是 900×560,默认 800×600 的测试视口会溢出
  tester.view.physicalSize = const Size(1600, 1200);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  _result = null;
  await tester.pumpWidget(MaterialApp(
    locale: const Locale('zh'),
    localizationsDelegates: kAppLocalizationsDelegates,
    supportedLocales: kSupportedLocales,
    home: ChangeNotifierProvider<AppState>.value(
      value: AppState(),
      child: _Opener(
        typeId: typeId,
        initialCategory: initial,
        onResult: (r) => _result = r,
      ),
    ),
  ));
  await tester.pumpAndSettle();
}

/// 按文案读取底部按钮的可用态(禁用 = onPressed 为 null)
bool _enabled(WidgetTester tester, String text) {
  final button = tester
      .element(find.text(text))
      .findAncestorWidgetOfExactType<Button>();
  expect(button, isNotNull, reason: '$text 未包在 base-ui Button 中');
  return button!.onPressed != null;
}

Future<void> _next(WidgetTester tester) async {
  await tester.tap(find.text('下一步 >'));
  await tester.pumpAndSettle();
}

Future<void> _back(WidgetTester tester) async {
  await tester.tap(find.text('< 上一步'));
  await tester.pumpAndSettle();
}

Future<void> _finish(WidgetTester tester) async {
  await tester.tap(find.text('完成'));
  await tester.pumpAndSettle();
}

Future<void> _typeName(WidgetTester tester, String name) async {
  await tester.enterText(find.byType(Input).first, name);
  await tester.pump();
}

/// 网格键盘导航:焦点在进第 2 步时已交给网格,方向键移格、Enter 开编辑。
/// 编辑器是 setState 之后那一帧才挂上的,并且开编辑还会 post-frame 抢焦点,
/// 所以一帧不够,固定泵两帧。
Future<void> _moveCell(WidgetTester tester, LogicalKeyboardKey key) async {
  await tester.sendKeyEvent(key);
  await tester.pump();
  await tester.pump();
}

/// 只读下拉(类型模式 / 模式)是 ComboBox<String>,`find.byType(ComboBox)`
/// 拿到的是原始类型 ComboBox<Object>,永远匹配不上,必须按谓词找
Finder _comboCells = find.byWidgetPredicate((w) => w is ComboBox<String>);

/// 第 2 步底部还有「返回类型」的两个下拉,判「网格里有没有编辑器」必须
/// 把范围限定在 DataGridView 内部,否则永远判成开着。
final _grid = find.byType(DataGridView);
Finder _gridCombos =
    find.descendant(of: find.byType(DataGridView), matching: _comboCells);
Finder _gridInputs =
    find.descendant(of: find.byType(DataGridView), matching: find.byType(Input));
Finder _gridEditableTexts = find
    .descendant(of: find.byType(DataGridView), matching: find.byType(EditableText));

/// 网格里是否挂着编辑器:可打字的一类(Input / 可编辑下拉)含 EditableText,
/// 只读下拉不含
bool _editorOpen(WidgetTester tester, {bool typingOnly = false}) {
  final editable = _gridEditableTexts.evaluate().isNotEmpty;
  if (typingOnly) return editable;
  return editable || _gridCombos.evaluate().isNotEmpty;
}

void main() {
  tearDown(() => FunctionWizardDialog.showOnNextCreate = true);

  group('三步骨架', () {
    testWidgets('标题逐步更换,底部按钮可用态跟着步骤走', (tester) async {
      await _open(tester);
      expect(find.text('请选择你要创建的例程类型'), findsOneWidget);
      expect(_enabled(tester, '< 上一步'), isFalse, reason: '首步没有上一步');
      expect(_enabled(tester, '下一步 >'), isTrue);
      expect(_enabled(tester, '完成'), isFalse, reason: '名称还没填');

      await _typeName(tester, 'hanshu');
      expect(_enabled(tester, '完成'), isTrue);

      await _next(tester);
      expect(find.text('请输入这个例程的参数'), findsOneWidget);
      expect(_enabled(tester, '< 上一步'), isTrue);
      expect(_enabled(tester, '下一步 >'), isTrue);

      await _next(tester);
      expect(find.text('请选择函数会有的选项'), findsOneWidget);
      expect(_enabled(tester, '下一步 >'), isFalse, reason: '末步没有下一步');

      // 往回走不能把已经填的东西丢掉
      await _back(tester);
      expect(find.text('请输入这个例程的参数'), findsOneWidget);
      await _back(tester);
      expect(find.text('请选择你要创建的例程类型'), findsOneWidget);
      expect(find.text('hanshu'), findsWidgets);
    });

    testWidgets('取消直接关闭,不产出结果', (tester) async {
      await _open(tester);
      await _typeName(tester, 'x');
      await tester.tap(find.text('取消'));
      await tester.pumpAndSettle();
      expect(_result, isNull);
    });

    testWidgets('入口分类被沿用,但用户可改', (tester) async {
      await _open(tester, initial: ObjectCategory.procedure);
      await _typeName(tester, 'p');
      await tester.tap(find.text('函数'));
      await tester.pumpAndSettle();
      await _finish(tester);
      expect(_result!.category, ObjectCategory.function);
    });

    testWidgets('「下次显示向导」勾选态写回静态开关', (tester) async {
      await _open(tester);
      expect(FunctionWizardDialog.showOnNextCreate, isTrue);
      await _typeName(tester, 'x');
      await tester.tap(find.text('下次显示向导'));
      await tester.pumpAndSettle();
      await _finish(tester);
      expect(FunctionWizardDialog.showOnNextCreate, isFalse);
    });
  });

  group('第 2 步参数网格', () {
    testWidgets('首次进入预置一行空参数,列头按参考工具五列', (tester) async {
      await _open(tester);
      await _typeName(tester, 'f');
      await _next(tester);
      for (final h in ['名称', '类型模式', '类型', '模式', '默认值']) {
        expect(find.text(h), findsOneWidget, reason: '表头缺列 $h');
      }
      expect(find.byType(DataGridView), findsOneWidget);
    });

    testWidgets('单元格编辑写回结果,签名按方言生成', (tester) async {
      await _open(tester);
      await _typeName(tester, 'hanshu');
      await _next(tester);

      await _moveCell(tester, LogicalKeyboardKey.enter);
      await tester.enterText(_gridInputs.last, 'a');
      await tester.pump();

      // 名称 → 类型模式 → 类型:PG 两列都可用
      await _moveCell(tester, LogicalKeyboardKey.arrowRight);
      await _moveCell(tester, LogicalKeyboardKey.enter);
      expect(_comboCells.evaluate().isNotEmpty, isTrue,
          reason: 'PG 的类型模式列应给出只读下拉');
      await _moveCell(tester, LogicalKeyboardKey.arrowRight);
      await _moveCell(tester, LogicalKeyboardKey.enter);
      expect(_editorOpen(tester, typingOnly: true), isTrue,
          reason: '类型是可编辑下拉,应给出可打字的编辑器');

      await _finish(tester);
      final params = _result!.params;
      expect(params, hasLength(1));
      expect(params.first.name, 'a');
      expect(params.first.mode, 'IN');
      expect(RoutineSql.signature('postgresql', params), 'IN "a" INT');
    });

    testWidgets('MySQL 的类型模式与默认值列不给编辑入口', (tester) async {
      await _open(tester, typeId: 'mysql');
      await _typeName(tester, 'f');
      await _next(tester);

      // (0,0) 名称可编辑
      await _moveCell(tester, LogicalKeyboardKey.enter);
      expect(_editorOpen(tester, typingOnly: true), isTrue);

      // 右移到「类型模式」(col 1):Enter 不该开编辑器
      await _moveCell(tester, LogicalKeyboardKey.arrowRight);
      expect(_editorOpen(tester), isFalse);
      await _moveCell(tester, LogicalKeyboardKey.enter);
      expect(_editorOpen(tester), isFalse, reason: 'MySQL 没有类型命名空间');

      // 走到「默认值」(col 4):同样拦掉
      for (var i = 0; i < 3; i++) {
        await _moveCell(tester, LogicalKeyboardKey.arrowRight);
      }
      await _moveCell(tester, LogicalKeyboardKey.enter);
      expect(_editorOpen(tester), isFalse, reason: 'MySQL 参数不支持默认值');

      // 但「模式」(col 3)是 MySQL 用得上的
      await _moveCell(tester, LogicalKeyboardKey.arrowLeft);
      await _moveCell(tester, LogicalKeyboardKey.enter);
      expect(_editorOpen(tester), isTrue);
    });

    testWidgets('⊕ 在当前行下方插入一行', (tester) async {
      await _open(tester);
      await _typeName(tester, 'f');
      await _next(tester);
      await tester.tap(find.byIcon(Icons.add));
      await tester.pumpAndSettle();
      await _finish(tester);
      expect(_result!.params, hasLength(2), reason: '初始预置一行 + ⊕ 一行');
    });

    testWidgets('参数行再多也只撑到可用高度,不会把底部按钮顶出窗口',
        (tester) async {
      await _open(tester);
      await _typeName(tester, 'f');
      await _next(tester);
      for (var i = 0; i < 14; i++) {
        await tester.tap(find.byIcon(Icons.add));
        await tester.pump();
      }
      await tester.pumpAndSettle();
      // 溢出会当场记成框架错误让本用例失败;这里再确认底部两行没压到页脚
      final ret = tester.getRect(find.text('返回类型:'));
      final footer = tester.getRect(find.text('完成'));
      expect(ret.bottom, lessThanOrEqualTo(footer.top));
      await tester.tap(find.text('取消'));
      await tester.pumpAndSettle();
    });

    testWidgets('⊕ 之后 ⊖ 删掉的是刚插入的那一行', (tester) async {
      await _open(tester);
      await _typeName(tester, 'f');
      await _next(tester);
      await tester.tap(find.byIcon(Icons.add));
      await tester.pumpAndSettle();
      await tester.tap(find.byIcon(Icons.remove));
      await tester.pumpAndSettle();
      await _finish(tester);
      expect(_result!.params, hasLength(1));
    });

    testWidgets('双击开编辑后,再点同一格不会把编辑器拆掉', (tester) async {
      await _open(tester);
      await _typeName(tester, 'f');
      await _next(tester);

      final grid = tester.getRect(_grid);
      final nameCell = Offset(grid.left + 40, grid.top + 30);
      // 第一次按下选中格子,第二次按下落在双击判定窗口内 → 开编辑
      await tester.tapAt(nameCell);
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 50));
      await tester.tapAt(nameCell);
      await tester.pump();
      await tester.pump();
      expect(_editorOpen(tester, typingOnly: true), isTrue,
          reason: '双击参数格应进入编辑');

      // 编辑器里再点一下(定位光标):选择回调会被格子的外层 Listener 收到,
      // 若据此关掉编辑器,下拉就打不开、输入框点一下就丢编辑态
      await tester.tapAt(nameCell);
      await tester.pump();
      await tester.pump();
      expect(_editorOpen(tester, typingOnly: true), isTrue,
          reason: '点编辑器内部不能退出编辑');

      // 点别的列才退出
      await tester.tapAt(Offset(grid.left + 200, grid.top + 30));
      await tester.pump();
      await tester.pump();
      expect(_editorOpen(tester), isFalse);
      // 双击手势的判定计时器还挂着,不排掉测试会报「Timer is still pending」
      await tester.pump(const Duration(milliseconds: 500));
    });
  });

  group('第 2 步返回类型', () {
    testWidgets('函数出现「返回类型:」一行,过程不出现', (tester) async {
      await _open(tester);
      await _typeName(tester, 'f');
      await _next(tester);
      expect(find.text('返回类型:'), findsOneWidget,
          reason: 'CREATE FUNCTION 必须有 RETURNS');
    });

    testWidgets('过程没有 RETURNS 子句,这一行就不给', (tester) async {
      await _open(tester, initial: ObjectCategory.procedure);
      await _typeName(tester, 'p');
      await _next(tester);
      expect(find.text('返回类型:'), findsNothing);
    });

    testWidgets('MySQL 的返回类型只有一个下拉(没有类型命名空间)', (tester) async {
      await _open(tester, typeId: 'mysql');
      await _typeName(tester, 'f');
      await _next(tester);
      expect(find.text('返回类型:'), findsOneWidget);
      expect(_comboCells, findsOneWidget);
    });

    testWidgets('参考图逐字符:pg_catalog + char 拼成 RETURNS pg_catalog."char"',
        (tester) async {
      await _open(tester);
      await _typeName(tester, '函数');
      await _next(tester);

      // 参数:aaa / pg_catalog / bytea
      await _moveCell(tester, LogicalKeyboardKey.enter);
      await tester.enterText(_gridEditableTexts.last, 'aaa');
      await tester.pump();
      await _moveCell(tester, LogicalKeyboardKey.arrowRight);
      await _moveCell(tester, LogicalKeyboardKey.enter);
      await tester.tap(_gridCombos.last);
      await tester.pumpAndSettle();
      await tester.tap(find.text('pg_catalog').last);
      await tester.pumpAndSettle();
      await _moveCell(tester, LogicalKeyboardKey.arrowRight);
      await _moveCell(tester, LogicalKeyboardKey.enter);
      await tester.tap(_gridCombos.last);
      await tester.pumpAndSettle();
      await tester.tap(find.text('bytea').last);
      await tester.pumpAndSettle();
      // 收起网格编辑器,否则下面找返回类型的下拉会先撞上它还挂着的那一个
      await _moveCell(tester, LogicalKeyboardKey.escape);

      // 返回类型行是此刻仅有的两个下拉
      await tester.tap(_comboCells.first);
      await tester.pumpAndSettle();
      await tester.tap(find.text('pg_catalog').last);
      await tester.pumpAndSettle();
      await tester.enterText(
        find.descendant(
            of: _comboCells.last, matching: find.byType(EditableText)),
        'char',
      );
      await tester.pump();

      await _next(tester);
      await _finish(tester);
      expect(_result!.returnType, 'pg_catalog."char"');
      expect(RoutineSql.signature('postgresql', _result!.params),
          'IN "aaa" pg_catalog.bytea');

      final sql = RoutineSql.buildTemplate(
        typeId: 'postgresql',
        category: _result!.category,
        name: _result!.name,
        schema: 'public',
        signature: RoutineSql.signature('postgresql', _result!.params),
        returnType: _result!.returnType,
      );
      // 与参考工具同形:RETURNS 的类型带引号,占位 RETURN 的字面量跟着类型走
      // (char → `' '`)。差异只剩两处有意的:参数名一律加引号、末行恒写
      // SECURITY(PG 默认 INVOKER,写出来才能让向导第 3 步的选择可见)。
      expect(sql, '''
CREATE FUNCTION "public"."函数" (IN "aaa" pg_catalog.bytea)
RETURNS pg_catalog."char"
AS \$BODY\$BEGIN
  -- Routine body goes here...

  RETURN ' ';
END
\$BODY\$
  LANGUAGE 'plpgsql'
  SECURITY INVOKER;
''');
    });

    testWidgets('带长度修饰的返回类型不整包加引号(varchar(100) 会成语法错)',
        (tester) async {
      expect(RoutineSql.qualifiedType('postgresql',
          schema: 'pg_catalog', type: 'varchar(100)'),
          'pg_catalog.varchar(100)');
      expect(RoutineSql.qualifiedType('postgresql',
          schema: 'pg_catalog', type: 'integer[]'),
          'pg_catalog.integer[]');
      expect(
          RoutineSql.qualifiedType('postgresql', type: 'integer'), 'integer');
      expect(RoutineSql.qualifiedType('mysql',
          schema: 'pg_catalog', type: 'int'), 'int');
      expect(RoutineSql.qualifiedType('sqlserver',
          schema: 'dbo', type: 'MyType'), '[dbo].[MyType]');
    });
  });

  group('参考图逐字符', () {
    testWidgets('过程 hanshu / 参数 a = pg_catalog.bool / INVOKER 生成参考工具的 DDL',
        (tester) async {
      await _open(tester, initial: ObjectCategory.procedure);
      await _typeName(tester, 'hanshu');
      await _next(tester);

      // 名称
      await _moveCell(tester, LogicalKeyboardKey.enter);
      await tester.enterText(_gridEditableTexts.last, 'a');
      await tester.pump();

      // 类型模式 = pg_catalog(只读下拉)
      await _moveCell(tester, LogicalKeyboardKey.arrowRight);
      await _moveCell(tester, LogicalKeyboardKey.enter);
      await tester.tap(_gridCombos.last);
      await tester.pumpAndSettle();
      await tester.tap(find.text('pg_catalog').last);
      await tester.pumpAndSettle();

      // 类型 = bool(可编辑下拉,手打)
      await _moveCell(tester, LogicalKeyboardKey.arrowRight);
      await _moveCell(tester, LogicalKeyboardKey.enter);
      await tester.enterText(_gridEditableTexts.last, 'bool');
      await tester.pump();

      // 模式沿用默认的 IN,第 3 步保持 INVOKER
      await _next(tester);
      await _finish(tester);

      final params = _result!.params;
      expect(RoutineSql.signature('postgresql', params), 'IN "a" pg_catalog.bool');
      expect(_result!.category, ObjectCategory.procedure);
      expect(_result!.options.securityDefiner, isFalse);

      final sql = RoutineSql.buildTemplate(
        typeId: 'postgresql',
        category: _result!.category,
        name: _result!.name,
        schema: 'public',
        signature: RoutineSql.signature('postgresql', params),
        language: RoutineSql.defaultLanguage('postgresql'),
        options: _result!.options,
      );
      expect(sql, '''
CREATE PROCEDURE "public"."hanshu" (IN "a" pg_catalog.bool)
AS \$BODY\$BEGIN
  -- Routine body goes here...

END
\$BODY\$
  LANGUAGE 'plpgsql'
  SECURITY INVOKER;
''');
    });
  });

  group('第 3 步选项', () {
    testWidgets('安全性选 DEFINER 会写进结果与模板', (tester) async {
      await _open(tester);
      await _typeName(tester, 'f');
      await _next(tester);
      await _next(tester);
      expect(find.text('INVOKER'), findsOneWidget, reason: '默认 INVOKER');

      await tester.tap(_comboCells.last);
      await tester.pumpAndSettle();
      await tester.tap(find.text('DEFINER').last);
      await tester.pumpAndSettle();

      await _finish(tester);
      expect(_result!.options.securityDefiner, isTrue);

      final sql = RoutineSql.buildTemplate(
        typeId: 'postgresql',
        category: _result!.category,
        name: _result!.name,
        schema: 'public',
        signature: RoutineSql.signature('postgresql', _result!.params),
        options: _result!.options,
      );
      expect(sql, contains('SECURITY DEFINER'));
    });

    testWidgets('PG 五个选项逐行出现,勾选与特性都写进 DDL', (tester) async {
      await _open(tester);
      await _typeName(tester, 'f');
      await _next(tester);
      await _next(tester);
      for (final s in ['窗口', '特性:', '防漏', '输入 NULL 时返回 NULL', '安全性:']) {
        expect(find.text(s), findsOneWidget, reason: '参考工具第 3 步缺行 $s');
      }

      await tester.tap(find.text('窗口'));
      await tester.pump();
      await tester.tap(find.text('防漏'));
      await tester.pump();
      await tester.tap(find.text('输入 NULL 时返回 NULL'));
      await tester.pump();
      // 特性 = IMMUTABLE(第 3 步的两个下拉里,特性在前)
      await tester.tap(_comboCells.first);
      await tester.pumpAndSettle();
      await tester.tap(find.text('IMMUTABLE').last);
      await tester.pumpAndSettle();

      await _finish(tester);
      expect(
        _result!.options,
        const RoutineOptions(
          window: true,
          volatility: 'IMMUTABLE',
          leakProof: true,
          strict: true,
        ),
      );

      final sql = RoutineSql.buildTemplate(
        typeId: 'postgresql',
        category: _result!.category,
        name: _result!.name,
        schema: 'public',
        options: _result!.options,
      );
      expect(sql, contains('  WINDOW\n  IMMUTABLE\n  LEAKPROOF\n  STRICT\n'));
    });

    testWidgets('非 PG 方言只剩安全性一行(四个子句它们不认)', (tester) async {
      await _open(tester, typeId: 'mysql');
      await _typeName(tester, 'f');
      await _next(tester);
      await _next(tester);
      expect(find.text('安全性:'), findsOneWidget);
      for (final s in ['窗口', '特性:', '防漏', '输入 NULL 时返回 NULL']) {
        expect(find.text(s), findsNothing, reason: '$s 是 PG 专有子句');
      }
    });
  });
}
