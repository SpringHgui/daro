//! macOS 系统菜单栏（NSMainMenu）：菜单树 + 命令分发。
//!
//! 窗口内菜单条（`menu_bar`）只在 Windows / Linux 上画；macOS 上整条菜单
//! 挂到系统顶栏，窗口首帧和状态指纹变化时整份 `set_menus` 替换一次。
//!
//! gpui 的系统菜单没有「打开前」回调，勾选 / 置灰 / 动态条目只能重建；
//! AppKit 默认还会在菜单打开时按「这个动作有没有处理器」重算每一项的
//! 启停，把烘进去的状态全部覆盖，所以每次装菜单都顺手关掉整棵 NSMenu 的
//! 自动启停（`disable_menu_autoenables`），状态以这里烘的为准。
//!
//! 条目尽量复用已有的全局动作（⌘S / ⌘R / ⌘W 这些键位提示由键位系统按
//! 动作查绑定自动补）；其余命令走一个带载荷的 `MenuCmd`，点击时再按
//! `arg` 回查当前状态，不把下标烘死在菜单里。

// 菜单树的构建半份各平台都编译：macOS 装进系统菜单栏，其它平台只有单测
// 在用（分发半份 wire_menu_actions 全平台装配）。
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use gpui_kit::component::input as ed;
use gpui_kit::*;

use crate::{
    Act, CloseAllTabs, CloseTab, DaroApp, EditFindNext, EditSelectStmt, GridCopy, GridPaste,
    GridRedo, GridSelectAll, GridUndo, MinimizeWindow, OpenExtensions, OpenOptions, QuitApp,
    QueryFind, QueryReplace, QueryRun, QueryRunStmt, QueryStop, RefreshView, Save,
    ToggleFullscreen,
    LEFT_W, RIGHT_W, RUN_PANE_LABELS,
};

/// 第一栏（应用菜单）的名字。macOS 会把它画成程序名那一栏。
pub(crate) const APP_MENU_NAME: &str = "daro-rs";

/// 系统菜单栏的一条命令。`arg` 的含义按命令而定（已保存查询 / 收藏槽位 /
/// 结果页下标 / 排序列 / 引号字符），点击时再回查状态，菜单里不烘死对象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Cmd {
    About,
    CheckUpdates,
    ThemePicker,
    Feedback,
    Hide,
    HideOthers,
    ShowAll,
    ResetPaneWidth,
    NewConnection,
    NewQuery,
    NewTable,
    NewView,
    NewFunction,
    ImportConnections,
    ExportConnections,
    OpenSqlFile,
    RunSqlFile,
    OpenSavedQuery,
    DeleteSavedQuery,
    SaveAs,
    ExportResult,
    CloseWindow,
    ToggleTree,
    ToggleInfo,
    InfoPage,
    SetListView,
    ResultPane,
    SetRunPane,
    ToggleTheme,
    Prettify,
    QuoteCopy,
    ToggleContinueOnError,
    Explain,
    AddFavorite,
    OpenFavorite,
    ClearFavorite,
    ClearAllFavorites,
    StructureCompare,
    Cli,
    ComingSoon,
    DesignTable,
    QueryTable,
    FilterAdd,
    FilterClear,
    SortAsc,
    SortDesc,
    SortNone,
    ClearFilterSort,
}

/// 菜单栏命令的统一载荷。一个动作类型带全部命令：平台只认动作类型做
/// 可用性判断，命令自己按 `cmd` 分支，注册一次就够。
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = daro, no_json)]
pub(crate) struct MenuCmd {
    pub cmd: Cmd,
    pub arg: u32,
}

impl MenuCmd {
    fn new(cmd: Cmd, arg: u32) -> Self {
        Self { cmd, arg }
    }
}

fn mi(label: impl Into<SharedString>, cmd: Cmd) -> MenuItem {
    MenuItem::action(label, MenuCmd::new(cmd, 0))
}

fn mi_arg(label: impl Into<SharedString>, cmd: Cmd, arg: u32) -> MenuItem {
    MenuItem::action(label, MenuCmd::new(cmd, arg))
}

fn mi_disabled(label: impl Into<SharedString>, cmd: Cmd, disabled: bool) -> MenuItem {
    mi(label, cmd).disabled(disabled)
}

/// 参考界面里存在、daro 还没做的条目：保持灰态占位，不留「点了没反应」的入口。
fn dead(label: impl Into<SharedString>) -> MenuItem {
    MenuItem::action(label, NoAction).disabled(true)
}

fn sep() -> MenuItem {
    MenuItem::separator()
}

fn sub(name: impl Into<SharedString>, items: Vec<MenuItem>) -> MenuItem {
    MenuItem::submenu(Menu::new(name).items(items))
}

/// 与窗口内菜单同一句灰字占位（口径抄 `view_bar_menu` 那一批）。
fn placeholder_sub(name: &str) -> MenuItem {
    sub(name, vec![dead("（子菜单内容待截图核对）")])
}

