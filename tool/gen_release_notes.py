#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""生成 GitHub Release 正文：各平台下载表格 + 全部提交 + 关联 PR + Full Changelog 链接。

为什么需要它
------------
GitHub 自带的 `generate_release_notes` 只统计「经由 PR 合入的变更」。本仓库多数
提交是直接推到 master 的，于是 Release 正文里除了一行 Full Changelog 链接外
空无一物（见 0.3.1 Draft）。

本脚本改为以 git 提交为唯一准绳：
  1. 正文开头先给一张「平台 × 安装包/绿色版」的下载表格（链接直指本 Release 的
     asset，文件名由版本号拼出，见 DOWNLOAD_PLATFORMS）；
  2. 取上一个 tag → 当前 tag 的全部提交（默认排除 merge commit）；
  3. 按 Conventional Commits 前缀（feat/fix/chore…）归类成分节；
  4. 逐个提交反查 GitHub API，把它归属的 PR 编号 / 标题 / 作者补进来；
  5. 末尾补回 `**Full Changelog**: .../compare/<prev>...<tag>` 链接。

用法
----
    python3 tool/gen_release_notes.py --tag v0.3.1 --output release_body.md
    python3 tool/gen_release_notes.py --tag v0.3.1 --prev v0.3.0     # 显式指定区间
    python3 tool/gen_release_notes.py --tag v0.3.1 --no-pr           # 离线：跳过 PR 反查
    python3 tool/gen_release_notes.py --tag v0.3.1 --no-downloads    # 不要下载表格

参数（除 --tag 外均可省略）
    --tag      当前发布的 tag（默认取 GITHUB_REF_NAME，再退回 `git describe --tags`）
    --version  不带 v 的版本号（= pubspec.yaml 的 version），用来拼安装包文件名；
               默认去掉 --tag 的 v 前缀。CI 从 pubspec 解析后显式传入
    --prev     上一个 tag；省略则用 `git describe --tags --abbrev=0 <tag>^` 自动推断，
               推断不到（首个 release）则用全量历史、且不输出 compare 链接
    --repo     owner/repo，默认取 GITHUB_REPOSITORY，再退回 origin 远程地址解析
    --output   写入的文件路径（UTF-8 / LF）；省略则打印到 stdout
    --token    GitHub token，默认取 GITHUB_TOKEN / GH_TOKEN；没有则走匿名查询（限流更低）
    --no-pr    完全不调用 GitHub API（PR 信息留空）
    --no-downloads  不输出开头的下载表格

退出码
------
0  —— 生成成功。GitHub API 不可用时会打印告警并降级为「仅提交清单」，仍返回 0，
      以免 API 抖动把整个发布流水线卡死。
2  —— 参数或 git 仓库状态有问题（例如 tag 不存在）。此时应视为失败。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
from typing import Dict, List, Optional, Tuple

FIELD = "\x1f"  # git pretty 里的字段分隔
RECORD = "\x1e"  # git pretty 里的记录分隔

API_ROOT = "https://api.github.com"

# Conventional Commits 前缀 → 分节标题（顺序即输出顺序，其余归入「其它变更」）
TYPE_GROUPS: List[Tuple[Tuple[str, ...], str]] = [
    (("feat", "feature"), "✨ 新功能"),
    (("fix", "bugfix", "hotfix"), "🐛 缺陷修复"),
    (("perf",), "⚡ 性能优化"),
    (("refactor",), "♻️ 代码重构"),
    (("docs",), "📝 文档"),
    (("test", "tests"), "✅ 测试"),
    (("build", "deps"), "📦 构建 / 依赖"),
    (("ci",), "👷 CI / 工作流"),
    (("style",), "💄 代码风格"),
    (("revert",), "⏪ 回滚"),
    (("chore",), "🔧 杂项"),
]
OTHER_HEADING = "📌 其它变更"

