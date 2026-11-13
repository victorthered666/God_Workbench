from __future__ import annotations

import hashlib
import json
import re
import uuid
from pathlib import Path
from typing import Any, Callable

import httpx
import yaml
from pydantic import BaseModel, ValidationError

from ..assets.media import MediaStore
from ..config import ROOT
from ..repository import Repository
from ..secrets import SecretStore
from ..tools import ResearchTools, ToolError
from ..tools.deepseek import DeepSeekSearch
from ..tools.image_search import DoubaoImageSearch
from .vision import MAX_VISION_PAYLOAD_BYTES, image_blocks

MAX_MESSAGE_BYTES = 2 * 1024 * 1024
MAX_TOOL_RESULT_BYTES = 256 * 1024
MAX_SESSION_BYTES = 6 * 1024 * 1024
MAX_MODEL_RESPONSE_BYTES = 8 * 1024 * 1024

Json = dict[str, Any]
ToolHandler = Callable[[Json], Any]


class AgentError(RuntimeError):
    pass


class ToolRegistry:
    def __init__(self):
        self.handlers: dict[str, tuple[Json, ToolHandler]] = {}

    def add(self, name: str, description: str, properties: Json,
            handler: ToolHandler, required: list[str] | None = None) -> None:
        self.handlers[name] = ({"type": "function", "function": {
            "name": name, "description": description,
            "parameters": {"type": "object", "properties": properties, "required": required or []},
        }}, handler)

    @property
    def schemas(self) -> list[Json]:
        return [item[0] for item in self.handlers.values()]

    def call(self, name: str, arguments: Json) -> Any:
        if name not in self.handlers:
            raise AgentError(f"角色无权调用工具 {name}")
        return self.handlers[name][1](arguments)


