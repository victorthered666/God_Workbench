from __future__ import annotations

import json
import threading
import time
from typing import Any

import httpx

from ..secrets import SecretStore


class ToolError(RuntimeError):
    pass


class Budget:
    def __init__(self, limit: int = 24):
        self.limit = limit
        self.calls = 0

    def use(self) -> None:
        self.calls += 1
        if self.calls > self.limit:
            raise ToolError(f"已达到本阶段 {self.limit} 次检索预算，请缩小范围或经用户调整后重试")


MAX_SEARCH_RESPONSE_BYTES = 8 * 1024 * 1024


class ResearchTools:
    image_semaphore = threading.BoundedSemaphore(5)

    def __init__(self, secrets: SecretStore, budget: Budget | None = None,
                 transport: httpx.BaseTransport | None = None):
        self.secrets = secrets
        self.budget = budget or Budget()
        self.transport = transport

    def key(self, name: str) -> str:
        key = self.secrets.get(name)
        if not key:
            raise ToolError(f"尚未配置 {name}。请在“设置”中配置后重试；尚未产生检索调用")
        return key

    def post(self, url: str, body: dict[str, Any], key_name: str = "DEEPSEEK_API_KEY",
             extra_headers: dict[str, str] | None = None) -> dict[str, Any]:
        self.budget.use()
        headers = {"Authorization": f"Bearer {self.key(key_name)}", **(extra_headers or {})}
        for attempt in range(3):
            try:
                with httpx.Client(timeout=httpx.Timeout(45, connect=10), follow_redirects=False, transport=self.transport) as client:
                    with client.stream("POST", url, json=body, headers=headers) as response:
                        length = response.headers.get("content-length")
                        if length and int(length) > MAX_SEARCH_RESPONSE_BYTES:
                            raise ToolError("检索响应超过 8 MB，已停止读取")
                        chunks, size = [], 0
                        for chunk in response.iter_bytes():
                            size += len(chunk)
                            if size > MAX_SEARCH_RESPONSE_BYTES:
                                raise ToolError("检索响应超过 8 MB，已停止读取")
                            chunks.append(chunk)
                        response_data = b"".join(chunks)
                        status_code = response.status_code
            except httpx.RequestError as exc:
                if attempt == 2:
                    raise ToolError(f"网络请求失败：{type(exc).__name__}") from exc
                time.sleep(2 ** attempt)
                continue
            if status_code == 200:
                try:
                    return json.loads(response_data)
                except ValueError as exc:
                    raise ToolError("搜索服务返回了非 JSON 内容") from exc
            if status_code in (408, 429) or status_code >= 500:
                if attempt < 2:
                    time.sleep(2 ** attempt)
                    continue
            try:
                message = json.loads(response_data).get("error", {}).get("message", "")
            except (ValueError, AttributeError):
                message = ""
            hints = {400: "请求参数不合法", 401: "API Key 无效", 402: "服务余额不足",
                     403: "目标页面被安全策略拒绝", 404: "没有可用网页内容",
                     413: "请求体过大", 429: "请求过于频繁"}
            raise ToolError(f"检索失败 HTTP {status_code}：{hints.get(status_code, '服务错误')} {message[:240]}")
        raise ToolError("检索服务暂不可用")
