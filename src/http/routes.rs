use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use axum::{body::Body, extract::{Path, Query, State}, http::{header, StatusCode}, response::{Html, IntoResponse, Response}, routing::{get, post, put}, Json, Router};
use serde_json::{json, Value};
use tower_http::services::ServeDir;
use crate::{agent::runtime::{AgentExecutor, AgentRuntime}, domain::{AgentSettings, GateRequest, ProjectCreate, RevisionRequest, Role}, paths::{contained_file, safe_name}, repository::Repository, runner::Runner, secrets::SecretStore, workflow::Workflow, AppError, Result};

#[derive(Clone)]
pub struct AppState {
    pub repo: Repository,
    pub secrets: SecretStore,
    pub workflow: Workflow,
    pub runner: Runner,
    pub agents: Arc<AgentRuntime>,
    pub csrf_token: Arc<str>,
}

pub fn app(repo: Repository, secrets: SecretStore, workflow: Workflow, runner: Runner, agents: Arc<AgentRuntime>, root: PathBuf) -> Router {
    let csrf_token:Arc<str>=crate::domain::id().into();
    let state=AppState { repo,secrets,workflow,runner,agents,csrf_token };
    Router::new()
        .route("/",get(index))
        .route("/api/bootstrap",get(bootstrap))
        .route("/api/health",get(health))
        .route("/api/projects",post(create))
        .route("/api/projects/audit",post(create_audit))
        .route("/api/projects/{id}",get(project))
        .route("/api/projects/{id}/stages/{stage}/feedback",post(feedback))
        .route("/api/projects/{id}/stages/{stage}/approve",post(approve))
        .route("/api/projects/{id}/run/{stage}",post(run))
        .route("/api/projects/{id}/cancel",post(cancel))
        .route("/api/projects/{id}/revision",post(revision))
        .route("/api/projects/{id}/autorun",put(autorun))
        .route("/api/projects/{id}/review/acknowledge",post(acknowledge_review))
        .route("/api/projects/{id}/document",put(edit_document))
        .route("/api/projects/{id}/preview",get(preview))
        .route("/api/projects/{id}/asset",get(asset))
        .route("/api/projects/{id}/export",get(export))
        .route("/api/projects/{id}/download/{kind}",get(download))
        .route("/api/settings",get(settings))
        .route("/api/settings/{role}",put(save_settings))
        .route("/api/settings/search/key",put(save_search_key))
        .nest_service("/static",ServeDir::new(root.join("web")))
        .layer(axum::middleware::from_fn_with_state(state.clone(),super::middleware::local_write_guard))
        .with_state(state)
}

async fn index() -> Result<Html<String>> { Ok(Html(std::fs::read_to_string(crate::config::Config::from_env()?.root.join("web/index.html"))?)) }
async fn health() -> Json<Value> { Json(json!({"status":"ok"})) }

fn role_settings(state: &AppState) -> Result<Value> {
    let mut result=json!({});
    for role in [Role::Research,Role::Architect,Role::Writer,Role::Interaction,Role::Producer] {
        let mut config=state.agents.settings(role)?;
        let key_configured=state.secrets.configured(&config.api_key_env);
        config.api_key=None;
        let mut value=serde_json::to_value(config)?;
        value["key_configured"]=json!(key_configured);
        result[role.as_str()]=value;
    }
    Ok(result)
}

async fn bootstrap(State(state): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(json!({"csrf_token":state.csrf_token.as_ref(),"projects":state.repo.list_projects()?,"roles":role_settings(&state)?,"search_configured":state.secrets.configured("WEB_SEARCH_API_KEY")})))
}

async fn project(State(state): State<AppState>, Path(id): Path<String>, Query(query): Query<BTreeMap<String,usize>>) -> Result<Json<Value>> {
    let project=state.repo.require(&id)?;
    Ok(Json(json!({"project":project,"autorun":state.repo.setting(&format!("autorun:{id}"),serde_json::json!(false))?,"events":state.repo.events(&id)?,"versions":state.repo.versions(&id,query.get("version_limit").copied().unwrap_or(50).clamp(1,100),query.get("version_offset").copied().unwrap_or(0))?,"evidence":state.repo.evidence_list(&id,query.get("evidence_limit").copied().unwrap_or(100).clamp(1,200))?,"assets":state.repo.assets(&id)?})))
}

async fn create(State(state): State<AppState>, Json(request): Json<ProjectCreate>) -> Result<(StatusCode,Json<Value>)> {
    if request.name.trim().is_empty() || request.purpose.trim().is_empty() || request.references.trim().is_empty() || request.name.chars().count()>120 || request.purpose.len()>12000 || request.references.len()>12000 { return Err(AppError::Invalid("文档名称、设计目的和参考对象为必填项，且不能超过长度限制".into())); }
    let project=state.workflow.create(&request)?;
    state.runner.start(project.id.clone(),"research".into(),String::new())?;
    Ok((StatusCode::CREATED,Json(json!({"project":state.repo.require(&project.id)?}))))
}

