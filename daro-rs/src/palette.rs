//! daro-rs 的语义色板，取 Zed 默认主题族 One / Ayu / Gruvbox，外加 VS Code 2026
//! 默认的明暗两档：标题栏与状态栏压在 panel 上，编辑器（对象清单 / 数据页）再深
//! 一档，面板之间用 1px border 分隔，选中态是 element.selected。
//!
//! 色值直接抄 `zed/assets/themes/{one,ayu,gruvbox}/*.json` 与
//! `vscode/extensions/theme-defaults/themes/2026-{dark,light}.json`，别按截图手调；
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
    /// 树里"当前节点"的灰底
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
}

/// 一套可切换主题。字段取自 Zed 主题 JSON：
/// - `background` = editor.background，`chrome` / `status_bar` = title / status bar
/// - `selection` = element.selected，`selection_focus` / `hover` = element.hover
/// - `dialog_surface` / `tab_face` = surface / tab.inactive background
/// - `primary_face` = info.background（#rrggbb1a）压在 surface 上的实色
/// - `border_soft` = surface 与 border 按 0.62 相掺，取中间那档细线
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
        border_soft: 0x3f444f,
        foreground: 0xdce0e5,
        muted_foreground: 0xa9afbc,
        subtle_foreground: 0x7f8694,
        accent: 0x74ade8,
        on_accent: 0x1f2328,
        selection: 0x454a56,
        selection_focus: 0x3b414d,
        hover: 0x363c46,
        status_bar: 0x3b414d,
        dialog_surface: 0x2f343e,
        dialog_line: 0x464b57,
        tab_face: 0x2f343e,
        field_border: 0x464b57,
        field_edge: 0x464b57,
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
        line_number: 0x5c6370,
    };

    // One Light（one/one.json）
    pub const ONE_LIGHT: Self = Self {
        background: 0xfafafa,
        chrome: 0xdcdcdd,
        border: 0xc9c9ca,
        border_soft: 0xd6d6d7,
        foreground: 0x0a0a0a,
        muted_foreground: 0x58585a,
        subtle_foreground: 0x8a8b90,
        accent: 0x5c78e2,
        on_accent: 0xffffff,
        selection: 0xcacaca,
        selection_focus: 0xdfdfe0,
        hover: 0xdfdfe0,
        status_bar: 0xdcdcdd,
        dialog_surface: 0xebebec,
        dialog_line: 0xc9c9ca,
        tab_face: 0xebebec,
        field_border: 0xc9c9ca,
        field_edge: 0xc9c9ca,
        field_focus: 0x7d82e8,
        button_face: 0xebebec,
        button_border: 0xc9c9ca,
        primary_face: 0xe2e2fa,
        primary_face_hover: 0xcbcbf7,
        primary_border: 0xcbcdf6,
        primary_foreground: 0x0a0a0a,
        control_ring: 0x8a8b90,
        control_fill: 0x5c78e2,
        line_number: 0x9a9a9b,
    };

    // Ayu Dark（ayu/ayu.json）
    pub const AYU_DARK: Self = Self {
        background: 0x0d1016,
        chrome: 0x313337,
        border: 0x3f4043,
        border_soft: 0x333438,
        foreground: 0xbfbdb6,
        muted_foreground: 0x8a8986,
        subtle_foreground: 0x696a6a,
        accent: 0x5ac1fe,
        on_accent: 0x1f2328,
        selection: 0x3e4043,
        selection_focus: 0x2d2f34,
        hover: 0x2d2f34,
        status_bar: 0x313337,
        dialog_surface: 0x1f2127,
        dialog_line: 0x3f4043,
        tab_face: 0x1f2127,
        field_border: 0x3f4043,
        field_edge: 0x3f4043,
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
    };

    // Ayu Mirage（ayu/ayu.json）
    pub const AYU_MIRAGE: Self = Self {
        background: 0x242835,
        chrome: 0x464a52,
        border: 0x53565d,
        border_soft: 0x484b54,
        foreground: 0xcccac2,
        muted_foreground: 0x9a9a98,
        subtle_foreground: 0x7b7d7f,
        accent: 0x72cffe,
        on_accent: 0x1f2328,
        selection: 0x53565d,
        selection_focus: 0x43464f,
        hover: 0x43464f,
        status_bar: 0x464a52,
        dialog_surface: 0x353944,
        dialog_line: 0x53565d,
        tab_face: 0x353944,
        field_border: 0x53565d,
        field_edge: 0x53565d,
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
    };

    // Ayu Light（ayu/ayu.json）
    pub const AYU_LIGHT: Self = Self {
        background: 0xfcfcfc,
        chrome: 0xdcddde,
        border: 0xcfd1d2,
        border_soft: 0xdadbdc,
        foreground: 0x5c6166,
        muted_foreground: 0x8b8e92,
        subtle_foreground: 0xa9acae,
        accent: 0x3b9ee5,
        on_accent: 0x1f2328,
        selection: 0xcfd0d2,
        selection_focus: 0xdfe0e1,
        hover: 0xdfe0e1,
        status_bar: 0xdcddde,
        dialog_surface: 0xececed,
        dialog_line: 0xcfd1d2,
        tab_face: 0xececed,
        field_border: 0xcfd1d2,
        field_edge: 0xcfd1d2,
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
    };

    // Gruvbox Dark（gruvbox/gruvbox.json）
    pub const GRUVBOX_DARK: Self = Self {
        background: 0x282828,
        chrome: 0x4c4642,
        border: 0x5b534d,
        border_soft: 0x4e4844,
        foreground: 0xfbf1c7,
        muted_foreground: 0xc5b597,
        subtle_foreground: 0x998b78,
        accent: 0x83a598,
        on_accent: 0x1f2328,
        selection: 0x5b524c,
        selection_focus: 0x494340,
        hover: 0x494340,
        status_bar: 0x4c4642,
        dialog_surface: 0x3a3735,
        dialog_line: 0x5b534d,
        tab_face: 0x3a3735,
        field_border: 0x5b534d,
        field_edge: 0x5b534d,
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
    };

    // Gruvbox Dark Hard（gruvbox/gruvbox.json）
    pub const GRUVBOX_DARK_HARD: Self = Self {
        background: 0x1d2021,
        chrome: 0x4c4642,
        border: 0x5b534d,
        border_soft: 0x4e4844,
        foreground: 0xfbf1c7,
        muted_foreground: 0xc5b597,
        subtle_foreground: 0x998b78,
        accent: 0x83a598,
        on_accent: 0x1f2328,
        selection: 0x5b524c,
        selection_focus: 0x494340,
        hover: 0x494340,
        status_bar: 0x4c4642,
        dialog_surface: 0x393634,
        dialog_line: 0x5b534d,
        tab_face: 0x393634,
        field_border: 0x5b534d,
        field_edge: 0x5b534d,
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
    };

    // Gruvbox Dark Soft（gruvbox/gruvbox.json）
    pub const GRUVBOX_DARK_SOFT: Self = Self {
        background: 0x32302f,
        chrome: 0x4c4642,
        border: 0x5b534d,
        border_soft: 0x4f4844,
        foreground: 0xfbf1c7,
        muted_foreground: 0xc5b597,
        subtle_foreground: 0x998b78,
        accent: 0x83a598,
        on_accent: 0x1f2328,
        selection: 0x5b524c,
        selection_focus: 0x494340,
        hover: 0x494340,
        status_bar: 0x4c4642,
        dialog_surface: 0x3b3735,
        dialog_line: 0x5b534d,
        tab_face: 0x3b3735,
        field_border: 0x5b534d,
        field_edge: 0x5b534d,
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
    };

    // Gruvbox Light（gruvbox/gruvbox.json）
    pub const GRUVBOX_LIGHT: Self = Self {
        background: 0xfbf1c7,
        chrome: 0xd9c8a4,
        border: 0xc8b899,
        border_soft: 0xd6c6a3,
        foreground: 0x282828,
        muted_foreground: 0x5f5650,
        subtle_foreground: 0x897b6e,
        accent: 0x0b6678,
        on_accent: 0xffffff,
        selection: 0xc8b899,
        selection_focus: 0xddcca7,
        hover: 0xddcca7,
        status_bar: 0xd9c8a4,
        dialog_surface: 0xecddb4,
        dialog_line: 0xc8b899,
        tab_face: 0xecddb4,
        field_border: 0xc8b899,
        field_edge: 0xc8b899,
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
    };

    // Gruvbox Light Hard（gruvbox/gruvbox.json）
    pub const GRUVBOX_LIGHT_HARD: Self = Self {
        background: 0xf9f5d7,
        chrome: 0xd9c8a4,
        border: 0xc8b899,
        border_soft: 0xd6c6a4,
        foreground: 0x282828,
        muted_foreground: 0x5f5650,
        subtle_foreground: 0x897b6e,
        accent: 0x0b6678,
        on_accent: 0xffffff,
        selection: 0xc8b899,
        selection_focus: 0xddcca7,
        hover: 0xddcca7,
        status_bar: 0xd9c8a4,
        dialog_surface: 0xecddb5,
        dialog_line: 0xc8b899,
        tab_face: 0xecddb5,
        field_border: 0xc8b899,
        field_edge: 0xc8b899,
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
    };

    // Gruvbox Light Soft（gruvbox/gruvbox.json）
    pub const GRUVBOX_LIGHT_SOFT: Self = Self {
        background: 0xf2e5bc,
        chrome: 0xd9c8a4,
        border: 0xc8b899,
        border_soft: 0xd6c6a3,
        foreground: 0x282828,
        muted_foreground: 0x5f5650,
        subtle_foreground: 0x897b6e,
        accent: 0x0b6678,
        on_accent: 0xffffff,
        selection: 0xc8b899,
        selection_focus: 0xddcca7,
        hover: 0xddcca7,
        status_bar: 0xd9c8a4,
        dialog_surface: 0xecdcb3,
        dialog_line: 0xc8b899,
        tab_face: 0xecdcb3,
        field_border: 0xc8b899,
        field_edge: 0xc8b899,
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
    };

    // VS Code 2026 Light（2026-light.json），映射口径同上。
    pub const VSCODE_2026_LIGHT: Self = Self {
        background: 0xffffff,
        chrome: 0xfafafd,
        border: 0xe5e5e5,
        border_soft: 0xf0f1f2,
        foreground: 0x202020,
        muted_foreground: 0x606060,
        subtle_foreground: 0xbbbbbb,
        accent: 0x0069cc,
        on_accent: 0xffffff,
        selection: 0xd6d6d8,
        selection_focus: 0xe7e7e8,
        hover: 0xe6e6e9,
        status_bar: 0xfafafd,
        dialog_surface: 0xfafafd,
        dialog_line: 0xe4e5e6,
        tab_face: 0xfafafd,
        field_border: 0xd8d8d8,
        field_edge: 0xd8d8d8,
        field_focus: 0x0069cc,
        button_face: 0xeaeaea,
        button_border: 0xeaeaea,
        primary_face: 0x0069cc,
        primary_face_hover: 0x0063c1,
        primary_border: 0x0069cc,
        primary_foreground: 0xffffff,
        control_ring: 0x868686,
        control_fill: 0x0069cc,
        line_number: 0x606060,
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
