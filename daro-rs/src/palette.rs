//! daro-rs 的语义色板，取 Zed 默认主题族 One / Ayu / Gruvbox，外加 VS Code 2026
//! 默认的明暗两档：标题栏与状态栏压在 panel 上，编辑器（对象清单 / 数据页）再深
//! 一档，面板之间用 1px border 分隔，选中态是 element.selected。
//!
//! 色板源自 `zed/assets/themes/{one,ayu,gruvbox}/*.json` 与 VS Code 2026；
//! Light 2026 按 daro 的高密度工具栏和数据视图调整了界面色。
//! 推导字段（border_soft、primary_face 等）的算法写在 [`ThemeDef`] 的说明里。

use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy)]
pub struct Palette {
    /// 内容底：编辑器 / 对象清单 / 数据页
    pub background: u32,
    /// 铬件底：标题栏 / 菜单 / 工具条 / 状态栏
    pub chrome: u32,
    /// 硬分割线（面板外框、功能区下沿）
    pub border: u32,
    /// 面板之间的细线
    pub border_soft: u32,
    pub foreground: u32,
    pub muted_foreground: u32,
    /// 属性名、占位符这类更淡的字
    pub subtle_foreground: u32,
    /// 主色：链接 / 焦点 / 活动页签图标
    pub accent: u32,
    /// 强调色实底上的文字色
    pub on_accent: u32,
    /// 列表与活动项底色
    pub selection: u32,
    /// 树里当前节点的底色
    pub selection_focus: u32,
    pub hover: u32,
    pub status_bar: u32,
    /// 侧栏 / 面板底
    pub dialog_surface: u32,
    /// 面板 / 页签的 1px 外框
    pub dialog_line: u32,
    /// 未激活页签底色
    pub tab_face: u32,
    /// 表单控件外框（上 / 左 / 右发丝线）
    pub field_border: u32,
    /// 表单控件底边：未聚焦时比外框深一档
    pub field_edge: u32,
    /// 表单控件聚焦底边
    pub field_focus: u32,
    /// 按钮面
    pub button_face: u32,
    /// 次按钮外框
    pub button_border: u32,
    /// 主按钮（确定）底色：Zed 强调色按钮用 status.info.background，不是实心 accent
    pub primary_face: u32,
    /// 主按钮 hover 底色：info.background 掺背景（暗）/ 掺 accent（亮）
    pub primary_face_hover: u32,
    /// 主按钮描边：status.info.border
    pub primary_border: u32,
    /// 主按钮文字：浅色面（Zed info.background）跟正文走，实心底（VS Code
    /// button.background）用 on_accent，否则深字压深蓝读不出来
    pub primary_foreground: u32,
    /// 单选未选中圆环 / 复选未选中外框
    pub control_ring: u32,
    /// 单选选中圆 / 复选选中块
    pub control_fill: u32,
    /// 编辑器行号
    pub line_number: u32,
    /// 窗口失焦时的标题栏底
    pub title_bar_inactive: u32,
    /// 当前行行号
    pub line_number_active: u32,
    /// 编辑器/网格当前行底色
    pub active_line: u32,
    /// 高亮行底色（选中行、括号对）
    pub highlight_line: u32,
    /// 滚动条滑块
    pub scroll_thumb: u32,
    /// 滚动条轨道描边
    pub scroll_track_border: u32,
    /// 拖放目标底色
    pub drop_target: u32,
    /// 搜索命中底色
    pub match_bg: u32,
}

/// 一套可切换主题。字段取自 Zed 主题 JSON：
/// - `background` = editor.background，`chrome` / `status_bar` = title / status bar
/// - `selection` = element.selected，`selection_focus` = element.active，`hover` = element.hover
/// - `dialog_surface` / `tab_face` = surface / tab.inactive background
/// - `primary_face` = info.background（#rrggbb1a）压在 surface 上的实色
/// - `border_soft` / `field_border` = border.variant，`border` = border（硬分割）
pub struct ThemeDef {
    pub name: &'static str,
    pub dark: bool,
    /// 切换明暗时落到哪一档：同系列、同对比度的相反明暗档
    pub counterpart: usize,
    pub palette: Palette,
}

