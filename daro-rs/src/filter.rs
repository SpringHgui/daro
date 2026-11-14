//! 数据网格筛选器（E节）的纯模型：准则树、参考界面实测的 23 个算子、
//! WHERE / ORDER BY 生成、「文本」模式的反解析（生成 ↔ 解析往返）、
//! mock 行匹配。UI 在 main.rs 的 filter_pane / table_page。
//!
//! 连接词语义照 SQL 优先级：顶层连续的 AND 结成簇、OR 分隔簇；生成的
//! SQL 在混合连接词时给 AND 簇加括号，保证文本往返无歧义。组内连接词
//! 统一同词（组的 and 标志），参考界面组内行的连接词 chip 切的是整组。

use gpui_kit::component::input::{EditorState, InputState};
use gpui_kit::Entity;

use crate::mock::DbKind;
use crate::db_types::ColMeta;

/// 筛选算子。顺序 = 参考界面算子菜单的实测顺序（23 项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    ContainsCi,
    NotContains,
    NotContainsCi,
    StartsWith,
    NotStartsWith,
    EndsWith,
    NotEndsWith,
    IsNull,
    IsNotNull,
    IsBlank,
    IsNotBlank,
    Between,
    NotBetween,
    InList,
    NotInList,
    Custom,
}

pub const ALL_OPS: [FOp; 23] = [
    FOp::Eq,
    FOp::Ne,
    FOp::Lt,
    FOp::Le,
    FOp::Gt,
    FOp::Ge,
    FOp::Contains,
    FOp::ContainsCi,
    FOp::NotContains,
    FOp::NotContainsCi,
    FOp::StartsWith,
    FOp::NotStartsWith,
    FOp::EndsWith,
    FOp::NotEndsWith,
    FOp::IsNull,
    FOp::IsNotNull,
    FOp::IsBlank,
    FOp::IsNotBlank,
    FOp::Between,
    FOp::NotBetween,
    FOp::InList,
    FOp::NotInList,
    FOp::Custom,
];

impl FOp {
    /// 算子菜单文案，照参考界面实测逐字（首项在菜单里带 ✓）。
    pub fn label(self) -> &'static str {
        match self {
            FOp::Eq => "=",
            FOp::Ne => "!=",
            FOp::Lt => "<",
            FOp::Le => "<=",
            FOp::Gt => ">",
            FOp::Ge => ">=",
            FOp::Contains => "包含",
            FOp::ContainsCi => "包含（不区分大小写）",
            FOp::NotContains => "不包含",
            FOp::NotContainsCi => "不包含（不区分大小写）",
            FOp::StartsWith => "开头是",
            FOp::NotStartsWith => "开头不是",
            FOp::EndsWith => "结尾是",
            FOp::NotEndsWith => "结尾不是",
            FOp::IsNull => "是 null",
            FOp::IsNotNull => "不是 null",
            FOp::IsBlank => "是空的",
            FOp::IsNotBlank => "是非空的",
            FOp::Between => "介于",
            FOp::NotBetween => "不介于",
            FOp::InList => "在列表",
            FOp::NotInList => "不在列表",
            FOp::Custom => "[自定义]",
        }
    }

    /// (要不要值1, 要不要值2)。「介于」占两个值，IsNull 一族与自定义无值。
    pub fn arity(self) -> (bool, bool) {
        match self {
            FOp::IsNull | FOp::IsNotNull | FOp::IsBlank | FOp::IsNotBlank | FOp::Custom => {
                (false, false)
            }
            FOp::Between | FOp::NotBetween => (true, true),
            _ => (true, false),
        }
    }
}

/// 默认算子按列类型：文本列「包含」、其余「=」（参考界面实测 varchar→包含、
/// int8→=；bool / 数值同按「=」走）。
pub fn default_op(cols: &[ColMeta], col: usize) -> FOp {
    match cols.get(col) {
        Some(c) if !c.numeric && c.ty != "bool" => FOp::Contains,
        _ => FOp::Eq,
    }
}

/// 一条筛选准则。v2 只有「介于 / 不介于」用；custom 只在 Custom 用。
#[derive(Clone, Debug, PartialEq)]
pub struct FCrit {
    pub col: usize,
    pub op: FOp,
    pub v1: String,
    pub v2: String,
    pub custom: String,
    pub enabled: bool,
}

impl FCrit {
    pub fn new(cols: &[ColMeta], col: usize) -> Self {
        Self {
            col,
            op: default_op(cols, col),
            v1: String::new(),
            v2: String::new(),
            custom: String::new(),
            enabled: true,
        }
    }

    /// 值单元格显示的文本：空值显示占位 <?>（参考界面同款）。
    pub fn value_text(&self, which: usize) -> String {
        if self.op == FOp::Custom {
            let t = self.custom.trim().to_string();
            return if t.is_empty() { "<SQL>".into() } else { t };
        }
        let (need1, need2) = self.op.arity();
        let raw = if which == 2 { &self.v2 } else { &self.v1 };
        if !(which == 1 && need1 || which == 2 && need2) {
            return String::new();
        }
        if raw.trim().is_empty() {
            "<?>".to_string()
        } else {
            raw.trim().to_string()
        }
    }
}

/// 准则节点：单条准则或包了一层括号的组。组内连接词逐间隙存
/// （参考界面组内每行的 and / or chip 各自切换）。
#[derive(Clone, Debug, PartialEq)]
pub enum FNode {
    Crit(FCrit),
    Group {
        conns: Vec<bool>,
        children: Vec<FNode>,
    },
}

/// 顶层模型：条目 + 条目之间的连接词（true = AND）。
#[derive(Clone, Debug, PartialEq, Default)]
pub struct FilterModel {
    pub items: Vec<FNode>,
    pub conns: Vec<bool>,
}

impl FilterModel {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 收集全部准则（任意深度）。
    pub fn crits(&self) -> Vec<&FCrit> {
        fn walk<'a>(out: &mut Vec<&'a FCrit>, nodes: &'a [FNode]) {
            for n in nodes {
                match n {
                    FNode::Crit(c) => out.push(c),
                    FNode::Group { children, .. } => walk(out, children),
                }
            }
        }
        let mut out = Vec::new();
        walk(&mut out, &self.items);
        out
    }

    pub fn has_custom(&self) -> bool {
        self.crits().iter().any(|c| c.op == FOp::Custom)
    }
}

/// 「应用筛选 & 排序」后的快照：网格按它过滤 / 排序，状态栏 SQL 按它拼。
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Applied {
    /// 行匹配用的模型快照（与 where_sql 同源；Custom 准则按通过处理）
    pub model: FilterModel,
    pub where_sql: Option<String>,
    pub order_sql: Option<String>,
    /// 多键排序快照（列下标, 升序?）。空 = 无面板排序（回退表头排序）。
    pub sorts: Vec<(usize, bool)>,
    pub has_custom: bool,
}

