//! 外壳用的假数据：对齐 Flutter 版主界面默认看到的那一套
//! （分组 → 连接 → 库 → 对象网格），暂不接真实驱动。

use std::sync::LazyLock;

#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DbKind {
    MySql,
    PostgreSql,
    SqlServer,
    SQLite,
    Access,
    MongoDb,
    Redis,
}

impl DbKind {
    pub fn label(self) -> &'static str {
        match self {
            DbKind::MySql => "MySQL",
            DbKind::PostgreSql => "PostgreSQL",
            DbKind::SqlServer => "SQL Server",
            DbKind::SQLite => "SQLite",
            DbKind::Access => "Access",
            DbKind::MongoDb => "MongoDB",
            DbKind::Redis => "Redis",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            DbKind::MySql => 3306,
            DbKind::PostgreSql => 5432,
            DbKind::SqlServer => 1433,
            DbKind::SQLite | DbKind::Access => 0,
            DbKind::MongoDb => 27017,
            DbKind::Redis => 6379,
        }
    }

    pub fn default_user(self) -> &'static str {
        match self {
            DbKind::MySql => "root",
            DbKind::PostgreSql => "postgres",
            DbKind::SqlServer => "sa",
            DbKind::SQLite | DbKind::Access | DbKind::MongoDb | DbKind::Redis => "",
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Connection {
    pub name: String,
    pub kind: DbKind,

    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// 密码仅存在于内存：daro-rs 还没有落盘凭据的存储层，
    /// 接入驱动前也没有读取方，先随连接一起带着
    #[allow(dead_code)]
    pub password: String,
    pub group: String,
    pub connected: bool,
    /// 这条连接只提供**演示数据**，不去连真库。
    ///
    /// 引擎接上真驱动后（MySQL / PostgreSQL / SQLite），演示条目如果照旧
    /// 去连那台「演示主机」，点开就是一串连接错误 —— 演示树就没了。所以
    /// 演示条目显式标出来：树的层级、方言、字段结构都是真的，只有数据是假的。
    ///
    /// 用户自己新建的连接恒为 false（连真库）；这个标记可以在连接对话框里改。
    #[serde(default)]
    pub demo: bool,
}

impl Connection {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        kind: DbKind,
        host: impl Into<String>,
        port: u16,
        database: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
        group: impl Into<String>,
        connected: bool,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            host: host.into(),
            port,
            database: database.into(),
            user: user.into(),
            password: password.into(),
            group: group.into(),
            connected,
            demo: false,
        }
    }

    /// 把这条连接标成「只给演示数据」。
    pub fn as_demo(mut self) -> Self {
        self.demo = true;
        self
    }
}

/// 演示连接树：只服务测试夹具，生产版首次启动不预置任何连接。
#[cfg(test)]
pub fn connections() -> Vec<Connection> {
    vec![
        Connection::new("本地 MySQL", DbKind::MySql, "127.0.0.1", 3306, "bx11", "root", "", "开发环境", true).as_demo(),
        Connection::new("测试 SQL Server", DbKind::SqlServer, "192.168.10.21", 1433, "master", "sa", "", "测试环境", false),
        Connection::new("测试 MongoDB", DbKind::MongoDb, "192.168.10.22", 27017, "admin", "", "", "测试环境", false),
        Connection::new("缓存 Redis", DbKind::Redis, "127.0.0.1", 6379, "db0", "", "", "测试环境", true),
    ]
}

/// 连接展开后列出的数据库。参考界面按名称排序，初始库并不置顶，
/// 只是图标颜色不同（打开过的绿、没打开过的灰）。
pub fn databases(kind: DbKind) -> &'static [&'static str] {
    match kind {
        DbKind::MySql => &[
            "bx11",
            "bx_mall",
            "information_schema",
            "mysql",
            "performance_schema",
            "sys",
        ],
        DbKind::PostgreSql => &[
            "daro_dev",
            "datawarehouse",
            "dw",
            "postgres",
            "reporting",
        ],
        DbKind::SqlServer => &["daro", "master", "model", "msdb", "tempdb"],
        DbKind::MongoDb => &["admin", "config", "daro", "local"],
        DbKind::Redis => &["db0", "db1", "db2", "db3"],
        // SQLite / Access 档的库清单走 catalog（按连接的文件出），mock 档给空
        DbKind::SQLite | DbKind::Access => &[],
    }
}

