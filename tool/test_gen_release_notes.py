#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""gen_release_notes.py 的轻量自测：验证「下载表格补全四块」的渲染逻辑。

不依赖 git / GitHub / Flutter，纯本地跑：
    python3 tool/test_gen_release_notes.py

覆盖：
  1. 有 artifacts 时：主表含「大小」列、且出现 6 个 daro-* 链接；
  2. 有 artifacts 时：额外渲染「校验和 (SHA256)」表，6 个文件均带非空哈希；
  3. 有 repo 时：额外渲染「源码」表，含 2 个 archive/refs/tags 归档链接；
  4. MSIX / Microsoft Store 说明出现在下载表格下；
  5. 无 artifacts（目录不存在）时：不出现「大小」列与「校验和」表（本地预览降级）。
"""

import importlib.util
import os
import shutil
import sys
import tempfile

# Windows 下把输出重定向到管道(如 `| tail`)时 stdout 会用 GBK 编码,
# 末行的 ✅ 直接抛 UnicodeEncodeError 让整个脚本以非零码退出。
# 改成"编不出就替换",本地双击与 CI 管道里都能正常跑完。
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(errors="replace")

TOOL_DIR = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location(
    "gen_release_notes", os.path.join(TOOL_DIR, "gen_release_notes.py")
)
mod = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(mod)


def _make_fake_artifacts(d: str, version: str) -> None:
    for _plat, _arch, installer, portable in mod.DOWNLOAD_PLATFORMS:
        for _label, fname in (installer, portable):
            name = fname.format(ver=version)
            # 各文件给不同大小，确保「大小」列能体现差异、SHA256 不全相同
            with open(os.path.join(d, name), "wb") as fh:
                fh.write(b"\x00" * (1024 * (1 + len(name) % 7)))


def _check(cond: bool, msg: str) -> None:
    if not cond:
        print("[FAIL] " + msg)
        sys.exit(1)
    print("[ok]   " + msg)


def test_with_artifacts() -> None:
    tmp = tempfile.mkdtemp(prefix="relnotes-")
    try:
        version, tag, repo = "0.8.0", "v0.8.0", "SpringHgui/daro"
        _make_fake_artifacts(tmp, version)
        lines = mod.render_downloads(version, tag, repo, artifacts_dir=tmp)
        body = "\n".join(lines)

        # 主表表头含「大小」列，且 6 个安装包链接都在
        _check("大小" in lines[2], "有 artifacts 时主表含「大小」列")
        for _plat, _arch, installer, portable in mod.DOWNLOAD_PLATFORMS:
            for _label, fname in (installer, portable):
                name = fname.format(ver=version)
                url = "https://github.com/{}/releases/download/{}/{}".format(
                    repo, tag, name
                )
                _check(url in body, "下载表含链接: " + name)

        # 校验和表：标题 + 6 个文件各带非空 SHA256
        _check("### 🔐 校验和 (SHA256)" in body, "渲染了「校验和 (SHA256)」表")
        for _plat, _arch, installer, portable in mod.DOWNLOAD_PLATFORMS:
            for _label, fname in (installer, portable):
                name = fname.format(ver=version)
                _check(
                    "`" + name + "`" in body,
                    "校验和表列出行: " + name,
                )
        _check(body.count("`sha256:`") == 0, "未误用 sha256: 前缀占位")
        # 真实哈希是 64 位十六进制
        import re

        hashes = re.findall(r"\| `([0-9a-f]{64})` \|", body)
        _check(len(hashes) == 6, "6 个文件均带 64 位 SHA256（实得 %d）" % len(hashes))

        # 源码表：2 个 archive 归档链接
        _check("### 📄 源码" in body, "渲染了「源码」表")
        _check(
            "archive/refs/tags/{}.zip".format(tag) in body, "源码含 ZIP 归档链接"
        )
        _check(
            "archive/refs/tags/{}.tar.gz".format(tag) in body, "源码含 TAR.GZ 归档链接"
        )

        # MSIX / 商店说明
        _check("Microsoft Store" in body, "下载表下含 Microsoft Store / MSIX 说明")

        # 大小列确有可读数值（B/KB/MB...）
        _check(
            any(u in body for u in (" B", " KB", " MB", " GB")),
            "大小列出现可读体积单位",
        )
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def test_without_artifacts() -> None:
    tmp = tempfile.mkdtemp(prefix="relnotes-missing-")
    missing = os.path.join(tmp, "no-such-dir")
    try:
        version, tag, repo = "0.8.0", "v0.8.0", "SpringHgui/daro"
        lines = mod.render_downloads(version, tag, repo, artifacts_dir=missing)
        body = "\n".join(lines)
        _check("大小" not in lines[2], "无 artifacts 时主表不含「大小」列")
        _check("### 🔐 校验和" not in body, "无 artifacts 时不渲染校验和表")
        # 但链接与源码表仍应正常出现
        _check("### 📄 源码" in body, "无 artifacts 时仍渲染源码表")
        _check("Microsoft Store" in body, "无 artifacts 时仍含 MSIX 说明")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def test_source_archives_constant() -> None:
    _check(len(mod.SOURCE_ARCHIVES) == 2, "SOURCE_ARCHIVES 恰有 2 个归档")
    _check(
        all("archive/refs/tags/{tag}" in tpl for _l, tpl in mod.SOURCE_ARCHIVES),
        "SOURCE_ARCHIVES 用 archive/refs/tags 链接（不参与 releases/download 校验）",
    )


if __name__ == "__main__":
    test_source_archives_constant()
    test_with_artifacts()
    test_without_artifacts()
    print("\n全部通过 ✅")
