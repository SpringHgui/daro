#!/usr/bin/env bash
# 打 macOS .app：两个可执行文件 + Info.plist。
# 产出：daro-rs/dist/daro-rs-<version>-macos-<arch>.zip
#
# SQL Server 走纯 Rust 的 TDS 驱动（tiberius），不再链接系统 ODBC，
# 包里不需要额外运行库；TLS（rustls）随二进制静态编译，不依赖 OpenSSL/Homebrew。
#
# 可选参数：
#   --skip-build   复用已有的 target/release 两个可执行文件
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

version="$(sed -n 's/^version = "\([^"]*\)".*/\1/p' Cargo.toml | head -1)"
if [[ -z "$version" ]]; then
  echo "无法从 Cargo.toml 读取 version" >&2
  exit 1
fi

case "$(uname -m)" in
  arm64) arch=arm64 ;;
  x86_64) arch=x64 ;;
  *)
    echo "未支持的 macOS 架构：$(uname -m)" >&2
    exit 1
    ;;
esac

if [[ "${1:-}" != "--skip-build" ]]; then
  cargo build --release --locked --bin daro-rs --bin daro-mcpd
fi

bins=(daro-rs daro-mcpd)
for bin in "${bins[@]}"; do
  if [[ ! -f "target/release/$bin" ]]; then
    echo "缺少 target/release/$bin，请先 cargo build --release --bin daro-rs --bin daro-mcpd" >&2
    exit 1
  fi
done

name="daro-rs-${version}-macos-${arch}"
app="dist/${name}/daro-rs.app"
rm -rf "dist/${name}"
mkdir -p "$app/Contents/MacOS"
for bin in "${bins[@]}"; do
  cp "target/release/$bin" "$app/Contents/MacOS/$bin"
  chmod +x "$app/Contents/MacOS/$bin"
done
sed "s/__VERSION__/${version}/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

# 应用图标：由项目 logo 生成 .icns 放进 Contents/Resources/，Info.plist 的
# CFBundleIconFile 指向它。没有图标时 Finder 显示空白通用图标（issue #21）。
# 必须在 codesign 之前放好：签完再动 bundle 内容会把签名打坏。
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
iconset="$tmpdir/daro.iconset"
mkdir -p "$iconset" "$app/Contents/Resources"
logo="assets/icons/ui/logo.png"

# 先做一张 1024×1024 母版：logo 原图只有 256×256，直接从它放大到各档
# 在部分 macOS 上会产出尺寸不符的 png。统一从母版「缩小」更可靠；
# 母版本身也校验，放大失败就明确报错。
master="$tmpdir/daro-master-1024.png"
if ! sips -z 1024 1024 "$logo" --out "$master" > /dev/null 2>&1; then
  echo "生成 1024 图标母版失败" >&2
  exit 1
fi

while read -r icon_name size; do
  out="$iconset/$icon_name.png"
  if ! sips -z "$size" "$size" "$master" --out "$out" > /dev/null 2>&1; then
    echo "生成图标 $icon_name ($size×$size) 失败" >&2
    exit 1
  fi
  # 名里的尺寸会被 Finder/系统当作实际像素使用，不符会显示错误图标。
  w="$(sips -g pixelWidth "$out" | awk '/pixelWidth/{print $2}')"
  h="$(sips -g pixelHeight "$out" | awk '/pixelHeight/{print $2}')"
  if [[ "$w" != "$size" || "$h" != "$size" ]]; then
    echo "图标 $icon_name 实际 ${w}×${h}，应为 ${size}×${size}" >&2
    exit 1
  fi
done <<'ICONSET'
icon_16x16.png 16
icon_16x16@2x.png 32
icon_32x32.png 32
icon_32x32@2x.png 64
icon_128x128.png 128
icon_128x128@2x.png 256
icon_256x256.png 256
icon_256x256@2x.png 512
icon_512x512.png 512
icon_512x512@2x.png 1024
ICONSET
# 不用 iconutil：macOS 26 runner 上它对同一份尺寸校验通过的合法 iconset
# 反复报 "Failed to generate ICNS"，且匿名拿不到 runner 日志。.icns 本身
# 只是「icns + 总长 + (类型 + 长度 + 数据)」的容器，PNG 负载是 10.7+ 的
# 标准写法，直接写入既避开 runner 工具差异，产物也能在任意平台校验。
python3 - "$iconset" "$app/Contents/Resources/daro.icns" <<'PY'
import struct
import sys
from pathlib import Path

iconset, out = Path(sys.argv[1]), Path(sys.argv[2])
mapping = [
    ("ic11", "icon_16x16@2x.png"),
    ("ic12", "icon_32x32@2x.png"),
    ("ic07", "icon_128x128.png"),
    ("ic13", "icon_128x128@2x.png"),
    ("ic08", "icon_256x256.png"),
    ("ic14", "icon_256x256@2x.png"),
    ("ic09", "icon_512x512.png"),
    ("ic10", "icon_512x512@2x.png"),
]
entries = bytearray()
for typ, name in mapping:
    data = (iconset / name).read_bytes()
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        sys.exit(f"{name} 不是 PNG")
    entries += typ.encode("ascii")
    entries += struct.pack(">I", len(data) + 8)
    entries += data
