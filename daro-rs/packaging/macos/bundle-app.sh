#!/usr/bin/env bash
# 打 macOS .app：两个可执行文件 + Info.plist + ODBC 运行库。
# 产出：daro-rs/dist/daro-rs-<version>-macos-<arch>.zip
#
# 为什么必须随包带 libodbc：驱动层链接系统 ODBC，链接器记下的是 Homebrew 的绝对路径
# （/opt/homebrew/opt/unixodbc/lib/libodbc.2.dylib）。没装 unixodbc 的机器上 dyld 在
# main 之前就 SIGABRT，界面一句提示都没有，见 issue #13。
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
fw="$app/Contents/Frameworks"
rm -rf "dist/${name}"
mkdir -p "$app/Contents/MacOS" "$fw"
for bin in "${bins[@]}"; do
  cp "target/release/$bin" "$app/Contents/MacOS/$bin"
  chmod +x "$app/Contents/MacOS/$bin"
done
sed "s/__VERSION__/${version}/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

# libodbc 自己还依赖 libtool 的 libltdl，只带 libodbc 仍然起不来
odbc_lib="$(brew --prefix unixodbc)/lib/libodbc.2.dylib"
ltdl_lib="$(brew --prefix libtool)/lib/libltdl.7.dylib"
for lib in "$odbc_lib" "$ltdl_lib"; do
  if [[ ! -f "$lib" ]]; then
    echo "找不到 $lib，请先 brew install unixodbc libtool" >&2
    exit 1
  fi
  cp -L "$lib" "$fw/"
  chmod +w "$fw/$(basename "$lib")"
done

# 取某个 Mach-O 里匹配 $2 的那条依赖路径（otool -L 的第一列）
dep_of() {
  otool -L "$1" | awk -v pat="$2" 'NR>1 && $1 ~ pat {print $1; exit}'
}

ltdl_old="$(dep_of "$fw/libodbc.2.dylib" 'libltdl[.]7[.]dylib')"
install_name_tool -id "@executable_path/../Frameworks/libltdl.7.dylib" "$fw/libltdl.7.dylib"
install_name_tool -id "@executable_path/../Frameworks/libodbc.2.dylib" "$fw/libodbc.2.dylib"
if [[ -n "$ltdl_old" ]]; then
  install_name_tool -change "$ltdl_old" "@executable_path/../Frameworks/libltdl.7.dylib" "$fw/libodbc.2.dylib"
fi
for bin in "${bins[@]}"; do
  old="$(dep_of "$app/Contents/MacOS/$bin" 'libodbc[.]2[.]dylib')"
  if [[ -z "$old" ]]; then
    echo "$bin 没有链接 libodbc，打包脚本的改写规则需要跟着改" >&2
    exit 1
  fi
  install_name_tool -change "$old" "@executable_path/../Frameworks/libodbc.2.dylib" "$app/Contents/MacOS/$bin"
done

# install_name_tool 会让已有签名失效；arm64 上签名无效等于起不来，必须重签。
# 由内向外签，最后签 bundle 把嵌套代码封进去。
codesign --force --sign - "$fw/libltdl.7.dylib" "$fw/libodbc.2.dylib"
for bin in "${bins[@]}"; do
  codesign --force --sign - "$app/Contents/MacOS/$bin"
done
codesign --force --sign - "$app"
codesign --verify --strict "$app"

# 收口检查：包里不许再留 Homebrew 绝对路径，@executable_path 引用必须真的存在。
# 以后哪个依赖又引入了新的 brew dylib，这里会直接把构建拦下来，而不是发出去才闪退。
fail=0
for f in "$app/Contents/MacOS/"* "$fw/"*; do
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