pub(crate) fn build_menus(app: &DaroApp, fullscreen: bool) -> Vec<Menu> {
    let bare = app.opened.is_empty();
    let table = app
        .active_table()
        .is_some_and(|t| !t.design && !t.query && !t.role && t.cat == 0);
    let query = app.active_query_id().is_some();
    let mut menus = vec![
        app_menu(app),
        file_menu(app, bare),
        edit_menu(app, bare),
        view_menu(app, bare),
    ];
    if table {
        menus.push(table_menu(app));
    } else if query {
        menus.push(query_menu(app));
        menus.push(format_menu(app));
    }
    menus.push(favorites_menu(app));
    menus.push(tools_menu());
    menus.push(window_menu(fullscreen));
    menus.push(help_menu());
    menus
}

fn app_menu(app: &DaroApp) -> Menu {
    Menu::new(APP_MENU_NAME).items(vec![
        mi("关于 daro-rs", Cmd::About),
        sep(),
        mi("检查更新...", Cmd::CheckUpdates),
        sep(),
        MenuItem::action("设置...", OpenOptions),
        mi("主题...", Cmd::ThemePicker),
        MenuItem::action("扩展...", OpenExtensions),
        sub(
            "面板布局",
            vec![
                mi("显示导航窗格", Cmd::ToggleTree).checked(app.show_tree),
                mi("显示信息窗格", Cmd::ToggleInfo).checked(app.show_info),
                sep(),
                mi("重置面板宽度", Cmd::ResetPaneWidth),
            ],
        ),
        sep(),
        // 系统「服务」：挂上后其他 App 的「服务」才落到这里。
        MenuItem::os_submenu("服务", SystemMenuType::Services),
        sep(),
        // 隐藏不绑 ⌘H：那个键位在 daro 里是「替换」，不能让系统菜单抢走。
        mi("隐藏 daro-rs", Cmd::Hide),
        mi("隐藏其他", Cmd::HideOthers),
        mi("显示全部", Cmd::ShowAll),
        sep(),
        MenuItem::action("退出 daro-rs", QuitApp),
    ])
}

fn file_menu(app: &DaroApp, bare: bool) -> Menu {
    let has_conn = app.sel_conn_ix().is_some();
    let has_query = app.active_query_id().is_some();
    let mut items = vec![
        dead("新建项目..."),
        mi("新建连接...", Cmd::NewConnection),
        // 新建 ▸：查询随时能开；表 / 视图 / 函数要落在当前库上，没有打开的
        // 连接就整项置灰。
        sub(
            "新建",
            vec![
                mi("查询", Cmd::NewQuery),
                mi_disabled("表", Cmd::NewTable, !has_conn),
                mi_disabled("视图", Cmd::NewView, !has_conn),
                mi_disabled("函数", Cmd::NewFunction, !has_conn),
            ],
        ),
        dead("使用 URI 打开..."),
        sub(
            "打开文件",
            vec![
                dead("项目..."),
                mi("连接...", Cmd::ImportConnections),
                mi("SQL 文件...", Cmd::OpenSqlFile),
                mi("运行 SQL 文件...", Cmd::RunSqlFile),
            ],
        ),
        recent_queries_menu(app),
    ];

    // 编辑器级的那几行在「刚打开」态整组消失（同窗口内口径）：没有页签就
    // 没有落点，留着就是假入口。
    if !bare {
        items.push(sep());
        items.push(MenuItem::action("保存", Save));
        items.push(mi_disabled("另存为...", Cmd::SaveAs, !has_query));
        items.push(dead("另存为外部文件..."));
        items.push(sep());
        items.push(mi_disabled(
            "导出结果...",
            Cmd::ExportResult,
            app.active_export_result().is_none(),
        ));
    }

    items.push(sep());
    items.push(mi("导入连接...", Cmd::ImportConnections));
    items.push(mi("导出连接...", Cmd::ExportConnections));
    items.push(sep());
    items.push(mi("管理云...", Cmd::ComingSoon));
    items.push(sep());
    items.push(mi("关闭窗口", Cmd::CloseWindow));
    items.push(MenuItem::action("关闭选项卡", CloseTab).disabled(bare));
    items.push(sep());
    items.push(MenuItem::action("退出 daro-rs", QuitApp));
    Menu::new("文件").items(items)
}

fn recent_queries_menu(app: &DaroApp) -> MenuItem {
    let mut items = Vec::new();
    if app.saved_queries.is_empty() {
        items.push(dead("（暂无最近使用过的查询）"));
    } else {
        for (ix, sq) in app.saved_queries.iter().enumerate() {
            items.push(mi_arg(sq.name.clone(), Cmd::OpenSavedQuery, ix as u32));
        }
        items.push(sep());
        for (ix, sq) in app.saved_queries.iter().enumerate() {
            items.push(mi_arg(
                format!("删除：{}", sq.name),
                Cmd::DeleteSavedQuery,
                ix as u32,
            ));
        }
    }
    sub("打开最近使用过的", items)
}

