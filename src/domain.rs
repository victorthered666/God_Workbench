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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
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
