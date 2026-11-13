from __future__ import annotations

import os
from pathlib import Path

import pytest

os.environ["GDD_DATA_DIR"] = ""

from app.models import ProjectCreate
from app.repository import Repository
from app.secrets import SecretStore
from app.workflow import Workflow


@pytest.fixture(autouse=True)
def isolate_api_environment(monkeypatch):
    monkeypatch.delenv("DEEPSEEK_API_KEY", raising=False)
    monkeypatch.delenv("WEB_SEARCH_API_KEY", raising=False)


@pytest.fixture
def workspace(tmp_path, monkeypatch):
    monkeypatch.setattr("app.workflow.OUTPUT_DIR", tmp_path / "outputs")
    repo = Repository(tmp_path / "test.db")
    secrets = SecretStore(tmp_path / "secrets")
    flow = Workflow(repo, secrets)
    return tmp_path, repo, secrets, flow


@pytest.fixture
def project(workspace):
    _, _, _, flow = workspace
    return flow.create(ProjectCreate(name="测试：海岛/设计", purpose="验证轻量生存循环", references="示例参考对象", platform="PC", audience="测试用户"))


class FakeResponse:
    def __init__(self, payload):
        self._payload = payload

    def model_dump(self):
        return self._payload


class FakeAgent:
    def __init__(self, results):
        self.results = results
        self.calls = []

    def run(self, instruction, result_type, feedback="", images=None):
        self.calls.append({"instruction": instruction, "feedback": feedback, "result_type": result_type})
        value = self.results.get(self.calls[-1]["result_type"])
        if isinstance(value, Exception):
            raise value
        return FakeResponse(value)
