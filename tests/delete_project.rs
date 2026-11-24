use std::sync::Arc;
use async_trait::async_trait;
use gdd_harness::{agent::runtime::{AgentExecutor, AgentTask}, config::Config, domain::{Annotation, Project, Role}, repository::Repository, workflow::Workflow};
use serde_json::{json, Value};

struct Silent;
#[async_trait]
impl AgentExecutor for Silent {
    async fn run(&self, _task: AgentTask, _project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> { unreachable!() }
    async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
}

#[tokio::test]
async fn delete_removes_rows_keys_and_working_directory() {
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("test.db")).unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(Silent),config);
    let project=workflow.create(&gdd_harness::domain::ProjectCreate{name:"待删除".into(),purpose:"p".into(),references:"r".into(),platform:"".into(),audience:"".into()}).unwrap();
    let root=project.working_dir().unwrap();
    repo.save_artifact(&project.id,"document",json!({"markdown":"# 正文"}),"").unwrap();
    repo.set_setting(&format!("session:{}:writer",project.id),json!("会话内容")).unwrap();
    repo.set_setting(&format!("autorun:{}",project.id),json!(true)).unwrap();
    assert!(root.is_dir());

    workflow.delete_project(&project.id).unwrap();
    assert!(repo.get(&project.id).unwrap().is_none(),"项目行已删除");
    assert!(!root.is_dir(),"工作目录整体删除");
    assert_eq!(repo.setting(&format!("session:{}:writer",project.id),json!(null)).unwrap(),json!(null),"会话散键已清");
    assert_eq!(repo.setting(&format!("autorun:{}",project.id),json!(null)).unwrap(),json!(null),"全自动开关已清");
}

#[tokio::test]
async fn delete_rejects_running_project_and_foreign_directory() {
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("test.db")).unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(Silent),config);
    let project=workflow.create(&gdd_harness::domain::ProjectCreate{name:"运行中".into(),purpose:"p".into(),references:"r".into(),platform:"".into(),audience:"".into()}).unwrap();
    // 1) 运行中不可删
    repo.update(&project.id,Some("running:review"),None,None,None,None).unwrap();
    assert!(workflow.delete_project(&project.id).is_err());
    repo.update(&project.id,Some("review"),None,None,None,None).unwrap();
    // 2) 工作目录被指到输出根之外时拒绝删除，保护数据
    let foreign=dir.path().join("foreign");
    std::fs::create_dir_all(&foreign).unwrap();
    let mut artifacts=repo.require(&project.id).unwrap().artifacts;
    artifacts["working_dir"]=json!(foreign);
    repo.update(&project.id,None,Some(artifacts.clone()),None,None,None).unwrap();
    let error=workflow.delete_project(&project.id).unwrap_err();
    assert!(error.to_string().contains("拒绝删除"),"{error}");
    assert!(foreign.is_dir(),"外部目录原样保留");
    assert!(repo.get(&project.id).unwrap().is_some(),"项目未被误删");
    // 3) 目录不存在（已被手动清理）：仅清数据库，不报错
    artifacts["working_dir"]=json!(dir.path().join("outputs").join("不存在的目录-00000000"));
    repo.update(&project.id,None,Some(artifacts),None,None,None).unwrap();
    workflow.delete_project(&project.id).unwrap();
    assert!(repo.get(&project.id).unwrap().is_none());
}

/// 工作目录形如"输出根的直接子目录且以 id 前 8 位结尾"时才允许删文件。
#[tokio::test]
async fn delete_removes_directory_only_for_matching_name() {
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("test.db")).unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(Silent),config);
    let project=workflow.create(&gdd_harness::domain::ProjectCreate{name:"命名匹配".into(),purpose:"p".into(),references:"r".into(),platform:"".into(),audience:"".into()}).unwrap();
    let root=project.working_dir().unwrap();
    // 改名去掉 id 后缀 → 形态不匹配，拒绝
    let renamed=dir.path().join("outputs").join("被改名");
    std::fs::rename(&root,&renamed).unwrap();
    let mut artifacts=repo.require(&project.id).unwrap().artifacts;
    artifacts["working_dir"]=json!(&renamed);
    repo.update(&project.id,None,Some(artifacts),None,None,None).unwrap();
    assert!(workflow.delete_project(&project.id).is_err());
    assert!(renamed.is_dir());
}
