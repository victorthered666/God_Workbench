from __future__ import annotations

import secrets as token_secrets
from pathlib import Path

import markdown as markdown_lib
from bs4 import BeautifulSoup
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import FileResponse, HTMLResponse, JSONResponse
from fastapi.staticfiles import StaticFiles

from .agents.runtime import load_role_config
from .config import ROOT
from .export.package import Exporter
from .models import AgentSettingUpdate, GateRequest, ProjectCreate, RevisionRequest
from .paths import safe_name
from .repository import Repository
from .runner import Runner
from .secrets import SecretStore
from .workflow import Workflow, WorkflowError


def create_app(repo: Repository | None = None, secret_store: SecretStore | None = None) -> FastAPI:
    repo = repo or Repository()
    secret_store = secret_store or SecretStore()
    workflow = Workflow(repo, secret_store)
    runner = Runner(workflow)
    app = FastAPI(title="构策 · 游戏策划文档工作台", docs_url=None, redoc_url=None)
    app.mount("/static", StaticFiles(directory=ROOT / "web"), name="static")
    csrf_token = token_secrets.token_urlsafe(32)

    @app.middleware("http")
    async def local_write_guard(request: Request, call_next):
        if request.method not in ("GET", "HEAD", "OPTIONS"):
            if request.headers.get("x-gdd-token") != csrf_token:
                return JSONResponse({"detail": "写入请求缺少本机令牌"}, status_code=403)
            if request.client and request.client.host not in ("127.0.0.1", "::1", "testclient"):
                return JSONResponse({"detail": "仅允许本机操作"}, status_code=403)
        return await call_next(request)

    @app.exception_handler(WorkflowError)
    async def workflow_error(_, exc: WorkflowError):
        return JSONResponse({"detail": str(exc)}, status_code=409)

    @app.exception_handler(ValueError)
    async def value_error(_, exc: ValueError):
        return JSONResponse({"detail": str(exc)}, status_code=400)

    def role_settings() -> dict:
        result = {}
        for role in ("research", "architect", "writer", "interaction", "producer"):
            config = load_role_config(repo, role)
            result[role] = {k: v for k, v in config.items() if k != "system"}
            result[role]["prompt"] = repo.setting(f"prompt:{role}", (ROOT / f"agents/{role}.md").read_text(encoding="utf-8"))
            result[role]["key_configured"] = secret_store.configured(config["api_key_env"])
        return result

    @app.get("/", response_class=HTMLResponse)
    def index():
        return (ROOT / "web/index.html").read_text(encoding="utf-8")

    @app.get("/api/bootstrap")
    def bootstrap():
        return {"csrf_token": csrf_token, "projects": repo.list_projects(), "roles": role_settings(),
                "search_configured": secret_store.configured("WEB_SEARCH_API_KEY")}

    @app.get("/api/projects/{project_id}")
    def project(project_id: str, version_limit: int = 50, evidence_limit: int = 100):
        return {"project": workflow.get(project_id), "events": repo.events(project_id),
                "versions": repo.versions(project_id, min(max(version_limit, 1), 100)),
                "evidence": repo.evidence_list(project_id, min(max(evidence_limit, 1), 200)),
                "assets": repo.assets(project_id)}

    @app.post("/api/projects", status_code=201)
    def create(request: ProjectCreate):
        project = workflow.create(request)
        runner.start(project["id"], "research")
        return {"project": workflow.get(project["id"])}

    @app.post("/api/projects/{project_id}/stages/{stage}/feedback")
    def feedback(project_id: str, stage: str, request: GateRequest):
        if not request.feedback.strip() and request.content is None:
            raise HTTPException(400, "请填写反馈或修改内容")
        updated = workflow.feedback(project_id, stage, request.content, request.feedback)
        return {"project": updated}

    @app.post("/api/projects/{project_id}/stages/{stage}/approve")
    def approve(project_id: str, stage: str):
        project, next_stage = workflow.approve(project_id, stage)
        runner.start(project_id, next_stage)
        return {"project": workflow.get(project_id), "next_stage": next_stage}

    @app.post("/api/projects/{project_id}/run/{stage}")
    def run(project_id: str, stage: str):
        runner.start(project_id, stage)
        return {"status": "started"}

    @app.post("/api/projects/{project_id}/cancel")
    def cancel(project_id: str):
        runner.cancel(project_id)
        return {"status": "cancel_requested"}

    @app.post("/api/projects/{project_id}/revision")
    def revision(project_id: str, request: RevisionRequest):
        project, stage = workflow.start_revision(project_id, request)
        runner.start(project["id"], stage)
        return {"project": workflow.get(project["id"])}

    @app.put("/api/projects/{project_id}/document")
    def edit_document(project_id: str, request: GateRequest):
        if not request.content:
            raise HTTPException(400, "文档不能为空")
        workflow.manual_document(project_id, request.content)
        return {"project": workflow.get(project_id)}

    @app.get("/api/projects/{project_id}/preview")
    def preview(project_id: str):
        project = workflow.get(project_id)
        content = project["artifacts"].get("document", {}).get("content", {}).get("markdown", "")
        html = markdown_lib.markdown(content, extensions=["tables", "fenced_code", "sane_lists"])
        soup = BeautifulSoup(html, "html.parser")
        for tag in soup(["script", "style", "iframe", "object", "embed", "form"]):
            tag.decompose()
        for tag in soup.find_all(True):
            for attribute in list(tag.attrs):
                if attribute.lower().startswith("on") or attribute.lower() in ("srcdoc", "style"):
                    del tag.attrs[attribute]
            if tag.name == "a":
                href = tag.get("href", "")
                if not href.startswith(("http://", "https://", "#")):
                    del tag.attrs["href"]
                else:
                    tag["target"] = "_blank"
                    tag["rel"] = "noopener noreferrer"
            if tag.name == "img":
                source = tag.get("src", "")
                tag["src"] = f"/api/projects/{project_id}/asset?path={source}"
                tag["loading"] = "lazy"
        return {"html": str(soup), "markdown": content}

    @app.get("/api/projects/{project_id}/asset")
    def asset(project_id: str, path: str):
        project = workflow.get(project_id)
        root = Path(project["artifacts"]["working_dir"]).resolve()
        target = (root / path).resolve()
        if root not in target.parents or not target.is_file() or target.suffix.lower() not in (".png", ".jpg", ".jpeg", ".webp"):
            raise HTTPException(404, "图片不存在")
        return FileResponse(target)

    @app.get("/api/projects/{project_id}/export")
    def export(project_id: str):
        return workflow.archive(project_id, Exporter(repo, secret_store).export)

    @app.get("/api/projects/{project_id}/download/{kind}")
    def download(project_id: str, kind: str):
        project = workflow.get(project_id)
        if project["status"] != "approved":
            raise HTTPException(409, "审核通过并导出后才能下载")
        root = Path(project["artifacts"]["working_dir"])
        title = safe_name(project["name"])
        candidates = {"markdown": root / f"{title}.md", "docx": root / f"{title}.docx",
                      "zip": root / f"{title}-v{project['revision']}.zip"}
        path = candidates.get(kind)
        if not path or not path.is_file():
            raise HTTPException(404, "请先生成导出包")
        return FileResponse(path, filename=path.name)

    @app.get("/api/settings")
    def settings():
        return role_settings()

    @app.put("/api/settings/{role}")
    def save_settings(role: str, update: AgentSettingUpdate):
        if role not in ("research", "architect", "writer", "interaction", "producer"):
            raise HTTPException(404, "未知角色")
        data = update.model_dump(exclude={"api_key"})
        prompt = data.pop("prompt")
        repo.set_setting(f"agent:{role}", data)
        repo.set_setting(f"prompt:{role}", prompt)
        if update.api_key:
            secret_store.set(update.api_key_env, update.api_key)
        return {"status": "saved", "key_configured": secret_store.configured(update.api_key_env)}

    @app.put("/api/settings/search/key")
    def save_search_key(request: dict):
        key = str(request.get("api_key", "")).strip()
        if not key:
            raise HTTPException(400, "密钥不能为空")
        secret_store.set("WEB_SEARCH_API_KEY", key)
        return {"status": "saved", "configured": True}

    @app.get("/api/health")
    def health():
        return {"status": "ok"}

    @app.on_event("shutdown")
    def shutdown():
        runner.stop()

    return app