impl Palette {
    // One Dark（one/one.json）
    pub const ONE_DARK: Self = Self {
        background: 0x282c33,
        chrome: 0x3b414d,
        border: 0x464b57,
        border_soft: 0x363c46,
        foreground: 0xdce0e5,
        muted_foreground: 0xa9afbc,
        subtle_foreground: 0x878a98,
        accent: 0x74ade8,
        on_accent: 0x1f2328,
        selection: 0x454a56,
        selection_focus: 0x454a56,
        hover: 0x363c46,
        status_bar: 0x3b414d,
        dialog_surface: 0x2f343e,
        dialog_line: 0x464b57,
        tab_face: 0x2f343e,
        field_border: 0x363c46,
        field_edge: 0x363c46,
        field_focus: 0x47679e,
        button_face: 0x2e343e,
        button_border: 0x464b57,
        // #74ade81a 压在 dialog_surface 上的实色近似
        primary_face: 0x36404f,
        primary_face_hover: 0x2e364d,
        primary_border: 0x293b5b,
        primary_foreground: 0xdce0e5,
        control_ring: 0x7f8694,
        control_fill: 0x74ade8,
        line_number: 0x4e5a5f,
        // 窗口失焦时的标题栏：title_bar.inactive_background
        title_bar_inactive: 0x2e343e,
        // 当前行行号：editor.active_line_number
        line_number_active: 0xd0d4da,
        // 当前行底色：editor.active_line.background 压在 editor 底
        active_line: 0x2d323b,
        // 高亮行（选中行/括号对）：editor.highlighted_line.background
        highlight_line: 0x2f343e,
        // 滚动条滑块：scrollbar.thumb.background 压在 editor 底
        scroll_thumb: 0x585c63,
        // 滚动条轨道描边：scrollbar.track.border
        scroll_track_border: 0x2e333c,
        // 拖放目标：drop_target.background 压在 panel 底
        drop_target: 0x595f69,
        // 搜索命中：search.match_background 压在 editor 底
        match_bg: 0x46607b,
    };

    // One Light（one/one.json）
    pub const ONE_LIGHT: Self = Self {
        background: 0xfafafa,
        chrome: 0xdcdcdd,
        border: 0xc9c9ca,
        border_soft: 0xdfdfe0,
        foreground: 0x0a0a0a,
        muted_foreground: 0x58585a,
        subtle_foreground: 0x7e8086,
        accent: 0x5c78e2,
        on_accent: 0xffffff,
        selection: 0xcacaca,
        selection_focus: 0xcacaca,
        hover: 0xdfdfe0,
        status_bar: 0xdcdcdd,
        dialog_surface: 0xebebec,
        dialog_line: 0xc9c9ca,
        tab_face: 0xebebec,
        field_border: 0xdfdfe0,
        field_edge: 0xdfdfe0,
        field_focus: 0x7d82e8,
        button_face: 0xebebec,
        button_border: 0xc9c9ca,
        primary_face: 0xe2e2fa,
        primary_face_hover: 0xcbcbf7,
        primary_border: 0xcbcdf6,
        primary_foreground: 0x0a0a0a,
        control_ring: 0x8a8b90,
        control_fill: 0x5c78e2,
        line_number: 0xb4b4bb,
        // 窗口失焦时的标题栏：title_bar.inactive_background
        title_bar_inactive: 0xebebec,
        // 当前行行号：editor.active_line_number
        line_number_active: 0x44454b,
        // 当前行底色：editor.active_line.background 压在 editor 底
        active_line: 0xefeff0,
        // 高亮行（选中行/括号对）：editor.highlighted_line.background
        highlight_line: 0xebebec,
        // 滚动条滑块：scrollbar.thumb.background 压在 editor 底
        scroll_thumb: 0xc0c1c3,
        // 滚动条轨道描边：scrollbar.track.border
        scroll_track_border: 0xeeeeee,
        // 拖放目标：drop_target.background 压在 panel 底
        drop_target: 0xb4b5b9,
        // 搜索命中：search.match_background 压在 editor 底
        match_bg: 0xbbc6f0,
    };

