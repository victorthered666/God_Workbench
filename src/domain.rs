use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use validator::Validate;

pub type JsonMap = BTreeMap<String, Value>;
pub fn now() -> String { Utc::now().to_rfc3339() }

/// LLM 结构化输出的常见形状错误：把字符串字段写成数组/对象。
/// 宽容反序列化为字符串（数组按行拼接），而不是整个任务失败。
pub mod flex {
    use serde::{Deserialize, Deserializer};
    use serde_json::Value;

    pub fn string_or_sequence<'de, D>(deserializer: D) -> Result<String, D::Error>
    where D: Deserializer<'de> {
        Ok(match Value::deserialize(deserializer)? {
            Value::String(text) => text,
            Value::Array(items) => items.iter().map(|item| match item {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            }).collect::<Vec<_>>().join("\n"),
            Value::Null => String::new(),
            other => other.to_string(),
        })
    }

    /// 模型可能把数值写成 "2-3页" / "3" / null：取字符串里首个数字段。
    pub fn number_lenient<'de, D>(deserializer: D) -> Result<f32, D::Error>
    where D: Deserializer<'de> {
        Ok(match Value::deserialize(deserializer)? {
            Value::Number(n) => n.as_f64().unwrap_or(0.0) as f32,
            Value::String(text) => text.split(|c: char| !c.is_ascii_digit() && c != '.')
                .find(|part| part.chars().any(|c| c.is_ascii_digit()))
                .and_then(|part| part.parse::<f32>().ok())
                .unwrap_or(0.0),
            _ => 0.0,
        })
    }
}
pub fn id() -> String { uuid::Uuid::new_v4().simple().to_string() }

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct ProjectCreate {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    #[validate(length(min = 1, max = 12000))]
    pub purpose: String,
    #[validate(length(min = 1, max = 12000))]
    pub references: String,
    #[serde(default)] pub platform: String,
    #[serde(default)] pub audience: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub reference_text: String,
    pub platform: String,
    pub audience: String,
    pub status: String,
    pub revision: u32,
    pub review_round: u32,
    pub artifacts: Value,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Project {
    pub fn artifact(&self, stage: &str) -> Option<&Value> {
        self.artifacts.get(stage).and_then(|value| value.get("content"))
    }
    pub fn approved(&self, stage: &str) -> bool {
        self.artifacts.get(stage).and_then(|value| value.get("approved")).and_then(Value::as_bool).unwrap_or(false)
    }
    pub fn working_dir(&self) -> crate::Result<std::path::PathBuf> {
        self.artifacts.get("working_dir").and_then(Value::as_str).map(Into::into).ok_or_else(|| crate::AppError::Workflow("项目缺少工作目录".into()))
    }
    pub fn running(&self) -> bool { self.status.starts_with("running:") }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct GateRequest {
    #[serde(default)] pub feedback: String,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevisionRequest {
    pub purpose: String,
    #[serde(default)] pub scope: String,
    #[serde(default)] pub constraints: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AgentSettings {
    #[serde(default = "default_model")] pub model: String,
    #[serde(default = "default_base_url")] pub base_url: String,
    #[serde(default = "default_key_env")] pub api_key_env: String,
    #[serde(default = "default_temperature")] pub temperature: f64,
    #[serde(default = "default_tokens")] pub max_tokens: u32,
    /// 留空表示跟随模型默认；结构化输出任务建议 disabled，避免推理 token 挤占输出预算。
    #[serde(default = "default_thinking")] pub thinking: String,
    #[serde(default = "default_vision_model")] pub vision_model: String,
    #[serde(default)] pub skills: Vec<String>,
    #[serde(default)] pub prompt: String,
    #[serde(default, skip_serializing)] pub api_key: Option<String>,
}
fn default_model() -> String { "deepseek-v4-pro".into() }
fn default_base_url() -> String { "https://api.deepseek.com".into() }
fn default_key_env() -> String { "DEEPSEEK_API_KEY".into() }
fn default_temperature() -> f64 { 0.3 }
fn default_tokens() -> u32 { 16000 }
fn default_thinking() -> String { "enabled".into() }
fn default_vision_model() -> String { "deepseek-flash".into() }

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ResearchReport {
    #[serde(deserialize_with = "flex::string_or_sequence")] pub summary: String,
    pub findings: Vec<Finding>,
    pub recommendations: Vec<String>,
    pub writing_advice: Vec<String>,
    #[serde(default)] pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Finding {
    pub topic: String,
    #[serde(deserialize_with = "flex::string_or_sequence")] pub observation: String,
    #[serde(deserialize_with = "flex::string_or_sequence")] pub implication: String,
    #[serde(default)] pub evidence_ids: Vec<String>,
    pub confidence: Confidence,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum Confidence { 事实, 推断, 建议 }

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Framework {
    pub title: String,
    pub summary: String,
    pub sections: Vec<FrameworkSection>,
    #[serde(default)] pub boundaries: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct FrameworkSection {
    pub id: String,
    pub title: String,
    pub purpose: String,
    pub requirements: Vec<String>,
    #[serde(default)] pub visuals: Vec<String>,
    #[serde(default)] pub acceptance: Vec<String>,
    /// 该章撰写完成后的预估正文页数（不含附录）；模型缺省时由确定性公式兜底。
    #[serde(default, deserialize_with = "flex::number_lenient")]
    pub estimated_pages: f32,
}

/// 单份文档页数上限：撰写前拆分评估的阈值。
pub const PAGE_LIMIT: f32 = 30.0;

/// 撰写前拆分评估（框架确认闸门展示）。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SplitPlan {
    pub estimated_total_pages: f32,
    pub page_limit: u32,
    pub recommended: bool,
    pub advice: String,
    #[serde(default)] pub parts: Vec<SplitPart>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SplitPart {
    pub name: String,
    pub title: String,
    pub section_ids: Vec<String>,
    pub estimated_pages: f32,
}

impl SplitPlan {
    /// 由框架章节评估：模型预估与条目密度机械估算取较大者——
    /// 实测模型对页数系统性低估（巫师3实测 129 页，模型仅估 ~28），
    /// 密度下限（每条要求/验收/图示 ≈0.8 页，章基础 2 页）更接近真实成文篇幅。
    /// 超过页数上限时按顺序装箱：卷数 = ceil(总页数/25)，保证单卷贴近 25 页、
    /// 尽量不超过 30 页上限；剩余章节数等于剩余卷数时每卷强制保留至少一章。
    pub fn evaluate(sections: &[FrameworkSection]) -> Self {
        let density=|section:&FrameworkSection| 2.0 + 0.8*(section.requirements.len()+section.acceptance.len()+section.visuals.len()) as f32;
        let estimate=|section:&FrameworkSection| section.estimated_pages.max(density(section));
        let estimates:Vec<f32>=sections.iter().map(estimate).collect();
        let total: f32 = estimates.iter().sum();
        let recommended = total > PAGE_LIMIT;
        let mut parts:Vec<SplitPart>=Vec::new();
        if recommended {
            let target_parts=(((total/25.0).ceil() as usize).max(2)).min(sections.len());
            let per_part=total/target_parts as f32;
            let mut chunk: Vec<(String,f32)>=Vec::new();
            let mut chunk_pages=0.0;
            let mut consumed=0usize;
            for (index,section) in sections.iter().enumerate() {
                let current=estimates[index];
                let sections_after=sections.len()-index-1;
                let parts_left_after=target_parts-parts.len()-1;
                // 剩余章节数恰好等于剩余卷数：每卷必须至少留一章，当前卷立即收口；
                // 中点启发式：本卷页数加上下一章的一半已超过平均目标时落袋，避免最后一卷爆仓。
                let must_reserve=parts_left_after>0 && sections_after==parts_left_after && !chunk.is_empty();
                let balanced=!chunk.is_empty() && chunk_pages+current*0.5>per_part;
                if (must_reserve||balanced) && sections_after>=parts_left_after {
                    parts.push(Self::part(parts.len(),&sections[consumed..index],&chunk));
                    consumed=index;
                    chunk.clear();
                    chunk_pages=0.0;
                }
                chunk.push((section.id.clone(),current));
                chunk_pages+=current;
            }
            if !chunk.is_empty() { parts.push(Self::part(parts.len(),&sections[consumed..],&chunk)); }
        }
        let advice=if recommended {
            format!("预估正文约 {:.0} 页，超过单份文档 {:.0} 页的可读上限。建议按下方拆分为 {} 卷分别撰写：每卷是独立项目，走同一五角色流水线；先写总纲卷、后写分卷，卷间通过相互引用保持数值一致。",total,PAGE_LIMIT,parts.len())
        } else {
            format!("预估正文约 {:.0} 页，在单份文档 {:.0} 页的可读上限内，可按单文档撰写。",total,PAGE_LIMIT)
        };
        Self { estimated_total_pages:total, page_limit:PAGE_LIMIT as u32, recommended, advice, parts }
    }

    fn part(index: usize, sections: &[FrameworkSection], chunk: &[(String,f32)]) -> SplitPart {
        let pages: f32 = chunk.iter().map(|(_,p)|*p).sum();
        const NAMES:[&str;16]=["卷一","卷二","卷三","卷四","卷五","卷六","卷七","卷八","卷九","卷十","卷十一","卷十二","卷十三","卷十四","卷十五","卷十六"];
        let name=NAMES[index.min(NAMES.len()-1)].to_string();
        let first=sections.first().map(|s|s.title.as_str()).unwrap_or("");
        let last=sections.last().map(|s|s.title.as_str()).unwrap_or("");
        let title=if sections.len()<=1 || first==last { first.to_string() } else { format!("{first} ~ {last}") };
        SplitPart { name, title, section_ids: chunk.iter().map(|(id,_)|id.clone()).collect(), estimated_pages:pages }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DocumentDraft { #[serde(deserialize_with = "flex::string_or_sequence")] pub markdown: String }

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct InteractionSupplement {
    /// 完整的交互设计补充章节 Markdown，以 ## 标题开头；不含原文其他内容。
    #[serde(deserialize_with = "flex::string_or_sequence")]
    pub markdown: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DocumentPatch {
    /// 逐字取自文档章节清单的标题
    pub heading: String,
    /// 该章节的完整替换 Markdown（以标题行开头）
    pub markdown: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DocumentPatches { pub patches: Vec<DocumentPatch> }

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ImpactPlan {
    pub research_needed: bool,
    #[serde(deserialize_with = "flex::string_or_sequence")] pub reason: String,
    #[serde(default)] pub affected_sections: Vec<String>,
    #[serde(default)] pub preserved_sections: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct Review {
    #[serde(default)]
    pub approved: bool,
    #[serde(default)] pub findings: Vec<ReviewFinding>,
    #[serde(default, deserialize_with = "flex::string_or_sequence")] pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReviewFinding {
    #[serde(default, deserialize_with = "flex::string_or_sequence")] pub section: String,
    #[serde(default, deserialize_with = "flex::string_or_sequence")] pub problem: String,
    #[serde(default)] pub severity: Severity,
    /// 模型偶尔漏填 owner；默认 writer（绝大多数发现在正文层面可修）。
    #[serde(default = "default_finding_owner")] pub owner: Role,
    #[serde(default, deserialize_with = "flex::string_or_sequence")] pub recommendation: String,
}
fn default_finding_owner() -> Role { Role::Writer }


#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Role { Research, Architect, Writer, Interaction, Producer }
impl Role {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Research => "research", Self::Architect => "architect", Self::Writer => "writer", Self::Interaction => "interaction", Self::Producer => "producer" }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct ResourcePlan {
    #[serde(default)] pub features: Vec<ResourceItem>,
    #[serde(default)] pub art: Vec<ResourceItem>,
    #[serde(default)] pub audio: Vec<ResourceItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ResourceItem {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "flex::string_or_sequence")] pub specification: String,
    pub source_section: String,
    #[serde(default)] pub dependencies: String,
    pub priority: Priority,
    #[serde(deserialize_with = "flex::string_or_sequence")] pub acceptance: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum Priority { P0, P1, P2 }

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectAuditCreate {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    /// 已撰写好的完整文档 Markdown。
    pub markdown: String,
    #[serde(default)] pub platform: String,
    #[serde(default)] pub audience: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct StructureAssessment {
    /// 从文档中识别/推断出的设计目的；未写明则描述缺失情况。
    #[serde(default, deserialize_with = "flex::string_or_sequence")] pub design_purpose: String,
    /// 缺失的结构性章节名称（如：设计目的、受众与平台、核心循环、验收标准）。
    #[serde(default)] pub missing_sections: Vec<String>,
    #[serde(default, deserialize_with = "flex::string_or_sequence")] pub comment: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AuditFinding {
    #[serde(default, deserialize_with = "flex::string_or_sequence")] pub section: String,
    #[serde(default, deserialize_with = "flex::string_or_sequence")] pub problem: String,
    /// 文中原文引用，作为问题证据。
    #[serde(default, deserialize_with = "flex::string_or_sequence")] pub evidence: String,
    #[serde(default)] pub severity: Severity,
    #[serde(default, deserialize_with = "flex::string_or_sequence")] pub recommendation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AuditReport {
    #[serde(default)] pub structure: StructureAssessment,
    /// 前后不一致、与设计目的不一致的问题清单。
    #[serde(default)] pub consistency_findings: Vec<AuditFinding>,
    #[serde(default, deserialize_with = "flex::string_or_sequence")] pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Annotation { pub x: f64, pub y: f64, pub text: String }
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Annotations { pub labels: Vec<Annotation> }

pub trait AgentResult: serde::de::DeserializeOwned + JsonSchema + Serialize + Send + 'static {}
impl<T: serde::de::DeserializeOwned + JsonSchema + Serialize + Send + 'static> AgentResult for T {}

#[allow(dead_code)]
pub struct VersionRecord { pub id: String, pub project_id: String, pub stage: String, pub version: u32, pub content: Value, pub feedback: String, pub created_at: String }

#[cfg(test)]
mod flex_tests {
    use super::flex::string_or_sequence;

    #[test]
    fn tolerates_sequence_and_null_shapes() {
        #[derive(serde::Deserialize)]
        struct Probe {
            #[serde(deserialize_with = "string_or_sequence")]
            text: String,
        }
        let plain:Probe=serde_json::from_str(r#"{"text":"普通字符串"}"#).unwrap();
        assert_eq!(plain.text,"普通字符串");
        let listed:Probe=serde_json::from_str(r#"{"text":["第一行","第二行"]}"#).unwrap();
        assert_eq!(listed.text,"第一行\n第二行");
        let null:Probe=serde_json::from_str(r#"{"text":null}"#).unwrap();
        assert_eq!(null.text,"");
    }
}
