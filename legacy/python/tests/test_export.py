from __future__ import annotations

import json
import zipfile

from app.export.package import Exporter
from app.paths import safe_name
from app.export.cover import calc_title_layout
from app.models import DocumentDraft, Framework, ResearchReport, ResourcePlan, Review


def test_archive_includes_documents_sources_and_hashes(workspace, project):
    _, repo, secrets, flow = workspace
    from test_workflow import DOCUMENT, FRAMEWORK, RESEARCH, RESOURCES, REVIEW
    for stage, content in (("research", RESEARCH), ("framework", FRAMEWORK), ("document", {"markdown": DOCUMENT}), ("resources", RESOURCES), ("review", REVIEW)):
        repo.save_artifact(project["id"], stage, content)
    repo.approve(project["id"], "review")
    repo.update(project["id"], status="approved")
    repo.evidence(project["id"], "模拟检索", {"kind": "web", "results": [{"id": "a1", "title": "模拟来源", "url": "https://example.com"}]})
    result = flow.archive(project["id"], Exporter(repo, secrets).export)
    assert (repo.get(project["id"])["artifacts"]["working_dir"])
    for kind in ("markdown", "docx", "zip"):
        from pathlib import Path
        assert Path(result[kind]).is_file()
    with zipfile.ZipFile(result["zip"]) as archive:
        manifest = json.loads(archive.read(f"{safe_name(project['name'])}/manifest.json"))
        assert manifest["approval"]["approved"]
        assert manifest["files"]


def test_long_cover_title_does_not_drop_lines():
    _, lines = calc_title_layout("星屿采集玩法设计（离线演示）", 9906)
    assert lines
    assert all(lines)
    assert "星" in "".join(lines)
    _, wrapped = calc_title_layout("星屿采集玩法设计（离线演示）", 9906)
    assert len(wrapped) >= 2
    assert all(wrapped)
