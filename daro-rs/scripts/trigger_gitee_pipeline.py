#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""发完 GitHub Release 后，触发 Gitee Go 上的发布流水线并盯到出结果。

发布动作（建 Gitee 发行版、挂 GitHub 产物）全部在 Gitee 服务器上执行，见
.workflow/gitee-publish.yml 与 daro-rs/scripts/gitee_publish.py。这里只做
「按一下按钮」，用的是 Gitee Go 的仓库流水线接口（与官方 gitee-cli 的
`gitee pipeline run` 同一套口径）：

  POST https://go-api.gitee.com/{owner}/{repo}/gitee-go/ipipe/rest/v5/pipelines/builds
       Authorization: Bearer <私人令牌>，body {"fileName": "...", "ref": "..."}
  GET  .../rest/v5/pipelines/builds/{id}/status     → data.status

为什么不靠镜像同步 tag 触发：Gitee 镜像仓的同步是手动/滞后的，指望 tag 推过去
自动跑流水线等于把发版挂在一次手动同步上。发完 Release 直接点名触发。

环境变量（GitHub Secrets）：
  GITEE_TOKEN           必填，与 Gitee 通用变量 DARO_GITEE_TOKEN 是同一个私人令牌
  GITEE_OWNER           必填
  GITEE_SRC_REPO        缺省 daro（Gitee 上的源码镜像仓，.workflow/ 所在仓）
  GITEE_SRC_REF         缺省 main
  GITEE_PIPELINE_FILE   缺省 gitee-publish.yml（不带 .workflow/ 前缀）
  GITEE_POLL_MINUTES    缺省 30，等到终态的上限

退出码：0 流水线成功（或等待超时但仍在跑，打 warning）；2 触发失败或流水线失败。
"""

from __future__ import annotations

import json
import os
import sys
import time
import urllib.error
import urllib.request

UA = "daro-rs-release-bot"
TERMINAL = {"SUCCESS", "FAILED", "CANCELLED", "SKIPPED", "TIMEOUT"}


def fail(msg: str) -> None:
    print(f"::error::{msg}", file=sys.stderr, flush=True)
    sys.exit(2)


def call(method: str, url: str, *, body: dict | None = None, token: str,
         timeout: int = 60) -> dict:
    data = json.dumps(body).encode("utf-8") if body is not None else None
    headers = {
        "User-Agent": UA,
        "Accept": "application/json",
        "Authorization": f"Bearer {token}",
    }
    if data is not None:
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read()
    except urllib.error.HTTPError as e:
        detail = e.read().decode("utf-8", "replace")[:300]
        if e.code in (401, 403):
            fail(f"{method} {url} 回 {e.code}：令牌无权限或已过期（{detail}）")
        return {"code": e.code, "msg": f"HTTP {e.code} {detail}"}
    except Exception as e:
        return {"code": -1, "msg": f"传输层失败：{e}"}
    try:
        return json.loads(raw)
    except ValueError:
        return {"code": -1, "msg": f"响应不是 JSON：{raw[:200]!r}"}


def main() -> None:
    token = os.environ.get("GITEE_TOKEN", "").strip()
    owner = os.environ.get("GITEE_OWNER", "").strip()
    repo = os.environ.get("GITEE_SRC_REPO", "daro").strip()
    ref = os.environ.get("GITEE_SRC_REF", "main").strip()
    pipe = os.environ.get("GITEE_PIPELINE_FILE", "gitee-publish.yml").strip()
    minutes = int(os.environ.get("GITEE_POLL_MINUTES", "30"))
    if not token or not owner:
        fail("缺 GITEE_TOKEN / GITEE_OWNER")

    api = f"https://go-api.gitee.com/{owner}/{repo}/gitee-go/ipipe"
    console = f"https://gitee.com/{owner}/{repo}/gitee_go"
    print(f"Gitee Go 流水线：{owner}/{repo} {ref} → {pipe}")

    svc = call("GET", f"{api}/rest/v1/billing/gitee-go-service/status", token=token)
    status_val = (svc.get("data") or {}).get("giteeGoStatus")
    if svc.get("code") != 0 or status_val != "active":
        fail(f"Gitee Go 服务未开通或不可用（code={svc.get('code')} msg={svc.get('msg')} "
             f"status={status_val}）：到 {console} 开通后再发版")

    resp = call("POST", f"{api}/rest/v5/pipelines/builds",
                body={"fileName": pipe, "ref": ref}, token=token)
    build = resp.get("data") or {}
    if resp.get("code") != 0 or not build.get("id"):
        hint = ""
        if "不存在" in str(resp.get("msg")) or "not" in str(resp.get("msg")).lower():
            hint = (f"\n多半是 Gitee 镜像仓 {ref} 上还没有 .workflow/{pipe}："
                    f"先在 Gitee 仓库页点「同步」把源码（含 .workflow/）拉过去，再重跑这一步。")
        fail(f"触发流水线失败（code={resp.get('code')} msg={resp.get('msg')}）{hint}")

    bid = int(build["id"])
    number = build.get("buildNumber")
    print(f"已触发构建 #{number}（id={bid}），在 {console} → 构建历史 看日志")

    deadline = time.time() + minutes * 60
    last = None
    while True:
        st = call("GET", f"{api}/rest/v5/pipelines/builds/{bid}/status", token=token)
        cur = (st.get("data") or {}).get("status")
        if st.get("code") != 0:
            print(f"  查询状态失败（{st.get('msg')}），15s 后重试")
        elif cur != last:
            print(f"  构建 #{number} 状态：{cur}")
            last = cur
        if cur in TERMINAL:
            if cur == "SUCCESS":
                print(f"Gitee 发行版已同步：https://gitee.com/{owner}/{repo}/releases")
                return
            fail(f"Gitee 发布流水线 #{number} 结束于 {cur}，"
                 f"到构建历史看日志：{console}")
        if time.time() > deadline:
            print(f"::warning::等了 {minutes} 分钟仍未结束（当前 {cur}），流水线可能还在排队/"
                  f"运行；结果到 {console} 的构建历史查看，GitHub 这边不再阻塞发布。")
            return
        time.sleep(15)


if __name__ == "__main__":
    main()