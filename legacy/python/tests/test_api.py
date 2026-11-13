from __future__ import annotations

from fastapi.testclient import TestClient

from app.factory import create_app
from app.models import ProjectCreate
from app.repository import Repository
from app.secrets import SecretStore


def make_client(tmp_path):
    return TestClient(create_app(Repository(tmp_path / "api.db"), SecretStore(tmp_path / "secrets")))


def test_bootstrap_and_write_guard(tmp_path):
    client = make_client(tmp_path)
    response = client.get("/api/bootstrap")
    assert response.status_code == 200
    data = response.json()
    assert set(data["roles"]) == {"research", "architect", "writer", "interaction", "producer"}
    assert not data["roles"]["research"]["key_configured"]
    assert client.post("/api/projects", json={"name": "测试", "purpose": "目的", "references": "参考"}).status_code == 403
    assert client.get("/static/app.js").status_code == 200


def test_settings_not_expose_secret(tmp_path):
    client = make_client(tmp_path)
    token = client.get("/api/bootstrap").json()["csrf_token"]
    settings = client.get("/api/settings").json()["research"]
    settings.update({"api_key": "not-a-real-secret", "skills": ["game-research", "evidence"]})
    response = client.put("/api/settings/research", json=settings, headers={"X-GDD-Token": token})
    assert response.status_code == 200
    assert "not-a-real-secret" not in response.text
    assert "not-a-real-secret" not in client.get("/api/settings").text


def test_export_requires_approved(tmp_path):
    client = make_client(tmp_path)
    from app.workflow import Workflow
    from app.config import OUTPUT_DIR
    flow = Workflow(Repository(tmp_path / "api.db"), SecretStore(tmp_path / "secrets"))
    import app.workflow
    from unittest.mock import patch
    with patch("app.workflow.OUTPUT_DIR", tmp_path / "outputs"):
        project = flow.create(ProjectCreate(name="测试", purpose="目的", references="参考"))
    assert client.get(f"/api/projects/{project['id']}/export").status_code == 409
