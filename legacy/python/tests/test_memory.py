from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest
from PIL import Image

from app.agents.runtime import MAX_SESSION_BYTES, Agent
from app.agents.vision import MAX_VISION_PAYLOAD_BYTES, image_blocks
from app.export.cover import calc_title_layout, split_title


def test_title_breaking_is_finite_for_degenerate_inputs():
    for title in (" ", "a", "ab", "星屿采集玩法设计（离线演示）", "　" * 10, "a" * 120):
        for width in (0, 1, 10, 100, 9906):
            if not title.strip() or width <= 0:
                with pytest.raises(ValueError):
                    calc_title_layout(title, width)
                continue
            _, lines = calc_title_layout(title, width)
            assert lines
            assert all(lines)
            assert len(lines) <= len(title)
    with pytest.raises(ValueError):
        split_title("", 0)


def test_vision_payload_is_bounded(tmp_path):
    path = tmp_path / "large.png"
    Image.new("RGB", (2400, 1800), "#738e86").save(path)
    blocks = image_blocks(tmp_path, ["large.png"] * 20, limit=20)
    assert len(blocks) <= 6
    payload = json.dumps(blocks)
    assert len(payload.encode()) <= MAX_VISION_PAYLOAD_BYTES * 4 // 3 + 100
    assert len(blocks[0]["image_url"]["url"]) < 2_800_000


def test_session_never_persists_image_payload(tmp_path):
    from app.repository import Repository
    from app.secrets import SecretStore
    repo = Repository(tmp_path / "memory.db")
    secrets = SecretStore(tmp_path / "keys")
    agent = Agent("research", repo, secrets, {"id": "project", "artifacts": {}}, __import__("app.agents.runtime", fromlist=["ToolRegistry"]).ToolRegistry())
    agent.messages = [{"role": "user", "content": [{"type": "text", "text": "image"}, {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64," + "x" * 10_000_000}}]}]
    agent._save_session()
    stored = repo.setting("session:project:research")
    assert "base64" not in json.dumps(stored)
    agent.messages = [{"role": "user", "content": "x" * 1_000_000} for _ in range(20)]
    agent._save_session()
    assert len(json.dumps(repo.setting("session:project:research")).encode()) <= MAX_SESSION_BYTES


def test_title_breaking_under_memory_limit():
    code = """
import sys
sys.path.insert(0, r'D:\\GDC_Gen')
from app.export.cover import calc_title_layout
for width in (1, 10, 100, 9906):
    for title in ('a'*120, '星屿采集玩法设计（离线演示）', ' '*120):
        try:
            calc_title_layout(title, width)
        except ValueError:
            pass
print('finite')
"""
    result = subprocess.run([sys.executable, "-I", "-c", code], capture_output=True, text=True, timeout=10)
    assert result.returncode == 0, result.stderr
    assert "finite" in result.stdout