/// 数据页签的筛选器状态（随 crate::OpenTab 存亡）。
#[derive(Clone)]
pub struct FilterState {
    pub open: bool,
    /// true = 「文本」模式（单选对的另一头是「创建工具」）
    pub text_mode: bool,
    pub model: FilterModel,
    /// 排序方式（列下标, 升序?），应用前只是草稿
    pub sorts: Vec<(usize, bool)>,
    /// 文本模式的当前正文（切回创建工具 / 应用时解析）
    pub text_sql: String,
    pub edited: bool,
    pub applied: Option<Applied>,
    /// 上次应用失败的原因（漏斗红 ! 徽标 + 面板红字）
    pub error: Option<String>,
    /// 文本模式的编辑器实体（要 window，切到文本模式时惰建）
    pub text_editor: Option<Entity<EditorState>>,
    /// ↑↓ 移动的选中准则路径（顶层下标起，逐层进组）
    pub sel: Option<Vec<usize>>,
    /// 选中落在组的「)」行而不是「(」行（两行分开高亮）
    pub sel_close: bool,
    /// 面板当前子视图：false = 筛选、true = 排序方式（左图标条切换）
    pub sort_view: bool,
    /// 面板高（逻辑像素），底边可拖
    pub pane_h: f32,
    /// 排序视图「文本」模式正文（ORDER BY 正文，不含 ORDER BY 关键字）
    pub sort_sql: String,
    /// 排序视图的文本编辑器实体（切到排序文本模式时惰建）
    pub sort_editor: Option<Entity<EditorState>>,
    /// 排序视图选中序号（↑↓ / Ctrl+Left/Right 移动）
    pub sel_sort: Option<usize>,
    /// 排序剪贴板（复制 / 复制所有排序 / 粘贴）
    pub sort_clip: Vec<(usize, bool)>,
    /// 列名下拉：打开于准则路径 / 排序序号（二选一）+ 锚点 y
    pub col_pick_crit: Option<Vec<usize>>,
    pub col_pick_sort: Option<usize>,
    pub col_pick_y: f32,
    /// 列名下拉的搜索输入实体（首次打开时惰建）
    pub col_pick_search: Option<Entity<InputState>>,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            open: false,
            text_mode: false,
            model: FilterModel::default(),
            sorts: Vec::new(),
            text_sql: String::new(),
            edited: false,
            applied: None,
            error: None,
            text_editor: None,
            sel: None,
            sel_close: false,
            sort_view: false,
            pane_h: 300.,
            sort_sql: String::new(),
            sort_editor: None,
            sel_sort: None,
            sort_clip: Vec::new(),
            col_pick_crit: None,
            col_pick_sort: None,
            col_pick_y: 0.,
            col_pick_search: None,
        }
    }
}

// ── SQL 生成 ──────────────────────────────────────────────────

fn quote_ident(kind: DbKind, name: &str) -> String {
    match kind {
        DbKind::MySql => format!("`{name}`"),
        _ => format!("\"{name}\""),
    }
}

fn is_numeric_col(cols: &[ColMeta], col: usize) -> bool {
    cols.get(col).is_some_and(|c| c.numeric)
}

/// 值字面量：数值列且解析得出数 → 裸值；否则单引号（内部 ' 双写）。
fn sql_lit(numeric_col: bool, v: &str) -> String {
    let t = v.trim();
    if numeric_col && t.parse::<f64>().is_ok() {
        t.to_string()
    } else {
        format!("'{}'", t.replace('\'', "''"))
    }
}

fn like_lit(v: &str, pre: &str, post: &str) -> String {
    format!("'{}{}{}'", pre, v.replace('\'', "''"), post)
}

/// 一条准则的 SQL 片段。PG 的不区分大小写走 ILIKE；其余方言没有通用
/// ILIKE，退化成 LIKE（解析按方言归属回对应算子：MySQL 侧 LIKE 恒回
/// 区分大小写档，SQL Server 侧同文——mock 档记录在案）。
pub fn crit_sql(kind: DbKind, cols: &[ColMeta], c: &FCrit) -> String {
    let col = cols
        .get(c.col)
        .map(|m| quote_ident(kind, &m.name))
        .unwrap_or_default();
    let (need1, _) = c.op.arity();
    let v1 = if need1 { c.v1.trim() } else { "" };
    let num = is_numeric_col(cols, c.col);
    match c.op {
        FOp::Eq => format!("{col} = {}", sql_lit(num, v1)),
        FOp::Ne => format!("{col} <> {}", sql_lit(num, v1)),
        FOp::Lt => format!("{col} < {}", sql_lit(num, v1)),
        FOp::Le => format!("{col} <= {}", sql_lit(num, v1)),
        FOp::Gt => format!("{col} > {}", sql_lit(num, v1)),
        FOp::Ge => format!("{col} >= {}", sql_lit(num, v1)),
        FOp::Contains => format!("{col} LIKE {}", like_lit(v1, "%", "%")),
        FOp::ContainsCi => {
            if kind == DbKind::PostgreSql {
                format!("{col} ILIKE {}", like_lit(v1, "%", "%"))
            } else {
                format!("{col} LIKE {}", like_lit(v1, "%", "%"))
            }
        }
        FOp::NotContains => format!("{col} NOT LIKE {}", like_lit(v1, "%", "%")),
        FOp::NotContainsCi => {
            if kind == DbKind::PostgreSql {
                format!("{col} NOT ILIKE {}", like_lit(v1, "%", "%"))
            } else {
                format!("{col} NOT LIKE {}", like_lit(v1, "%", "%"))
            }
        }
        FOp::StartsWith => format!("{col} LIKE {}", like_lit(v1, "", "%")),
        FOp::NotStartsWith => format!("{col} NOT LIKE {}", like_lit(v1, "", "%")),
        FOp::EndsWith => format!("{col} LIKE {}", like_lit(v1, "%", "")),
        FOp::NotEndsWith => format!("{col} NOT LIKE {}", like_lit(v1, "%", "")),
        FOp::IsNull => format!("{col} IS NULL"),
        FOp::IsNotNull => format!("{col} IS NOT NULL"),
        FOp::IsBlank => format!("{col} = ''"),
        FOp::IsNotBlank => format!("{col} <> ''"),
        FOp::Between | FOp::NotBetween => {
            let kw = if c.op == FOp::Between { "" } else { "NOT " };
            format!(
                "{col} {kw}BETWEEN {} AND {}",
                sql_lit(num, v1),
                sql_lit(num, c.v2.trim())
            )
        }
        FOp::InList | FOp::NotInList => {
            let items: Vec<String> = v1
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| sql_lit(num, s))
                .collect();
            if items.is_empty() {
                // 空列表：SQL 里 IN () 不合法，mock 档生成恒假表达式
                return if c.op == FOp::InList {
                    "1 = 0".to_string()
                } else {
                    "1 = 1".to_string()
                };
            }
            let kw = if c.op == FOp::InList { "IN" } else { "NOT IN" };
            format!("{col} {kw} ({})", items.join(", "))
        }
        FOp::Custom => c.custom.trim().to_string(),
    }
}

/// WHERE 正文；None = 模型为空或全部禁用（不加 WHERE）。
pub fn where_sql(kind: DbKind, cols: &[ColMeta], m: &FilterModel) -> Option<String> {
    let sql = emit_model(kind, cols, &m.items, &m.conns);
    if sql.trim().is_empty() {
        None
    } else {
        Some(sql)
    }
}

