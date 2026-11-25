#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把最新一次 GitHub Release 原样搬到 Gitee：建发行版 + 挂产物附件。

跑在 Gitee Go 流水线（.workflow/gitee-publish.yml）里；GitHub 发布工作流发完
GitHub Release 后经 go-api 触发它（daro-rs/scripts/trigger_gitee_pipeline.py），
也可以在 Gitee Go 页面手动执行（补发 / 重跑）。

构建只在 GitHub 做（Gitee Go 云端没有 Windows / macOS runner），这里不重新打包：
从 GitHub Release 下回 5 个产物 + SHA256SUMS.txt（有 release-notes.md 也一起下），
按校验和核对字节没坏，再在 Gitee 建同 tag 的发行版并挂附件。

为什么不在 GitHub runner 上调 Gitee v5：从 GitHub 的 IP 调 Gitee 会撞风控
（BangHe 项目实测：不稳定、疑似要登录态）。Gitee Go 构建机在 Gitee 自己的网络里。

流程（幂等，可重跑）：
  1. 定位目标 tag：TARGET_TAG 环境变量优先，否则取 GitHub「latest release」；
  2. 按版本号拼产物名，从 GitHub Release 下载（gh-proxy 镜像优先、直连兜底），
     用 SHA256SUMS.txt 校验每个产物；
  3. 确认 Gitee 镜像仓有该 tag——Gitee 发行版绑死 git tag，镜像同步滞后时
     从 GitHub 原样取过来推过去（提交与 GitHub 一致，不带历史）；
  4. 建/复用发行版并挂附件（gitee_release.py，含回读校验）；
  5. 正文：优先用 Release 里的 release-notes.md 附件（由 GitHub 工作流生成，
     含 macOS「仍要打开」说明）；旧版本没有该附件时退到 GitHub API 的正文。

凭据（Gitee Go 仓库级「通用变量」，界面配置，不进仓库；名字不能以 GITEE_ 开头）：
  DARO_GITEE_TOKEN  必填，私人令牌（projects 权限：推 tag + 调 v5 发行版接口）
  DARO_GITEE_USER   可选，令牌所属账号登录名；缺省用令牌调 /api/v5/user 反查
建完变量还要在流水线里引用才会进构建（.workflow/gitee-publish.yml 的
variables.global 列表；可视化界面是「编辑流水线 → 变量设置 → 引用变量」）。
本地手工跑时用 GITEE_TOKEN / GITEE_USER 也行（同名变量优先取 DARO_ 前缀的）。
其余可覆盖：GITEE_OWNER（缺省 SpringHgui）/ GITEE_SRC_REPO（缺省 daro）/
GH_REPO（缺省 SpringHgui/daro）/ TARGET_TAG。注意 Gitee Go 的系统参数 GITEE_REPO
和 GITEE_BRANCH 不能当仓名/分支用，脚本不读它们。

退出码：0 成功或已是对应版本；2 参数/凭据/下载/校验/接口任一环节不对。
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

UA = "daro-rs-release-bot"
TAG_PREFIX = "daro-rs-v"

GH_REPO = os.environ.get("GH_REPO", "SpringHgui/daro").strip()
OWNER = os.environ.get("GITEE_OWNER", "SpringHgui").strip()
REPO = os.environ.get("GITEE_SRC_REPO", "daro").strip()
GITEE_URL = f"https://gitee.com/{OWNER}/{REPO}.git"

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
RELEASE_SCRIPT = os.path.join(SCRIPT_DIR, "gitee_release.py")


def env_first(*names: str) -> str:
    for name in names:
        val = os.environ.get(name, "").strip()
        if val:
            return val
    return ""


TOKEN = env_first("DARO_GITEE_TOKEN", "GITEE_TOKEN")


def log(msg: str) -> None:
    print(msg, flush=True)


def fail(msg: str) -> None:
    print(f"::error::{msg}", file=sys.stderr, flush=True)
    sys.exit(2)


