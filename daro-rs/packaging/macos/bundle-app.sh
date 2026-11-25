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
  out="$iconset/$icon_name"
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
# 不用 iconutil：它失败时只报一句 "Failed to generate ICNS"，不说是哪个
# 文件有问题。.icns 只是「icns + 总长 + (类型 + 长度 + 数据)」的容器，
# 直接按规范写入并在缺文件/非 PNG 时点名报错；PNG 负载是 10.7+ 的标准写法。
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
# （issue #21）。弹窗里没有「仍要打开」，但 Apple 仍保留「系统设置 → 隐私与
# 安全性 → 仍要打开」的一次性例外通道，包内说明按这条给用户可执行的步骤。
if [[ -z "$sign_identity" ]]; then
  cat > "dist/${name}/打开说明.txt" <<'TXT'
daro-rs macOS 版 · 打开说明
============================

当前安装包使用 ad-hoc 临时签名，未加入 Apple 开发者计划，也未做公证。
从浏览器或微信下载解压后双击，Gatekeeper 会拦下，弹窗是：

    "daro-rs" Not Opened
    Apple could not verify "daro-rs" is free of malware that may harm your
    Mac or compromise your privacy.

弹窗只有「完成」和「移到废纸篓」，没有「仍要打开」。这不是文件损坏，
是签名问题。任选下面的方法放行：

方法一：系统设置里放行（推荐）
  1. 双击 daro-rs.app，弹出上面的提示时点「完成」（不要点「移到废纸篓」）。
  2. 打开「系统设置 → 隐私与安全性」，向下滚动到「安全性」，会出现
     「已阻止“daro-rs”使用，因为它不是来自已识别的开发者」，
     点右侧「仍要打开」，输入登录密码。
     该按钮只在尝试打开后 1 小时内出现；如果看不到，回访达再双击一次
     触发拦截，再回设置页等几秒。
  3. 再次双击即可启动。系统会记住这次信任，之后直接双击就能打开。

方法二：终端清除隔离属性
  打开「终端」，执行（路径按实际位置调整）：

      xattr -dr com.apple.quarantine /Applications/daro-rs.app

  然后再双击启动。个别新版 macOS 上此方法可能无效，无效时用方法一。

方法三：从源码自行构建
  本地构建的可执行文件不带下载隔离属性，不会被拦。在 daro-rs/ 目录执行：

      cargo build --release --bin daro-rs

  然后运行 target/release/daro-rs（需已安装 Rust 工具链）。

以上步骤彻底免除需要 Developer ID 正式签名 + 公证（需 Apple 开发者账号）。
下载完整性可用发布页的 SHA256SUMS.txt 校验。
TXT
fi

(cd dist && zip -qry "${name}.zip" "${name}")
rm -rf "dist/${name}"
echo
echo "macOS 包：$root/dist/${name}.zip"
shasum -a 256 "dist/${name}.zip"