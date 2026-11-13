from __future__ import annotations

from ..models import SearchResult
from . import ResearchTools, ToolError


class DeepSeekSearch:
    """Official DeepSeek Search/Fetch endpoints; these do not provide image search."""

    def __init__(self, tools: ResearchTools):
        self.tools = tools

    def search(self, query: str, count: int = 5, need_content: bool = True) -> list[SearchResult]:
        query = query.strip()
        if not 1 <= len(query) <= 128:
            raise ToolError("DeepSeek 检索词必须为 1–128 个字符")
        if not 1 <= count <= 50:
            raise ToolError("DeepSeek 检索条数必须为 1–50")
        data = self.tools.post("https://api.deepseek.com/v1/tools/search", {
            "query": query, "count": count, "need_content": need_content,
        })
        if not isinstance(data.get("results"), list):
            raise ToolError("DeepSeek 搜索返回缺少 results 数组")
        return [SearchResult.model_validate(item) for item in data["results"]]

    def fetch(self, url: str) -> dict[str, str]:
        if not 1 <= len(url) <= 8192 or not url.startswith(("http://", "https://")) or "@" in url:
            raise ToolError("待抓取网址不符合要求")
        data = self.tools.post("https://api.deepseek.com/v1/tools/fetch", {"url": url})
        if not isinstance(data.get("title"), str) or not isinstance(data.get("content"), str):
            raise ToolError("DeepSeek 抓取返回格式不符合文档")
        return {"title": data["title"], "content": data["content"], "url": url}