/// 数据库下面是否有「模式」这一层。实测参考界面：PostgreSQL 有
/// （public 等），SQL Server 也有（dbo 等），MySQL 系没有，库即模式。
pub fn has_schemas(kind: DbKind) -> bool {
    matches!(kind, DbKind::PostgreSql | DbKind::SqlServer)
}

pub fn schemas(kind: DbKind) -> &'static [&'static str] {
    match kind {
        DbKind::PostgreSql => &["public"],
        DbKind::SqlServer => &["dbo"],
        _ => &[],
    }
}

/// 系统库不参与统计，也不该长出业务表。
const SYSTEM_DBS: [&str; 10] = [
    "information_schema",
    "mysql",
    "performance_schema",
    "sys",
    "master",
    "model",
    "msdb",
    "tempdb",
    "config",
    "local",
];

pub fn is_system_db(db: &str) -> bool {
    SYSTEM_DBS.contains(&db)
}

/// 库名散列出一个稳定的对象数：同一个库每次展开数量一致，不同库有差异，
/// 免得整棵树上每个库都挂着同一份 113 张表。
fn spread(db: &str, max: usize) -> usize {
    if is_system_db(db) {
        return 0;
    }
    let mut h: u32 = 2166136261;
    for b in db.as_bytes() {
        h = (h ^ *b as u32).wrapping_mul(16777619);
    }
    (h as usize) % (max + 1)
}

/// 某个库某一分下的对象清单。连接自己的初始库给全量，其余按库名截断。
pub fn objects(conn: &Connection, db: &str, cat: usize) -> &'static [&'static str] {
    let pool: &'static [&'static str] = match cat {
        0 => &*SORTED_TABLES,
        1 => &VIEWS,
        2 => &MATERIALIZED_VIEWS,
        3 => &ROUTINES,
        _ => &QUERIES,
    };
    if db == conn.database && !conn.database.is_empty() {
        return pool;
    }
    &pool[..spread(db, pool.len())]
}

/// 演示连接的账号清单（照参考界面实测的 `mysql.user` 排序：先按用户名，
/// 同名再按 host）。真连接走驱动查，这里只服务演示树。
const USERS: [&str; 12] = [
    "agent@%",
    "aliyun_root@127.0.0.1",
    "aurora@%",
    "bx11read@%",
    "bxread@%",
    "mysql.session@localhost",
    "mysql.sys@localhost",
    "prod@%",
    "rds_service@%",
    "root@%",
    "root@::1",
    "root@127.0.0.1",
];

pub fn users() -> &'static [&'static str] {
    &USERS
}

/// 演示连接的 PG 角色清单：照参考界面截图那台集群（PG 17 的预定义角色 +
/// `postgres`，共 17 条），顺序也按 `ORDER BY rolname` 排好。
/// MySQL 那边是 `mysql.user` 的「用户名@主机」形态，PG 只有角色名。
const PG_ROLES: [&str; 17] = [
    "pg_checkpoint",
    "pg_create_subscription",
    "pg_database_owner",
    "pg_execute_server_program",
    "pg_maintain",
    "pg_monitor",
    "pg_read_all_data",
    "pg_read_all_settings",
    "pg_read_all_stats",
    "pg_read_server_files",
    "pg_signal_autovacuum_worker",
    "pg_signal_backend",
    "pg_stat_scan_tables",
    "pg_use_reserved_connections",
    "pg_write_all_data",
    "pg_write_server_files",
    "postgres",
];

pub fn pg_roles() -> &'static [&'static str] {
    &PG_ROLES
}