/// 条目列表 → SQL。AND 连续的条目结成一个簇（`(a AND b)`），OR 分隔簇；
/// 多簇时每个单条目簇加括号（`a OR b` → `(a) OR (b)`），已有括号的簇
/// （组 / 多条目簇）不重复包。这个口径保证 gen(parse(gen(m))) 恒等于
/// gen(m)（文本模式往返幂等）。禁用项剔除，连接词沿用条目原位前隙。
fn emit_model(kind: DbKind, cols: &[ColMeta], nodes: &[FNode], conns: &[bool]) -> String {
    let kept: Vec<(usize, String)> = nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| node_sql(kind, cols, n).map(|s| (i, s)))
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    // AND 簇：连续 and 连接的存活条目
    let mut clusters: Vec<Vec<usize>> = vec![vec![0]];
    for k in 1..kept.len() {
        let and = conns.get(kept[k].0.saturating_sub(1)).copied().unwrap_or(true);
        if and {
            clusters.last_mut().unwrap().push(k);
        } else {
            clusters.push(vec![k]);
        }
    }
    let render = |k: &[usize]| -> String {
        let inner = k
            .iter()
            .map(|&i| kept[i].1.clone())
            .collect::<Vec<_>>()
            .join(" AND ");
        // 单簇（纯 AND 链）不加括号：文本往返保持平展模型；多簇时
        // AND 簇必须自带括号防 OR 抢优先级
        if k.len() > 1 && clusters.len() > 1 {
            format!("({inner})")
        } else {
            inner
        }
    };
    let parts: Vec<String> = clusters.iter().map(|k| render(k)).collect();
    if parts.len() == 1 {
        return parts.into_iter().next().unwrap();
    }
    parts
        .iter()
        .map(|p| {
            if p.starts_with('(') && p.ends_with(')') {
                p.clone()
            } else {
                format!("({p})")
            }
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn node_sql(kind: DbKind, cols: &[ColMeta], n: &FNode) -> Option<String> {
    match n {
        FNode::Crit(c) => {
            if c.enabled {
                Some(crit_sql(kind, cols, c))
            } else {
                None
            }
        }
        FNode::Group { conns, children } => {
            let inner: Vec<String> =
                children.iter().filter_map(|c| node_sql(kind, cols, c)).collect();
            if inner.is_empty() {
                return None;
            }
            let joiner = |i: usize| {
                if conns.get(i).copied().unwrap_or(true) {
                    " AND "
                } else {
                    " OR "
                }
            };
            let mut s = inner[0].clone();
            for (i, part) in inner.iter().enumerate().skip(1) {
                s.push_str(joiner(i - 1));
                s.push_str(part);
            }
            Some(format!("({s})"))
        }
    }
}

/// ORDER BY 子句（含 " ORDER BY " 前缀）；空排序 → None。
pub fn order_by_sql(kind: DbKind, cols: &[ColMeta], sorts: &[(usize, bool)]) -> Option<String> {
    if sorts.is_empty() {
        return None;
    }
    let dirs: Vec<String> = sorts
        .iter()
        .filter_map(|(c, asc)| {
            cols.get(*c)
                .map(|m| format!("{} {}", quote_ident(kind, &m.name), if *asc { "ASC" } else { "DESC" }))
        })
        .collect();
    if dirs.is_empty() {
        return None;
    }
    Some(format!(" ORDER BY {}", dirs.join(", ")))
}

/// 「排序方式」文本模式的正文：逗号分隔的引号列名，降序追加 DESC、
/// 升序省略关键字（基线截图形态：`flowno` DESC, `dealerid`, `roomno`）。
/// 越界列下标跳过；空 sorts → 空串。
#[allow(dead_code)] // UI 接线在 main.rs 的筛选面板（排序方式视图）
pub fn order_text(kind: DbKind, cols: &[ColMeta], sorts: &[(usize, bool)]) -> String {
    sorts
        .iter()
        .filter_map(|(c, asc)| {
            cols.get(*c).map(|m| {
                let q = quote_ident(kind, &m.name);
                if *asc { q } else { format!("{q} DESC") }
            })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// 排序方向词（裸词 ASC / DESC 被 tokenize 成 Tok::Ident，据此归类）。
#[allow(dead_code)]
fn sort_dir(w: &str) -> Option<bool> {
    match w.to_ascii_uppercase().as_str() {
        "ASC" => Some(true),
        "DESC" => Some(false),
        _ => None,
    }
}

/// 排序文本 → (列下标, 升序?) 序列。文法 = 引号列名 + 可选 ASC / DESC
/// （大小写不敏感），逗号分隔；空白文本 → 空序列。为宽松起见裸词也按
/// 列名解析（ASC / DESC 保留字除外）。解析失败给中文原因（面板红字）。
#[allow(dead_code)]
pub fn parse_order(_kind: DbKind, cols: &[ColMeta], text: &str) -> Result<Vec<(usize, bool)>, String> {
    let toks = tokenize(text)?;
    let mut out: Vec<(usize, bool)> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let Some(Tok::Ident(name)) = toks.get(i) else {
            return Err("排序文本应以列名开头".into());
        };
        if sort_dir(name).is_some() {
            return Err(format!("「{name}」是排序方向保留字，不能作列名"));
        }
        let cix = col_index(cols, name)?;
        i += 1;
        // 可选方向词：DESC → false，ASC / 省略 → true
        let mut asc = true;
        if let Some(Tok::Ident(w)) = toks.get(i)
            && let Some(d) = sort_dir(w)
        {
            asc = d;
            i += 1;
        }
        out.push((cix, asc));
        if i == toks.len() {
            break;
        }
        if toks.get(i) == Some(&Tok::Comma) {
            i += 1;
            if i == toks.len() {
                return Err("排序文本末尾悬空逗号".into());
            }
        } else {
            return Err("排序缺少逗号分隔".into());
        }
    }
    Ok(out)
}

// ── 行匹配（mock 网格） ───────────────────────────────────────

/// 单条准则对一个单元格值的判定。null / 空串在 mock 档同义（单元格空串）。
pub fn crit_matches(cell: &str, cols: &[ColMeta], c: &FCrit) -> bool {
    let num = is_numeric_col(cols, c.col);
    let cmp = |a: &str, b: &str| -> std::cmp::Ordering {
        if num
            && let (Ok(x), Ok(y)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>())
        {
            return x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal);
        }
        a.cmp(b)
    };
    let eq_num = |a: &str, b: &str| -> bool {
        if num
            && let (Ok(x), Ok(y)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>())
        {
            return x == y;
        }
        a.trim() == b.trim()
    };
    let (need1, _) = c.op.arity();
    let v1 = if need1 { c.v1.trim() } else { "" };
    match c.op {
        FOp::Eq => eq_num(cell, v1),
        FOp::Ne => !eq_num(cell, v1),
        FOp::Lt => cmp(cell, v1) == std::cmp::Ordering::Less,
        FOp::Le => cmp(cell, v1) != std::cmp::Ordering::Greater,
        FOp::Gt => cmp(cell, v1) == std::cmp::Ordering::Greater,
        FOp::Ge => cmp(cell, v1) != std::cmp::Ordering::Less,
        FOp::Contains => cell.contains(v1),
        FOp::ContainsCi => cell.to_lowercase().contains(&v1.to_lowercase()),
        FOp::NotContains => !cell.contains(v1),
        FOp::NotContainsCi => !cell.to_lowercase().contains(&v1.to_lowercase()),
        FOp::StartsWith => cell.starts_with(v1),
        FOp::NotStartsWith => !cell.starts_with(v1),
        FOp::EndsWith => cell.ends_with(v1),
        FOp::NotEndsWith => !cell.ends_with(v1),
        FOp::IsNull | FOp::IsBlank => cell.trim().is_empty(),
        FOp::IsNotNull | FOp::IsNotBlank => !cell.trim().is_empty(),
        FOp::Between | FOp::NotBetween => {
            let inside = cmp(cell, v1) != std::cmp::Ordering::Less
                && cmp(cell, c.v2.trim()) != std::cmp::Ordering::Greater;
            if c.op == FOp::Between { inside } else { !inside }
        }
        FOp::InList | FOp::NotInList => {
            let hit = v1
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .any(|s| eq_num(cell, s));
            if c.op == FOp::InList { hit } else { !hit }
        }
        // 自定义 SQL 片段 mock 无法求值：不参与行匹配（界面有提示）
        FOp::Custom => true,
    }
}

/// 一行数据是否过得了整个模型（自定义准则按通过处理，调用方负责提示）。
pub fn row_matches(row: &[String], cols: &[ColMeta], m: &FilterModel) -> bool {
    fn nodes_match(row: &[String], cols: &[ColMeta], nodes: &[FNode], conns: &[bool]) -> bool {
        if nodes.is_empty() {
            return true;
        }
        // 按 OR 边界切成 AND 簇：簇内全过、任一簇过即过。
        // 不能把首节点预置进 any——AND 链中途失败时首节点的 true 会漏出去。
        let mut cluster_ok = true;
        let mut any = false;
        for (i, n) in nodes.iter().enumerate() {
            cluster_ok = cluster_ok && node_matches(row, cols, n);
            let is_last = i + 1 == nodes.len();
            if is_last || !conns.get(i).copied().unwrap_or(true) {
                any = any || cluster_ok;
                cluster_ok = true;
            }
        }
        any
    }
    fn node_matches(row: &[String], cols: &[ColMeta], n: &FNode) -> bool {
        match n {
            FNode::Crit(c) => {
                if !c.enabled {
                    return true;
                }
                match row.get(c.col) {
                    Some(cell) => crit_matches(cell, cols, c),
                    None => true,
                }
            }
            FNode::Group { conns, children } => {
                if children.is_empty() {
                    return true;
                }
                nodes_match(row, cols, children, conns)
            }
        }
    }
    nodes_match(row, cols, &m.items, &m.conns)
}

/// 多键排序：返回 (data 里的) 行下标序。sorts 逐键比较，数值列按数值。
pub fn multi_sort_order(
    data: &[crate::db_types::Row],
    picks: &[usize],
    sorts: &[(usize, bool)],
    cols: &[ColMeta],
) -> Vec<usize> {
    let cell = |r: usize, c: usize| data.get(r).and_then(|row| row.get(c)).map(|s| s.as_str()).unwrap_or("");
    let mut order: Vec<usize> = picks.to_vec();
    order.sort_by(|&a, &b| {
        for (c, asc) in sorts {
            let numeric = is_numeric_col(cols, *c);
            let mut ord = crate::compare_cells(cell(a, *c), cell(b, *c), numeric);
            if !asc {
                ord = ord.reverse();
            }
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        a.cmp(&b)
    });
    order
}

/// 一列的去重值（建议值候选，保持首现顺序）。
pub fn distinct_values(data: &[crate::db_types::Row], col: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for row in data {
        if let Some(v) = row.get(col)
            && !v.trim().is_empty()
            && !out.iter().any(|o| o == v)
        {
            out.push(v.clone());
        }
    }
    out
}

// ── 反解析（文本模式往返） ─────────────────────────────────────
// 只认自己生成的文法：引号列名（".." / `..`）、'..' 字面量、AND / OR、
// 23 个算子的 SQL 形态。方言差异：ILIKE 只在 PG 侧出现并回读为
// 不区分大小写档；MySQL 侧 LIKE 恒回区分大小写档。

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Lit(String),
    LParen,
    RParen,
    And,
    Or,
    Not,
    Like,
    ILike,
    Is,
    Null,
    Between,
    In,
    Cmp(u8), // 0 = 1 < 2 <= 3 > 4 >= 5 !=/<>
    Comma,
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let kw = |w: &str| -> Option<Tok> {
        match w.to_ascii_uppercase().as_str() {
            "AND" => Some(Tok::And),
            "OR" => Some(Tok::Or),
            "NOT" => Some(Tok::Not),
            "LIKE" => Some(Tok::Like),
            "ILIKE" => Some(Tok::ILike),
            "IS" => Some(Tok::Is),
            "NULL" => Some(Tok::Null),
            "BETWEEN" => Some(Tok::Between),
            "IN" => Some(Tok::In),
            _ => None,
        }
    };
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '\'' => {
                // '...' 字面量，'' 转义
                let mut v = String::new();
                i += 1;
                loop {
                    if i >= cs.len() {
                        return Err("字符串缺少结束引号".into());
                    }
                    if cs[i] == '\'' {
                        if i + 1 < cs.len() && cs[i + 1] == '\'' {
                            v.push('\'');
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    } else {
                        v.push(cs[i]);
                        i += 1;
                    }
                }
                out.push(Tok::Lit(v));
            }
            '"' | '`' => {
                // 引号列名，"" / `` 转义
                let close = c;
                let mut v = String::new();
                i += 1;
                loop {
                    if i >= cs.len() {
                        return Err("列名缺少结束引号".into());
                    }
                    if cs[i] == close {
                        if i + 1 < cs.len() && cs[i + 1] == close {
                            v.push(close);
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    } else {
                        v.push(cs[i]);
                        i += 1;
                    }
                }
                out.push(Tok::Ident(v));
            }
            '<' => {
                if i + 1 < cs.len() && (cs[i + 1] == '>' || cs[i + 1] == '=') {
                    out.push(Tok::Cmp(if cs[i + 1] == '>' { 5 } else { 2 }));
                    i += 2;
                } else {
                    out.push(Tok::Cmp(1));
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < cs.len() && cs[i + 1] == '=' {
                    out.push(Tok::Cmp(4));
                    i += 2;
                } else {
                    out.push(Tok::Cmp(3));
                    i += 1;
                }
            }
            '=' => {
                out.push(Tok::Cmp(0));
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '!' if i + 1 < cs.len() && cs[i + 1] == '=' => {
                out.push(Tok::Cmp(5));
                i += 2;
            }
            _ if c.is_ascii_digit() => {
                // 数字字面量（含小数 / 负号后接数字由符号位单独处理不了——
                // 生成的负数字面量带引号，裸负数 mock 档不支持）
                let start = i;
                while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_' || cs[i] == '.')
                {
                    i += 1;
                }
                let w: String = cs[start..i].iter().collect();
                out.push(Tok::Lit(w));
            }
            _ if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                    i += 1;
                }
                let w: String = cs[start..i].iter().collect();
                out.push(kw(&w).unwrap_or(Tok::Ident(w)));
            }
            _ => return Err(format!("无法识别的字符「{c}」")),
        }
    }
    Ok(out)
}

fn col_index(cols: &[ColMeta], name: &str) -> Result<usize, String> {
    cols.iter()
        .position(|c| c.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("未知列「{name}」"))
}

/// LIKE 模式归类：`%x%` → 包含、`x%` → 开头是、`%x` → 结尾是、
/// 无 % → 等值、中间带 % 的复杂模式解析失败（mock 档不硬解）。
/// ILIKE 的不区分大小写只在「包含」档有对应算子，其余档忽略。
fn classify_like(cols: &[ColMeta], col: usize, v: &str, ci: bool, neg: bool) -> Result<FNode, String> {
    let op = match (v.starts_with('%'), v.ends_with('%'), v.len()) {
        (_, _, 0) => return Err("LIKE 缺少模式".into()),
        (true, true, 1) => return Err("只有通配符的 LIKE 模式".into()),
        (true, true, _) => if ci { FOp::ContainsCi } else { FOp::Contains },
        (false, true, _) => FOp::StartsWith,
        (true, false, _) => FOp::EndsWith,
        (false, false, _) => FOp::Eq,
    };
    let op = if neg {
        match op {
            FOp::Contains => FOp::NotContains,
            FOp::ContainsCi => FOp::NotContainsCi,
            FOp::StartsWith => FOp::NotStartsWith,
            FOp::EndsWith => FOp::NotEndsWith,
            FOp::Eq => FOp::Ne,
            o => o,
        }
    } else {
        op
    };
    let mut c = FCrit::new(cols, col);
    c.op = op;
    c.v1 = v.trim_matches('%').to_string();
    Ok(FNode::Crit(c))
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    cols: &'a [ColMeta],
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// 条目列表 + 连接词（AND = true）。top=顶层才查「末尾多余内容」，
    /// 括号内的子列表合法停在 RParen 上。
    fn parse_list(&mut self, top: bool) -> Result<FilterModel, String> {
        let mut m = FilterModel::default();
        let first = self.parse_pred()?;
        m.items.push(first);
        loop {
            if self.eat(&Tok::And) {
                m.conns.push(true);
                m.items.push(self.parse_pred()?);
            } else if self.eat(&Tok::Or) {
                m.conns.push(false);
                m.items.push(self.parse_pred()?);
            } else {
                break;
            }
        }
        if top && self.pos != self.toks.len() {
            return Err("条件末尾有多余内容".into());
        }
        Ok(m)
    }

    fn parse_pred(&mut self) -> Result<FNode, String> {
        if self.eat(&Tok::LParen) {
            let inner = self.parse_list(false)?;
            if !self.eat(&Tok::RParen) {
                return Err("括号不配对".into());
            }
            // 括号一律保留为组：单条目的括号（生成的单子组 / OR 簇括号）
            // 只有这样文本往返才不丢结构
            return Ok(FNode::Group {
                conns: inner.conns,
                children: inner.items,
            });
        }
        let Some(Tok::Ident(col)) = self.next() else {
            return Err("条件应当以列名开头".into());
        };
        let cix = col_index(self.cols, &col)?;
        let mut neg = false;
        if self.eat(&Tok::Not) {
            neg = true;
        }
        let mut c = FCrit::new(self.cols, cix);
        match self.next() {
            Some(Tok::Cmp(k)) => {
                let Some(Tok::Lit(v)) = self.next() else {
                    return Err("比较运算符缺少值".into());
                };
                c.op = match (k, v.as_str()) {
                    (0, "") => FOp::IsBlank,
                    (5, "") => FOp::IsNotBlank,
                    (0, _) => FOp::Eq,
                    (5, _) => FOp::Ne,
                    (1, _) => FOp::Lt,
                    (2, _) => FOp::Le,
                    (3, _) => FOp::Gt,
                    (4, _) => FOp::Ge,
                    _ => return Err("未知的比较运算符".into()),
                };
                if v.is_empty() && !matches!(c.op, FOp::IsBlank | FOp::IsNotBlank) {
                    return Err("比较值为空".into());
                }
                c.v1 = v;
            }
            Some(Tok::Like) => {
                let Some(Tok::Lit(v)) = self.next() else {
                    return Err("LIKE 缺少模式".into());
                };
                return classify_like(self.cols, cix, &v, false, neg);
            }
            Some(Tok::ILike) => {
                let Some(Tok::Lit(v)) = self.next() else {
                    return Err("ILIKE 缺少模式".into());
                };
                return classify_like(self.cols, cix, &v, true, neg);
            }
            Some(Tok::Is) => {
                if neg {
                    return Err("IS 前不能有 NOT".into());
                }
                let is_not = self.eat(&Tok::Not);
                if !self.eat(&Tok::Null) {
                    return Err("IS 后应为 NULL".into());
                }
                c.op = if is_not { FOp::IsNotNull } else { FOp::IsNull };
            }
            Some(Tok::Between) => {
                let Some(Tok::Lit(a)) = self.next() else {
                    return Err("BETWEEN 缺少下界".into());
                };
                if !self.eat(&Tok::And) {
                    return Err("BETWEEN 缺少 AND".into());
                }
                let Some(Tok::Lit(b)) = self.next() else {
                    return Err("BETWEEN 缺少上界".into());
                };
                c.op = if neg { FOp::NotBetween } else { FOp::Between };
                c.v1 = a;
                c.v2 = b;
            }
            Some(Tok::In) => {
                if !self.eat(&Tok::LParen) {
                    return Err("IN 缺少列表括号".into());
                }
                let mut items = Vec::new();
                loop {
                    match self.next() {
                        Some(Tok::Lit(v)) => items.push(v),
                        _ => return Err("IN 列表应为值".into()),
                    }
                    if self.eat(&Tok::RParen) {
                        break;
                    }
                    if !self.eat(&Tok::Comma) {
                        return Err("IN 列表应以逗号分隔".into());
                    }
                }
                c.op = if neg { FOp::NotInList } else { FOp::InList };
                c.v1 = items.join(", ");
            }
            _ => return Err("列名后应为运算符".into()),
        }
        Ok(FNode::Crit(c))
    }
}

/// 文本模式 WHERE 正文 → 模型。解析失败给中文原因（面板红字）。
pub fn parse_where(_kind: DbKind, cols: &[ColMeta], text: &str) -> Result<FilterModel, String> {
    let toks = tokenize(text)?;
    let mut p = Parser { toks, pos: 0, cols };
    p.parse_list(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::DbKind;

    fn crit(col: usize, op: FOp, v1: &str) -> FCrit {
        FCrit {
            col,
            op,
            v1: v1.into(),
            v2: String::new(),
            custom: String::new(),
            enabled: true,
        }
    }

    fn crit2(col: usize, op: FOp, v1: &str, v2: &str) -> FCrit {
        FCrit { v2: v2.into(), ..crit(col, op, v1) }
    }

    /// 与 mock 档同形的列元数据（测试口径）。
    fn cols() -> Vec<ColMeta> {
        crate::mock::MOCK_COLUMNS
            .iter()
            .map(|m| ColMeta::new(m.name, m.ty, m.numeric))
            .collect()
    }

    #[test]
    fn default_op_follows_column_type() {
        let c = cols();
        assert_eq!(default_op(&c, 3), FOp::Contains, "name varchar → 包含");
        assert_eq!(default_op(&c, 0), FOp::Eq, "id int8 → =");
        assert_eq!(default_op(&c, 7), FOp::Eq, "enabled bool → =");
        assert_eq!(default_op(&c, 99), FOp::Eq, "越界列防御 → =");
    }

    #[test]
    fn op_labels_match_reference_menu() {
        assert_eq!(ALL_OPS.len(), 23);
        assert_eq!(ALL_OPS[0].label(), "=");
        assert_eq!(ALL_OPS[1].label(), "!=");
        assert_eq!(ALL_OPS[6].label(), "包含");
        assert_eq!(ALL_OPS[7].label(), "包含（不区分大小写）");
        assert_eq!(ALL_OPS[14].label(), "是 null");
        assert_eq!(ALL_OPS[18].label(), "介于");
        assert_eq!(ALL_OPS[22].label(), "[自定义]");
    }

    #[test]
    fn crit_sql_pg_all_shapes() {
        let k = DbKind::PostgreSql;
        let c = cols();
        assert_eq!(crit_sql(k, &c, &crit(0, FOp::Eq, "42")), "\"id\" = 42");
        assert_eq!(crit_sql(k, &c, &crit(0, FOp::Ne, "7")), "\"id\" <> 7");
        assert_eq!(crit_sql(k, &c, &crit(3, FOp::Eq, "x")), "\"name\" = 'x'");
        assert_eq!(
            crit_sql(k, &c, &crit(3, FOp::Contains, "报")),
            "\"name\" LIKE '%报%'"
        );
        assert_eq!(
            crit_sql(k, &c, &crit(3, FOp::ContainsCi, "a")),
            "\"name\" ILIKE '%a%'"
        );
        assert_eq!(
            crit_sql(k, &c, &crit(3, FOp::StartsWith, "a")),
            "\"name\" LIKE 'a%'"
        );
        assert_eq!(
            crit_sql(k, &c, &crit(3, FOp::EndsWith, "a")),
            "\"name\" LIKE '%a'"
        );
        assert_eq!(crit_sql(k, &c, &crit(0, FOp::IsNull, "")), "\"id\" IS NULL");
        assert_eq!(crit_sql(k, &c, &crit(0, FOp::IsNotBlank, "")), "\"id\" <> ''");
        assert_eq!(
            crit_sql(k, &c, &crit2(0, FOp::Between, "1", "9")),
            "\"id\" BETWEEN 1 AND 9"
        );
        assert_eq!(
            crit_sql(k, &c, &crit2(0, FOp::NotBetween, "1", "9")),
            "\"id\" NOT BETWEEN 1 AND 9"
        );
        assert_eq!(
            crit_sql(k, &c, &crit(3, FOp::InList, "a, b,c")),
            "\"name\" IN ('a', 'b', 'c')"
        );
        assert_eq!(
            crit_sql(k, &c, &crit(3, FOp::NotInList, "a")),
            "\"name\" NOT IN ('a')"
        );
        assert_eq!(crit_sql(k, &c, &crit(3, FOp::InList, "")), "1 = 0");
        let mut cu = crit(0, FOp::Custom, "");
        cu.custom = "MOD(\"id\", 2) = 1".to_string();
        assert_eq!(crit_sql(k, &c, &cu), "MOD(\"id\", 2) = 1");
        // 单引号转义
        assert_eq!(crit_sql(k, &c, &crit(3, FOp::Eq, "o'x")), "\"name\" = 'o''x'");
    }

    #[test]
    fn crit_sql_mysql_backticks() {
        let k = DbKind::MySql;
        let c = cols();
        assert_eq!(crit_sql(k, &c, &crit(0, FOp::Eq, "42")), "`id` = 42");
        assert_eq!(crit_sql(k, &c, &crit(3, FOp::Contains, "a")), "`name` LIKE '%a%'");
        // MySQL 侧不区分大小写档与区分大小写档同文（方言无 ILIKE）
        assert_eq!(crit_sql(k, &c, &crit(3, FOp::ContainsCi, "a")), "`name` LIKE '%a%'");
    }

    #[test]
    fn where_sql_grouping_and_pruning() {
        let k = DbKind::PostgreSql;
        let c = cols();
        // 单条
        let m = FilterModel {
            items: vec![FNode::Crit(crit(0, FOp::Eq, "1"))],
            conns: vec![],
        };
        assert_eq!(where_sql(k, &c, &m).as_deref(), Some("\"id\" = 1"));
        // 全 AND：结成一个簇，单簇不加括号（文本往返保持平展模型）
        let m = FilterModel {
            items: vec![
                FNode::Crit(crit(0, FOp::Eq, "1")),
                FNode::Crit(crit(3, FOp::Eq, "a")),
            ],
            conns: vec![true],
        };
        assert_eq!(
            where_sql(k, &c, &m).as_deref(),
            Some("\"id\" = 1 AND \"name\" = 'a'")
        );
        // 混合连接词：AND 簇、OR 分隔，单条目簇加括号、多条件簇不重包
        let m = FilterModel {
            items: vec![
                FNode::Crit(crit(0, FOp::Eq, "1")),
                FNode::Crit(crit(3, FOp::Eq, "a")),
                FNode::Crit(crit(4, FOp::Eq, "b")),
            ],
            conns: vec![true, false],
        };
        assert_eq!(
            where_sql(k, &c, &m).as_deref(),
            Some("(\"id\" = 1 AND \"name\" = 'a') OR (\"url\" = 'b')")
        );
        // 组：恒加括号，组内连接词逐间隙
        let m = FilterModel {
            items: vec![
                FNode::Crit(crit(0, FOp::Eq, "1")),
                FNode::Group {
                    conns: vec![false],
                    children: vec![
                        FNode::Crit(crit(3, FOp::Eq, "a")),
                        FNode::Crit(crit(4, FOp::Eq, "b")),
                    ],
                },
            ],
            conns: vec![true],
        };
        assert_eq!(
            where_sql(k, &c, &m).as_deref(),
            Some("\"id\" = 1 AND (\"name\" = 'a' OR \"url\" = 'b')")
        );
        // 禁用准则剔除；全禁用 → None
        let mut off = crit(0, FOp::Eq, "1");
        off.enabled = false;
        let m = FilterModel {
            items: vec![
                FNode::Crit(off.clone()),
                FNode::Crit(crit(3, FOp::Eq, "a")),
            ],
            conns: vec![true],
        };
        assert_eq!(where_sql(k, &c, &m).as_deref(), Some("\"name\" = 'a'"));
        let m = FilterModel {
            items: vec![FNode::Crit(off)],
            conns: vec![],
        };
        assert_eq!(where_sql(k, &c, &m), None);
        assert_eq!(where_sql(k, &c, &FilterModel::default()), None);
    }

    #[test]
    fn order_by_sql_multi_key() {
        let k = DbKind::PostgreSql;
        let c = cols();
        assert_eq!(order_by_sql(k, &c, &[]), None);
        assert_eq!(
            order_by_sql(k, &c, &[(3, true), (0, false)]).as_deref(),
            Some(" ORDER BY \"name\" ASC, \"id\" DESC")
        );
        assert_eq!(
            order_by_sql(DbKind::MySql, &c, &[(0, true)]).as_deref(),
            Some(" ORDER BY `id` ASC")
        );
    }

    #[test]
    fn order_text_shapes_and_dialects() {
        let c = cols();
        // 基线三键形态：降序追加 DESC、升序省略关键字
        assert_eq!(
            order_text(DbKind::MySql, &c, &[(3, false), (0, true), (6, true)]),
            "`name` DESC, `id`, `flags`"
        );
        // PG / 其它方言双引号
        assert_eq!(
            order_text(DbKind::PostgreSql, &c, &[(3, false), (0, true), (6, true)]),
            "\"name\" DESC, \"id\", \"flags\""
        );
        // 越界列下标跳过；空 sorts → 空串
        assert_eq!(order_text(DbKind::MySql, &c, &[(99, true), (0, false)]), "`id` DESC");
        assert_eq!(order_text(DbKind::MySql, &c, &[]), "");
        assert_eq!(order_text(DbKind::MySql, &c, &[(99, true)]), "");
    }

    #[test]
    fn order_text_round_trip() {
        let c = cols();
        for k in [DbKind::MySql, DbKind::PostgreSql] {
            // order_text 输出再 parse 得到原 sorts
            let sorts = vec![(3, false), (0, true), (6, false), (4, true)];
            let text = order_text(k, &c, &sorts);
            let back = parse_order(k, &c, &text).unwrap_or_else(|e| panic!("解析失败 {e}: {text}"));
            assert_eq!(back, sorts);
            // 空文本 / 空白文本 → 空序列
            assert_eq!(parse_order(k, &c, "").unwrap(), vec![]);
            assert_eq!(parse_order(k, &c, "   ").unwrap(), vec![]);
        }
        // 手写文本：asc/desc 大小写混合、裸词列名（保留字除外）均可解析
        let k = DbKind::PostgreSql;
        assert_eq!(
            parse_order(k, &c, "\"name\" desc, \"id\" AsC").unwrap(),
            vec![(3, false), (0, true)]
        );
        assert_eq!(parse_order(k, &c, "name DESC,id").unwrap(), vec![(3, false), (0, true)]);
        // 未知列大小写不敏感命中
        assert_eq!(parse_order(k, &c, "\"NAME\"").unwrap(), vec![(3, true)]);
    }

    #[test]
    fn order_text_rejects_garbage() {
        let k = DbKind::PostgreSql;
        let c = cols();
        assert!(parse_order(k, &c, "SELECT 1").is_err(), "垃圾文本");
        assert!(parse_order(k, &c, "\"nosuch\" DESC").is_err(), "未知列");
        assert!(parse_order(k, &c, "\"name\" DESC,").is_err(), "悬空逗号");
        assert!(parse_order(k, &c, ",\"name\"").is_err(), "前导逗号");
        assert!(parse_order(k, &c, "\"name\" \"id\"").is_err(), "逗号缺失");
        assert!(parse_order(k, &c, "\"name\" DESC \"id\"").is_err(), "逗号缺失（带方向）");
        assert!(parse_order(k, &c, "DESC").is_err(), "方向保留字当列名");
        assert!(parse_order(k, &c, "\"name\" = 1").is_err(), "非排序文法");
        assert!(parse_order(k, &c, "\"name").is_err(), "列名缺少结束引号（tokenize 报）");
        assert!(parse_order(k, &c, "1").is_err(), "数字开头");
    }

    #[test]
    fn text_mode_round_trip() {
        let k = DbKind::PostgreSql;
        let c = cols();
        // 平展模型（无括号歧义）：往返后模型逐字段一致
        let flat = FilterModel {
            items: vec![
                FNode::Crit(crit(0, FOp::Gt, "5")),
                FNode::Crit(crit(3, FOp::Contains, "报")),
            ],
            conns: vec![true],
        };
        let sql = where_sql(k, &c, &flat).unwrap();
        let back = parse_where(k, &c, &sql).unwrap_or_else(|e| panic!("解析失败 {e}: {sql}"));
        assert_eq!(back, flat);
        // 混合连接词的平展模型会被簇括号规整成组：SQL 幂等 + 行判定等价
        let mixed = FilterModel {
            items: vec![
                FNode::Crit(crit(0, FOp::Le, "10")),
                FNode::Crit(crit(3, FOp::StartsWith, "用")),
                FNode::Crit(crit(4, FOp::EndsWith, "s")),
            ],
            conns: vec![false, true],
        };
        let sql = where_sql(k, &c, &mixed).unwrap();
        let norm = parse_where(k, &c, &sql).unwrap_or_else(|e| panic!("解析失败 {e}: {sql}"));
        let sql2 = where_sql(k, &c, &norm).expect("回读后再生成");
        assert_eq!(sql2, sql, "生成不幂等: {sql}");
        let norm2 = parse_where(k, &c, &sql2).unwrap();
        assert_eq!(norm2, norm, "第二次往返模型稳定");
        // 组模型（含不区分大小写 / NOT / null / between / in）：发射器会把
        // AND 簇规整加括号，回读是**等价**模型而非逐字段相同——
        // 断言 SQL 幂等 + 模型稳定 + 行判定等价
        let grouped = FilterModel {
            items: vec![
                FNode::Group {
                    conns: vec![false],
                    children: vec![
                        FNode::Crit(crit(3, FOp::ContainsCi, "a")),
                        FNode::Crit(crit(3, FOp::NotContains, "b")),
                    ],
                },
                FNode::Crit(crit(0, FOp::IsNull, "")),
                FNode::Crit(crit(3, FOp::IsNotBlank, "")),
                FNode::Crit(crit2(0, FOp::Between, "1", "99")),
                FNode::Crit(crit(3, FOp::InList, "a, b")),
            ],
            conns: vec![true, false, true, true],
        };
        let sql = where_sql(k, &c, &grouped).unwrap();
        let norm = parse_where(k, &c, &sql).unwrap_or_else(|e| panic!("解析失败 {e}: {sql}"));
        let sql2 = where_sql(k, &c, &norm).expect("回读后再生成");
        assert_eq!(sql2, sql, "生成不幂等: {sql}");
        let norm2 = parse_where(k, &c, &sql2).unwrap();
        assert_eq!(norm2, norm, "第二次往返模型稳定");
        let row = |id: &str, name: &str| -> Vec<String> {
            let mut r: Vec<String> = (0..11).map(|_| String::new()).collect();
            r[0] = id.into();
            r[3] = name.into();
            r
        };
        for (id, name) in
            [("1", "abc"), ("", "b"), ("50", "banana"), ("", "apple"), ("50", "a")]
        {
            let r = row(id, name);
            assert_eq!(
                row_matches(&r, &c, &grouped),
                row_matches(&r, &c, &norm),
                "行判定等价 id={id:?} name={name:?}"
            );
        }
        // MySQL 侧 LIKE 往返归到区分大小写档（方言无 ILIKE，mock 档记录在案）
        let km = DbKind::MySql;
        let ci = FilterModel {
            items: vec![FNode::Crit(crit(3, FOp::ContainsCi, "a"))],
            conns: vec![],
        };
        let sql = where_sql(km, &c, &ci).unwrap();
        let back = parse_where(km, &c, &sql).unwrap();
        assert_eq!(back.items[0], FNode::Crit(crit(3, FOp::Contains, "a")));
    }

    #[test]
    fn text_mode_rejects_garbage() {
        let k = DbKind::PostgreSql;
        let c = cols();
        assert!(parse_where(k, &c, "SELECT 1").is_err());
        assert!(parse_where(k, &c, "\"nosuch\" = 1").is_err(), "未知列");
        assert!(parse_where(k, &c, "\"id\" =").is_err(), "缺值");
        assert!(parse_where(k, &c, "\"id\" = 1 AND").is_err(), "悬空连接词");
        assert!(parse_where(k, &c, "\"id\" = 'x' extra").is_err(), "尾部多余");
        assert!(parse_where(k, &c, "(\"id\" = 1").is_err(), "括号不配对");
    }

    #[test]
    fn row_matching_semantics() {
        let row: Vec<String> = ["101", "0", "13", "首页面板", "/home", "HomePanel", "1", "t", "0", "1", "1770000000000"]
            .into_iter()
            .map(String::from)
            .collect();
        let c = cols();
        // 数值列数值比较
        let m = FilterModel {
            items: vec![FNode::Crit(crit(0, FOp::Lt, "200"))],
            conns: vec![],
        };
        assert!(row_matches(&row, &c, &m));
        let m = FilterModel {
            items: vec![FNode::Crit(crit(0, FOp::Lt, "100"))],
            conns: vec![],
        };
        assert!(!row_matches(&row, &c, &m));
        // "9" 与 "101" 数值比 9 < 101（字符串比会反过来）
        let m = FilterModel {
            items: vec![FNode::Crit(crit(0, FOp::Gt, "9"))],
            conns: vec![],
        };
        assert!(row_matches(&row, &c, &m));
        // 文本包含
        let m = FilterModel {
            items: vec![FNode::Crit(crit(3, FOp::Contains, "面板"))],
            conns: vec![],
        };
        assert!(row_matches(&row, &c, &m));
        // AND 簇 + OR
        let m = FilterModel {
            items: vec![
                FNode::Crit(crit(0, FOp::Gt, "500")),
                FNode::Crit(crit(3, FOp::Contains, "面板")),
            ],
            conns: vec![false],
        };
        assert!(row_matches(&row, &c, &m), "OR 右臂命中");
        let m = FilterModel {
            items: vec![
                FNode::Crit(crit(0, FOp::Gt, "500")),
                FNode::Crit(crit(3, FOp::Contains, "不存在的词")),
            ],
            conns: vec![false],
        };
        assert!(!row_matches(&row, &c, &m));
        // 组 OR
        let m = FilterModel {
            items: vec![FNode::Group {
                conns: vec![false],
                children: vec![
                    FNode::Crit(crit(0, FOp::Eq, "999")),
                    FNode::Crit(crit(4, FOp::Eq, "/home")),
                ],
            }],
            conns: vec![],
        };
        assert!(row_matches(&row, &c, &m));
        // 禁用准则视为通过
        let mut off = crit(0, FOp::Lt, "1");
        off.enabled = false;
        let m = FilterModel {
            items: vec![FNode::Crit(off), FNode::Crit(crit(4, FOp::Eq, "/home"))],
            conns: vec![true],
        };
        assert!(row_matches(&row, &c, &m));
        // 空单元格（null/空串）
        let empty_row: Vec<String> = vec![String::new(); 11];
        let m = FilterModel {
            items: vec![FNode::Crit(crit(3, FOp::IsNull, ""))],
            conns: vec![],
        };
        assert!(row_matches(&empty_row, &c, &m));
        // 自定义准则不参与判定（恒通过）
        let mut cu = crit(0, FOp::Custom, "");
        cu.custom = "MOD(\"id\", 2) = 1".to_string();
        let m = FilterModel {
            items: vec![FNode::Crit(cu), FNode::Crit(crit(4, FOp::Eq, "/home"))],
            conns: vec![true],
        };
        assert!(row_matches(&row, &c, &m));
    }

    #[test]
    fn multi_key_sort_and_distinct() {
        let data: Vec<crate::db_types::Row> = (0..12)
            .map(|r| {
                let mut row: Vec<String> = (0..11)
                    .map(|c| mock_cell_of(r, c))
                    .collect();
                row[6] = (r % 3).to_string(); // flags 0/1/2 循环
                row[0] = (100 - r).to_string(); // id 递减
                std::sync::Arc::new(row)
            })
            .collect();
        let c = cols();
        // flags ASC 主键、id DESC 次键
        let order = multi_sort_order(&data, &(0..12).collect::<Vec<_>>(), &[(6, true), (0, false)], &c);
        assert_eq!(order.len(), 12);
        let key = |i: usize| (data[i][6].parse::<i64>().unwrap(), data[i][0].parse::<i64>().unwrap());
        for w in order.windows(2) {
            let (a, b) = (key(w[0]), key(w[1]));
            assert!(a.0 < b.0 || (a.0 == b.0 && a.1 >= b.1), "{a:?} 应排在 {b:?} 前");
        }
        // 稳定：同键保持 picks 原相对次序（这里 id 次键已唯一，用单键验证稳定性）
        let order = multi_sort_order(&data, &(0..12).collect::<Vec<_>>(), &[(6, true)], &c);
        for w in order.windows(2) {
            let a = data[w[0]][6].parse::<i64>().unwrap();
            let b = data[w[1]][6].parse::<i64>().unwrap();
            assert!(a <= b);
        }

        let col = 3; // name 列 20 个词循环，12 行内应有重复
        let dv = distinct_values(&data, col);
        assert!(!dv.is_empty());
        let mut seen = Vec::new();
        for v in &dv {
            assert!(!seen.contains(v), "去重失败: {v}");
            seen.push(v.clone());
        }
    }

    // 12 行的小数据用 mock_cell 的同款散列口径太绕，这里直接给静态表
    fn mock_cell_of(row: usize, col: usize) -> String {
        match col {
            0 => (100 - row).to_string(),
            1 => "0".into(),
            2 => "0".into(),
            3 => ["首页面板", "用户中心", "内容管理", "消息通知"][row % 4].into(),
            4 => format!("/slug{row}"),
            5 => "Icon".into(),
            6 => (row % 3).to_string(),
            7 => "t".into(),
            8 => "0".into(),
            9 => "1".into(),
            _ => "1770000000000".into(),
        }
    }
}
