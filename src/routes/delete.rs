use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub async fn delete_object(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> AppResult<Response> {
    if key.is_empty() || key.contains("..") {
        return Err(AppError::BadRequest("invalid key".into()));
    }
    let bucket = state.s3.config.bucket.clone();
    state
        .s3
        .client
        .delete_object()
        .bucket(&bucket)
        .key(&key)
        .send()
        .await?;
    tracing::info!(key = %key, "deleted object");
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        axum::Json(serde_json::json!({
            "ok": true,
            "key": key,
        })),
    )
        .into_response())
}

pub async fn delete_prefix(
    State(state): State<AppState>,
    Path(prefix): Path<String>,
) -> AppResult<Response> {
    if prefix.is_empty() {
        return Err(AppError::BadRequest("empty prefix".into()));
    }
    let bucket = state.s3.config.bucket.clone();
    let mut token: Option<String> = None;
    let mut deleted: u64 = 0;
    loop {
        let mut req = state
            .s3
            .client
            .list_objects_v2()
            .bucket(&bucket)
            .prefix(&prefix)
            .max_keys(1000);
        if let Some(t) = &token {
            req = req.continuation_token(t);
        }
        let resp = req.send().await?;
        for obj in resp.contents() {
            if let Some(k) = obj.key() {
                state
                    .s3
                    .client
                    .delete_object()
                    .bucket(&bucket)
                    .key(k)
                    .send()
                    .await?;
                deleted += 1;
            }
        }
        if resp.is_truncated() == Some(true) {
            token = resp.next_continuation_token().map(|s| s.to_string());
            if token.is_none() {
                break;
            }
        } else {
            break;
        }
    }
    tracing::info!(prefix = %prefix, deleted, "deleted prefix");
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        axum::Json(serde_json::json!({
            "ok": true,
            "prefix": prefix,
            "deleted": deleted,
        })),
    )
        .into_response())
}
