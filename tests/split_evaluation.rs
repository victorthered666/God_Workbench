use std::sync::Arc;
use async_trait::async_trait;
use gdd_harness::{agent::runtime::{AgentExecutor, AgentTask}, config::Config, domain::{Annotation, Project, Role}, repository::Repository, workflow::Workflow};
use serde_json::{json, Value};

/// 框架章节可配置预估页数的模拟 Agent。
struct SizedFramework { pages: f32 }
#[async_trait]
impl AgentExecutor for SizedFramework {
    async fn run(&self, task: AgentTask, _project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> {
        Ok(match task {
            AgentTask::Framework=>json!({
                "title":"大文档","summary":"含 10 章的框架",
                "sections":(1..=10).map(|i|json!({
                    "id":format!("sec{i}"),"title":format!("第{i}章 系统规则"),"purpose":"规则",
                    "requirements":[format!("要求甲{i}"),format!("要求乙{i}")],
                    "visuals":[],"acceptance":[format!("验收{i}")],
                    "estimated_pages":self.pages
                })).collect::<Vec<_>>()
            }),
            AgentTask::Research|AgentTask::DocumentAudit|AgentTask::RevisionPlan|AgentTask::Review|AgentTask::Document|AgentTask::DocumentPatch|AgentTask::InteractionSupplement=>json!({"summary":"无"}),
            AgentTask::Resources=>json!({"features":[],"art":[],"audio":[]}),
        })
    }
    async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
}

async fn setup(repo:&Repository,workflow:&Workflow) -> String {
    let project=workflow.create(&gdd_harness::domain::ProjectCreate{name:"拆分测试".into(),purpose:"目的".into(),references:"参考".into(),platform:"".into(),audience:"".into()}).unwrap();
    workflow.execute(&project.id,"research","").await.unwrap();
    workflow.approve(&project.id,"research").unwrap();
    let _=repo;
    project.id
}

/// 零条目章节的极简框架（密度下限 = 2 页/章）。
struct LeanFramework;
#[async_trait]
impl AgentExecutor for LeanFramework {
    async fn run(&self, task: AgentTask, _project: &Project, _instruction: String, _feedback: &str) -> gdd_harness::Result<Value> {
        Ok(match task {
            AgentTask::Framework=>json!({
                "title":"小文档","summary":"含 10 章的轻量框架",
                "sections":(1..=10).map(|i|json!({
                    "id":format!("sec{i}"),"title":format!("第{i}章"),"purpose":"规则",
                    "requirements":[],"visuals":[],"acceptance":[],"estimated_pages":2.0
                })).collect::<Vec<_>>()
            }),
            AgentTask::Research=>json!({"summary":"无"}),
            _=>json!({"summary":"无"}),
        })
    }
    async fn annotate(&self, _project: &Project, _path: &str, _title: &str) -> gdd_harness::Result<Vec<Annotation>> { Ok(vec![]) }
}

#[tokio::test]
async fn oversized_framework_produces_balanced_split_plan() {
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("test.db")).unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(SizedFramework{pages:9.0}),config);
    let id=setup(&repo,&workflow).await;
    workflow.execute(&id,"framework","").await.unwrap();
    let project=repo.require(&id).unwrap();
    assert_eq!(project.status,"framework_review");
    let plan:serde_json::Value=project.artifact("split_plan").unwrap().clone();
    assert_eq!(plan["recommended"],json!(true),"10 章 × 9 页 = 90 页必须触发拆分建议");
    assert_eq!(plan["estimated_total_pages"],json!(90.0));
    let parts=plan["parts"].as_array().unwrap();
    assert!(parts.len()>=2 && parts.len()<=6,"建议卷数在 2–6：{}",parts.len());
    // 每卷页数尽量均衡且接近上限但不超过太多
    let pages:Vec<f32>=parts.iter().map(|p|p["estimated_pages"].as_f64().unwrap() as f32).collect();
    let spread=pages.iter().fold(f32::MIN,|a,&b|a.max(b))-pages.iter().fold(f32::MAX,|a,&b|a.min(b));
    assert!(spread<=30.0,"各卷页数差不应过大：{pages:?}");
    assert_eq!(pages.iter().map(|p|*p as i32).sum::<i32>(),90,"分卷页数之和等于总页数");
    // 每卷的章节 id 拼起来覆盖全部章节
    let mut ids:Vec<String>=parts.iter().flat_map(|p|p["section_ids"].as_array().unwrap().iter().map(|v|v.as_str().unwrap().to_string())).collect();
    ids.sort();
    let mut expected:Vec<String>=(1..=10).map(|i|format!("sec{i}")).collect();
    expected.sort();
    assert_eq!(ids,expected,"分卷必须覆盖全部章节且不重复");
    assert!(repo.events(&id).unwrap().iter().any(|e|e["message"].as_str().map(|m|m.contains("拆分评估")).unwrap_or(false)));
}