    // Ayu Dark（ayu/ayu.json）
    pub const AYU_DARK: Self = Self {
        background: 0x0d1016,
        chrome: 0x313337,
        border: 0x3f4043,
        border_soft: 0x2d2f34,
        foreground: 0xbfbdb6,
        muted_foreground: 0x8a8986,
        subtle_foreground: 0x696a6a,
        accent: 0x5ac1fe,
        on_accent: 0x1f2328,
        selection: 0x3e4043,
        selection_focus: 0x3e4043,
        hover: 0x2d2f34,
        status_bar: 0x313337,
        dialog_surface: 0x1f2127,
        dialog_line: 0x3f4043,
        tab_face: 0x1f2127,
        field_border: 0x2d2f34,
        field_edge: 0x2d2f34,
        field_focus: 0x1b4a6e,
        button_face: 0x1f2127,
        button_border: 0x3f4043,
        primary_face: 0x0d2f4e,
        primary_face_hover: 0x0d243a,
        primary_border: 0x1b4a6e,
        primary_foreground: 0xbfbdb6,
        control_ring: 0x696a6a,
        control_fill: 0x5ac1fe,
        line_number: 0x4b4c4e,
        // 窗口失焦时的标题栏：title_bar.inactive_background
        title_bar_inactive: 0x1f2127,
        // 当前行行号：editor.active_line_number
        line_number_active: 0xcbcccd,
        // 当前行底色：editor.active_line.background 压在 editor 底
        active_line: 0x1a1d23,
        // 高亮行（选中行/括号对）：editor.highlighted_line.background
        highlight_line: 0x1f2127,
        // 滚动条滑块：scrollbar.thumb.background 压在 editor 底
        scroll_thumb: 0x424446,
        // 滚动条轨道描边：scrollbar.track.border
        scroll_track_border: 0x1b1e24,
        // 拖放目标：drop_target.background 压在 panel 底
        drop_target: 0x555557,
        // 搜索命中：search.match_background 压在 editor 底
        match_bg: 0x2c5773,
    };

    // Ayu Mirage（ayu/ayu.json）
    pub const AYU_MIRAGE: Self = Self {
        background: 0x242835,
        chrome: 0x464a52,
        border: 0x53565d,
        border_soft: 0x43464f,
        foreground: 0xcccac2,
        muted_foreground: 0x9a9a98,
        subtle_foreground: 0x7b7d7f,
        accent: 0x72cffe,
        on_accent: 0x1f2328,
        selection: 0x53565d,
        selection_focus: 0x53565d,
        hover: 0x43464f,
        status_bar: 0x464a52,
        dialog_surface: 0x353944,
        dialog_line: 0x53565d,
        tab_face: 0x353944,
        field_border: 0x43464f,
        field_edge: 0x43464f,
        field_focus: 0x24556f,
        button_face: 0x353944,
        button_border: 0x53565d,
        primary_face: 0x123950,
        primary_face_hover: 0x183347,
        primary_border: 0x24556f,
        primary_foreground: 0xcccac2,
        control_ring: 0x7b7d7f,
        control_fill: 0x72cffe,
        line_number: 0x575c6b,
        // 窗口失焦时的标题栏：title_bar.inactive_background
        title_bar_inactive: 0x353944,
        // 当前行行号：editor.active_line_number
        line_number_active: 0xe1e3ea,
        // 当前行底色：editor.active_line.background 压在 editor 底
        active_line: 0x313540,
        // 高亮行（选中行/括号对）：editor.highlighted_line.background
        highlight_line: 0x353944,
        // 滚动条滑块：scrollbar.thumb.background 压在 editor 底
        scroll_thumb: 0x56585f,
        // 滚动条轨道描边：scrollbar.track.border
        scroll_track_border: 0x323641,
        // 拖放目标：drop_target.background 压在 panel 底
        drop_target: 0x686a6e,
        // 搜索命中：search.match_background 压在 editor 底
        match_bg: 0x446b85,
    };

