//! 手绘图标：把 Navicat Lite 那套彩色图标用矢量重画一遍。
//!
//! 为什么不用位图 / SVG：GPUI 的 `svg()` 一律按**单色 alpha 蒙版**上色
//! （见 `gpui::svg_renderer::render_alpha_mask`），彩色 SVG 进去出来只剩一个颜色；
//! 而功能区那批图标恰恰是「蓝表 + 红文档 + 绿加号」的多色组合。
//! 位图又会在大图标（右侧信息卡）上糊掉。
//! 所以统一走 `canvas` + 四边形 / 多边形 / 折线自己画：
//! 任意 DPI 都清晰，颜色是图标本色（Navicat 的图标也不跟明暗主题走）。
//!
//! 坐标系：所有图形都按 **24×24 设计格**落笔，绘制时按目标尺寸整体缩放。
//! 这样同一份图标能同时喂 15px 的树行和 64px 的信息卡大图。

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::Sizable as _;
use gpui_kit::*;

use crate::mock::DbKind;
use crate::to_hsla;

// ── 图标本色（与参考工具取样一致，不参与明暗主题）───────────────
const BLUE_DEEP: u32 = 0x1c6fb5;
const BLUE_MID: u32 = 0x63b3e6;
const GREEN: u32 = 0x45b649;
const GREEN_DEEP: u32 = 0x2b8c33;
const GREEN_BRIGHT: u32 = 0x57c046;
const YELLOW: u32 = 0xf5b731;
const YELLOW_DEEP: u32 = 0xe09520;
const ORANGE: u32 = 0xef8f31;
const ORANGE_DEEP: u32 = 0xcf7018;
const ORANGE_LIGHT: u32 = 0xf7b56d;
const STEEL_DEEP: u32 = 0x64707c;
const WHITE: u32 = 0xffffff;
const HAIR: u32 = 0x5d4233;
// 右侧「模式」大图那套绿
const SCHEMA_HEAD: u32 = 0x0b7a3f;
const SCHEMA_BODY: u32 = 0x3ec488;
const SCHEMA_CELL: u32 = 0x5fd3a0;
const SCHEMA_DARK: u32 = 0x33ab75;
const SCHEMA_WIRE: u32 = 0x5d6f80;

/// 图标种类。树 / 功能区 / 工具条都走这一套，
/// 单色线稿（lucide）通过 `Lucide` 变体继续可用。
#[derive(Clone, Copy, PartialEq)]
pub enum Glyph {
    // ── 树节点 ──
    /// 分组文件夹
    Folder,
    /// 连接：按数据库品牌上色
    Conn(DbKind),
    /// 库
    Db,
    /// 模式（三个绿块的拓扑图，与右侧信息卡同款）
    Schema,
    /// 表
    Table,
    /// 视图
    View,
    /// 实体化视图
    MatView,
    /// 函数
    Function,
    /// 查询
    Query,
    /// 字段 / 列
    Column,
    // ── 功能区 ──
    RibbonConn,
    RibbonNewQuery,
    RibbonTable,
    RibbonView,
    RibbonMatView,
    RibbonFunction,
    RibbonUser,
    RibbonTools,
    RibbonQuery,
    RibbonBackup,
    RibbonAutoRun,
    RibbonModel,
    RibbonBi,
    // ── 对象面板工具条 ──
    OpenTable,
    DesignTable,
    NewTable,
    AiTable,
    DropTable,
    ImportWizard,
    ExportWizard,
    // ── 数据页工具条 ──
    /// 开始事务：灰圆柱 + 蓝三角
    Txn,
    /// 单元格编辑器：灰框 + 蓝铅笔
    CellEdit,
    /// 筛选 & 排序：灰漏斗 + 蓝下箭头
    FilterSort,
    /// 列：蓝色田字格
    Columns,
    /// 历史：蓝色回拨时钟
    History,
    // ── 查询页工具条 ──
    /// 保存：灰蓝软盘
    SaveFile,
    /// 美化 SQL：魔杖 + 蓝星
    Beautify,
    /// 代码段：蓝色圆环
    Snippet,
    /// 主键：金色钥匙
    KeyGold,
    // ── 右侧信息卡 ──
    HeroSchema,
    HeroTable,
    HeroDb,
    // ── 标题栏 ──
    Logo,
    /// 单色线稿，颜色由调用方给
    Lucide(IconName),
}

impl From<IconName> for Glyph {
    fn from(name: IconName) -> Self {
        // 常见的几款直接映射到重画版，其余退回线稿
        match name {
            IconName::Table2 => Glyph::Table,
            IconName::Eye => Glyph::View,
            IconName::Layers => Glyph::MatView,
            IconName::SquareFunction => Glyph::Function,
            IconName::SquareTerminal => Glyph::Query,
            IconName::Database => Glyph::Db,
            IconName::Folder | IconName::FolderOpen | IconName::FolderClosed => Glyph::Folder,
            IconName::Boxes => Glyph::Schema,
            other => Glyph::Lucide(other),
        }
    }
}

