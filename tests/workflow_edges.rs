use std::sync::Arc;
use async_trait::async_trait;
use gdd_harness::{agent::runtime::{AgentExecutor, AgentTask}, config::Config, domain::{Annotation, Project, Role}, repository::Repository, workflow::Workflow};
use serde_json::{json, Value};

struct RejectingAgents;
#[async_trait]
impl AgentExecutor for RejectingAgents {
    async fn run(&self, task: AgentTask, project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> {
        Ok(match task {
            AgentTask::DocumentPatch=>json!({"patches":[{"heading":"模拟正文","markdown":"# 模拟正文

修订后的正文"}]}),
            AgentTask::Review=>json!({"approved":false,"findings":[{"section":"核心循环","problem":"缺少出口","severity":"high","owner":"writer","recommendation":"补充"}],"summary":"不通过"}),
            AgentTask::Framework=>json!({"title":"框架","summary":"框架","sections":[{"id":"loop","title":"核心循环","purpose":"循环","requirements":["采集"],"visuals":[],"acceptance":[]}],"boundaries":[]}),
            AgentTask::Document=>json!({"markdown":"# 离线模拟\n\n正文"}),
            AgentTask::InteractionSupplement=>json!({"markdown":"## 交互设计补充\n\n修订后的交互说明"}),
            AgentTask::RevisionPlan|AgentTask::Research=>json!({"summary":"调研","findings":[],"recommendations":[],"writing_advice":[],"limitations":[]}),
            AgentTask::Resources=>json!({"features":[],"art":[],"audio":[]}),
        })
    }
    async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
}

#[tokio::test]
async fn three_revisions_stop_for_human_review() {
    let dir=tempfile::tempdir().unwrap();
    let repo=Repository::new(dir.path().join("test.db")).unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path(),8766);
    let workflow=Workflow::new(repo.clone(),Arc::new(RejectingAgents),config);
    let project=workflow.create(&gdd_harness::domain::ProjectCreate{name:"测试".into(),purpose:"测试".into(),references:"参考".into(),platform:"".into(),audience:"".into()}).unwrap();
    repo.save_artifact(&project.id,"document",json!({"markdown":"# 模拟正文"}),"").unwrap();
    repo.save_artifact(&project.id,"framework",json!({"title":"测试","summary":"测试","sections":[]}),"").unwrap();
    repo.update(&project.id,Some("review"),None,None,None,None).unwrap();
    for expected in 1..=3 {
        workflow.execute(&project.id,"review","").await.unwrap();
        workflow.execute(&project.id,"revise","").await.unwrap();
        assert_eq!(repo.require(&project.id).unwrap().review_round,expected);
    }
    assert_eq!(repo.require(&project.id).unwrap().status,"needs_human");
    assert!(workflow.execute(&project.id,"revise","").await.is_err());
}