out.write_bytes(b"icns" + struct.pack(">I", len(entries) + 8) + bytes(entries))
PY

# 有 Developer ID 证书就正式签名 + 公证，Gatekeeper 不再拦首次启动；
# 没有则保持 ad-hoc 签名，并在包内附打开说明（见下方 zip 之前）。
sign_identity="${MACOS_SIGN_IDENTITY:-}"

# 由内向外签：先签嵌套可执行文件，再签 bundle。
# 签 bundle 不会顺手签 Contents/MacOS 里的其它 Mach-O，daro-mcpd 漏签时
# --verify --strict 会报「code object is not signed at all / In subcomponent」；
# 反过来单独给主程序 daro-rs 签名也会失败——它处在 MainExecutable 位置，
# codesign 会按整个 bundle 处理并连带校验还没签的嵌套代码。主程序随 bundle 一起签。
if [[ -n "$sign_identity" ]]; then
  for bin in "${bins[@]}"; do
    [[ "$bin" == "daro-rs" ]] && continue
    codesign --force --sign "$sign_identity" --options runtime --timestamp "$app/Contents/MacOS/$bin"
    codesign --verify --strict "$app/Contents/MacOS/$bin"
  done
  codesign --force --sign "$sign_identity" --options runtime --timestamp "$app"
else
  for bin in "${bins[@]}"; do
    [[ "$bin" == "daro-rs" ]] && continue
    codesign --force --sign - "$app/Contents/MacOS/$bin"
    codesign --verify --strict "$app/Contents/MacOS/$bin"
  done
  codesign --force --sign - "$app"
fi
codesign --verify --strict --verbose=2 "$app"

# 公证：需要 Apple 开发者凭据，齐备时才做；stapler 把票据贴回 bundle，
# 离线也能通过 Gatekeeper。凭据缺失时保持 ad-hoc，不阻断发版。
if [[ -n "$sign_identity" && -n "${APPLE_ID:-}" && -n "${APPLE_APP_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
  notary_zip="$tmpdir/daro-notarize.zip"
  ditto -c -k --keepParent "$app" "$notary_zip"
  xcrun notarytool submit "$notary_zip" \
    --apple-id "$APPLE_ID" --password "$APPLE_APP_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" --wait
  xcrun stapler staple "$app"
  codesign --verify --strict --verbose=2 "$app"
fi

# 收口检查：包里不许再留 Homebrew 绝对路径。哪个依赖又引入了 brew dylib，
# 这里会直接把构建拦下来，而不是发出去才闪退（issue #13）。
fail=0
for f in "$app/Contents/MacOS/"*; do
  while IFS= read -r dep; do
    case "$dep" in
      /opt/homebrew/* | /usr/local/*)
        echo "$f 仍指向 Homebrew 绝对路径：$dep" >&2
        fail=1
        ;;
      @executable_path/*)
        if [[ ! -f "$app/Contents/MacOS/${dep#@executable_path/}" ]]; then
          echo "$f 引用的 $dep 不在包里" >&2
          fail=1
        fi
        ;;
    esac
  done < <(otool -L "$f" | awk 'NR>1 {print $1}')
done
if [[ "$fail" != 0 ]]; then
  echo "macOS 包自检未通过，不产出 zip" >&2
  exit 1
fi

# ad-hoc 签名的包从浏览器下载后会被 macOS 打上隔离属性，被 Gatekeeper 拦下
# （issue #21）。实测 macOS 26 上弹窗没有「仍要打开」，xattr 放行也不保证
# 有效，所以不再给虚假的放行步骤，只如实说明现状与可行路径。
if [[ -z "$sign_identity" ]]; then
  cat > "dist/${name}/打开说明.txt" <<'TXT'
daro-rs macOS 版 · 打开说明
============================

当前安装包使用 ad-hoc 临时签名，未加入 Apple 开发者计划，也未做公证。
在 macOS 26 (arm64) 实测：从浏览器下载解压后双击，Gatekeeper 会直接拦下，
弹窗是：

    "daro-rs" Not Opened
    Apple could not verify "daro-rs" is free of malware that may harm your
    Mac or compromise your privacy.

按钮只有 Move to Trash / Done，没有「仍要打开」，点不进去。这不是文件损坏，
是签名问题。目前可行的只有两条路：

1. 等正式签名版：项目配置 Developer ID 证书并做公证后双击即可打开。
   这是唯一能让官方 zip 直接运行的办法。

2. 从源码自行构建：本地构建的可执行文件不带浏览器的隔离属性，不会被拦。
   在 daro-rs/ 目录执行：

       cargo build --release --bin daro-rs

   然后运行 target/release/daro-rs（需已安装 Rust 工具链）。

xattr -cr daro-rs.app 这类命令行放行在新版 macOS 上不保证有效：CI 实机
测试中清除隔离属性后 open 虽返回成功，应用仍没有启动。若你的机器也是这样，
请改用上面的源码构建方式。

下载完整性可用发布页的 SHA256SUMS.txt 校验。
TXT
fi

(cd dist && zip -qry "${name}.zip" "${name}")
rm -rf "dist/${name}"
echo
echo "macOS 包：$root/dist/${name}.zip"
shasum -a 256 "dist/${name}.zip"