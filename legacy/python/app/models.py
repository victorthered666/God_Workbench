from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, Field, HttpUrl, field_validator


class ProjectCreate(BaseModel):
    name: str = Field(min_length=1, max_length=120)
    purpose: str = Field(min_length=1, max_length=12000)
    references: str = Field(min_length=1, max_length=12000)
    platform: str = ""
    audience: str = ""


class RevisionRequest(BaseModel):
    purpose: str = Field(min_length=1, max_length=12000)
    scope: str = ""
    constraints: str = ""


class GateRequest(BaseModel):
    feedback: str = Field(default="", max_length=12000)
    content: str | None = Field(default=None, max_length=300000)


class AgentSetting(BaseModel):
    model: str = "deepseek-v4-pro"
    base_url: str = "https://api.deepseek.com"
    api_key_env: str = "DEEPSEEK_API_KEY"
    temperature: float = Field(default=0.3, ge=0, le=2)
    max_tokens: int = Field(default=16000, ge=512, le=65536)
    vision_model: str = "deepseek-flash"
    skills: list[str] = Field(default_factory=list)
    prompt: str = ""


class AgentSettingUpdate(AgentSetting):
    api_key: str | None = Field(default=None, max_length=512)


class Evidence(BaseModel):
    id: str
    title: str
    url: str
    snippet: str = ""
    content: str = ""
    site_name: str = ""
    date: str | None = None
    authority: int | None = None


class Finding(BaseModel):
    topic: str
    observation: str
    implication: str
    evidence_ids: list[str] = Field(default_factory=list)
    confidence: Literal["事实", "推断", "建议"] = "推断"


class ResearchReport(BaseModel):
    summary: str
    findings: list[Finding] = Field(min_length=1)
    recommendations: list[str] = Field(min_length=1)
    writing_advice: list[str] = Field(min_length=1)
    limitations: list[str] = Field(default_factory=list)


class FrameworkSection(BaseModel):
    id: str
    title: str
    purpose: str
    requirements: list[str] = Field(min_length=1)
    visuals: list[str] = Field(default_factory=list)
    acceptance: list[str] = Field(default_factory=list)


class Framework(BaseModel):
    title: str
    summary: str
    sections: list[FrameworkSection] = Field(min_length=1)
    boundaries: list[str] = Field(default_factory=list)


class ReviewFinding(BaseModel):
    section: str
    problem: str
    severity: Literal["low", "medium", "high"]
    owner: Literal["research", "architect", "writer", "interaction", "producer"]
    recommendation: str


class Review(BaseModel):
    approved: bool
    findings: list[ReviewFinding] = Field(default_factory=list)
    summary: str


class ResourceItem(BaseModel):
    id: str
    name: str
    specification: str
    source_section: str
    dependencies: str = ""
    priority: Literal["P0", "P1", "P2"] = "P1"
    acceptance: str


class ResourcePlan(BaseModel):
    features: list[ResourceItem] = Field(default_factory=list)
    art: list[ResourceItem] = Field(default_factory=list)
    audio: list[ResourceItem] = Field(default_factory=list)


class ImpactPlan(BaseModel):
    research_needed: bool
    reason: str
    affected_sections: list[str] = Field(default_factory=list)
    preserved_sections: list[str] = Field(default_factory=list)


class DocumentDraft(BaseModel):
    markdown: str = Field(min_length=100)


class SearchResult(BaseModel):
    title: str
    url: str
    snippet: str = ""
    summary: str = ""
    content: str = ""
    site_name: str = ""
    date: str | None = None
    authority: int | None = None


class ImageCandidate(BaseModel):
    url: HttpUrl
    title: str = ""
    width: int | None = None
    height: int | None = None
    source_page: str = ""
