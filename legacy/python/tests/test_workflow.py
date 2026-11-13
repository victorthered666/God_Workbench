from __future__ import annotations

import pytest

from app.models import DocumentDraft, Framework, ResearchReport, ResourcePlan, Review, RevisionRequest
from app.workflow import WorkflowError, safe_name
from app.repository import Repository


RESEARCH = {"summary": "调研结论", "findings": [{"topic": "循环", "observation": "搜索证据", "implication": "减少操作层级", "evidence_ids": ["a1"], "confidence": "事实"}], "recommendations": ["先做采集闭环"], "writing_advice": ["明确资源消耗"], "limitations": []}
FRAMEWORK = {"title": "测试策划", "summary": "框架", "sections": [{"id": "loop", "title": "核心循环", "purpose": "明确循环", "requirements": ["采集与消耗"], "visuals": ["流程图"], "acceptance": ["闭环可验收"]}], "boundaries": []}
DOCUMENT = "# 测试策划\n\n## 核心循环\n\n玩家通过采集获得补给。\n\n## 交互\n\n| 操作 | 反馈 |\n| --- | --- |\n| 采集 | 资源入包 |\n\n" + "这是用于离线测试的模拟正文，不代表真实调研或已验证设计。" * 6
RESOURCES = {"features": [{"id": "F-001", "name": "采集", "specification": "点击资源", "source_section": "核心循环", "priority": "P0", "acceptance": "资源入包"}], "art": [], "audio": []}
REVIEW = {"approved": True, "findings": [], "summary": "模拟审核通过"}


def results(review=REVIEW):
    return {ResearchReport: RESEARCH, Framework: FRAMEWORK, DocumentDraft: {"markdown": DOCUMENT}, ResourcePlan: RESOURCES, Review: review}


def test_full_workflow_requires_real_gates(workspace, project, monkeypatch):
    _, repo, _, flow = workspace
    fake = type("Fake", (), {})()
    fake.run = lambda instruction, result_type, feedback="", images=None, history=None: result_type.model_validate(results()[result_type])
    monkeypatch.setattr(flow, "agent", lambda *args, **kwargs: fake)
    with pytest.raises(WorkflowError, match="前置"):
        flow.execute(project["id"], "document")
    flow.execute(project["id"], "research")
    with pytest.raises(WorkflowError, match="尚未由用户确认"):
        flow.execute(project["id"], "framework")
    _, next_stage = flow.approve(project["id"], "research")
    assert next_stage == "framework"
    flow.execute(project["id"], "framework")
    flow.approve(project["id"], "framework")
    for stage in ("document", "interaction", "resources", "review"):
        flow.execute(project["id"], stage)
    assert flow.get(project["id"])["status"] == "approved"
    assert flow.get(project["id"])["artifacts"]["document"]["content"]["markdown"].count("附录") == 1
    assert len(repo.versions(project["id"])) >= 7
    with pytest.raises(WorkflowError, match="当前状态"):
        flow.execute(project["id"], "research")


def test_manual_feedback_and_framework_confirmation(workspace, project):
    _, repo, _, flow = workspace
    import json
    flow.repo.save_artifact(project["id"], "research", RESEARCH)
    flow.repo.update(project["id"], status="research_review")
    with pytest.raises(Exception):
        flow.feedback(project["id"], "research", "{bad json", "手动修改")
    flow.feedback(project["id"], "research", json.dumps(RESEARCH), "仅手动修改")
    assert flow.get(project["id"])["artifacts"]["research"]["version"] == 2


def test_review_never_passes_after_three_rounds(workspace, project, monkeypatch):
    _, repo, _, flow = workspace
    denied = {"approved": False, "findings": [{"section": "核心循环", "problem": "缺少出口", "severity": "high", "owner": "writer", "recommendation": "补充出口"}], "summary": "不通过"}
    fake = type("Fake", (), {})()
    fake.run = lambda instruction, result_type, feedback="", images=None, history=None: result_type.model_validate(results(denied)[result_type])
    monkeypatch.setattr(flow, "agent", lambda *args, **kwargs: fake)
    flow.repo.save_artifact(project["id"], "document", {"markdown": DOCUMENT})
    flow.repo.save_artifact(project["id"], "framework", FRAMEWORK)
    flow.repo.update(project["id"], status="review")
    for expected_round in (1, 2, 3):
        flow.execute(project["id"], "review")
        assert flow.get(project["id"])["status"] == "review"
        flow.execute(project["id"], "revise")
        assert flow.get(project["id"])["review_round"] == expected_round
    assert flow.get(project["id"])["status"] == "needs_human"
    with pytest.raises(WorkflowError, match="3 轮"):
        flow.execute(project["id"], "revise")


def test_architecture_issue_returns_to_human(workspace, project, monkeypatch):
    _, repo, _, flow = workspace
    denied = {"approved": False, "findings": [{"section": "框架", "problem": "目标矛盾", "severity": "high", "owner": "architect", "recommendation": "重新确认框架"}], "summary": "不通过"}
    repo.save_artifact(project["id"], "document", {"markdown": DOCUMENT})
    repo.save_artifact(project["id"], "review", denied)
    repo.update(project["id"], status="review")
    with pytest.raises(WorkflowError, match="人工调整"):
        flow.execute(project["id"], "revise")


def test_revision_retains_base_and_needs_review(workspace, project, monkeypatch):
    _, repo, _, flow = workspace
    repo.save_artifact(project["id"], "document", {"markdown": DOCUMENT})
    repo.save_artifact(project["id"], "review", REVIEW)
    repo.approve(project["id"], "review")
    repo.update(project["id"], status="approved")
    from app.models import ImpactPlan
    plan = {"research_needed": False, "reason": "局部规则修改", "affected_sections": ["核心循环"], "preserved_sections": ["交互"]}
    monkeypatch.setattr(flow, "agent", lambda *args, **kwargs: type("F", (), {"run": lambda self, *a, **k: type("R", (), {"model_dump": lambda s: plan})()})())
    flow.start_revision(project["id"], RevisionRequest(purpose="调整资源产出", constraints="保留交互"))
    flow.execute(project["id"], "revision_plan")
    assert flow.get(project["id"])["revision"] == 2
    assert flow.get(project["id"])["artifacts"]["base_document"]["markdown"] == DOCUMENT
    assert flow.get(project["id"])["status"] == "revision_review"


def test_recover_interrupted(workspace, project):
    _, repo, _, flow = workspace
    repo.update(project["id"], status="running:research")
    Repository(repo.path)
    assert repo.get(project["id"])["status"] == "interrupted"


def test_safe_folder_name():
    assert "/" not in safe_name("a/b")
    assert safe_name("...") == "未命名策划文档"