async fn create_audit(State(state): State<AppState>, Json(request): Json<crate::domain::ProjectAuditCreate>) -> Result<(StatusCode,Json<Value>)> {
    if request.name.trim().is_empty() || request.name.chars().count()>120 { return Err(AppError::Invalid("文档名称必填且不超过 120 字".into())); }
    if request.markdown.trim().is_empty() { return Err(AppError::Invalid("导入的文档内容不能为空".into())); }
    if request.markdown.len()>2*1024*1024 { return Err(AppError::Invalid("导入文档超过 2 MiB 限制".into())); }
    let project=state.workflow.create_audit(&request)?;
    state.runner.start(project.id.clone(),"audit".into(),String::new())?;
    Ok((StatusCode::CREATED,Json(json!({"project":state.repo.require(&project.id)?}))))
}

async fn feedback(State(state): State<AppState>, Path((id,stage)): Path<(String,String)>, Json(request): Json<GateRequest>) -> Result<Json<Value>> {
    if request.feedback.trim().is_empty() && request.content.is_none() { return Err(AppError::Invalid("请填写反馈或修改内容".into())); }
    Ok(Json(json!({"project":state.workflow.feedback(&id,&stage,&request).await?})))
}

async fn approve(State(state): State<AppState>, Path((id,stage)): Path<(String,String)>) -> Result<Json<Value>> {
    let next=state.workflow.approve(&id,&stage)?;
    state.runner.start(id.clone(),next.clone(),String::new())?;
    Ok(Json(json!({"project":state.repo.require(&id)?,"next_stage":next})))
}

async fn run(State(state): State<AppState>, Path((id,stage)): Path<(String,String)>) -> Result<Json<Value>> {
    state.runner.start(id,stage,String::new())?;
    Ok(Json(json!({"status":"started"})))
}

async fn cancel(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    state.runner.cancel(&id)?;
    Ok(Json(json!({"status":"cancel_requested"})))
}

async fn autorun(State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>) -> Result<Json<Value>> {
    state.repo.require(&id)?;
    let enabled=body.get("enabled").and_then(Value::as_bool).ok_or_else(||AppError::Invalid("enabled 必须为布尔值".into()))?;
    state.repo.set_setting(&format!("autorun:{id}"),json!(enabled))?;
    Ok(Json(json!({"status":"saved","autorun":enabled})))
}

async fn acknowledge_review(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    let project=state.repo.require(&id)?;
    if project.running() { return Err(AppError::Workflow("任务运行时不能确认".into())); }
    let review:crate::domain::Review=serde_json::from_value(project.artifact("review").cloned().ok_or_else(||AppError::Workflow("尚无审核产物".into()))?)?;
    if review.approved { return Err(AppError::Workflow("审核已通过".into())); }
    if !review.findings.iter().any(|f|matches!(f.owner,Role::Research|Role::Architect)) {
        return Err(AppError::Workflow("当前没有需要人工知悉的架构类问题".into()));
    }
    let mut artifacts=project.artifacts.clone();
    let deferred:Vec<Value>=review.findings.iter()
        .filter(|f|matches!(f.owner,Role::Research|Role::Architect))
        .map(|f|serde_json::to_value(f).unwrap_or(json!({}))).collect();
    artifacts["review_acknowledged"]=json!(true);
    artifacts["deferred_findings"]=json!(deferred);
    state.repo.update(&id,Some("review"),Some(artifacts),Some(None),None,None)?;
    state.repo.event(&id,"gate","用户人工知悉架构类审核问题；继续修订其余问题")?;
    state.runner.start(id,"revise".into(),String::new())?;
    Ok(Json(json!({"status":"acknowledged"})))
}

async fn revision(State(state): State<AppState>, Path(id): Path<String>, Json(request): Json<RevisionRequest>) -> Result<Json<Value>> {
    if request.purpose.trim().is_empty() { return Err(AppError::Invalid("修改目的不能为空".into())); }
    state.workflow.start_revision(&id,&request)?;
    state.runner.start(id.clone(),"revision_plan".into(),String::new())?;
    Ok(Json(json!({"project":state.repo.require(&id)?})))
}

async fn edit_document(State(state): State<AppState>, Path(id): Path<String>, Json(request): Json<GateRequest>) -> Result<Json<Value>> {
    state.workflow.manual_document(&id,request.content.as_deref().unwrap_or(""))?;
    Ok(Json(json!({"project":state.repo.require(&id)?})))
}

async fn preview(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    let (html,markdown)=super::preview::preview(&state.repo.require(&id)?)?;
    Ok(Json(json!({"html":html,"markdown":markdown})))
}

async fn asset(State(state): State<AppState>, Path(id): Path<String>, Query(query): Query<BTreeMap<String,String>>) -> Result<Response> {
    let project=state.repo.require(&id)?;
    let path=contained_file(&project.working_dir()?,query.get("path").map(String::as_str).unwrap_or(""))?;
    let extension=path.extension().and_then(|v|v.to_str()).unwrap_or("").to_lowercase();
    let mime=match extension.as_str() { "png"=>"image/png","jpg"|"jpeg"=>"image/jpeg","webp"=>"image/webp",_=>return Err(AppError::Invalid("图片格式不支持".into())) };
    Ok(([(header::CONTENT_TYPE,mime)],Body::from(tokio::fs::read(path).await?)).into_response())
}

