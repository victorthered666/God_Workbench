from __future__ import annotations

import hashlib
import json
import re
import zipfile
from pathlib import Path

from ..paths import safe_name
from ..repository import Repository, now
from ..secrets import SecretStore
from .docx import render_docx


class Exporter:
    def __init__(self, repo: Repository, secrets: SecretStore):
        self.repo = repo
        self.secrets = secrets

    def export(self, project: dict) -> dict:
        root = Path(project["artifacts"]["working_dir"])
        title = safe_name(project["name"])
        markdown = project["artifacts"]["document"]["content"]["markdown"]
        paths = re.findall(r"!\[[^\]]*\]\(([^)]+)\)", markdown)
        for path in paths:
            resolved = (root / path).resolve()
            if root.resolve() not in resolved.parents or not resolved.is_file():
                raise ValueError(f"正文图片缺失或路径越界：{path}")
        md_path = root / f"{title}.md"
        docx_path = root / f"{title}.docx"
        md_path.write_text(markdown, encoding="utf-8")
        render_docx(title, markdown, root, docx_path)
        for stage, artifact in project["artifacts"].items():
            if isinstance(artifact, dict) and "content" in artifact:
                path = root / ("research" if stage == "research" else "reviews" if stage == "review" else "versions") / f"{stage}.json"
                path.write_text(json.dumps(artifact, ensure_ascii=False, indent=2), encoding="utf-8")
        (root / "research/evidence.json").write_text(json.dumps(self.repo.evidence_list(project["id"], limit=100000), ensure_ascii=False, indent=2), encoding="utf-8")
        (root / "reviews/history.json").write_text(json.dumps(self.repo.all_versions(project["id"]), ensure_ascii=False, indent=2), encoding="utf-8")
        manifest = {
            "name": title, "project_id": project["id"], "revision": project["revision"],
            "created_at": project["created_at"], "exported_at": now(),
            "approval": project["artifacts"]["review"]["content"],
            "documents": [md_path.name, docx_path.name],
            "assets": self.repo.assets(project["id"]),
            "notice": "网络图片仅用于研究参考，可检索不代表取得再发布授权。本文档不包含 API Key。",
            "files": [],
        }
        for path in sorted(root.rglob("*")):
            if path.is_file() and path.name not in ("manifest.json",) and path.suffix != ".zip":
                manifest["files"].append({"path": str(path.relative_to(root)).replace("\\", "/"),
                                          "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                                          "bytes": path.stat().st_size})
        (root / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
        zip_path = root / f"{title}-v{project['revision']}.zip"
        with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(root.rglob("*")):
                if path.is_file() and path.suffix != ".zip":
                    archive.write(path, arcname=f"{title}/{path.relative_to(root)}")
        return {"directory": str(root), "markdown": str(md_path), "docx": str(docx_path), "zip": str(zip_path)}