/// 「编辑」菜单跟窗口内口径一致：对象页只有三条；查询页是编辑器那一套
/// （文本操作直接挂 gpui 的输入动作，派发给当前焦点，和 ⌘C / ⌘V 同一个
/// 落点）；其它页签按网格。
fn edit_menu(app: &DaroApp, bare: bool) -> Menu {
    if bare || app.active_tab == 0 {
        let has_sel = app
            .selected_object_acts()
            .iter()
            .any(|&act| app.object_ref(act).is_some());
        let has_clip = app.table_clip.is_some() || app.object_clip.is_some();
        return Menu::new("编辑").items(vec![
            MenuItem::action("复制", GridCopy).disabled(!has_sel),
            MenuItem::action("粘贴", GridPaste).disabled(!has_clip),
            sep(),
            MenuItem::action("全选", GridSelectAll)
                .disabled(app.pane_objects_visible().is_empty()),
        ]);
    }

    if app.active_query_id().is_some() {
        let has_sel = active_query_state(app).is_some_and(|q| q.has_selection);
        let quote = |label: &str, ch: char| {
            mi_arg(label, Cmd::QuoteCopy, ch as u32).disabled(!has_sel)
        };
        return Menu::new("编辑").items(vec![
            MenuItem::action("撤消", ed::Undo),
            MenuItem::action("重做", ed::Redo),
            sep(),
            MenuItem::action("剪切", ed::Cut).disabled(!has_sel),
            MenuItem::action("复制", ed::Copy).disabled(!has_sel),
            sub(
                "含引号复制",
                vec![
                    quote("单引号 '…'", '\''),
                    quote("双引号 \"…\"", '"'),
                    quote("反引号 `…`", '`'),
                ],
            ),
            MenuItem::action("粘贴", ed::Paste),
            dead("从剪贴板堆栈粘贴"),
            placeholder_sub("线条"),
            sep(),
            MenuItem::action("选择当前语句", EditSelectStmt),
            MenuItem::action("全选", ed::SelectAll),
            sep(),
            MenuItem::action("查找", QueryFind),
            MenuItem::action("查找下一个", EditFindNext),
            MenuItem::action("替换", QueryReplace),
            sep(),
            sub(
                "代码补全",
                vec![MenuItem::action("显示代码补全", ed::ShowCompletions)],
            ),
            dead("创建片段"),
        ]);
    }

    let (can_undo, can_redo) = app.grid_undo_state();
    Menu::new("编辑").items(vec![
        MenuItem::action("撤消", GridUndo).disabled(!can_undo),
        MenuItem::action("重做", GridRedo).disabled(!can_redo),
        sep(),
        dead("剪切"),
        MenuItem::action("复制", GridCopy),
        sub(
            "含引号复制",
            vec![dead("单引号 '…'"), dead("双引号 \"…\""), dead("反引号 `…`")],
        ),
        MenuItem::action("粘贴", GridPaste),
        dead("从剪贴板堆栈粘贴"),
        placeholder_sub("线条"),
        sep(),
        dead("选择当前语句"),
        MenuItem::action("全选", GridSelectAll),
        sep(),
        dead("查找"),
        dead("查找下一个"),
        dead("替换"),
        sep(),
        sub("代码补全", vec![dead("显示代码补全")]),
        dead("创建片段"),
    ])
}

fn active_query_state(app: &DaroApp) -> Option<&crate::QueryState> {
    let qid = app.active_query_id()?;
    app.opened
        .iter()
        .find(|t| t.qid == qid)
        .and_then(|t| t.qstate.as_ref())
}

fn view_menu(app: &DaroApp, bare: bool) -> Menu {
    let ddl_capable = app.info_ddl_capable();
    let nav = sub(
        "导航窗格",
        vec![
            mi("显示导航窗格", Cmd::ToggleTree).checked(app.show_tree),
            dead("隐藏连接组"),
            dead("仅显示活跃对象"),
            dead("在顶部显示筛选"),
        ],
    );
    let info = sub(
        "信息窗格",
        vec![
            mi("显示信息窗格", Cmd::ToggleInfo).checked(app.show_info),
            sep(),
            mi_arg("常规", Cmd::InfoPage, 0).checked(app.info_page == 0),
            mi_arg("代码段", Cmd::InfoPage, 1)
                .checked(app.info_page == 1)
                .disabled(!ddl_capable),
            dead("标识符"),
            dead("AI 对话"),
        ],
    );

    if bare {
        // 「刚打开」态：树级条目，刷新 / 结果 / 网格视图那一组不出现。
        return Menu::new("查看").items(vec![
            nav,
            info,
            // 列表＝对象面板的列表视图（和「显示 ▸」那两档同一份 list_view）。
            mi_arg("列表", Cmd::SetListView, 1).checked(app.list_view),
            mi("详细信息", Cmd::ComingSoon),
            dead("ER 图表"),
            dead("隐藏对象组"),
            placeholder_sub("排序"),
            dead("选择列"),
            dead("显示隐藏的项目"),
            dead("显示表配置文件"),
        ]);
    }

    let display = sub(
        "显示",
        vec![
            dead("在网格中显示备注"),
            dead("在网格中显示图像"),
            sep(),
            dead("冻结列"),
            dead("解除所有冻结列"),
            sep(),
            dead("设置列宽"),
            dead("设置行高"),
            placeholder_sub("缩放"),
            sep(),
            mi_arg("大型图标", Cmd::SetListView, 0).checked(!app.list_view),
            mi_arg("列表", Cmd::SetListView, 1).checked(app.list_view),
            mi("详细信息", Cmd::ComingSoon),
            mi("切换主题", Cmd::ToggleTheme),
            mi("选择主题...", Cmd::ThemePicker),
        ],
    );

    Menu::new("查看").items(vec![
        MenuItem::action("刷新", RefreshView),
        sep(),
        nav,
        info,
        dead("查询编辑器"),
        sub("结果", result_items(app)),
        sep(),
        MenuItem::action("网格视图", NoAction).checked(true),
        mi("表单视图", Cmd::ComingSoon),
        sep(),
        dead("单元格编辑器"),
        dead("数据分析"),
        dead("显示查询分析和状态"),
        dead("自动换行"),
        sep(),
        display,
    ])
}