impl Glyph {
    /// 单色线稿（需要调用方给主题色）还是彩色图标（自带本色）。
    #[allow(dead_code)]
    pub fn is_colored(self) -> bool {
        !matches!(self, Glyph::Lucide(_))
    }
}

// ── 画笔 ──────────────────────────────────────────────────────

/// 把 24 单位设计格映射到目标框内的画笔。
struct Pen<'a> {
    win: &'a mut Window,
    ox: Pixels,
    oy: Pixels,
    k: f32,
}

impl<'a> Pen<'a> {
    fn x(&self, v: f32) -> Pixels {
        self.ox + px(v * self.k)
    }
    fn y(&self, v: f32) -> Pixels {
        self.oy + px(v * self.k)
    }
    fn l(&self, v: f32) -> Pixels {
        px(v * self.k)
    }

    /// 直角矩形
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: u32) {
        self.rrect(x, y, w, h, 0., c);
    }

    /// 圆角矩形（半径按设计格给）
    fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: u32) {
        self.rrect_a(x, y, w, h, r, c, 1.);
    }

    fn rrect_a(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: u32, a: f32) {
        let b = Bounds::new(point(self.x(x), self.y(y)), size(self.l(w), self.l(h)));
        let color = if a >= 1. {
            to_hsla(c)
        } else {
            to_hsla(c).opacity(a)
        };
        self.win.paint_quad(quad(
            b,
            px(r * self.k),
            color,
            Edges::<Pixels>::default(),
            transparent_black(),
            BorderStyle::default(),
        ));
    }

    /// 圆（半径按设计格给）
    fn circle(&mut self, cx: f32, cy: f32, r: f32, c: u32) {
        self.rrect(cx - r, cy - r, r * 2., r * 2., r, c);
    }

    fn circle_a(&mut self, cx: f32, cy: f32, r: f32, c: u32, a: f32) {
        self.rrect_a(cx - r, cy - r, r * 2., r * 2., r, c, a);
    }

    /// 填充多边形
    fn poly(&mut self, pts: &[(f32, f32)], c: u32) {
        if pts.len() < 3 {
            return;
        }
        let mut b = PathBuilder::fill();
        b.move_to(point(self.x(pts[0].0), self.y(pts[0].1)));
        for p in &pts[1..] {
            b.line_to(point(self.x(p.0), self.y(p.1)));
        }
        b.close();
        if let Ok(path) = b.build() {
            self.win.paint_path(path, to_hsla(c));
        }
    }

    /// 折线
    fn stroke(&mut self, pts: &[(f32, f32)], w: f32, c: u32) {
        if pts.len() < 2 {
            return;
        }
        let mut b = PathBuilder::stroke(self.l(w));
        b.move_to(point(self.x(pts[0].0), self.y(pts[0].1)));
        for p in &pts[1..] {
            b.line_to(point(self.x(p.0), self.y(p.1)));
        }
        if let Ok(path) = b.build() {
            self.win.paint_path(path, to_hsla(c));
        }
    }

    /// 贝塞尔折线：segs 为 (控制点, 终点) 交替
    fn curve(&mut self, from: (f32, f32), segs: &[((f32, f32), (f32, f32))], w: f32, c: u32) {
        let mut b = PathBuilder::stroke(self.l(w));
        b.move_to(point(self.x(from.0), self.y(from.1)));
        for (ctrl, to) in segs {
            b.curve_to(
                point(self.x(to.0), self.y(to.1)),
                point(self.x(ctrl.0), self.y(ctrl.1)),
            );
        }
        if let Ok(path) = b.build() {
            self.win.paint_path(path, to_hsla(c));
        }
    }

    /// 圆环描边（用四段三次贝塞尔拼整圆，比四边矩形更像镜片）
    fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32, c: u32) {
        const K: f32 = 0.552_284_8;
        let mut b = PathBuilder::stroke(self.l(width));
        b.move_to(point(self.x(cx + r), self.y(cy)));
        let arc = |b: &mut PathBuilder, _from: (f32, f32), to: (f32, f32), ctrl1: (f32, f32), ctrl2: (f32, f32)| {
            b.cubic_bezier_to(
                point(self.x(to.0), self.y(to.1)),
                point(self.x(ctrl1.0), self.y(ctrl1.1)),
                point(self.x(ctrl2.0), self.y(ctrl2.1)),
            );
        };
        let k = r * K;
        arc(&mut b, (cx + r, cy), (cx, cy + r), (cx + r, cy + k), (cx + k, cy + r));
        arc(&mut b, (cx, cy + r), (cx - r, cy), (cx - k, cy + r), (cx - r, cy + k));
        arc(&mut b, (cx - r, cy), (cx, cy - r), (cx - r, cy - k), (cx - k, cy - r));
        arc(&mut b, (cx, cy - r), (cx + r, cy), (cx + k, cy - r), (cx + r, cy - k));
        b.close();
        if let Ok(path) = b.build() {
            self.win.paint_path(path, to_hsla(c));
        }
    }

    /// 旋转矩形（中心点 / 宽高 / 角度，度数）
    fn rot_rect(&mut self, cx: f32, cy: f32, w: f32, h: f32, deg: f32, c: u32) {
        let rad = deg.to_radians();
        let (s, co) = (rad.sin(), rad.cos());
        let (hw, hh) = (w / 2., h / 2.);
        let pt = |dx: f32, dy: f32| (cx + dx * co - dy * s, cy + dx * s + dy * co);
        let pts = [pt(-hw, -hh), pt(hw, -hh), pt(hw, hh), pt(-hw, hh)];
        self.poly(&pts, c);
    }
}