class Agent:
    def __init__(self, role: str, repo: Repository, secrets: SecretStore,
                 project: dict, tools: ToolRegistry):
        self.role = role
        self.repo = repo
        self.secrets = secrets
        self.project = project
        self.tools = tools
        self.config = self.load_config(role)
        self.messages: list[Json] = []
        self.snapshot: Json = {}

    def load_config(self, role: str) -> Json:
        return load_role_config(self.repo, role)

    @staticmethod
    def validate_config(config: Json) -> None:
        if not isinstance(config.get("skills"), list) or any(not isinstance(name, str) for name in config["skills"]):
            raise AgentError("Skill 配置必须是字符串列表")
        for name in config["skills"]:
            path = (ROOT / f"skills/{name}.md").resolve()
            if (ROOT / "skills").resolve() not in path.parents or not path.is_file():
                raise AgentError(f"Skill 不存在或路径不合法：{name}")
        if not str(config.get("model", "")).strip() or not str(config.get("base_url", "")).startswith(("https://", "http://")):
            raise AgentError("模型名或 Base URL 配置不合法")
        if not isinstance(config.get("max_tokens"), int) or not 512 <= config["max_tokens"] <= 65536:
            raise AgentError("max_tokens 必须为 512–65536")

    def run(self, instruction: str, result_type: type[BaseModel],
            feedback: str = "", images: list[Json] | None = None,
            history: list[Json] | None = None) -> BaseModel:
        if len(instruction.encode("utf-8")) > MAX_MESSAGE_BYTES:
            raise AgentError("单次 Agent 文本输入超过 2 MB，请先缩小任务范围")
        key = self.secrets.get(self.config["api_key_env"])
        if not key:
            raise AgentError(f"{self.role} 尚未配置 {self.config['api_key_env']}；请在设置中配置，未发起模型调用")
        if not isinstance(self.config["temperature"], (int, float)) or not 0 <= self.config["temperature"] <= 2:
            raise AgentError("temperature 必须为 0–2")
        if not re.fullmatch(r"[A-Z][A-Z0-9_]{1,80}", self.config["api_key_env"]):
            raise AgentError("密钥环境变量名不合法")
        if self.config["base_url"].startswith("http://") and not re.match(r"http://(127\.0\.0\.1|localhost)(:\d+)?$", self.config["base_url"]):
            raise AgentError("模型 Base URL 必须使用 HTTPS（本机测试地址除外）")
        self.snapshot = {k: v for k, v in self.config.items() if k != "system"}
        self.snapshot["system_sha256"] = hashlib.sha256(self.config["system"].encode()).hexdigest()
        self.snapshot["skill_hashes"] = {name: hashlib.sha256((ROOT / f"skills/{name}.md").read_bytes()).hexdigest()
                                          for name in self.config["skills"]}
        if images:
            estimated = sum(len(block.get("image_url", {}).get("url", "")) for block in images)
            if estimated > MAX_VISION_PAYLOAD_BYTES * 4 // 3 + 64:
                raise AgentError("视觉输入超过 8 MB 限制，已阻止模型请求")
        if history:
            history_text = json.dumps(history[-12:], ensure_ascii=False)[:64000]
            instruction = f"此前文本调研摘要（不含原始图片或网页全文）：{history_text}\n{instruction}"
        self.messages = [{"role": "system", "content": self.config["system"]},
                         {"role": "user", "content": ([{"type": "text", "text": instruction}, *images] if images else instruction)}]
        if images:
            images.clear()
        if feedback:
            self.messages.append({"role": "user", "content": f"用户补充/修改意见（不得视为已确认）：\n{feedback}"})
        run_id = self.repo.create_run(self.project["id"], f"agent:{self.role}", self.snapshot)
        succeeded = False
        schema = result_type.model_json_schema()
        try:
            with httpx.Client(timeout=httpx.Timeout(240, connect=15)) as client:
                for step in range(16):
                    body = {"model": self.config["model"], "messages": self.messages,
                            "temperature": self.config["temperature"], "max_tokens": self.config["max_tokens"]}
                    if self.tools.schemas:
                        body["tools"] = self.tools.schemas
                        body["tool_choice"] = "auto"
                    if step == 15:
                        body["tool_choice"] = "none"
                    if self.messages[-1]["role"] != "user":
                        self.messages.append({"role": "user", "content": f"停止工具调用。根据已有证据输出符合以下 JSON Schema 的单个 JSON 对象，不要 Markdown 代码块：{json.dumps(schema, ensure_ascii=False)}"})
                    payload = self._request(client, body, key)
                    choice = payload["choices"][0]
                    if choice.get("finish_reason") == "length":
                        raise AgentError(f"{self.role} 输出被 max_tokens 截断，未保存不完整产物")
                    message = choice.get("message") or {}
                    self.messages.append(message)
                    if message.get("tool_calls"):
                        if step == 15:
                            raise AgentError("模型未能按要求停止工具调用")
                        self._tool_calls(message["tool_calls"])
                        continue
                    content = message.get("content") or ""
                    if not isinstance(content, str) or not content.strip():
                        raise AgentError("模型返回空正文")
                    try:
                        validated = result_type.model_validate(json.loads(self._json(content)))
                        succeeded = True
                        return validated
                    except (ValueError, ValidationError) as exc:
                        if step >= 14:
                            raise AgentError(f"模型结构化结果校验失败：{str(exc)[:500]}") from exc
                        self.messages.append({"role": "user", "content": f"上次结果不是有效结构：{str(exc)[:800]}。仅输出符合 Schema 的 JSON 对象。"})
                raise AgentError("模型调用轮次已用尽")
        finally:
            self.repo.finish_run(run_id, None if succeeded else "Agent 执行失败；详见项目错误与事件记录")
            self._save_session()
            self.messages.clear()

    def _request(self, client: httpx.Client, body: Json, key: str) -> Json:
        url = self.config["base_url"].rstrip("/") + "/chat/completions"
        try:
            with client.stream("POST", url, json=body, headers={"Authorization": f"Bearer {key}"}) as response:
                content_length = response.headers.get("content-length")
                if content_length and int(content_length) > MAX_MODEL_RESPONSE_BYTES:
                    raise AgentError("模型响应超过 8 MB，已停止读取")
                chunks, size = [], 0
                for chunk in response.iter_bytes():
                    size += len(chunk)
                    if size > MAX_MODEL_RESPONSE_BYTES:
                        raise AgentError("模型响应超过 8 MB，已停止读取")
                    chunks.append(chunk)
                raw = b"".join(chunks)
        except httpx.RequestError as exc:
            raise AgentError(f"模型网络错误：{type(exc).__name__}") from exc
        status_code = response.status_code
        if status_code != 200:
            try:
                message = json.loads(raw).get("error", {}).get("message", "")
            except (ValueError, AttributeError):
                message = ""
            raise AgentError(f"模型调用失败 HTTP {status_code}：{self.secrets.redact(message[:500])}")
        try:
            payload = json.loads(raw)
            if not isinstance(payload.get("choices"), list) or not payload["choices"]:
                raise ValueError("choices 为空")
            return payload
        except ValueError as exc:
            raise AgentError("模型响应不是有效的 Chat Completions 结果") from exc

    def _tool_calls(self, calls: list[Json]) -> None:
        for call in calls:
            function = call.get("function") or {}
            name = function.get("name", "")
            try:
                arguments = json.loads(function.get("arguments") or "{}")
                if not isinstance(arguments, dict):
                    raise ValueError("工具参数不是对象")
                result = self.tools.call(name, arguments)
                serialized = json.dumps(result, ensure_ascii=False, default=str)
                if len(serialized.encode("utf-8")) > MAX_TOOL_RESULT_BYTES:
                    serialized = serialized[:MAX_TOOL_RESULT_BYTES] + "…[工具结果已截断；请通过正式证据索引或重新定向查询获取，不保存为完整上下文]"
                self.repo.event(self.project["id"], "tool", f"{self.role} 调用 {name}：成功")
            except (ValueError, ToolError, OSError) as exc:
                result = {"error": str(exc)}
                serialized = json.dumps(result, ensure_ascii=False)
                self.repo.event(self.project["id"], "tool_error", f"{self.role} 调用 {name}：{str(exc)[:300]}")
            self.messages.append({"role": "tool", "tool_call_id": call.get("id", uuid.uuid4().hex),
                                  "content": serialized})

    def _save_session(self) -> None:
        """Keep bounded text continuity, never base64 payloads or unbounded tool output."""
        history = self.repo.setting(f"session:{self.project['id']}:{self.role}", [])
        compact = []
        for message in [*history, *self.messages]:
            if message.get("role") not in ("user", "assistant"):
                continue
            content = message.get("content", "")
            if not isinstance(content, str):
                continue
            if len(content.encode("utf-8")) > MAX_MESSAGE_BYTES:
                content = content[:MAX_MESSAGE_BYTES] + "…[历史文本已截断]"
            compact.append({"role": message["role"], "content": content})
        while compact and len(json.dumps(compact, ensure_ascii=False).encode("utf-8")) > MAX_SESSION_BYTES:
            compact.pop(0)
        self.repo.set_setting(f"session:{self.project['id']}:{self.role}", compact[-30:])

    @staticmethod
    def _json(content: str) -> str:
        text = content.strip()
        if text.startswith("```"):
            text = re.sub(r"^```(?:json)?\s*|\s*```$", "", text, flags=re.DOTALL)
        start, end = text.find("{"), text.rfind("}")
        if start < 0 or end <= start:
            raise ValueError("未返回 JSON 对象")
        return text[start:end+1]