fn result_items(app: &DaroApp) -> Vec<MenuItem> {
    let (tabs, pane) = app
        .active_result_panes()
        .map_or((Vec::new(), usize::MAX), |x| x);
    let mut items = Vec::new();
    if tabs.is_empty() {
        items.push(dead("（本轮没有查询结果集）"));
    } else {
        for (i, label, _) in tabs {
            items.push(mi_arg(label, Cmd::ResultPane, i as u32).checked(pane == i));
        }
    }
    items.push(sep());
    items.push(
        MenuItem::action("在编辑器下方显示结果", NoAction).checked(true),
    );
    items.push(dead("在编辑器旁边显示结果"));
    items.push(dead("在新页面显示结果"));
    items.push(sep());
    for (pane_no, label) in RUN_PANE_LABELS.iter().enumerate() {
        items.push(
            mi_arg(*label, Cmd::SetRunPane, pane_no as u32)
                .checked(app.run_pane == pane_no as u8),
        );
    }
    items
}

fn format_menu(app: &DaroApp) -> Menu {
    let has_query = app.active_query_id().is_some();
    let indent = if has_query {
        sub(
            "缩进",
            vec![
                MenuItem::action("增加缩排", ed::Indent),
                MenuItem::action("减少缩排", ed::Outdent),
            ],
        )
    } else {
        sub("缩进", vec![dead("增加缩排"), dead("减少缩排")])
    };
    Menu::new("格式").items(vec![
        indent,
        placeholder_sub("注释"),
        placeholder_sub("转换大小写"),
        sep(),
        mi_disabled("美化 SQL", Cmd::Prettify, !has_query),
        dead("美化 SQL 选项..."),
        dead("简化 SQL"),
        sep(),
        placeholder_sub("重构"),
    ])
}

fn favorites_menu(app: &DaroApp) -> Menu {
    let has_tab = app.active_tab > 0 && app.active_tab <= app.opened.len();
    let mut items = vec![
        mi_disabled("添加到收藏夹...", Cmd::AddFavorite, !has_tab),
        sep(),
    ];
    for ix in 0..crate::favorites::SLOTS {
        let Some(fav) = app.favorites.get(ix).and_then(|s| s.as_ref()) else {
            continue;
        };
        let label = format!("{}. {}", ix + 1, fav.name);
        // 槽位条目挂 Favorite1..9，行尾的 ⌘1…9 由键位系统补。
        items.push(match crate::favorite_action(ix) {
            Some(action) => MenuItem::Action {
                name: label.into(),
                action,
                os_action: None,
                checked: false,
                disabled: false,
            },
            None => mi_arg(label, Cmd::OpenFavorite, ix as u32),
        });
    }

    let mut clear = Vec::new();
    let mut any = false;
    for (ix, fav) in app.favorites.iter().enumerate() {
        let Some(fav) = fav else { continue };
        any = true;
        clear.push(mi_arg(
            format!("{}. {}", ix + 1, fav.name),
            Cmd::ClearFavorite,
            ix as u32,
        ));
    }
    if any {
        clear.push(sep());
        clear.push(mi("全部清除", Cmd::ClearAllFavorites));
    } else {
        clear.push(dead("（无收藏）"));
    }
    items.push(sep());
    items.push(sub("清除收藏夹", clear));
    Menu::new("收藏夹").items(items)
}

fn tools_menu() -> Menu {
    Menu::new("工具").items(vec![
        dead("数据传输..."),
        dead("数据生成..."),
        dead("数据脱敏..."),
        dead("数据字典..."),
        dead("数据同步..."),
        mi("结构同步...", Cmd::StructureCompare),
        mi("命令列界面...", Cmd::Cli),
        placeholder_sub("服务器监控"),
        dead("在数据库或模式中查找..."),
        sep(),
        dead("历史日志..."),
        sep(),
        MenuItem::action("选项...", OpenOptions),
    ])
}

fn window_menu(fullscreen: bool) -> Menu {
    Menu::new("窗口").items(vec![
        MenuItem::action("最小化", MinimizeWindow),
        MenuItem::action("全屏", ToggleFullscreen).checked(fullscreen),
        sep(),
        MenuItem::action("关闭所有选项卡", CloseAllTabs),
    ])
}

fn help_menu() -> Menu {
    Menu::new("帮助").items(vec![
        mi("提供 daro 反馈意见...", Cmd::Feedback),
        sep(),
        mi("检查更新...", Cmd::CheckUpdates),
        sep(),
        mi("关于...", Cmd::About),
    ])
}

