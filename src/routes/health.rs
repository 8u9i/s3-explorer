use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

use crate::state::AppState;

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let bucket = state.s3.config.bucket.as_str();
    (
        axum::http::StatusCode::OK,
        Json(json!({
            "status": "ok",
            "bucket": bucket,
            "endpoint": state.s3.config.endpoint,
        })),
    )
}

pub async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    let bucket = state.s3.config.bucket.as_str();
    match state
        .s3
        .client
        .head_bucket()
        .bucket(bucket)
        .send()
        .await
    {
        Ok(_) => (
            axum::http::StatusCode::OK,
            Json(json!({
                "status": "ready",
                "bucket": bucket,
                "endpoint": state.s3.config.endpoint,
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "head_bucket failed");
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "degraded",
                    "bucket": bucket,
                    "error": format!("{e:?}"),
                })),
            )
                .into_response()
        }
    }
}

pub async fn version() -> impl IntoResponse {
    (
        axum::http::StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "application/json",
        )],
        Json(json!({
            "name": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
        })),
    )
}

#[allow(dead_code)]
pub fn _headers_unused(_h: &HeaderMap) {}