    // Ayu Light（ayu/ayu.json）
    pub const AYU_LIGHT: Self = Self {
        background: 0xfcfcfc,
        chrome: 0xdcddde,
        border: 0xcfd1d2,
        border_soft: 0xdfe0e1,
        foreground: 0x5c6166,
        muted_foreground: 0x8b8e92,
        subtle_foreground: 0xa9acae,
        accent: 0x3b9ee5,
        on_accent: 0x1f2328,
        selection: 0xcfd0d2,
        selection_focus: 0xcfd0d2,
        hover: 0xdfe0e1,
        status_bar: 0xdcddde,
        dialog_surface: 0xececed,
        dialog_line: 0xcfd1d2,
        tab_face: 0xececed,
        field_border: 0xdfe0e1,
        field_edge: 0xdfe0e1,
        field_focus: 0xc4daf6,
        button_face: 0xececed,
        button_border: 0xcfd1d2,
        primary_face: 0xdeebfa,
        primary_face_hover: 0xbddcf6,
        primary_border: 0xc4daf6,
        primary_foreground: 0x5c6166,
        control_ring: 0xa9acae,
        control_fill: 0x3b9ee5,
        line_number: 0xb0b3b5,
        // 窗口失焦时的标题栏：title_bar.inactive_background
        title_bar_inactive: 0xececed,
        // 当前行行号：editor.active_line_number
        line_number_active: 0x313435,
        // 当前行底色：editor.active_line.background 压在 editor 底
        active_line: 0xf0f0f1,
        // 高亮行（选中行/括号对）：editor.highlighted_line.background
        highlight_line: 0xececed,
        // 滚动条滑块：scrollbar.thumb.background 压在 editor 底
        scroll_thumb: 0xcccecf,
        // 滚动条轨道描边：scrollbar.track.border
        scroll_track_border: 0xefeff0,
        // 拖放目标：drop_target.background 压在 panel 底
        drop_target: 0xbbbdbf,
        // 搜索命中：search.match_background 压在 editor 底
        match_bg: 0xafd6f3,
    };

    // Gruvbox Dark（gruvbox/gruvbox.json）
    pub const GRUVBOX_DARK: Self = Self {
        background: 0x282828,
        chrome: 0x4c4642,
        border: 0x5b534d,
        border_soft: 0x494340,
        foreground: 0xfbf1c7,
        muted_foreground: 0xc5b597,
        subtle_foreground: 0x998b78,
        accent: 0x83a598,
        on_accent: 0x1f2328,
        selection: 0x5b524c,
        selection_focus: 0x5b524c,
        hover: 0x494340,
        status_bar: 0x4c4642,
        dialog_surface: 0x3a3735,
        dialog_line: 0x5b534d,
        tab_face: 0x3a3735,
        field_border: 0x494340,
        field_edge: 0x494340,
        field_focus: 0x303a36,
        button_face: 0x3a3735,
        button_border: 0x5b534d,
        primary_face: 0x1e2321,
        primary_face_hover: 0x222523,
        primary_border: 0x303a36,
        primary_foreground: 0xfbf1c7,
        control_ring: 0x998b78,
        control_fill: 0x83a598,
        line_number: 0x6e6b5e,
        // 窗口失焦时的标题栏：title_bar.inactive_background
        title_bar_inactive: 0x3a3735,
        // 当前行行号：editor.active_line_number
        line_number_active: 0xdedcd3,
        // 当前行底色：editor.active_line.background 压在 editor 底
        active_line: 0x353332,
        // 高亮行（选中行/括号对）：editor.highlighted_line.background
        highlight_line: 0x3a3735,
        // 滚动条滑块：scrollbar.thumb.background 压在 editor 底
        scroll_thumb: 0x4e4a43,
        // 滚动条轨道描边：scrollbar.track.border
        scroll_track_border: 0x373432,
        // 拖放目标：drop_target.background 压在 panel 底
        drop_target: 0x807666,
        // 搜索命中：search.match_background 压在 editor 底
        match_bg: 0x4c5a55,
    };

    // Gruvbox Dark Hard（gruvbox/gruvbox.json）
    pub const GRUVBOX_DARK_HARD: Self = Self {
        background: 0x1d2021,
        chrome: 0x4c4642,
        border: 0x5b534d,
        border_soft: 0x494340,
        foreground: 0xfbf1c7,
        muted_foreground: 0xc5b597,
        subtle_foreground: 0x998b78,
        accent: 0x83a598,
        on_accent: 0x1f2328,
        selection: 0x5b524c,
        selection_focus: 0x5b524c,
        hover: 0x494340,
        status_bar: 0x4c4642,
        dialog_surface: 0x393634,
        dialog_line: 0x5b534d,
        tab_face: 0x393634,
        field_border: 0x494340,
        field_edge: 0x494340,
        field_focus: 0x303a36,
        button_face: 0x393634,
        button_border: 0x5b534d,
        primary_face: 0x1e2321,
        primary_face_hover: 0x1e2221,
        primary_border: 0x303a36,
        primary_foreground: 0xfbf1c7,
        control_ring: 0x998b78,
        control_fill: 0x83a598,
        line_number: 0x6e6b5e,
        // 窗口失焦时的标题栏：title_bar.inactive_background
        title_bar_inactive: 0x393634,
        // 当前行行号：editor.active_line_number
        line_number_active: 0xdedcd3,
        // 当前行底色：editor.active_line.background 压在 editor 底
        active_line: 0x32302f,
        // 高亮行（选中行/括号对）：editor.highlighted_line.background
        highlight_line: 0x393634,
        // 滚动条滑块：scrollbar.thumb.background 压在 editor 底
        scroll_thumb: 0x46443f,
        // 滚动条轨道描边：scrollbar.track.border
        scroll_track_border: 0x343130,
        // 拖放目标：drop_target.background 压在 panel 底
        drop_target: 0x7f7666,
        // 搜索命中：search.match_background 压在 editor 底
        match_bg: 0x465551,
    };