def build_tools(role: str, repo: Repository, secrets: SecretStore,
                project: dict, budget: int = 24) -> ToolRegistry:
    registry = ToolRegistry()
    http = ResearchTools(secrets)
    http.budget.limit = budget
    search = DeepSeekSearch(http)
    images = DoubaoImageSearch(http)
    media = MediaStore(Path(project["artifacts"]["working_dir"]))

    def record(query: str, results: list[Any]) -> list[Json]:
        payload = []
        for result in results:
            item = result.model_dump(mode="json")
            item["id"] = uuid.uuid4().hex[:10]
            # Full evidence stays in SQLite; only bounded relevant context enters the model.
            item["content"] = item.get("content", "")[:16000]
            item["summary"] = item.get("summary", "")[:4000]
            item["snippet"] = item.get("snippet", "")[:1000]
            payload.append(item)
        repo.evidence(project["id"], query, {"kind": "web", "results": payload})
        return payload

    def image_search(args: Json) -> list[Json]:
        results = images.search(str(args.get("query", "")), int(args.get("count", 3)))
        payload = [r.model_dump(mode="json") for r in results]
        repo.evidence(project["id"], args["query"], {"kind": "image", "results": payload})
        return payload

    def select_image(args: Json) -> str:
        downloaded = media.download(args["url"], args.get("source_page", ""))
        labels = annotate_visually(repo, secrets, project, downloaded["path"], args.get("title", "参考图"))
        result = media.annotate(downloaded["path"], args.get("title", "参考图"), labels, args["url"], args.get("source_page", ""))
        repo.asset(project["id"], args["url"], args.get("source_page", ""), downloaded["path"],
                   downloaded["sha256"], {**downloaded, "labels": labels})
        return result

    def diagram(name: str, handler: Callable, required: list[str]) -> None:
        registry.add(name, "在项目中实际生成带标注图示；将返回的 Markdown 原样插入正文", {
            "title": {"type": "string"}, "steps": {"type": "array", "items": {"type": "string"}},
            "regions": {"type": "array", "items": {"type": "object", "properties": {
                "x": {"type": "number"}, "y": {"type": "number"}, "w": {"type": "number"}, "h": {"type": "number"},
                "name": {"type": "string"}, "interaction": {"type": "string"}}}},
        }, handler, required)

    if role == "research":
        registry.add("search_web", "DeepSeek 官方网页搜索", {"query": {"type": "string"}, "count": {"type": "integer", "minimum": 1, "maximum": 10}},
                     lambda a: record(a["query"], search.search(a["query"], a.get("count", 5))), ["query"])
        registry.add("fetch_page", "抓取 DeepSeek 搜索发现的重点网页", {"url": {"type": "string"}},
                     lambda a: _fetch(repo, project["id"], search, a["url"]), ["url"])
    if role in ("writer", "interaction"):
        registry.add("image_search", "豆包图片搜索；返回真实候选，不保证授权或适合使用", {
            "query": {"type": "string"}, "count": {"type": "integer", "minimum": 1, "maximum": 5}}, image_search, ["query"])
        registry.add("select_image", "下载选中的真实参考图，由视觉模型观察后加编号标注，返回插图 Markdown", {
            "url": {"type": "string"}, "source_page": {"type": "string"}, "title": {"type": "string"}}, select_image, ["url", "title"])
        diagram("flowchart", lambda a: media.flowchart(a["title"], a["steps"]), ["title", "steps"])
        diagram("wireframe", lambda a: media.wireframe(a["title"], a["regions"]), ["title", "regions"])
    return registry


