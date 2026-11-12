//! 参考客户端图标库：`assets/icons/legacy/size{16,24,32,48,64,128}/` 下按像素档存放。
//!
//! 素材来自参考客户端主程序的 PE 资源（TImageCollection / RT_RCDATA / RT_BITMAP），
//! 出处与逐张映射见 `assets/icons/legacy/SOURCES.json`。
//!
//! 取图按**逻辑尺寸的 2 倍**挑最近档（本应用基线 200% DPI）：
//! 15–18px 的树行/工具条取 32px 档，28px 功能区取 64px 档，
//! 信息卡 64px 大图取 128px 档；这样 200% DPI 下正好 1:1，不会放大发虚。
//!
//! 彩色位图必须走 `img()`：`svg()` 那条路径只认 alpha 蒙版，会把它们压成单色。

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use gpui_kit::*;

include!(concat!(env!("OUT_DIR"), "/legacy_index.rs"));

/// (像素档, 图标 id) → 位图。
static ICONS: LazyLock<HashMap<(u16, &'static str), Arc<Image>>> = LazyLock::new(|| {
    LEGACY_ICONS
        .iter()
        .map(|(class, id, bytes)| {
            (
                (*class, *id),
                Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec())),
            )
        })
        .collect()
});

const CLASSES: [u16; 6] = [16, 24, 32, 48, 64, 128];

/// 逻辑尺寸 → 像素档。基线按 200% DPI，取距离最近的一档（同距取大）。
fn class_for(size: f32) -> u16 {
    let target = (size.max(1.) * 2.).round() as u16;
    CLASSES
        .iter()
        .copied()
        .min_by_key(|class| (class.abs_diff(target), u16::MAX - *class))
        .unwrap_or(64)
}

fn image(id: &str, class: u16) -> Option<Arc<Image>> {
    ICONS.get(&(class, id)).cloned()
}

pub fn has(id: &str) -> bool {
    ICONS.keys().any(|(_, key)| *key == id)
}

fn pick(id: &str, size: f32) -> Option<Arc<Image>> {
    let wanted = class_for(size);
    std::iter::once(wanted)
        .chain(CLASSES.iter().copied().filter(|class| *class != wanted))
        .find_map(|class| image(id, class))
}

fn framed(image: Arc<Image>, size: f32) -> AnyElement {
    img(image)
        .flex_none()
        .w(px(size))
        .h(px(size))
        .into_any_element()
}

/// 按逻辑尺寸取参考客户端图标；id 缺失时回退到其它尺寸档，再缺失就留空位。
pub fn icon(id: &str, size: f32) -> AnyElement {
    match pick(id, size) {
        Some(image) => framed(image, size),
        None => div().flex_none().w(px(size)).h(px(size)).into_any_element(),
    }
}

/// 灰阶版本：给未打开/禁用的节点用（彩色位图直接去色）。
/// 库节点改走 Zed 线稿后暂时没有调用方，留作其它彩色位图的通用弱化入口。
#[allow(dead_code)]
pub fn icon_muted(id: &str, size: f32) -> AnyElement {
    match pick(id, size) {
        Some(image) => img(image)
            .grayscale(true)
            .flex_none()
            .w(px(size))
            .h(px(size))
            .into_any_element(),
        None => div().flex_none().w(px(size)).h(px(size)).into_any_element(),
    }
}
