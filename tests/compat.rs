use std::sync::Arc;
use async_trait::async_trait;
use gdd_harness::{agent::runtime::AgentExecutor, config::Config, domain::{Annotation, Project, ProjectCreate, Role}, repository::Repository, secrets::SecretStore, workflow::Workflow};
use serde_json::{json, Value};

struct FakeAgents;
#[async_trait]
impl AgentExecutor for FakeAgents {
    async fn run(&self, role: Role, _project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> {
        Ok(match role {
            Role::Research=>json!({"summary":"模拟调研","findings":[{"topic":"循环","observation":"模拟证据","implication":"明确循环","evidence_ids":["a1"],"confidence":"事实"}],"recommendations":["先做循环"],"writing_advice":["明确消耗"],"limitations":[]}),
            Role::Architect=>json!({"title":"测试","summary":"框架","sections":[{"id":"loop","title":"核心循环","purpose":"规则","requirements":["采集"],"visuals":["流程"],"acceptance":["闭环"]}],"boundaries":[]}),
            Role::Writer=>json!({"markdown":"# 测试策划\n\n## 核心循环\n\n玩家采集资源并制作工具。\n\n## 交互\n\n| 操作 | 反馈 |\n| --- | --- |\n| 采集 | 资源入包 |\n\n这是离线模拟正文，不代表真实调研。"}),
            Role::Interaction=>json!({"markdown":"## 交互设计补充\n\n采集不可用时必须说明原因并提示恢复方式（离线模拟补充）。"}),
            Role::Producer=>json!({"features":[{"id":"F-001","name":"采集","specification":"选择资源","source_section":"核心循环","dependencies":"","priority":"P0","acceptance":"资源入包"}],"art":[],"audio":[]}),
        })
    }
    async fn run_writer_patch(&self, _project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> {
        Ok(json!({"patches":[]}))
    }
    async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
}

#[tokio::test]
async fn workflow_requires_human_gates() {
    let root=tempfile::tempdir().unwrap();
    let repo=Repository::new(root.path().join("test.db")).unwrap();
    let _secrets=SecretStore::new(root.path()).unwrap();
    let config=Config::with_dirs(std::env!("CARGO_MANIFEST_DIR"),root.path(),root.path().join("outputs"),8766);
    config.init().unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(FakeAgents),config);
    let project=workflow.create(&ProjectCreate{name:"测试：离线/设计".into(),purpose:"验证工作流".into(),references:"离线对象".into(),platform:"PC".into(),audience:"测试".into()}).unwrap();
    assert!(workflow.execute(&project.id,"document","").await.is_err());
    workflow.execute(&project.id,"research","").await.unwrap();
    assert!(workflow.execute(&project.id,"framework","").await.is_err());
    assert_eq!(workflow.approve(&project.id,"research").unwrap(),"framework");
    workflow.execute(&project.id,"framework","").await.unwrap();
    workflow.approve(&project.id,"framework").unwrap();
    for stage in ["document","interaction","resources"] { workflow.execute(&project.id,stage,"").await.unwrap(); }
    let document=repo.require(&project.id).unwrap().artifact("document").unwrap().get("markdown").unwrap().as_str().unwrap().to_string();
    assert!(document.contains("附录：功能与制作资源拆解"));
    assert!(repo.versions(&project.id,50,0).unwrap().len()>=5);
}

#[test]
fn reads_existing_sqlite_contract() {
    let source=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/gdd.db");
    if !source.exists() { return; }
    let dir=tempfile::tempdir().unwrap();
    let copy=dir.path().join("copy.db");
    std::fs::copy(source,&copy).unwrap();
    let repo=Repository::new(copy).unwrap();
    let projects=repo.list_projects().unwrap();
    assert!(!projects.is_empty());
    let id=projects[0]["id"].as_str().unwrap();
    assert!(repo.require(id).is_ok());
}