async fn export(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    let project=state.repo.require(&id)?;
    let audit_type=project.artifacts.get("project_type").and_then(Value::as_str)==Some("document_review");
    if audit_type {
        if project.status!="audit_done" { return Err(AppError::Workflow("结构审核完成后再导出报告".into())); }
    } else if project.status!="approved" || !project.approved("review") {
        return Err(AppError::Workflow("架构审核尚未通过，不能标记为定稿导出".into()));
    }
    Ok(Json(crate::export::package::export(&state.repo,&project)?))
}

async fn download(State(state): State<AppState>, Path((id,kind)): Path<(String,String)>) -> Result<Response> {
    let project=state.repo.require(&id)?;
    let audit_type=project.artifacts.get("project_type").and_then(Value::as_str)==Some("document_review");
    if project.status!="approved" && !(audit_type && project.status=="audit_done") { return Err(AppError::Workflow("审核通过并导出后才能下载".into())); }
    let title=safe_name(&project.name);
    let filename=match kind.as_str() { "markdown"=>format!("{title}.md"),"docx"=>format!("{title}.docx"),"zip"=>format!("{title}-v{}.zip",project.revision),_=>return Err(AppError::Invalid("未知下载格式".into())) };
    let path=project.working_dir()?.join(&filename);
    if !path.is_file() { return Err(AppError::Invalid("请先生成导出包".into())); }
    let disposition=header::HeaderValue::from_str(&format!("attachment; filename=\"{}\"",filename)).map_err(|_|AppError::Invalid("下载文件名无效".into()))?;
    Ok(([(header::CONTENT_TYPE,header::HeaderValue::from_static("application/octet-stream")),(header::CONTENT_DISPOSITION,disposition)],Body::from(tokio::fs::read(path).await?)).into_response())
}

async fn settings(State(state): State<AppState>) -> Result<Json<Value>> { Ok(Json(role_settings(&state)?)) }

async fn save_settings(State(state): State<AppState>, Path(role): Path<String>, Json(request): Json<AgentSettings>) -> Result<Json<Value>> {
    let role=match role.as_str() { "research"=>Role::Research,"architect"=>Role::Architect,"writer"=>Role::Writer,"interaction"=>Role::Interaction,"producer"=>Role::Producer,_=>return Err(AppError::Invalid("未知角色".into())) };
    let mut settings=request.clone();
    if settings.api_key_env.is_empty() || !settings.api_key_env.chars().all(|c|c.is_ascii_uppercase()||c.is_ascii_digit()||c=='_') { return Err(AppError::Invalid("密钥环境变量名不合法".into())); }
    // 保存前做与运行时一致的校验：一条坏配置会永久打挂 GET /api/bootstrap。
    if !(0.0..=2.0).contains(&settings.temperature) { return Err(AppError::Invalid("temperature 必须在 0–2 之间".into())); }
    if !(512..=65536).contains(&settings.max_tokens) { return Err(AppError::Invalid("max_tokens 必须在 512–65536 之间".into())); }
    if settings.base_url.is_empty() { return Err(AppError::Invalid("Base URL 不能为空".into())); }
    if settings.base_url.starts_with("http://") && !settings.base_url.starts_with("http://127.0.0.1") && !settings.base_url.starts_with("http://localhost") { return Err(AppError::Invalid("Base URL 必须使用 HTTPS（本机测试地址除外）".into())); }
    if !matches!(settings.thinking.as_str(), "" | "enabled" | "disabled") { return Err(AppError::Invalid("thinking 只能为空、enabled 或 disabled".into())); }
    for skill in &settings.skills {
        if skill.is_empty() || !skill.chars().all(|c|c.is_ascii_alphanumeric()||c=='-') { return Err(AppError::Invalid(format!("Skill 名称不合法：{skill}"))); }
    }
    if let Some(key)=&settings.api_key { if !key.is_empty() { state.secrets.set(&settings.api_key_env,key)?; } }
    settings.api_key=None;
    state.repo.set_setting(&format!("agent:{}",role.as_str()),serde_json::to_value(&settings)?)?;
    state.repo.set_setting(&format!("prompt:{}",role.as_str()),json!(settings.prompt))?;
    Ok(Json(json!({"status":"saved","key_configured":state.secrets.configured(&settings.api_key_env)})))
}

async fn save_search_key(State(state): State<AppState>, Json(request): Json<Value>) -> Result<Json<Value>> {
    let key=request.get("api_key").and_then(Value::as_str).unwrap_or("").trim();
    if key.is_empty() { return Err(AppError::Invalid("密钥不能为空".into())); }
    state.secrets.set("WEB_SEARCH_API_KEY",key)?;
    Ok(Json(json!({"status":"saved","configured":true})))
}