# ---------- 下载表格 ----------
# 放在正文最前面（紧跟摘要行），让来访者先拿到安装包再往下看变更清单。
# 文件名里的 {ver} 是不带 v 前缀的版本号（取自 pubspec.yaml，不是 tag）。
# ⚠️ 这些文件名必须与 installer/ 下三个打包脚本的实际产物逐字一致：
#      installer/windows/build_installer.bat   → -windows-x64.exe / -windows-x64-portable.zip
#      installer/macos/make_dmg.sh             → -macos-universal.dmg / .zip
#      installer/linux/package_linux.sh        → -linux-amd64.deb / -linux-x64.tar.xz
#    改产物名时这里必须同步，否则 Release 里的链接会 404。
#    release.yml 的「校验正文里的下载链接」步骤会拿实际 artifacts 逐个比对兜底。
DOWNLOAD_COLUMNS = ("平台", "架构", "安装包", "便携 / 免安装")

DOWNLOAD_PLATFORMS = [
    (
        "Windows",
        "x64 (64-bit)",
        ("EXE", "daro-{ver}-windows-x64.exe"),
        ("ZIP", "daro-{ver}-windows-x64-portable.zip"),
    ),
    (
        "macOS",
        "universal (x64 + arm64)",
        ("DMG", "daro-{ver}-macos-universal.dmg"),
        ("ZIP", "daro-{ver}-macos-universal.zip"),
    ),
    (
        "Linux",
        "x64 (amd64)",
        ("DEB", "daro-{ver}-linux-amd64.deb"),
        ("TAR.XZ", "daro-{ver}-linux-x64.tar.xz"),
    ),
]

DOWNLOAD_NOTES = [
    "**Windows**：安装包需管理员权限；绿色版 ZIP 解压即用、不写注册表，适合放 U 盘或免装环境。",
    "**macOS**：产物仅 ad-hoc 签名、未公证，首次打开请右键 →「打开」。",
    "**Linux**：需系统库 `libgtk-3` / `libsqlite3`；`.deb` 面向 Debian / Ubuntu 系，`tar.xz` 解压后直接运行。",
]

# GitHub 为每个 tag 自动生成的源码归档（不进 artifacts、不由我们上传，
# 但永远可用，故单独成表，且链接形如 archive/refs/tags/…、不参与
# release.yml「校验正文里的下载链接」那道 releases/download/ 比对）。
SOURCE_ARCHIVES = [
    ("ZIP", "https://github.com/{repo}/archive/refs/tags/{tag}.zip"),
    ("TAR.GZ", "https://github.com/{repo}/archive/refs/tags/{tag}.tar.gz"),
]

# Windows 的 Microsoft Store 渠道：.msix 由独立 job 产出、不进 GitHub Release
# （artifact 名 msix-store 不匹配 daro-* 通配），这里只做说明，不给会 404 的链接。
MSIX_NOTE = (
    "**Microsoft Store**：Windows 另提供商店版 `.msix`（未签名，入库时由微软重签）。"
    "从 Actions 运行页下载 `msix-store` 产物后提交 Partner Center。"
)

_CONVENTIONAL = re.compile(
    r"^(?P<type>[a-zA-Z]+)"
    r"(?:\((?P<scope>[^)]*)\))?"
    r"(?P<breaking>!)?"
    r":\s*(?P<desc>.*)$"
)


class Commit:
    def __init__(self, sha: str, short: str, author: str, date: str, subject: str) -> None:
        self.sha = sha
        self.short = short
        self.author = author
        self.date = date
        self.subject = subject

        # 解析 Conventional Commits，失败则原文进「其它变更」
        m = _CONVENTIONAL.match(subject)
        if m and m.group("desc").strip():
            self.type = m.group("type").lower()
            self.scope = (m.group("scope") or "").strip()
            self.desc = m.group("desc").strip()
            self.breaking = bool(m.group("breaking"))
        else:
            self.type = ""
            self.scope = ""
            self.desc = subject.strip()
            self.breaking = False

        self.pr = None  # type: Optional[Dict[str, object]]

    @property
    def display(self) -> str:
        """正文中展示的文案：有 scope 的前置加粗，便于一眼看出影响面。"""
        return "**({})** {}".format(self.scope, self.desc) if self.scope else self.desc