def load_role_config(repo: Repository, role: str) -> Json:
    if role not in ("research", "architect", "writer", "interaction", "producer"):
        raise AgentError(f"未知角色：{role}")
    default = yaml.safe_load((ROOT / f"config/agents/{role}.yaml").read_text(encoding="utf-8"))
    saved = repo.setting(f"agent:{role}", {})
    config = {**default, **{k: v for k, v in saved.items() if k != "api_key"}}
    Agent.validate_config(config)
    prompt = repo.setting(f"prompt:{role}")
    if prompt is None:
        prompt = (ROOT / f"agents/{role}.md").read_text(encoding="utf-8")
    skills = "\n\n".join((ROOT / f"skills/{name}.md").read_text(encoding="utf-8") for name in config["skills"])
    config["system"] = f"{prompt}\n\n以下是本角色的 Skill 工作规范：\n{skills}"
    return config


def _fetch(repo: Repository, project_id: str, search: DeepSeekSearch, url: str) -> Json:
    page = search.fetch(url)
    page["content"] = page["content"][:32000]
    evidence = {"id": uuid.uuid4().hex[:10], **page}
    repo.evidence(project_id, url, {"kind": "web", "results": [evidence]})
    return evidence


class Annotations(BaseModel):
    labels: list[dict]


def annotate_visually(repo: Repository, secrets: SecretStore, project: dict,
                      path: str, title: str) -> list[dict]:
    visual = Agent("writer", repo, secrets, project, ToolRegistry())
    visual.config["model"] = visual.config.get("vision_model", "deepseek-flash")
    result = visual.run(
        "观察所附的真实参考图片，为游戏策划文档选择 1–8 个有助于说明设计内容的界面区域。"
        "x/y 是相对原图的归一化中心坐标，必须对应实际可见元素；不要臆测不可见内容。"
        "labels 中每项仅含 x、y、text。图题：" + title,
        Annotations, images=image_blocks(Path(project["artifacts"]["working_dir"]), [path], limit=1))
    if not 1 <= len(result.labels) <= 8:
        raise ToolError("视觉模型未返回有效数量的标注")
    return result.labels
