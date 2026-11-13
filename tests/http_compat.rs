use std::sync::Arc;
use async_trait::async_trait;
use axum::{body::Body, http::{Request, StatusCode}};
use gdd_harness::{agent::runtime::{AgentExecutor, AgentRuntime}, config::Config, domain::{Annotation, Project, ProjectCreate, Role}, http::routes, repository::Repository, runner::Runner, secrets::SecretStore, workflow::Workflow};
use serde_json::{json, Value};
use tower::ServiceExt;

struct FakeAgents;
#[async_trait]
impl AgentExecutor for FakeAgents {
    async fn run(&self, _role: Role, _project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> { unreachable!() }
    async fn run_writer_patch(&self, _project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> { unreachable!() }
    async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
}

#[tokio::test]
async fn bootstrap_preview_and_export_gate_match_frontend_contract() {
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(config.database()).unwrap();
    let secrets=SecretStore::new(&config.data_dir).unwrap();
    let agents=Arc::new(AgentRuntime::new(repo.clone(),secrets.clone(),&config.root));
    let workflow=Workflow::new(repo.clone(),agents.clone(),config.clone());
    let project=workflow.create(&ProjectCreate{name:"测试".into(),purpose:"目的".into(),references:"参考".into(),platform:"".into(),audience:"".into()}).unwrap();
    repo.save_artifact(&project.id,"document",json!({"markdown":"# 测试\n\n| 操作 | 反馈 |\n| --- | --- |\n| 采集 | 入包 |"}),"").unwrap();
    let runner=Runner::new(workflow.clone());
    let app=routes::app(repo.clone(),secrets,workflow,runner,agents,config.root.clone());
    let response=app.clone().oneshot(Request::get("/api/bootstrap").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(),StatusCode::OK);
    let bytes=axum::body::to_bytes(response.into_body(),1_000_000).await.unwrap();
    let value:Value=serde_json::from_slice(&bytes).unwrap();
    assert!(value["roles"]["research"]["api_key_env"].is_string());
    assert!(value["roles"]["research"]["base_url"].is_string());
    let response=app.clone().oneshot(Request::get(format!("/api/projects/{}/preview",project.id)).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(),StatusCode::OK);
    let bytes=axum::body::to_bytes(response.into_body(),1_000_000).await.unwrap();
    let value:Value=serde_json::from_slice(&bytes).unwrap();
    assert!(value["html"].as_str().unwrap().contains("<table>"),"preview should render Markdown tables");
    let response=app.clone().oneshot(Request::get(format!("/api/projects/{}/preview",project.id)).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(),StatusCode::OK);
    let response=app.oneshot(Request::get(format!("/api/projects/{}/export",project.id)).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(),StatusCode::CONFLICT);
}