def http(url: str, *, method: str = "GET", timeout: int = 60, headers: dict | None = None,
         dest: str | None = None) -> tuple[int, bytes]:
    req = urllib.request.Request(url, method=method,
                                 headers={"User-Agent": UA, **(headers or {})})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            if dest:
                with open(dest, "wb") as f:
                    shutil.copyfileobj(resp, f)
                return resp.status, b""
            return resp.status, resp.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()
    except Exception as e:
        return 0, str(e).encode("utf-8", "replace")


def asset_names(version: str) -> list[str]:
    return [
        f"daro-rs-{version}-windows-x64-portable.zip",
        f"daro-rs-setup-{version}-x64.exe",
        f"daro-rs-{version}-macos-arm64.zip",
        f"daro-rs-{version}-macos-x64.zip",
        f"daro-rs-{version}-linux-x64.tar.gz",
        "SHA256SUMS.txt",
    ]


def resolve_tag() -> str:
    tag = os.environ.get("TARGET_TAG", "").strip()
    if tag:
        return tag
    # github.com 的 latest 重定向不带 API 配额；Gitee 构建机连 GitHub 不稳，
    # 直连与 gh-proxy 镜像各试一次，再退到 JSON 接口。
    for base in (f"https://github.com/{GH_REPO}", f"https://gh-proxy.com/https://github.com/{GH_REPO}"):
        url = f"{base}/releases/latest"
        try:
            req = urllib.request.Request(url, method="HEAD", headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=40) as resp:
                final = resp.geturl()
        except urllib.error.HTTPError as e:
            final = e.geturl() if hasattr(e, "geturl") else ""
        except Exception as e:
            log(f"  解析 latest 失败（{url}）：{e}")
            continue
        m = re.search(rf"/releases/tag/({re.escape(TAG_PREFIX)}[0-9][^/?#]*)", final or "")
        if m:
            return m.group(1)
        log(f"  解析 latest 未拿到 tag（{url} → {final}）")
    for url in (f"https://api.github.com/repos/{GH_REPO}/releases/latest",
                f"https://gh-proxy.com/https://api.github.com/repos/{GH_REPO}/releases/latest"):
        status, body = http(url, timeout=40, headers={"Accept": "application/vnd.github+json"})
        if status == 200:
            try:
                tag = json.loads(body).get("tag_name") or ""
            except ValueError:
                tag = ""
            if tag:
                return tag
        log(f"  解析 latest 失败（{url}）：HTTP {status}")
    fail("定位不到最新 GitHub Release 的 tag（GitHub 网络不通？）。"
         "可在 Gitee Go 给这次构建设环境变量 TARGET_TAG=daro-rs-vX.Y.Z 后重跑。")


def download(tag: str, name: str, dest: str, *, optional: bool = False) -> bool:
    for base in (f"https://gh-proxy.com/https://github.com/{GH_REPO}/releases/download/{tag}",
                 f"https://github.com/{GH_REPO}/releases/download/{tag}"):
        url = f"{base}/{name}"
        if os.path.exists(dest):
            os.remove(dest)
        status, _ = http(url, timeout=300, dest=dest)
        if status == 200 and os.path.isfile(dest) and os.path.getsize(dest) > 0:
            log(f"  下载 {name}（{os.path.getsize(dest) / 1048576:.1f} MB）← {url}")
            return True
        log(f"  下载失败（HTTP {status}）：{url}")
    if optional:
        return False
    fail(f"从 GitHub Release {tag} 下载不到 {name}（gh-proxy 与直连都失败）")


def verify_sums(sums_path: str, files: list[str]) -> None:
    want = {}
    with open(sums_path, encoding="utf-8") as f:
        for line in f:
            parts = line.split()
            if len(parts) == 2:
                want[parts[1].lstrip("*")] = parts[0].lower()
    for path in files:
        name = os.path.basename(path)
        if name not in want:
            fail(f"SHA256SUMS.txt 里没有 {name}：产物清单与脚本拼的名字对不上")
        h = hashlib.sha256()
        with open(path, "rb") as f:
            for chunk in iter(lambda: f.read(1 << 20), b""):
                h.update(chunk)
        if h.hexdigest() != want[name]:
            fail(f"{name} 的 sha256 与 SHA256SUMS.txt 不一致：下载到的不是这次构建的产物")
        log(f"  校验通过：{name}")