    // Gruvbox Dark Soft（gruvbox/gruvbox.json）
    pub const GRUVBOX_DARK_SOFT: Self = Self {
        background: 0x32302f,
        chrome: 0x4c4642,
        border: 0x5b534d,
        border_soft: 0x494340,
        foreground: 0xfbf1c7,
        muted_foreground: 0xc5b597,
        subtle_foreground: 0x998b78,
        accent: 0x83a598,
        on_accent: 0x1f2328,
        selection: 0x5b524c,
        selection_focus: 0x5b524c,
        hover: 0x494340,
        status_bar: 0x4c4642,
        dialog_surface: 0x3b3735,
        dialog_line: 0x5b534d,
        tab_face: 0x3b3735,
        field_border: 0x494340,
        field_edge: 0x494340,
        field_focus: 0x303a36,
        button_face: 0x3b3735,
        button_border: 0x5b534d,
        primary_face: 0x1e2321,
        primary_face_hover: 0x252826,
        primary_border: 0x303a36,
        primary_foreground: 0xfbf1c7,
        control_ring: 0x998b78,
        control_fill: 0x83a598,
        line_number: 0x6e6b5e,
        // 窗口失焦时的标题栏：title_bar.inactive_background
        title_bar_inactive: 0x3b3735,
        // 当前行行号：editor.active_line_number
        line_number_active: 0xdedcd3,
        // 当前行底色：editor.active_line.background 压在 editor 底
        active_line: 0x393533,
        // 高亮行（选中行/括号对）：editor.highlighted_line.background
        highlight_line: 0x3b3735,
        // 滚动条滑块：scrollbar.thumb.background 压在 editor 底
        scroll_thumb: 0x554f48,
        // 滚动条轨道描边：scrollbar.track.border
        scroll_track_border: 0x393634,
        // 拖放目标：drop_target.background 压在 panel 底
        drop_target: 0x807666,
        // 搜索命中：search.match_background 压在 editor 底
        match_bg: 0x525f59,
    };

    // Gruvbox Light（gruvbox/gruvbox.json）
    pub const GRUVBOX_LIGHT: Self = Self {
        background: 0xfbf1c7,
        chrome: 0xd9c8a4,
        border: 0xc8b899,
        border_soft: 0xddcca7,
        foreground: 0x282828,
        muted_foreground: 0x5f5650,
        subtle_foreground: 0x897b6e,
        accent: 0x0b6678,
        on_accent: 0xffffff,
        selection: 0xc8b899,
        selection_focus: 0xc8b899,
        hover: 0xddcca7,
        status_bar: 0xd9c8a4,
        dialog_surface: 0xecddb4,
        dialog_line: 0xc8b899,
        tab_face: 0xecddb4,
        field_border: 0xddcca7,
        field_edge: 0xddcca7,
        field_focus: 0xadc5cc,
        button_face: 0xecddb4,
        button_border: 0xc8b899,
        primary_face: 0xd2dee2,
        primary_face_hover: 0xaac6cd,
        primary_border: 0xadc5cc,
        primary_foreground: 0x282828,
        control_ring: 0x897b6e,
        control_fill: 0x0b6678,
        line_number: 0xa9a389,
        // 窗口失焦时的标题栏：title_bar.inactive_background
        title_bar_inactive: 0xecddb4,
        // 当前行行号：editor.active_line_number
        line_number_active: 0x3b382b,
        // 当前行底色：editor.active_line.background 压在 editor 底
        active_line: 0xf0e2b9,
        // 高亮行（选中行/括号对）：editor.highlighted_line.background
        highlight_line: 0xecddb4,
        // 滚动条滑块：scrollbar.thumb.background 压在 editor 底
        scroll_thumb: 0xd5caa9,
        // 滚动条轨道描边：scrollbar.track.border
        scroll_track_border: 0xeee0b7,
        // 拖放目标：drop_target.background 压在 panel 底
        drop_target: 0xa59982,
        // 搜索命中：search.match_background 压在 editor 底
        match_bg: 0x9bb9a7,
    };

