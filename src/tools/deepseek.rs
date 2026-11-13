use serde_json::{json, Value};
use crate::{domain::id, paths::truncate_utf8, repository::Repository, AppError, Result};
use super::http::ToolHttp;

#[derive(Clone)]
pub struct DeepSeek { http: ToolHttp }

impl DeepSeek {
    pub fn new(http: ToolHttp) -> Self { Self { http } }

    pub async fn search(&self, repo: &Repository, project_id: &str, query: &str, count: u8) -> Result<Value> {
        if query.trim().is_empty() || query.chars().count()>128 || !(1..=50).contains(&count) {
            return Err(AppError::Tool("DeepSeek 检索词需 1–128 字符，条数需 1–50".into()));
        }
        let data=self.http.post("https://api.deepseek.com/v1/tools/search",json!({"query":query,"count":count,"need_content":true}),"DEEPSEEK_API_KEY",false).await?;
        let results=data.get("results").and_then(Value::as_array).ok_or_else(||AppError::Tool("DeepSeek 搜索返回缺少 results 数组".into()))?;
        let payload: Vec<Value>=results.iter().map(|result|json!({"id":id()[..10],"title":result.get("title").and_then(Value::as_str).unwrap_or(""),"url":result.get("url").and_then(Value::as_str).unwrap_or(""),"snippet":result.get("snippet").and_then(Value::as_str).map(|s|truncate_utf8(s,1000)).unwrap_or(""),"summary":result.get("summary").and_then(Value::as_str).map(|s|truncate_utf8(s,4000)).unwrap_or(""),"content":result.get("content").and_then(Value::as_str).map(|s|truncate_utf8(s,16000)).unwrap_or(""),"site_name":result.get("site_name").cloned().unwrap_or(Value::Null),"date":result.get("date").cloned().unwrap_or(Value::Null),"authority":result.get("authority").cloned().unwrap_or(Value::Null)})).collect();
        repo.evidence(project_id,query,json!({"kind":"web","results":payload}))?;
        Ok(Value::Array(payload))
    }

    pub async fn fetch(&self, repo: &Repository, project_id: &str, url: &str) -> Result<Value> {
        if !url.starts_with("https://") && !url.starts_with("http://") || url.len()>8192 || url.contains('@') {
            return Err(AppError::Tool("待抓取网址不符合要求".into()));
        }
        let data=self.http.post("https://api.deepseek.com/v1/tools/fetch",json!({"url":url}),"DEEPSEEK_API_KEY",false).await?;
        let content=data.get("content").and_then(Value::as_str).ok_or_else(||AppError::Tool("DeepSeek 抓取返回格式不符合文档".into()))?;
        let evidence=json!({"id":id()[..10],"title":data.get("title").and_then(Value::as_str).unwrap_or(""),"url":url,"content":truncate_utf8(content,32000)});
        repo.evidence(project_id,url,json!({"kind":"web","results":[evidence]}))?;
        Ok(evidence)
    }
}
