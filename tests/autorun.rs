use std::sync::Arc;
use async_trait::async_trait;
use gdd_harness::{agent::runtime::AgentExecutor, config::Config, domain::{Annotation, Project, Role}, repository::Repository, runner::Runner, workflow::Workflow};
use serde_json::{json, Value};
use std::time::Duration;

struct AutoAgents;
#[async_trait]
impl AgentExecutor for AutoAgents {
    async fn run(&self, role: Role, project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> {
        Ok(match role {
            Role::Research=>json!({"summary":"模拟调研","findings":[{"topic":"循环","observation":"模拟证据","implication":"明确循环","evidence_ids":[],"confidence":"事实"}],"recommendations":["先做循环"],"writing_advice":["明确消耗"],"limitations":[]}),
            Role::Architect if project.status=="review" || project.status.starts_with("running:review") || project.status.starts_with("running:revise")=>json!({"approved":true,"findings":[],"summary":"模拟审核通过"}),
            Role::Architect if project.status=="revision_planning" || project.status.starts_with("running:revision_plan")=>json!({"research_needed":false,"reason":"局部修改","affected_sections":["核心循环"],"preserved_sections":[]}),
            Role::Architect=>json!({"title":"测试","summary":"框架","sections":[{"id":"loop","title":"核心循环","purpose":"规则","requirements":["采集"],"visuals":["流程"],"acceptance":["闭环"]}],"boundaries":[]}),
            Role::Writer|Role::Interaction=>json!({"markdown":"# 测试策划\n\n## 核心循环\n\n玩家采集资源。\n\n## 交互\n\n| 操作 | 反馈 |\n| --- | --- |\n| 采集 | 入包 |\n\n离线模拟正文。"}),
            Role::Producer=>json!({"features":[{"id":"F-001","name":"采集","specification":"选择资源","source_section":"核心循环","dependencies":"","priority":"P0","acceptance":"资源入包"}],"art":[],"audio":[]}),
        })
    }
    async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
}

#[tokio::test]
async fn autorun_chain_runs_to_approval_without_human_gates() {
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("test.db")).unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(AutoAgents),config);
    let runner=Runner::new(workflow.clone());
    let project=workflow.create(&gdd_harness::domain::ProjectCreate{name:"全自动测试".into(),purpose:"验证自动链".into(),references:"离线对象".into(),platform:"PC".into(),audience:"测试".into()}).unwrap();
    repo.set_setting(&format!("autorun:{}",project.id),json!(true)).unwrap();
    runner.start(project.id.clone(),"research".into(),String::new()).unwrap();
    let mut approved=false;
    for _ in 0..120 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if repo.require(&project.id).unwrap().status=="approved" { approved=true; break; }
    }
    assert!(approved,"workflow should reach approved automatically, got {}",repo.require(&project.id).unwrap().status);
    let review=repo.require(&project.id).unwrap().artifact("review").unwrap().clone();
    assert_eq!(review.get("approved").and_then(Value::as_bool),Some(true));
    let finished=repo.require(&project.id).unwrap();
    let document=finished.artifact("document").unwrap().get("markdown").unwrap().as_str().unwrap();
    assert!(document.contains("附录：功能与制作资源拆解"));
    assert!(repo.versions(&project.id,50,0).unwrap().len()>=7);
    // Toggle off: no further auto chaining.
    repo.set_setting(&format!("autorun:{}",project.id),json!(false)).unwrap();
}