def _git(args: List[str], cwd: str, check: bool = True) -> str:
    proc = subprocess.run(
        ["git"] + args,
        cwd=cwd,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if check and proc.returncode != 0:
        raise RuntimeError(
            "git {} 失败（退出码 {}）：{}".format(
                " ".join(args), proc.returncode, (proc.stderr or "").strip()
            )
        )
    return proc.stdout or ""


def repo_root() -> str:
    """定位 git 仓库根。优先当前工作目录（CI 就是仓库根），
    否则退回脚本自身所在位置，这样在别的目录里调用也不会炸。"""
    candidates = [
        os.getcwd(),
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    ]
    for d in candidates:
        try:
            root = _git(["rev-parse", "--show-toplevel"], cwd=d).strip()
        except Exception:  # noqa: BLE001
            continue
        if root:
            return root
    raise RuntimeError("当前目录与脚本所在目录都不是 git 仓库，无法读取提交历史")


def resolve_tag(explicit: Optional[str], cwd: str) -> str:
    if explicit:
        tag = explicit.strip()
    else:
        tag = (os.environ.get("GITHUB_REF_NAME") or "").strip()
        if not tag:
            tag = _git(["describe", "--tags", "--abbrev=0"], cwd=cwd).strip()
    if not tag:
        raise RuntimeError("无法确定当前 tag，请显式传 --tag")

    # 校验 tag 是否真实存在于仓库中
    _git(["rev-parse", "--verify", "refs/tags/" + tag], cwd=cwd)
    return tag


def resolve_version(explicit: Optional[str], tag: str) -> str:
    """下载表格里用的是「不带 v 前缀」的版本号（= pubspec.yaml 的 version）。
    优先用 --version 显式传入（CI 从 pubspec 解析后传进来，最权威），
    否则退化为「去掉 tag 的 v 前缀」——v0.3.1 → 0.3.1。"""
    if explicit:
        return explicit.strip()
    return tag[1:] if tag[:1] in ("v", "V") else tag


def resolve_prev(tag: str, explicit: Optional[str], cwd: str) -> Optional[str]:
    if explicit is not None:
        prev = explicit.strip()
        if prev:
            _git(["rev-parse", "--verify", "refs/tags/" + prev], cwd=cwd)
            return prev
        return None
    out = _git(
        ["describe", "--tags", "--abbrev=0", tag + "^"], cwd=cwd, check=False
    ).strip()
    return out or None


def resolve_repo(explicit: Optional[str], cwd: str) -> str:
    if explicit:
        return explicit.strip()
    env = (os.environ.get("GITHUB_REPOSITORY") or "").strip()
    if env:
        return env
    url = _git(["remote", "get-url", "origin"], cwd=cwd, check=False).strip()
    m = re.search(r"github\.com[:/]+([^/]+)/([^/\s]+?)(?:\.git)?$", url)
    if m:
        return "{}/{}".format(m.group(1), m.group(2))
    return ""


def collect_commits(rev_range: str, cwd: str) -> List[Commit]:
    fmt = FIELD.join(["%H", "%h", "%an", "%aI", "%s"]) + RECORD
    out = _git(["log", "--no-merges", "--pretty=format:" + fmt, rev_range], cwd=cwd)

    commits = []  # type: List[Commit]
    for chunk in out.split(RECORD):
        chunk = chunk.strip("\n")
        if not chunk:
            continue
        parts = chunk.split(FIELD)
        if len(parts) != 5:
            continue
        commits.append(Commit(parts[0], parts[1], parts[2], parts[3], parts[4]))
    return commits


def fetch_pr(commit: Commit, repo: str, token: str, state: Dict[str, bool]) -> None:
    """反查提交归属的 PR。失败时只告警一次并整体停用查询，避免刷屏 / 浪费配额。"""
    if state.get("disabled"):
        return
    url = "{}/repos/{}/commits/{}/pulls".format(API_ROOT, repo, commit.sha)
    headers = {
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "daro-release-notes",
    }
    if token:
        headers["Authorization"] = "Bearer " + token
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except Exception as exc:  # noqa: BLE001 — 网络 / 鉴权问题一律降级
        if not state.get("warned"):
            print(
                "[gen_release_notes] 告警：GitHub API 查询失败（{}），"
                "本次 Release 正文将不含 PR 信息。".format(exc),
                file=sys.stderr,
            )
            state["warned"] = True
        state["disabled"] = True
        return

    if not isinstance(payload, list):
        return
    # 提交可能同时归属于多个 PR（例如被 cherry-pick），只展示编号最小的那个
    prs = [p for p in payload if isinstance(p, dict) and p.get("number")]
    if not prs:
        return
    prs.sort(key=lambda p: p.get("number") or 0)
    first = prs[0]
    user = first.get("user") or {}
    commit.pr = {
        "number": first.get("number"),
        "title": (first.get("title") or "").strip(),
        "url": first.get("html_url") or "",
        "login": (user.get("login") or "").strip(),
    }


def _title_for(ctype: str) -> str:
    for keys, heading in TYPE_GROUPS:
        if ctype in keys:
            return heading
    return OTHER_HEADING


def _human_size(n: int) -> str:
    f = float(n)
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if f < 1024 or unit == "TB":
            if unit == "B":
                return "{} B".format(int(f))
            return "{:.1f} {}".format(f, unit)
        f /= 1024
    return "{:.1f} TB".format(f)


def _artifact_stats(filename: str, artifacts_dir: str) -> Tuple[str, str]:
    """读本地 artifacts 目录，返回 (可读大小, sha256)；文件缺失则 ('—', '—')。"""
    path = os.path.join(artifacts_dir, filename)
    if not os.path.isfile(path):
        return ("—", "—")
    try:
        size = os.path.getsize(path)
        h = hashlib.sha256()
        with open(path, "rb") as fh:
            for chunk in iter(lambda: fh.read(1 << 20), b""):
                h.update(chunk)
        return (_human_size(size), h.hexdigest())
    except OSError:
        return ("—", "—")


def _join2(a: str, b: str) -> str:
    return "<br>".join(x for x in (a, b) if x and x != "—") or "—"


def render_downloads(
    version: str, tag: str, repo: str, artifacts_dir: str = ""
) -> List[str]:
    """渲染「各平台安装包」表格 + 校验和表 + 源码表。

    - repo 为空时退化为纯文件名（不带链接）；
    - artifacts_dir 存在且有文件时，主表加「大小」列，并额外渲染一张完整 SHA256 表；
      本地预览（无 artifacts）则省略这两块，只给链接。
    """

    def cell(label: str, filename: str) -> str:
        name = filename.format(ver=version)
        if not repo:
            return "`{}`".format(name)
        url = "https://github.com/{}/releases/download/{}/{}".format(repo, tag, name)
        if artifacts_dir and os.path.isdir(artifacts_dir):
            size, _sha = _artifact_stats(name, artifacts_dir)
            suffix = "<br><sub>{}</sub>".format(size) if size != "—" else ""
            return "[{}]({}){}".format(label, url, suffix)
        return "[{}]({})".format(label, url)

    has_artifacts = bool(artifacts_dir) and os.path.isdir(artifacts_dir)

    columns = list(DOWNLOAD_COLUMNS)
    if has_artifacts:
        columns = columns + ["大小"]

    lines = ["### 📦 下载", ""]
    lines.append("| " + " | ".join(columns) + " |")
    lines.append("|" + "---|" * len(columns))
    for platform, arch, installer, portable in DOWNLOAD_PLATFORMS:
        row = [platform, arch, cell(*installer), cell(*portable)]
        if has_artifacts:
            s1, _ = _artifact_stats(installer[1].format(ver=version), artifacts_dir)
            s2, _ = _artifact_stats(portable[1].format(ver=version), artifacts_dir)
            row.append(_join2(s1, s2))
        lines.append("| " + " | ".join(row) + " |")
    lines.append("")
    for note in DOWNLOAD_NOTES:
        lines.append("> - " + note)
    lines.append("> - " + MSIX_NOTE)
    lines.append("")

    if has_artifacts:
        lines.append("### 🔐 校验和 (SHA256)")
        lines.append("")
        lines.append("| 文件 | SHA256 |")
        lines.append("|---|---|")
        for _platform, _arch, installer, portable in DOWNLOAD_PLATFORMS:
            for _label, fname in (installer, portable):
                name = fname.format(ver=version)
                _size, sha = _artifact_stats(name, artifacts_dir)
                lines.append("| `{}` | `{}` |".format(name, sha if sha != "—" else ""))
        lines.append("")

    # 源码归档：GitHub 自动生成，永远可用，单独成表（链接走 archive/refs/tags/）。
    if repo:
        lines.append("### 📄 源码")
        lines.append("")
        lines.append("| 归档 | 链接 |")
        lines.append("|---|---|")
        for label, tpl in SOURCE_ARCHIVES:
            url = tpl.format(repo=repo, tag=tag)
            lines.append(
                "| Source code ({}) | [{}]({}) |".format(label.lower(), label, url)
            )
        lines.append("")

    return lines


def render(
    tag: str,
    prev: Optional[str],
    repo: str,
    commits: List[Commit],
    pr_lookup_ran: bool,
    version: Optional[str] = None,
    with_downloads: bool = True,
    artifacts_dir: str = "",
) -> str:
    lines = []  # type: List[str]
    lines.append("<!-- 自动生成：tool/gen_release_notes.py，请勿手工维护 -->")
    lines.append("")

    link = "https://github.com/{}".format(repo) if repo else ""
    prs = {}  # type: Dict[int, Dict[str, object]]
    for c in commits:
        if c.pr:
            prs.setdefault(int(c.pr["number"]), c.pr)

    summary = "本次发布包含 **{}** 个提交".format(len(commits))
    if pr_lookup_ran:
        summary += "、**{}** 个 Pull Request".format(len(prs))
    if prev:
        summary += "（`{}` → `{}`）".format(prev, tag)
    else:
        summary += "（首个发布，区间截至 `{}`）".format(tag)
    lines.append(summary + "。")
    lines.append("")

    # 下载表格紧跟摘要行：访客第一眼就能拿到安装包，不必翻到正文末尾。
    if with_downloads:
        lines.extend(
            render_downloads(
                version or resolve_version(None, tag), tag, repo, artifacts_dir
            )
        )

    breaking = [c for c in commits if c.breaking]

    # 按分节标题归类，保持 TYPE_GROUPS 的既定顺序
    buckets = {}  # type: Dict[str, List[Commit]]
    for c in commits:
        buckets.setdefault(_title_for(c.type), []).append(c)
    order = [h for _, h in TYPE_GROUPS if h in buckets]
    if OTHER_HEADING in buckets:
        order.append(OTHER_HEADING)

    if breaking:
        lines.append("### ⚠️ 破坏性变更")
        lines.append("")
        for c in breaking:
            lines.append("- " + _render_commit(c, link))
        lines.append("")

    for heading in order:
        lines.append("### " + heading)
        lines.append("")
        for c in buckets[heading]:
            lines.append("- " + _render_commit(c, link))
        lines.append("")

    if prs:
        lines.append("### 🔀 合并的 Pull Request")
        lines.append("")
        for number in sorted(prs):
            pr = prs[number]
            title = pr["title"] or "(无标题)"
            suffix = " · @{}".format(pr["login"]) if pr["login"] else ""
            lines.append("- [#{}]({}) {}{}".format(number, pr["url"], title, suffix))
        lines.append("")

    authors = []  # type: List[str]
    for c in commits:
        if c.author and c.author not in authors:
            authors.append(c.author)
    if authors:
        lines.append("贡献者：" + "、".join(authors))
        lines.append("")

    if prev and link:
        lines.append("**Full Changelog**: {}/compare/{}...{}".format(link, prev, tag))
        lines.append("")
    elif link:
        lines.append("**Full Changelog**: {}/commits/{}".format(link, tag))
        lines.append("")

    return "\n".join(lines).rstrip("\n") + "\n"


def _render_commit(c: Commit, link: str) -> str:
    parts = [c.display]
    if link:
        parts.append("[`{}`]({}/commit/{})".format(c.short, link, c.sha))
    else:
        parts.append("`{}`".format(c.short))
    if c.author:
        parts.append(c.author)
    if c.pr:
        parts.append("[#{}]({})".format(c.pr["number"], c.pr["url"]))
    return " · ".join(parts)


def parse_args(argv: List[str]) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="生成 GitHub Release 正文（下载表格 + 提交清单 + 关联 PR + Full Changelog）",
    )
    p.add_argument("--tag", help="当前发布的 tag，默认 GITHUB_REF_NAME 或 git describe")
    p.add_argument(
        "--version",
        help="不带 v 的版本号（= pubspec.yaml 的 version），用于拼安装包文件名；"
        "默认去掉 --tag 的 v 前缀",
    )
    p.add_argument("--prev", help="上一个 tag；默认自动推断，传空串表示按首个发布处理")
    p.add_argument("--repo", help="owner/repo，默认 GITHUB_REPOSITORY 或 origin 地址")
    p.add_argument("--output", help="输出文件路径（UTF-8 / LF）；省略则打到 stdout")
    p.add_argument("--token", help="GitHub token，默认 GITHUB_TOKEN / GH_TOKEN")
    p.add_argument("--no-pr", action="store_true", help="跳过 GitHub API，正文不含 PR 信息")
    p.add_argument(
        "--no-downloads", action="store_true", help="不输出开头的「各平台安装包」下载表格"
    )
    p.add_argument(
        "--artifacts-dir",
        default="artifacts",
        help="产物目录（CI 下载 artifacts 后传入），用于填充「大小」列与 SHA256 表；"
        "目录不存在时这两块自动省略（本地预览常见）",
    )
    return p.parse_args(argv)


