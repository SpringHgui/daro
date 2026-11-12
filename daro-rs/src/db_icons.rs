//! 数据库图标：引擎品牌矢量 logo、库节点线稿与应用 logo。
//!
//! 引擎图标统一成品牌色实心瓦片 + 白色实心图形（矢量，出处见
//! `assets/icons/engines/SOURCES.json`），任意 DPI 都清晰；未连接的连接节点
//! 用同款图标去色。库节点走 Zed 线稿圆柱，与树里其它对象一致。

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use gpui_kit::component::Icon;
use gpui_kit::component::Sizable as _;
use gpui_kit::*;

use crate::mock::DbKind;
use crate::zed_icons::ZedIcon;

/// 应用 logo（透明底 PNG，256×256）：标题栏 15px、关于框 52px。
static LOGO: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../assets/icons/ui/logo.png").to_vec(),
    ))
});

/// 信息卡大图用的库圆柱矢量。
static DATABASE_HERO: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        include_bytes!("../assets/icons/ui/database_hero.svg").to_vec(),
    ))
});

/// 复选勾：白色描边矢量。
static CHECK: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        include_bytes!("../assets/icons/ui/check.svg").to_vec(),
    ))
});

pub fn check_icon() -> Arc<Image> {
    CHECK.clone()
}

/// 引擎品牌 logo：新建连接弹窗的类型磁贴、扩展列表、树节点共用。
static ENGINE_TILES: LazyLock<HashMap<&'static str, Arc<Image>>> = LazyLock::new(|| {
    [
        ("access", &include_bytes!("../assets/icons/engines/access.svg")[..]),
        ("mysql", &include_bytes!("../assets/icons/engines/mysql.svg")[..]),
        (
            "postgresql",
            &include_bytes!("../assets/icons/engines/postgresql.svg")[..],
        ),
        (
            "sqlserver",
            &include_bytes!("../assets/icons/engines/sqlserver.svg")[..],
        ),
        ("oracle", &include_bytes!("../assets/icons/engines/oracle.svg")[..]),
        ("sqlite", &include_bytes!("../assets/icons/engines/sqlite.svg")[..]),
        ("mariadb", &include_bytes!("../assets/icons/engines/mariadb.svg")[..]),
        ("mongodb", &include_bytes!("../assets/icons/engines/mongodb.svg")[..]),
        ("redis", &include_bytes!("../assets/icons/engines/redis.svg")[..]),
        (
            "snowflake",
            &include_bytes!("../assets/icons/engines/snowflake.svg")[..],
        ),
    ]
    .into_iter()
    .map(|(id, bytes)| {
        (
            id,
            Arc::new(Image::from_bytes(ImageFormat::Svg, bytes.to_vec())),
        )
    })
    .collect()
});

/// 小尺寸简化版：SQL Server 原稿是折面网线，16–18px 下会糊成噪点，只留缎带
/// 外轮廓实心填充；≥26px 仍用带折面的原稿。
static ENGINE_TILES_SMALL: LazyLock<HashMap<&'static str, Arc<Image>>> = LazyLock::new(|| {
    [(
        "sqlserver",
        &include_bytes!("../assets/icons/engines/sqlserver_small.svg")[..],
    )]
    .into_iter()
    .map(|(id, bytes)| {
        (
            id,
            Arc::new(Image::from_bytes(ImageFormat::Svg, bytes.to_vec())),
        )
    })
    .collect()
});

fn tile(id: &str, size: f32, muted: bool) -> AnyElement {
    let image = if size <= 24.0 {
        ENGINE_TILES_SMALL.get(id).or_else(|| ENGINE_TILES.get(id))
    } else {
        ENGINE_TILES.get(id)
    };
    match image {
        Some(image) => {
            let mut el = img(image.clone()).flex_none().w(px(size)).h(px(size));
            if muted {
                el = el.grayscale(true).opacity(0.55);
            }
            el.into_any_element()
        }
        None => database(size),
    }
}

/// 按引擎 id 取图标（树行等小尺寸处）。
pub fn icon(id: &str, size: f32) -> AnyElement {
    tile(id, size, false)
}

/// 引擎瓦片的矢量版：连接弹窗的类型磁贴 / 列表行 / 表单顶部 / 扩展卡片用。
pub fn vector_tile(id: &str, size: f32) -> AnyElement {
    tile(id, size, false)
}

/// 应用 logo：透明底，直接贴到当前底色上。
pub fn logo(size: f32) -> AnyElement {
    img(LOGO.clone())
        .flex_none()
        .w(px(size))
        .h(px(size))
        .into_any_element()
}

pub fn asset_id(kind: DbKind) -> &'static str {
    match kind {
        DbKind::MySql => "mysql",
        DbKind::PostgreSql => "postgresql",
        DbKind::SqlServer => "sqlserver",
        DbKind::SQLite => "sqlite",
        DbKind::Access => "access",
        DbKind::MongoDb => "mongodb",
        DbKind::Redis => "redis",
    }
}

/// 连接 / 引擎图标。
pub fn engine(kind: DbKind, size: f32) -> AnyElement {
    icon(asset_id(kind), size)
}

/// 查询页运行上下文栏的引擎图标。
pub fn engine_bar(kind: DbKind, size: f32) -> AnyElement {
    engine(kind, size)
}

/// 未连接的连接节点图标：厂商 logo 去色。
pub fn engine_muted(kind: DbKind, size: f32) -> AnyElement {
    tile(asset_id(kind), size, true)
}

/// 库节点图标：与树里表 / 视图等对象同款的单色线稿圆柱，跟随主题文字色。
pub fn database(size: f32) -> AnyElement {
    Icon::new(ZedIcon::Database)
        .with_size(px(size))
        .into_any_element()
}

/// 信息卡大图标：库圆柱的矢量版。
pub fn database_hero(size: f32) -> AnyElement {
    img(DATABASE_HERO.clone())
        .flex_none()
        .w(px(size))
        .h(px(size))
        .into_any_element()
}

// ── 信息卡 64px 大图标：与树节点同一套 Zed 线稿/品牌 logo，
// 不再依赖按像素档准备的位图。 ──

fn hero_glyph(icon: ZedIcon, size: f32) -> AnyElement {
    Icon::new(icon).with_size(px(size)).into_any_element()
}

pub fn hero_table(size: f32) -> AnyElement {
    hero_glyph(ZedIcon::Table, size)
}

pub fn hero_view(size: f32) -> AnyElement {
    hero_glyph(ZedIcon::Eye, size)
}

pub fn hero_matview(size: f32) -> AnyElement {
    hero_glyph(ZedIcon::Layers, size)
}

pub fn hero_function(size: f32) -> AnyElement {
    hero_glyph(ZedIcon::Code, size)
}

pub fn hero_query(size: f32) -> AnyElement {
    hero_glyph(ZedIcon::FileCode, size)
}

pub fn hero_schema(size: f32) -> AnyElement {
    hero_glyph(ZedIcon::Box, size)
}

pub fn hero_folder(size: f32) -> AnyElement {
    hero_glyph(ZedIcon::Folder, size)
}

/// 连接信息卡大图标：与树节点同款厂商 logo，大尺寸下仍是矢量。
pub fn hero_engine(kind: DbKind, size: f32) -> AnyElement {
    engine(kind, size)
}