#[tokio::test]
async fn undersized_framework_advises_single_document() {
    // 章节无条目时密度下限 = 2.0，模型估 2.0 → max 相同，10 章 20 页不触发拆分。
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("test.db")).unwrap();
    let lean=Arc::new(LeanFramework);
    let workflow=Workflow::new(repo.clone(),lean,config);
    let id=setup(&repo,&workflow).await;
    workflow.execute(&id,"framework","").await.unwrap();
    let plan:serde_json::Value=repo.require(&id).unwrap().artifact("split_plan").unwrap().clone();
    assert_eq!(plan["recommended"],json!(false));
    assert_eq!(plan["parts"].as_array().unwrap().len(),0);
}

/// 模型把页数写成 "8-10页" 字符串也能宽容解析出首个数字。
#[tokio::test]
async fn string_page_estimates_are_parsed_leniently() {
    let plan=gdd_harness::domain::SplitPlan::evaluate(&[
        gdd_harness::domain::FrameworkSection{id:"a".into(),title:"甲".into(),purpose:"p".into(),requirements:vec![],visuals:vec![],acceptance:vec![],estimated_pages:8.0},
        gdd_harness::domain::FrameworkSection{id:"b".into(),title:"乙".into(),purpose:"p".into(),requirements:vec![],visuals:vec![],acceptance:vec![],estimated_pages:28.0},
    ]);
    assert_eq!(plan.estimated_total_pages,36.0);
    assert!(plan.recommended);
    let parsed:Result<gdd_harness::domain::FrameworkSection,_>=serde_json::from_value(json!({"id":"c","title":"丙","purpose":"p","requirements":[],"estimated_pages":"5-6页"}));
    assert_eq!(parsed.unwrap().estimated_pages,5.0);
}

/// 大密度长文档（191 页，17 章）：装箱后任何一卷不得显著超过 30 页上限，
/// 每卷至少一章，所有章节恰好覆盖一次。
#[tokio::test]
async fn dense_document_packing_avoids_oversized_final_part() {
    let sections:Vec<gdd_harness::domain::FrameworkSection>=(0..17).map(|i|gdd_harness::domain::FrameworkSection{
        id:format!("s{i:02}"),title:format!("第{i}章 系统规则密度"),purpose:"p".into(),
        requirements:(0..6).map(|j|format!("要求{i}-{j}")).collect(),
        visuals:vec![],acceptance:(0..5).map(|j|format!("验收{i}-{j}")).collect(),
        estimated_pages:11.0,
    }).collect();
    let plan=gdd_harness::domain::SplitPlan::evaluate(&sections);
    assert!(plan.recommended);
    let pages:Vec<f32>=plan.parts.iter().map(|p|p.estimated_pages).collect();
    let total: f32 =pages.iter().sum();
    assert!((total-187.0).abs()<0.5,"分卷页数之和守恒：{pages:?}");
    for page in &pages { assert!(page<=&41.0,"单卷不超过上限+单章余量：{pages:?}"); }
    let mut ids:Vec<String>=plan.parts.iter().flat_map(|p|p.section_ids.clone()).collect();
    ids.sort();
    let mut expected:Vec<String>=(0..17).map(|i|format!("s{i:02}")).collect();
    expected.sort();
    assert_eq!(ids,expected,"覆盖全部章节且不重复");
    assert!(plan.parts.iter().all(|p|!p.section_ids.is_empty()));
}

/// 一键按建议创建分卷子项目：名称、目的、父项目记录与事件齐全。
#[tokio::test]
async fn split_project_creates_sub_projects() {
    let dir=tempfile::tempdir().unwrap();
    let config=Config::with_dirs(env!("CARGO_MANIFEST_DIR"),dir.path(),dir.path().join("outputs"),8766);
    config.init().unwrap();
    let repo=Repository::new(dir.path().join("test.db")).unwrap();
    let workflow=Workflow::new(repo.clone(),Arc::new(SizedFramework{pages:9.0}),config);
    let id=setup(&repo,&workflow).await;
    workflow.execute(&id,"framework","").await.unwrap();
    let created=workflow.split_project(&id).unwrap();
    assert!(created.len()>=2);
    for project in &created {
        assert!(project.name.contains("拆分测试"),"子项目名继承父名：{}",project.name);
        assert!(project.name.contains("卷"),"子项目名含卷号：{}",project.name);
        assert!(project.purpose.contains("拆分撰写卷") && project.purpose.contains("【"),"目的含本卷章节清单");
        assert!(project.purpose.contains("要求："),"目的携带框架要求条目");
    }
    let parent=repo.require(&id).unwrap();
    assert_eq!(parent.artifacts["split_executed"].as_array().unwrap().len(),created.len());
    assert!(repo.events(&id).unwrap().iter().any(|e|e["message"].as_str().map(|m|m.contains("分卷项目")).unwrap_or(false)));
    // 知悉接口落标记
    workflow.acknowledge_split(&id).unwrap();
    assert_eq!(repo.require(&id).unwrap().artifacts["split_acknowledged"],json!(true));
}
