use askama::Template;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::routes::{guess_content_type, html_escape, human_size, render_crumbs, CrumbSegment};
use crate::routes::upload::sanitize_key;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CopyQuery {
    pub from: String,
    #[serde(default)]
    pub to: Option<String>,
}

pub async fn copy_object(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CopyQuery>,
) -> AppResult<Response> {
    if q.from.is_empty() || q.from.contains("..") {
        return Err(AppError::BadRequest("invalid from key".into()));
    }
    let dest = q
        .to
        .clone()
        .ok_or_else(|| AppError::BadRequest("missing 'to' key".into()))?;
    let dest = sanitize_key(&dest);
    if dest.is_empty() {
        return Err(AppError::BadRequest("invalid to key".into()));
    }
    if dest == q.from {
        return Err(AppError::BadRequest("source and destination are the same".into()));
    }
    let bucket = state.s3.config.bucket.clone();

    let copy_source = format!("{}/{}", bucket, q.from);
    let src = urlencoding::encode(&copy_source).into_owned();
    let out = state
        .s3
        .client
        .copy_object()
        .bucket(&bucket)
        .key(&dest)
        .copy_source(&src)
        .send()
        .await?;
    let etag = out
        .copy_object_result()
        .and_then(|r| r.e_tag())
        .unwrap_or("")
        .trim_matches('"')
        .to_string();
    tracing::info!(from = %q.from, to = %dest, "copied object");

    let is_json = headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("application/json"))
        .unwrap_or(false);
    if is_json {
        return Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            axum::Json(serde_json::json!({
                "ok": true,
                "from": q.from,
                "to": dest,
                "etag": etag,
            })),
        )
            .into_response());
    }
    let back = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("/");
    let loc: axum::http::HeaderValue = back
        .parse()
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("/"));
    Ok((
        StatusCode::SEE_OTHER,
        [(header::LOCATION, loc)],
        format!("copied to {dest}"),
    )
        .into_response())
}

pub async fn copy_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CopyQuery>,
) -> AppResult<Response> {
    if q.from.is_empty() {
        return Err(AppError::BadRequest("missing from".into()));
    }
    let bucket = state.s3.config.bucket.clone();
    let head = state
        .s3
        .client
        .head_object()
        .bucket(&bucket)
        .key(&q.from)
        .send()
        .await
        .map_err(|e| AppError::S3(format!("{e:?}")))?;
    let content_type = head
        .content_type()
        .map(|s| s.to_string())
        .unwrap_or_else(|| guess_content_type(&q.from));
    let size = head.content_length().unwrap_or(0);
    let last_modified = head
        .last_modified()
        .map(|d| d.fmt(aws_smithy_types::date_time::Format::HttpDate).unwrap_or_default())
        .unwrap_or_default();

    let enc = urlencoding::encode(&q.from).into_owned();
    let base = crate::state::public_base_url(&headers);
    let size_label = human_size(size);
    let name = q
        .from
        .rsplit_once('/')
        .map(|(_, b)| b.to_string())
        .unwrap_or_else(|| q.from.clone());
    let to_suggestion = if q.from.contains('.') {
        let pos = q.from.rfind('.').unwrap();
        let stem = &q.from[..pos];
        let ext = &q.from[pos..];
        let combined = format!("{}-copy{}", stem, ext);
        combined
    } else {
        format!("{}-copy", q.from)
    };
    let has_modified = !last_modified.is_empty();

    #[derive(Template)]
    #[template(path = "copy.html")]
    struct CopyPage {
        from: String,
        from_enc: String,
        content_type: String,
        size_label: String,
        last_modified: String,
        has_modified: bool,
        action_url: String,
        name: String,
        to_suggestion: String,
        crumb_segments: Vec<CrumbSegment>,
    }
    let crumb_segments = render_crumbs(&q.from);
    let page = CopyPage {
        from: html_escape(&q.from),
        from_enc: enc,
        content_type,
        size_label,
        last_modified,
        has_modified,
        action_url: format!("{}/api/copy?from={}", base, urlencoding::encode(&q.from).into_owned()),
        name: html_escape(&name),
        to_suggestion,
        crumb_segments,
    };
    let html = page
        .render()
        .map_err(|e| AppError::Internal(format!("template: {e}")))?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct MoveQuery {
    pub from: String,
    pub to: String,
}

pub async fn move_object(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<MoveQuery>,
) -> AppResult<Response> {
    if q.from.is_empty() || q.to.is_empty() {
        return Err(AppError::BadRequest("missing keys".into()));
    }
    if q.from.contains("..") {
        return Err(AppError::BadRequest("invalid from key".into()));
    }
    let to = sanitize_key(&q.to);
    if to.is_empty() {
        return Err(AppError::BadRequest("invalid to key".into()));
    }
    let bucket = state.s3.config.bucket.clone();
    let copy_source = format!("{}/{}", bucket, q.from);
    let src = urlencoding::encode(&copy_source).into_owned();
    state
        .s3
        .client
        .copy_object()
        .bucket(&bucket)
        .key(&to)
        .copy_source(&src)
        .send()
        .await?;
    state
        .s3
        .client
        .delete_object()
        .bucket(&bucket)
        .key(&q.from)
        .send()
        .await?;
    tracing::info!(from = %q.from, to = %to, "moved object");

    let is_json = headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("application/json"))
        .unwrap_or(false);
    if is_json {
        return Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            axum::Json(serde_json::json!({
                "ok": true,
                "from": q.from,
                "to": to,
            })),
        )
            .into_response());
    }
    let back = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("/");
    let loc: axum::http::HeaderValue = back
        .parse()
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("/"));
    Ok((
        StatusCode::SEE_OTHER,
        [(header::LOCATION, loc)],
        format!("moved to {to}"),
    )
        .into_response())
}