fn table_menu(app: &DaroApp) -> Menu {
    let sort_cix = app
        .active_data_tab()
        .and_then(|t| (t.cell.1 < t.cols.len()).then_some(t.cell.1));
    let sort_items = match sort_cix {
        Some(cix) => vec![
            mi_arg("升序", Cmd::SortAsc, cix as u32),
            mi_arg("降序", Cmd::SortDesc, cix as u32),
            mi_arg("清除排序", Cmd::SortNone, cix as u32),
        ],
        None => vec![dead("当前没有可排序的列")],
    };
    let can_design = app.active_object_act().is_some();
    Menu::new("表").items(vec![
        dead("开始事务"),
        sep(),
        mi_disabled("设计表...", Cmd::DesignTable, !can_design),
        mi("查询表...", Cmd::QueryTable),
        sep(),
        sub(
            "筛选",
            vec![
                mi("添加筛选条件", Cmd::FilterAdd),
                mi("清除筛选", Cmd::FilterClear),
            ],
        ),
        sub("排序", sort_items),
        mi("移除所有筛选 & 排序", Cmd::ClearFilterSort),
        sep(),
        mi("AI 自动填充...", Cmd::ComingSoon),
        sep(),
        MenuItem::action("刷新", RefreshView),
    ])
}

fn query_menu(app: &DaroApp) -> Menu {
    if app.active_query_id().is_none() {
        return Menu::new("查询").items(vec![dead("（没有激活的查询页）")]);
    }
    let (running, stopping, continue_on_error) = active_query_state(app)
        .map(|q| (q.job.is_some(), q.stopping, q.continue_on_error))
        .unwrap_or((false, false, false));
    Menu::new("查询").items(vec![
        MenuItem::action("运行", QueryRun),
        MenuItem::action("运行当前语句", QueryRunStmt),
        MenuItem::action(if stopping { "停止中…" } else { "停止" }, QueryStop)
            .disabled(!running || stopping),
        mi("遇到错误时继续", Cmd::ToggleContinueOnError).checked(continue_on_error),
        sep(),
        mi("解释", Cmd::Explain),
        mi("询问 AI...", Cmd::ComingSoon),
        sub(
            "AI 处理",
            vec![
                mi("解释 SQL", Cmd::ComingSoon),
                mi("优化 SQL", Cmd::ComingSoon),
                mi("生成 SQL", Cmd::ComingSoon),
            ],
        ),
    ])
}

/// 菜单可见状态的指纹：菜单树只在它变化时整份重建，避免每帧 set_menus。
/// 只收「影响条目 / 勾选 / 置灰」的状态；翻页、滚动、光标位置这类不进。
pub(crate) fn menu_revision(app: &DaroApp, fullscreen: bool) -> u64 {
    let mut h = DefaultHasher::new();
    fullscreen.hash(&mut h);
    app.opened.len().hash(&mut h);
    app.active_tab.hash(&mut h);
    app.conns.len().hash(&mut h);
    app.show_tree.hash(&mut h);
    app.show_info.hash(&mut h);
    app.info_page.hash(&mut h);
    app.info_ddl_capable().hash(&mut h);
    app.list_view.hash(&mut h);
    app.run_pane.hash(&mut h);
    app.sel_conn_ix().is_some().hash(&mut h);
    app.active_query_id().is_some().hash(&mut h);
    app.active_export_result().is_some().hash(&mut h);
    app.active_object_act().is_some().hash(&mut h);
    app.table_clip.is_some().hash(&mut h);
    app.object_clip.is_some().hash(&mut h);
    let (can_undo, can_redo) = app.grid_undo_state();
    can_undo.hash(&mut h);
    can_redo.hash(&mut h);
    app.selected_object_acts()
        .iter()
        .any(|&act| app.object_ref(act).is_some())
        .hash(&mut h);
    app.pane_objects_visible().is_empty().hash(&mut h);
    if let Some(t) = app.active_data_tab() {
        t.cell.1.hash(&mut h);
        t.cols.len().hash(&mut h);
    }
    for t in &app.opened {
        t.conn_name.hash(&mut h);
        t.db.hash(&mut h);
        t.schema.hash(&mut h);
        t.cat.hash(&mut h);
        t.obj.hash(&mut h);
        t.design.hash(&mut h);
        t.query.hash(&mut h);
        t.role.hash(&mut h);
    }
    if let Some(q) = active_query_state(app) {
        q.pane.hash(&mut h);
        q.job.is_some().hash(&mut h);
        q.stopping.hash(&mut h);
        q.continue_on_error.hash(&mut h);
        q.has_selection.hash(&mut h);
        for o in &q.outcomes {
            o.result.is_some().hash(&mut h);
            o.explain.hash(&mut h);
            o.pinned.hash(&mut h);
            o.pin_seq.hash(&mut h);
        }
    }
    for sq in &app.saved_queries {
        sq.name.hash(&mut h);
        sq.key().hash(&mut h);
    }
    for fav in &app.favorites {
        if let Some(fav) = fav {
            fav.name.hash(&mut h);
        }
    }
    h.finish()
}

