//! 真驱动共用的网格数据类型：列元数据 / 行标识 / 一页取数 / 一条语句的回执。
//!
//! 从 SQLite 驱动里抽出来是因为 PostgreSQL 档要讲同一套话：网格只认
//! 「列清单 + 文本值 + 每行一个身份」，身份长什么样（SQLite 的 rowid、
//! PG 的主键列值）由各自驱动决定，UI 层只管原样带回去写回。
//!
//! 值的口径全后端统一：NULL 打字面 `NULL`，二进制打 `<BLOB n 字节>` 占位，
//! 其余按驱动返回的文本原样展示。

/// 数据网格的一列：名字 / 类型文案 / 是否数值列（右对齐 + 数值排序）。
#[derive(Clone, Debug, PartialEq)]
pub struct ColMeta {
    pub name: String,
    pub ty: String,
    pub numeric: bool,
    /// 初始列宽（网格可拖调；按名字 / 类型长度估）
    pub width: f32,
    /// 可参与 UPDATE / INSERT（生成列、隐藏列恒 false）
    pub writable: bool,
    /// 主键列（写回时按这些列定位行；SQLite 用 rowid，恒 false）
    pub pk: bool,
}

impl ColMeta {
    pub fn new(name: impl Into<String>, ty: impl Into<String>, numeric: bool) -> Self {
        let name = name.into();
        let ty = ty.into();
        let w = (name.chars().count() + ty.chars().count()).max(8) as f32 * 7.0 + 24.0;
        Self {
            name,
            ty,
            numeric,
            width: w.clamp(48.0, 240.0),
            writable: true,
            pk: false,
        }
    }
}

/// 目录缓存：把已驻留的名字列表固化成树渲染用的 `'static` 切片。
pub fn leak_slice(v: Vec<&'static str>) -> &'static [&'static str] {
    Box::leak(v.into_boxed_slice())
}

/// 一行的身份，写回时原样送回：
/// - SQLite：单元素，rowid 的十进制文本；无 rowid 表是空向量
/// - PostgreSQL：主键列的值，顺序与 `ColMeta::pk` 的列序一致；无主键表是空向量
pub type RowKey = Vec<String>;

/// 网格工作集里的一行：按 `Arc` 共享，基线 / 撤销快照只克隆指针列表，
/// 改格时对目标行做写时复制，避免整页多份副本。
pub type Row = std::sync::Arc<Vec<String>>;

/// 一次 SELECT 的取回：列元数据 + 行 + 平行行身份。
#[derive(Debug)]
pub struct SelectPage {
    pub columns: Vec<ColMeta>,
    pub rows: Vec<Vec<String>>,
    pub keys: Vec<RowKey>,
    /// 结果被行数上限截断（只用于查询页提示）
    pub truncated: bool,
}

impl SelectPage {
    /// mock 档没有真列，从 MOCK_COLUMNS 换算一份同构元数据。
    pub fn mock() -> Self {
        Self {
            columns: crate::mock::MOCK_COLUMNS
                .iter()
                .map(|m| ColMeta::new(m.name, m.ty, m.numeric))
                .collect(),
            rows: Vec::new(),
            keys: Vec::new(),
            truncated: false,
        }
    }
}

/// 一条语句的执行结果：SELECT 出网格，写操作带影响行数。
#[derive(Debug)]
pub struct ExecOutcome {
    pub select: Option<SelectPage>,
    pub affected: usize,
    /// 写操作的一句话消息（如「3 rows affected」）；SELECT 被截断时也有消息
    pub message: String,
}

/// 后台脚本里一条语句的回执。
pub struct StmtRun {
    pub elapsed_ms: u128,
    pub result: Result<ExecOutcome, String>,
}

/// 一次后台脚本的回执（在工作线程上攒好，回 UI 线程只负责上屏）。
pub struct ScriptRun {
    pub runs: Vec<StmtRun>,
    /// 连接都没开起来（文件被删 / 网络不通），一条都没跑
    pub open_error: Option<String>,
    /// 点了「停止」（或事务中止后必须停），剩余语句没执行
    pub cancelled: bool,
    /// 脚本结束时事务挂在半路，已自动回滚
    pub rolled_back: bool,
    /// 失败发生在显式事务里：PostgreSQL 会把事务标成「已中止」，之后的语句
    /// 必然连带失败，只能就地回滚收尾（SQLite 没有这个状态，恒 false）
    pub aborted_tx: bool,
    /// 整个脚本耗时
    pub total_ms: u128,
}

/// 把网格工作集写回真库。调用方给：
/// - `table` 目标表
/// - `columns` 当前列清单（对取数结果的顺序）
/// - `updates` (行身份, 列下标, 新值) 列表——行身份按驱动口径（SQLite 是
///   单元素 rowid，PG 是主键列值）
/// - `deletes` 行身份列表
/// - `inserts` 行值列表
/// 全部包在一个事务里；任何一条失败整体回滚。
pub struct GridChanges<'a> {
    pub table: &'a str,
    pub columns: &'a [ColMeta],
    pub updates: &'a [(RowKey, usize, String)],
    pub deletes: &'a [RowKey],
    pub inserts: &'a [Vec<String>],
}