/// 两个颜色按权重混合（图标内部的压边、阴影用）
fn blend_color(a: u32, b: u32, t: f32) -> u32 {
    let mix = |x: u32, y: u32| {
        ((x as f32 * (1. - t) + y as f32 * t).round() as u32).min(255)
    };
    (mix(a >> 16 & 0xff, b >> 16 & 0xff) << 16)
        | (mix(a >> 8 & 0xff, b >> 8 & 0xff) << 8)
        | mix(a & 0xff, b & 0xff)
}

// ── 通用零件 ──────────────────────────────────────────────────

/// 表卡片：参考工具里出现频率最高的那个图标，也是「还原度」最显眼的一处。
///
/// 结构照实测取样：深色表头（约三成高）+ 2×2 棋盘表身。四个格子的明度
/// 从左上到右下逐级加深（#cee8ff → #94cff9 → #adddff → #7cc3f9），
/// 格子之间是一条比格子更浅的细线——放大是玻璃感，缩到 15px 也不会糊成条纹。
fn table_card(
    p: &mut Pen,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    head: u32,
    tl: u32,
    tr: u32,
    bl: u32,
    br: u32,
) {
    let r = (w * 0.11).min(2.4);
    let hh = h * 0.32;
    let bw = w * 0.55;
    let bh = (h - hh) * 0.52;
    let bottom = h - hh - bh;
    // 四个格子
    p.rrect(x, y + hh, bw, bh, 0., tl);
    p.rect(x + bw, y + hh, w - bw, bh, tr);
    p.rect(x, y + hh + bh, bw, bottom, bl);
    p.rect(x + bw, y + hh + bh, w - bw, bottom, br);
    // 分格线
    let line = (w * 0.026).max(0.35);
    p.rect(x + bw, y + hh, line, h - hh, blend_color(tl, 0xffffff, 0.32));
    p.rect(x, y + hh + bh, w, line, blend_color(tr, 0xffffff, 0.32));
    // 表头压在最上面
    p.rrect(x, y, w, hh, r, head);
    p.rect(x, y + hh - 0.5, w, 0.5, blend_color(head, 0x000000, 0.22));
    p.rect(x + line, y + 0.5, w - line * 2., 0.7, blend_color(head, 0xffffff, 0.35));
}

/// 蓝表（树 / 对象列表 / 功能区同款）
fn table(p: &mut Pen, x: f32, y: f32, w: f32, h: f32) {
    table_card(
        p,
        x,
        y,
        w,
        h,
        0x0a7cd4,
        0xd2ebff,
        0x93cff9,
        0xb2ddfd,
        0x7cc3f9,
    );
}

/// 文档窗口（查询 / 新建查询的底）
fn sheet(p: &mut Pen, x: f32, y: f32, w: f32, h: f32, body: u32, head: u32) {
    p.rrect(x, y, w, h, 1.6, body);
    p.rrect(x, y, w, h * 0.24, 1.6, head);
    for i in 0..3 {
        p.circle_a(
            x + w * 0.2 + i as f32 * w * 0.15,
            y + h * 0.12,
            w * 0.04,
            WHITE,
            0.9,
        );
    }
}

/// 绿底白加号（新建类图标的角标）
fn plus_badge(p: &mut Pen, cx: f32, cy: f32, r: f32) {
    p.circle(cx, cy, r, WHITE);
    p.circle(cx, cy, r * 0.9, GREEN_BRIGHT);
    p.rect(cx - r * 0.46, cy - r * 0.15, r * 0.92, r * 0.3, WHITE);
    p.rect(cx - r * 0.15, cy - r * 0.46, r * 0.3, r * 0.92, WHITE);
}

/// 圆底白减号
fn minus_badge(p: &mut Pen, cx: f32, cy: f32, r: f32, bg: u32) {
    p.circle(cx, cy, r, WHITE);
    p.circle(cx, cy, r * 0.9, bg);
    p.rect(cx - r * 0.46, cy - r * 0.16, r * 0.92, r * 0.32, WHITE);
}

/// 数据库圆柱（库 / 连接）
fn cylinder(p: &mut Pen, cx: f32, cy: f32, w: f32, h: f32, body: u32, dark: u32) {
    let r = w / 2.;
    let top = cy - h / 2. + r * 0.55;
    p.rrect(cx - w / 2., top, w, h - r * 0.55, r * 0.45, body);
    p.circle(cx, top + r * 0.1, r, dark);
    p.circle(cx, top + r * 0.1, r * 0.72, body);
    p.rrect_a(cx - w / 2., cy, w, h * 0.1, r * 0.4, dark, 0.5);
}