impl DaroApp {
    /// macOS：按指纹同步系统菜单栏。每帧 render 里调用，指纹没变直接返回。
    #[cfg(target_os = "macos")]
    pub(crate) fn sync_native_menus(&mut self, window: &Window, cx: &mut Context<Self>) {
        let fullscreen = window.is_fullscreen();
        let rev = menu_revision(self, fullscreen);
        if self.native_menu_rev == Some(rev) {
            return;
        }
        self.native_menu_rev = Some(rev);
        cx.set_menus(build_menus(self, fullscreen));
        // 平台默认在菜单打开时按「动作有没有处理器」重算启停，会把这里烘的
        // 置灰 / 勾选全部覆盖；整棵菜单关掉自动启停，状态以烘的为准。
        disable_menu_autoenables();
    }
}

/// 注册系统菜单栏用到的动作：菜单命令载荷、退出 / 最小化 / 全屏。
/// 与 [crate::wire_global_keys] 同一条装配（main 与 headless 用例共用）。
pub(crate) fn wire_menu_actions(root: Entity<DaroApp>, win: AnyWindowHandle, cx: &mut App) {
    let root_cmd = root.clone();
    cx.on_action::<MenuCmd>(move |ev: &MenuCmd, cx| {
        let (cmd, arg) = (ev.cmd, ev.arg);
        let root = root_cmd.clone();
        cx.defer(move |cx| {
            let _ = win.update(cx, |_, window, cx| {
                root.update(cx, |this, cx| {
                    run_menu_command(this, cmd, arg, window, cx)
                });
            });
        });
    });
    cx.on_action::<QuitApp>(move |_, cx| cx.quit());
    // 最小化 / 全屏作用于当时按着的那扇窗（弹窗也算），不固定回主窗。
    cx.on_action::<MinimizeWindow>(move |_, cx| {
        if let Some(w) = cx.active_window() {
            let _ = w.update(cx, |_, window, _| window.minimize_window());
        }
    });
    cx.on_action::<ToggleFullscreen>(move |_, cx| {
        if let Some(w) = cx.active_window() {
            let _ = w.update(cx, |_, window, _| window.toggle_fullscreen());
        }
    });
}

