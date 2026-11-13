from __future__ import annotations

from typing import Any

from ..models import ImageCandidate
from . import ResearchTools, ToolError


class DoubaoImageSearch:
    """Volcengine SearchInfinity API Key transport and documented image result shape."""

    def __init__(self, tools: ResearchTools):
        self.tools = tools

    def search(self, query: str, count: int = 3) -> list[ImageCandidate]:
        query = query.strip()
        if not 1 <= len(query) <= 100:
            raise ToolError("图片检索词必须为 1–100 个字符")
        if not 1 <= count <= 5:
            raise ToolError("豆包图片检索每次最多返回 5 条")
        with self.tools.image_semaphore:
            data = self.tools.post("https://open.feedcoopapi.com/search_api/web_search", {
                "Query": query, "SearchType": "image", "Count": count,
            }, key_name="WEB_SEARCH_API_KEY", extra_headers={"X-Traffic-Tag": "skill_web_search_common"})
        if (data.get("ResponseMetadata") or {}).get("Error"):
            error = data["ResponseMetadata"]["Error"]
            raise ToolError(f"豆包搜索 {error.get('Code', '')}: {error.get('Message', '')}")
        result = data.get("Result") or {}
        items = result.get("ImageResults") or []
        if not isinstance(items, list):
            raise ToolError("豆包图片搜索返回格式不符合 ImageResults 约定")
        candidates = []
        for item in items:
            image: dict[str, Any] = item.get("Image") or {}
            if not image.get("Url"):
                continue
            try:
                candidates.append(ImageCandidate(
                    url=image["Url"], title=item.get("Title") or "",
                    width=image.get("Width"), height=image.get("Height"),
                    source_page=item.get("Url") or item.get("SourceUrl") or item.get("PageUrl") or "",
                ))
            except ValueError:
                continue
        return candidates