/// 眼镜（视图 / 实体化视图的标志件）：粗圆环 + 透明镜片 + 鼻梁。
fn glasses(p: &mut Pen, cx: f32, cy: f32, scale: f32, tint: u32, lens: u32) {
    let r = 2.75 * scale;
    let stroke = 1.25 * scale;
    let dx = 3.15 * scale;
    p.circle(cx - dx, cy, r - stroke * 0.25, lens);
    p.circle(cx + dx, cy, r - stroke * 0.25, lens);
    p.ring(cx - dx, cy, r - stroke / 2., stroke, tint);
    p.ring(cx + dx, cy, r - stroke / 2., stroke, tint);
    p.stroke(
        &[
            (cx - dx + r - stroke, cy - stroke * 0.15),
            (cx + dx - r + stroke, cy - stroke * 0.15),
        ],
        stroke * 0.9,
        tint,
    );
}

/// 品牌色（连配色都照各家官方标的观感取，树里一眼能分出引擎）
fn brand(kind: DbKind) -> (u32, u32) {
    match kind {
        DbKind::MySql => (0x1f86c8, 0x0f5f96),
        DbKind::PostgreSql => (0x39709b, 0x244f72),
        DbKind::SqlServer => (0xc0392b, 0x8e2a1f),
        DbKind::SQLite => (0x0f80b0, 0x003b57),
        DbKind::MongoDb => (0x4db33d, 0x357a2a),
        DbKind::Redis => (0xd0342c, 0x952018),
    }
}

// ── 逐个图标 ──────────────────────────────────────────────────

