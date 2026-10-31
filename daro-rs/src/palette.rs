//! daro-rs 的语义色板。参照 Navicat Premium Lite 的观感：
//! 浅灰铬件（菜单 / 功能区 / 工具栏 / 状态栏）压在白内容底上，
//! 选中态是 Navicat 那套浅蓝 (#cce8ff)，树里"当前节点"用灰底。

#[derive(Clone, Copy)]
pub struct Palette {
    /// 内容底：列表 / 树 / 属性面板
    pub background: u32,
    /// 铬件底：菜单条 / 功能区 / 工具栏 / 状态栏
    pub chrome: u32,
    /// 硬分割线（面板外框、功能区下沿）
    pub border: u32,
    /// 面板之间的细线
    pub border_soft: u32,
    pub foreground: u32,
    pub muted_foreground: u32,
    /// 属性名、占位符这类更淡的字
    pub subtle_foreground: u32,
    /// 主色：表图标 / 链接 / 焦点
    pub accent: u32,
    /// 列表与活动项底色
    pub selection: u32,
    /// 树里"当前节点"的灰底
    pub selection_focus: u32,
    pub hover: u32,
    pub status_bar: u32,
}

impl Palette {
    pub const LIGHT: Self = Self {
        background: 0xffffff,
        chrome: 0xf0f0f0,
        border: 0xdbdbdb,
        border_soft: 0xe6e6e6,
        foreground: 0x1f1f1f,
        muted_foreground: 0x5f5f5f,
        subtle_foreground: 0x8c8c8c,
        accent: 0x0a84d8,
        selection: 0xcce8ff,
        selection_focus: 0xd9d9d9,
        hover: 0xe9e9e9,
        status_bar: 0xf0f0f0,
    };

    pub const DARK: Self = Self {
        background: 0x1e1e1e,
        chrome: 0x2b2b2b,
        border: 0x3d3d3d,
        border_soft: 0x343434,
        foreground: 0xe2e2e2,
        muted_foreground: 0xa6a6a6,
        subtle_foreground: 0x7d7d7d,
        accent: 0x4da3e8,
        selection: 0x1e4a6e,
        selection_focus: 0x3a3a3a,
        hover: 0x333333,
        status_bar: 0x2b2b2b,
    };

    pub fn of(dark: bool) -> Self {
        if dark { Self::DARK } else { Self::LIGHT }
    }
}