pub(crate) fn run_menu_command(
    app: &mut DaroApp,
    cmd: Cmd,
    arg: u32,
    window: &mut Window,
    cx: &mut Context<DaroApp>,
) {
    let me = cx.entity();
    match cmd {
        Cmd::About => crate::about_dialog::open_daro_about(app.dark, cx),
        Cmd::CheckUpdates => crate::update_dialog::open_center(me, app.dark, cx),
        Cmd::ThemePicker => crate::theme_dialog::open_theme_dialog(me, app.dark, window, cx),
        Cmd::Feedback => crate::about_dialog::open_issue_tracker(),
        Cmd::Hide => {
            #[cfg(target_os = "macos")]
            cx.hide();
        }
        Cmd::HideOthers => {
            #[cfg(target_os = "macos")]
            cx.hide_other_apps();
        }
        Cmd::ShowAll => {
            #[cfg(target_os = "macos")]
            cx.unhide_other_apps();
        }
        Cmd::ResetPaneWidth => {
            app.left_w = LEFT_W;
            app.right_w = RIGHT_W;
            app.save();
            cx.notify();
        }
        Cmd::NewConnection => crate::conn_dialog::open_new_connection(me, app.dark, cx),
        Cmd::NewQuery => app.open_query_tab(None, window, cx),
        Cmd::NewTable | Cmd::NewView | Cmd::NewFunction => {
            let (c, d, s, _) = app.pane_path();
            let k = match cmd {
                Cmd::NewTable => 0,
                Cmd::NewView => 1,
                _ => 3,
            };
            app.open_new_designer(Act::Obj(c, d, s, k, 0), window, cx);
        }
        Cmd::ImportConnections => crate::ncx_import_dialog::open_ncx_import(me, app.dark, cx),
        Cmd::ExportConnections => crate::ncx_export_dialog::open_ncx_export(me, app.dark, cx),
        Cmd::OpenSqlFile => app.open_sql_file_in_editor(window, cx),
        Cmd::RunSqlFile => app.query_run_file_active(cx),
        Cmd::OpenSavedQuery => {
            if let Some(sq) = app.saved_queries.get(arg as usize).cloned() {
                app.open_saved_query(sq, window, cx);
            }
        }
        Cmd::DeleteSavedQuery => {
            if let Some(key) = app.saved_queries.get(arg as usize).map(|q| q.key()) {
                app.delete_saved_query(&key);
                cx.notify();
            }
        }
        Cmd::SaveAs => {
            if let Some(qid) = app.active_query_id() {
                app.query_save_as(qid, cx);
            }
        }
        Cmd::ExportResult => {
            if let Some((qid, oi)) = app.active_export_result() {
                app.query_export_result(qid, oi, cx);
            }
        }
        Cmd::CloseWindow => window.remove_window(),
        Cmd::ToggleTree => {
            app.show_tree = !app.show_tree;
            app.save();
            cx.notify();
        }
        Cmd::ToggleInfo => {
            app.show_info = !app.show_info;
            app.save();
            cx.notify();
        }
        Cmd::InfoPage => app.set_info_page(arg as u8, cx),
        Cmd::SetListView => {
            app.list_view = arg == 1;
            app.save();
            cx.notify();
        }
        Cmd::ResultPane => app.query_set_pane(arg as usize, cx),
        Cmd::SetRunPane => app.set_run_pane(arg as u8, cx),
        Cmd::ToggleTheme => app.toggle_theme(cx),
        Cmd::Prettify => {
            if let Some(qid) = app.active_query_id() {
                app.query_prettify(qid, window, cx);
            }
        }
        Cmd::QuoteCopy => {
            let quote = char::from_u32(arg).unwrap_or('\'');
            let text = active_query_state(app)
                .map(|q| q.editor.clone())
                .map(|e| e.read(cx).selected_text().to_string())
                .unwrap_or_default();
            if !text.is_empty() {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(format!(
                    "{quote}{text}{quote}"
                )));
            }
        }
        Cmd::ToggleContinueOnError => {
            if let Some(qid) = app.active_query_id() {
                if let Some(q) = DaroApp::query_state_mut(&mut app.opened, qid) {
                    q.continue_on_error = !q.continue_on_error;
                }
                cx.notify();
            }
        }
        Cmd::Explain => {
            if let Some(qid) = app.active_query_id() {
                app.query_explain(qid, window, cx);
            }
        }
        Cmd::AddFavorite => {
            if let Some(ix) = app.active_tab.checked_sub(1) {
                app.start_add_favorite_for_tab(ix, cx);
            }
        }
        Cmd::OpenFavorite => app.open_favorite(arg as usize, window, cx),
        Cmd::ClearFavorite => {
            app.clear_favorite(arg as usize);
            cx.notify();
        }
        Cmd::ClearAllFavorites => {
            app.clear_all_favorites();
            cx.notify();
        }
        Cmd::StructureCompare => {
            let _ = crate::structure_compare::open_structure_compare(me, app.dark, cx);
        }
        Cmd::Cli => app.open_cli(None, cx),
        Cmd::ComingSoon => crate::ent_coming_soon(app.dark, cx),
        Cmd::DesignTable => {
            if let Some(act) = app.active_object_act() {
                app.open_designer(act, window, cx);
            }
        }
        Cmd::QueryTable => app.query_active_table(window, cx),
        Cmd::FilterAdd => app.grid_menu_open_filter(true, cx),
        Cmd::FilterClear => app.filter_clear(cx),
        Cmd::SortAsc => app.grid_menu_sort(arg as usize, Some(true), cx),
        Cmd::SortDesc => app.grid_menu_sort(arg as usize, Some(false), cx),
        Cmd::SortNone => app.grid_menu_sort(arg as usize, None, cx),
        Cmd::ClearFilterSort => app.grid_menu_clear_filter_sort(cx),
    }
}