    // Gruvbox Light Hard（gruvbox/gruvbox.json）
    pub const GRUVBOX_LIGHT_HARD: Self = Self {
        background: 0xf9f5d7,
        chrome: 0xd9c8a4,
        border: 0xc8b899,
        border_soft: 0xddcca7,
        foreground: 0x282828,
        muted_foreground: 0x5f5650,
        subtle_foreground: 0x897b6e,
        accent: 0x0b6678,
        on_accent: 0xffffff,
        selection: 0xc8b899,
        selection_focus: 0xc8b899,
        hover: 0xddcca7,
        status_bar: 0xd9c8a4,
        dialog_surface: 0xecddb5,
        dialog_line: 0xc8b899,
        tab_face: 0xecddb5,
        field_border: 0xddcca7,
        field_edge: 0xddcca7,
        field_focus: 0xadc5cc,
        button_face: 0xecddb5,
        button_border: 0xc8b899,
        primary_face: 0xd2dee2,
        primary_face_hover: 0xaac6cd,
        primary_border: 0xadc5cc,
        primary_foreground: 0x282828,
        control_ring: 0x897b6e,
        control_fill: 0x0b6678,
        line_number: 0xa9a389,
        // 窗口失焦时的标题栏：title_bar.inactive_background
        title_bar_inactive: 0xecddb5,
        // 当前行行号：editor.active_line_number
        line_number_active: 0x3b382b,
        // 当前行底色：editor.active_line.background 压在 editor 底
        active_line: 0xefe3be,
        // 高亮行（选中行/括号对）：editor.highlighted_line.background
        highlight_line: 0xecddb5,
        // 滚动条滑块：scrollbar.thumb.background 压在 editor 底
        scroll_thumb: 0xd4cdb5,
        // 滚动条轨道描边：scrollbar.track.border
        scroll_track_border: 0xeee1bb,
        // 拖放目标：drop_target.background 压在 panel 底
        drop_target: 0xa59982,
        // 搜索命中：search.match_background 压在 editor 底
        match_bg: 0x9abcb1,
    };

    // Gruvbox Light Soft（gruvbox/gruvbox.json）
    pub const GRUVBOX_LIGHT_SOFT: Self = Self {
        background: 0xf2e5bc,
        chrome: 0xd9c8a4,
        border: 0xc8b899,
        border_soft: 0xddcca7,
        foreground: 0x282828,
        muted_foreground: 0x5f5650,
        subtle_foreground: 0x897b6e,
        accent: 0x0b6678,
        on_accent: 0xffffff,
        selection: 0xc8b899,
        selection_focus: 0xc8b899,
        hover: 0xddcca7,
        status_bar: 0xd9c8a4,
        dialog_surface: 0xecdcb3,
        dialog_line: 0xc8b899,
        tab_face: 0xecdcb3,
        field_border: 0xddcca7,
        field_edge: 0xddcca7,
        field_focus: 0xadc5cc,
        button_face: 0xecdcb3,
        button_border: 0xc8b899,
        primary_face: 0xd2dee2,
        primary_face_hover: 0xaac6cd,
        primary_border: 0xadc5cc,
        primary_foreground: 0x282828,
        control_ring: 0x897b6e,
        control_fill: 0x0b6678,
        line_number: 0xa9a389,
        // 窗口失焦时的标题栏：title_bar.inactive_background
        title_bar_inactive: 0xecdcb3,
        // 当前行行号：editor.active_line_number
        line_number_active: 0x3b382b,
        // 当前行底色：editor.active_line.background 压在 editor 底
        active_line: 0xeedeb5,
        // 高亮行（选中行/括号对）：editor.highlighted_line.background
        highlight_line: 0xecdcb3,
        // 滚动条滑块：scrollbar.thumb.background 压在 editor 底
        scroll_thumb: 0xcfc2a2,
        // 滚动条轨道描边：scrollbar.track.border
        scroll_track_border: 0xeddeb5,
        // 拖放目标：drop_target.background 压在 panel 底
        drop_target: 0xa59981,
        // 搜索命中：search.match_background 压在 editor 底
        match_bg: 0x96b2a1,
    };