def fetch_release_body(tag: str, body_path: str) -> None:
    """正文优先级：Release 里的 release-notes.md → GitHub API 的正文 → 最小兜底。"""
    url = f"https://github.com/{GH_REPO}/releases/tag/{tag}"
    for api in (f"https://api.github.com/repos/{GH_REPO}/releases/tags/{tag}",
                f"https://gh-proxy.com/https://api.github.com/repos/{GH_REPO}/releases/tags/{tag}"):
        status, body = http(api, timeout=40, headers={"Accept": "application/vnd.github+json"})
        if status == 200:
            try:
                text = (json.loads(body).get("body") or "").strip()
            except ValueError:
                text = ""
            if text:
                with open(body_path, "w", encoding="utf-8", newline="\n") as f:
                    f.write(text + "\n")
                log("  正文取自 GitHub Release 的 body（没有 release-notes.md 附件）")
                return
        log(f"  取 GitHub 正文失败（{api}）：HTTP {status}")
    log("  拿不到 GitHub 正文，用最小正文兜底（只发生在补发旧版本时）")
    with open(body_path, "w", encoding="utf-8", newline="\n") as f:
        f.write(f"daro-rs {tag[len(TAG_PREFIX):]}\n\n"
                f"完整更新说明见 GitHub Release：{url}\n\n"
                "### macOS 打开说明\n\n"
                "- 当前安装包为 ad-hoc 签名，首次打开会被 Gatekeeper 拦下。双击后点「完成」，"
                "到「系统设置 → 隐私与安全性 → 安全性」点「仍要打开」并输入密码，再双击启动。\n"
                "- 或打开终端执行 `xattr -dr com.apple.quarantine <daro-rs.app 路径>` 后启动；"
                "个别新版 macOS 上可能无效。\n")


def git(*args: str, cwd: str, env: dict | None = None, hint: str = "") -> str:
    proc = subprocess.run(["git", *args], cwd=cwd, env=env, capture_output=True,
                          text=True, encoding="utf-8", errors="replace")
    if proc.returncode != 0:
        fail(f"git {' '.join(args)} 失败：\n{proc.stderr.strip() or proc.stdout.strip()}{hint}")
    return proc.stdout


def push_credentials(work: str) -> tuple[list[str], dict]:
    user = env_first("DARO_GITEE_USER", "GITEE_USER")
    if not user:
        status, body = http(f"https://gitee.com/api/v5/user?access_token={urllib.parse.quote(TOKEN)}",
                            timeout=30)
        if status == 200:
            try:
                user = json.loads(body).get("login") or ""
            except ValueError:
                user = ""
        user = user or "oauth2"
    cred = os.path.join(work, "git-credentials")
    with open(cred, "w", encoding="utf-8") as f:
        f.write(f"https://{urllib.parse.quote(user, safe='')}:"
                f"{urllib.parse.quote(TOKEN, safe='')}@gitee.com\n")
    os.chmod(cred, 0o600)
    # 令牌只落在临时凭据文件里：不进 argv、不进 URL、不进报错信息
    helper = f"credential.helper=store --file={cred.replace(os.sep, '/')}"
    auth = ["-c", "credential.helper=", "-c", helper]
    env = os.environ.copy()
    env["GIT_TERMINAL_PROMPT"] = "0"
    env.pop("GIT_ASKPASS", None)
    return auth, env