/// 关掉整棵主菜单的自动启停：AppKit 默认会用 `validateMenuItem:` 按
/// 「动作有没有处理器」重算 enabled，把烘进去的置灰 / 勾选全部覆盖。
#[cfg(target_os = "macos")]
fn disable_menu_autoenables() {
    use objc2::runtime::AnyObject;
    unsafe {
        let app: *mut AnyObject =
            objc2::msg_send![objc2::class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let menu: *mut AnyObject = objc2::msg_send![app, mainMenu];
        if menu.is_null() {
            return;
        }
        clear_menu_autoenables(menu);
    }
}

#[cfg(target_os = "macos")]
unsafe fn clear_menu_autoenables(menu: *mut objc2::runtime::AnyObject) {
    use objc2::runtime::{AnyObject, Bool};
    unsafe {
        let _: () = objc2::msg_send![menu, setAutoenablesItems: Bool::NO];
        let count: isize = objc2::msg_send![menu, numberOfItems];
        for i in 0..count {
            let item: *mut AnyObject = objc2::msg_send![menu, itemAtIndex: i];
            if item.is_null() {
                continue;
            }
            let submenu: *mut AnyObject = objc2::msg_send![item, submenu];
            if !submenu.is_null() {
                clear_menu_autoenables(submenu);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_test_support::{isolate_store, open_app};
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt;

    // `use super::*` 会把 gpui 的同名 test 宏带进来，生成的内部 #[test] 会再命中它。
    use core::prelude::v1::test;

    fn menu_names(menus: &[Menu]) -> Vec<String> {
        menus.iter().map(|m| m.name.to_string()).collect()
    }

    fn item<'a>(menus: &'a [Menu], menu: &str, label: &str) -> &'a MenuItem {
        let m = menus
            .iter()
            .find(|m| m.name.as_ref() == menu)
            .unwrap_or_else(|| panic!("菜单栏没有「{menu}」：{:?}", menu_names(menus)));
        m.items
            .iter()
            .find(|i| matches!(i, MenuItem::Action { name, .. } if name.as_ref() == label))
            .unwrap_or_else(|| panic!("「{menu}」菜单没有「{label}」"))
    }

    fn submenu<'a>(menus: &'a [Menu], menu: &str, label: &str) -> &'a Menu {
        let m = menus.iter().find(|m| m.name.as_ref() == menu).unwrap();
        m.items
            .iter()
            .find_map(|i| match i {
                MenuItem::Submenu(sub) if sub.name.as_ref() == label => Some(sub),
                _ => None,
            })
            .unwrap_or_else(|| panic!("「{menu}」没有子菜单「{label}」"))
    }

    #[gpui_kit::test]
    fn native_menu_bar_matches_bare_context(cx: &mut TestAppContext) {
        isolate_store("natvmenubare");
        let (_handle, app) = open_app(cx);
        let menus = app.read_with(cx, |a, _| build_menus(a, false));
        let names = menu_names(&menus);
        assert_eq!(names[0], APP_MENU_NAME, "第一栏是 App 菜单");
        for name in ["文件", "编辑", "查看", "收藏夹", "工具", "窗口", "帮助"] {
            assert!(names.contains(&name.to_string()), "缺「{name}」：{names:?}");
        }
        assert!(!names.contains(&"查询".to_string()), "没有查询页就没有「查询」菜单");
        assert!(!names.contains(&"表".to_string()), "没有数据页就没有「表」菜单");

        // 刚打开：保存 / 关页点不动或整组不出现，关闭窗口可以。
        let file = menus.iter().find(|m| m.name == "文件").unwrap();
        assert!(
            !file
                .items
                .iter()
                .any(|i| matches!(i, MenuItem::Action { name, .. } if name == "保存")),
            "没有页签时整组编辑器条目都不出现"
        );
        assert!(item(&menus, "文件", "关闭选项卡").is_disabled());
        assert!(!item(&menus, "文件", "关闭窗口").is_disabled());
        assert!(item(&menus, "编辑", "复制").is_disabled(), "没选对象就复制不了");
        assert!(item(&menus, "编辑", "全选").is_disabled(), "面板没对象就全选不了");
        let edit = menus.iter().find(|m| m.name == "编辑").unwrap();
        assert_eq!(edit.items.len(), 4, "复制 / 粘贴 ─ 全选");

        // 对象面板的「列表」勾选态跟 list_view 走。
        let list_view = app.read_with(cx, |a, _| a.list_view);
        assert_eq!(item(&menus, "查看", "列表").is_checked(), list_view);
    }

    #[gpui_kit::test]
    fn native_menu_bar_adds_query_menu_and_tracks_state(cx: &mut TestAppContext) {
        isolate_store("natvmenuquery");
        let (handle, app) = open_app(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            app.update(cx, |a, cx| a.open_query_tab(None, window, cx));
            window.render_frame(cx);
        })
        .unwrap();

        let menus = app.read_with(cx, |a, _| build_menus(a, false));
        let names = menu_names(&menus);
        assert!(names.contains(&"查询".to_string()), "查询页要出「查询」：{names:?}");
        assert!(names.contains(&"格式".to_string()), "查询页要出「格式」：{names:?}");
        assert!(!names.contains(&"表".to_string()));
        assert!(!item(&menus, "文件", "保存").is_disabled());
        assert!(!item(&menus, "文件", "另存为...").is_disabled());
        // 没在跑：「停止」灰。
        assert!(item(&menus, "查询", "停止").is_disabled());
        // 编辑菜单换成编辑器那一套；没有选区时剪切 / 复制 / 含引号复制灰。
        assert!(!item(&menus, "编辑", "替换").is_disabled());
        assert!(item(&menus, "编辑", "剪切").is_disabled());
        let quoted = submenu(&menus, "编辑", "含引号复制");
        assert!(quoted.items.iter().all(|i| i.is_disabled()), "没有选区时三档引号复制都灰");
        // 「查看 ▸ 结果」在还没有结果时列一行说明。
        let results = submenu(&menus, "查看", "结果");
        assert!(results.items[0].is_disabled());

        // 勾选态跟状态栏同一份开关。
        let checked = |menus: &[Menu]| {
            submenu(menus, "查看", "导航窗格")
                .items
                .iter()
                .find(|i| matches!(i, MenuItem::Action { name, .. } if name == "显示导航窗格"))
                .map(|i| i.is_checked())
        };
        assert_eq!(checked(&menus), Some(true));
        let flipped = app.update(cx, |a, _| {
            a.show_tree = false;
            build_menus(a, false)
        });
        assert_eq!(checked(&flipped), Some(false), "关掉导航窗格后勾选态跟着变");
    }

    #[gpui_kit::test]
    fn native_menu_revision_follows_menu_visible_state(cx: &mut TestAppContext) {
        isolate_store("natvmenurev");
        let (_handle, app) = open_app(cx);
        app.update(cx, |a, _| {
            let r1 = menu_revision(a, false);
            assert_eq!(r1, menu_revision(a, false), "同一状态指纹稳定");
            a.show_info = !a.show_info;
            let r2 = menu_revision(a, false);
            assert_ne!(r1, r2, "信息窗格开关要触发重建");
            let r3 = menu_revision(a, true);
            assert_ne!(r2, r3, "全屏状态要触发重建");
        });
    }
}