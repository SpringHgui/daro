//! Tabler 图标主题的内置资源。
//!
//! 文件由 `scripts/fetch_tabler_icons.mjs` 从 `@tabler/icons` 抓取并规范化
//! （MIT，见 assets/tabler/LICENSE.txt）。清单在编译期从目录生成，
//! 缺文件不会打断构建，只会让该图标回退到 Zed 线稿。

const PREFIX: &str = "icons/tabler/";

include!(concat!(env!("OUT_DIR"), "/tabler_assets.rs"));

/// 按 `assets/tabler` 下的文件名取内嵌 SVG。
pub fn load_file(name: &str) -> Option<&'static [u8]> {
    FILES.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

/// 按资源路径取内嵌 SVG。
pub fn load(path: &str) -> Option<&'static [u8]> {
    load_file(path.strip_prefix(PREFIX)?)
}

pub fn all_paths() -> impl Iterator<Item = String> {
    FILES.iter().map(|(n, _)| format!("{PREFIX}{n}"))
}