/// 演示库里的定时事件。参考界面只有 MySQL / MariaDB 的事件清单，
/// 其它方言在面板里按「该连接类型没有事件」提示。
const DEMO_EVENTS: [&str; 2] = ["ev_archive_sys_log", "ev_purge_expired_sessions"];

pub fn events(kind: DbKind) -> &'static [&'static str] {
    match kind {
        DbKind::MySql => &DEMO_EVENTS,
        _ => &[],
    }
}

/// 演示库的表空间：MySQL 8 自带系统 / 临时 / mysql / sys 四个
/// （`information_schema.TABLESPACES` 的口径），PostgreSQL 是安装时
/// 固定有的两个默认表空间。
const DEMO_MYSQL_TABLESPACES: [&str; 4] = ["innodb_system", "innodb_temporary", "mysql", "sys"];
const DEMO_PG_TABLESPACES: [&str; 2] = ["pg_default", "pg_global"];

pub fn tablespaces(kind: DbKind) -> &'static [&'static str] {
    match kind {
        DbKind::MySql => &DEMO_MYSQL_TABLESPACES,
        DbKind::PostgreSql => &DEMO_PG_TABLESPACES,
        _ => &[],
    }
}

/// 对象网格里的表（daro 演示库有 108 张表）。
pub const TABLES: [&str; 113] = [
    "sys_user",
    "sys_role",
    "sys_menu",
    "sys_dept",
    "sys_dict",
    "sys_dict_item",
    "sys_config",
    "sys_log",
    "sys_login_log",
    "sys_oper_log",
    "sys_job",
    "sys_job_log",
    "sys_file",
    "sys_notice",
    "sys_post",
    "sys_user_role",
    "sys_role_menu",
    "sys_role_dept",
    "sys_user_post",
    "sys_tenant",
    "sys_tenant_package",
    "sys_oauth_client",
    "sys_oauth_token",
    "sys_sms_log",
    "sys_mail_log",
    "sys_area",
    "sys_sensitive_word",
    "sys_blacklist",
    "sys_whitelist",
    "sys_sequence",
    "bx_member",
    "bx_member_level",
    "bx_member_tag",
    "bx_member_address",
    "bx_member_point",
    "bx_member_wallet",
    "bx_member_coupon",
    "bx_member_sign",
    "bx_member_invite",
    "bx_member_balance_log",
    "bx_member_login_log",
    "bx_member_grade_log",
    "bx_wxcoupon",
    "bx_wxcoupon_mch",
    "bx_wxcoupon_record",
    "bx_wxcoupon_stock",
    "bx_wxuser",
    "bx_wxuser_tag",
    "bx_wxmenu",
    "bx_wxmsg",
    "bx_wxpay_order",
    "bx_wxpay_refund",
    "bx_wxrefund_log",
    "bx_wxtemplate",
    "bx_order",
    "bx_order_item",
    "bx_order_delivery",
    "bx_order_invoice",
    "bx_order_refund",
    "bx_order_comment",
    "bx_order_log",
    "bx_order_settle",
    "bx_order_split",
    "bx_order_presale",
    "bx_cart",
    "bx_cart_item",
    "bx_goods",
    "bx_goods_sku",
    "bx_goods_attr",
    "bx_goods_attr_value",
    "bx_goods_category",
    "bx_goods_brand",
    "bx_goods_spec",
    "bx_goods_spec_value",
    "bx_goods_image",
    "bx_goods_comment",
    "bx_goods_price_log",
    "bx_goods_stock",
    "bx_goods_stock_log",
    "bx_goods_collect",
    "bx_goods_browse_log",
    "bx_pay_channel",
    "bx_pay_log",
    "bx_pay_merchant",
    "bx_pay_config",
    "bx_pay_notify_log",
    "bx_delivery_company",
    "bx_delivery_template",
    "bx_delivery_region",
    "bx_express_log",
    "bx_shop",
    "bx_shop_config",
    "bx_shop_settle",
    "bx_shop_deposit_log",
    "bx_shop_staff",
    "bx_shop_apply",
    "bx_promo_activity",
    "bx_promo_seckill",
    "bx_promo_groupbuy",
    "bx_promo_coupon",
    "bx_promo_point",
    "bx_promo_bargain",
    "bx_promo_lottery",
    "bx_stock_cert",
    "bx_stock_cert_file",
    "bx_stock_batch",
    "bx_supplier",
    "bx_purchase_order",
    "bx_purchase_item",
    "bx_report_daily",
    "bx_report_monthly",
    "bx_report_shop",
    "bx_report_goods",
];

