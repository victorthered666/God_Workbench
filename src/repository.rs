use std::{path::{Path, PathBuf}, sync::Arc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use crate::{domain::{id, now, Project, ProjectCreate}, AppError, Result};

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Clone)]
pub struct Repository {
    connection: Arc<Mutex<Connection>>,
}

impl Repository {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() { std::fs::create_dir_all(parent)?; }
        let connection = Connection::open(path)?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
        connection.execute_batch(SCHEMA)?;
        let repo = Self { connection: Arc::new(Mutex::new(connection)) };
        repo.recover_interrupted()?;
        Ok(repo)
    }

    pub fn memory() -> Result<Self> { Self::new(":memory:") }

    fn map_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
        Ok(Project {
            id: row.get(0)?, name: row.get(1)?, purpose: row.get(2)?, reference_text: row.get(3)?,
            platform: row.get(4)?, audience: row.get(5)?, status: row.get(6)?,
            revision: row.get(7)?, review_round: row.get(8)?, artifacts: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_else(|_| json!({})),
            error: row.get(10)?, created_at: row.get(11)?, updated_at: row.get(12)?,
        })
    }

    pub fn create_project(&self, data: &ProjectCreate) -> Result<Project> {
        let timestamp = now();
        let project = Project { id: id(), name: data.name.clone(), purpose: data.purpose.clone(), reference_text: data.references.clone(), platform: data.platform.clone(), audience: data.audience.clone(), status: "research_review".into(), revision: 1, review_round: 0, artifacts: json!({}), error: None, created_at: timestamp.clone(), updated_at: timestamp };
        self.connection.lock().execute("INSERT INTO projects VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,NULL,?11,?12)", params![project.id, project.name, project.purpose, project.reference_text, project.platform, project.audience, project.status, project.revision, project.review_round, "{}", project.created_at, project.updated_at])?;
        Ok(project)
    }

    pub fn get(&self, project_id: &str) -> Result<Option<Project>> {
        let result = self.connection.lock().query_row("SELECT id,name,purpose,reference_text,platform,audience,status,revision,review_round,artifacts,error,created_at,updated_at FROM projects WHERE id=?1", [project_id], Self::map_project).optional()?;
        Ok(result)
    }

    pub fn require(&self, project_id: &str) -> Result<Project> { self.get(project_id)?.ok_or_else(|| AppError::Workflow("项目不存在".into())) }

    pub fn list_projects(&self) -> Result<Vec<Value>> {
        let db = self.connection.lock();
        let mut statement = db.prepare("SELECT id,name,purpose,reference_text,platform,audience,status,revision,review_round,error,created_at,updated_at FROM projects ORDER BY updated_at DESC LIMIT 200")?;
        let mut rows = statement.query([])?;
        let mut projects = vec![];
        while let Some(row) = rows.next()? {
            let project_id: String = row.get(0)?;
            let review: Option<String> = db.query_row("SELECT content FROM versions WHERE project_id=?1 AND stage='review' ORDER BY created_at DESC LIMIT 1", [&project_id], |row| row.get(0)).optional()?;
            projects.push(json!({"id":project_id,"name":row.get::<_,String>(1)?,"purpose":row.get::<_,String>(2)?,"reference_text":row.get::<_,String>(3)?,"platform":row.get::<_,String>(4)?,"audience":row.get::<_,String>(5)?,"status":row.get::<_,String>(6)?,"revision":row.get::<_,u32>(7)?,"review_round":row.get::<_,u32>(8)?,"error":row.get::<_,Option<String>>(9)?,"created_at":row.get::<_,String>(10)?,"updated_at":row.get::<_,String>(11)?,"artifacts":{"review":review.map(|text|json!({"content":serde_json::from_str::<Value>(&text).unwrap_or(json!({}))})).unwrap_or(json!({}))}}));
        }
        Ok(projects)
    }

    pub fn update(&self, project_id: &str, status: Option<&str>, artifacts: Option<Value>, error: Option<Option<String>>, revision: Option<u32>, review_round: Option<u32>) -> Result<()> {
        let project = self.require(project_id)?;
        self.connection.lock().execute("UPDATE projects SET status=?2,artifacts=?3,error=?4,revision=?5,review_round=?6,updated_at=?7 WHERE id=?1", params![project_id, status.unwrap_or(&project.status), artifacts.map(|v|v.to_string()).unwrap_or_else(||project.artifacts.to_string()), error.unwrap_or(project.error), revision.unwrap_or(project.revision), review_round.unwrap_or(project.review_round), now()])?;
        Ok(())
    }

    pub fn save_artifact(&self, project_id: &str, stage: &str, content: Value, feedback: &str) -> Result<u32> {
        let mut db = self.connection.lock();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut artifacts: Value = tx.query_row("SELECT artifacts FROM projects WHERE id=?1", [project_id], |row| row.get(0)).map(|text:String|serde_json::from_str(&text))??;
        let version = artifacts.get(stage).and_then(|v|v.get("version")).and_then(Value::as_u64).unwrap_or(0) as u32 + 1;
        artifacts[stage] = json!({"version":version,"content":content,"approved":false,"feedback":feedback,"created_at":now()});
        tx.execute("INSERT INTO versions VALUES (?1,?2,?3,?4,?5,?6,?7)", params![id(),project_id,stage,version,content.to_string(),feedback,now()])?;
        tx.execute("UPDATE projects SET artifacts=?2,updated_at=?3 WHERE id=?1", params![project_id,artifacts.to_string(),now()])?;
        tx.commit()?;
        Ok(version)
    }

    pub fn approve(&self, project_id: &str, stage: &str) -> Result<()> {
        let project = self.require(project_id)?;
        if project.artifacts.get(stage).is_none() { return Err(AppError::Invalid("当前阶段尚无产物".into())); }
        let mut artifacts = project.artifacts;
        artifacts[stage]["approved"] = json!(true);
        self.update(project_id, None, Some(artifacts), Some(None), None, None)
    }

    pub fn versions(&self, project_id: &str, limit: usize, offset: usize) -> Result<Vec<Value>> {
        let db = self.connection.lock();
        let mut statement = db.prepare("SELECT * FROM versions WHERE project_id=?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3")?;
        let mut rows = statement.query(params![project_id,limit,offset])?;
        let mut result = vec![];
        while let Some(row) = rows.next()? {
            result.push(json!({"id":row.get::<_,String>(0)?,"project_id":row.get::<_,String>(1)?,"stage":row.get::<_,String>(2)?,"version":row.get::<_,u32>(3)?,"content":serde_json::from_str::<Value>(&row.get::<_,String>(4)?)?,"feedback":row.get::<_,String>(5)?,"created_at":row.get::<_,String>(6)?}));
        }
        result.reverse();
        Ok(result)
    }

    pub fn create_run(&self, project_id: &str, stage: &str, snapshot: Value) -> Result<String> {
        let run_id = id();
        self.connection.lock().execute("INSERT INTO runs VALUES (?1,?2,?3,'running',?4,NULL,?5,?6)", params![run_id,project_id,stage,snapshot.to_string(),now(),now()])?;
        Ok(run_id)
    }

    pub fn finish_run(&self, run_id: &str, error: Option<&str>) -> Result<()> {
        self.connection.lock().execute("UPDATE runs SET status=?2,error=?3,updated_at=?4 WHERE id=?1", params![run_id,if error.is_some(){"failed"}else{"completed"},error,now()])?;
        Ok(())
    }

    fn recover_interrupted(&self) -> Result<()> {
        let db = self.connection.lock();
        db.execute("UPDATE runs SET status='interrupted',error='服务重启；请检查日志后恢复，未自动重发外部请求。',updated_at=?1 WHERE status='running'", [now()])?;
        db.execute("UPDATE projects SET status='interrupted',error='任务被中断；可从当前阶段恢复。',updated_at=?1 WHERE status LIKE 'running:%'", [now()])?;
        Ok(())
    }

    pub fn event(&self, project_id: &str, kind: &str, message: &str) -> Result<()> {
        self.connection.lock().execute("INSERT INTO events(project_id,kind,message,created_at) VALUES (?1,?2,?3,?4)", params![project_id,kind,message,now()])?;
        Ok(())
    }

    pub fn events(&self, project_id: &str) -> Result<Vec<Value>> {
        let db = self.connection.lock();
        let mut statement = db.prepare("SELECT * FROM events WHERE project_id=?1 ORDER BY id DESC LIMIT 200")?;
        let mut rows = statement.query([project_id])?;
        let mut result = vec![];
        while let Some(row)=rows.next()? { result.push(json!({"id":row.get::<_,i64>(0)?,"project_id":row.get::<_,String>(1)?,"kind":row.get::<_,String>(2)?,"message":row.get::<_,String>(3)?,"created_at":row.get::<_,String>(4)?})); }
        Ok(result)
    }

    pub fn setting(&self, key: &str, default: Value) -> Result<Value> {
        let result = self.connection.lock().query_row("SELECT value FROM settings WHERE key=?1",[key],|row|row.get::<_,String>(0)).optional()?;
        result.map(|text|serde_json::from_str(&text).map_err(Into::into)).unwrap_or(Ok(default))
    }

    pub fn set_setting(&self, key: &str, value: Value) -> Result<()> {
        self.connection.lock().execute("INSERT INTO settings VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key,value.to_string()])?;
        Ok(())
    }

    pub fn evidence(&self, project_id: &str, query: &str, payload: Value) -> Result<()> {
        self.connection.lock().execute("INSERT INTO evidence VALUES (?1,?2,?3,?4,?5)",params![id(),project_id,query,payload.to_string(),now()])?;
        Ok(())
    }

    pub fn evidence_list(&self, project_id: &str, limit: usize) -> Result<Vec<Value>> {
        let db=self.connection.lock();
        let mut statement=db.prepare("SELECT * FROM evidence WHERE project_id=?1 ORDER BY created_at DESC LIMIT ?2")?;
        let mut rows=statement.query(params![project_id,limit])?;
        let mut result=vec![];
        while let Some(row)=rows.next()? { result.push(json!({"id":row.get::<_,String>(0)?,"project_id":row.get::<_,String>(1)?,"query":row.get::<_,String>(2)?,"payload":serde_json::from_str::<Value>(&row.get::<_,String>(3)?)?,"created_at":row.get::<_,String>(4)?})); }
        Ok(result)
    }

    pub fn asset(&self, project_id: &str, source_url: &str, source_page: &str, local_path: &str, sha256: &str, metadata: Value) -> Result<()> {
        self.connection.lock().execute("INSERT INTO assets VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",params![id(),project_id,source_url,source_page,local_path,sha256,metadata.to_string(),now()])?;
        Ok(())
    }

    pub fn assets(&self, project_id: &str) -> Result<Vec<Value>> {
        let db=self.connection.lock();
        let mut statement=db.prepare("SELECT * FROM assets WHERE project_id=?1 ORDER BY created_at")?;
        let mut rows=statement.query([project_id])?;
        let mut result=vec![];
        while let Some(row)=rows.next()? { result.push(json!({"id":row.get::<_,String>(0)?,"project_id":row.get::<_,String>(1)?,"source_url":row.get::<_,String>(2)?,"source_page":row.get::<_,String>(3)?,"local_path":row.get::<_,String>(4)?,"sha256":row.get::<_,String>(5)?,"metadata":serde_json::from_str::<Value>(&row.get::<_,String>(6)?)?,"created_at":row.get::<_,String>(7)?})); }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_projects_versions_and_gates() {
        let repo=Repository::memory().unwrap();
        let project=repo.create_project(&ProjectCreate{name:"测试".into(),purpose:"目的".into(),references:"参考".into(),platform:"PC".into(),audience:"用户".into()}).unwrap();
        assert_eq!(repo.list_projects().unwrap().len(),1);
        repo.save_artifact(&project.id,"research",json!({"summary":"结论"}),"").unwrap();
        assert_eq!(repo.save_artifact(&project.id,"research",json!({"summary":"修订"}),"").unwrap(),2);
        assert_eq!(repo.versions(&project.id,50,0).unwrap().len(),2);
        repo.approve(&project.id,"research").unwrap();
        assert!(repo.require(&project.id).unwrap().approved("research"));
    }
}
