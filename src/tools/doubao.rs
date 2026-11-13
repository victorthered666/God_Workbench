use serde_json::{json, Value};
use crate::{repository::Repository, AppError, Result};
use super::http::ToolHttp;

#[derive(Clone)]
pub struct Doubao { http: ToolHttp }

impl Doubao {
    pub fn new(http: ToolHttp) -> Self { Self { http } }

    pub async fn search(&self, repo: &Repository, project_id: &str, query: &str, count: u8) -> Result<Value> {
        if query.trim().is_empty() || query.chars().count()>100 || !(1..=5).contains(&count) {
            return Err(AppError::Tool("豆包图片检索词需 1–100 字符，每次最多 5 条".into()));
        }
        let data=self.http.post("https://open.feedcoopapi.com/search_api/web_search",json!({"Query":query,"SearchType":"image","Count":count}),"WEB_SEARCH_API_KEY",true).await?;
        if let Some(error)=data.pointer("/ResponseMetadata/Error") {
            return Err(AppError::Tool(format!("豆包搜索 {}: {}",error.get("Code").and_then(Value::as_str).unwrap_or(""),error.get("Message").and_then(Value::as_str).unwrap_or(""))));
        }
        let items=data.pointer("/Result/ImageResults").and_then(Value::as_array).ok_or_else(||AppError::Tool("豆包图片搜索返回格式不符合 ImageResults 约定".into()))?;
        let mut results=vec![];
        for item in items {
            let Some(image)=item.get("Image") else { continue; };
            let Some(url)=image.get("Url").and_then(Value::as_str) else { continue; };
            if !url.starts_with("https://") { continue; }
            results.push(json!({"title":item.get("Title").and_then(Value::as_str).unwrap_or(""),"url":url,"width":image.get("Width").cloned().unwrap_or(Value::Null),"height":image.get("Height").cloned().unwrap_or(Value::Null),"source_page":item.get("Url").or_else(||item.get("SourceUrl")).or_else(||item.get("PageUrl")).and_then(Value::as_str).unwrap_or("")}));
        }
        repo.evidence(project_id,query,json!({"kind":"image","results":results}))?;
        Ok(Value::Array(results))
    }
}
