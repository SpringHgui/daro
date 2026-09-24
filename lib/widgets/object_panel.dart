import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../app/app_state.dart';
import '../app/connection_manager.dart';
import '../data/db_data.dart';
import '../data/drivers/db_driver.dart';
import '../data/routine_sql.dart';
import '../data/user_sql.dart';
import '../l10n/locale_config.dart';
import '../theme/app_theme.dart';
import 'data_export_wizard.dart';
import 'data_import_wizard.dart';
import 'function_wizard_dialog.dart';
import 'object_category_icon.dart';
import 'table_context_menu.dart';

// 中部对象面板:展示当前浏览数据库中的对象(表 / 视图 / 函数,由
// objectCategory 决定),单击选中、双击表 / 视图打开数据页。
// 数据来源:连接树单击库或分组节点 → AppState.objectContext →
// ConnectionManager 拉取的对象列表(懒加载)。未选择库时显示空态提示。
// 性能优化:
//   1. 垂直 ListView.builder + itemExtent 只构建可见行(懒加载)
//   2. 每行通过 Row 横向排列各列对应项,保持列优先顺序
//   3. Selector 精确重建每个表项,选中操作不触发全面板重建
//   4. GestureDetector 替代 InkWell 消除 Material 墨水动画开销
//   5. const TableIcon 共享同一 CustomPaint 实例,避免重复分配
class ObjectPanel extends StatefulWidget {
  const ObjectPanel({super.key});

  @override
  State<ObjectPanel> createState() => _ObjectPanelState();
}

class _ObjectPanelState extends State<ObjectPanel> {
  /// 订阅 ConnectionManager:表列表加载完成后重建面板
  ConnectionManager? _listenedManager;

  /// 新建视图的默认名自增序号(与 function_wizard 的 `_routineNameCounter` 同构,
  /// 不持久化:重启后从 view_1 重新计,重名由引擎端 CREATE 报错兜住)
  static int _viewNameCounter = 0;

  /// 新建角色的默认名自增序号(与 _viewNameCounter 同构,各自独立计数)
  static int _roleNameCounter = 0;

  /// 对象列表的键盘焦点(F2 改名 / Ctrl+C 复制 / Ctrl+V 粘贴)
  final FocusNode _listFocus = FocusNode();

  /// 列表模式纵向滚动控制器:框选要把选框矩形按当前滚动偏移换算成内容坐标
  final ScrollController _itemsScroll = ScrollController();

  /// 网格模式横向滚动控制器(网格纵向不滚动,列排满高度才向右开新列)
  final ScrollController _gridScrollX = ScrollController();

  /// 正在内联改名的表名(null = 无编辑中项)
  String? _renaming;

  /// 框选起手时是否按住 Ctrl(按下即定,拖动途中再按 Ctrl 不改变本次语义)
  bool _marqueeAdditive = false;

  /// 网格几何:列数 / 行数。由 [_buildGrid] 的 LayoutBuilder 每次 build 刷新,
  /// 供框选与改名单元格按 [_gridCellWidth] / [_itemHeight] 反算索引
  int _gridCols = 1;
  int _gridRows = 1;

  /// 工具栏搜索框输入文本(小写),用于过滤当前分类的对象列表
  String _objectSearchText = '';

  /// 上一次对象浏览上下文键,用于检测切换时重置搜索
  String _lastContextKey = '';

  void _onManagerChanged() {
    if (mounted) setState(() {});
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final manager = context.read<AppState>().connectionManager;
    if (_listenedManager != manager) {
      _listenedManager?.removeListener(_onManagerChanged);
      _listenedManager = manager;
      manager.addListener(_onManagerChanged);
    }
  }