/// 对象列表按名称排序（参考界面的列表就是字母序），排一次存着，别每帧再排
pub static SORTED_TABLES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut v = TABLES.to_vec();
    v.sort_unstable();
    v
});

pub const VIEWS: [&str; 6] = [
    "v_member_order",
    "v_goods_stock",
    "v_order_daily",
    "v_shop_summary",
    "v_coupon_stat",
    "v_pay_stat",
];

pub const MATERIALIZED_VIEWS: [&str; 4] = [
    "mv_order_daily",
    "mv_member_growth",
    "mv_shop_rank",
    "mv_coupon_usage",
];

/// 「查询」分类下保存的 SQL 片段。
pub const QUERIES: [&str; 5] = [
    "最近 7 天订单",
    "库存低于安全线",
    "未结算店铺",
    "优惠券核销率",
    "会员增长月报",
];

pub const ROUTINES: [&str; 8] = [
    "sp_close_expired_order",
    "sp_settle_shop",
    "sp_grant_coupon",
    "fn_order_amount",
    "fn_member_level",
    "fn_goods_price",
    "trg_order_insert",
    "trg_stock_update",
];

/// 打开表数据页的假列。列形（名称 / 类型 / 列宽）照参考界面打开
/// PostgreSQL 表的实测复制的，全部表共用——行内容本来就是虚构的。
pub struct MockColumn {
    pub name: &'static str,
    pub ty: &'static str,
    /// 数值列右对齐（与 Flutter 版数据网格同一口径）
    pub numeric: bool,
    pub width: f32,
}

pub const MOCK_COLUMNS: [MockColumn; 11] = [
    MockColumn { name: "id", ty: "int8", numeric: true, width: 64. },
    MockColumn { name: "org_id", ty: "int8", numeric: true, width: 58. },
    MockColumn { name: "parent_id", ty: "int8", numeric: true, width: 62. },
    MockColumn { name: "name", ty: "varchar(32)", numeric: false, width: 96. },
    MockColumn { name: "url", ty: "varchar(32)", numeric: false, width: 112. },
    MockColumn { name: "icon", ty: "varchar(32)", numeric: false, width: 92. },
    MockColumn { name: "flags", ty: "int4", numeric: true, width: 48. },
    MockColumn { name: "enabled", ty: "bool", numeric: false, width: 56. },
    MockColumn { name: "priority", ty: "float8", numeric: true, width: 56. },
    MockColumn { name: "create_by", ty: "int8", numeric: true, width: 60. },
    MockColumn { name: "create_at", ty: "int8", numeric: true, width: 118. },
];

/// 假行里 name / url / icon 三列共用一组业务词，按下标错开取。
const MOCK_NAMES: [&str; 20] = [
    "首页面板", "用户中心", "内容管理", "消息通知", "财务管理", "库存中心", "报表统计",
    "权限配置", "日志审计", "系统设置", "接口服务", "任务调度", "审批中心", "合同管理",
    "客户档案", "供应商", "商品目录", "订单中心", "营销活动", "门店管理",
];

const MOCK_SLUGS: [&str; 20] = [
    "home", "users", "content", "messages", "finance", "inventory", "reports", "permissions",
    "audit", "settings", "api", "jobs", "approvals", "contracts", "customers", "suppliers",
    "catalog", "orders", "campaigns", "stores",
];

