"""把「导入来源」工具的官方标识合成到与引擎图标同一套圆角瓦片底上。

用法:
  python tool/compose_tool_icons.py      # 输出 assets/icons/tools/*.svg

与 compose_engine_icons.py 的区别只在素材:引擎标是矢量 SVG,而 Navicat /
Another Redis Desktop Manager 的官方标识只有位图(前者是 Commons 上的
PNG lockup,后者是它仓库里的 512x512 应用图标)。这里把位图按 base64 内嵌进
同一个 24 viewBox 的瓦片模板,渲染端因此与 assets/icons/engines/ 完全同一条路
(SvgPicture.asset),不必再引一条位图资源链。

素材口径(原始文件与出处记在 assets/icons/brands/SOURCES.json):
  * brands/<id>.png         官方原件,**不随应用分发**(未登记进 pubspec)
  * brands/<id>.mark.png    从原件裁出的图形部分并缩到 128px,随瓦片内嵌分发
    - navicat: 官方 lockup 是「金色图形 + Navicat 字标」横版(553x175),
      字标在 44px 以下完全不可辨,故只取图形区 x=21..160 y=19..155;
      软件名由界面上的文字承担。
    - another-redis-desktop-manager: 原件本就是方形应用瓦片(圆角外透明),整幅取用。
  裁剪/缩放一次性用 System.Drawing 完成(等比、双三次插值,未改几何与配色),
  产物入库,所以本脚本的输出可离线复现。
"""
import base64
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(REPO, 'assets', 'icons', 'brands')
DST = os.path.join(REPO, 'assets', 'icons', 'tools')

# 与 compose_engine_icons.py 保持一致的瓦片度量
TILE = (1.2, 1.2, 21.6, 21.6, 3.4)      # x, y, w, h, rx
LIVE = 17.6                              # 官方标最长边允许占据的瓦片内边长
PLATE = ('#ffffff', '#e8edf2')           # 底板渐变:上亮下稍暗
RIM = '#a9b4c0'                          # 描边,亮/暗主题下都能把瓦片与背景分开

TOOLS = ('navicat', 'another-redis-desktop-manager')

# 工具 id -> 安全框(瓦片内允许占据的边长);默认 LIVE
# ARDM 的原件本身就是一块圆角瓦片,且内容四周自带约 4% 留白,
# 再按引擎标的 17.6 缩一次会缩成「瓦片里嵌个小方块」,故让它接近铺满。
BOX = {'another-redis-desktop-manager': 20.6}


def g(v):
    """去掉浮点噪声,保持 SVG 可读。"""
    return ('%.4f' % v).rstrip('0').rstrip('.')


def compose(tid):
    mark = os.path.join(SRC, '%s.mark.png' % tid)
    with open(mark, 'rb') as f:
        b64 = base64.b64encode(f.read()).decode('ascii')

    tx, ty, tw, th, tr = TILE
    live = BOX.get(tid, LIVE)
    x, y, side = tx + (tw - live) / 2.0, ty + (th - live) / 2.0, live

    out = ['<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"'
           ' viewBox="0 0 24 24">']
    out.append('  <!-- %s:厂商官方标识裁取图形部分后合成到统一圆角瓦片底;'
               '出处见 assets/icons/brands/SOURCES.json -->' % tid)
    out.append('  <defs>')
    out.append('    <linearGradient id="plate" x1="0" y1="0" x2="0" y2="1">')
    out.append('      <stop offset="0" stop-color="%s"/>' % PLATE[0])
    out.append('      <stop offset="1" stop-color="%s"/>' % PLATE[1])
    out.append('    </linearGradient>')
    out.append('  </defs>')
    out.append('  <rect x="%s" y="%s" width="%s" height="%s" rx="%s" fill="url(#plate)"/>'
               % (g(tx), g(ty), g(tw), g(th), g(tr)))
    out.append('  <image x="%s" y="%s" width="%s" height="%s" preserveAspectRatio="xMidYMid meet"'
               ' xlink:href="data:image/png;base64,%s"/>' % (g(x), g(y), g(side), g(side), b64))
    out.append('  <rect x="%s" y="%s" width="%s" height="%s" rx="%s" fill="none" stroke="%s"'
               ' stroke-width="0.7" stroke-opacity="0.85"/>'
               % (g(tx + 0.35), g(ty + 0.35), g(tw - 0.7), g(th - 0.7), g(tr - 0.35), RIM))
    out.append('</svg>')

    os.makedirs(DST, exist_ok=True)
    path = os.path.join(DST, '%s.svg' % tid)
    with open(path, 'w', encoding='utf-8', newline='\n') as f:
        f.write('\n'.join(out) + '\n')
    return os.path.getsize(path)


def main():
    only = [a for a in sys.argv[1:] if not a.startswith('-')]
    for tid in TOOLS:
        if only and tid not in only:
            continue
        print('%-30s %6d B' % (tid, compose(tid)))
    return 0


if __name__ == '__main__':
    sys.exit(main())
