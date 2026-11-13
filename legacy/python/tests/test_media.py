from __future__ import annotations

import io

import httpx
import pytest
from PIL import Image

from app.assets.media import MediaStore
from app.assets.network import download_image, public_https_url
from app.tools import ToolError


def test_media_diagrams_and_annotations(tmp_path):
    store = MediaStore(tmp_path)
    Image.new("RGB", (800, 500), "#ffffff").save(tmp_path / "assets/images/source.png")
    caption = store.annotate("assets/images/source.png", "标注样例", [{"x": .5, "y": .5, "text": "主按钮"}])
    assert "主按钮" in caption
    assert "![标注样例]" in caption
    flow = store.flowchart("离线模拟流程", ["采集", "制作", "消耗"])
    wire = store.wireframe("离线模拟界面", [{"x": .1, "y": .1, "w": .8, "h": .2, "name": "状态栏", "interaction": "显示资源"}, {"x": .1, "y": .5, "w": .8, "h": .3, "name": "操作区", "interaction": "点击采集"}])
    for text in (caption, flow, wire):
        path = text.split("](")[1].split(")")[0]
        assert store.safe_path(path).is_file()
    with pytest.raises(ToolError, match="越界"):
        store.safe_path("../../secret.png")


def test_download_checks_content(tmp_path):
    data = io.BytesIO()
    Image.new("RGB", (12, 10), "red").save(data, format="PNG")
    response = httpx.Response(200, headers={"Content-Type": "image/png"}, content=data.getvalue())
    client = httpx.Client(transport=httpx.MockTransport(lambda request: response))
    import app.assets.network as network
    network.public_https_url = lambda url: None
    result = download_image("https://images.example.com/test.png", tmp_path / "image", client)
    assert result["width"] == 12
    assert result["sha256"]


def test_reject_non_image_and_private_url(tmp_path, monkeypatch):
    import socket
    with pytest.raises(ToolError, match="内网"):
        public_https_url("https://127.0.0.1/image.png")
    with pytest.raises(ToolError, match="HTTPS"):
        public_https_url("http://example.com/a.png")