fn draw(g: Glyph, p: &mut Pen) {
    match g {
        Glyph::Lucide(_) => {}

        // 分组文件夹（Navicat 的黄文件夹：后片深、前片浅、上沿一道高光）
        Glyph::Folder => {
            p.rrect(2.0, 6.0, 20.0, 13.4, 1.8, 0xe8a33d);
            p.rrect(2.0, 4.2, 9.4, 4.6, 1.6, 0xf3b957);
            p.rrect(2.6, 7.8, 18.8, 10.8, 1.2, 0xf7c96a);
            p.rect(2.6, 7.8, 18.8, 1.1, 0xfadfa0);
        }

        // 连接：按引擎上色的小方块 + 白色数据库圆柱（两道刻线，缩到 16px 也认得出）
        Glyph::Conn(kind) => {
            let (c, d) = brand(kind);
            p.rrect(2.4, 2.4, 19.2, 19.2, 3.4, d);
            p.rrect(2.4, 2.4, 19.2, 18.0, 3.4, c);
            p.rect(2.4, 3.4, 19.2, 1.2, blend_color(c, WHITE, 0.45));
            // 圆柱
            p.rrect(6.4, 6.6, 11.2, 10.4, 5.2, WHITE);
            p.rect(6.4, 11.6, 11.2, 1.0, d);
            p.rect(6.4, 14.4, 11.2, 1.0, d);
            p.rrect(6.4, 5.6, 11.2, 4.4, 2.2, WHITE);
            p.rect(6.4, 9.2, 11.2, 0.9, blend_color(d, WHITE, 0.2));
        }

        Glyph::Db | Glyph::HeroDb => {
            cylinder(p, 12., 12.2, 15.0, 15.0, BLUE_MID, BLUE_DEEP);
            p.circle_a(12., 5.6, 7.5, WHITE, 0.45);
        }

        // 模式 = 三个绿块的拓扑图（树里 15px、信息卡 62px 同一份）
        Glyph::Schema | Glyph::HeroSchema => {
            let w = 10.6;
            let h = 6.2;
            let (tx, ty) = (12.0 - w / 2., 2.2);
            let (bx, by) = (0.6, 13.4);
            let (rx, ry) = (12.8, 13.4);
            p.rect(11.5, 7.2, 1.0, 4.2, SCHEMA_WIRE);
            p.rect(5.6, 10.6, 12.8, 1.0, SCHEMA_WIRE);
            p.rect(11.0, 12.4, 1.2, 3.2, SCHEMA_WIRE);
            for (x, y) in [(tx, ty), (bx, by), (rx, ry)] {
                p.rrect(x, y, w, h, 1.3, SCHEMA_BODY);
                p.rrect(x, y, w, h * 0.38, 1.3, SCHEMA_HEAD);
                p.rect(x + 0.5, y + h * 0.4, w * 0.48, h * 0.28, SCHEMA_CELL);
                p.rect(x + w * 0.5, y + h * 0.4, w * 0.48, h * 0.28, SCHEMA_BODY);
                p.rect(x + 0.5, y + h * 0.68, w * 0.48, h * 0.28, SCHEMA_CELL);
                p.rect(x + w * 0.5, y + h * 0.68, w * 0.48, h * 0.28, SCHEMA_DARK);
            }
        }

        Glyph::Table | Glyph::HeroTable => table(p, 1.8, 2.6, 20.4, 18.8),

        Glyph::View => {
            table_card(
                p,
                1.8,
                1.6,
                20.4,
                15.2,
                0x5aa9e0,
                0xdcefff,
                0xa9d9f8,
                0xc2e3fb,
                0x93cdf5,
            );
            glasses(p, 12.0, 16.9, 1.05, 0x0d63a8, 0xc2e3fb);
        }

        Glyph::MatView => {
            table_card(
                p,
                1.8,
                1.6,
                20.4,
                15.2,
                0xdf5138,
                0xfbd9c9,
                0xf0a98e,
                0xf6c2ab,
                0xea987c,
            );
            glasses(p, 12.0, 16.9, 1.05, 0xb5311c, 0xf6c2ab);
        }

        Glyph::Function | Glyph::RibbonFunction => {
            // 手绘 f(x)：蓝墨水、斜体，笔画比线稿粗一档才立得住
            let c = 0x1a7fd0;
            p.curve(
                (6.2, 18.4),
                &[
                    ((5.8, 13.0), (6.4, 9.0)),
                    ((7.2, 4.2), (9.6, 3.6)),
                    ((11.2, 3.2), (10.4, 6.2)),
                ],
                2.2,
                c,
            );
            p.rect(4.0, 10.4, 6.2, 1.8, c);
            p.curve(
                (14.6, 5.0),
                &[((12.4, 8.6), (12.4, 11.9)), ((12.4, 15.2), (14.6, 18.6))],
                1.8,
                c,
            );
            p.curve(
                (20.6, 5.0),
                &[((22.8, 8.6), (22.8, 11.9)), ((22.8, 15.2), (20.6, 18.6))],
                1.8,
                c,
            );
            p.stroke(&[(14.8, 8.8), (20.2, 15.8)], 1.9, c);
            p.stroke(&[(20.2, 8.8), (14.8, 15.8)], 1.9, c);
        }

        // 查询：后一张红窗、前一张蓝窗（Navicat 的固定构图）
        Glyph::Query | Glyph::RibbonQuery => {
            sheet(p, 7.4, 0.8, 14.4, 13.8, 0xf9c9b5, 0xe2553a);
            sheet(p, 1.2, 5.0, 15.6, 16.8, 0xf4faff, 0x4c9fdc);
            p.rect(3.2, 10.4, 11.2, 1.4, 0x74b6e6);
            p.rect(3.2, 13.6, 8.6, 1.4, 0xa8d8f7);
            p.rect(3.2, 16.8, 9.8, 1.4, 0xa8d8f7);
        }

        // 连接 = 一公一母两个插头斜着对插 + 亮绿加号
        Glyph::RibbonConn => {
            // 母头（右上）
            p.rot_rect(15.6, 8.2, 8.4, 8.0, 45., 0x8f9dab);
            p.rot_rect(15.6, 8.2, 6.0, 8.0, 45., 0xa8b5c2);
            p.rot_rect(15.6, 8.2, 3.2, 4.2, 45., 0x66747f);
            // 公头（左下）
            p.rot_rect(8.2, 15.6, 8.4, 8.0, 45., 0x8f9dab);
            p.rot_rect(8.2, 15.6, 6.0, 8.0, 45., 0xa8b5c2);
            for off in [-1.9f32, 1.9] {
                let (dx, dy) = (off * 0.707, -off * 0.707);
                p.rot_rect(11.6 + dx, 12.2 + dy, 1.8, 3.8, 45., 0x5f6d7a);
            }
            plus_badge(p, 19.4, 19.0, 3.8);
        }

        Glyph::RibbonNewQuery => {
            sheet(p, 7.2, 0.8, 14.6, 14.0, 0xf9c9b5, 0xe2553a);
            sheet(p, 1.2, 4.8, 15.8, 17.0, 0xf4faff, 0x4c9fdc);
            p.rect(3.2, 10.2, 11.4, 1.4, 0x74b6e6);
            p.rect(3.2, 13.4, 8.8, 1.4, 0xa8d8f7);
            p.rect(3.2, 16.6, 10.0, 1.4, 0xa8d8f7);
            plus_badge(p, 18.9, 18.6, 4.3);
        }

        Glyph::RibbonTable => table(p, 1.6, 2.4, 20.8, 19.2),

        Glyph::RibbonView => {
            table_card(
                p,
                1.6,
                1.2,
                20.8,
                15.4,
                0x5aa9e0,
                0xdcefff,
                0xa9d9f8,
                0xc2e3fb,
                0x93cdf5,
            );
            glasses(p, 12.0, 17.2, 1.12, 0x0d63a8, 0xc2e3fb);
        }

        Glyph::RibbonMatView => {
            table_card(
                p,
                1.6,
                1.2,
                20.8,
                15.4,
                0xdf5138,
                0xfbd9c9,
                0xf0a98e,
                0xf6c2ab,
                0xea987c,
            );
            glasses(p, 12.0, 17.2, 1.12, 0xb5311c, 0xf6c2ab);
        }

        // 角色：黄衣人像（深色短发 + 肤色脸 + 圆肩）
        Glyph::RibbonUser => {
            // 肩 / 衣
            p.rrect(3.4, 14.6, 17.2, 8.4, 4.4, YELLOW);
            p.rect(3.4, 14.6, 17.2, 1.9, YELLOW_DEEP);
            // 颈
            p.rect(10.5, 11.8, 3.0, 3.6, 0xdfae7d);
            // 头发（后片）
            p.circle(12.0, 7.8, 4.8, HAIR);
            // 脸
            p.circle(12.0, 9.4, 4.15, 0xe8b98c);
            // 刘海：盖住额头，留出下半张脸
            p.rrect(7.6, 3.0, 8.8, 4.6, 3.0, HAIR);
            p.poly(&[(7.6, 6.0), (12.0, 8.0), (16.4, 6.0), (16.4, 3.4), (7.6, 3.4)], HAIR);
            // 领口
            p.poly(&[(9.0, 14.6), (12.0, 17.4), (15.0, 14.6)], 0xfdf3d8);
        }

        // 其它：蓝色托盘里插着扳手 / 螺丝刀
        Glyph::RibbonTools => {
            p.rot_rect(8.2, 9.6, 2.6, 12.0, 0., 0x7a8896);
            p.ring(8.2, 5.2, 2.5, 1.4, 0x7a8896);
            p.poly(&[(6.4, 0.8), (10.0, 0.8), (8.2, 4.0)], 0x7a8896);
            p.rot_rect(14.8, 8.6, 2.0, 9.4, 0., 0x93a1ae);
            p.rot_rect(14.8, 13.8, 3.2, 3.2, 0., 0x6c7a88);
            p.rrect(1.4, 14.6, 21.2, 7.4, 2.2, 0x1f7fc4);
            p.rect(1.4, 14.6, 21.2, 2.0, 0x3d9bd8);
        }

        // 备份：灰色圆角块 + 白色循环箭头（Ent 角标由功能区叠在右下角）
        Glyph::RibbonBackup => {
            p.rrect(2.4, 1.6, 19.2, 17.4, 3.4, 0x8e9ba8);
            p.rrect(2.4, 1.6, 19.2, 15.8, 3.4, 0xa4b1be);
            // 环
            p.ring(12.0, 10.2, 5.0, 2.3, WHITE);
            // 右上开一个缺口（用块底色盖掉），形成「循环」的断口
            p.poly(&[(12.4, 2.4), (19.6, 2.4), (19.6, 9.6), (12.4, 9.6)], 0xa4b1be);
            // 断口处压一个顺时针的箭头
            p.poly(&[(11.6, 2.0), (16.2, 3.4), (12.4, 6.8)], WHITE);
        }

        // 自动运行：青绿机器人（白脸 + 深色眼 + 红天线球）
        Glyph::RibbonAutoRun => {
            p.rect(11.3, 0.6, 1.4, 2.8, 0x2c8f88);
            p.circle(12.0, 1.2, 1.5, 0xe0483a);
            p.rrect(2.2, 3.0, 19.6, 14.4, 5.0, 0x35b0a6);
            p.rrect(2.2, 3.0, 19.6, 5.2, 5.0, 0x53c3b8);
            p.rrect(5.0, 8.2, 14.0, 8.4, 2.6, WHITE);
            p.circle(9.0, 12.2, 2.3, 0x1f2d33);
            p.circle(15.0, 12.2, 2.3, 0x1f2d33);
            p.circle_a(8.7, 11.7, 0.85, WHITE, 0.95);
            p.circle_a(14.7, 11.7, 0.85, WHITE, 0.95);
        }

        // 模型：橙色立方体（上小下大 + 连杆）
        Glyph::RibbonModel => {
            let cube = |p: &mut Pen, cx: f32, cy: f32, s: f32| {
                let (w, h) = (s * 0.86, s * 0.5);
                p.poly(
                    &[
                        (cx, cy - h),
                        (cx + w, cy - h * 0.45),
                        (cx, cy + h * 0.1),
                        (cx - w, cy - h * 0.45),
                    ],
                    ORANGE_LIGHT,
                );
                p.poly(
                    &[
                        (cx - w, cy - h * 0.45),
                        (cx, cy + h * 0.1),
                        (cx, cy + h * 1.05),
                        (cx - w, cy + h * 0.5),
                    ],
                    ORANGE_DEEP,
                );
                p.poly(
                    &[
                        (cx + w, cy - h * 0.45),
                        (cx, cy + h * 0.1),
                        (cx, cy + h * 1.05),
                        (cx + w, cy + h * 0.5),
                    ],
                    ORANGE,
                );
            };
            p.rect(10.4, 8.6, 1.8, 4.2, 0xd5671a);
            cube(p, 8.4, 6.4, 7.2);
            cube(p, 15.6, 15.4, 7.4);
        }

        // BI：紫红色柱状图
        Glyph::RibbonBi => {
            p.rrect(1.2, 8.4, 4.4, 9.0, 1.0, 0xd05fb8);
            p.rrect(6.8, 4.4, 4.4, 13.0, 1.0, 0xb03bd0);
            p.rrect(12.4, 10.4, 4.4, 7.0, 1.0, 0x8e3bbf);
            p.rrect(18.0, 2.4, 4.2, 15.0, 1.0, 0xa64bd8);
            p.rect(1.0, 18.2, 22.0, 1.4, 0x6f2ea8);
        }

        // 打开表：浅蓝的开口文件夹
        Glyph::OpenTable => {
            p.rrect(2.0, 4.2, 9.8, 3.4, 1.2, 0x5aa9e0);
            p.poly(
                &[(2.0, 7.2), (22.0, 7.2), (19.0, 19.4), (2.0, 19.4)],
                0x8ec9ef,
            );
            p.poly(
                &[(2.8, 8.4), (21.2, 8.4), (18.6, 18.4), (2.8, 18.4)],
                0xc9e6fb,
            );
            p.rect(2.0, 7.2, 20.0, 1.6, 0x3f93d0);
            p.rect(2.6, 9.6, 6.6, 1.0, WHITE);
        }

        // 设计表：蓝铅笔
        Glyph::DesignTable => {
            p.poly(&[(3.0, 17.6), (18.4, 2.6), (21.4, 5.6), (6.0, 20.6)], 0x3f8fbf);
            p.poly(&[(3.0, 17.6), (6.0, 20.6), (2.0, 22.0)], 0xf0d9a0);
            p.poly(
                &[(18.4, 2.6), (21.4, 5.6), (19.8, 7.2), (16.8, 4.2)],
                0x2f7099,
            );
            p.rect(9.4, 11.0, 4.0, 3.0, 0x9fd0ea);
        }

        // 新建表：绿圆加号
        Glyph::NewTable => {
            p.circle(12.0, 12.0, 9.6, GREEN_DEEP);
            p.circle(12.0, 12.0, 8.6, GREEN_BRIGHT);
            p.rect(7.0, 10.8, 10.0, 2.4, WHITE);
            p.rect(10.8, 7.0, 2.4, 10.0, WHITE);
        }

        // AI 新建表：蓝星光 + 小表
        Glyph::AiTable => {
            p.poly(
                &[
                    (10.0, 1.4),
                    (12.6, 8.0),
                    (19.2, 10.6),
                    (12.6, 13.2),
                    (10.0, 19.8),
                    (7.4, 13.2),
                    (0.8, 10.6),
                    (7.4, 8.0),
                ],
                0x4c9fdc,
            );
            p.circle(10.0, 10.6, 2.8, 0xdcefff);
            table(p, 11.4, 12.4, 11.0, 10.0);
        }

        // 删除表：红圆减号
        Glyph::DropTable => minus_badge(p, 12.0, 12.0, 9.4, 0xef8f8f),

        // 导入向导：箭头拐进角落（绿）
        Glyph::ImportWizard => {
            p.curve(
                (3.4, 4.6),
                &[
                    ((3.2, 11.0), (7.8, 11.0)),
                    ((11.4, 11.0), (11.4, 6.2)),
                ],
                2.4,
                STEEL_DEEP,
            );
            p.poly(&[(8.4, 2.0), (14.4, 6.4), (8.4, 10.8)], GREEN);
        }

        // 开始事务：灰色数据库圆柱 + 右下蓝色播放三角（三角先垫一圈白，缩到 15px 也分得开）
        Glyph::Txn => {
            cylinder(p, 9.6, 9.6, 12.4, 13.0, 0xa9b6c2, 0x7d8a97);
            p.poly(&[(12.8, 12.0), (22.0, 16.6), (12.8, 21.2)], WHITE);
            p.poly(&[(13.6, 13.8), (20.6, 16.6), (13.6, 19.4)], 0x1f7fc4);
        }

        // 单元格编辑器：灰色方框轮廓 + 蓝铅笔
        Glyph::CellEdit => {
            p.rrect_a(3.0, 3.0, 18.0, 18.0, 2.8, 0x8e9ba8, 1.0);
            p.rrect_a(4.6, 4.6, 14.8, 14.8, 1.8, WHITE, 1.0);
            p.poly(&[(6.6, 17.8), (15.6, 8.8), (17.8, 11.0), (8.8, 20.0)], 0x2f8fd8);
            p.poly(&[(6.6, 17.8), (8.8, 20.0), (5.2, 21.2)], 0xf0d9a0);
            p.poly(&[(15.6, 8.8), (17.8, 11.0), (19.2, 9.6), (17.0, 7.4)], 0x1c6fb5);
        }

        // 筛选 & 排序：灰漏斗 + 蓝色下箭头
        Glyph::FilterSort => {
            p.poly(&[(2.4, 3.4), (21.6, 3.4), (14.2, 11.4), (14.2, 18.4), (9.8, 20.6), (9.8, 11.4)], 0x8e9ba8);
            p.poly(&[(4.6, 5.6), (19.4, 5.6), (13.4, 12.2), (13.4, 16.8)], 0xc8d2da);
            p.rect(17.2, 9.8, 2.6, 7.6, 0x1f7fc4);
            p.poly(&[(15.4, 16.2), (21.6, 16.2), (18.5, 21.6)], 0x1f7fc4);
        }

        // 列：蓝色田字格
        Glyph::Columns => {
            p.rrect(2.4, 3.4, 19.2, 17.2, 2.0, 0x5aa9e0);
            p.rect(4.0, 5.0, 7.0, 5.6, WHITE);
            p.rect(13.0, 5.0, 7.0, 5.6, WHITE);
            p.rect(4.0, 12.4, 7.0, 6.0, WHITE);
            p.rect(13.0, 12.4, 7.0, 6.0, WHITE);
        }

        // 历史：蓝色回拨时钟
        Glyph::History => {
            p.ring(12.8, 12.6, 8.0, 2.3, 0x1f7fc4);
            p.poly(&[(1.4, 5.6), (8.4, 5.6), (4.9, 12.2)], 0x1f7fc4);
            p.rect(11.9, 7.6, 2.0, 5.8, 0x1f7fc4);
            p.rect(11.9, 11.5, 5.2, 2.0, 0x1f7fc4);
        }

        // 保存：灰蓝软盘（参考工具是 3.5 寸盘造型）
        Glyph::SaveFile => {
            p.rrect(2.6, 2.6, 18.8, 18.8, 2.0, 0x8fa3b8);
            p.rrect(4.4, 4.4, 15.2, 15.2, 1.2, 0xb9c9d8);
            // 上部滑片
            p.rect(7.2, 4.4, 7.6, 4.6, 0xe9f0f6);
            p.rect(8.8, 5.4, 4.4, 2.6, 0x7f93a8);
            // 下部标签
            p.rect(6.6, 12.4, 10.8, 6.4, 0xf4f8fb);
            p.rect(7.8, 13.6, 8.4, 0.9, 0x9db0c2);
            p.rect(7.8, 15.4, 6.6, 0.9, 0x9db0c2);
        }

        // 美化 SQL：灰杆魔杖 + 蓝色星芒
        Glyph::Beautify => {
            p.rot_rect(11.2, 14.6, 3.2, 14.0, 38., 0x93a1ae);
            p.rot_rect(11.2, 14.6, 2.2, 12.6, 38., 0xc2ccd6);
            p.poly(&[(16.6, 2.4), (18.2, 7.0), (22.8, 8.6), (18.2, 10.2), (16.6, 14.8), (15.0, 10.2), (10.4, 8.6), (15.0, 7.0)], 0x3b8fd0);
            p.circle(19.4, 3.6, 1.5, 0x8ecbff);
            p.circle(21.8, 10.4, 1.1, 0x8ecbff);
        }

        // 代码段：蓝色圆环（参考工具就是一个开口圆）
        Glyph::Snippet => {
            p.ring(12.0, 12.0, 8.2, 2.3, 0x2f8fd8);
            p.poly(&[(15.6, 2.6), (21.0, 2.6), (21.0, 8.0), (15.6, 8.0)], 0xffffff);
        }

        // 主键：金色钥匙（参考工具是金钥匙 + 主键序号）
        Glyph::KeyGold => {
            p.ring(7.4, 8.0, 4.6, 2.6, 0xf2b01e);
            p.rot_rect(14.2, 15.4, 3.2, 10.4, 45., 0xf2b01e);
            p.rot_rect(17.0, 18.2, 3.4, 2.4, 45., 0xf2b01e);
            p.rot_rect(15.8, 16.4, 2.6, 2.4, 45., 0xf2b01e);
            p.circle_a(7.4, 8.0, 2.0, WHITE, 0.85);
        }

        // 导出向导：箭头拐出角落（橙）
        Glyph::ExportWizard => {
            p.curve(
                (14.0, 4.6),
                &[
                    ((14.2, 11.0), (9.6, 11.0)),
                    ((6.0, 11.0), (6.0, 6.2)),
                ],
                2.4,
                STEEL_DEEP,
            );
            p.poly(&[(9.0, 2.0), (3.0, 6.4), (9.0, 10.8)], ORANGE);
        }

        // 标题栏 logo：金色同心环
        Glyph::Logo => {
            p.circle(12.0, 12.0, 9.0, 0xe8b23f);
            p.circle(12.0, 12.0, 6.6, WHITE);
            p.circle(12.0, 12.0, 3.4, 0xe8b23f);
            p.circle(12.0, 12.0, 1.6, WHITE);
        }

        // 字段行
        Glyph::Column => {
            p.rrect(3.0, 4.0, 18.0, 4.2, 1.0, 0x94cff9);
            p.rrect(3.0, 10.0, 18.0, 4.2, 1.0, 0xcee8ff);
            p.rrect(3.0, 16.0, 18.0, 4.2, 1.0, 0xcee8ff);
        }
    }
}

// ── 对外入口 ──────────────────────────────────────────────────

/// 画一个彩色图标，返回定宽定高的元素。
pub fn art(glyph: Glyph, size: f32) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _| {
            let mut pen = Pen {
                win: window,
                ox: bounds.origin.x,
                oy: bounds.origin.y,
                k: size / 24.,
            };
            draw(glyph, &mut pen);
        },
    )
    .flex_none()
    .w(px(size))
    .h(px(size))
    .into_any_element()
}

/// 有主题色的图标：彩色走手绘，线稿走 lucide。
pub fn icon(glyph: Glyph, size: f32, tint: Hsla) -> AnyElement {
    match glyph {
        Glyph::Lucide(name) => Icon::new(name)
            .with_size(px(size))
            .text_color(tint)
            .into_any_element(),
        other => art(other, size),
    }
}
