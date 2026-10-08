#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""gen_release_notes.py 的轻量自测：验证下载表格的渲染逻辑。

不依赖 git / GitHub / Flutter，纯本地跑：
    python3 tool/test_gen_release_notes.py

覆盖：
  1. 一个产物一行、共 6 行，每行的链接文字就是源文件名、且指向 releases/download；
  2. 有 artifacts 时：主表追加「大小」「SHA256」两列，6 个文件均带 64 位哈希，
     且**不再**另起一张 /「校验和 (SHA256)」表（校验和已并入主表）；
  3. 有 repo 时：额外渲染「源码」表，含 2 个 archive/refs/tags 归档链接；
  4. MSIX / Microsoft Store 说明出现在下载表格下；
  5. 无 artifacts（目录不存在）时：主表退化为 4 列，无体积、无哈希（本地预览降级）。
"""

import importlib.util
import os
import re
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


def _all_files(version: str):
    """按渲染顺序摊平出 (平台, 架构, 类型, 实际文件名)。"""
    for platform, arch, installer, portable in mod.DOWNLOAD_PLATFORMS:
        for kind, fname in (installer, portable):
            yield platform, arch, kind, fname.format(ver=version)


def _make_fake_artifacts(d: str, version: str) -> None:
    for _plat, _arch, _kind, name in _all_files(version):
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

        # 表头 = 4 个固定列 + 并入的 大小 / SHA256
        columns = list(mod.DOWNLOAD_COLUMNS) + list(mod.DOWNLOAD_STAT_COLUMNS)
        _check(
            lines[2].count("|") == len(columns) + 1,
            "表头恰为 %d 列（大小 / SHA256 已并入主表）" % len(columns),
        )
        _check("SHA256" in lines[2], "表头含 SHA256 列")
        _check("大小" in lines[2], "表头含「大小」列")
        _check("### 🔐 校验和" not in body, "不再另起一张「校验和」表（已并入主表）")
        _check(body.count("<sub>") == 0, "不再用链接下的小字重复一遍体积")

        # 一个产物一行，共 6 行；链接文字必须是源文件名
        rows = lines[4:10]
        _check(
            len(rows) == 6 and all(r.startswith("| ") for r in rows),
            "6 个产物各占一行（实得 %d）" % len(rows),
        )
        for platform, arch, kind, name in _all_files(version):
            url = "https://github.com/{}/releases/download/{}/{}".format(repo, tag, name)
            _check(
                "[{}]({})".format(name, url) in body,
                "链接文字为源文件名: " + name,
            )
            _check(
                "| {} | {} | {} | ".format(platform, arch, kind) in body,
                "行首平台 / 架构 / 类型: {} {} {}".format(platform, arch, kind),
            )

        # 每行末尾的 SHA256 是真实哈希（64 位十六进制），且不是占位符
        _check(body.count("`sha256:`") == 0, "未误用 sha256: 前缀占位")
        hashes = re.findall(r"\| `([0-9a-f]{64})` \|", body)
        _check(len(hashes) == 6, "6 行均带 64 位 SHA256（实得 %d）" % len(hashes))
        _check(
            all(re.search(r"\| `[0-9a-f]{64}` \|$", r) for r in rows),
            "每行的 SHA256 都在行尾（列序：… | 大小 | SHA256 |）",
        )

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

        # 「大小」列确有可读数值（B/KB/MB...）
        _check(
            any(u in body for u in (" B |", " KB |", " MB |", " GB |")),
            "「大小」列出现可读体积单位",
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
        _check(
            lines[2].count("|") == len(mod.DOWNLOAD_COLUMNS) + 1,
            "无 artifacts 时退化为 %d 列" % len(mod.DOWNLOAD_COLUMNS),
        )
        for col in mod.DOWNLOAD_STAT_COLUMNS:
            _check(col not in lines[2], "无 artifacts 时表头不含「%s」列" % col)
        _check(
            not re.search(r"[0-9a-f]{64}", body),
            "无 artifacts 时表内不出现 SHA256",
        )
        # 但链接（文字仍是源文件名）与源码表仍应正常出现
        for _plat, _arch, _kind, name in _all_files(version):
            _check("[" + name + "](" in body, "无 artifacts 时链接仍在: " + name)
            _check(
                "`{}`".format(name) not in body,
                "有 repo 时不再退化成行内代码: " + name,
            )
        _check("### 📄 源码" in body, "无 artifacts 时仍渲染源码表")
        _check("Microsoft Store" in body, "无 artifacts 时仍含 MSIX 说明")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def test_columns_constant() -> None:
    _check(
        mod.DOWNLOAD_COLUMNS == ("平台", "架构", "类型", "文件"),
        "DOWNLOAD_COLUMNS 恰为 4 个固定列（平台 / 架构 / 类型 / 文件）",
    )
    _check(
        mod.DOWNLOAD_STAT_COLUMNS == ("大小", "SHA256"),
        "DOWNLOAD_STAT_COLUMNS 恰为 大小 / SHA256（并入主表，不另起表）",
    )
    _check(
        mod.DOWNLOAD_COLUMNS[-1] == "文件",
        "最后一列是「文件」——链接文字即源文件名，不再用 EXE / ZIP 这类缩写",
    )


def test_source_archives_constant() -> None:
    _check(len(mod.SOURCE_ARCHIVES) == 2, "SOURCE_ARCHIVES 恰有 2 个归档")
    _check(
        all("archive/refs/tags/{tag}" in tpl for _l, tpl in mod.SOURCE_ARCHIVES),
        "SOURCE_ARCHIVES 用 archive/refs/tags 链接（不参与 releases/download 校验）",
    )


if __name__ == "__main__":
    test_columns_constant()
    test_source_archives_constant()
    test_with_artifacts()
    test_without_artifacts()
    print("\n全部通过 ✅")
