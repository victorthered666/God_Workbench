use std::{collections::HashMap, sync::Arc};
use parking_lot::Mutex;
use serde_json::json;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use crate::{domain::Project, workflow::Workflow, AppError, Result};

const AUTO_STOP_STATUSES: [&str; 3] = ["approved", "needs_human", "interrupted"];

type Slot=(u64,Option<tokio::task::AbortHandle>);

/// RAII 槽位守卫：任务无论正常结束、出错还是 panic，Drop 都会释放项目槽位；
/// 代数不匹配时说明已被链式后续任务接管，不误删新槽位。
struct SlotGuard {
    running: Arc<Mutex<HashMap<String,Slot>>>,
    generation: Arc<Mutex<u64>>,
    id: String,
    generation_value: u64,
}
impl SlotGuard {
    fn release(&self) {
        let mut map=self.running.lock();
        if let Some((generation,_))=map.get(&self.id) {
            if *generation==self.generation_value { map.remove(&self.id); }
        }
    }
}
impl Drop for SlotGuard {
    fn drop(&mut self) { self.release(); }
}

#[derive(Clone)]
pub struct Runner {
    workflow: Workflow,
    running: Arc<Mutex<HashMap<String,Slot>>>,
    generation: Arc<Mutex<u64>>,
    jobs: Arc<Mutex<JoinSet<()>>>,
    semaphore: Arc<Semaphore>,
}

impl Runner {
    pub fn new(workflow: Workflow) -> Self { Self { workflow, running:Default::default(), generation:Default::default(), jobs:Default::default(), semaphore:Arc::new(Semaphore::new(2)) } }

    pub fn start(&self, project_id: String, stage: String, feedback: String) -> Result<()> {
        let mut running=self.running.lock();
        if running.contains_key(&project_id) { return Err(AppError::Workflow("该项目已有任务运行".into())); }
        let generation_value={ let mut generation=self.generation.lock(); *generation+=1; *generation };
        running.insert(project_id.clone(),(generation_value,None));
        drop(running);
        let workflow=self.workflow.clone();
        let runner=self.clone();
        let running_ref=self.running.clone();
        let generation_ref=self.generation.clone();
        let semaphore=self.semaphore.clone();
        let id=project_id.clone();
        let handle=self.jobs.lock().spawn(async move {
            let guard=SlotGuard { running:running_ref.clone(), generation:generation_ref, id:id.clone(), generation_value };
            let _permit=semaphore.acquire().await;
            let outcome=workflow.execute(&id,&stage,&feedback).await;
            // 释放槽位必须先于链式启动，否则后续 start 会被误判为重复任务。
            guard.release();
            match outcome {
                Ok(project) => {
                    if Runner::autorun_enabled(&workflow.repo,&id) {
                        if let Some(next)=Runner::next_auto_stage(&workflow,&id,&project) {
                            if let Err(error)=runner.start(id.clone(),next.clone(),String::new()) {
                                eprintln!("自动流程未能启动 {next}：{error}");
                            }
                        }
                    }
                },
                Err(error)=>eprintln!("stage failed: {error}"),
            }
        });
        // 仅当槽位代数仍属于本次启动时才回填句柄；若任务已飞快完成并链式
        // 启动了下一代任务，这里不会覆盖新任务的占位槽。
        let mut running=self.running.lock();
        if let Some((generation,slot))=running.get_mut(&project_id) {
            if *generation==generation_value && slot.is_none() { *slot=Some(handle); }
        }
        Ok(())
    }

    pub fn cancel(&self, project_id: &str) -> Result<()> {
        let mut running=self.running.lock();
        let entry=running.get_mut(project_id).ok_or_else(||AppError::Invalid("没有运行中的任务".into()))?;
        match entry.1.take() {
            Some(handle)=>{
                drop(running);
                handle.abort();
                self.workflow.repo.update(project_id,Some("interrupted"),None,Some(Some("任务已取消；已发出的外部请求不会自动重发".into())),None,None)?;
                Ok(())
            },
            None=>Err(AppError::Invalid("任务正在启动，请稍后取消".into())),
        }
    }

    fn autorun_enabled(repo:&crate::repository::Repository, project_id:&str) -> bool {
        repo.setting(&format!("autorun:{project_id}"),json!(false)).ok().and_then(|value|value.as_bool()).unwrap_or(false)
    }

    /// Full-auto chaining: confirm human gates, then keep executing the next
    /// stage until the document is approved or a safety rail stops the flow.
    fn next_auto_stage(workflow:&Workflow, project_id:&str, project:&Project) -> Option<String> {
        if AUTO_STOP_STATUSES.contains(&project.status.as_str()) { return None; }
        if project.artifacts.get("project_type").and_then(serde_json::Value::as_str)==Some("document_review") {
            return if project.status=="audit_ready" { Some("audit".into()) } else { None };
        }
        let gate=match project.status.as_str() {
            "research_review"=>Some("research"),
            "framework_review"=>Some("framework"),
            "revision_review"=>Some("revision_plan"),
            _=>None,
        };
        if let Some(stage)=gate { return workflow.approve(project_id,stage).ok(); }
        match project.status.as_str() {
            "interaction"=>Some("interaction".into()),
            "resources"=>Some("resources".into()),
            "review"=>{
                let review_rejected=project.artifact("review").map(|review|review.get("approved").and_then(serde_json::Value::as_bool)!=Some(true)).unwrap_or(false);
                Some(if review_rejected { "revise".into() } else { "review".into() })
            },
            _=>None,
        }
    }

    pub async fn shutdown(&self) {
        self.jobs.lock().abort_all();
        let mut jobs=self.jobs.lock();
        while jobs.join_next().await.is_some() {}
    }
}