  @override
  void dispose() {
    _listenedManager?.removeListener(_onManagerChanged);
    _listFocus.dispose();
    _itemsScroll.dispose();
    _gridScrollX.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    // 布局模式(网格/列表)变化时才重建
    final grid = context.select<AppState, bool>((a) => a.objectGridLayout);
    // 对象浏览上下文变化时重建(未选择库 → 空态)
    final (connection, database, schema) = context.select<AppState, (String?, String?, String?)>(
        (a) => (a.objectConnection, a.objectDatabase, a.objectSchema));
    // 浏览分类(表 / 视图 / 函数 / 查询 / 备份)变化时重建
    final category =
        context.select<AppState, ObjectCategory>((a) => a.objectCategory);

    if (connection == null || database == null) {
      // 未选择数据库:操作栏整体禁用(保留全部操作位但灰显不可点),
      // 内容区直接留白,不展示任何数据或空态提示
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _buildDisabledToolbar(t, category),
          Expanded(child: _blank(t)),
        ],
      );
    }

    // 切换连接 / 库 / 模式 / 分类时重置搜索文本(_ToolbarSearch 通过 Key 重建,
    // 其内部 controller / 展开态随之清空,与此处重置保持一致)
    final contextKey = '$connection|$database|$schema|$category';
    if (contextKey != _lastContextKey) {
      _lastContextKey = contextKey;
      _objectSearchText = '';
    }

    final app = context.read<AppState>();

    // 当前连接的数据库类型 id(用于按类型区分"新建表"按钮行为:
    // MySQL 系无下拉;PostgreSQL 系点击主体建常规表、箭头下拉 常规/外部/分区)
    String? typeId;
    for (final conn in app.connections) {
      if (conn.name == connection) {
        typeId = conn.typeId;
        break;
      }
    }

    final state = app.connectionManager
        .tableStateOf(connection, database, schema: schema);

    // 有模式层(PostgreSQL / SQL Server)时,库级(schema==null)不是有效的
    // 对象展示层——表归属于具体模式,需展开并选择模式后才展示数据
    if (typeId != null &&
        kSchemaLayerTypes.contains(typeId) &&
        schema == null) {
      // 有模式层但未选模式:内容区留白,不展示数据或提示
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _buildDisabledToolbar(t, category),
          Expanded(child: _blank(t)),
        ],
      );
    }

    // 上下文已设置但对象列表尚未加载(数据库 / 模式未打开):
    // 不展示任何数据,操作栏整体禁用——单击节点 ≠ 打开节点
    if (state.status == LoadStatus.idle) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _buildDisabledToolbar(t, category),
          Expanded(child: _blank(t)),
        ],
      );
    }

    // 顶部工具栏(含新增按钮) + 内容区:列表展示时工具栏固定一行
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // 工具栏随选中态变化重建:打开 / 设计按钮的可用性依赖当前选中表
        ValueListenableBuilder<Set<String>>(
          valueListenable: app.selectionNotifier,
          builder: (context, selected, _) => _buildToolbar(
            t,
            category,
            typeId,
            selected,
            connection,
            database,
            schema,
          ),
        ),
        Expanded(
          child: _buildContent(
            t,
            app,
            connection,
            database,
            schema,
            state,
            category,
            grid,
          ),
        ),
      ],
    );
  }

  /// 顶部工具栏:按当前对象分类(表 / 视图 / 函数 / 用户 / 查询)展示
  /// 打开/设计/新建/删除/导入/导出等操作,图标使用语义强调色。
  /// 具体 DDL 能力尚未实现,点击后弹出占位提示。
  Widget _buildToolbar(
    AppPalette t,
    ObjectCategory category,
    String? typeId,
    Set<String> selected,
    String connection,
    String database,
    String? schema,
  ) {
    final app = context.read<AppState>();
    final c = AppColors.of(context);
    final l = context.l10n;
    final label = category.labelOf(l);
    // 只有表 / 视图 / 函数 / 过程 / 角色有"设计"语义
    final canDesign = switch (category) {
      ObjectCategory.table ||
      ObjectCategory.view ||
      ObjectCategory.function ||
      ObjectCategory.procedure ||
      ObjectCategory.user => true,
      _ => false,
    };
    // 选中态门控:打开表需要至少选中一项;设计需精确选中一个对象
    final hasSelection = selected.isNotEmpty;
    final singleSelection = selected.length == 1;
    final canDesignEnabled = singleSelection && canDesign;

    // 导入 / 导出向导目前只对表有意义,且要求精确选中一张表(目标唯一)
    final canImportExport = category == ObjectCategory.table &&
        singleSelection &&
        app.connectionManager.isConnected(connection);
    final transferTable = canImportExport ? selected.single : null;

    // 例程分类(函数 / 过程)支持「新建函数 / 新建过程」向导
    final isRoutine = category == ObjectCategory.function ||
        category == ObjectCategory.procedure;

    final stripTokens = _toolbarTokens(t);
    // 搜索框底色随工具条走灰(secondary),但 surfaceColor 不能动 —— ToolStrip
    // 下拉项的 hover 文字色依赖 surfaceColor(浅色才能压在强调色上);这里只给
    // 搜索框单独一份 surfaceColor = t.secondary,使输入框灰底融进灰工具条。
    final searchTokens = stripTokens.copyWith(surfaceColor: t.secondary);

    return ToolStrip(
      tokens: stripTokens,
      trailing: ExpandableSearch(
        key: ValueKey('$connection|$database|$category'),
        tokens: searchTokens,
        onChanged: (v) => setState(() => _objectSearchText = v.toLowerCase()),
      ),
      items: [
        // 打开:表 / 视图 / 实体化视图打开数据页;函数 / 过程打开设计页;
        // 查询打开已保存查询到编辑页
        ToolStripButton(
          icon: Icons.folder_open_outlined,
          iconColor: c.iconWarning,
          text: l.actionOpen(label),
          enabled: hasSelection,
          onPressed: hasSelection
              ? () {
                  if (category == ObjectCategory.table ||
                      category == ObjectCategory.view ||
                      category == ObjectCategory.materializedView) {
                    for (final name in selected) {
                      app.openTable(
                        name,
                        connection: connection,
                        database: database,
                        schema: schema,
                        select: false,
                      );
                    }
                  } else if (category == ObjectCategory.function ||
                      category == ObjectCategory.procedure) {
                    for (final name in selected) {
                      app.designRoutine(
                        name,
                        connection: connection,
                        database: database,
                        category: category,
                        schema: schema,
                      );
                    }
                  } else if (category == ObjectCategory.query) {
                    for (final name in selected) {
                      final q = app.savedQueryOf(
                        name,
                        connection: connection,
                        database: database,
                      );
                      if (q != null) app.openSavedQuery(q);
                    }
                  } else {
                    _showStub(l.actionOpen(label));
                  }
                }
              : null,
        ),
        if (canDesign)
          ToolStripButton(
            icon: Icons.edit_outlined,
            iconColor: c.iconPrimary,
            text: l.actionDesign(label),
            enabled: canDesignEnabled,
            onPressed: canDesignEnabled
                ? () {
                    final name = selected.single;
                    if (category == ObjectCategory.table) {
                      app.designTable(
                        name,
                        connection: connection,
                        database: database,
                        schema: schema,
                      );
                    } else if (category == ObjectCategory.user) {
                      app.designUser(
                        name,
                        connection: connection,
                        database: database,
                        schema: schema,
                      );
                    } else {
                      app.designRoutine(
                        name,
                        connection: connection,
                        database: database,
                        category: category,
                        schema: schema,
                      );
                    }
                  }
                : null,
          ),
        // 新建表:随数据库类型表现不同
        // - PostgreSQL 系:分离式按钮,点击主体 = 新建常规表,箭头下拉
        //   展示 常规 / 外部 / 分区 三种
        // - 其它(MySQL 等):普通按钮,点击直接新建表,无下拉选项
        if (category == ObjectCategory.table)
          const {
            'postgresql',
            'aliyun-rds-postgres',
            'aliyun-polardb-postgres',
            'aliyun-oceanbase-postgres',
          }.contains(typeId)
              ? ToolStripDropDownButton(
                  icon: Icons.add_circle_outline,
                  iconColor: c.iconSuccess,
                  text: l.actionNew(label),
                  onPressed: () =>
                      app.newTableDesigner(connection: connection, database: database, schema: schema),
                  items: [
                    ToolStripDropDownEntry(
                      text: l.tableKindRegular,
                      onPressed: () => app.newTableDesigner(
                          connection: connection, database: database, schema: schema),
                    ),
                    ToolStripDropDownEntry(
                      text: l.tableKindExternal,
                      onPressed: () => _showStub(l.newExternalTable),
                    ),
                    ToolStripDropDownEntry(
                      text: l.tableKindPartition,
                      onPressed: () => _showStub(l.newPartitionTable),
                    ),
                  ],
                )
              : ToolStripButton(
                  icon: Icons.add_circle_outline,
                  iconColor: c.iconSuccess,
                  text: l.actionNew(label),
                  onPressed: () =>
                      app.newTableDesigner(connection: connection, database: database, schema: schema),
                )
        else if (category == ObjectCategory.query)
          // 新建查询:直接打开查询编辑页(自动关联当前连接 / 库)
          ToolStripButton(
            icon: Icons.add_circle_outline,
            iconColor: c.iconSuccess,
            text: l.actionNew(l.catQuery),
            onPressed: () => app.newQuery(),
          )
        else if (isRoutine)
          // 新建函数 / 新建过程:打开函数向导(两步:类型 + 名称 → 参数)。
          // 类型支持过程时用下拉二选一,否则直接进入向导(初始分类固定)
          RoutineSql.supportsProcedure(typeId ?? '')
              ? ToolStripDropDownButton(
                  icon: Icons.add_circle_outline,
                  iconColor: c.iconSuccess,
                  text: l.actionNew(category == ObjectCategory.procedure
                          ? l.catProcedure
                          : l.catFunction),
                  onPressed: () => showFunctionWizard(
                    context,
                    app: app,
                    connection: connection,
                    database: database,
                    schema: schema,
                    initialCategory: category,
                  ),
                  items: [
                    ToolStripDropDownEntry(
                      text: l.actionNew(l.catFunction),
                      onPressed: () => showFunctionWizard(
                        context,
                        app: app,
                        connection: connection,
                        database: database,
                        schema: schema,
                        initialCategory: ObjectCategory.function,
                      ),
                    ),
                    ToolStripDropDownEntry(
                      text: l.actionNew(l.catProcedure),
                      onPressed: () => showFunctionWizard(
                        context,
                        app: app,
                        connection: connection,
                        database: database,
                        schema: schema,
                        initialCategory: ObjectCategory.procedure,
                      ),
                    ),
                  ],
                )
              : ToolStripButton(
                  icon: Icons.add_circle_outline,
                  iconColor: c.iconSuccess,
                  text: l.actionNew(label),
                  onPressed: () => showFunctionWizard(
                    context,
                    app: app,
                    connection: connection,
                    database: database,
                    schema: schema,
                    initialCategory: category,
                  ),
                )
        else if (category == ObjectCategory.view)
          // 新建视图:直接开一张空的设计页,名称先取自增默认名 `view_N`
          // (与函数 / 过程「跳过向导」路径同构 —— 设计页靠 widget.name 拼
          // CREATE VIEW,所以名称必须在打开页面前定下来,不能留到保存时再问)。
          // Access 的 getDefinition 恒为 null,但新建不需要读定义,故照常提供
          ToolStripButton(
            icon: Icons.add_circle_outline,
            iconColor: c.iconSuccess,
            text: l.actionNew(label),
            onPressed: () => app.designRoutine(
              _defaultViewName(),
              connection: connection,
              database: database,
              category: ObjectCategory.view,
              schema: schema,
              isNew: true,
            ),
          )
        else if (category == ObjectCategory.user)
          // 新建角色:直接开一张空的账号设计页,名称先取自增默认名 `role_N`
          // (与「新建视图」同构 —— 设计页靠 widget.name 拼 CREATE USER,
          // 所以名称必须在打开页面前定下来,不能留到保存时再问)。
          // 不支持账号管理的类型(SQLite / Access)连分组都不显示,
          // 这里由 UserSql 再兜一层,避免类型表里漏配时给出死按钮
          ToolStripButton(
            icon: Icons.add_circle_outline,
            iconColor: c.iconSuccess,
            text: l.ctxNewRole,
            enabled: UserSql.supportsUsers(typeId ?? ''),
            onPressed: UserSql.supportsUsers(typeId ?? '')
                ? () => app.designUser(
                      _defaultRoleName(),
                      connection: connection,
                      database: database,
                      schema: schema,
                      isNew: true,
                    )
                : null,
          )
        else
          ToolStripButton(
            icon: Icons.add_circle_outline,
            iconColor: c.iconSuccess,
            text: l.actionNew(label),
            onPressed: () => _showStub(l.actionNew(label)),
          ),
        // 删除:查询分类删除本地已保存的查询(确认后执行);
        // 其余对象分类走真实 DDL(DROP)删除
        ToolStripButton(
          icon: Icons.remove_circle_outline,
          iconColor: const Color(0xFFDC2626),
          text: l.actionDelete(label),
          enabled: hasSelection,
          onPressed: hasSelection
              ? () => _deleteSelected(category, selected,
                  connection: connection, database: database, schema: schema)
              : null,
        ),
        if (category == ObjectCategory.table) ...[
          ToolStripButton(
            icon: Icons.file_download_outlined,
            iconColor: c.iconSuccess,
            text: l.importWizard,
            enabled: canImportExport,
            onPressed: canImportExport
                ? () => _openImportWizard(transferTable!, connection, database, schema)
                : null,
          ),
          ToolStripButton(
            icon: Icons.file_upload_outlined,
            iconColor: const Color(0xFFE9A23B),
            text: l.exportWizard,
            enabled: canImportExport,
            onPressed: canImportExport
                ? () => _openExportWizard(transferTable!, connection, database, schema)
                : null,
          ),
        ],
      ],
    );
  }

  /// 无数据库上下文时显示的工具栏:保留正常工具栏的全部操作位,
  /// 但每个按钮均禁用(灰显不可点),符合「数据库未打开时整个操作栏禁用」的预期。
  /// 分类按钮的文案 / 布局与正常工具栏保持一致(查询分类的新建按钮为「新建查询」)。
  Widget _buildDisabledToolbar(AppPalette t, ObjectCategory category) {
    final c = AppColors.of(context);
    final l = context.l10n;
    final label = category.labelOf(l);
    // 与正常工具栏一致:仅表 / 视图 / 函数 / 过程有「设计」语义;导入 / 导出仅对表有意义
    final canDesign = switch (category) {
      ObjectCategory.table ||
      ObjectCategory.view ||
      ObjectCategory.function ||
      ObjectCategory.procedure => true,
      _ => false,
    };
    final canImportExport = category == ObjectCategory.table;
    final newText =
        category == ObjectCategory.query ? l.actionNew(l.catQuery) : l.actionNew(label);
    return ToolStrip(
      tokens: _toolbarTokens(t),
      items: [
        ToolStripButton(
          icon: Icons.folder_open_outlined,
          iconColor: c.iconWarning,
          text: l.actionOpen(label),
          enabled: false,
        ),
        if (canDesign)
          ToolStripButton(
            icon: Icons.edit_outlined,
            iconColor: c.iconPrimary,
            text: l.actionDesign(label),
            enabled: false,
          ),
        ToolStripButton(
          icon: Icons.add_circle_outline,
          iconColor: c.iconSuccess,
          text: newText,
          enabled: false,
        ),
        ToolStripButton(
          icon: Icons.remove_circle_outline,
          iconColor: const Color(0xFFDC2626),
          text: l.actionDelete(label),
          enabled: false,
        ),
        if (canImportExport) ...[
          ToolStripButton(
            icon: Icons.file_download_outlined,
            iconColor: c.iconSuccess,
            text: l.importWizard,
            enabled: false,
          ),
          ToolStripButton(
            icon: Icons.file_upload_outlined,
            iconColor: const Color(0xFFE9A23B),
            text: l.exportWizard,
            enabled: false,
          ),
        ],
      ],
    );
  }

  /// 对象面板工具栏的 tokens:底色取灰(t.secondary,明亮主题 #F1F1F1,
  /// 与 Navicat 对象面板工具条的 #F0F0F0 对齐),而不是早先的"与正文同白"。
  ///
  /// 之前为规避窄面板"整条带子发灰"刻意用白底;但对比 Navicat 可见其工具条
  /// 本就是灰底、列表才是白底,白底工具条反而和参考图差异明显。改回灰底后,
  /// 面板从标签条到列表形成"灰工具条 + 白领区"的标准桌面布局;搜索框也随
  /// 工具条走灰(见 [_buildToolbar] 里的 searchTokens),边框线仍把输入框勾勒
  /// 出来。hover / pressed 在该灰底上派生(暗色提亮、亮色加深),交互反馈不变。
  DesktopTokens _toolbarTokens(AppPalette t) {
    final isDark = t.background.computeLuminance() < 0.5;
    final hoverBlend =
        isDark ? Colors.white.withValues(alpha: 0.08) : Colors.black.withValues(alpha: 0.08);
    final pressedBlend =
        isDark ? Colors.white.withValues(alpha: 0.14) : Colors.black.withValues(alpha: 0.14);
    return t.desktopTokensFor(context).copyWith(
      controlColor: t.secondary,
      controlHoverColor: Color.alphaBlend(hoverBlend, t.secondary),
      controlPressedColor: Color.alphaBlend(pressedBlend, t.secondary),
      // 尺寸令牌一并收紧:条高 = controlHeight + compactSpacing * 2 ≈ 28
      controlHeight: 22,
      compactSpacing: 3,
      fontSize: 12,
    );
  }

  /// 「导入向导」入口:把 CSV / JSON 文件导入选中的表
  Future<void> _openImportWizard(
    String table,
    String connection,
    String database,
    String? schema,
  ) async {
    final app = context.read<AppState>();
    final conn = app.connectionByName(connection);
    if (conn == null) return;
    await showDataImportWizard(
      context,
      app: app,
      conn: conn,
      database: database,
      table: table,
      schema: schema,
    );
  }

  /// 「导出向导」入口:把选中的表导出为 CSV / SQL / JSON 文件
  Future<void> _openExportWizard(
    String table,
    String connection,
    String database,
    String? schema,
  ) async {
    final app = context.read<AppState>();
    final conn = app.connectionByName(connection);
    if (conn == null) return;
    await showDataExportWizard(
      context,
      app: app,
      conn: conn,
      database: database,
      table: table,
      schema: schema,
    );
  }

  /// 占位提示:未实现的功能统一弹窗告知用户。
  void _showStub(String action) {
    MessageBox.show(
      context,
      title: action,
      message: context.l10n.stubWip(action),
      okText: context.l10n.btnGotIt,
    );
  }

  /// 新建视图的默认名:view_N(与函数 / 过程的默认名同风格)。
  /// 设计页靠 [ViewDesignPage.name] 拼 `CREATE VIEW <名>`,所以名称必须在
  /// 打开页面前定下来 —— 用户可在设计页里改这个默认名(它就是最终对象名)。
  static String _defaultViewName() {
    _viewNameCounter++;
    return 'view_$_viewNameCounter';
  }

  /// 新建角色的默认名:role_N(与视图的默认名同风格,各自独立计数)。
  /// 设计页靠 `widget.name` 拼 `CREATE USER <名>`,名称同样要在打开页面前定下来。
  static String _defaultRoleName() {
    _roleNameCounter++;
    return 'role_$_roleNameCounter';
  }

  /// 「删除」的统一入口(工具栏按钮与 Del 快捷键共用同一口径):
  /// 查询分类删除本地已保存项,备份分类仍是占位,其余分类走真实 DDL(DROP)。
  void _deleteSelected(
    ObjectCategory category,
    Set<String> selected, {
    required String connection,
    required String database,
    String? schema,
  }) {
    if (selected.isEmpty) return;
    // 拷一份:删除流程会清空 AppState 的选中集合,不能迭代它自己
    final names = Set<String>.of(selected);
    if (category == ObjectCategory.query) {
      _deleteQueries(names, connection, database);
    } else if (category == ObjectCategory.backup) {
      _showStub(
          context.l10n.actionDelete(category.labelOf(context.l10n)),
        );
    } else {
      _deleteObjects(category, names.toList(),
          connection: connection, database: database, schema: schema);
    }
  }

  /// 删除选中的已保存查询(确认后执行,并清空面板选中)
  Future<void> _deleteQueries(
    Set<String> selected,
    String connection,
    String database,
  ) async {
    final l = context.l10n;
    final message = selected.length == 1
        ? l.deleteQueryConfirmOne(selected.single)
        : l.deleteQueryConfirmMany('${selected.length}');
    final result = await MessageBox.show(
      context,
      title: l.deleteQueryTitle,
      message: message,
      type: MessageBoxType.warning,
      buttons: MessageBoxButtons.okCancel,
      okText: l.btnDelete,
    );
    if (result != MessageBoxResult.ok || !mounted) return;
    final app = context.read<AppState>();
    for (final name in selected) {
      app.deleteSavedQuery(name, connection: connection, database: database);
    }
    app.clearObjectSelection();
  }

  /// 删除选中对象(表 / 视图 / 实体化视图 / 函数 / 过程):确认后 DROP 并刷新列表。
  /// 失败项弹窗提示,其余照常刷新。
  Future<void> _deleteObjects(
    ObjectCategory category,
    List<String> names, {
    required String connection,
    required String database,
    String? schema,
  }) async {
    final l = context.l10n;
    final label = category.labelOf(l);
    final message = names.length == 1
        ? l.deleteObjectConfirmOne(label, names.single)
        : l.deleteObjectConfirmMany('${names.length}', category.pluralOf(l));
    final result = await MessageBox.show(
      context,
      title: l.actionDelete(label),
      message: message,
      type: MessageBoxType.warning,
      buttons: MessageBoxButtons.okCancel,
      okText: l.btnDelete,
    );
    if (result != MessageBoxResult.ok || !mounted) return;
    final app = context.read<AppState>();
    final failed = await app.dropObjects(
      category,
      names,
      connection: connection,
      database: database,
      schema: schema,
    );
    if (!mounted) return;
    app.clearObjectSelection();
    if (failed.isNotEmpty) {
      MessageBox.show(
        context,
        title: l.actionDelete(label),
        message: l.deleteFailedNames(failed.join(', ')),
        type: MessageBoxType.error,
        okText: l.btnGotIt,
      );
    }
  }

  /// 内容区:按加载状态渲染(加载中 / 错误重试 / 空态 / 网格或列表)
  Widget _buildContent(
    AppPalette t,
    AppState app,
    String connection,
    String database,
    String? schema,
    TableListState state,
    ObjectCategory category,
    bool grid,
  ) {
    // 查询分类:列表来自本地已保存查询(不依赖驱动的加载状态,
    // 无需等待库对象列表拉取)
    if (category == ObjectCategory.query) {
      return _renderItems(
        t,
        app,
        [
          for (final q in app.savedQueriesOf(connection, database)) q.name,
        ],
        category,
        grid,
      );
    }
    switch (state.status) {
      case LoadStatus.idle:
      case LoadStatus.loading:
        return _stateView(
          t,
          icon: const Spinner(size: 20),
          title: context.l10n.loadingObjectsTitle(database),
        );
      case LoadStatus.error:
        return _stateView(
          t,
          icon: const Icon(Icons.error_outline),
          title: context.l10n.openDatabaseFailedTitle(database),
          description: state.error,
          action: Button(
            text: context.l10n.btnRetry,
            onPressed: () => _retryAll(app, connection, database, schema),
          ),
        );
      case LoadStatus.loaded:
        break;
    }

    // 分类级降级:某一类对象单独读取失败(典型如「角色」要读 mysql.user /
    // pg_roles,生产只读账号普遍无权限),表与视图仍可用 —— 只在本分类内
    // 提示原因并可重试,不再让整个库显示为加载失败
    final categoryError = state.categoryErrorOf(category);
    if (categoryError != null) {
      return _stateView(
        t,
        icon: const Icon(Icons.error_outline),
        title: context.l10n
            .categoryListFailedTitle(category.pluralOf(context.l10n)),
        description: categoryError,
        action: Button(
          text: context.l10n.btnRetry,
          onPressed: () => _retryAll(app, connection, database, schema),
        ),
      );
    }

    // 当前分类的子项列表(查询分类已在上方提前返回,不会走到这里);
    // 列表字段空安全兑底:热重载后旧实例的 views/functions 可能为 null
    final rawItems = switch (category) {
      ObjectCategory.table => state.tables ?? const <String>[],
      ObjectCategory.view => state.views ?? const <String>[],
      ObjectCategory.materializedView => state.materializedViews ?? const <String>[],
      ObjectCategory.function => state.functions ?? const <String>[],
      ObjectCategory.procedure => state.procedures ?? const <String>[],
      ObjectCategory.user => state.users ?? const <String>[],
      ObjectCategory.query => const <String>[],
      ObjectCategory.backup => const <String>[],
    };
    if (rawItems.isEmpty) {
      return _blank(t);
    }

    return _renderItems(
      t,
      app,
      rawItems,
      category,
      grid,
      state: state,
    );
  }

  /// 搜索过滤 + 网格 / 列表渲染(表与查询面板共用);过滤后无匹配时留白。
  /// [state] 为当前上下文的对象列表状态,列表模式据此显示「行」「注释」两列
  /// (查询分类来自本地保存的 SQL,无状态可传,两列一律留空)。
  Widget _renderItems(
    AppPalette t,
    AppState app,
    List<String> rawItems,
    ObjectCategory category,
    bool grid, {
    TableListState? state,
  }) {
    // 应用搜索过滤(_objectSearchText 已小写,子串匹配)
    final items = _objectSearchText.isEmpty
        ? rawItems
        : rawItems
            .where((n) => n.toLowerCase().contains(_objectSearchText))
            .toList();
    if (items.isEmpty) {
      // 无匹配(或分类为空):留白,不展示空态提示
      return _blank(t);
    }

    // 回写顺序列表供 Shift 范围选择(与可见项一致)
    app.setObjectTables(items);

    return _itemSurface(
      app: app,
      category: category,
      grid: grid,
      // 可见项(已过滤)决定 F2 / Ctrl+C 的作用范围;
      // 未过滤的 rawItems 用于粘贴命名避重
      visible: items,
      all: rawItems,
      child: grid
          ? _buildGrid(t, items, category)
          : _buildList(t, items, category, state),
    );
  }

  /// 按连接名取 ConnectionInfo(重试时需要)
  ConnectionInfo _connOf(AppState app, String connection) {
    return app.connections.firstWhere((c) => c.name == connection);
  }

  /// 多列网格(详细布局,默认):**列高 = 可视区高度**,第一列从上到下排满
  /// 再向右开第二列(资源管理器式列优先),列数超出可视宽度时横向滚动。
  /// 纵向不滚动——旧实现按 `ceil(数量/列数)` 定行数,对象少时会摊成「一行几个」,
  /// 看上去就是横着排。
  Widget _buildGrid(AppPalette t, List<String> items, ObjectCategory category) {
    return LayoutBuilder(
      builder: (context, constraints) {
        var rows = (constraints.maxHeight / _itemHeight).floor();
        if (rows < 1) rows = 1;
        var cols = (items.length / rows).ceil();
        if (cols < 1) cols = 1;
        // 框选命中测试按列优先反算索引,需要当前网格几何
        _gridCols = cols;
        _gridRows = rows;

        return Container(
          color: t.background,
          child: ListView.builder(
            controller: _gridScrollX,
            scrollDirection: Axis.horizontal,
            // ignore: deprecated_member_use
            cacheExtent: 500,
            itemExtent: _gridCellWidth,
            addAutomaticKeepAlives: false,
            addRepaintBoundaries: true,
            itemCount: cols,
            itemBuilder: (context, col) => SizedBox(
              width: _gridCellWidth,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  for (int rowIndex = 0; rowIndex < rows; rowIndex++)
                    _buildCell(
                      context,
                      col,
                      rows,
                      rowIndex,
                      items,
                      category,
                    ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }

  /// 列表模式(详情视图):固定表头 名称 / 行 / 注释 + 单列对象行。
  /// 「行」为展开库时一并拉到的**估算**行数(读目录统计信息,不扫描数据),
  /// 引擎取不到估算值的对象无键(界面显示横杠);
  /// 「注释」用展开库时一并拉到的对象注释,空注释留空。
  Widget _buildList(
    AppPalette t,
    List<String> items,
    ObjectCategory category,
    TableListState? state,
  ) {
    final comments = switch (category) {
      ObjectCategory.table => state?.tableComments,
      ObjectCategory.view || ObjectCategory.materializedView =>
        state?.viewComments,
      ObjectCategory.function => state?.functionComments,
      _ => null,
    };
    // 只有表 / 视图 / 物化视图有行数语义(函数 / 用户 / 本地查询没有)
    final hasRows = switch (category) {
      ObjectCategory.table ||
      ObjectCategory.view ||
      ObjectCategory.materializedView =>
        true,
      _ => false,
    };
    final estimates = hasRows ? state?.rowEstimates : null;

    return Container(
      color: t.background,
      child: Column(
        children: [
          _listHeader(t),
          Expanded(
            child: ListView.builder(
              controller: _itemsScroll,
              itemExtent: _itemHeight,
              itemCount: items.length,
              itemBuilder: (context, index) {
                final name = items[index];
                return _buildObjectItem(
                  context,
                  name,
                  category,
                  meta: _RowMeta(
                    hasRows: hasRows,
                    rows: estimates?[name],
                    comment: comments?[name] ?? '',
                  ),
                  editing: _renaming == name,
                  onCommitRename: _commitRename,
                );
              },
            ),
          ),
        ],
      ),
    );
  }

  /// 列表模式表头:与数据行共用 [_listColumns],列起点严格对齐。
  /// 底色取 secondary(灰),与上方工具条连成统一的灰色控制区,
  /// 下方数据行才是白底 —— 对齐 Navicat 的"灰头 + 白领"。
  Widget _listHeader(AppPalette t) {
    return SizedBox(
      height: _listHeaderHeight,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: t.secondary,
          border: Border(bottom: BorderSide(color: t.divider)),
        ),
        child: _listColumns(
          name: _listHeaderCell(t, context.l10n.colName),
          rows: _listHeaderCell(t, context.l10n.colRowsEstimated),
          comment: _listHeaderCell(t, context.l10n.colComment),
        ),
      ),
    );
  }

  Widget _listHeaderCell(AppPalette t, String text) => Text(
        text,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: TextStyle(fontSize: _rowFontSize, color: t.mutedForeground),
      );

  Widget _buildCell(
    BuildContext context,
    int col,
    int rows,
    int rowIndex,
    List<String> items,
    ObjectCategory category,
  ) {
    final index = col * rows + rowIndex;

    // 空白补位
    if (index >= items.length) {
      return SizedBox(
        width: _gridCellWidth,
        height: _itemHeight,
      );
    }

    final name = items[index];
    return SizedBox(
      width: _gridCellWidth,
      child: RepaintBoundary(
        child: _buildObjectItem(
          context,
          name,
          category,
          editing: _renaming == name,
          onCommitRename: _commitRename,
        ),
      ),
    );
  }

  // ── 交互外壳:键盘(F2 改名 / Ctrl+C / Ctrl+V)+ 鼠标框选 ──────────

  /// 把网格 / 列表内容包进可聚焦、可框选的交互层:
  /// 点内容区取得键盘焦点(编辑中点到编辑格以外则收起编辑器),
  /// 按住左键拖动按几何算出命中项交给 [AppState.selectMany]。
  Widget _itemSurface({
    required AppState app,
    required ObjectCategory category,
    required bool grid,
    required List<String> visible,
    required List<String> all,
    required Widget child,
  }) {
    return LayoutBuilder(
      builder: (context, constraints) {
        return Focus(
          focusNode: _listFocus,
          onKeyEvent: (node, event) =>
              _onListKey(app, category, visible, all, event),
          child: Listener(
            behavior: HitTestBehavior.translucent,
            onPointerDown: (event) {
              final editing = _renaming;
              if (editing != null) {
                // 编辑器自己不响应"点别处",靠失焦提交;点编辑格以内则保留光标
                if (!_pointerInEditingRow(
                    event.localPosition, editing, visible, grid)) {
                  FocusManager.instance.primaryFocus?.unfocus();
                }
                return;
              }
              if (!_listFocus.hasFocus) _listFocus.requestFocus();
            },
            child: MarqueeSelector(
              canStart: (event) {
                if (_renaming != null) return false;
                // 滚动条命中带让给滚动条:拖滚动条不该顺手改选中。
                // 网格横向滚动 → 底缘,列表纵向滚动 → 右缘;无溢出时不让,
                // 否则等于凭空挖一条点不动的死区。
                final sc = grid ? _gridScrollX : _itemsScroll;
                if (!sc.hasClients || sc.position.maxScrollExtent <= 0) {
                  return true;
                }
                return grid
                    ? event.localPosition.dy <
                        constraints.maxHeight - _scrollBarHitWidth
                    : event.localPosition.dx <
                        constraints.maxWidth - _scrollBarHitWidth;
              },
              onMarqueeStart: () => _marqueeAdditive = app.ctrlPressed,
              onMarqueeUpdate: (band) => app.selectMany(
                _itemsInBand(band, visible, grid),
                additive: _marqueeAdditive,
              ),
              onMarqueeEnd: () => _marqueeAdditive = false,
              child: child,
            ),
          ),
        );
      },
    );
  }

  /// 对象列表快捷键:F2 把单选中的表转入内联改名,Ctrl+C 复制选中表,
  /// Ctrl+V 把它们按 `原名_copy` 克隆回当前 连接 / 库 / 模式(结构 + 数据)。
  /// F2 / 复制粘贴只对表分类生效；Del 对所有分类生效,等价于点工具栏「删除{分类}」。
  /// 内联编辑器持有焦点时一律放行(Ctrl+C 归文本框)。
  KeyEventResult _onListKey(
    AppState app,
    ObjectCategory category,
    List<String> visible,
    List<String> all,
    KeyEvent event,
  ) {
    if (event is! KeyDownEvent || _renaming != null) {
      return KeyEventResult.ignored;
    }
    final connection = app.objectConnection;
    final database = app.objectDatabase;
    if (connection == null || database == null) return KeyEventResult.ignored;

    final isTable = category == ObjectCategory.table;
    final key = event.logicalKey;

    if (key == LogicalKeyboardKey.delete) {
      if (app.selectedTables.isEmpty) return KeyEventResult.ignored;
      _deleteSelected(category, app.selectedTables,
          connection: connection,
          database: database,
          schema: app.objectSchema);
      return KeyEventResult.handled;
    }

    if (key == LogicalKeyboardKey.f2) {
      if (!isTable || app.selectedTables.length != 1) {
        return KeyEventResult.ignored;
      }
      final name = app.selectedTables.first;
      // 改名要落库:未连接时不进入编辑态(与右键菜单的禁用口径一致)
      if (!visible.contains(name) ||
          !app.connectionManager.isConnected(connection)) {
        return KeyEventResult.ignored;
      }
      setState(() => _renaming = name);
      return KeyEventResult.handled;
    }

    if (!isTable || !app.ctrlPressed) return KeyEventResult.ignored;
    if (key == LogicalKeyboardKey.keyC) {
      _copyTables(app, visible, connection, database);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyV) {
      _pasteTables(app, all, connection, database);
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  /// 指针是否落在正在改名的那一格(视口坐标)。列表模式整行通栏,只比纵向;
  /// 网格纵向不滚动,横向列位要减去横向滚动偏移。
  bool _pointerInEditingRow(
    Offset p,
    String name,
    List<String> items,
    bool grid,
  ) {
    final index = items.indexOf(name);
    if (index < 0) return false;
    if (!grid) {
      final offset = _itemsScroll.hasClients ? _itemsScroll.offset : 0.0;
      // 列表模式的表头固定在 ListView 上方,视口 y 比内容 y 多出这一段
      final top = index * _itemHeight - offset + _listHeaderHeight;
      return p.dy >= top && p.dy < top + _itemHeight;
    }
    final row = index % _gridRows;
    final col = index ~/ _gridRows;
    final top = row * _itemHeight;
    if (p.dy < top || p.dy >= top + _itemHeight) return false;
    final offsetX = _gridScrollX.hasClients ? _gridScrollX.offset : 0.0;
    final left = col * _gridCellWidth - offsetX;
    return p.dx >= left && p.dx < left + _gridCellWidth;
  }

  /// 选框矩形(视口坐标)命中哪些对象项:
  /// 列表按行号直取(纵向滚动 + 固定表头要换算成内容坐标);
  /// 网格按列优先反算(`index = col * rows + row`),纵向即视口、横向减滚动偏移。
  List<String> _itemsInBand(Rect band, List<String> items, bool grid) {
    if (items.isEmpty) return const [];

    if (!grid) {
      final hasScroll = _itemsScroll.hasClients;
      final offset = hasScroll ? _itemsScroll.offset : 0.0;
      final viewportHeight = hasScroll
          ? _itemsScroll.position.viewportDimension
          : double.infinity;
      // 先扣掉固定表头那段,选框才落在 ListView 自己的坐标系里
      final content = band.shift(const Offset(0, -_listHeaderHeight));
      final top = content.top + offset;
      final bottom = content.bottom + offset;
      // 选框完全落在视口外(上下两端)时无命中
      if (bottom <= offset || top >= offset + viewportHeight) return const [];
      final lastRow = items.length - 1;
      final firstRow = (top / _itemHeight).floor().clamp(0, lastRow);
      final endRow = ((bottom / _itemHeight).ceil() - 1).clamp(0, lastRow);
      return [for (var r = firstRow; r <= endRow; r++) items[r]];
    }

    final offsetX = _gridScrollX.hasClients ? _gridScrollX.offset : 0.0;
    final viewportWidth = _gridScrollX.hasClients
        ? _gridScrollX.position.viewportDimension
        : double.infinity;
    final gridHeight = _gridRows * _itemHeight;
    // 选框完全落在网格可视区外时无命中(纵向已排满,横向可能还有列)
    if (band.bottom <= 0 || band.top >= gridHeight) return const [];
    if (band.right <= offsetX || band.left >= offsetX + viewportWidth) {
      return const [];
    }

    final lastRow = _gridRows - 1;
    final firstRow = (band.top / _itemHeight).floor().clamp(0, lastRow);
    final endRow = ((band.bottom / _itemHeight).ceil() - 1).clamp(0, lastRow);
    final firstCol =
        ((band.left + offsetX) / _gridCellWidth).floor().clamp(0, _gridCols - 1);
    final lastCol =
        ((band.right + offsetX) / _gridCellWidth).ceil().clamp(0, _gridCols) - 1;
    return [
      for (var c = firstCol; c <= lastCol; c++)
        for (var r = firstRow; r <= endRow; r++)
          if (c * _gridRows + r < items.length) items[c * _gridRows + r],
    ];
  }

  /// 内联改名收口:编辑器卸载 → 焦点收回列表;传入原名(Esc / 未改动)即取消。
  Future<void> _commitRename(String oldName, String input) async {
    if (_renaming == null) return;
    setState(() => _renaming = null);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _listFocus.requestFocus();
    });
    final newName = input.trim();
    if (newName.isEmpty || newName == oldName) return;
    final app = context.read<AppState>();
    final connection = app.objectConnection;
    final database = app.objectDatabase;
    final conn = app.connectionByName(connection);
    if (conn == null || database == null) return;
    final outcome = await app.renameTable(
      conn,
      database,
      oldName,
      newName,
      schema: app.objectSchema,
    );
    if (outcome.ok) {
      // 列表刷新后新名保持选中,详情面板与工具栏跟着走
      app.selectTable(newName);
      return;
    }
    if (!mounted) return;
    MessageBox.show(
      context,
      title: context.l10n.renameTableTitle,
      message: context.l10n.renameFailedDetail('${outcome.error}'),
      type: MessageBoxType.error,
      okText: context.l10n.btnGotIt,
    );
  }

  /// Ctrl+C:选中表记入应用内剪贴板,状态栏给一句回执
  void _copyTables(
    AppState app,
    List<String> visible,
    String connection,
    String database,
  ) {
    final names = [
      for (final n in visible)
        if (app.selectedTables.contains(n)) n,
    ];
    if (names.isEmpty) return;
    app.copyTablesToClipboard(
      names,
      connection: connection,
      database: database,
      schema: app.objectSchema,
    );
    app.logTreeAction(context.l10n.copiedTables('${names.length}'));
  }

  /// Ctrl+V:确认后按 [AppState.tablePastePlan] 排定的新名逐表克隆;
  /// 失败项弹窗列出原因,成功只在状态栏回执
  Future<void> _pasteTables(
    AppState app,
    List<String> all,
    String connection,
    String database,
  ) async {
    final cb = app.tableClipboard;
    if (cb == null) return;
    final conn = app.connectionByName(connection);
    if (conn == null || !app.connectionManager.isConnected(connection)) {
      _showPasteNotice(context.l10n.pasteNeedOpenConnection(connection));
      return;
    }
    if (cb.connection != connection ||
        cb.database != database ||
        cb.schema != app.objectSchema) {
      final from = '${cb.connection} · ${cb.database}'
          '${cb.schema == null ? '' : ' · ${cb.schema}'}';
      _showPasteNotice(context.l10n.pasteWrongContext(from));
      return;
    }
    final plan = app.tablePastePlan(all.toSet());
    if (plan.isEmpty) return;
    final result = await MessageBox.show(
      context,
      title: context.l10n.pasteTableTitle,
      message: context.l10n.pasteConfirmDetail(
        '${plan.length}',
        plan.map((e) => '${e.$1} → ${e.$2}').join('\n'),
      ),
      type: MessageBoxType.question,
      buttons: MessageBoxButtons.okCancel,
      okText: context.l10n.btnPaste,
    );
    if (result != MessageBoxResult.ok || !mounted) return;
    final failed = await app.pasteTables(plan);
    if (!mounted) return;
    _listFocus.requestFocus();
    if (failed.isEmpty) {
      app.logTreeAction(context.l10n.pastedTables('${plan.length}'));
      return;
    }
    MessageBox.show(
      context,
      title: context.l10n.pasteTableTitle,
      message: context.l10n.pasteFailedDetail(failed.join('\n')),
      type: MessageBoxType.error,
      okText: context.l10n.btnGotIt,
    );
  }

  void _showPasteNotice(String message) {
    if (!mounted) return;
    MessageBox.show(
      context,
      title: context.l10n.pasteTableTitle,
      message: message,
      type: MessageBoxType.info,
      okText: context.l10n.btnGotIt,
    );
  }

  /// 内容区状态视图(加载中 / 错误):复用 base-ui 的 [Empty]。
  /// 手绘 `Row(mainAxisSize.min)` + `Flexible(Text)` 会把整行撑到容器全宽,
  /// 长错误文本被挤成一行省略号、重试按钮贴到右缘甚至被裁掉;[Empty] 的
  /// `maxWidth` 让文本在固定宽度内换行居中,按钮紧随其下。
  Widget _stateView(
    AppPalette t, {
    required Widget icon,
    required String title,
    String? description,
    Widget? action,
  }) {
    return Container(
      color: t.background,
      child: Empty(
        icon: icon,
        title: title,
        description: description,
        action: action,
        compact: true,
        maxWidth: 520,
      ),
    );
  }

  /// 重新拉取当前库 / 模式的全部对象列表(整体失败与单分类降级共用入口)
  void _retryAll(
      AppState app, String connection, String database, String? schema) {
    final connInfo = _connOf(app, connection);
    if (schema == null) {
      app.connectionManager.retryExpandDatabase(connInfo, database);
    } else {
      app.connectionManager.retryExpandSchema(connInfo, database, schema);
    }
  }

  /// 空白内容区:无数据时直接留白,不展示任何数据或空态提示
  Widget _blank(AppPalette t) => Container(color: t.background);
}

/// 对象实例图标尺寸:面板自有尺寸(连接树分组节点用 database_tree 的独立尺寸)
const double _objectIconSize = 16;

/// 对象项行高与网格单元格宽度:渲染与框选 / 改名格命中测试共用,
/// 改了这里框选才会跟着对齐
const double _itemHeight = 22;
const double _gridCellWidth = 176;

/// 对象行与表头的字号:与 [_itemHeight] 配套的紧凑密度
const double _rowFontSize = 12;

/// 内容区右缘让给滚动条的命中带宽度:在此范围内按下不起框选(拖滚动条)
const double _scrollBarHitWidth = 12;

/// 列表模式表头高度与「行」列宽度:表头与数据行共用同一套列宽才能对齐;
/// 表头固定在列表上方不随滚动,框选命中测试要把这段高度从选框里扣掉
const double _listHeaderHeight = 20;
const double _rowsColWidth = 88;

/// 估算行数的展示:无统计值 -> 横杠;不足档位原样;过档位折成约数
/// (「约 1.2 万」「约 3.4 亿」)。既守住「行」列宽度,也让人一眼看出是约数。
String _formatRowEstimate(int? rows, AppLocalizations l) {
  if (rows == null) return '-';
  // 档位基数随语言分叉:中文与日语按万进位(10^4 / 10^8),英语按千进位
  // (10^3 / 10^6)。万、亿在英语里没有对应词,K 与 M 也不合中文习惯。
  final metric = l.localeName.startsWith('en');
  final small = metric ? 1000 : 10000;
  final large = metric ? 1000000 : 100000000;
  final (divisor, unit) = rows >= large
      ? (large, l.rowsUnitLarge)
      : rows >= small
          ? (small, l.rowsUnitSmall)
          : (0, '');
  if (divisor == 0) return '$rows';
  final scaled = (rows / divisor * 10).round() / 10;
  final text =
      scaled % 1 == 0 ? '${scaled.round()}' : scaled.toStringAsFixed(1);
  return l.rowsApprox(text, unit);
}

/// 列表模式一行的三列骨架(表头与数据行共用,故列起点必然对齐):
/// 名称 flex 3 / 行固定 [_rowsColWidth] / 注释 flex 2,左右各 8 内边距。
/// 行图标的缩进放在名称列**内部**,不参与列宽分配。
Widget _listColumns({
  required Widget name,
  required Widget rows,
  required Widget comment,
}) {
  return Padding(
    padding: const EdgeInsets.symmetric(horizontal: 8),
    child: Row(
      children: [
        Expanded(flex: 3, child: name),
        SizedBox(width: _rowsColWidth, child: rows),
        Expanded(flex: 2, child: comment),
      ],
    ),
  );
}

/// 列表模式某行的列元数据(网格模式传 null,只画名称)
class _RowMeta {
  const _RowMeta({
    required this.hasRows,
    required this.rows,
    required this.comment,
  });

  /// 该分类是否有「行数」语义(函数 / 用户 / 本地查询没有,列留空)
  final bool hasRows;

  /// 引擎目录里的估算行数;null = 取不到统计值 → 显示横杠
  final int? rows;
  final String comment;
}

/// 对象实例图标:与对应分组节点同源的自绘 SVG(assets/icons/ui/*);
/// 尺寸与连接树分组节点图标一致(单一数据源 ObjectCategoryIcon)
Widget _objectItemIcon(BuildContext context, ObjectCategory category) =>
    ObjectCategoryIcon(category: category, size: _objectIconSize);

/// 单个表项:ValueListenableBuilder 监听按项独立通知器,仅重建变化的 1~2 项;
/// 行交互(选中 PointerDown 零延迟 / 双击独立手势)自绘,不引入 Material 墨水。
/// [editing] 为真时该行改由 base-ui [InlineEditor] 就地改名(不再有选中与双击手势,
/// 提交 / 取消经 [onCommitRename] 上抛,传入原名即取消)。
/// [meta] 非空(列表模式)时,行按 [_listColumns] 三列排布并显示行数与注释;
/// 为空(网格模式)时只显示图标 + 名称。
Widget _buildObjectItem(
  BuildContext context,
  String name,
  ObjectCategory category, {
  bool editing = false,
  _RowMeta? meta,
  Future<void> Function(String oldName, String input)? onCommitRename,
}) {
  final t = Tokens.of(context);
  final app = context.read<AppState>();
  final notifier = app.itemNotifierFor(name);

  return ValueListenableBuilder<bool>(
    valueListenable: notifier,
    builder: (context, selected, _) {
      if (editing && onCommitRename != null) {
        return SizedBox(
          height: _itemHeight,
          child: ColoredBox(
            color: selected ? t.treeSelectedBg : Colors.transparent,
            child: InlineEditor(
              initialValue: name,
              height: _itemHeight,
              contentPadding: const EdgeInsets.symmetric(horizontal: 4),
              onCommit: (input) => onCommitRename(name, input),
              // Esc 以原名回传 → 宿主判为"未改动"即取消
              onCancel: () => onCommitRename(name, name),
            ),
          ),
        );
      }
      // 选中走 Listener.onPointerDown:一旦注册 onDoubleTap,
      // 手势竞技场会被 DoubleTapGestureRecognizer hold 住约 300ms,
      // onTap 必须等竞技场解决后才触发,这就是单击卡顿的来源
      return Listener(
        behavior: HitTestBehavior.opaque,
        onPointerDown: (event) {
          app.selectTable(name);
          // 右键表实例 → 表上下文菜单(打开 / 删除 / 清空 / 设计 / 转储SQL / 复制重命名);
          // 右键视图 / 实体化视图 → 视图菜单(按引擎能力裁剪);
          // 右键函数 / 过程 → 例程菜单(设计 / 删除)。
          // 仅当面板已关联连接与库时弹出(正常浏览对象时必然满足)
          if (event.buttons == kSecondaryMouseButton &&
              app.objectConnection != null &&
              app.objectDatabase != null) {
            ConnectionInfo? conn;
            for (final c in app.connections) {
              if (c.name == app.objectConnection) {
                conn = c;
                break;
              }
            }
            if (conn != null) {
              if (category == ObjectCategory.table) {
                showTableContextMenu(
                  context: context,
                  app: app,
                  conn: conn,
                  database: app.objectDatabase!,
                  table: name,
                  schema: app.objectSchema,
                  position: event.position,
                );
              } else if (category == ObjectCategory.view ||
                  category == ObjectCategory.materializedView) {
                showViewContextMenu(
                  context: context,
                  app: app,
                  category: category,
                  conn: conn,
                  database: app.objectDatabase!,
                  name: name,
                  schema: app.objectSchema,
                  position: event.position,
                );
              } else if (category == ObjectCategory.function ||
                  category == ObjectCategory.procedure) {
                showRoutineContextMenu(
                  context: context,
                  app: app,
                  category: category,
                  conn: conn,
                  database: app.objectDatabase!,
                  name: name,
                  schema: app.objectSchema,
                  position: event.position,
                );
              } else if (category == ObjectCategory.user) {
                showUserContextMenu(
                  context: context,
                  app: app,
                  conn: conn,
                  database: app.objectDatabase!,
                  name: name,
                  schema: app.objectSchema,
                  position: event.position,
                );
              }
            }
          }
        },
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          // 双击:表 / 视图 / 实体化视图打开数据页;函数 / 过程打开设计页
          onDoubleTap: switch (category) {
            ObjectCategory.table ||
            ObjectCategory.view ||
            ObjectCategory.materializedView =>
              () => app.openTable(
                    name,
                    connection: app.objectConnection!,
                    database: app.objectDatabase!,
                    schema: app.objectSchema,
                  ),
            ObjectCategory.function || ObjectCategory.procedure => () =>
                app.designRoutine(
                  name,
                  connection: app.objectConnection!,
                  database: app.objectDatabase!,
                  category: category,
                  schema: app.objectSchema,
                ),
            ObjectCategory.user => () => app.designUser(
                  name,
                  connection: app.objectConnection!,
                  database: app.objectDatabase!,
                  schema: app.objectSchema,
                ),
            _ => null,
          },
          child: SizedBox(
            height: _itemHeight,
            child: ColoredBox(
              color: selected ? t.treeSelectedBg : Colors.transparent,
              child: _rowContent(context, t, name, category, meta),
            ),
          ),
        ),
      );
    },
  );
}

/// 对象项内容行:[meta] 为空(网格模式)只画图标 + 名称;
/// 非空(列表模式)按 [_listColumns] 排成 名称 / 行 / 注释 三列,
/// 图标缩进留在名称列内部,使表头与数据行的列起点一致
Widget _rowContent(
  BuildContext context,
  AppPalette t,
  String name,
  ObjectCategory category,
  _RowMeta? meta,
) {
  final nameText = Text(
    name,
    maxLines: 1,
    overflow: TextOverflow.ellipsis,
    style: TextStyle(fontSize: _rowFontSize, color: t.foreground),
  );
  final nameCell = Row(
    children: [
      // 对象实例图标:与对应分组节点同源的 PNG
      _objectItemIcon(context, category),
      const SizedBox(width: 6),
      Expanded(child: nameText),
    ],
  );
  if (meta == null) {
    return Row(children: [
      const SizedBox(width: 8),
      Expanded(child: nameCell),
    ]);
  }
  return _listColumns(
    name: nameCell,
    // 估算行数:无统计值显示横杠,过万折成「约 N 万」;有值用正文色突出
    rows: Text(
      meta.hasRows ? _formatRowEstimate(meta.rows, context.l10n) : '',
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: TextStyle(
        fontSize: _rowFontSize,
        color: meta.rows == null ? t.mutedForeground : t.foreground,
      ),
    ),
    comment: Text(
      meta.comment,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: TextStyle(fontSize: _rowFontSize, color: t.mutedForeground),
    ),
  );
}

/// 对象面板工具栏最右侧的可展开搜索框(base-ui [ExpandableSearch]):
/// 收起时是放大镜图标按钮,点击展开为输入框(自动聚焦);
/// 清空内容并失焦或点关闭按钮时收起。输入经 [ExpandableSearch.onChanged] 上抛。
