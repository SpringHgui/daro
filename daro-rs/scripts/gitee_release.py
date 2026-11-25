#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""在 Gitee 仓创建发行版（Release）并把产物挂成附件。

调用方是 daro-rs/scripts/gitee_publish.py（跑在 Gitee Go 流水线里）。
前置：调用方已确认同 tag 存在于 Gitee 仓（镜像同步滞后时从 GitHub 取过来推）。

接口口径（v5，2026-11-19 实测，与 BangHe 项目同一套）：
  - 请求体一律 application/x-www-form-urlencoded; charset=utf-8。swagger 写的是
    JSON，但 JSON 通道会丢字段、中文乱码，表单编码才稳。
  - 查是否已存在：GET /repos/{owner}/{repo}/releases/tags/{tag}（匿名可读，404 = 不存在）
  - 创建：POST /repos/{owner}/{repo}/releases → 返回对象含 id
  - 传附件：POST /repos/{owner}/{repo}/releases/{release_id}/attach_files（multipart，字段名 file）
  - 列附件：GET /repos/{owner}/{repo}/releases/{release_id}/attach_files
  - 附件直链 302 到 foruda.gitee.com 签名地址，匿名 HEAD 可读

配额红线：单附件 ≤100 MB、每仓附件总量 ≤1 GB。daro 一次发版约 150 MB，
攒到十次左右需要到 Gitee 页面清老发行版的附件——本脚本只传不删。

