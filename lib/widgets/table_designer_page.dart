import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/gestures.dart' show kSecondaryButton;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:re_editor/re_editor.dart';
import 'package:re_highlight/languages/sql.dart';
import '../app/app_state.dart';
import '../app/connection_manager.dart';
import '../data/column_clipboard.dart';
import '../data/db_metadata.dart';
import '../data/drivers/db_driver.dart';
import '../data/table_design.dart';
import '../theme/app_theme.dart';
import '../theme/sql_highlight.dart';
import 'index_column_picker_dialog.dart';
import 'sql_find_bar.dart';

// ── 度量:全部取自参考工具(Navicat 表设计器)截图的设备像素实测 ÷ 200% 缩放 ──
//
// 截图为 3338×1802 设备像素、Windows 200% 缩放,故逻辑像素 = 实测值 / 2。
// 这些数字彼此是有关系的,不是凑出来的整数:
//   行内容 19.5 + 1px 底线 = 行距 20.5;属性面板控件同样 19.5 高、行距 26;
//   表头 29.5(淡蓝块自顶内缩 5、底端压 2px 强调条);行首槽 12(淡蓝块 + 2px 强调条);
//   工具栏 28.5、内标签条 20.5、属性面板上分隔条 3、属性标签列左起 11、字段自 201 起宽 300。
// 布局时把「内容 + 1px 线」拆成整数(20 + 1 = 21),避免 0.5px 造成 1px 线糊成两档。
const double _kRowContentH = 20; // 行内容高(= gridCellTokens 的 controlHeight)
const double _kRowExtent = _kRowContentH + 1; // 行占位 = 内容 + 1px 底线
const double _kHeaderH = 30; // 表头行占位(含 1px 底线)
const double _kHeaderInsetTop = 5; // 焦点列表头淡蓝块的顶部内缩
const double _kGutterW = 12; // 行首「当前行」槽宽
const double _kAccentBarW = 2; // 槽右端 / 焦点列表头下划线的强调条宽
const double _kCellPadX = 5; // 单元格文字水平留白(经 gridCellTokens 下发)
const double _kCheckSize = 13; // 勾选框方框边长(不随行高缩放)
const double _kToolbarH = 29; // 工具栏高
const double _kTabBarH = 21; // 「字段 / 索引 …」标签条高
const double _kIconBtnW = 25; // 纯图标按钮的点击块宽(实测 25 = 14 图标 + 5.5 × 2)
const double _kIconPadX = (_kIconBtnW - 14) / 2; // 图标按钮水平内边距 = 5.5
const double _kToolSepW = 17; // 工具栏分隔块宽(1px 线 + 两侧 8 留白)
const double _kPropSplitterH = 3; // 网格与属性面板之间的分隔条
const double _kPropRowH = 26; // 属性面板行距
const double _kPropPadL = 11; // 属性标签左起
const double _kPropLabelW = 190; // 属性标签列宽(字段起点 = 11 + 190 = 201)
const double _kPropFieldW = 300; // 属性字段默认宽

/// 表设计器:「新建表」与「设计表」共用同一页面(新建 + 编辑双模式)。
///
/// 以文档标签页形式打开(新建见 [AppState.newTableDesigner],编辑已有表见
/// [AppState.designTable]),含 11 个标签:
/// 字段 / 索引 / 外键 / 唯一键 / 检查 / 排除 / 规则 / 触发器 / 选项 / 注释 / SQL 预览。
/// 各标签实时采集设计数据([DesignTable]),「字段」页为「网格 + 底部纵向属性面板」:
/// 属性面板按方言展示默认 / 排序规则 / 维度 / 虚拟类型(IDENTITY)及其序列选项 /
/// 循环;「键」列以 钥匙 + 序号 表示主键。
/// 「字段」网格支持像数据表格那样操作行:Ctrl 加选 / Shift 连选,选中行可整批
/// 复制 / 粘贴(见 [ColumnRowClipboard],Tab 分隔一行一字段)/ 删除 / 设主键。
/// 「SQL 预览」标签按当前连接类型实时生成方言 DDL(预览即所见):
/// 新建模式输出 CREATE 语句,编辑模式输出 [DdlBuilder.buildAlterStatements]
/// 生成的变更语句。点「保存」经 [AppState.createTableDesign] /
/// [AppState.saveTableDesignEdit] 逐条执行,PostgreSQL 家族在事务中执行,失败回滚。
///
/// 编辑模式由 [existingTable] 触发:打开时经 [DatabaseDriver.readTableDesign]
/// 反查真实结构回填,并留存快照 [DesignTable.snapshot] 作为差异基线;
/// 驱动不支持结构反查时(SQLite / Access)降级为只读展示 —— 编辑控件全部
/// 禁用并提示,与「读取失败」(展示错误 + 重试)严格区分。
///
/// 版式(工具栏纯图标、网格度量、当前格反白、属性面板对齐)按参考工具逐像素
/// 复刻,数值见文件级 `_k*` 常量;表名不在页面上填:新建时点「保存」弹窗要,
/// 已有表改名走对象树 F2([AppState.renameTable])。
///
/// 交互约定:行选中与「键」列切换用 [Listener.onPointerDown] 零延迟;输入框一律
/// `selectAllOnFocus: false`(桌面聚焦全选坑);颜色全部走 [Tokens.of] / [AppColors.of],
/// 尺寸档位经 [gridCellTokens] 由 [TokenScope] 一次性下发。
class TableDesignerPage extends StatefulWidget {
  const TableDesignerPage({
    super.key,
    required this.title,
    this.tabKey = '',
    required this.connection,
    required this.database,
    this.schema,
    this.existingTable,
    this.designLoader,
    this.candidatesLoader,
    this.dialectOverride,
  });

  /// 标签标题(显示与表名解析用)
  final String title;

  /// 本标签的业务唯一键([OpenTab.key]):关闭 / 重命名标签时据此精确定位,
  /// 避免同名表在设计器里误伤另一个库 / 模式的同名标签。独立测试页可不传。
  final String tabKey;

  /// 所属连接名
  final String connection;

  /// 所属数据库
  final String database;

  /// 目标模式(PostgreSQL 等有模式层的类型;无模式层为 null)
  final String? schema;

  /// 非空 = 编辑已有表(「设计表」):打开时反查结构回填并可保存变更;
  /// 为 null = 新建表。
  final String? existingTable;

  /// 表结构反查注入点(测试用):默认 null 走 [AppState] 的 ConnectionManager。
  final Future<DesignTable?> Function(
      String database, String table, String? schema)? designLoader;

  /// 下拉候选注入点(测试用):默认 null 走 ConnectionManager 的系统目录查询。
  final Future<DesignCandidates> Function(String database)? candidatesLoader;

  /// 方言覆盖注入点(测试用):默认 null 取 [connection] 那条连接的类型。
  ///
  /// 存在的必要不是省事:单测里只要注册一条真连接,initState 的
  /// [_loadRefTables] / [_ensureCandidates] 就会真去建驱动连库,断言永远等不到
  /// 静止帧。只给方言、不给连接,才能把「界面按方言门控」这类分支测起来。
  final String? dialectOverride;

  @override
  State<TableDesignerPage> createState() => _TableDesignerPageState();
}

class _TableDesignerPageState extends State<TableDesignerPage> {
  /// 设计数据(各标签共写)
  final DesignTable _design = DesignTable();

  /// 表名输入(默认 Untitled,与参考工具一致)
  final TextEditingController _nameCtrl = TextEditingController(text: 'Untitled');

  /// SQL 预览控制器(只读,每次重建时同步 DDL 文本)
  final CodeLineEditingController _sqlCtrl = CodeLineEditingController.fromText('');

  // 预览编辑器的查找控制器:绑定 _sqlCtrl,须随本页 dispose
  late final CodeFindController _sqlFindController =
      CodeFindController(_sqlCtrl);

  /// 表注释输入控制器(注释标签页)
  final TextEditingController _commentCtrl = TextEditingController();

  /// 内容区(网格)的键盘焦点:点行选中后由网格持有,Ctrl+C / Ctrl+V 才有落点
  /// (焦点一旦进入单元格输入框,这两个键就是文本复制 / 粘贴,归输入框)
  final FocusNode _gridFocus = FocusNode(debugLabel: 'tableDesignerGrid');

  /// 当前连接类型 id(方言分支全靠它;类型显示名不在本页出现 ——
  /// 参考工具的设计器页面上没有标题条,库 / 连接 / 类型都在文档标签标题里)
  String _typeId = 'postgresql';

  /// 外键引用表候选(当前模式,懒加载)
  List<String> _refTables = const [];

  /// 模式 / 函数 / 用户 候选(索引字段弹窗的排序规则模式、触发器函数、表所有者)
  List<String> _schemas = const [];
  List<String> _functions = const [];
  List<String> _users = const [];

  /// 排序规则 / 运算符类别 / 表空间 候选(整页只查一次;为空时退化为手输)
  List<String> _collations = const [];
  List<String> _opClasses = const [];
  List<String> _tablespaces = const [];

  /// 候选查询是否已发出(发出后失败也不重发,避免每次展开控件都重试)
  bool _candidatesRequested = false;

  /// 触发器属性面板的子标签索引(0 = 常规,1 = 约束)
  int _triggerPropsTab = 0;

  /// 编辑基线:打开时反查到的结构快照,差异 / 脏态一律与它比较
  DesignTable? _original;

  @override
  void setState(VoidCallback fn) {
    // 整页级变更(增删行 / 换标签 / 改主键 / 保存态)都可能动到设计数据,
    // 差异清单一律作废,避免「已修改」显示成旧状态。
    _diffCache = null;
    super.setState(fn);
  }

  /// 编辑模式的差异清单 + 「改不动」原因,一次算完两处共用。
  ///
  /// 一次敲字有三处要读它:保存键可用性、状态栏「已修改 / 无变更」、SQL 预览。
  /// 而 `hasChanges` 的实现是「把整套 ALTER 生成出来再看空不空」,同一帧里
  /// 重复付三遍就是白给的开销(150 列的表上尤其明显),故缓存一份。
  ///
  /// [blocked] 与语句清单一起算:ALTER 表达不了的变更(触发器 / 规则 / 排除 /
  /// 存储参数)不会出现在 [stmts] 里,只看 `stmts.isEmpty` 会把「用户明明改了,
  /// 只是这页改不动」显示成「无变更」并灰掉保存键 —— 用户连拒绝原因都看不到。
  /// 作废口:[setState] 与 [_touchCell] / [_touchChrome]。
  ({List<String> stmts, String? blocked})? _diffCache;

  ({List<String> stmts, String? blocked}) get _diff {
    final base = _original;
    if (!_isEdit || _readOnly || base == null) {
      return (stmts: const [], blocked: null);
    }
    return _diffCache ??= (
      stmts: DdlBuilder.buildAlterStatements(_design, base, _typeId),
      blocked: DdlBuilder.alterUnsupported(_design, base, _typeId),
    );
  }

  /// 编辑模式的差异语句清单 = 「保存」要执行的东西。
  List<String> get _alterStatements => _diff.stmts;

  bool _saving = false;
  String? _error;

  /// 编辑模式:结构反查进行中 / 读取失败原因 / 驱动不支持编辑(只读降级)
  bool _loading = false;
  String? _loadError;
  bool _readOnly = false;

  /// 上次保存成功的提示(状态栏显示,一有变更即撤下)
  String? _note;

  /// 校验结果**现算**:定向重建改造后,build 里缓存的快照会在「只重建了
  /// 某一行 / 某块 chrome」时变陈旧,一律在需要它的那一块里当场算。
  /// 表名为空不算无效 —— 纯图标工具栏没有表名框,名字在点保存时弹窗补。
  ///
  /// 两条「改不动」的拦口都在这里并进状态栏,原则是**拒于按下保存之前**:
  /// - 新建:[DdlBuilder.createUnsupported]。方言表达不了的设计项(MySQL 连接
  ///   上的规则 / 排除 / 索引并发 …)会被 [DdlBuilder.buildStatements] 的方言门
  ///   跳过,不拦就是用户填了、界面回「保存成功」、库里什么都没有。
  /// - 编辑:[DdlBuilder.alterUnsupported](见 [_diff])。不并进来的话,改触发器
  ///   只会让保存键灰着 + 状态栏写「无变更」,拒绝原因永远看不到。
  DesignValidate get _validation {
    final r = DdlBuilder.validate(_design, typeId: _typeId, requireName: false);
    if (!r.ok) return r;
    if (_isEdit) {
      final blocked = _diff.blocked;
      return blocked == null ? r : DesignValidate(false, blocked);
    }
    final blocked = DdlBuilder.createUnsupported(_design, _typeId);
    return blocked == null ? r : DesignValidate(false, blocked);
  }

  /// 当前活动标签索引(TabControl 内部持有选中态,通过 onChanged 同步到此)
  int _tabIndex = 0;

  /// 当前单元格所在列(参考工具的「当前格」模型:点哪格哪格反白,
  /// 表头同步给该列画淡蓝块 + 2px 强调下划线)。切标签时归 0,
  /// 因为各标签列数与语义都不同。
  int _curCol = 0;

  // ── 定向重建(性能)──────────────────────────────────────────
  //
  // 整页 setState 是这个页面的卡顿根因:一屏有 160+ 个常开编辑器,实测
  // **每按一个字符重建整页 ≈ 140ms、每点一行 ≈ 130ms**(探针 A/B,见
  // 提交说明)。改成按依赖面分档通知:
  //
  // * [_rowRevs] —— 每行一个信号,单元格文本 / 勾选 / 下拉只重建**所在行**;
  // * [_chromeRev] —— 依赖「设计数据内容」的那几小块:保存键可用性、工具栏
  //   动作可用性、状态栏提示、表头当前列、底部属性面板、SQL 预览;
  // * 结构性变化(增删行 / 换标签 / 改主键 / 上下移)仍走整页 setState,
  //   它们本来就要重建全部行,且是低频动作。
  //
  // 行号会随增删错位,但 notifier 只是**信号**、内容永远现读模型,
  // 且任何改变行数的操作都伴随整页重建,因此最多多重建一次、不会漏重建。
  final ValueNotifier<int> _chromeRev = ValueNotifier<int>(0);
  final List<ValueNotifier<int>> _rowRevs = <ValueNotifier<int>>[];

  /// 底部属性面板单独一档:只有「换选中行」或「改到面板要显示的那个属性」
  /// (典型是改类型 → 排序规则可用性、改虚拟类型 → 序列那一组)才需要重画。
  /// 面板里有 10+ 个编辑器,跟着每次敲字重建就是白付的钱。
  final ValueNotifier<int> _propsRev = ValueNotifier<int>(0);

  /// 表头单独一档:它只依赖「当前列」,不依赖设计数据内容。
  final ValueNotifier<int> _curColRev = ValueNotifier<int>(0);

  ValueNotifier<int> _rowRev(int i) {
    while (_rowRevs.length <= i) {
      _rowRevs.add(ValueNotifier<int>(0));
    }
    return _rowRevs[i];
  }

  /// 单元格内容变了:重建所在行 + 依赖设计数据的那几小块。
  /// [propsAlso] = 这次改动会牵动底部属性面板(改类型 / 改勾选类属性)。
  void _touchCell(int row, {bool propsAlso = false}) {
    _diffCache = null;
    _dropStaleNote();
    if (row >= 0) _rowRev(row).value++;
    _chromeRev.value++;
    if (propsAlso) _propsRev.value++;
  }

  /// 只刷依赖设计数据的小块(保存键 / 状态栏 / 工具栏可用性),不动任何行
  void _touchChrome() {
    _diffCache = null;
    _dropStaleNote();
    _chromeRev.value++;
    _propsRev.value++;
  }

  /// 一有实质变更就撤下上次保存的提示,不长期挂「已保存」。
  ///
  /// 定向重建后 `build()` 不再每次敲字都跑,这句必须挂在变更信号上,
  /// 否则改完半天状态栏还写着「已保存 3 条变更」。
  void _dropStaleNote() {
    if (_note != null && _dirty) _note = null;
  }

  // ── 模式判定 ────────────────────────────────────

  /// 是否编辑已有表(「设计表」)
  bool get _isEdit => widget.existingTable != null;

  /// 相对编辑基线是否有待应用的变更。
  /// 不自己逐字段比对:直接复用差异生成,保证与「SQL 预览」同一口径
  /// (清单本身缓存在 [_alterStatements],一处算、三处用)。
  bool get _dirty => _alterStatements.isNotEmpty;