const MOCK_ICONS: [&str; 20] = [
    "HomePanel", "UserCog", "ListTree", "MessageBox", "WalletCard", "BoxesGrid", "ChartPie",
    "ShieldCheck", "ScrollText", "GearSetting", "ApiPlug", "TimerJobs", "StampApproval",
    "FileSign", "IdCard", "TruckLine", "TagsFill", "BagOrder", "Megaphone", "StoreShop",
];

fn table_hash(table: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in table.as_bytes() {
        h = (h ^ *b as u32).wrapping_mul(16777619);
    }
    h
}

/// 每张表的假行数：8..48，同一张表恒定，不同表有差异。
pub fn mock_row_count(table: &str) -> usize {
    8 + (table_hash(table) % 40) as usize
}

/// 第 row 行第 col 列的假值。id 从表名散列出的基值起连续递增，
/// parent_id 指回基值或更早的行，仿真树形业务数据的样子。
pub fn mock_cell(table: &str, row: usize, col: usize) -> String {
    let h = table_hash(table) as usize;
    let base = 100 + h % 900;
    match col {
        0 => (base + row).to_string(),
        1 => "0".to_string(),
        2 => {
            if row == 0 || row % 4 == 0 {
                "0".to_string()
            } else {
                (base + (row * 13 + h) % row).to_string()
            }
        }
        3 => MOCK_NAMES[(h + row) % MOCK_NAMES.len()].to_string(),
        4 => format!("/{}", MOCK_SLUGS[(h + row) % MOCK_SLUGS.len()]),
        5 => MOCK_ICONS[(h + row) % MOCK_ICONS.len()].to_string(),
        6 => (row % 3).to_string(),
        7 => if row % 2 == 0 { "t" } else { "f" }.to_string(),
        8 => "0".to_string(),
        9 => "1".to_string(),
        10 => (1_770_000_000_000i64 + row as i64 * 60_000).to_string(),
        _ => String::new(),
    }
}

/// 打开表时状态栏里展示的取数 SQL，照参考界面的方言格式拼；
/// 筛选面板应用的 WHERE 与多键排序按方言引号拼进子句
/// （MongoDB / Redis 无 ORDER BY 语法，照 MySQL 系反引号口径走——
/// mock 档这三类引擎共用同一套取数文案）。
pub fn mock_select_sql(
    kind: DbKind,
    cols: &[crate::db_types::ColMeta],
    db: &str,
    schema: &str,
    table: &str,
    where_sql: Option<&str>,
    orders: &[(usize, bool)],
) -> String {
    let where_part = where_sql.map(|w| format!(" WHERE {w}")).unwrap_or_default();
    let order = match orders {
        [] => String::new(),
        keys => {
            let cols: Vec<String> = keys
                .iter()
                .filter_map(|(c, asc)| {
                    cols.get(*c).map(|m| {
                        let dir = if *asc { "ASC" } else { "DESC" };
                        match kind {
                            DbKind::MySql | DbKind::MongoDb | DbKind::Redis => {
                                format!("`{}` {dir}", m.name)
                            }
                            DbKind::PostgreSql | DbKind::SqlServer | DbKind::SQLite => {
                                format!("\"{}\" {dir}", m.name)
                            }
                            DbKind::Access => format!("[{}] {dir}", m.name),
                        }
                    })
                })
                .collect();
            format!(" ORDER BY {}", cols.join(", "))
        }
    };
    match kind {
        DbKind::MySql | DbKind::MongoDb | DbKind::Redis => {
            format!("SELECT * FROM `{db}`.`{table}`{where_part}{order} LIMIT 0, 1000")
        }
        // SQLite 是单文件库：没有库名前缀，直接表名
        DbKind::SQLite => {
            format!("SELECT * FROM \"{table}\"{where_part}{order} LIMIT 1000 OFFSET 0")
        }
        DbKind::PostgreSql | DbKind::SqlServer => {
            format!("SELECT * FROM \"{schema}\".\"{table}\"{where_part}{order} LIMIT 1000 OFFSET 0")
        }
        // Access 不跑 SQL：状态栏给的是等价的整表读取口径
        DbKind::Access => format!("SELECT * FROM [{table}]{where_part}{order}"),
    }
}