退出码：0 成功；2 参数/凭据缺失、接口报错或附件回读对不上。
"""

from __future__ import annotations

import argparse
import json
import mimetypes
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

API_BASE = "https://gitee.com/api/v5"
USER_AGENT = "daro-rs-release-bot"


def fail(msg: str) -> None:
    print(f"::error::{msg}", file=sys.stderr, flush=True)
    sys.exit(2)


def send(method: str, url: str, data: bytes | None = None, headers: dict | None = None,
         timeout: int = 180) -> tuple[int, bytes]:
    req = urllib.request.Request(url, data=data, headers={"User-Agent": USER_AGENT,
                                                          **(headers or {})}, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()
    except Exception as e:
        fail(f"请求 {method} {url} 传输层失败：{e}")
        raise


def brief(body: bytes) -> str:
    text = body.decode("utf-8", "replace").strip().replace("\n", " ")
    return text[:200] if text else "(空响应)"


def get_json(path: str, *, optional: bool = False) -> dict | list | None:
    status, body = send("GET", f"{API_BASE}/{path.lstrip('/')}", timeout=60)
    if status == 404 and optional:
        return None
    if status != 200:
        fail(f"GET {path} 回 {status}：{brief(body)}")
    try:
        return json.loads(body)
    except ValueError:
        fail(f"GET {path} 回 200 但不是 JSON：{brief(body)}")


def multipart(fields: dict, filename: str, blob: bytes) -> tuple[bytes, str]:
    boundary = "----daro" + os.urandom(8).hex()
    ctype = mimetypes.guess_type(filename)[0] or "application/octet-stream"
    head = ""
    for key, val in fields.items():
        head += (f'--{boundary}\r\nContent-Disposition: form-data; name="{key}"\r\n\r\n'
                 f"{val}\r\n")
    head += (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; '
             f'filename="{filename}"\r\nContent-Type: {ctype}\r\n\r\n')
    payload = head.encode("utf-8") + blob + f"\r\n--{boundary}--\r\n".encode("utf-8")
    return payload, f"multipart/form-data; boundary={boundary}"


def head_len(url: str) -> tuple[int, int | None]:
    """HEAD 附件直链（跟 302 到签名地址），返回 (状态码, Content-Length)。"""
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT}, method="HEAD")
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            length = resp.headers.get("Content-Length")
            return resp.status, int(length) if length and length.isdigit() else None
    except urllib.error.HTTPError as e:
        return e.code, None
    except Exception as e:
        fail(f"HEAD {url} 传输层失败：{e}")
        raise


def release_id_for_tag(owner: str, repo: str, tag: str, title: str, body_md: str,
                       ref: str, token: str, dry: bool) -> int:
    """已有同 tag 的发行版就复用——重跑流水线不该堆出两个同名发行版。"""
    found = get_json(f"repos/{owner}/{repo}/releases/tags/{urllib.parse.quote(tag)}",
                     optional=True)
    if isinstance(found, dict) and found.get("id"):
        print(f"Gitee 发行版已存在（id={found['id']}），复用：tag {tag}")
        return int(found["id"])
    if dry:
        print(f"[dry-run] 将 POST repos/{owner}/{repo}/releases："
              f"tag_name={tag} name={title} target_commitish={ref} body={len(body_md)} 字符")
        return -1
    status, resp = send(
        "POST", f"{API_BASE}/repos/{owner}/{repo}/releases",
        data=urllib.parse.urlencode({
            "access_token": token, "tag_name": tag, "name": title, "body": body_md,
            "target_commitish": ref, "prerelease": "false",
        }).encode("utf-8"),
        headers={"Content-Type": "application/x-www-form-urlencoded; charset=utf-8"},
    )
    if status not in (200, 201):
        fail(f"创建 Gitee 发行版失败，回 {status}：{brief(resp)}\n"
             f"（tag {tag} 要先存在于 {owner}/{repo}；表单编码这条口径别再换回 JSON）")
    try:
        rid = int(json.loads(resp)["id"])
    except Exception:
        fail(f"创建发行版回 {status} 但解析不到 id：{brief(resp)}")
    print(f"Gitee 发行版已创建：id={rid}")
    return rid


def upload_assets(owner: str, repo: str, rid: int, assets: list[str], tag: str,
                  token: str, dry: bool) -> None:
    base = f"repos/{owner}/{repo}/releases/{rid}/attach_files"
    existing = {} if rid < 0 else {a["name"] for a in get_json(base) or []}
    expected = []
    for path in assets:
        name = os.path.basename(path)
        expected.append(name)
        if not os.path.isfile(path):
            fail(f"读不到要挂进发行版的产物：{path}")
        size = os.path.getsize(path)
        if size > 100 * 1024 * 1024:
            fail(f"{name} 有 {size / 1048576:.0f} MB，超 Gitee 单附件 100 MB 上限")
        if name in existing:
            print(f"附件已存在，跳过：{name}")
            continue
        if dry:
            print(f"[dry-run] 将上传附件 {name}（{size / 1048576:.1f} MB）→ {base}")
            continue
        with open(path, "rb") as f:
            blob = f.read()
        payload, ctype = multipart({"access_token": token, "release_id": str(rid)}, name, blob)
        # 产物单件最大 ~40 MB：Gitee 侧限速时 180s 不够，给足 10 分钟
        status, resp = send("POST", f"{API_BASE}/{base}", data=payload,
                            headers={"Content-Type": ctype}, timeout=600)
        if status not in (200, 201):
            fail(f"上传附件 {name} 失败，回 {status}：{brief(resp)}")
        info = {}
        try:
            info = json.loads(resp)
        except ValueError:
            pass
        print(f"附件已上传：{name}（{size / 1048576:.1f} MB）")
        if info.get("browser_download_url"):
            print(f"  直链：{info['browser_download_url']}")

    if dry:
        return
    # 回读附件清单确认都在：「接口 200 但页面没附件」等于发版没完成
    now = {a["name"]: a for a in get_json(base) or []}
    missing = [n for n in expected if n not in now]
    if missing:
        fail(f"发行版附件回读缺：{', '.join(missing)}（上传接口没报错但附件不在，别当成功）")
    for name in expected:
        # 只发 HEAD：Gitee 附件直链不认 Range，拿 Content-Length 跟接口报的 size 比，
        # 既不白下几十 MB，又能证明字节确实落地。
        url = now[name].get("browser_download_url") or \
            f"https://gitee.com/{owner}/{repo}/releases/download/{tag}/{name}"
        status, length = head_len(url)
        if status != 200:
            fail(f"附件直链回读 {name} 回 {status}（发行版页面上有附件但链接不通）")
        api_size = now[name].get("size")
        if length is not None and api_size is not None and length != api_size:
            fail(f"附件 {name} 直链长度 {length} 与接口报的 {api_size} 不一致，链接指向的不是这份产物")
        print(f"  匿名可下：{url}（HTTP {status}，{length if length is not None else api_size} B）")


def main() -> None:
    ap = argparse.ArgumentParser(description="在 Gitee 建发行版并挂产物附件")
    ap.add_argument("--owner", required=True, help="Gitee 命名空间")
    ap.add_argument("--repo", required=True, help="Gitee 镜像仓名，如 daro")
    ap.add_argument("--tag", required=True, help="git tag，如 daro-rs-v1.0.13（须已推到该仓）")
    ap.add_argument("--version", required=True, help="不带前缀的版本号，如 1.0.13")
    ap.add_argument("--title", required=True, help="发行版标题，与 GitHub Release 同名")
    ap.add_argument("--body-file", required=True, help="发行版正文文件")
    ap.add_argument("--ref", default="main", help="target_commitish，Gitee 镜像仓默认分支")
    ap.add_argument("--assets", nargs="+", required=True, help="要挂成附件的产物路径")
    ap.add_argument("--dry-run", action="store_true", help="只打印计划，不发任何请求")
    args = ap.parse_args()

    if args.tag != f"daro-rs-v{args.version}":
        fail(f"tag {args.tag} 与版本 {args.version} 对不齐，发行版标题和附件会串版")

    token = os.environ.get("GITEE_TOKEN", "").strip()
    if not token and not args.dry_run:
        fail("环境变量 GITEE_TOKEN 没给（Gitee 私人令牌，需 projects 权限）")
    try:
        with open(args.body_file, encoding="utf-8") as f:
            body_md = f.read()
    except OSError as e:
        fail(f"读不到发行版正文 {args.body_file}：{e}")
    if not body_md.strip():
        fail(f"发行版正文 {args.body_file} 是空的")

    print(f"Gitee 目标：{args.owner}/{args.repo} tag {args.tag}")
    rid = release_id_for_tag(args.owner, args.repo, args.tag, args.title, body_md,
                             args.ref, token, args.dry_run)
    upload_assets(args.owner, args.repo, rid, args.assets, args.tag, token, args.dry_run)
    if not args.dry_run:
        print(f"发行版页面：https://gitee.com/{args.owner}/{args.repo}/releases/tag/{args.tag}")


if __name__ == "__main__":
    main()