  /// 「保存」是否可点:编辑模式额外要求「有变更」且不在加载中
  bool get _canSave =>
      !_saving &&
      !_loading &&
      _loadError == null &&
      _validation.ok &&
      !_readOnly &&
      (!_isEdit || _dirty);

  /// 调整已有列顺序仅 MySQL 家族能生成 `MODIFY ... AFTER`
  bool get _canReorderColumns => !_isEdit || DdlBuilder.isMysqlLike(_typeId);

  /// 当前标签的动作按钮是否可用:只读降级时全灰;编辑模式下排除 / 规则两个标签
  /// 在非 PostgreSQL 连接上置灰(该方言库里既读不出也改不了这两类对象),
  /// 与 [DdlBuilder.alterUnsupported] 的拦口保持一致。
  /// 触发器(标签 7)已支持编辑(按 DROP + CREATE 重建),不在置灰名单内。
  bool get _actionsEnabled {
    if (_readOnly) return false;
    if (_isEdit && (_tabIndex == 5 || _tabIndex == 6) && !DdlBuilder.isPgLike(_typeId)) {
      return false;
    }
    return true;
  }

  /// 各标签当前选中行
  int _selColumn = 0;
  int _selIndex = -1;
  int _selFk = -1;
  int _selUnique = -1;
  int _selCheck = -1;
  int _selExclude = -1;
  int _selRule = -1;
  int _selTrigger = -1;

  /// 字段网格的多选行(索引集合,支持 Ctrl 加选 / Shift 连选)。
  /// [_selColumn] 始终是锚点:▶ 标记、底部属性面板、上移 / 下移跟随它;
  /// 复制 / 粘贴 / 删除 / 设主键作用于整个选择集。其它标签为单选,不维护本集合。
  final Set<int> _selColumns = {};

  // ── 下拉候选 ──────────────────────────────────────────────

  /// 常用字段类型(小写存储,覆盖 PostgreSQL / MySQL 主流;类型格可输入自定义值)
  static const List<String> _typeOptions = [
    'int2',
    'int4',
    'int8',
    'integer',
    'smallint',
    'mediumint',
    'bigint',
    'tinyint',
    'serial',
    'bigserial',
    'smallserial',
    'decimal',
    'numeric',
    'float',
    'real',
    'double precision',
    'money',
    'bit',
    'bool',
    'boolean',
    'char',
    'varchar',
    'bpchar',
    'text',
    'tinytext',
    'mediumtext',
    'longtext',
    'date',
    'time',
    'datetime',
    'timestamp',
    'timestamptz',
    'interval',
    'year',
    'uuid',
    'json',
    'jsonb',
    'xml',
    'bytea',
    'blob',
    'tinyblob',
    'mediumblob',
    'longblob',
    'varbit',
    'inet',
    'cidr',
    'macaddr',
    'enum',
    'set',
  ];

  /// 索引方法(PostgreSQL 全量;MySQL 仅 btree / hash,保存时由服务端校验)
  static const List<String> _indexMethods = ['btree', 'hash', 'gist', 'gin', 'brin', 'spgist'];

  /// 排除约束方法
  static const List<String> _excludeMethods = ['gist', 'spgist', 'btree', 'hash', 'gin', 'brin'];

  /// 排除元素的比较运算符候选(仍可手输)。EXCLUDE 只能用到与所选索引方法
  /// 配对的那些操作符,列表按常用排在前,首项空 = 未指定(保存时校验拦下)。
  static const List<String> _excludeOperators = [
    '',
    '&&',
    '=',
    '<<<',
    '>>>',
    '<<',
    '>>',
    '&<',
    '&>',
    '&<=',
    '>>=',
    '@>',
    '<@',
    '-|-',
    '~-',
    '@-@',
    '##',
    '<',
    '>',
    '<=',
    '>=',
    '<>',
  ];

  /// 外键删除 / 更新行为
  static const List<String> _fkActions = [
    'NO ACTION',
    'RESTRICT',
    'CASCADE',
    'SET NULL',
    'SET DEFAULT',
  ];

  /// 可延迟 / 延迟 的 YES / NO(空 = 不生成子句)
  static const List<String> _yesNo = ['', 'YES', 'NO'];

  /// 触发器 FOR EACH
  static const List<String> _forEachOptions = ['行', '语句'];

  /// 触发器时机
  static const List<String> _timingOptions = ['BEFORE', 'AFTER', 'INSTEAD OF'];

  /// 触发器 FOR EACH(候选按方言:MySQL 只有行级;SQL Server 根本没有该子句,
  /// 列表留空让控件置灰,而不是给一个改了不生效的选项)
  List<String> get _forEachChoices {
    if (DdlBuilder.isPgLike(_typeId)) return _forEachOptions;
    if (DdlBuilder.isMysqlLike(_typeId)) return const ['行'];
    return const [];
  }

  /// 触发器时机(候选按方言:MySQL 无 INSTEAD OF;SQL Server 无 BEFORE)
  List<String> get _timingChoices {
    if (DdlBuilder.isMysqlLike(_typeId)) return const ['BEFORE', 'AFTER'];
    if (DdlBuilder.isSqlServerLike(_typeId)) return const ['AFTER', 'INSTEAD OF'];
    return _timingOptions;
  }

  /// 触发器主体形态:PostgreSQL 引用独立函数,MySQL / SQL Server 把过程体
  /// 写进触发器本身。两条路互斥,属性面板只画当前方言用得上的那一组。
  bool get _triggerUsesFunction => DdlBuilder.isPgLike(_typeId);

  /// 规则事件
  static const List<String> _ruleEvents = ['INSERT', 'UPDATE', 'DELETE', 'SELECT'];

  /// 规则动作变体:`DO ALSO`(附带执行)/ `DO INSTEAD`(代替执行)/ `DO NOTHING`(丢弃)
  static const List<String> _ruleActions = ['ALSO', 'INSTEAD', 'NOTHING'];

  /// 「虚拟类型」下拉候选(与截图一致;与 identityMode 的双向映射见下方函数)
  static const List<String> _identityOptions = [
    '无',
    'GENERATED ALWAYS AS IDENTITY',
    'GENERATED BY DEFAULT AS IDENTITY',
  ];

  /// 默认值候选(仍可手工输入其它表达式)
  static const List<String> _defaultOptions = [
    '',
    'NULL',
    'true',
    'false',
    'current_timestamp',
    'current_date',
    '0',
    "''",
  ];

  /// PostgreSQL 常用排序规则候选(仍可手工输入)
  static const List<String> _collationOptions = [
    '',
    'default',
    'C',
    'POSIX',
    'en_US.utf8',
    'zh_CN.utf8',
  ];

  /// 删除类动作图标色(实测参考工具的红圈减号 #FF4A37)
  static const Color _iconDanger = Color(0xFFFF4A37);

  /// 添加类动作图标色(实测参考工具绿圈加号 #45C300)
  static const Color _iconAdd = Color(0xFF45C300);

  /// 「键」列 / 主键按钮的金色钥匙(实测 #F5AA00)
  static const Color _iconKey = Color(0xFFF5AA00);

  /// 字段网格列宽(实测:名称 200 / 类型 120 / 长度 60 / 小数点 60 /
  /// 不是 null 60 / 键 60 / 注释 300;行首槽另占 [_kGutterW])
  static const List<double> _fieldWidths = [200, 120, 60, 60, 60, 60, 300];

  // ── 生命周期 ──────────────────────────────────────────────

  @override
  void initState() {
    super.initState();
    final app = context.read<AppState>();
    final conn = app.connectionByName(widget.connection);
    _typeId = widget.dialectOverride ?? conn?.typeId ?? 'postgresql';
    // 新建模式预置一行空字段(与参考工具一致);编辑模式的列由反查结果填充
    if (!_isEdit) _design.columns.add(DesignColumn());
    _loadRefTables();
    _ensureCandidates();
    if (_isEdit) _loadExisting();
  }

  @override
  void dispose() {
    _nameCtrl.dispose();
    _commentCtrl.dispose();
    // findController 监听 SQL 编辑器,必须先于它销毁
    _sqlFindController.dispose();
    _sqlCtrl.dispose();
    _gridFocus.dispose();
    _chromeRev.dispose();
    _propsRev.dispose();
    _curColRev.dispose();
    for (final r in _rowRevs) {
      r.dispose();
    }
    super.dispose();
  }

  /// 懒加载下拉候选:外键「被引用的表」与模式 / 函数 / 用户 列表。
  /// 加载失败保持空列表(仍可手动输入表名)。
  Future<void> _loadRefTables() async {
    final app = context.read<AppState>();
    final conn = app.connectionByName(widget.connection);
    if (conn == null || !app.connectionManager.isConnected(conn.name)) return;
    final manager = app.connectionManager;
    try {
      final state = manager.tableStateOf(conn.name, widget.database, schema: widget.schema);
      if (state.status != LoadStatus.loaded) {
        if (widget.schema != null) {
          await manager.expandSchema(conn, widget.database, widget.schema!);
        } else {
          await manager.expandDatabase(conn, widget.database);
        }
      }
      final fresh = manager.tableStateOf(conn.name, widget.database, schema: widget.schema);
      // 模式列表独立缓存(函数名的 schema 候选、外键被引用模式)
      final schemas = manager.schemaStateOf(conn.name, widget.database);
      if (schemas.status == LoadStatus.idle) {
        await manager.ensureSchemas(conn, widget.database);
      }
      if (!mounted) return;
      setState(() {
        _refTables = fresh.tables ?? const [];
        _functions = fresh.functions ?? const [];
        _users = fresh.users ?? const [];
        _schemas = manager.schemaStateOf(conn.name, widget.database).schemas;
      });
    } catch (_) {
      // 连接未就绪等场景:静默,允许手动输入表名
    }
  }

  /// 懒加载排序规则 / 运算符类别 / 表空间 候选(整页只发一次查询)。
  ///
  /// 连接未就绪时不置已发标志(下次打开控件可重试);查询失败或驱动无
  /// 对应系统目录时候选留空,控件退化为手输,不打扰用户。
  Future<void> _ensureCandidates() async {
    if (_candidatesRequested) return;
    final loader = widget.candidatesLoader;
    final app = context.read<AppState>();
    final conn = app.connectionByName(widget.connection);
    final manager = app.connectionManager;
    if (loader == null &&
        (conn == null || !manager.isConnected(conn.name))) {
      return; // 连接未就绪:下次打开控件仍可重试
    }
    _candidatesRequested = true;
    try {
      final fresh = loader != null
          ? await loader(widget.database)
          : await manager.readDesignCandidates(conn!, widget.database);
      if (!mounted) return;
      setState(() {
        _collations = fresh.collations;
        _opClasses = fresh.opClasses;
        _tablespaces = fresh.tablespaces;
      });
    } catch (_) {
      // 无系统目录读权限 / 连接已断开:候选留空,控件退化为手输
    }
  }

  /// 排序规则候选:优先用数据库实际目录,取不到时退回常用静态列表
  /// (首项空串 = 不指定,与静态列表保持一致)。
  List<String> get _collationChoices =>
      _collations.isEmpty ? _collationOptions : ['', ..._collations];

