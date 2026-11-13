use std::{collections::HashMap, sync::Arc};
use parking_lot::Mutex;
use serde_json::json;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use crate::{domain::Project, workflow::Workflow, AppError, Result};

const AUTO_STOP_STATUSES: [&str; 3] = ["approved", "needs_human", "interrupted"];

#[derive(Clone)]
pub struct Runner {
    workflow: Workflow,
    running: Arc<Mutex<HashMap<String, Option<tokio::task::AbortHandle>>>>,
    jobs: Arc<Mutex<JoinSet<()>>>,
    semaphore: Arc<Semaphore>,
}

impl Runner {
    pub fn new(workflow: Workflow) -> Self { Self { workflow, running:Default::default(), jobs:Default::default(), semaphore:Arc::new(Semaphore::new(2)) } }

    pub fn start(&self, project_id: String, stage: String, feedback: String) -> Result<()> {
        let mut running=self.running.lock();
        if running.contains_key(&project_id) { return Err(AppError::Workflow("该项目已有任务运行".into())); }
        // Reserve the slot before spawning so a fast-completing task can never
        // be overwritten by this function's bookkeeping after it finishes.
        running.insert(project_id.clone(),None);
        drop(running);
        let workflow=self.workflow.clone();
        let runner=self.clone();
        let running_ref=self.running.clone();
        let semaphore=self.semaphore.clone();
        let id=project_id.clone();
        let handle=self.jobs.lock().spawn(async move {
            let _permit=semaphore.acquire().await;
            let outcome=workflow.execute(&id,&stage,&feedback).await;
            // Release the project slot before any auto-run chaining.
            running_ref.lock().remove(&id);
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
        // The task may already have completed and chained; only fill our own
        // placeholder if it is still the live entry.
        let mut running=self.running.lock();
        if let Some(slot)=running.get_mut(&project_id) {
            if slot.is_none() { *slot=Some(handle); }
        }
        Ok(())
    }

    pub fn cancel(&self, project_id: &str) -> Result<()> {
        let mut running=self.running.lock();
        let entry=running.remove(project_id).ok_or_else(||AppError::Invalid("没有运行中的任务".into()))?;
        drop(running);
        match entry {
            Some(handle)=>{
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
