use axum::{body::Body, extract::{Request, State}, http::{Method, StatusCode}, middleware::Next, response::{IntoResponse, Response}, Json};
use serde_json::json;
use crate::http::routes::AppState;

pub async fn local_write_guard(State(state): State<AppState>, request: Request<Body>, next: Next) -> Response {
    if !matches!(request.method(),&Method::GET|&Method::HEAD|&Method::OPTIONS) {
        if request.headers().get("x-gdd-token").and_then(|v|v.to_str().ok())!=Some(state.csrf_token.as_ref()) {
            return (StatusCode::FORBIDDEN,Json(json!({"detail":"写入请求缺少本机令牌"}))).into_response();
        }
    }
    next.run(request).await
}
