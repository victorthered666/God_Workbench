use std::sync::Arc;
use async_trait::async_trait;
use gdd_harness::{agent::runtime::{AgentExecutor, AgentTask}, config::Config, domain::{Annotation, Project, ProjectAuditCreate, Role}, export::package::export, repository::Repository, workflow::Workflow};
use serde_json::{json, Value};

struct AuditAgents;
#[async_trait]
impl AgentExecutor for AuditAgents {
    async fn run(&self, task: AgentTask, _project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> {
        Ok(match task {
            AgentTask::DocumentAudit=>json!({
                "structure":{"design_purpose":"打造快节奏采集制作循环","missing_sections":["设计目的","验收标准"],"comment":"缺少目标陈述与验收章节"},
                "consistency_findings":[{"section":"核心循环","problem":"循环图说明资源唯一消耗点在 S4，但正文 S2 又写招架消耗剑势","evidence":"S2：招架成功 +0.5 格剑势（消耗）","severity":"high","recommendation":"统一 S2 为资源生成点"}],
                "summary":"结构缺两章，存在一处高危资源规则矛盾。"
            }),
            _=>json!({"summary":"无"}),
        })
    }
    async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
}

#[tokio::test]
async fn document_audit_import_report_and_reaudit() {
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("test.db")).unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(AuditAgents),config);
    let project=workflow.create_audit(&ProjectAuditCreate{
        name:"外部文档审核".into(),
        markdown:"# 玩法方案\n\n## 核心循环\n\n采集并制作。".into(),
        platform:"PC".into(),audience:String::new(),
    }).unwrap();
    let first=repo.require(&project.id).unwrap();
    assert_eq!(first.status,"audit_ready");
    assert_eq!(first.artifacts.get("project_type").and_then(Value::as_str),Some("document_review"));

    workflow.execute(&project.id,"audit","").await.unwrap();
    let done=repo.require(&project.id).unwrap();
    assert_eq!(done.status,"audit_done");
    assert!(done.artifact("audit_report").is_some());
    let report_path=done.working_dir().unwrap().join("audit_report.md");
    let report_text=std::fs::read_to_string(&report_path).unwrap();
    assert!(report_text.contains("结构完整性"));
    assert!(report_text.contains("验收标准"));
    assert!(report_text.contains("资源规则矛盾"));
    // 原文档保持导入版本，不被报告覆盖
    assert!(std::fs::read_to_string(done.working_dir().unwrap().join("document.md")).unwrap().contains("采集并制作"));

    // 手动修改导入文档 → 回到待审核
    workflow.manual_document(&project.id,"# 修改后文档\n\n补充设计目的。").unwrap();
    assert_eq!(repo.require(&project.id).unwrap().status,"audit_ready");
    // 重新审核
    workflow.execute(&project.id,"audit","").await.unwrap();
    assert_eq!(repo.require(&project.id).unwrap().status,"audit_done");

    // 导出报告
    let exported=export(&repo,&repo.require(&project.id).unwrap()).unwrap();
    let md=std::fs::read_to_string(exported["markdown"].as_str().unwrap()).unwrap();
    assert!(md.contains("文档审核报告"));
    assert!(exported["docx"].as_str().unwrap().ends_with(".docx"));
    assert!(std::path::Path::new(exported["zip"].as_str().unwrap()).is_file());
}

/// 审核通过必须落上 review 产物的 approved 标记，否则导出门禁永远拒绝。
#[tokio::test]
async fn approved_review_sets_export_gate_flag() {
    use async_trait::async_trait;
    use gdd_harness::agent::runtime::AgentTask;
    struct ReviewPass;
    #[async_trait]
    impl AgentExecutor for ReviewPass {
        async fn run(&self, task: AgentTask, _project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> {
            Ok(match task { AgentTask::Review=>json!({"approved":true,"findings":[],"summary":"通过"}), _=>json!({"summary":"无"}) })
        }
        async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
    }
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("gate.db")).unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(ReviewPass),config);
    let project=workflow.create(&gdd_harness::domain::ProjectCreate{name:"导出门禁".into(),purpose:"p".into(),references:"r".into(),platform:String::new(),audience:String::new()}).unwrap();
    repo.save_artifact(&project.id,"document",json!({"markdown":"# d"}),"").unwrap();
    repo.save_artifact(&project.id,"framework",json!({"title":"t","summary":"s","sections":[]}),"").unwrap();
    repo.update(&project.id,Some("review"),None,None,None,None).unwrap();
    workflow.execute(&project.id,"review","").await.unwrap();
    let done=repo.require(&project.id).unwrap();
    assert_eq!(done.status,"approved");
    assert!(done.approved("review"),"review artifact gate flag must be true after approval");
}

/// 项目类型门禁：审核文档项目不可运行五角色阶段，普通项目不可运行 audit。
#[tokio::test]
async fn project_type_gates_stages() {
    struct NoCall;
    #[async_trait]
    impl AgentExecutor for NoCall {
        async fn run(&self, _task: AgentTask, _project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> { unreachable!("should be gated before any model call") }
        async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
    }
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("gate2.db")).unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(NoCall),config.clone());
    let audit_project=workflow.create_audit(&ProjectAuditCreate{name:"类型门禁".into(),markdown:"# d".into(),platform:String::new(),audience:String::new()}).unwrap();
    let err=workflow.execute(&audit_project.id,"resources","").await.unwrap_err();
    assert!(err.to_string().contains("只能执行结构审核"),"{err}");
    let err=workflow.execute(&audit_project.id,"research","").await.unwrap_err();
    assert!(err.to_string().contains("只能执行结构审核"),"{err}");
    let normal=workflow.create(&gdd_harness::domain::ProjectCreate{name:"普通".into(),purpose:"p".into(),references:"r".into(),platform:String::new(),audience:String::new()}).unwrap();
    let err=workflow.execute(&normal.id,"audit","").await.unwrap_err();
    assert!(err.to_string().contains("仅适用于审核文档项目"),"{err}");
}
