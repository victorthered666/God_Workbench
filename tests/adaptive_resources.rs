use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use async_trait::async_trait;
use gdd_harness::{agent::runtime::{AgentExecutor, AgentTask}, config::Config, domain::{Annotation, Project, Role}, repository::Repository, workflow::Workflow};
use serde_json::{json, Value};

/// 首次调用模拟输出截断，之后按片段名返回小规模资源，验证自适应拆分重试。
struct FlakyProducer { calls: AtomicUsize }
#[async_trait]
impl AgentExecutor for FlakyProducer {
    async fn run(&self, task: AgentTask, _project: &Project, instruction: String, _feedback: &str) -> gdd_harness::Result<Value> {
        match task {
            AgentTask::Resources=>{
                if self.calls.fetch_add(1,Ordering::SeqCst)==0 {
                    return Err(gdd_harness::AppError::OutputTruncated);
                }
                let name=if instruction.contains("内容甲") {"甲块资源"} else {"乙块资源"};
                Ok(json!({"features":[{"id":"F-999","name":name,"specification":"模拟规格","source_section":name,"dependencies":"","priority":"P1","acceptance":"可验收"}],"art":[],"audio":[]}))
            },
            AgentTask::Research|AgentTask::Framework|AgentTask::DocumentAudit|AgentTask::RevisionPlan|AgentTask::Review|AgentTask::Document|AgentTask::DocumentPatch|AgentTask::InteractionSupplement=>Ok(json!({"summary":"无"})),
        }
    }
    async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
}

#[tokio::test]
async fn truncated_resource_chunk_is_split_and_merged() {
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("test.db")).unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(FlakyProducer{calls:AtomicUsize::new(0)}),config);
    let project=workflow.create(&gdd_harness::domain::ProjectCreate{name:"自适应拆分测试".into(),purpose:"验证截断重试".into(),references:"离线对象".into(),platform:"".into(),audience:"".into()}).unwrap();
    let filler="这是用于撑起章节长度的离线模拟正文。".repeat(20);
    let document=format!("# 标题\n\n## 一\n\n内容甲开头。\n\n### 一甲\n\n{filler}\n\n## 二\n\n内容乙开头。\n\n### 二甲\n\n{filler}\n");
    repo.save_artifact(&project.id,"document",json!({"markdown":document}),"").unwrap();
    repo.update(&project.id,Some("resources"),None,None,None,None).unwrap();
    workflow.execute(&project.id,"resources","").await.unwrap();
    let finished=repo.require(&project.id).unwrap();
    assert_eq!(finished.status,"review");
    let plan:gdd_harness::domain::ResourcePlan=serde_json::from_value(finished.artifact("resources").unwrap().clone()).unwrap();
    let names:Vec<&str>=plan.features.iter().map(|item|item.name.as_str()).collect();
    assert!(names.contains(&"甲块资源") && names.contains(&"乙块资源"),"both halves should contribute, got {names:?}");
    // 模型返回 F-999 时同名沿用 F-999；无 ID 资源分配未占用新 ID
    assert_eq!(plan.features[0].id,"F-999");
    assert!(plan.features[1].id.starts_with("F-"));
    assert!(finished.artifact("document").unwrap().get("markdown").unwrap().as_str().unwrap().contains("附录：功能与制作资源拆解"));
}

/// 审核首次被截断时，以压缩指令重试一次并通过。
struct TruncatingReviewer;
#[async_trait]
impl AgentExecutor for TruncatingReviewer {
    async fn run(&self, task: AgentTask, _project: &Project, instruction: String, _feedback: &str) -> gdd_harness::Result<Value> {
        match task {
            AgentTask::Review=>{
                if !instruction.contains("【输出压缩要求】") {
                    return Err(gdd_harness::AppError::OutputTruncated);
                }
                Ok(json!({"approved":true,"findings":[],"summary":"压缩后模拟审核通过"}))
            },
            _=>Ok(json!({"summary":"无"})),
        }
    }
    async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
}

#[tokio::test]
async fn review_retries_with_compression_after_truncation() {
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("test.db")).unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(TruncatingReviewer),config);
    let project=workflow.create(&gdd_harness::domain::ProjectCreate{name:"审核压缩测试".into(),purpose:"验证压缩重试".into(),references:"离线对象".into(),platform:"".into(),audience:"".into()}).unwrap();
    repo.save_artifact(&project.id,"document",json!({"markdown":"# 模拟正文\n\n需要审核的正文。"}),"").unwrap();
    repo.save_artifact(&project.id,"framework",json!({"title":"t","summary":"s","sections":[]}),"").unwrap();
    repo.update(&project.id,Some("review"),None,None,None,None).unwrap();
    workflow.execute(&project.id,"review","").await.unwrap();
    assert_eq!(repo.require(&project.id).unwrap().status,"approved");
    let events=repo.events(&project.id).unwrap();
    assert!(events.iter().any(|e|e["message"].as_str().map(|m|m.contains("压缩重试")).unwrap_or(false)));
}
