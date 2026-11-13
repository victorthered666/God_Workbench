use serde_json::{json, Value};
use crate::{config::{MAX_MESSAGE_BYTES, MAX_SESSION_BYTES}, paths::truncate_utf8, repository::Repository, Result};

pub fn save(repo: &Repository, project_id: &str, role: &str, messages: &[Value]) -> Result<()> {
    let key=format!("session:{project_id}:{role}");
    let history=repo.setting(&key,json!([]))?;
    let mut compact:Vec<Value>=history.as_array().into_iter().flatten().chain(messages).filter_map(|message| {
        let role=message.get("role")?.as_str()?;
        if !matches!(role,"user"|"assistant") { return None; }
        let content=message.get("content")?.as_str()?;
        Some(json!({"role":role,"content":format!("{}{}",truncate_utf8(content,MAX_MESSAGE_BYTES),if content.len()>MAX_MESSAGE_BYTES{"…[历史文本已截断]"}else{""})}))
    }).collect();
    while compact.len()>1 && serde_json::to_vec(&compact)?.len()>MAX_SESSION_BYTES { compact.remove(0); }
    let start=compact.len().saturating_sub(30);
    repo.set_setting(&key,Value::Array(compact[start..].to_vec()))
}
