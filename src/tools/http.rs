use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde_json::Value;
use crate::{config::MAX_RESPONSE_BYTES, secrets::SecretStore, AppError, Result};

#[derive(Clone)]
pub struct ToolHttp {
    client: Client,
    secrets: SecretStore,
    image_limit: Arc<Semaphore>,
}

impl ToolHttp {
    pub fn new(secrets: SecretStore) -> Result<Self> {
        static IMAGE_LIMIT: std::sync::OnceLock<Arc<Semaphore>>=std::sync::OnceLock::new();
        Ok(Self { client: Client::builder().timeout(Duration::from_secs(45)).connect_timeout(Duration::from_secs(10)).redirect(reqwest::redirect::Policy::none()).build()?, secrets, image_limit:IMAGE_LIMIT.get_or_init(||Arc::new(Semaphore::new(5))).clone() })
    }

    pub async fn post(&self, url: &str, body: Value, key_name: &str, traffic: bool) -> Result<Value> {
        let key=self.secrets.get(key_name)?.ok_or_else(||AppError::Tool(format!("尚未配置 {key_name}，未发起检索调用")))?;
        let _permit=if traffic { Some(self.image_limit.acquire().await.map_err(|_|AppError::Tool("图片检索并发控制已关闭".into()))?) } else { None };
        for attempt in 0..3 {
            let mut request=self.client.post(url).bearer_auth(&key).json(&body);
            if traffic { request=request.header("X-Traffic-Tag","skill_web_search_common"); }
            let mut response=match request.send().await {
                Ok(response) => response,
                Err(error) if attempt<2 => { let _=error; tokio::time::sleep(Duration::from_secs(1<<attempt)).await; continue; },
                Err(error) => return Err(error.into()),
            };
            let status=response.status();
            let bytes=bounded_body(&mut response).await?;
            if status==StatusCode::OK {
                return serde_json::from_slice(&bytes).map_err(|_|AppError::Tool("搜索服务返回了非 JSON 内容".into()));
            }
            if matches!(status,StatusCode::REQUEST_TIMEOUT|StatusCode::TOO_MANY_REQUESTS)||status.is_server_error() {
                if attempt<2 { tokio::time::sleep(Duration::from_secs(1<<attempt)).await; continue; }
            }
            let detail=serde_json::from_slice::<Value>(&bytes).ok().and_then(|v|v.pointer("/error/message").and_then(Value::as_str).map(str::to_owned)).unwrap_or_default();
            let hint=match status { StatusCode::BAD_REQUEST=>"请求参数不合法",StatusCode::UNAUTHORIZED=>"API Key 无效",StatusCode::PAYMENT_REQUIRED=>"服务余额不足",StatusCode::FORBIDDEN=>"目标页面被安全策略拒绝",StatusCode::NOT_FOUND=>"没有可用网页内容",StatusCode::PAYLOAD_TOO_LARGE=>"请求体过大",StatusCode::TOO_MANY_REQUESTS=>"请求过于频繁",_=>"服务错误" };
            return Err(AppError::Tool(format!("检索失败 HTTP {status}：{hint} {}",crate::paths::truncate_utf8(&detail,240))));
        }
        Err(AppError::Tool("检索服务暂不可用".into()))
    }
}

pub async fn bounded_body(response: &mut reqwest::Response) -> Result<Vec<u8>> {
    if response.content_length().is_some_and(|length|length as usize>MAX_RESPONSE_BYTES) {
        return Err(AppError::Tool("网络响应超过 8 MiB，已停止读取".into()));
    }
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await? {
        if bytes.len().checked_add(chunk.len()).is_none_or(|size|size>MAX_RESPONSE_BYTES) {
            return Err(AppError::Tool("网络响应超过 8 MiB，已停止读取".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