def ensure_tag(repo: str, tag: str, auth: list[str], env: dict) -> None:
    """Gitee 发行版绑死 git tag；镜像同步滞后时把 tag 从 GitHub 原样取过来推。

    推的必须是与 GitHub 同一个提交——别推本地伪造 tag，否则之后的镜像同步对不上。
    """
    if git("ls-remote", GITEE_URL, f"refs/tags/{tag}", cwd=repo, env=env).strip():
        log(f"Gitee 镜像已有 tag {tag}")
        return
    log(f"Gitee 镜像还没有 tag {tag}（镜像未同步），从 GitHub 取过来再推")
    git("fetch", "-q", "--no-tags", f"https://github.com/{GH_REPO}.git",
        f"+refs/tags/{tag}:refs/daro-src/{tag}", cwd=repo, env=env,
        hint=(f"\n（Gitee 构建机连不上 GitHub：到 Gitee 的 {OWNER}/{REPO} 仓库页点「同步」"
              f"把 tag 带过来，再重跑这条流水线；或给构建配 TARGET_TAG 后重跑）"))
    git(*auth, "push", "-q", GITEE_URL, f"refs/daro-src/{tag}:refs/tags/{tag}",
        cwd=repo, env=env)
    log(f"已把 tag {tag} 推到 Gitee 镜像")


def default_branch() -> str:
    status, body = http(f"https://gitee.com/api/v5/repos/{OWNER}/{REPO}", timeout=30)
    if status == 200:
        try:
            branch = json.loads(body).get("default_branch") or ""
        except ValueError:
            branch = ""
        if branch:
            return branch
    return "main"


def make_release(tag: str, version: str, body_path: str, ref: str, assets: list[str]) -> None:
    log(f"建/复用 Gitee 发行版 {OWNER}/{REPO} 并挂附件（含回读校验）")
    proc = subprocess.run(
        [sys.executable, RELEASE_SCRIPT,
         "--owner", OWNER, "--repo", REPO, "--tag", tag, "--version", version,
         "--title", f"daro-rs v{version}", "--body-file", body_path,
         "--ref", ref, "--assets", *assets],
        env={**os.environ, "GITEE_TOKEN": TOKEN})
    if proc.returncode != 0:
        fail(f"Gitee 发行版步骤失败（退出码 {proc.returncode}），见上方输出")


def main() -> None:
    if not TOKEN:
        fail("没有 DARO_GITEE_TOKEN：Gitee Go → 仓库「通用变量」建一个私人令牌（projects "
             "权限），再在流水线 variables.global 里引用（可视化界面是「编辑流水线 → 变量设置 "
             "→ 引用变量」）——没被引用的变量不会进构建环境。见 .workflow/gitee-publish.yml")
    if "${" in TOKEN:
        fail("DARO_GITEE_TOKEN 拿到的还是 ${...} 字面量：变量被引用了但值没替换进构建，"
             "到流水线「变量设置 → 引用变量」确认已勾选并保存")

    tag = resolve_tag()
    if not re.fullmatch(rf"{re.escape(TAG_PREFIX)}\d+\.\d+\.\d+", tag):
        fail(f"tag「{tag}」不是 {TAG_PREFIX}X.Y.Z 形态，产物名与发行版标题会对不齐")
    version = tag[len(TAG_PREFIX):]
    log(f"目标：GitHub {GH_REPO} Release {tag} → Gitee {OWNER}/{REPO}")

    work = tempfile.mkdtemp(prefix="daro-gitee-publish-")
    try:
        paths = []
        for name in asset_names(version):
            dest = os.path.join(work, name)
            download(tag, name, dest)
            paths.append(dest)
        notes_path = os.path.join(work, "release-notes.md")
        body_path = os.path.join(work, "release-body.md")
        if download(tag, "release-notes.md", notes_path, optional=True):
            shutil.copyfile(notes_path, body_path)
        else:
            fetch_release_body(tag, body_path)

        verify_sums(os.path.join(work, "SHA256SUMS.txt"), paths[:-1])

        repo = os.path.join(work, "repo")
        os.makedirs(repo)
        git("init", "-q", ".", cwd=repo)
        auth, env = push_credentials(work)
        ensure_tag(repo, tag, auth, env)

        make_release(tag, version, body_path, default_branch(), paths)
        log(f"完成：https://gitee.com/{OWNER}/{REPO}/releases/tag/{tag}")
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()