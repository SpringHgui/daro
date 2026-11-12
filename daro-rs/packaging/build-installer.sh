#!/usr/bin/env bash
# 本地打 Windows 安装包：
#   1) cargo build --release --bin daro-rs --bin daro-mcpd
#   2) Inno Setup 编译 packaging/inno/daro-rs.iss（分步向导安装器）
# 产出：daro-rs/packaging/dist/daro-rs-setup-<Cargo.toml 版本>-x64.exe
#
# 可选参数：
#   --skip-build   复用已有的 target/release 两个 exe，只重编安装器
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

version="$(sed -n 's/^version = "\([^"]*\)".*/\1/p' Cargo.toml | head -1)"
if [[ -z "$version" ]]; then
  echo "无法从 Cargo.toml 读取 version" >&2
  exit 1
fi

if [[ "${1:-}" != "--skip-build" ]]; then
  cargo build --release --bin daro-rs --bin daro-mcpd
fi

for exe in daro-rs daro-mcpd; do
  if [[ ! -f "target/release/$exe.exe" ]]; then
    echo "缺少 target/release/$exe.exe，请先 cargo build --release --bin daro-rs --bin daro-mcpd" >&2
    exit 1
  fi
done

# ISCC 定位：优先环境变量，其次 PATH，最后常见的安装目录
find_iscc() {
  if [[ -n "${ISCC:-}" ]]; then
    [[ -f "$ISCC" ]] && { echo "$ISCC"; return; }
  fi
  if command -v iscc >/dev/null 2>&1; then
    command -v iscc
    return
  fi
  local candidates=(
    "/c/Program Files/Inno Setup 7/ISCC.exe"
    "/c/Program Files (x86)/Inno Setup 7/ISCC.exe"
    "/c/Program Files/Inno Setup 6/ISCC.exe"
    "/c/Program Files (x86)/Inno Setup 6/ISCC.exe"
    "$LOCALAPPDATA/Inno Setup 6/ISCC.exe"
    "/c/Users/$USERNAME/AppData/Local/Programs/Inno Setup 6/ISCC.exe"
    "/c/Users/$USERNAME/AppData/Local/Programs/Inno Setup 7/ISCC.exe"
  )
  local c
  for c in "${candidates[@]}"; do
    [[ -f "$c" ]] && { echo "$c"; return; }
  done
  return 1
}

iscc_path="$(find_iscc || true)"
if [[ -z "${iscc_path:-}" ]]; then
  echo "未找到 Inno Setup 的 ISCC.exe。请安装后重试，或指定路径：" >&2
  echo "  ISCC='/c/Program Files/Inno Setup 7/ISCC.exe' bash packaging/build-installer.sh" >&2
  exit 1
fi

# 注意：Git Bash 会把 /Dxx 改写成本地路径，所以参数要写成双斜杠 //D
"$iscc_path" //Q //DAppVersion="$version" packaging/inno/daro-rs.iss

out="packaging/dist/daro-rs-setup-$version-x64.exe"
echo
echo "安装包：$root/${out#./}"
sha256sum "$out"
