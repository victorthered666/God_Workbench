use std::{collections::HashMap, sync::Arc};
use tokio::sync::Semaphore;
use parking_lot::Mutex;
use tokio::task::JoinSet;
use crate::{workflow::Workflow, AppError, Result};

#[derive(Clone)]
pub struct Runner {
    workflow: Workflow,
    running: Arc<Mutex<HashMap<String, tokio::task::AbortHandle>>>,
    jobs: Arc<Mutex<JoinSet<()>>>,
    semaphore: Arc<Semaphore>,
}

impl Runner {
    pub fn new(workflow: Workflow) -> Self { Self { workflow, running:Default::default(), jobs:Default::default(), semaphore:Arc::new(Semaphore::new(2)) } }

    pub fn start(&self, project_id: String, stage: String, feedback: String) -> Result<()> {
        let mut running=self.running.lock();
        if running.contains_key(&project_id) { return Err(AppError::Workflow("该项目已有任务运行".into())); }
        let workflow=self.workflow.clone();
        let semaphore=self.semaphore.clone();
        let running_ref=self.running.clone();
        let id=project_id.clone();
        let handle=self.jobs.lock().spawn(async move {
            let _permit=semaphore.acquire().await;
            if let Err(error)=workflow.execute(&id,&stage,&feedback).await {
                eprintln!("stage failed: {error}");
            }
            running_ref.lock().remove(&id);
        });
        running.insert(project_id,handle);
        Ok(())
    }

    pub fn cancel(&self, project_id: &str) -> Result<()> {
        let handle=self.running.lock().remove(project_id).ok_or_else(||AppError::Invalid("没有运行中的任务".into()))?;
        handle.abort();
        self.workflow.repo.update(project_id,Some("interrupted"),None,Some(Some("任务已取消；已发出的外部请求不会自动重发".into())),None,None)?;
        Ok(())
    }

    pub async fn shutdown(&self) {
        self.jobs.lock().abort_all();
        let mut jobs=self.jobs.lock();
        while jobs.join_next().await.is_some() {}
    }
}
