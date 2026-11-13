from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

from .agents.runtime import Agent, build_tools
from .config import MAX_REVIEW_ROUNDS, OUTPUT_DIR
from .models import DocumentDraft, Framework, ImpactPlan, ResearchReport, ResourcePlan, Review
from .paths import safe_name
from .repository import Repository, now
from .secrets import SecretStore

STAGES = {
    "research": "research_review", "framework": "framework_review",
    "revision_plan": "revision_review", "document": "interaction",
    "interaction": "resources", "resources": "review",
    "review": "review", "revise": "review",
}


class WorkflowError(RuntimeError):
    pass


def project_dir(project: dict) -> Path:
    return Path(project["artifacts"]["working_dir"])


def write_json(path: Path, content: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(content, ensure_ascii=False, indent=2, default=str), encoding="utf-8")


class Workflow:
    MAX_DOCUMENT_BYTES = 2 * 1024 * 1024

    def __init__(self, repo: Repository, secrets: SecretStore):
        self.repo = repo
        self.secrets = secrets

    def create(self, request: Any) -> dict:
        project = self.repo.create_project(request)
        folder = OUTPUT_DIR / f"{safe_name(request.name)}-{project['id'][:8]}"
        for path in ("assets/images", "assets/diagrams", "research", "reviews", "versions"):
            (folder / path).mkdir(parents=True, exist_ok=True)
        project["artifacts"] = {"working_dir": str(folder)}
        self.repo.update(project["id"], artifacts=project["artifacts"])
        self.repo.event(project["id"], "stage", "已创建项目；等待调研策划")
        return project

    def get(self, project_id: str) -> dict:
        project = self.repo.get(project_id)
        if not project:
            raise WorkflowError("项目不存在")
        return project

    def artifact(self, project: dict, stage: str, required: bool = True) -> Any:
        artifact = project["artifacts"].get(stage)
        if required and not artifact:
            raise WorkflowError(f"缺少 {stage} 阶段产物")
        return artifact["content"] if artifact else None

    def agent(self, role: str, project: dict, budget: int = 24) -> Agent:
        document = self.artifact(project, "document", False)
        if document and len(document.get("markdown", "").encode("utf-8")) > self.MAX_DOCUMENT_BYTES:
            raise WorkflowError("正文超过 2 MB；请先拆分为独立章节项目，避免无界上下文和内存增长")
        agent = Agent(role, self.repo, self.secrets, project, build_tools(role, self.repo, self.secrets, project, budget))
        return agent

    def _save(self, project: dict, stage: str, content: Any, status: str, feedback: str = "") -> None:
        project = self.get(project["id"])
        self.repo.save_artifact(project["id"], stage, content, feedback)
        self.repo.update(project["id"], status=status, error=None)
        root = project_dir(project)
        if isinstance(content, dict) and "markdown" in content:
            (root / f"{stage}.md").write_text(content["markdown"], encoding="utf-8")
        else:
            write_json(root / f"{stage}.json", content)

    def execute(self, project_id: str, stage: str, feedback: str = "") -> dict:
        project = self.get(project_id)
        if project["status"].startswith("running:"):
            raise WorkflowError("已有阶段正在执行，请等待完成")
        expected = {
            "research": ("research_review", "interrupted"),
            "framework": ("framework_review", "interrupted"),
            "revision_plan": ("revision_review", "revision_planning", "interrupted"),
            "document": ("framework_approved", "interrupted"),
            "interaction": ("interaction", "interrupted"),
            "resources": ("resources", "interrupted"),
            "review": ("review", "interrupted"),
            "revise": ("review", "needs_human"),
        }
        if stage == "framework" and project["status"] == "research_review":
            raise WorkflowError("调研意见尚未由用户确认")
        if stage == "document" and project["status"] == "framework_review":
            raise WorkflowError("文档框架尚未由用户确认")
        if project["status"] not in expected.get(stage, ()):
            raise WorkflowError(f"当前状态 {project['status']} 不能执行 {stage}；请先完成或确认前置阶段")
        self.repo.update(project_id, status=f"running:{stage}", error=None)
        self.repo.event(project_id, "stage", f"开始执行 {stage}")
        run_id = self.repo.create_run(project_id, stage, {})
        if feedback:
            self.repo.save_artifact(project_id, "feedback", {"stage": stage, "feedback": feedback}, feedback)
        try:
            result = getattr(self, f"do_{stage}")(project, feedback)
            self.repo.finish_run(run_id)
            self.repo.event(project_id, "stage", f"完成 {stage}")
            return self.get(project_id)
        except Exception as exc:
            message = self.secrets.redact(str(exc))[:2000]
            self.repo.finish_run(run_id, message)
            self.repo.update(project_id, status="interrupted", error=message)
            self.repo.event(project_id, "error", message)
            raise

    def do_research(self, project: dict, feedback: str) -> None:
        instruction = (
            "请针对以下设计目的与参考对象完成有证据的调研。先搜索再抓取重点来源，给出可执行的撰写意见。\n"
            f"设计目的：{project['purpose']}\n参考对象：{project['reference_text']}\n"
            f"平台：{project['platform'] or '未指定'}\n受众：{project['audience'] or '未指定'}\n"
            "最后输出 ResearchReport JSON。来源 ID 必须真实来自工具。"
        )
        result = self.agent("research", project).run(instruction, ResearchReport, feedback,
            history=self.repo.setting(f"session:{project['id']}:research", []))
        self._save(project, "research", result.model_dump(), "research_review", feedback)

    def do_revision_plan(self, project: dict, feedback: str) -> None:
        instruction = (
            "用户希望修改已完成的策划文档。请根据修改目的、范围、约束和当前文档判断影响范围。"
            "不要重写文档。research_needed 表示是否必须重新联网调研；列出受影响与可保留章节。\n"
            f"修改请求：{json.dumps(project['artifacts']['revision_request'], ensure_ascii=False)}\n"
            f"当前文档：{self.artifact(project, 'document')}"
        )
        result = self.agent("architect", project).run(instruction, ImpactPlan, feedback)
        self._save(project, "revision_plan", result.model_dump(), "revision_review", feedback)

    def do_framework(self, project: dict, feedback: str) -> None:
        research = self.artifact(project, "research")
        if not project["artifacts"]["research"].get("approved"):
            raise WorkflowError("调研意见尚未由用户确认")
        instruction = (
            f"根据用户已确认的调研意见形成可执行 GDD 框架。\n{json.dumps(research, ensure_ascii=False)}\n"
            f"修改影响计划（如有）：{json.dumps(self.artifact(project, 'revision_plan', False), ensure_ascii=False)}\n"
            "章节 id 使用稳定英文短横线格式，输出 Framework JSON。"
        )
        result = self.agent("architect", project).run(instruction, Framework, feedback)
        self._save(project, "framework", result.model_dump(), "framework_review", feedback)

    def do_document(self, project: dict, feedback: str) -> None:
        if not project["artifacts"]["framework"].get("approved"):
            raise WorkflowError("文档框架尚未由用户确认")
        evidence = self.repo.evidence_list(project["id"])
        instruction = (
            f"依据已确认框架撰写完整 GDD，不做提纲式占位。\n{json.dumps(self.artifact(project, 'framework'), ensure_ascii=False)}\n"
            f"已登记检索证据（优先使用其 URL，引用格式 [标题](URL)）：{json.dumps(evidence, ensure_ascii=False)[:45000]}\n"
            "关键参考对象至少提供一张真实参考截图，用 image_search 和 select_image；"
            "核心循环至少一张自绘流程图。每张图紧邻解释它的正文，写图号、标题与来源。"
            "输出 DocumentDraft JSON，markdown 为完整文档。"
        )
        if self.artifact(project, "document", False):
            instruction += f"\n保留仍有效的旧版正文，仅修改受影响内容：\n{self.artifact(project, 'document')}"
        result = self.agent("writer", project).run(instruction, DocumentDraft, feedback)
        self._save(project, "document", result.model_dump(), "interaction", feedback)

    def do_interaction(self, project: dict, feedback: str) -> None:
        document = self.artifact(project, "document")
        instruction = (
            "审阅以下 GDD，补全交互入口、操作、状态、反馈、异常与空状态；"
            "至少为关键界面使用 wireframe 生成一张编号线框图，并用正文解释编号。"
            "保留所有原有图片及资源附录占位。输出完整 DocumentDraft JSON。\n" + document["markdown"]
        )
        result = self.agent("interaction", project).run(instruction, DocumentDraft, feedback)
        self._save(project, "document", result.model_dump(), "resources", feedback)
        self.repo.save_artifact(project["id"], "interaction", {"completed": True})

    def do_resources(self, project: dict, feedback: str) -> None:
        instruction = (
            "依据以下实际正文，提取全部功能、美术、音效资源，输出 ResourcePlan JSON。"
            "不得把参考对象中的资源误记为本项目资源。\n" + self.artifact(project, "document")["markdown"]
        )
        result = self.agent("producer", project).run(instruction, ResourcePlan, feedback)
        from .export.resources import resource_markdown
        plan = result.model_dump()
        markdown = re.sub(r"\n## 附录：功能与制作资源拆解[\s\S]*$", "", self.artifact(project, "document")["markdown"])
        markdown += resource_markdown(result)
        self.repo.save_artifact(project["id"], "resources", plan)
        self._save(project, "document", {"markdown": markdown}, "review", feedback)

    def do_review(self, project: dict, feedback: str) -> None:
        evidence = self.repo.evidence_list(project["id"])
        instruction = (
            "独立审核完整 GDD。检查目标一致性、框架覆盖、规则闭环、交互状态、"
            "图文引用、来源可追溯与资源清单。只报告有正文证据的问题；"
            "若可通过，approved=true 且 findings 为空。输出 Review JSON。\n"
            f"已确认框架：{json.dumps(self.artifact(project, 'framework'), ensure_ascii=False)}\n"
            f"文档：{self.artifact(project, 'document')['markdown']}\n"
            f"证据索引：{json.dumps(evidence, ensure_ascii=False)[:20000]}"
        )
        result = self.agent("architect", project, budget=0).run(instruction, Review, feedback)
        review = result.model_dump()
        if not review["approved"] and not review["findings"]:
            raise WorkflowError("审核拒绝但未给出可修订问题，未自动通过")
        self._save(project, "review", review, "review")
        if review["approved"]:
            self.repo.update(project["id"], status="approved", error=None)
        elif project["review_round"] >= MAX_REVIEW_ROUNDS:
            self.repo.update(project["id"], status="needs_human", error="自动修订已达 3 轮，审核仍未通过；请人工处理或明确继续。")

    def do_revise(self, project: dict, feedback: str) -> None:
        review = Review.model_validate(self.artifact(project, "review"))
        if review.approved:
            raise WorkflowError("审核已通过，不需要自动修订")
        if project["review_round"] >= MAX_REVIEW_ROUNDS:
            raise WorkflowError("自动修订已达 3 轮，请人工处理")
        findings_by_role: dict[str, list] = {}
        for finding in review.findings:
            findings_by_role.setdefault(finding.owner, []).append(finding.model_dump())
        if any(role in ("research", "architect") for role in findings_by_role):
            raise WorkflowError("问题涉及调研或架构，请人工调整已确认意见/框架，不绕过人工门禁自动改写")
        document = self.artifact(project, "document")["markdown"]
        for role in ("writer", "interaction", "producer"):
            if role not in findings_by_role:
                continue
            if role == "producer":
                plan = self.agent(role, project).run(
                    f"依据正文和以下审核问题更新资源清单：{json.dumps(findings_by_role[role], ensure_ascii=False)}\n正文：{document}", ResourcePlan)
                from .export.resources import resource_markdown
                self.repo.save_artifact(project["id"], "resources", plan.model_dump())
                document = re.sub(r"\n## 附录：功能与制作资源拆解[\s\S]*$", "", document) + resource_markdown(plan)
            else:
                result = self.agent(role, project).run(
                    f"仅修复以下审核问题，保留其他已验证内容、真实图片与来源，输出完整 DocumentDraft JSON。\n"
                    f"问题：{json.dumps(findings_by_role[role], ensure_ascii=False)}\n正文：{document}", DocumentDraft)
                document = result.markdown
        next_round = project["review_round"] + 1
        self.repo.save_artifact(project["id"], "document", {"markdown": document})
        (project_dir(project) / "document.md").write_text(document, encoding="utf-8")
        self.repo.update(project["id"], review_round=next_round)
        project = self.get(project["id"])
        self.do_review(project, "")

    def feedback(self, project_id: str, stage: str, content: str | None, feedback: str) -> dict:
        project = self.get(project_id)
        if stage not in ("research", "framework", "revision_plan"):
            raise WorkflowError("该阶段不支持反馈修改")
        if project["status"] != STAGES[stage]:
            raise WorkflowError("当前状态不接受此阶段反馈")
        if content is not None:
            if stage == "research":
                content = ResearchReport.model_validate_json(content).model_dump()
            elif stage == "framework":
                content = Framework.model_validate_json(content).model_dump()
            else:
                content = ImpactPlan.model_validate_json(content).model_dump()
            self._save(project, stage, content, project["status"], feedback)
        if content is not None:
            return self.get(project_id)
        return self.execute(project_id, stage, feedback)

    def approve(self, project_id: str, stage: str) -> tuple[dict, str | None]:
        project = self.get(project_id)
        if project["status"] != STAGES.get(stage):
            raise WorkflowError("当前阶段不可确认或前置条件尚未满足")
        if content := project["artifacts"].get(stage):
            content["approved"] = True
            self.repo.update(project_id, artifacts=project["artifacts"])
        self.repo.approve(project_id, stage)
        self.repo.event(project_id, "gate", f"用户确认 {stage}")
        next_stage = {"research": "framework", "framework": "document", "revision_plan": None}[stage]
        if stage == "revision_plan":
            plan = self.artifact(project, "revision_plan")
            next_stage = "research" if plan["research_needed"] else "framework"
            if next_stage == "framework":
                artifacts = self.repo.get(project_id)["artifacts"]
                artifacts["research"]["approved"] = True
                self.repo.update(project_id, artifacts=artifacts)
        project = self.get(project_id)
        project["status"] = "framework_approved" if stage == "framework" else "interrupted"
        self.repo.update(project_id, status=project["status"])
        if next_stage == "document":
            self.repo.update(project_id, status="framework_approved")
        return project, next_stage

    def start_revision(self, project_id: str, request: Any) -> tuple[dict, str]:
        project = self.get(project_id)
        if project["status"].startswith("running:"):
            raise WorkflowError("任务运行时不能发起修改")
        if not self.artifact(project, "document", False):
            raise WorkflowError("尚无可修改的正文")
        artifacts = project["artifacts"]
        artifacts["revision_request"] = request.model_dump()
        artifacts["base_document"] = self.artifact(project, "document")
        for stage in ("research", "framework", "document", "interaction", "resources", "review", "revision_plan"):
            if stage in artifacts and isinstance(artifacts[stage], dict):
                artifacts[stage]["approved"] = False
        self.repo.update(project_id, artifacts=artifacts, revision=project["revision"]+1,
                         review_round=0, status="revision_planning", error=None)
        self.repo.save_artifact(project_id, "revision_request", request.model_dump())
        return self.get(project_id), "revision_plan"

    def manual_document(self, project_id: str, markdown: str) -> None:
        project = self.get(project_id)
        if project["status"].startswith("running:"):
            raise WorkflowError("任务运行时不能手动编辑")
        if not markdown.strip():
            raise WorkflowError("文档不能为空")
        self._save(project, "document", {"markdown": markdown}, "resources" if self.artifact(project, "document", False) else project["status"])
        self.repo.event(project_id, "edit", "用户手动保存正文；需重新整理资源并审核")

    def archive(self, project_id: str, exporter) -> dict:
        project = self.get(project_id)
        if project["status"] != "approved" or not self.artifact(project, "review").get("approved"):
            raise WorkflowError("架构审核尚未通过，不能标记为定稿导出")
        return exporter(project)