  /// 反查已有表结构回填,并留存基线快照。
  ///
  /// 驱动返回 null = 该类型不支持结构编辑(SQLite / Access):降级为只读展示,
  /// 仅按 [DatabaseDriver.describeTable] 填列基础信息;抛错 = 读取失败,
  /// 展示错误与「重试」(不能静默变成一张空表,否则用户会误以为表无字段)。
  Future<void> _loadExisting() async {
    final table = widget.existingTable;
    if (table == null) return;
    final app = context.read<AppState>();
    final loader = widget.designLoader;
    final conn =
        loader == null ? app.connectionByName(widget.connection) : null;
    if (loader == null && conn == null) {
      setState(() => _loadError = '连接 "${widget.connection}" 不存在');
      return;
    }
    setState(() {
      _loading = true;
      _loadError = null;
    });
    try {
      final read = loader != null
          ? await loader(widget.database, table, widget.schema)
          : await app.connectionManager
              .readTableDesign(conn!, widget.database, table, schema: widget.schema);
      if (!mounted) return;
      if (read == null) {
        final cols = conn == null
            ? const <ColumnDef>[]
            : await app.connectionManager.describeTable(
                conn, widget.database, table, schema: widget.schema);
        if (!mounted) return;
        _applyDesign(
            DesignTable()
              ..name = table
              ..schema = widget.schema
              ..columns.addAll(cols.map(_columnFromDef)),
            readOnly: true);
        return;
      }
      _applyDesign(read);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _loading = false;
        _readOnly = false;
        _loadError = e.toString();
      });
    }
  }

  /// 反查结果落到界面,并重取基线快照(必须为独立副本,否则改完就没基线了)。
  void _applyDesign(DesignTable read, {bool readOnly = false}) {
    final fallback = widget.existingTable ?? '';
    final name = read.name.trim().isEmpty ? fallback : read.name.trim();
    _design
      ..name = name
      ..schema = widget.schema
      ..tableComment = read.tableComment
      ..unlogged = read.unlogged
      ..owner = read.owner
      ..inherits = read.inherits
      ..cluster = read.cluster
      ..tablespace = read.tablespace
      ..pkName = read.pkName;
    // 存储参数整张表带走:reloptions 除 fillfactor 外还有 autovacuum_* 等一堆,
    // 只搬 fillFactor 会让「选项」页其余格子空着,基线里却有值 → 一保存就被当成清空。
    _design.storageOptions
      ..clear()
      ..addAll(read.storageOptions);
    _design.columns
      ..clear()
      ..addAll(read.columns);
    _design.indexes
      ..clear()
      ..addAll(read.indexes);
    _design.foreignKeys
      ..clear()
      ..addAll(read.foreignKeys);
    _design.uniqueKeys
      ..clear()
      ..addAll(read.uniqueKeys);
    _design.checks
      ..clear()
      ..addAll(read.checks);
    _design.excludes
      ..clear()
      ..addAll(read.excludes);
    _design.rules
      ..clear()
      ..addAll(read.rules);
    _design.triggers
      ..clear()
      ..addAll(read.triggers);
    _original = _design.snapshot();
    _nameCtrl.text = name;
    _commentCtrl.text = read.tableComment;
    _selColumn = _design.columns.isEmpty ? -1 : 0;
    _selColumns
      ..clear()
      ..addAll(_selColumn < 0 ? const <int>[] : [_selColumn]);
    _selIndex = _design.indexes.isEmpty ? -1 : 0;
    _selFk = _design.foreignKeys.isEmpty ? -1 : 0;
    _selUnique = _design.uniqueKeys.isEmpty ? -1 : 0;
    _selCheck = _design.checks.isEmpty ? -1 : 0;
    _selExclude = _design.excludes.isEmpty ? -1 : 0;
    _selRule = _design.rules.isEmpty ? -1 : 0;
    _selTrigger = _design.triggers.isEmpty ? -1 : 0;
    setState(() {
      _loading = false;
      _readOnly = readOnly;
      _loadError = null;
      _error = null;
      _note = null;
    });
  }

  /// 只读降级时把 [ColumnDef] 映射为网格行:仅名称 / 类型 / 长度 / 小数点 /
  /// 可空 / 主键 / 默认值 / 注释。这些信息不足以生成 ALTER,故该模式不可保存。
  DesignColumn _columnFromDef(ColumnDef d) {
    final split = splitColumnType(d.type);
    return DesignColumn(
      name: d.name,
      type: baseTypeOf(d.type, _typeId),
      length: split.length,
      decimal: split.decimal,
      notNull: !d.nullable,
      primaryKey: d.primaryKey,
      comment: d.comment,
      defaultValue: d.defaultValue ?? '',
    );
  }

  // ── 保存 ──────────────────────────────────────────────────

  /// 保存:新建模式执行 CREATE 全套语句;编辑模式先二次确认再执行 ALTER。
  ///
  /// 工具栏是纯图标、页面上没有表名框(参考工具同),所以表名在这里补:
  /// 名为空时先弹「表名」输入框,取消即放弃这次保存。已有表的改名走对象树 F2
  /// (`AppState.renameTable`),不在本页做 —— 那是整棵对象树的统一入口。
  Future<void> _save() async {
    if (_readOnly || _loading || _saving) return;
    final app = context.read<AppState>();
    if (isUntitledTableName(_design.name)) {
      final typed = await InputDialog.show(
        context,
        title: '表名',
        message: '请输入要创建的表名',
        initialValue: '',
        okText: '保存',
      );
      if (typed == null) return;
      final name = typed.trim();
      if (name.isEmpty) {
        if (mounted) setState(() => _error = '表名不能为空');
        return;
      }
      if (!mounted) return;
      setState(() {
        _design.name = name;
        _nameCtrl.text = name;
        _error = null;
      });
    }
    final base = _original;
    if (_isEdit && base == null) return;
    final stmts = _isEdit
        ? _alterStatements
        : DdlBuilder.buildStatements(_design, _typeId);
    if (_isEdit) {
      // 无法用 ALTER 表达的变更(改列序 / 触发器 / 表选项等)直接阻断并说明原因,
      // 不静默丢弃用户的修改
      final blocked = DdlBuilder.alterUnsupported(_design, base!, _typeId);
      if (blocked != null) {
        setState(() => _error = blocked);
        return;
      }
      if (stmts.isEmpty) {
        setState(() => _note = '没有待应用的变更');
        return;
      }
      // 危险变更(删列 / 主键重建)只靠这一次确认;正文回显语句数与首条语句
      final risky = stmts.any((s) => RegExp(r'\bDROP\s+(COLUMN|PRIMARY\s+KEY)\b',
              caseSensitive: false)
          .hasMatch(s));
      final result = await MessageBox.show(
        context,
        title: '确认变更表结构',
        message: '将在 ${widget.database} 对 ${_design.name.trim()} 执行 '
            '${stmts.length} 条变更语句。\n\n'
            '${stmts.first}${stmts.length > 1 ? '\n…' : ''}'
            '${risky ? '\n\n含删除列 / 主键重建,数据不可逆。' : ''}',
        type: risky ? MessageBoxType.warning : MessageBoxType.question,
        buttons: MessageBoxButtons.okCancel,
        okText: risky ? '仍要执行' : '执行变更',
        cancelText: '取消',
      );
      if (!mounted || result != MessageBoxResult.ok) return;
    }
    setState(() {
      _saving = true;
      _error = null;
      _note = null;
    });
    final outcome = _isEdit
        ? await app.saveTableDesignEdit(
            _design,
            base!,
            connection: widget.connection,
            database: widget.database,
            schema: widget.schema,
          )
        : await app.createTableDesign(
            _design,
            connection: widget.connection,
            database: widget.database,
            schema: widget.schema,
          );
    if (!mounted) return;
    if (!outcome.ok) {
      // 保留编辑内容与基线(仍为「已修改」),原文回显驱动错误
      final detail = outcome.error ?? (_isEdit ? '保存表结构失败' : '新建表失败');
      final String? tail;
      if (!_isEdit) {
        tail = null;
      } else if (outcome.rolledBack) {
        tail = '所有变更已回滚';
      } else if (outcome.failedAt > 0) {
        tail = '第 ${outcome.failedAt} 条语句失败,之前的变更已生效';
      } else {
        tail = null;
      }
      setState(() {
        _saving = false;
        _error = tail == null ? detail : '$detail · $tail';
      });
      return;
    }
    if (!_isEdit) {
      // 成功后关闭设计器标签;对象列表已由 createTableDesign 刷新
      app.closeTab(widget.tabKey);
      return;
    }
    // 编辑成功后不关标签(可继续改),只把基线前移到当前设计
    final oldTitle = widget.title;
    final newTitle = '${_design.name.trim()} (设计)';
    setState(() {
      _saving = false;
      _original = _design.snapshot();
      _note = '已保存 ${stmts.length} 条变更';
    });
    // 表被重命名:同步换标签标题,否则下次打开会错配到旧表名
    if (newTitle != oldTitle) app.renameTab(widget.tabKey, newTitle);
  }

  // ── 主布局 ────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final t = Tokens.of(context);
    // 一有实质变更就撤下上次保存的提示,不长期挂「已保存」
    if (_note != null && _dirty) _note = null;
    return Shortcuts(
      shortcuts: const {
        SingleActivator(LogicalKeyboardKey.keyS, control: true): _SaveIntent(),
        SingleActivator(LogicalKeyboardKey.keyS, meta: true): _SaveIntent(),
      },
      child: Actions(
        actions: {
          _SaveIntent: CallbackAction<_SaveIntent>(
            onInvoke: (_) {
              if (_canSave) _save();
              return null;
            },
          ),
        },
        child: TokenScope(
          // 整页落到「单元格档」:字号 12 / 控件 20 高 / 水平留白 5。
          // 参考工具的设计器从工具栏到属性面板都是这一档,逐控件传 height
          // 会漏(且 ComboBox 没有 height 形参),在 TokenScope 上一次性收口。
          tokens: gridCellTokens(
              TokenScope.maybeOf(context) ?? DesktopTokens.winForm),
          child: Container(
            color: t.background,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                // 页内不再有标题条:表名 @ 库.模式 由文档标签标题承载
                _toolbar(t),
                // TabControl 仅作标签条(子标签不挂 child,内容区由下方 Expanded
                // 自渲染——TabControl 内容区是 shrink-wrap,不能承载 Expanded 子级)
                TabControl(
                  initialIndex: 0,
                  barHeight: _kTabBarH,
                  tabPaddingX: 11.5,
                  tabBarColor: t.muted,
                  selectedTabColor: t.surface,
                  unselectedTabColor: t.muted,
                  hoverTabColor: t.secondary,
                  contentPadding: EdgeInsets.zero,
                  onChanged: (i) => setState(() {
                    _tabIndex = i;
                    _curCol = 0;
                  }),
                  tabs: [
                    for (final label in const [
                      '字段', '索引', '外键', '唯一键', '检查', '排除',
                      '规则', '触发器', '选项', '注释', 'SQL 预览',
                    ])
                      TabItem(label: label),
                    ],
                ),
                Expanded(
                  child: Focus(
                    focusNode: _gridFocus,
                    onKeyEvent: _onGridKey,
                    child: _tabBody(t, _tabIndex),
                  ),
                ),
                _statusBar(t),
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// 内容区容器:加载中 / 读取失败占满整块;只读降级在顶部加提示条。
  Widget _tabBody(AppPalette t, int index) {
    final table = widget.existingTable ?? '';
    if (_loading) {
      return Empty(
        icon: const Spinner(size: 20),
        title: '正在加载 $table 的结构 ...',
        compact: true,
        maxWidth: 520,
      );
    }
    if (_loadError != null) {
      return Empty(
        icon: const Icon(Icons.error_outline),
        title: '读取 $table 结构失败',
        description: _loadError,
        action: Button(text: '重试', onPressed: _loadExisting),
        compact: true,
        maxWidth: 520,
      );
    }
    final body = _tabContent(t, index);
    if (!_readOnly) return body;
    final warn = AppColors.of(context).iconWarning;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: 22,
          alignment: Alignment.centerLeft,
          padding: const EdgeInsets.symmetric(horizontal: 12),
          // 由主题警告色派生底色,明暗两套都能看清
          color: Color.alphaBlend(warn.withValues(alpha: 0.16), t.surface),
          child: Row(
            children: [
              Icon(Icons.lock_outline, size: 13, color: warn),
              const SizedBox(width: 6),
              const Expanded(
                child: Label(
                  '当前数据库类型暂不支持编辑已有表结构,仅可查看',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ),
        ),
        Expanded(child: body),
      ],
    );
  }

  /// 按活动标签索引返回对应内容
  Widget _tabContent(AppPalette t, int index) {
    switch (index) {
      case 0:
        return _fieldsTab(t);
      case 1:
        return _indexTab(t);
      case 2:
        return _fkTab(t);
      case 3:
        return _uniqueTab(t);
      case 4:
        return _checkTab(t);
      case 5:
        return _excludeTab(t);
      case 6:
        return _ruleTab(t);
      case 7:
        return _triggerTab(t);
      case 8:
        return _optionsTab(t);
      case 9:
        return _commentsTab(t);
      default:
        return _sqlTab(t);
    }
  }

  Widget _toolbar(AppPalette t) {
    return Container(
      height: _kToolbarH,
      color: t.secondary,
      child: Row(
        children: [
          // 保存按钮的可用性取决于设计数据(校验 + 是否有变更),挂在 chrome
          // 信号上:敲字时只重画这一颗按钮,不牵动整页与网格。
          ValueListenableBuilder(
            valueListenable: _chromeRev,
            builder: (context, _, __) => ToolbarButton(
              icon: Icons.save_outlined,
              iconColor: AppColors.of(context).iconPrimary,
              tooltip: _isEdit ? '保存变更 (Ctrl+S)' : '保存并建表 (Ctrl+S)',
              enabled: _canSave,
              paddingX: _kIconPadX,
              onTap: _save,
            ),
          ),
          _toolSep(t),
          // 窄窗口下动作按钮可能多于可用宽:横向滚动而非 RenderFlex 溢出
          Expanded(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: ValueListenableBuilder(
                valueListenable: _chromeRev,
                builder: (context, _, __) => _tabActions(t),
              ),
            ),
          ),
        ],
      ),
    );
  }

  /// 工具栏分隔块:1px 竖线 + 两侧留白(实测分隔线 23 高,几乎占满工具栏)
  Widget _toolSep(AppPalette t) => SizedBox(
        width: _kToolSepW,
        child: Center(
          child: Container(width: 1, height: 23, color: t.border),
        ),
      );

  /// 「添加 X / 删除 X」图标对(绿色 ⊕ / 红色 ⊖)
  List<Widget> _addRemoveActions({
    required String label,
    required VoidCallback onAdd,
    required VoidCallback onRemove,
  }) {
    return [
      ToolbarButton(
        icon: Icons.add_circle_outline,
        iconColor: _iconAdd,
        tooltip: '添加$label',
        paddingX: _kIconPadX,
        onTap: onAdd,
      ),
      ToolbarButton(
        icon: Icons.remove_circle_outline,
        iconColor: _iconDanger,
        tooltip: '删除$label',
        enabled: _actionsEnabled,
        paddingX: _kIconPadX,
        onTap: onRemove,
      ),
    ];
  }

  /// 当前标签的图标动作。
  ///
  /// 只放参考工具图标栏上有的那几个:字段页 = ⊕ 添加 / ⊖ 删除 / 🔑 主键 ▾,
  /// 其余标签 = ⊕ / ⊖。插入字段 / 复制 / 粘贴 / 上移 / 下移收进行右键菜单
  /// (见 [_columnContextMenu]),工具栏不再挂文字按钮。
  Widget _tabActions(AppPalette t) {
    final actions = <Widget>[];
    switch (_tabIndex) {
      case 0:
        final hasSel = _selectedColumns().isNotEmpty;
        actions.addAll([
          ToolbarButton(
            icon: Icons.add_circle_outline,
            iconColor: _iconAdd,
            tooltip: '添加字段',
            enabled: _actionsEnabled,
            paddingX: _kIconPadX,
            onTap: _addColumn,
          ),
          ToolbarButton(
            icon: Icons.remove_circle_outline,
            iconColor: _iconDanger,
            tooltip: '删除字段',
            enabled: _actionsEnabled && hasSel,
            paddingX: _kIconPadX,
            onTap: _removeColumn,
          ),
          // 「主键 ▾」:弹层开合由 DropDownButton 的 Listener 处理,按钮仅提供视觉态
          DropDownButton(
            width: 140,
            trigger: ToolbarButton(
              icon: Icons.key,
              iconColor: _iconKey,
              tooltip: '设置 / 取消主键(多选即复合主键)',
              showCaret: true,
              enabled: _actionsEnabled && _design.columns.isNotEmpty,
              paddingX: _kIconPadX,
              onTap: () {},
            ),
            items: [
              ListItem(
                title: '设置主键',
                enabled: _actionsEnabled && hasSel,
                onSelect: () => _setPk(true),
              ),
              ListItem(
                title: '取消主键',
                enabled: _actionsEnabled && hasSel,
                onSelect: () => _setPk(false),
              ),
            ],
          ),
        ]);
      case 1:
        actions.addAll(_addRemoveActions(
          label: '索引',
          onAdd: () => _addRow(_design.indexes, (i) => _selIndex = i),
          onRemove: () => _removeRow(_design.indexes, () => _selIndex, (i) => _selIndex = i),
        ));
      case 2:
        actions.addAll(_addRemoveActions(
          label: '外键',
          onAdd: _addFk,
          onRemove: () => _removeRow(_design.foreignKeys, () => _selFk, (i) => _selFk = i),
        ));
      case 3:
        actions.addAll(_addRemoveActions(
          label: '唯一键',
          onAdd: () => _addRow(_design.uniqueKeys, (i) => _selUnique = i),
          onRemove: () => _removeRow(_design.uniqueKeys, () => _selUnique, (i) => _selUnique = i),
        ));
      case 4:
        actions.addAll(_addRemoveActions(
          label: '检查',
          onAdd: () => _addRow(_design.checks, (i) => _selCheck = i),
          onRemove: () => _removeRow(_design.checks, () => _selCheck, (i) => _selCheck = i),
        ));
      case 5:
        actions.addAll(_addRemoveActions(
          label: '排除',
          onAdd: () => _addRow(_design.excludes, (i) => _selExclude = i),
          onRemove: () => _removeRow(_design.excludes, () => _selExclude, (i) => _selExclude = i),
        ));
      case 6:
        actions.addAll(_addRemoveActions(
          label: '规则',
          onAdd: () => _addRow(_design.rules, (i) => _selRule = i),
          onRemove: () => _removeRow(_design.rules, () => _selRule, (i) => _selRule = i),
        ));
      case 7:
        actions.addAll(_addRemoveActions(
          label: '触发器',
          onAdd: () => _addRow(_design.triggers, (i) => _selTrigger = i),
          onRemove: () => _removeRow(_design.triggers, () => _selTrigger, (i) => _selTrigger = i),
        ));
      default:
        return const SizedBox.shrink();
    }
    final row = Row(mainAxisSize: MainAxisSize.min, children: actions);
    if (_actionsEnabled) return row;
    // 只读降级 / 不支持编辑的标签:整组动作置灰不可点
    return IgnorePointer(
      ignoring: true,
      child: Opacity(opacity: 0.45, child: row),
    );
  }

  /// 工具栏当前标签的补充说明(图标栏放不下文字,统一挪到状态条)
  String? _toolbarHint(AppPalette t) {
    // 排除 / 规则 / 触发器仅 PostgreSQL 家族生成 DDL,其它类型提示但不拦截
    if ((_tabIndex == 5 || _tabIndex == 6 || _tabIndex == 7) &&
        !DdlBuilder.isPgLike(_typeId)) {
      return '仅 PostgreSQL 支持该功能,其它类型保存时忽略';
    }
    // 编辑已有表时上移 / 下移置灰(PG / SQL Server 无对应 ALTER)
    if (_tabIndex == 0 && !_canReorderColumns) {
      return '调整已有列的顺序仅 MySQL / MariaDB 支持';
    }
    return null;
  }

  /// 底部状态条:错误 / 校验提示 / 上次保存结果 / 当前标签说明 + 变更状态。
  /// 整块挂在 [_chromeRev] 上:校验结果与「已修改 / 无变更」都随敲字变化,
  /// 但不该为此重建整页(实测整页重建一次 ≈ 130ms)。
  Widget _statusBar(AppPalette t) {
    return Container(
      height: 26,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      decoration: BoxDecoration(
        color: t.statusBar,
        border: Border(top: BorderSide(color: t.border)),
      ),
      child: ValueListenableBuilder(
        valueListenable: _chromeRev,
        builder: (context, _, __) {
          final validation = _validation;
          // 加载期间字段为空并非「设计无效」,不抢提示位
          final showInvalid =
              !validation.ok && !_loading && _loadError == null;
          // 工具栏改纯图标后,原来挂在按钮后面的说明文字统一挪到这里
          final hint = showInvalid || _error != null || _note != null
              ? null
              : (!_isEdit && isUntitledTableName(_design.name)
                  ? '未命名表:点「保存」时会要求输入表名'
                  : _toolbarHint(t));
          return Row(
            children: [
              if (_error != null)
                Expanded(
                  child: Text(
                    _error!,
                    style: const TextStyle(
                        fontSize: 12, color: Color(0xFFDC2626)),
                    overflow: TextOverflow.ellipsis,
                  ),
                )
              else if (showInvalid)
                Expanded(
                  child: Text(
                    validation.error ?? '设计数据不完整',
                    style:
                        TextStyle(fontSize: 12, color: t.disabledForeground),
                    overflow: TextOverflow.ellipsis,
                  ),
                )
              else if (_note != null)
                Expanded(
                  child: Text(
                    _note!,
                    style: TextStyle(fontSize: 12, color: t.mutedForeground),
                    overflow: TextOverflow.ellipsis,
                  ),
                )
              else if (hint != null)
                Expanded(
                  child: Text(
                    hint,
                    style:
                        TextStyle(fontSize: 12, color: t.disabledForeground),
                    overflow: TextOverflow.ellipsis,
                  ),
                )
              else
                const Spacer(),
              // 变更状态指示(仅编辑模式):只读 / 已修改 / 无变更
              if (_isEdit && !_loading && _loadError == null) ...[
                Text(
                  _readOnly ? '只读' : (_dirty ? '已修改' : '无变更'),
                  style: TextStyle(
                    fontSize: 12,
                    color: _dirty ? t.accent : t.disabledForeground,
                  ),
                ),
                const SizedBox(width: 10),
              ],
              if (_saving) ...[
                const Spinner(size: 12),
                const SizedBox(width: 8),
                Text(
                  _isEdit ? '正在保存变更 ...' : '正在创建表 ...',
                  style: TextStyle(fontSize: 12, color: t.mutedForeground),
                ),
              ],
            ],
          );
        },
      ),
    );
  }

  // ── 字段标签 ──────────────────────────────────────────────

  Widget _fieldsTab(AppPalette t) {
    final cols = _design.columns;
    // 主键序号与表级 PRIMARY KEY 列序同一来源,保证 🔑n 与 SQL 一致
    final ordinals = DdlBuilder.pkOrdinals(_design);
    return _editorGrid(
      t,
      headers: const ['名称', '类型', '长度', '小数点', '不是 null', '键', '注释'],
      widths: _fieldWidths,
      rowCount: cols.length,
      anchor: () => cols.isEmpty ? -1 : _selColumn,
      selectedRows: () => _selColumns,
      onSelect: _selectColumnAt,
      onContextMenu: _columnContextMenu,
      cells: (i) => _fieldRow(t, cols[i], i, ordinals[cols[i]]),
      propsPanel: _props(
        t,
        () {
          if (cols.isEmpty) return null;
          return cols[_selColumn.clamp(0, cols.length - 1)];
        },
        (c) => _columnProps(t, c),
        hint: _identityHintText(),
      ),
    );
  }

  /// 字段页底部属性面板行(顺序与截图一致:
  /// 默认 / 排序规则 / 维度 / 虚拟类型 / 递增 / 最小 / 最大 / 开始值 / 缓存 / 循环)
  List<Widget> _columnProps(AppPalette t, DesignColumn c) {
    final isPg = DdlBuilder.isPgLike(_typeId);
    final isMysql = DdlBuilder.isMysqlLike(_typeId);
    final isSqlServer = DdlBuilder.isSqlServerLike(_typeId);
    final identityOn = c.hasIdentity;
    final rows = <Widget>[
      _propField(
        t,
        '默认',
        ComboBox<String>(
          items: _defaultOptions,
          value: c.defaultValue,
          editable: true,
          enabled: !_readOnly,
          hint: '字符串自动加引号',
          onChanged: (v) {
              c.defaultValue = v ?? '';
              _touchCell(_selColumn, propsAlso: true);
            },
        ),
      ),
      _propField(
        t,
        '排序规则',
        // 两个下拉并排(实测 200 + 204,紧挨着不留缝):左为排序规则名
        // (仅字符类型可用),右为占位(PG 无对应语法,不参与 DDL)
        Row(
          children: [
            SizedBox(
              width: 200,
              child: ComboBox<String>(
                items: _collationChoices,
                value: c.collation,
                editable: true,
                enabled: _isTextType(c.type) && !_readOnly,
                onChanged: (v) {
                  c.collation = v ?? '';
                  _touchCell(_selColumn, propsAlso: true);
                },
              ),
            ),
            SizedBox(width: 204, child: _disabledInput),
          ],
        ),
        fieldWidth: 404,
      ),
      _propField(
        t,
        '维度',
        _modelInput(c.dimension, (v) => c.dimension = v,
            enabled: isPg, textAlign: TextAlign.right, keyboardType: TextInputType.number),
      ),
    ];
    if (isMysql) {
      // MySQL / MariaDB 无 IDENTITY 概念:保留「自增」复选框(AUTO_INCREMENT)
      rows.add(_propField(
        t,
        '自增',
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            CheckBox(
              value: c.autoIncrement,
              size: _kCheckSize,
              enabled: !_readOnly,
              onChanged: (v) {
                c.autoIncrement = v ?? false;
                _touchCell(_selColumn, propsAlso: true);
              },
            ),
            const SizedBox(width: 6),
            Text('AUTO_INCREMENT', style: TextStyle(fontSize: 12, color: t.mutedForeground)),
          ],
        ),
      ));
      return rows;
    }
    rows.add(_propField(
      t,
      isSqlServer ? '标识' : '虚拟类型',
      ComboBox<String>(
        items: _identityOptions,
        value: _identityLabelOf(c.identityMode),
        // SQLite / Access 无自增 / 标识列的统一写法,禁用并提示
        enabled: (isPg || isSqlServer) && !_readOnly,
        onChanged: (v) {
            _applyIdentity(c, _identityModeOf(v ?? '无'));
            // 虚拟类型直接决定面板自身要不要展开「序列」那一组属性
            _touchCell(_selColumn, propsAlso: true);
          },
      ),
    ));
    Widget num(String label, String value, ValueChanged<String> set) =>
        _propField(
          t,
          label,
          _modelInput(value, set,
              enabled: identityOn,
              textAlign: TextAlign.right,
              keyboardType: TextInputType.number),
        );
    rows.add(num('递增', c.identityIncrement, (v) => c.identityIncrement = v));
    rows.add(num('开始值', c.identityStart, (v) => c.identityStart = v));
    if (isPg) {
      rows.add(num('最小', c.identityMinValue, (v) => c.identityMinValue = v));
      rows.add(num('最大', c.identityMaxValue, (v) => c.identityMaxValue = v));
      rows.add(num('缓存', c.identityCache, (v) => c.identityCache = v));
      rows.add(_propField(
        t,
        '循环',
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            CheckBox(
              value: c.identityCycle,
              size: _kCheckSize,
              enabled: identityOn && !_readOnly,
              onChanged: (v) {
                c.identityCycle = v ?? false;
                _touchCell(_selColumn, propsAlso: true);
              },
            ),
            const SizedBox(width: 6),
            Text('CYCLE', style: TextStyle(fontSize: 12, color: t.mutedForeground)),
          ],
        ),
      ));
    }
    return rows;
  }

  /// 属性面板尾部的方言提示(无提示时返回 null,面板不占行)
  String? _identityHintText() {
    if (DdlBuilder.isPgLike(_typeId)) return null;
    if (DdlBuilder.isMysqlLike(_typeId)) return 'MySQL / MariaDB 自增:生成 AUTO_INCREMENT。';
    if (DdlBuilder.isSqlServerLike(_typeId)) return 'SQL Server 标识列:生成 IDENTITY(开始值,递增)。';
    return '当前类型不支持自增 / 标识列,这些属性不参与 DDL 生成。';
  }

  /// 字符类类型(决定「排序规则」是否可编辑)
  static bool _isTextType(String type) => const {
        'char',
        'varchar',
        'bpchar',
        'character',
        'text',
        'tinytext',
        'mediumtext',
        'longtext',
        'citext',
        'nvarchar',
        'nchar',
        'enum',
        'set',
      }.contains(type.trim().toLowerCase());

  static String _identityLabelOf(String mode) => switch (mode) {
        'ALWAYS' => 'GENERATED ALWAYS AS IDENTITY',
        'BY DEFAULT' => 'GENERATED BY DEFAULT AS IDENTITY',
        _ => '无',
      };

  static String _identityModeOf(String label) => switch (label) {
        'GENERATED ALWAYS AS IDENTITY' => 'ALWAYS',
        'GENERATED BY DEFAULT AS IDENTITY' => 'BY DEFAULT',
        _ => '',
      };

  /// 类型变更:若「最大」仍是旧类型的自动上限,跟随新类型更新(不覆盖用户手输值)
  void _onColumnTypeChanged(DesignColumn c, String newType) {
    final old = c.type;
    c.type = newType;
    if (c.hasIdentity &&
        c.identityMaxValue.isNotEmpty &&
        c.identityMaxValue == kIdentityMaxValue(old)) {
      c.identityMaxValue = kIdentityMaxValue(newType);
    }
  }

  /// 「字段」一行:名称 / 类型 / 长度 / 小数点 / 不是 null / 键 / 注释。
  ///
  /// 只交内容出去,宽度与网格线由 [_cell] 统一包(见 [_editorGrid])。
  /// 数值格(长度 / 小数点)**左对齐**:参考工具的设计器不右对齐数字
  /// (右对齐是数据浏览页的规则,那里是查询结果,不是表单)。
  List<_GridCell> _fieldRow(AppPalette t, DesignColumn c, int index, int? pkOrdinal) {
    // 无长度参数的整数类型:长度格显示位宽(仅信息展示,禁用编辑)
    final bit = bitWidthOf(c.type);
    return [
      _GridCell(_modelInput(c.name, (v) => c.name = v,
          hint: '列名', flat: true, row: index)),
      _GridCell(ComboBox<String>(
        items: _typeOptions,
        value: c.type.isEmpty ? null : c.type,
        editable: true,
        enabled: !_readOnly,
        showBorder: false,
        hint: '类型',
        onChanged: (v) {
          _onColumnTypeChanged(c, v ?? '');
          // 类型变了:本行的「长度 / 小数点」可用性、面板的「排序规则」都要跟着变
          _touchCell(index, propsAlso: true);
        },
      )),
      _GridCell(_modelInput(
        bit ?? c.length,
        (v) => c.length = v,
        hint: '长度',
        enabled: bit == null,
        keyboardType: TextInputType.number,
        flat: true,
        row: index,
      )),
      _GridCell(_modelInput(
        c.decimal,
        (v) => c.decimal = v,
        hint: '小数点',
        enabled: bit == null,
        keyboardType: TextInputType.number,
        flat: true,
        row: index,
      )),
      _GridCell(
        CheckBox(
          value: c.notNull,
          // 方框恒 13:不随行高缩到 10,否则参考工具里那颗醒目的勾选块会变小
          size: _kCheckSize,
          // IDENTITY 列必须 NOT NULL(由 _applyIdentity 保证),不再开放编辑
          enabled: !(c.hasIdentity && DdlBuilder.isPgLike(_typeId)) && !_readOnly,
          onChanged: (v) {
            c.notNull = v ?? false;
            _touchCell(index);
          },
        ),
        center: true,
      ),
      // 「键」格:按下即切换主键(零延迟);主键行显示 钥匙 + 序号
      _GridCell(
        MouseRegion(
          cursor: SystemMouseCursors.click,
          child: Listener(
            onPointerDown: (_) {
              if (!_readOnly) _togglePkAt(index);
            },
            behavior: HitTestBehavior.opaque,
            child: c.primaryKey
                ? Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.key, size: 14, color: _iconKey),
                      const SizedBox(width: 2),
                      Text(
                        '${pkOrdinal ?? 1}',
                        style: TextStyle(
                          fontSize: 12,
                          color: t.foreground,
                          decoration: TextDecoration.none,
                          fontWeight: FontWeight.w400,
                        ),
                      ),
                    ],
                  )
                : const SizedBox.shrink(),
          ),
        ),
        center: true,
      ),
      _GridCell(_modelInput(c.comment, (v) => c.comment = v,
          hint: '注释', flat: true, row: index)),
    ];
  }

  // ── 索引标签 ──────────────────────────────────────────────

  Widget _indexTab(AppPalette t) {
    final items = _design.indexes;
    return _editorGrid(
      t,
      headers: const ['名称', '字段', '索引方法', '唯一键', '并发', '注释'],
      widths: const [140, 150, 110, 64, 64, 180],
      rowCount: items.length,
      anchor: () => _selIndex,
      onSelect: (i, ctrl, shift) => setState(() => _selIndex = i),
      cells: (i) => _indexRow(t, items[i], i),
      propsPanel: _props(
        t,
        () => _selIndex >= 0 && _selIndex < items.length
            ? items[_selIndex]
            : null,
        (it) => [
          _propField(t, '表空间',
              _candidateCombo(_tablespaces, it.tablespace,
                  (v) => it.tablespace = v, '表空间名',
                  enabled: DdlBuilder.isPgLike(_typeId))),
          _propField(t, '填充因子 (%)', _modelInput(it.fillFactor, (v) => it.fillFactor = v,
              enabled: DdlBuilder.isPgLike(_typeId) || DdlBuilder.isSqlServerLike(_typeId))),
          _propField(t, '正在缓冲', _disabledInput),
          _propField(t, '快速更新', _disabledInput),
          _propField(t, '待处理列表限制', _disabledInput),
          _propField(t, '每范围页数', _disabledInput),
          // 约束名 = 该索引背后的主键 / 唯一约束:由 CREATE TABLE / ALTER ADD CONSTRAINT
          // 生成,此处仅展示,开放编辑会得到两条互相冲突的语句
          _propField(t, '约束', _readonlyInput(_backingConstraintName(it))),
        ],
      ),
    );
  }

  /// 索引对应的约束名(主键 / 唯一键同名约束);无则留空
  String _backingConstraintName(DesignIndex idx) {
    final key = idx.name.trim().toLowerCase();
    if (key.isEmpty) return '';
    final pk = _design.pkName.trim();
    if (pk.toLowerCase() == key) return pk;
    for (final uk in _design.uniqueKeys) {
      if (uk.name.trim().toLowerCase() == key) return uk.name.trim();
    }
    return '';
  }

  /// 打开「选择数据表字段」弹窗,确认后写回字段项并同步逗号串。
  ///
  /// [target] 是宿主对象(索引 / 唯一键 / 排除)持有的字段项列表,原地替换;
  /// [apply] 让宿主把项回写成网格显示的逗号串。索引与唯一键用同一列集,
  /// 排除约束的「元素」多两列运算符([forExclude])。
  Future<void> _pickColumnItems(
    List<DesignIndexColumn> target,
    VoidCallback apply, {
    bool forExclude = false,
  }) async {
    await _ensureCandidates();
    final picked = await IndexColumnPickerDialog.show(
      context,
      fields: [
        for (final c in _design.columns) c.name.trim(),
      ].where((e) => e.isNotEmpty).toList(),
      selected: target,
      schemaCandidates: _schemas,
      collationCandidates: _collationChoices,
      opClassCandidates: _opClasses,
      operatorCandidates: _excludeOperators,
      forExclude: forExclude,
    );
    if (picked == null) return;
    setState(() {
      target
        ..clear()
        ..addAll(picked);
      apply();
    });
  }

  Future<void> _pickIndexColumns(DesignIndex idx) =>
      _pickColumnItems(idx.columnItems, idx.applyColumnItems);

  Future<void> _pickUniqueColumns(DesignUniqueKey uk) =>
      _pickColumnItems(uk.columnItems, uk.applyColumnItems);

  Future<void> _pickExcludeElements(DesignExclude ex) =>
      _pickColumnItems(ex.columnItems, ex.applyColumnItems, forExclude: true);

  /// 「字段 / 元素」格:可直填逗号串,也可点 `...` 逐项设选项
  Widget _columnCell(String value, ValueChanged<String> set, int row,
          String hint, VoidCallback pick) =>
      Row(
        children: [
          Expanded(
            child: _modelInput(value, set, hint: hint, flat: true, row: row),
          ),
          _dotsButton(onTap: _readOnly ? null : pick),
        ],
      );

  List<_GridCell> _indexRow(AppPalette t, DesignIndex it, int row) {
    // 复选框可用性统一收口(只读降级不逐个传参);[dialectOk] 再叠一层方言门,
    // 与 [DdlBuilder.createUnsupported] 的拦口一致:改了不生效的开关直接灰掉。
    Widget box(bool value, ValueChanged<bool> set, {bool dialectOk = true}) => CheckBox(
        value: value,
        size: _kCheckSize,
        enabled: !_readOnly && dialectOk,
        onChanged: (v) {
          set(v ?? false);
          _touchCell(row);
        });
    final isPg = DdlBuilder.isPgLike(_typeId);
    return [
      _GridCell(_modelInput(it.name, (v) => it.name = v,
          hint: '索引名', flat: true, row: row)),
      // 字段格:可直接输逗号串,也可点 `...` 用弹窗逐字段设选项
      _GridCell(_columnCell(it.columns, (v) => it.columns = v, row,
          '多列用逗号分隔', () => _pickIndexColumns(it))),
      _GridCell(ComboBox<String>(
        items: _indexMethods,
        value: it.method.isEmpty ? null : it.method,
        enabled: !_readOnly,
        showBorder: false,
        onChanged: (v) {
          it.method = v ?? '';
          _touchCell(row);
        },
      )),
      _GridCell(box(it.unique, (v) => it.unique = v), center: true),
      // CONCURRENTLY 是 PostgreSQL 专属
      _GridCell(box(it.concurrent, (v) => it.concurrent = v, dialectOk: isPg),
          center: true),
      // 注释:PG 走 COMMENT ON INDEX,MySQL 内联进 CREATE INDEX,SQL Server
      // 的扩展属性不覆盖索引层次 → 那一型上填了必然丢,直接灰掉
      _GridCell(_modelInput(it.comment, (v) => it.comment = v,
          hint: '注释', flat: true, row: row,
          // _modelInput 内部已并上 !_readOnly,这里只表达方言门
          enabled: DdlBuilder.isPgLike(_typeId) || DdlBuilder.isMysqlLike(_typeId))),
    ];
  }

  // ── 外键标签 ──────────────────────────────────────────────

  Widget _fkTab(AppPalette t) {
    final items = _design.foreignKeys;
    return _editorGrid(
      t,
      headers: const ['名称', '字段', '被引用的模式', '被引用的表（父）', '被引用的字段', '删除时', '更新时', '注释'],
      widths: const [110, 100, 100, 120, 100, 96, 96, 180],
      rowCount: items.length,
      anchor: () => _selFk,
      onSelect: (i, ctrl, shift) => setState(() => _selFk = i),
      cells: (i) => _fkRow(t, items[i], i),
      propsPanel: _props(
        t,
        () => _selFk >= 0 && _selFk < items.length ? items[_selFk] : null,
        (fk) => [
          _propField(
            t,
            '符合全部',
            CheckBox(
              value: fk.matchAll,
              size: _kCheckSize,
              // MATCH FULL 只有 PG / MySQL 会输出(SQL Server 无此子句)
              enabled: !_readOnly &&
                  (DdlBuilder.isPgLike(_typeId) || DdlBuilder.isMysqlLike(_typeId)),
              onChanged: (v) {
                fk.matchAll = v ?? false;
                _touchChrome();
              },
            ),
            fieldWidth: 24,
          ),
          _propField(t, '可延迟', _yesNoCombo(fk.deferrable, (v) => fk.deferrable = v)),
          _propField(t, '延迟', _yesNoCombo(fk.deferred, (v) => fk.deferred = v)),
        ],
      ),
    );
  }

  List<_GridCell> _fkRow(AppPalette t, DesignForeignKey fk, int row) {
    return [
      _GridCell(_modelInput(fk.name, (v) => fk.name = v,
          hint: '约束名', flat: true, row: row)),
      _GridCell(_modelInput(fk.columns, (v) => fk.columns = v,
          hint: '本表字段', flat: true, row: row)),
      _GridCell(_modelInput(fk.refSchema, (v) => fk.refSchema = v,
          hint: widget.schema ?? '模式', flat: true, row: row)),
      _GridCell(ComboBox<String>(
        items: _refTables,
        value: fk.refTable.isEmpty ? null : fk.refTable,
        editable: true,
        enabled: !_readOnly,
        showBorder: false,
        hint: '表名',
        onChanged: (v) {
          fk.refTable = v ?? '';
          _touchCell(row);
        },
      )),
      _GridCell(_modelInput(fk.refColumns, (v) => fk.refColumns = v,
          hint: '如 id', flat: true, row: row)),
      _GridCell(ComboBox<String>(
        items: _fkActions,
        value: fk.onDelete,
        enabled: !_readOnly,
        showBorder: false,
        onChanged: (v) {
          fk.onDelete = v ?? 'NO ACTION';
          _touchCell(row);
        },
      )),
      _GridCell(ComboBox<String>(
        items: _fkActions,
        value: fk.onUpdate,
        enabled: !_readOnly,
        showBorder: false,
        onChanged: (v) {
          fk.onUpdate = v ?? 'NO ACTION';
          _touchCell(row);
        },
      )),
      _GridCell(_modelInput(fk.comment, (v) => fk.comment = v,
          hint: '注释', flat: true, row: row,
          enabled: DdlBuilder.isPgLike(_typeId))),
    ];
  }

  // ── 唯一键标签 ────────────────────────────────────────────

  Widget _uniqueTab(AppPalette t) {
    final items = _design.uniqueKeys;
    return _editorGrid(
      t,
      headers: const ['名称', '字段', '注释'],
      widths: const [160, 200, 240],
      rowCount: items.length,
      anchor: () => _selUnique,
      onSelect: (i, ctrl, shift) => setState(() => _selUnique = i),
      cells: (i) => _uniqueRow(t, items[i], i),
      propsPanel: _props(
        t,
        () =>
            _selUnique >= 0 && _selUnique < items.length ? items[_selUnique] : null,
        (uk) => [
          _propField(t, '表空间',
              _candidateCombo(_tablespaces, uk.tablespace,
                  (v) => uk.tablespace = v, '表空间名',
                  enabled: DdlBuilder.isPgLike(_typeId))),
          _propField(t, '填充因子 (%)', _modelInput(uk.fillFactor, (v) => uk.fillFactor = v,
              enabled: DdlBuilder.isPgLike(_typeId))),
          _propField(t, '可延迟', _yesNoCombo(uk.deferrable, (v) => uk.deferrable = v)),
          _propField(t, '延迟', _yesNoCombo(uk.deferred, (v) => uk.deferred = v)),
        ],
      ),
    );
  }

  List<_GridCell> _uniqueRow(AppPalette t, DesignUniqueKey uk, int row) {
    return [
      _GridCell(_modelInput(uk.name, (v) => uk.name = v,
          hint: '约束名', flat: true, row: row)),
      _GridCell(_columnCell(uk.columns, (v) => uk.columns = v, row,
          '多列用逗号分隔', () => _pickUniqueColumns(uk))),
      _GridCell(_modelInput(uk.comment, (v) => uk.comment = v,
          hint: '注释', flat: true, row: row,
          enabled: DdlBuilder.isPgLike(_typeId))),
    ];
  }

  // ── 检查标签 ──────────────────────────────────────────────

  Widget _checkTab(AppPalette t) {
    final items = _design.checks;
    return _editorGrid(
      t,
      headers: const ['名称', '表达式', '无继承', '注释'],
      widths: const [160, 300, 60, 220],
      rowCount: items.length,
      anchor: () => _selCheck,
      onSelect: (i, ctrl, shift) => setState(() => _selCheck = i),
      cells: (i) => _checkRow(t, items[i], i),
      propsPanel: null,
    );
  }

  List<_GridCell> _checkRow(AppPalette t, DesignCheck ck, int row) {
    return [
      _GridCell(_modelInput(ck.name, (v) => ck.name = v,
          hint: '约束名', flat: true, row: row)),
      // 表达式往往跨几行(如 CHECK (a > 0 AND b <> a)),格子里塞不下 → `...` 开大弹窗
      _GridCell(Row(
        children: [
          Expanded(
            child: _modelInput(ck.expression, (v) => ck.expression = v,
                hint: '如 age > 0', flat: true, row: row),
          ),
          _dotsButton(onTap: _readOnly ? null : () => _editCheckExpression(ck)),
        ],
      )),
      _GridCell(CheckBox(
          value: ck.noInherit,
          size: _kCheckSize,
          // NO INHERIT 是 PG 语法,其余方言勾了也不生成 → 直接灰掉
          enabled: !_readOnly && DdlBuilder.isPgLike(_typeId),
          onChanged: (v) {
            ck.noInherit = v ?? false;
            _touchCell(row);
          }),
          center: true),
      _GridCell(_modelInput(ck.comment, (v) => ck.comment = v,
          hint: '注释', flat: true, row: row,
          enabled: DdlBuilder.isPgLike(_typeId))),
    ];
  }

  Future<void> _editCheckExpression(DesignCheck ck) async {
    final text = await _MultilineEditorDialog.show(
      context,
      title: '检查表达式',
      value: ck.expression,
      hint: '如 age > 0 AND age < 130(不含 CHECK 关键字)',
    );
    if (text == null || text == ck.expression) return;
    setState(() => ck.expression = text);
  }

  // ── 排除标签(仅 PostgreSQL) ──────────────────────────────

  Widget _excludeTab(AppPalette t) {
    final items = _design.excludes;
    return _editorGrid(
      t,
      headers: const ['名称', '索引方法', '元素', '注释'],
      widths: const [150, 110, 280, 180],
      rowCount: items.length,
      anchor: () => _selExclude,
      onSelect: (i, ctrl, shift) => setState(() => _selExclude = i),
      cells: (i) => _excludeRow(t, items[i], i),
      propsPanel: _props(
        t,
        () => _selExclude >= 0 && _selExclude < items.length
            ? items[_selExclude]
            : null,
        (ex) => [
          _propField(
            t,
            '排除条件 (WHERE)',
            _modelInput(
              ex.predicate,
              (v) => ex.predicate = v,
              hint: "部分排除,如 state = 'active'",
            ),
          ),
          _propField(t, '可延迟', _yesNoCombo(ex.deferrable, (v) => ex.deferrable = v)),
          _propField(t, '延迟', _yesNoCombo(ex.deferred, (v) => ex.deferred = v)),
        ],
        hint: '「元素」按 PostgreSQL 原样进括号,每个元素的比较操作符写在同一格:'
            'col WITH = / int4range(a, b) WITH &&;点 `...` 可逐字段配运算符与排序。',
      ),
    );
  }

  List<_GridCell> _excludeRow(AppPalette t, DesignExclude ex, int row) {
    return [
      _GridCell(_modelInput(ex.name, (v) => ex.name = v,
          hint: '约束名', flat: true, row: row)),
      _GridCell(ComboBox<String>(
        items: _excludeMethods,
        value: ex.method,
        enabled: !_readOnly,
        showBorder: false,
        onChanged: (v) {
          ex.method = v ?? 'gist';
          _touchCell(row);
        },
      )),
      // 元素格:`col WITH =` 可直接输,也可点 `...` 用弹窗逐字段配运算符 / 排序
      _GridCell(_columnCell(ex.columns, (v) => ex.columns = v, row,
          '如 col WITH =', () => _pickExcludeElements(ex))),
      _GridCell(_modelInput(ex.comment, (v) => ex.comment = v,
          hint: '注释', flat: true, row: row,
          enabled: DdlBuilder.isPgLike(_typeId))),
    ];
  }

  // ── 规则标签(仅 PostgreSQL) ──────────────────────────────

  Widget _ruleTab(AppPalette t) {
    final items = _design.rules;
    return _editorGrid(
      t,
      headers: const ['名称', '事件', '代替运行', '启用', '注释'],
      widths: const [150, 100, 100, 56, 200],
      rowCount: items.length,
      anchor: () => _selRule,
      onSelect: (i, ctrl, shift) => setState(() => _selRule = i),
      cells: (i) => _ruleRow(t, items[i], i),
      propsPanel: _props(
        t,
        () => _selRule >= 0 && _selRule < items.length
            ? items[_selRule]
            : null,
        (r) => [
          // 语句体跨行是常态(INSERT INTO … VALUES (…) 后面还跟分号),
          // 网格那一格放不下,参考工具也把它放在属性区。
          _propField(
            t,
            '语句',
            _ModelTextarea(
              value: r.statement,
              hint: 'DO 后括号里的语句,如 INSERT INTO log VALUES (NEW.id)',
              enabled: !_readOnly,
              onChanged: (v) {
                r.statement = v;
                _touchChrome();
              },
            ),
            height: 78,
            expandField: true,
          ),
          _propField(
            t,
            '限定条件 (WHERE)',
            _modelInput(
              r.qualification,
              (v) => r.qualification = v,
              hint: '如 NEW.amount > 1000',
            ),
          ),
        ],
        hint: 'SELECT 规则只能 DO INSTEAD(PG 语法限制);代替运行选 NOTHING 时不生成语句体。',
      ),
    );
  }

  List<_GridCell> _ruleRow(AppPalette t, DesignRule r, int row) {
    return [
      _GridCell(
          _modelInput(r.name, (v) => r.name = v, hint: '规则名', flat: true, row: row)),
      _GridCell(ComboBox<String>(
        items: _ruleEvents,
        value: r.event,
        enabled: !_readOnly,
        showBorder: false,
        onChanged: (v) {
          r.event = v ?? 'INSERT';
          // SELECT 规则只能 DO INSTEAD(PG 语法限制):把模型一起扳过来,
          // 否则网格里显示「ALSO」而生成的 SQL 写「INSTEAD」,两者对不上。
          if (r.event == 'SELECT') r.action = 'INSTEAD';
          _touchCell(row, propsAlso: true);
        },
      )),
      _GridCell(ComboBox<String>(
        items: _ruleActions,
        value: r.action,
        enabled: !_readOnly && r.event != 'SELECT',
        showBorder: false,
        onChanged: (v) {
          r.action = v ?? 'ALSO';
          _touchCell(row, propsAlso: true);
        },
      )),
      _GridCell(CheckBox(
          value: r.enable,
          size: _kCheckSize,
          enabled: !_readOnly,
          onChanged: (v) {
            r.enable = v ?? true;
            _touchCell(row);
          }),
          center: true),
      _GridCell(_modelInput(r.comment, (v) => r.comment = v,
          hint: '注释', flat: true, row: row)),
    ];
  }

  // ── 触发器标签 ────────────────────────────────────────────

  Widget _triggerTab(AppPalette t) {
    final items = _design.triggers;
    return _editorGrid(
      t,
      headers: const ['名称', '给每个', '触发', '插入', '更新', '删除', '截断', '更新字段', '启用', '注释'],
      widths: const [110, 70, 90, 44, 44, 44, 44, 110, 44, 140],
      rowCount: items.length,
      anchor: () => _selTrigger,
      onSelect: (i, ctrl, shift) => setState(() => _selTrigger = i),
      cells: (i) => _triggerRow(t, items[i], i),
      propsPanel: _props(
        t,
        () => _selTrigger >= 0 && _selTrigger < items.length
            ? items[_selTrigger]
            : null,
        (tr) => [
          // 属性面板内再分「常规 / 约束」两个子页(与参考工具一致)
          TabControl(
            initialIndex: _triggerPropsTab,
            tabBarColor: t.surface,
            selectedTabColor: t.background,
            hoverTabColor: t.secondary,
            contentPadding: EdgeInsets.zero,
            onChanged: (i) => setState(() => _triggerPropsTab = i),
            tabs: const [TabItem(label: '常规'), TabItem(label: '约束')],
          ),
          ...(_triggerPropsTab == 0
              ? _triggerGeneralProps(t, tr)
              : _triggerConstraintProps(t, tr)),
        ],
      ),
    );
  }

  /// 触发器「常规」子页。
  ///
  /// PostgreSQL:WHEN 条件 / 触发函数(模式 + 函数名)/ 参数。
  /// MySQL 与 SQL Server:没有独立函数,过程体直接写在触发器里 → 换成多行
  /// 「触发主体」输入区;此时画「触发函数」会得到一个填了也不生成任何语句的
  /// 死控件,故整组不出现。
  List<Widget> _triggerGeneralProps(AppPalette t, DesignTrigger tr) {
    if (!_triggerUsesFunction) {
      return [
        _propField(
          t,
          '触发主体',
          _ModelTextarea(
            value: tr.body,
            hint: DdlBuilder.isMysqlLike(_typeId)
                ? '如 SET NEW.updated_at = NOW()(多条语句各占一行)'
                : '如 UPDATE audit SET n = n + 1 FROM inserted',
            enabled: !_readOnly,
            onChanged: (v) {
              tr.body = v;
              _touchChrome();
            },
          ),
          height: 78,
          expandField: true,
        ),
        _propField(t, '当', _modelInput(tr.when, (v) => tr.when = v, hint: 'WHEN 条件',
            // WHEN 子句是 PG 专属:MySQL / SQL Server 里条件写进主体的 IF
            enabled: !_readOnly && DdlBuilder.isPgLike(_typeId))),
      ];
    }
    final fn = tr.function.trim();
    final dot = fn.lastIndexOf('.');
    final fnSchema = dot < 0 ? '' : fn.substring(0, dot);
    final fnName = dot < 0 ? fn : fn.substring(dot + 1);
    return [
      _propField(t, '当', _modelInput(tr.when, (v) => tr.when = v, hint: 'WHEN 条件')),
      _propField(
        t,
        '触发函数',
        Row(
          children: [
            SizedBox(
              width: 172,
              child: ComboBox<String>(
                items: _schemas,
                value: fnSchema.isEmpty ? null : fnSchema,
                editable: true,
                enabled: !_readOnly,
                hint: '模式',
                onChanged: (v) =>
                    setState(() => tr.function = _joinQualified(v ?? '', fnName)),
              ),
            ),
            const SizedBox(width: 6),
            SizedBox(
              width: 182,
              child: ComboBox<String>(
                items: _functions,
                value: fnName.isEmpty ? null : fnName,
                editable: true,
                enabled: !_readOnly,
                hint: '函数名',
                onChanged: (v) =>
                    setState(() => tr.function = _joinQualified(fnSchema, v ?? '')),
              ),
            ),
          ],
        ),
        // 两个下拉合计 172 + 6 + 182 = 360,比默认字段宽(300)宽一档
        fieldWidth: 360,
      ),
      _propField(t, '参数', _modelInput(tr.parameters, (v) => tr.parameters = v, hint: '如 NEW.id')),
    ];
  }

  /// 触发器「约束」子页:可延迟 / 延迟(仅 PostgreSQL 生成 CREATE CONSTRAINT TRIGGER)
  List<Widget> _triggerConstraintProps(AppPalette t, DesignTrigger tr) {
    final isPg = DdlBuilder.isPgLike(_typeId);
    final deferredEnabled = isPg && tr.deferrable.trim().toUpperCase() == 'YES';
    return [
      _propField(t, '可延迟',
          _yesNoCombo(tr.deferrable, (v) => tr.deferrable = v, enabled: isPg)),
      _propField(t, '延迟',
          _yesNoCombo(tr.deferred, (v) => tr.deferred = v, enabled: deferredEnabled)),
      Padding(
        padding: const EdgeInsets.only(top: 4),
        child: Text(
          isPg
              ? '可延迟的触发器生成 CREATE CONSTRAINT TRIGGER … DEFERRABLE;'
                  '延迟需先置「可延迟 = YES」。'
              : '仅 PostgreSQL 支持约束触发器,其它类型保存时忽略。',
          style: TextStyle(fontSize: 11.5, color: t.disabledForeground),
        ),
      ),
    ];
  }

  /// 模式 + 函数名 合成模型里的限定名(模式为空时只留函数名)
  String _joinQualified(String schema, String name) {
    final s = schema.trim();
    final n = name.trim();
    if (s.isEmpty) return n;
    return n.isEmpty ? s : '$s.$n';
  }

  List<_GridCell> _triggerRow(AppPalette t, DesignTrigger tr, int row) {
    // 复选框可用性统一收口(只读降级不逐个传参);[dialectOk] 再叠一层方言门:
    // 方言用不上的开关一律灰掉,不给「改了不生效」的控件。
    Widget box(bool value, ValueChanged<bool> set,
            {bool fallback = false, bool dialectOk = true}) =>
        CheckBox(
            value: value,
            size: _kCheckSize,
            enabled: !_readOnly && dialectOk,
            onChanged: (v) {
              set(v ?? fallback);
              _touchCell(row);
            });
    final isPg = DdlBuilder.isPgLike(_typeId);
    return [
      _GridCell(_modelInput(tr.name, (v) => tr.name = v,
          hint: '触发器名', flat: true, row: row)),
      _GridCell(ComboBox<String>(
        items: _forEachChoices,
        value: tr.forEach,
        enabled: !_readOnly && _forEachChoices.isNotEmpty,
        showBorder: false,
        onChanged: (v) {
          tr.forEach = v ?? '行';
          _touchCell(row);
        },
      )),
      _GridCell(ComboBox<String>(
        items: _timingChoices,
        value: tr.timing,
        enabled: !_readOnly,
        showBorder: false,
        onChanged: (v) {
          tr.timing = v ?? 'BEFORE';
          _touchCell(row);
        },
      )),
      _GridCell(box(tr.insert, (v) => tr.insert = v), center: true),
      _GridCell(box(tr.update, (v) => tr.update = v), center: true),
      _GridCell(box(tr.delete, (v) => tr.delete = v), center: true),
      // TRUNCATE 事件只有 PostgreSQL 有(MySQL / SQL Server 无此触发点)
      _GridCell(box(tr.truncate, (v) => tr.truncate = v), center: true),
      // UPDATE OF 子句是 PG 专属
      _GridCell(_modelInput(tr.updateColumns, (v) => tr.updateColumns = v,
          hint: '更新字段', flat: true, row: row, enabled: isPg)),
      // DISABLE TRIGGER 是 PG 专属
      _GridCell(box(tr.enable, (v) => tr.enable = v, fallback: true, dialectOk: isPg),
          center: true),
      // 注释:PostgreSQL 没有 COMMENT ON TRIGGER,MySQL / SQL Server 同样没有,
      // 即没有任何方言能把这一格落进 DDL。灰掉并说明,不给「填了会丢」的入口。
      _GridCell(_modelInput(tr.comment, (v) => tr.comment = v,
          hint: '各方言均不支持', flat: true, row: row, enabled: false)),
    ];
  }

  // ── 选项标签 ──────────────────────────────────────────────

  Widget _optionsTab(AppPalette t) {
    final isPg = DdlBuilder.isPgLike(_typeId);
    final isSrv = DdlBuilder.isSqlServerLike(_typeId);
    // 分组按「有没有对应的 ALTER」划,而不是按「是不是新建」划:
    // - 表空间 / 所有者 / 填充因子:PG 能用 ALTER TABLE SET TABLESPACE / OWNER TO /
    //   SET (…) 落地 → 设计表里照改照存。
    // - 不记录 / 继承 / 集群:任何方言都没有对应 ALTER(前两项要重建表),编辑模式
    //   只回显不改写,免得给出一个改了不生效的控件。
    // - SQL Server 的填充因子只在 CREATE TABLE 里认(`WITH (FILLFACTOR = n)`),
    //   改它同样得重建表。
    final createOnly = !_readOnly && !_isEdit;
    final pgCreateOnly = createOnly && isPg;
    final storageOn = !_readOnly && (isPg || (isSrv && !_isEdit));
    final ownerOn = !_readOnly && isPg;
    Widget combo(List<String> items, String value, ValueChanged<String> set, String hint,
            {bool enabled = true}) =>
        ComboBox<String>(
          items: items,
          value: value.isEmpty ? null : value,
          editable: true,
          enabled: enabled,
          hint: hint,
          onChanged: (v) => setState(() => set(v ?? '')),
        );
    return SingleChildScrollView(
      padding: const EdgeInsets.all(12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            height: 28,
            child: CheckBox(
              value: _design.unlogged,
              label: '不记录',
              enabled: pgCreateOnly,
              onChanged: (v) => setState(() => _design.unlogged = v ?? false),
            ),
          ),
          _propField(
            t,
            '所有者',
            combo(_users, _design.owner, (v) => _design.owner = v, '角色名',
                enabled: ownerOn),
          ),
          _propField(
            t,
            '表空间',
            combo(_tablespaces, _design.tablespace, (v) => _design.tablespace = v, '表空间名',
                enabled: storageOn),
          ),
          _propField(
            t,
            '继承自',
            Row(
              children: [
                Expanded(
                  child: _modelInput(_design.inherits, (v) => _design.inherits = v,
                      enabled: pgCreateOnly, hint: '父表,多个用逗号分隔'),
                ),
                const SizedBox(width: 4),
                _dotsButton(tooltip: '选择父表', onTap: pgCreateOnly ? _pickInherits : null),
              ],
            ),
          ),
          _propField(
            t,
            '填充因子 (%)',
            _modelInput(_design.fillFactor, (v) => _design.fillFactor = v,
                enabled: storageOn, keyboardType: TextInputType.number),
          ),
          _propField(
            t,
            '集群',
            combo(_clusterCandidates, _design.cluster, (v) => _design.cluster = v, '索引名',
                enabled: pgCreateOnly),
          ),
          // 参考工具的「选项」页就这六项,不再往下铺 PG 的全部 reloption:
          // autovacuum_* / parallel_workers 等由反查留在模型里(见 _applyDesign),
          // 生成时原样带进 WITH (…),界面不动它们就不会在保存时被清掉。
          const SizedBox(height: 10),
          Text(
            _isEdit
                ? (isPg
                    ? '「设计表」用 ALTER TABLE SET / RESET 改填充因子,'
                        'SET TABLESPACE 改表空间、OWNER TO 改所有者;'
                        '不记录 / 继承 / 集群没有对应的 ALTER,只回显。'
                        '库里读到的其它存储参数不在本页显示,也原样保留。'
                    : '表空间 / 填充因子没有对应的 ALTER(仅 PostgreSQL 有),已有表不修改。')
                : isPg
                    ? '不记录生成 CREATE UNLOGGED TABLE,继承生成 INHERITS(...),'
                        '填充因子生成 WITH (fillfactor = …),表空间生成 TABLESPACE;'
                        '所有者与集群各补一条 ALTER TABLE … OWNER TO / CLUSTER … USING。'
                        '库里已有的其它存储参数原样保留,不改写。'
                    : '表选项仅 PostgreSQL / SQL Server 部分支持;SQL Server 只认填充因子,其它项对其它类型不生成。',
            style: TextStyle(fontSize: 11.5, color: t.disabledForeground),
          ),
        ],
      ),
    );
  }

  /// 「集群」候选:本表已设计的索引名 + 主键约束名
  List<String> get _clusterCandidates => [
        for (final i in _design.indexes) i.name.trim(),
        _design.pkName.trim(),
      ].where((e) => e.isNotEmpty).toList();

  /// 选父表(排除本表自身;手工输入的不在候选里时并入,以免勾选丢失)
  ///
  /// 候选与回写都用**未加引号**的 `模式.表`(参考工具那样带引号显示,生成时由
  /// [DdlBuilder] 自己加);这也和驱动反查的口径一致——库里读回的「继承自」就是
  /// `nspname.relname`,弹窗里勾一下不该把它变成裸表名、进而被判成「改了继承」。
  Future<void> _pickInherits() async {
    final self = _design.name.trim().toLowerCase();
    final schema = (_design.schema ?? widget.schema ?? '').trim();
    final current = _design.inherits
        .split(',')
        .map((e) => e.trim())
        .where((e) => e.isNotEmpty)
        .toList();
    String lastName(String value) => value.split('.').last.trim().toLowerCase();
    final items = <String>[
      for (final e in _refTables)
        if (e.trim().isNotEmpty && e.trim().toLowerCase() != self)
          if (schema.isEmpty) e.trim() else '$schema.${e.trim()}',
    ];
    for (final c in current) {
      // 并入时按原文比对:模型里的名字一律会被加引号,`PUBLIC.LOGS` 与
      // `public.logs` 是两个不同的表,忽略大小写去重会把用户写的那份弄丢
      if (!items.contains(c) && lastName(c) != self) {
        items.add(c);
      }
    }
    final picked = await ListPickerDialog.show<String>(
      context,
      title: '选择继承的父表',
      items: items,
      selected: current,
      itemToString: _inheritLabel,
      emptyHint: '当前库 / 模式没有其它表,可直接在输入框填写父表名。',
      okText: '确定',
      cancelText: '取消',
    );
    if (picked == null) return;
    setState(() => _design.inherits = picked.join(', '));
  }

  /// 「继承自」弹窗的行文本:逐段加引号的限定名(与生成的 INHERITS 写法一致)
  String _inheritLabel(String value) => value
      .split('.')
      .map((e) => e.trim())
      .where((e) => e.isNotEmpty)
      .map((e) => DdlBuilder.ident(_typeId, e))
      .join('.');

  // ── 注释标签 ──────────────────────────────────────────────

  Widget _commentsTab(AppPalette t) {
    // 注释页整区留给编辑器(与参考工具一致):不加标题与底部说明,避免挤压输入区
    return Padding(
      padding: const EdgeInsets.fromLTRB(2, 2, 2, 2),
      child: Textarea(
        controller: _commentCtrl,
        hint: '输入表的注释…',
        enabled: !_readOnly,
        onChanged: (v) => setState(() => _design.tableComment = v),
        expands: true,
      ),
    );
  }

  // ── SQL 预览标签 ──────────────────────────────────────────

  /// 预览即所见:新建模式输出 CREATE 全套,编辑模式输出待应用的 ALTER。
  /// 编辑模式无差异时给一行说明,不留空白(否则看起来像加载失败)。
  String _previewSql() {
    final base = _original;
    if (!_isEdit || base == null) return DdlBuilder.buildPreview(_design, _typeId);
    // 与「保存」执行的是同一份清单(缓存见 [_alterStatements])
    final alter = _alterStatements;
    return alter.isEmpty ? '-- 没有待应用的变更' : alter.join('\n');
  }

  Widget _sqlTab(AppPalette t) {
    final sql = _previewSql();
    if (_sqlCtrl.text != sql) _sqlCtrl.text = sql;
    final isDark = t.isDark;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: 26,
          padding: const EdgeInsets.symmetric(horizontal: 12),
          alignment: Alignment.centerLeft,
          color: t.secondary,
          child: Text(
            _isEdit
                ? '「保存」将按以下顺序执行这些变更语句(PostgreSQL 在事务中执行,失败自动回滚)'
                : '「保存」将按以下顺序执行这些语句(PostgreSQL 在事务中执行,失败自动回滚)',
            style: TextStyle(fontSize: 11.5, color: t.mutedForeground),
          ),
        ),
        Expanded(
          child: CodeEditor(
            controller: _sqlCtrl,
            readOnly: true,
            findController: _sqlFindController,
            findBuilder: (context, controller, readOnly) =>
                SqlFindBar(controller: controller, readOnly: readOnly),
            shortcutsActivatorsBuilder: const SqlFindShortcuts(),
            wordWrap: false,
            chunkAnalyzer: const NonCodeChunkAnalyzer(),
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
            style: CodeEditorStyle(
              fontFamily: 'monospace',
              fontSize: 13,
              fontHeight: 20 / 13,
              textColor: t.foreground,
              backgroundColor: t.background,
              selectionColor: Color.alphaBlend(
                t.accent.withValues(alpha: 0.30),
                t.background,
              ),
              cursorColor: t.foreground,
              codeTheme: CodeHighlightTheme(
                languages: {'sql': CodeHighlightThemeMode(mode: langSql)},
                theme: sqlEditorTheme(isDark: isDark, foreground: t.foreground),
              ),
            ),
            indicatorBuilder:
                (context, editingController, chunkController, notifier) =>
                    DefaultCodeLineNumber(
              controller: editingController,
              notifier: notifier,
              textStyle: TextStyle(fontSize: 12.5, color: t.disabledForeground),
              focusedTextStyle: TextStyle(fontSize: 12.5, color: t.mutedForeground),
              minNumberCount: 3,
            ),
          ),
        ),
      ],
    );
  }

  // ── 通用网格脚手架 ─────────────────────────────────────────

  /// 1 设备像素的线宽:参考工具是原生窗口,网格线恒为 1 **物理**像素,
  /// 200% 缩放下是 0.5 逻辑像素。用固定 1.0 会画成两倍粗。
  double get _hairline =>
      (TokenScope.maybeOf(context) ?? DesktopTokens.winForm).borderWidth;

  /// 表头 + 行列表 + 底部属性面板(可选)的编辑网格(参考工具同款度量)。
  ///
  /// 结构:行首 [_kGutterW] 宽的「当前行」槽 + 按 [widths] 定宽的列,每格自带
  /// 右 / 下 1px 网格线;表头白底 + 1px 底线(不是灰底表头),行高 [_kRowExtent]。
  /// 网格块总宽 = 行首槽 + Σwidths,**最后一列不跟着窗口拉伸**,右侧余下是空白
  /// 内容底 —— 参考工具就是这样(实测注释列固定 300,右边整片留白)。
  /// 表头与行放在同一个水平滚动容器内,超宽时一起滚动、始终对齐。
  ///
  /// [cells] 返回一行的**单元格内容**:宽度、网格线、当前格反白由 [_cell] 统一包,
  /// 各标签不再各自写 SizedBox 宽度与列间距。
  /// [onSelect] 收到 Ctrl / Shift 修饰键,由调用方决定是否支持多选。
  /// [anchor] / [selectedRows] 传**取值闭包**而不是值:行与表头各自挂在
  /// notifier 上延迟重建,拿到快照值就会显示成上一次选中的样子。
  /// [selectedRows] 非空时命中 ≥2 行才给整行淡蓝底(批量操作要有可见范围),
  /// 单选时只留行首槽与当前格的标记,与参考工具一致。
  /// [onContextMenu] 非空时行右键可弹菜单(命中行 + 光标位置)。
  Widget _editorGrid(
    AppPalette t, {
    required List<String> headers,
    required List<double> widths,
    required int rowCount,
    required List<_GridCell> Function(int row) cells,
    required int Function() anchor,
    required void Function(int index, bool ctrl, bool shift) onSelect,
    Set<int> Function()? selectedRows,
    void Function(int index, Offset position)? onContextMenu,
    Widget? propsPanel,
  }) {
    final line = _hairline;
    final total = _kGutterW + widths.fold(0.0, (a, b) => a + b);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Expanded(
          child: Container(
            color: t.background,
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: SizedBox(
                width: total,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    // 表头只随「当前列」变而变 → 挂 chrome 信号,不跟整页走
                    ValueListenableBuilder(
                      valueListenable: _curColRev,
                      builder: (context, _, __) => SizedBox(
                        height: _kHeaderH,
                        child: Row(
                          children: [
                            _gutterCell(t,
                                line: line,
                                height: _kHeaderH,
                                header: true),
                            for (var i = 0; i < headers.length; i++)
                              _headCell(t, headers[i], widths[i],
                                  line: line, active: i == _curCol),
                          ],
                        ),
                      ),
                    ),
                    // 行列表(垂直滚动);无数据时留白,不显示空态提示。
                    // 每行一层 ValueListenableBuilder:改单元格只重建所在行。
                    Expanded(
                      child: rowCount == 0
                          ? const SizedBox.shrink()
                          : ListView.builder(
                              itemCount: rowCount,
                              itemExtent: _kRowExtent,
                              padding: EdgeInsets.zero,
                              itemBuilder: (context, i) =>
                                  ValueListenableBuilder(
                                valueListenable: _rowRev(i),
                                builder: (context, _, __) {
                                  final a = anchor();
                                  final rows = selectedRows;
                                  return _rowWrap(
                                    t,
                                    i,
                                    selected:
                                        rows == null ? i == a : rows().contains(i),
                                    anchor: i == a,
                                    batch: rows != null && rows().length > 1,
                                    widths: widths,
                                    cells: cells(i),
                                    line: line,
                                    onTap: (ctrl, shift) =>
                                        onSelect(i, ctrl, shift),
                                    onContextMenu: onContextMenu == null
                                        ? null
                                        : (pos) => onContextMenu(i, pos),
                                  );
                                },
                              ),
                            ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
        if (propsPanel != null) ...[
          // 网格与属性面板之间是 3px 的分隔条(参考工具的可拖动缝)
          SizedBox(
            height: _kPropSplitterH,
            child: ColoredBox(color: t.secondary),
          ),
          propsPanel,
        ],
      ],
    );
  }

  /// 表头格:白底 + 右 / 下 1px 结构线 + 5px 左内边距的常规字重黑字;
  /// [active](当前列)额外压一层淡蓝块,块自顶内缩 5、底端是 2px 强调条。
  Widget _headCell(AppPalette t, String title, double width,
      {required double line, required bool active}) {
    return SizedBox(
      width: width,
      child: DecoratedBox(
        decoration: BoxDecoration(
          border: Border(
            right: BorderSide(color: t.border, width: line),
            bottom: BorderSide(color: t.border, width: line),
          ),
        ),
        child: Stack(
          children: [
            if (active)
              Positioned(
                left: 0,
                right: 0,
                top: _kHeaderInsetTop,
                bottom: 0,
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    color: t.accentSoftBg,
                    border: Border(
                      bottom:
                          BorderSide(color: t.highlight, width: _kAccentBarW),
                    ),
                  ),
                ),
              ),
            Positioned.fill(
              child: Padding(
                padding: const EdgeInsets.only(left: _kCellPadX, right: 2),
                child: Align(
                  alignment: Alignment.centerLeft,
                  child: Text(
                    title,
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.w400,
                      color: bodyTextColor(context),
                      decoration: TextDecoration.none,
                    ),
                    overflow: TextOverflow.ellipsis,
                    maxLines: 1,
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// 行首「当前行」槽:当前行是淡蓝底 + 右端 2px 强调条(参考工具没有 ▶ 三角)。
  /// 槽内的横线比数据区深一档(实测 #DCDCDC vs #F0F0F0),照做。
  ///
  /// [height] 必须显式给:Row 给子项的是**无界高度**,DecoratedBox 没有子项时
  /// 会塌成 0 高 —— 那样淡蓝块与右端强调条根本不画,而且整行的 Listener
  /// (deferToChild)在这一列收不到命中,右键落在行首槽上就弹不出菜单。
  Widget _gutterCell(AppPalette t,
      {required double line,
      required double height,
      bool current = false,
      bool header = false}) {
    return SizedBox(
      width: _kGutterW,
      height: height,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: current ? t.accentSoftBg : null,
          border: Border(
            right: BorderSide(color: t.border, width: line),
            bottom: BorderSide(color: t.border, width: line),
          ),
        ),
        child: current
            ? Align(
                alignment: Alignment.centerRight,
                child: ColoredBox(
                  color: t.highlight,
                  child: SizedBox(
                      width: _kAccentBarW,
                      height: height - (header ? 0 : line)),
                ),
              )
            : null,
      ),
    );
  }

  /// 一行:Listener.onPointerDown 零延迟选中(无双击判定、无点击特效)+
  /// 行首槽 + 各列单元格。[batch] 为真时整行铺淡蓝底(多选批次)。
  Widget _rowWrap(AppPalette t, int index,
      {required bool selected,
      required bool anchor,
      required bool batch,
      required List<double> widths,
      required List<_GridCell> cells,
      required void Function(bool ctrl, bool shift) onTap,
      void Function(Offset position)? onContextMenu,
      required double line}) {
    return Listener(
      // opaque:整行是一条通宽带,落在行首槽 / 格与格之间的缝也算点这一行。
      // 默认的 deferToChild 会让 0 高的子项(见 [_gutterCell])把命中漏掉。
      behavior: HitTestBehavior.opaque,
      onPointerDown: (e) {
        if (e.buttons & kSecondaryButton != 0) {
          onContextMenu?.call(e.position);
          return;
        }
        onTap(HardwareKeyboard.instance.isControlPressed,
            HardwareKeyboard.instance.isShiftPressed);
      },
      child: Container(
        height: _kRowExtent,
        color: (batch && selected) ? t.accentSoftBg : null,
        child: Row(
          children: [
            _gutterCell(t,
                line: line, height: _kRowExtent, current: anchor),
            for (var c = 0; c < cells.length && c < widths.length; c++)
              _cell(t, c, widths[c], cells[c],
                  line: line, current: anchor && c == _curCol),
          ],
        ),
      ),
    );
  }

  /// 一个单元格:定宽 + 右 / 下 1px 网格线 + 内容;[current] 为当前格
  /// (锚点行 × 当前列),整格铺强调色底并把文字 / 光标 / 占位换成反白档。
  ///
  /// 控件本身一律扁平([Input.showBorder] / [ComboBox.showBorder] = false):
  /// 静止态每格只有文字与网格线,这是参考工具设计器网格的关键观感。
  Widget _cell(AppPalette t, int col, double width, _GridCell content,
      {required double line, required bool current}) {
    final dt = TokenScope.maybeOf(context) ?? DesktopTokens.winForm;
    Widget cell = SizedBox(
      width: width,
      // 显式给高:Row 给子项的是松高度约束,内容塌成 0 高时(空的「键」格)
      // 右 / 下网格线就画不出来,同一行的线也会各自错位。
      height: _kRowExtent,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: current ? t.accent : null,
          border: Border(
            right: BorderSide(color: t.gridLine, width: line),
            bottom: BorderSide(color: t.gridLine, width: line),
          ),
        ),
        child: content.center ? Center(child: content.child) : content.child,
      ),
    );
    if (current) {
      cell = TokenScope(
        tokens: dt.copyWith(
          foregroundColor: t.accentForeground,
          // 禁用格(如整数类型的位宽只读长度)在实色底上要仍可辨
          disabledForegroundColor:
              t.accentForeground.withValues(alpha: 0.72),
          // 光标取 primaryColor:不换成反白就会蓝底上画蓝线,看不见插入点
          primaryColor: t.accentForeground,
        ),
        child: cell,
      );
    }
    // 点哪格哪格成为当前格(表头同步高亮该列)。Listener 不吞事件,
    // 编辑器照常拿到这一次点击;behavior=opaque 让格子空白处也算命中。
    return Listener(
      behavior: HitTestBehavior.opaque,
      onPointerDown: (_) => _setCurrentCol(col),
      child: cell,
    );
  }

  /// 底部属性面板:每行一个属性(标签列 + 控件列),超高时面板内滚动;
  /// model 为空(无选中行)时整个面板隐藏。
  ///
  /// [model] 传**取值闭包**:面板挂在 [_chromeRev] 上延迟重建,快照 model 会
  /// 一直显示切换前的那一行。
  Widget _props<T>(
    AppPalette t,
    T? Function() model,
    List<Widget> Function(T) builder, {
    String? hint,
  }) {
    return ValueListenableBuilder(
      valueListenable: _propsRev,
      builder: (context, _, __) {
        final m = model();
        if (m == null) return const SizedBox.shrink();
        return Container(
          width: double.infinity,
          constraints: const BoxConstraints(maxHeight: 232),
          padding: const EdgeInsets.fromLTRB(_kPropPadL, 10, _kPropPadL, 8),
          color: t.background,
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (final w in builder(m)) w,
                if (hint != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 4),
                    child: Text(
                      hint,
                      style:
                          TextStyle(fontSize: 11.5, color: t.disabledForeground),
                    ),
                  ),
              ],
            ),
          ),
        );
      },
    );
  }

  /// 属性字段:标签列(左起 11、宽 190)+ 控件列(自 201 起、默认宽 300),
  /// 行距 26、标签带冒号 —— 字段起点 201 正好落在网格第一列(200 宽)之后,
  /// 上下两块因此是对齐的。
  Widget _propField(AppPalette t, String label, Widget child,
      {double? fieldWidth, double? height, bool expandField = false}) {
    // [expandField] = 控件吃掉剩余宽度(典型是多行框);此时行高不锁死,
    // 让子项按自身固有高度撑开,标签顶对齐,否则单行标签会垂直居中地飘在多行框中间。
    final rowH = height ?? _kPropRowH;
    return SizedBox(
      height: expandField && height == null ? null : rowH,
      child: Row(
        crossAxisAlignment: expandField || height != null
            ? CrossAxisAlignment.start
            : CrossAxisAlignment.center,
        children: [
          SizedBox(
            width: _kPropLabelW,
            height: _kPropRowH,
            child: Text(
              '$label:',
              style: TextStyle(
                fontSize: 12,
                color: bodyTextColor(context),
                decoration: TextDecoration.none,
                fontWeight: FontWeight.w400,
              ),
              overflow: TextOverflow.ellipsis,
              maxLines: 1,
            ),
          ),
          if (expandField)
            Expanded(child: child)
          else
            SizedBox(width: fieldWidth ?? _kPropFieldW, child: child),
        ],
      ),
    );
  }

  // ── 通用控件 ──────────────────────────────────────────────

  /// 绑定字符串字段的输入框:自持控制器、变更即时写回模型,外部值变化时同步。
  ///
  /// 只读降级在本方法统一门控:字段网格里几十处文本格共用此入口,
  /// 逐个传参会漏改且淹没真正的差异。
  ///
  /// [flat] = 网格单元格样式:不描边、不填底,文字直接落在网格上
  /// (水平留白走 [gridCellTokens] 的 controlPaddingX = 5)。
  /// [row] = 所在行号(网格内的单元格才传):写回时只重建该行 + 依赖设计数据
  /// 的那几小块;不传(属性面板 / 选项页)则只刷那几小块。
  Widget _modelInput(
    String value,
    ValueChanged<String> onChanged, {
    String? hint,
    bool enabled = true,
    double? width,
    TextAlign? textAlign,
    TextInputType? keyboardType,
    bool flat = false,
    int? row,
  }) {
    final input = _ModelInput(
      value: value,
      enabled: enabled && !_readOnly,
      hint: hint,
      textAlign: textAlign,
      keyboardType: keyboardType,
      showBorder: !flat,
      onChanged: (v) {
        onChanged(v);
        _touchCell(row ?? -1);
      },
    );
    return width == null ? input : SizedBox(width: width, child: input);
  }

  /// 候选下拉(仍可手输):候选为空时行为等同输入框。
  ///
  /// 与选项页局部的 `combo` 同构,供各属性面板复用;只读降级统一在此收口。
  Widget _candidateCombo(
          List<String> items, String value, ValueChanged<String> onChanged,
          String hint,
          {bool enabled = true}) =>
      ComboBox<String>(
        items: items,
        value: value.isEmpty ? null : value,
        editable: true,
        enabled: enabled && !_readOnly,
        hint: hint,
        onChanged: (v) {
          onChanged(v ?? '');
          _touchChrome();
        },
      );

  /// 灰色占位输入(正在缓冲 / 快速更新 等暂未支持项)
  Widget get _disabledInput => _ModelInput(value: '', onChanged: (_) {}, enabled: false, hint: '—');

  /// 只读展示输入(值由其它标签推导,不开放编辑)
  Widget _readonlyInput(String value) =>
      _ModelInput(value: value, onChanged: (_) {}, enabled: false, hint: '—');

  /// 单元格右侧的 `...` 按钮(打开选择弹窗);onTap 为 null 时置灰。
  /// 尺寸贴网格档:比行高再矮一档,才不会顶破 [_kRowContentH] 的行。
  Widget _dotsButton({required VoidCallback? onTap, String? tooltip}) => IconBtn(
        child: const Text(
          '...',
          style: TextStyle(fontSize: 13, height: 1.0, fontWeight: FontWeight.w600),
        ),
        size: const Size(20, 18),
        outline: true,
        tooltip: tooltip,
        onTap: onTap,
      );

  /// YES / NO 下拉(空 = 不生成子句)
  Widget _yesNoCombo(String value, ValueChanged<String> onChanged, {bool enabled = true}) {
    return ComboBox<String>(
      items: _yesNo,
      value: value,
      enabled: enabled && !_readOnly,
      onChanged: (v) {
        onChanged(v ?? '');
        _touchChrome();
      },
    );
  }

  // ── 字段动作 ──────────────────────────────────────────────

  /// 网格键盘入口:Ctrl/Cmd + C 复制选中行、+ V 把剪贴板整批粘贴为行。
  /// 焦点落在单元格输入框里时这两个键归输入框(文本复制 / 粘贴),
  /// 那种情况下走工具栏按钮或行右键菜单。
  KeyEventResult _onGridKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent || _tabIndex != 0) return KeyEventResult.ignored;
    if (!(HardwareKeyboard.instance.isControlPressed ||
        HardwareKeyboard.instance.isMetaPressed)) {
      return KeyEventResult.ignored;
    }
    if (event.logicalKey == LogicalKeyboardKey.keyC) {
      _copyColumns();
      return KeyEventResult.handled;
    }
    if (event.logicalKey == LogicalKeyboardKey.keyV && _actionsEnabled) {
      _pasteColumns();
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  /// 把键盘焦点交给网格(点行之后 Ctrl+C / Ctrl+V 才有落点)。
  /// 点击落在输入框里时不抢:那一次点击是要开始编辑单元格。
  void _focusGrid() {
    final focused = FocusManager.instance.primaryFocus;
    if (focused?.context?.widget is EditableText) return;
    _gridFocus.requestFocus();
  }

  /// 字段网格当前选择(升序、去越界);集合为空时回退到锚点行
  List<int> _selectedColumns() {
    final cols = _design.columns;
    final idx =
        _selColumns.where((i) => i >= 0 && i < cols.length).toList()..sort();
    if (idx.isNotEmpty) return idx;
    return _selColumn >= 0 && _selColumn < cols.length ? [_selColumn] : const [];
  }

  /// 只选中一行(锚点跟随)
  void _selectOnly(int index) {
    _selColumn = index;
    _selColumns
      ..clear()
      ..add(index);
  }

  /// 选中集变化:只重建**状态真的变了**的那几行 + 依赖设计数据的那几小块。
  ///
  /// 整页 setState 实测每点一行 ≈ 130ms(一屏 160+ 个常开编辑器全部重建),
  /// 而移动一个选中标记根本不需要重建没变的行。notifier 只是信号,
  /// 行内容一律现读模型,所以这里漏算最坏是多重建一次、不会显示成旧状态。
  void _applyFieldSelection(void Function() mutate) {
    final before = Set<int>.of(_selColumns);
    final beforeAnchor = _selColumn;
    mutate();
    final affected = <int>{
      ...before.where((r) => !_selColumns.contains(r)),
      ..._selColumns.where((r) => !before.contains(r)),
      beforeAnchor,
      _selColumn,
    };
    for (final r in affected) {
      if (r >= 0 && r < _design.columns.length) _rowRev(r).value++;
    }
    _chromeRev.value++;
    // 面板显示的是「选中那一行」的属性:锚点没换就不必重画(点同一行的
    // 另一格是最常见操作,面板 10+ 个编辑器重建一次就是十几毫秒)
    if (beforeAnchor != _selColumn) _propsRev.value++;
  }

  /// 点行:Ctrl 加选 / 取消单行,Shift 从锚点连选一段,普通点击为单选
  void _selectColumnAt(int index, bool ctrl, bool shift) {
    _applyFieldSelection(() {
      if (shift && _selColumn >= 0) {
        final lo = index < _selColumn ? index : _selColumn;
        final hi = index < _selColumn ? _selColumn : index;
        _selColumns
          ..clear()
          ..addAll(List<int>.generate(hi - lo + 1, (i) => lo + i));
      } else if (ctrl) {
        if (_selColumns.contains(index)) {
          // 至少保留一行选中,否则锚点行看起来仍是「当前行」
          if (_selColumns.length > 1) _selColumns.remove(index);
        } else {
          _selColumns.add(index);
        }
        _selColumn = index;
      } else {
        _selectOnly(index);
      }
    });
    _focusGrid();
  }

  /// 点格:该列成为当前列(该格反白、表头跟着高亮)。锚点行不变。
  void _setCurrentCol(int col) {
    if (_curCol == col) return;
    final old = _selColumn;
    _curCol = col;
    // 反白块要从旧位置挪走:锚点行(可能没变)+ 表头的当前列
    if (old >= 0 && old < _design.columns.length) _rowRev(old).value++;
    if (_selColumn >= 0 &&
        _selColumn < _design.columns.length &&
        _selColumn != old) {
      _rowRev(_selColumn).value++;
    }
    _curColRev.value++;
  }

  /// 行右键菜单:批量动作的稳定入口(焦点落在输入框里时 Ctrl+C 会被文本框
  /// 当作复制选中文本处理,这里给一条不依赖焦点的路径)
  void _columnContextMenu(int index, Offset position) {
    final cols = _design.columns;
    if (index < 0 || index >= cols.length) return;
    if (!_selColumns.contains(index)) {
      _applyFieldSelection(() => _selectOnly(index));
    }
    _focusGrid();
    final n = _selectedColumns().length;
    final multi = n > 1;
    final canEdit = _actionsEnabled;
    showContextMenu(
      context,
      position: position,
      items: [
        // 工具栏是纯图标(参考工具同),这些动作全部收在右键菜单里
        MenuItem(
            text: '插入字段',
            enabled: canEdit,
            onPressed: _insertColumn),
        MenuItem(
            text: multi ? '删除 $n 个字段' : '删除字段',
            enabled: canEdit,
            onPressed: _removeColumn),
        const MenuSeparator(),
        MenuItem(
            text: '设置主键',
            enabled: canEdit && multi,
            onPressed: () => _setPk(true)),
        MenuItem(
            text: '取消主键',
            enabled: canEdit && multi,
            onPressed: () => _setPk(false)),
        const MenuSeparator(),
        MenuItem(
            text: '上移',
            enabled: canEdit && _canReorderColumns && _selColumn > 0,
            onPressed: _moveColumnUp),
        MenuItem(
            text: '下移',
            enabled: canEdit &&
                _canReorderColumns &&
                _selColumn < cols.length - 1,
            onPressed: _moveColumnDown),
        const MenuSeparator(),
        MenuItem(
            text: multi ? '复制 $n 行' : '复制行',
            shortcut: 'Ctrl+C',
            onPressed: _copyColumns),
        MenuItem(
            text: '粘贴行',
            shortcut: 'Ctrl+V',
            enabled: canEdit,
            onPressed: _pasteColumns),
      ],
    );
  }

  /// 复制选中字段:Tab 分隔、一行一个(与数据浏览页「复制记录」同格式,
  /// 可直接贴进表格软件,也能原样粘回设计器)
  void _copyColumns() {
    final idx = _selectedColumns();
    if (idx.isEmpty) return;
    final picked = [for (final i in idx) _design.columns[i]];
    Clipboard.setData(ClipboardData(text: ColumnRowClipboard.encode(picked)));
    _note = '已复制 ${idx.length} 个字段';
    _touchChrome();
  }

  /// 粘贴字段:剪贴板文本按行解析,整批插到选中区之后(无选中则追加到末尾)
  Future<void> _pasteColumns() async {
    if (!_actionsEnabled) return;
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final rows = ColumnRowClipboard.parse(data?.text ?? '');
    if (!mounted) return;
    if (rows.isEmpty) {
      setState(() => _error = '剪贴板里没有可粘贴的字段:每行一个字段,单元格以 Tab 分隔');
      return;
    }
    setState(() {
      _error = null;
      final cols = _design.columns;
      final sel = _selectedColumns();
      var at = sel.isEmpty ? cols.length : sel.last + 1;
      // 新建表预置的那行空字段:粘贴时直接占用它,末尾不留空行
      if (at == cols.length &&
          cols.isNotEmpty &&
          ColumnRowClipboard.isBlank(cols.last)) {
        at = cols.length - 1;
        cols.removeAt(at);
      }
      cols.insertAll(at, rows);
      _selColumns
        ..clear()
        ..addAll(List<int>.generate(rows.length, (i) => at + i));
      _selColumn = at + rows.length - 1;
      _note = '已粘贴 ${rows.length} 个字段';
    });
  }

  void _addColumn() {
    setState(() {
      _design.columns.add(DesignColumn());
      _selectOnly(_design.columns.length - 1);
    });
  }

  void _insertColumn() {
    setState(() {
      final at = _selColumn.clamp(0, _design.columns.length);
      _design.columns.insert(at, DesignColumn());
      _selectOnly(at);
    });
  }

  /// 删除字段:多选时整批删除(从后往前移除,避免索引错位)
  void _removeColumn() {
    setState(() {
      final idx = _selectedColumns();
      if (idx.isEmpty) return;
      final cols = _design.columns;
      for (final i in idx.reversed) {
        cols.removeAt(i);
      }
      _selColumns.clear();
      _selColumn = cols.isEmpty ? -1 : idx.first.clamp(0, cols.length - 1);
      if (_selColumn >= 0) _selColumns.add(_selColumn);
    });
  }

  /// 显式设置 / 取消选中行的主键(供「主键 ▾」下拉与右键;多选即复合主键,
  /// 序号由 [DdlBuilder.pkOrdinals] 按列序推导,无需手工维护)
  void _setPk(bool on) {
    setState(() {
      for (final i in _selectedColumns()) {
        _design.columns[i].primaryKey = on;
      }
    });
  }

  /// 切换指定行的主键(「键」列点击,零延迟)
  void _togglePkAt(int index) {
    setState(() {
      if (index < 0 || index >= _design.columns.length) return;
      final c = _design.columns[index];
      c.primaryKey = !c.primaryKey;
      _selectOnly(index);
    });
  }

  /// 设置 / 清除列的 IDENTITY(虚拟类型);
  /// 开启时预填截图同款序列默认值,并自动勾选「不是 null」
  void _applyIdentity(DesignColumn c, String mode) {
    if (mode.isEmpty) {
      c.identityMode = '';
      c.identityIncrement = '';
      c.identityMinValue = '';
      c.identityMaxValue = '';
      c.identityStart = '';
      c.identityCache = '';
      c.identityCycle = false;
      return;
    }
    c.identityMode = mode;
    if (c.identityIncrement.isEmpty) c.identityIncrement = '1';
    if (c.identityMinValue.isEmpty) c.identityMinValue = '1';
    if (c.identityMaxValue.isEmpty) c.identityMaxValue = kIdentityMaxValue(c.type);
    if (c.identityStart.isEmpty) c.identityStart = '1';
    if (c.identityCache.isEmpty) c.identityCache = '1';
    c.notNull = true;
  }

  void _moveColumnUp() {
    setState(() {
      if (_selColumn <= 0) return;
      final i = _selColumn;
      final c = _design.columns.removeAt(i);
      _design.columns.insert(i - 1, c);
      _selectOnly(i - 1);
    });
  }

  void _moveColumnDown() {
    setState(() {
      final i = _selColumn;
      if (i < 0 || i >= _design.columns.length - 1) return;
      final c = _design.columns.removeAt(i);
      _design.columns.insert(i + 1, c);
      _selectOnly(i + 1);
    });
  }

  void _addFk() {
    setState(() {
      final fk = DesignForeignKey(refSchema: widget.schema ?? '');
      _design.foreignKeys.add(fk);
      _selFk = _design.foreignKeys.length - 1;
    });
  }

  /// 通用「添加」:向列表追加一行新模型并选中
  void _addRow<T>(
    List<T> list,
    ValueChanged<int> select,
  ) {
    setState(() {
      list.add(_newRow<T>());
      select(list.length - 1);
    });
  }

  /// 按类型创建一行设计数据(与 _addRow 的 T 对应)
  T _newRow<T>() {
    if (T == DesignIndex) return DesignIndex() as T;
    if (T == DesignForeignKey) return DesignForeignKey() as T;
    if (T == DesignUniqueKey) return DesignUniqueKey() as T;
    if (T == DesignCheck) return DesignCheck() as T;
    if (T == DesignExclude) return DesignExclude() as T;
    if (T == DesignRule) return DesignRule() as T;
    if (T == DesignTrigger) return DesignTrigger() as T;
    throw StateError('不支持的模型类型: $T');
  }

  /// 通用「删除」:删除选中行并回退选中到安全位置
  void _removeRow<T>(
    List<T> list,
    int Function() selectedOf,
    ValueChanged<int> select,
  ) {
    setState(() {
      final sel = selectedOf();
      if (sel < 0 || sel >= list.length) return;
      list.removeAt(sel);
      select(list.isEmpty ? -1 : (sel < list.length ? sel : list.length - 1));
    });
  }
}

