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