    // VS Code 2026 Dark（vscode/extensions/theme-defaults/themes/2026-dark.json）
    // 字段按 workbench 色映射：editor.background / titleBar / panel.border /
    // list.activeSelectionBackground 等；带 alpha 的色（选中、悬停、focusBorder）
    // 已按其底色调成实色。
    pub const VSCODE_2026_DARK: Self = Self {
        background: 0x121314,
        chrome: 0x191a1b,
        border: 0x2a2b2c,
        border_soft: 0x272829,
        foreground: 0xbbbfbf,
        muted_foreground: 0x8c8c8c,
        subtle_foreground: 0x555555,
        accent: 0x48a0c7,
        on_accent: 0xffffff,
        selection: 0x383939,
        selection_focus: 0x2c2d2e,
        hover: 0x2b2c2d,
        status_bar: 0x191a1b,
        dialog_surface: 0x202122,
        dialog_line: 0x2a2b2c,
        tab_face: 0x191a1b,
        field_border: 0x333536,
        field_edge: 0x333536,
        field_focus: 0x2f708c,
        button_face: 0x202122,
        button_border: 0x333536,
        primary_face: 0x297aa0,
        primary_face_hover: 0x2b7da3,
        primary_border: 0x297aa0,
        primary_foreground: 0xffffff,
        control_ring: 0x707070,
        control_fill: 0x48a0c7,
        line_number: 0x858889,
        // 以下按 VS Code 2026 的 editor.lineHighlightBackground / scrollbar /
        // list.dropBackground / find.matchBackground 折算成实色
        title_bar_inactive: 0x171819,
        line_number_active: 0xcccccc,
        active_line: 0x1e1f20,
        highlight_line: 0x232425,
        scroll_thumb: 0x3d3f40,
        scroll_track_border: 0x1c1d1f,
        drop_target: 0x233949,
        match_bg: 0x2b4c66,
    };

    // Light 2026：白色工作区和对话框、浅色窗口栏、淡蓝选中态。
    pub const VSCODE_2026_LIGHT: Self = Self {
        background: 0xffffff,
        chrome: 0xf7f8fa,
        border: 0xdfe4ea,
        border_soft: 0xebeff3,
        foreground: 0x191e26,
        muted_foreground: 0x475364,
        subtle_foreground: 0x5b6778,
        accent: 0x005fb8,
        on_accent: 0xffffff,
        selection: 0xd9eaff,
        selection_focus: 0xe4f0ff,
        hover: 0xeff4fa,
        status_bar: 0xf7f8fa,
        dialog_surface: 0xffffff,
        dialog_line: 0xdfe4ea,
        tab_face: 0xf7f8fa,
        field_border: 0xcbd4df,
        field_edge: 0xb5c1cf,
        field_focus: 0x005fb8,
        button_face: 0xffffff,
        button_border: 0xcbd4df,
        primary_face: 0x005fb8,
        primary_face_hover: 0x0052a1,
        primary_border: 0x005fb8,
        primary_foreground: 0xffffff,
        control_ring: 0x758396,
        control_fill: 0x005fb8,
        line_number: 0x5b6778,
        title_bar_inactive: 0xf2f4f7,
        line_number_active: 0x191e26,
        active_line: 0xf6f8fa,
        highlight_line: 0xeef2f7,
        scroll_thumb: 0xc1c1c1,
        scroll_track_border: 0xe8ecf1,
        drop_target: 0xcce2f7,
        match_bg: 0xfbe4b6,
    };

    /// 当前主题；参数是调用方要的明暗档，与当前主题不一致时落到它的对应档。
    pub fn of(dark: bool) -> Self {
        let theme = active();
        if theme.dark == dark {
            theme.palette
        } else {
            THEMES[theme.counterpart].palette
        }
    }