/// Ctrl/Cmd + S 触发的保存意图(供 [Shortcuts] / [Actions] 绑定)
/// 编辑网格里的一格内容。
///
/// 只带内容与「怎么摆」,宽度 / 网格线 / 当前格反白由 [_editorGrid] 一侧的
/// [_cell] 统一负责 —— 各标签的行构建器因此不必各自写死列宽与列间距。
class _GridCell {
  const _GridCell(this.child, {this.center = false});

  final Widget child;

  /// 在列内水平居中(勾选框 / 钥匙图标这类定宽内容);
  /// 文字格为 false,左内边距由控件自己的 controlPaddingX 提供。
  final bool center;
}

class _SaveIntent extends Intent {
  const _SaveIntent();
}

/// 绑定模型字符串字段的输入框:自持 [TextEditingController],
/// 输入即时写回模型(触发父级重建以刷新 SQL 预览);外部值变化时同步文本。
class _ModelInput extends StatefulWidget {
  const _ModelInput({
    required this.value,
    required this.onChanged,
    this.hint,
    this.enabled = true,
    this.textAlign,
    this.keyboardType,
    this.showBorder = true,
  });

  final String value;
  final ValueChanged<String> onChanged;
  final String? hint;
  final bool enabled;

  /// 数字属性右对齐用
  final TextAlign? textAlign;
  final TextInputType? keyboardType;

