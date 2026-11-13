use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Workflow(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Agent(String),
    #[error("{0}")]
    Tool(String),
    #[error("本地数据操作失败")]
    Storage(#[from] rusqlite::Error),
    #[error("本地文件操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("数据格式不符合约定：{0}")]
    Json(#[from] serde_json::Error),
    #[error("配置格式不符合约定：{0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("图片处理失败：{0}")]
    Image(#[from] image::ImageError),
    #[error("网络请求失败：{0}")]
    Network(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, AppError>;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::Workflow(message) => (StatusCode::CONFLICT, message.clone()),
            Self::Invalid(message) => (StatusCode::BAD_REQUEST, message.clone()),
            Self::Agent(message) | Self::Tool(message) | Self::Other(message) => (StatusCode::BAD_REQUEST, message.clone()),
            Self::Network(message) => (StatusCode::BAD_GATEWAY, message.clone()),
            other => (StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        };
        (status, Json(json!({"detail": message}))).into_response()
    }
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        Self::Network(format!("{}：{}", if value.is_timeout() { "请求超时" } else { "网络错误" }, value.to_string().chars().take(240).collect::<String>()))
    }
}
