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
# sips / iconutil 都是 macOS 自带工具，CI runner 与本机都可用。
# 必须在 codesign 之前放好：签完再动 bundle 内容会把签名打坏。
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
iconset="$tmpdir/daro.iconset"
mkdir -p "$iconset" "$app/Contents/Resources"
logo="assets/icons/ui/logo.png"
while read -r icon_name size; do
  sips -z "$size" "$size" "$logo" --out "$iconset/$icon_name.png" > /dev/null
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
iconutil -c icns "$iconset" -o "$app/Contents/Resources/daro.icns"

# 有 Developer ID 证书就正式签名 + 公证，Gatekeeper 不再拦首次启动；
# 没有则保持 ad-hoc 签名，并在包内附首次打开说明（见下方 zip 之前）。
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

# ad-hoc 签名的包从浏览器下载后会被 macOS 打上隔离属性，首次启动被
# Gatekeeper 拦下（issue #21「无法打开」）。把放行步骤放进包里。
if [[ -z "$sign_identity" ]]; then
  cat > "dist/${name}/首次打开说明.txt" <<'TXT'
daro-rs macOS 版 · 首次打开说明
================================

本版本使用临时签名，尚未加入 Apple 开发者公证计划。从 GitHub 下载后，
macOS 会因为「来自互联网」而拦截首次启动，这是系统提示，不是应用损坏。

方法一（推荐）：在 Finder 里按住 Control 点击 daro-rs（或右键点击），
选择「打开」，在弹出的提示里再点一次「打开」。之后就可以正常双击启动。

方法二：打开「终端」，cd 到本文件夹后执行：

    xattr -cr daro-rs.app

然后双击 daro-rs 即可。

若提示的不是「无法验证开发者」而是「已损坏 / 无法识别」，请先用发布页的
SHA256SUMS.txt 校验下载包完整性，重新下载后再执行方法二。
TXT
fi

(cd dist && zip -qry "${name}.zip" "${name}")
rm -rf "dist/${name}"
echo
echo "macOS 包：$root/dist/${name}.zip"
shasum -a 256 "dist/${name}.zip"