  /// `false` = 网格单元格样式(不描边不填底,水平留白由 tokens 给)
  final bool showBorder;

  @override
  State<_ModelInput> createState() => _ModelInputState();
}

class _ModelInputState extends State<_ModelInput> {
  late final TextEditingController _controller;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.value);
  }

  @override
  void didUpdateWidget(_ModelInput oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.value != _controller.text) {
      _controller.text = widget.value;
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Input(
      controller: _controller,
      hint: widget.hint,
      enabled: widget.enabled,
      selectAllOnFocus: false,
      textAlign: widget.textAlign,
      keyboardType: widget.keyboardType,
      showBorder: widget.showBorder,
      // 不写死 contentPadding:留白与垂直居中交给 tokens(controlPaddingX /
      // controlHeight),这样网格格(5 / 20)与属性面板字段共用同一档
      onChanged: widget.onChanged,
    );
  }
}

/// 网格格子塞不下时的多行编辑大弹窗(参考工具点 `...` 打开的那一个)。
///
/// 用在「检查」的表达式格:条件常写成 `a > 0 AND b <> a` 这类跨行 SQL,
/// 单行输入框既看不清也改不动。确定回传文本,取消返回 null。
class _MultilineEditorDialog extends StatefulWidget {
  const _MultilineEditorDialog({
    required this.title,
    required this.value,
    this.hint,
  });

