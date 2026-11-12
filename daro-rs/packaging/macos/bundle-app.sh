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

# 由内向外签，最后签 bundle 把嵌套代码封进去。
# x64 上只签 bundle 不会把 Contents/MacOS 里的 daro-mcpd 一起封进去，
# --verify --strict 会报嵌套代码未签名（arm64 靠链接器的默认 ad-hoc 签名躲过），
# 所以 bundle 用 --deep 重签；逐个签名保留，失败时能落到具体文件。
for bin in "${bins[@]}"; do
  codesign --force --sign - "$app/Contents/MacOS/$bin"
  codesign --verify --strict "$app/Contents/MacOS/$bin"
done
codesign --force --deep --sign - "$app"
codesign --verify --strict --verbose=2 "$app"

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

(cd dist && zip -qry "${name}.zip" "${name}")
rm -rf "dist/${name}"
echo
echo "macOS 包：$root/dist/${name}.zip"
shasum -a 256 "dist/${name}.zip"