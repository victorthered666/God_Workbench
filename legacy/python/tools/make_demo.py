"""Install an explicitly simulated demo for offline UI/export acceptance."""
from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from app.repository import Repository
from app.secrets import SecretStore
from app.workflow import Workflow
from app.config import DB_PATH

repo = Repository(DB_PATH)
secret_store = SecretStore()
workflow = Workflow(repo, secret_store)
from app.models import ProjectCreate


def main():
    if any(project["name"] == "星屿采集玩法设计（离线演示）" for project in repo.list_projects()):
        raise SystemExit("Demo already exists; not overwriting it")
    source = ROOT / "data/qa-final"
    markdown = (source / "星屿采集玩法设计（离线模拟样例）.md").read_text(encoding="utf-8")
    project = workflow.create(ProjectCreate(
        name="星屿采集玩法设计（离线演示）",
        purpose="仅用于验收图文预览、来源记录、资源拆解与导出；不是实际调研成果。",
        references="本地程序生成的模拟界面，非真实游戏参考对象。", platform="PC", audience="界面验收"))
    root = Path(project["artifacts"]["working_dir"])
    shutil.copytree(source / "assets", root / "assets", dirs_exist_ok=True)
    for stage, content in (
        ("research", {"summary": "离线模拟，未执行网络调研。", "findings": [{"topic": "模拟循环", "observation": "本地构造的设计样例", "implication": "仅用于检查文档结构", "evidence_ids": [], "confidence": "建议"}], "recommendations": ["正式项目请使用真实 Agent 调研"], "writing_advice": ["核对规则、图示与附录"], "limitations": ["无网络证据，不代表真实竞品分析"]}),
        ("framework", {"title": project["name"], "summary": "离线模拟框架", "sections": [{"id": "loop", "title": "核心循环", "purpose": "验收流程图与规则", "requirements": ["探索、收集、制作"], "visuals": ["流程图"], "acceptance": ["结构与图文对应"]}], "boundaries": ["无真实调研证据"]}),
        ("document", {"markdown": markdown}),
        ("interaction", {"completed": True, "note": "本地模拟"}),
        ("resources", {"note": "资源清单已在正文附录中，由本地模拟样例生成"}),
        ("review", {"approved": True, "findings": [], "summary": "本地模拟审核：仅表示导出流程测试通过，不代表真实 Agent 审核通过。"}),
    ):
        repo.save_artifact(project["id"], stage, content)
    repo.evidence(project["id"], "无网络检索", {"kind": "offline", "results": [{"id": "OFFLINE", "title": "离线模拟素材，无外部来源", "url": "", "snippet": "所有图像均由本地程序绘制；未调用搜索或模型。"}]})
    repo.asset(project["id"], "离线程序绘制", "无外部来源", "assets/images/mock-source.png", "offline-demo", {"width": 1200, "height": 700, "labels": ["状态区域（模拟）", "操作区域（模拟）"]})
    for stage in ("research", "framework", "review"):
        repo.approve(project["id"], stage)
    repo.update(project["id"], status="approved", error=None)
    project = repo.get(project["id"])
    from app.export.package import Exporter
    result = Exporter(repo, secret_store).export(project)
    print(json.dumps({"project_id": project["id"], **result}, ensure_ascii=False))


if __name__ == "__main__":
    main()