def main(argv: List[str]) -> int:
    # Windows 控制台默认用 ANSI/OEM 代码页，这里统一成 UTF-8，避免中文 print 崩掉
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            try:
                stream.reconfigure(encoding="utf-8", errors="replace")
            except Exception:  # noqa: BLE001
                pass

    args = parse_args(argv)

    try:
        cwd = repo_root()
        tag = resolve_tag(args.tag, cwd)
        prev = resolve_prev(tag, args.prev, cwd)
    except Exception as exc:  # noqa: BLE001
        print("[gen_release_notes] 错误：{}".format(exc), file=sys.stderr)
        return 2

    version = resolve_version(args.version, tag)
    repo = resolve_repo(args.repo, cwd)
    rev_range = "{}..{}".format(prev, tag) if prev else tag
    commits = collect_commits(rev_range, cwd)

    token = (args.token or os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN") or "").strip()
    pr_lookup_ran = False
    if args.no_pr:
        print("[gen_release_notes] 已按 --no-pr 跳过 PR 反查。", file=sys.stderr)
    elif not repo:
        print(
            "[gen_release_notes] 告警：无法确定 owner/repo，本次正文不含 PR 信息。",
            file=sys.stderr,
        )
    else:
        pr_lookup_ran = True
        if not token:
            # 公开仓库允许匿名调用，只是配额低（60 次/小时）；失败会自动降级
            print(
                "[gen_release_notes] 提示：未提供 token（--token / GITHUB_TOKEN），"
                "将以匿名方式查询 PR，可能受接口限流影响。",
                file=sys.stderr,
            )
        state = {}  # type: Dict[str, bool]
        for c in commits:
            fetch_pr(c, repo, token, state)

    body = render(
        tag,
        prev,
        repo,
        commits,
        pr_lookup_ran,
        version=version,
        with_downloads=not args.no_downloads,
        artifacts_dir=args.artifacts_dir,
    )

    if args.output:
        with open(args.output, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(body)
        print(
            "[gen_release_notes] 已写入 {}（{} → {}，{} 个提交）。".format(
                args.output, prev or "(首个发布)", tag, len(commits)
            )
        )
    else:
        sys.stdout.write(body)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