  final String title;
  final String value;
  final String? hint;

  static Future<String?> show(
    BuildContext context, {
    required String title,
    required String value,
    String? hint,
  }) {
    return showDialog<String>(
      context: context,
      builder: (_) =>
          _MultilineEditorDialog(title: title, value: value, hint: hint),
    );
  }

  @override
  State<_MultilineEditorDialog> createState() => _MultilineEditorDialogState();
}

class _MultilineEditorDialogState extends State<_MultilineEditorDialog> {
  late final TextEditingController _controller =
      TextEditingController(text: widget.value);

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return DialogBox(
      title: widget.title,
      width: 640,
      height: 260,
      onClose: () => Navigator.of(context).pop(),
      footer: Padding(
        padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.end,
          children: [
            Button(
                text: '确定',
                onPressed: () => Navigator.of(context).pop(_controller.text)),
            const SizedBox(width: 8),
            Button(text: '取消', onPressed: () => Navigator.of(context).pop()),
          ],
        ),
      ),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 4),
        child: Textarea(
          controller: _controller,
          hint: widget.hint,
          expands: true,
          showBorder: true,
        ),
      ),
    );
  }
}

/// [_ModelInput] 的多行版:同一套「自持控制器 + 外部值变化时同步」契约。
///
/// 只用在属性面板里那些值本身是多段 SQL 的字段(典型是 MySQL / SQL Server 的
/// 触发主体)。单行 Input 装不下它,而直接写 [Textarea] 又要调用方自己管控制器。
class _ModelTextarea extends StatefulWidget {
  const _ModelTextarea({
    required this.value,
    required this.onChanged,
    this.hint,
    this.enabled = true,
  });

  final String value;
  final ValueChanged<String> onChanged;
  final String? hint;
  final bool enabled;

  @override
  State<_ModelTextarea> createState() => _ModelTextareaState();
}

class _ModelTextareaState extends State<_ModelTextarea> {
  late final TextEditingController _controller;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.value);
  }

  @override
  void didUpdateWidget(_ModelTextarea oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.value != _controller.text) {
      _controller.text = widget.value;
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    // 不用 expands:属性面板整体挂在 SingleChildScrollView 里,子项拿到的是
    // **无界高度**约束,撑满式的文本框会在布局阶段自激(实测直接把 pumpAndSettle
    // 卡死)。按固定行数自然撑高,行高由 _propField 的 height 留出。
    return Textarea(
      controller: _controller,
      hint: widget.hint,
      enabled: widget.enabled,
      minLines: 3,
      maxLines: 3,
      onChanged: widget.onChanged,
    );
  }
}
