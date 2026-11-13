from __future__ import annotations

import json

import httpx
import pytest

from app.tools import ResearchTools, ToolError
from app.tools.deepseek import DeepSeekSearch
from app.tools.image_search import DoubaoImageSearch


class Secrets:
    def get(self, key):
        return "test-key"


def web_response(request):
    return httpx.Response(200, json={"results": [{"title": "官方规则", "url": "https://example.com/rule", "snippet": "摘要", "summary": "详细摘要", "content": "正文", "site_name": "Example", "site_icon": None, "date": None, "authority": 3, "rerank_score": 2.0}]})


def test_deepseek_document_contract():
    requests = []
    def capture(request):
        requests.append(request)
        return web_response(request)
    transport = httpx.MockTransport(capture)
    tools = ResearchTools(Secrets(), transport=transport)
    result = DeepSeekSearch(tools).search("游戏规则", count=5)
    assert result[0].title == "官方规则"
    request = requests[0]
    assert str(request.url) == "https://api.deepseek.com/v1/tools/search"
    assert json.loads(request.read()) == {"query": "游戏规则", "count": 5, "need_content": True}
    assert request.headers["Authorization"] == "Bearer test-key"


def test_doubao_image_contract():
    def response(request):
        return httpx.Response(200, json={"Result": {"ImageResults": [{"Title": "游戏 HUD", "Url": "https://example.com/page", "Image": {"Url": "https://example.com/hud.png", "Width": 1920, "Height": 1080}}]}})
    tools = ResearchTools(Secrets(), transport=httpx.MockTransport(response))
    results = DoubaoImageSearch(tools).search("游戏 HUD", 1)
    assert str(results[0].url) == "https://example.com/hud.png"
    assert results[0].source_page == "https://example.com/page"


def test_query_and_budget_validation():
    tools = ResearchTools(Secrets())
    with pytest.raises(ToolError, match="1–128"):
        DeepSeekSearch(tools).search(" ")
    with pytest.raises(ToolError, match="最多"):
        DoubaoImageSearch(tools).search("test", 6)
    tools.budget.calls = 24
    with pytest.raises(ToolError, match="预算"):
        DeepSeekSearch(tools).search("test")


def test_secret_redaction(tmp_path):
    from app.secrets import SecretStore
    secrets = SecretStore(tmp_path)
    secrets.set("TEST_API_KEY", "super-secret-value")
    assert secrets.get("TEST_API_KEY") == "super-secret-value"
    assert secrets.redact("super-secret-value") == "[已隐藏]"
    assert "super-secret-value" not in secrets.file.read_text()