    /// 回执 / 校验失败的红色：13 套主题 JSON 里都没有语义错误色槽位，各主题
    /// 自带的 accent 可能是蓝、绿甚至橙，报错不能用它。按主题明暗与主色深浅
    /// 分三档，全主题共用同一组常量，改主题时红仍是红（issue #27 同口径：
    /// 状态色不再散写 0xc0392b / 0xDC2626 这类字面量）。
    pub fn error(dark: bool) -> u32 {
        let p = Self::of(dark);
        // 相对亮度（Rec.709），主色本身很深时红再压暗一档，避免浮在暗底上刺眼
        let r = ((p.accent >> 16) & 0xff) as f64;
        let g = ((p.accent >> 8) & 0xff) as f64;
        let b = (p.accent & 0xff) as f64;
        let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        if !dark {
            0xc0_39_2b
        } else if lum >= 128. {
            0xe0_6c_75
        } else {
            0xd0_56_52
        }
    }
}

/// 主题清单，顺序照选主题列表：先暗后亮，各自按名字排。
/// 切换明暗只看 `counterpart`：Ayu Mirage 的亮色对应 Ayu Light。
pub static THEMES: [ThemeDef; 13] = [
    ThemeDef {
        name: "Ayu Dark",
        dark: true,
        counterpart: 7,
        palette: Palette::AYU_DARK,
    },
    ThemeDef {
        name: "Ayu Mirage",
        dark: true,
        counterpart: 7,
        palette: Palette::AYU_MIRAGE,
    },
    ThemeDef {
        name: "Dark 2026",
        dark: true,
        counterpart: 11,
        palette: Palette::VSCODE_2026_DARK,
    },
    ThemeDef {
        name: "Gruvbox Dark",
        dark: true,
        counterpart: 8,
        palette: Palette::GRUVBOX_DARK,
    },
    ThemeDef {
        name: "Gruvbox Dark Hard",
        dark: true,
        counterpart: 9,
        palette: Palette::GRUVBOX_DARK_HARD,
    },
    ThemeDef {
        name: "Gruvbox Dark Soft",
        dark: true,
        counterpart: 10,
        palette: Palette::GRUVBOX_DARK_SOFT,
    },
    ThemeDef {
        name: "One Dark",
        dark: true,
        counterpart: 12,
        palette: Palette::ONE_DARK,
    },
    ThemeDef {
        name: "Ayu Light",
        dark: false,
        counterpart: 0,
        palette: Palette::AYU_LIGHT,
    },
    ThemeDef {
        name: "Gruvbox Light",
        dark: false,
        counterpart: 3,
        palette: Palette::GRUVBOX_LIGHT,
    },
    ThemeDef {
        name: "Gruvbox Light Hard",
        dark: false,
        counterpart: 4,
        palette: Palette::GRUVBOX_LIGHT_HARD,
    },
    ThemeDef {
        name: "Gruvbox Light Soft",
        dark: false,
        counterpart: 5,
        palette: Palette::GRUVBOX_LIGHT_SOFT,
    },
    ThemeDef {
        name: "Light 2026",
        dark: false,
        counterpart: 2,
        palette: Palette::VSCODE_2026_LIGHT,
    },
    ThemeDef {
        name: "One Light",
        dark: false,
        counterpart: 6,
        palette: Palette::ONE_LIGHT,
    },
];

/// 当前主题下标。默认 Light 2026，启动时按存档覆盖。
static ACTIVE: AtomicUsize = AtomicUsize::new(11);

/// 测试并行跑时每个用例各占一个线程，主题档也放线程上，免得一个用例的
/// 悬浮预览把另一个用例正在断言的主题改掉。
#[cfg(test)]
thread_local! {
    static ACTIVE_TEST: std::cell::Cell<usize> = const { std::cell::Cell::new(11) };
}

pub fn active_index() -> usize {
    #[cfg(test)]
    let ix = ACTIVE_TEST.with(|c| c.get());
    #[cfg(not(test))]
    let ix = ACTIVE.load(Ordering::Relaxed);
    ix.min(THEMES.len() - 1)
}

pub fn set_active(ix: usize) {
    #[cfg(test)]
    ACTIVE_TEST.with(|c| c.set(ix.min(THEMES.len() - 1)));
    ACTIVE.store(ix.min(THEMES.len() - 1), Ordering::Relaxed);
}

pub fn active() -> &'static ThemeDef {
    &THEMES[active_index()]
}

pub fn find(name: &str) -> Option<usize> {
    THEMES.iter().position(|t| t.name.eq_ignore_ascii_case(name))
}

/// 旧存档只有 `dark` 一个字段时，按 VS Code 2026 的明暗档起步。
pub fn index_for_dark(dark: bool) -> usize {
    find(if dark { "Dark 2026" } else { "Light 2026" }).unwrap_or(0)
}
