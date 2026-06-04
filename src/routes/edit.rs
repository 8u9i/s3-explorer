use askama::Template;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use serde::Deserialize;
use tokio::io::AsyncReadExt;

use crate::error::{AppError, AppResult};
use crate::routes::{
    guess_content_type, html_escape, human_size, is_text_key, render_crumbs, CrumbSegment,
};
use crate::routes::template_into_response;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct EditQuery {
    pub key: String,
}

#[derive(Template)]
#[template(path = "edit.html")]
pub struct EditPage {
    pub key: String,
    pub name: String,
    pub content_type: String,
    pub size_label: String,
    #[allow(dead_code)]
    pub last_modified: String,
    pub short_modified: String,
    pub has_modified: bool,
    pub etag: String,
    pub text_content: String,
    pub save_url: String,
    pub preview_url: String,
    pub download_url: String,
    pub copy_url: String,
    pub crumb_segments: Vec<CrumbSegment>,
}

const EDIT_FETCH_LIMIT: usize = 1_048_576;

pub async fn edit_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<EditQuery>,
) -> AppResult<Response> {
    if q.key.is_empty() {
        return Err(AppError::BadRequest("missing key".into()));
    }
    if q.key.contains("..") {
        return Err(AppError::BadRequest("invalid key".into()));
    }
    let bucket = state.s3.config.bucket.clone();
    let head = state
        .s3
        .client
        .head_object()
        .bucket(&bucket)
        .key(&q.key)
        .send()
        .await;

    let (content_type, size, last_modified, etag) = match head {
        Ok(h) => (
            h.content_type()
                .map(|s| s.to_string())
                .unwrap_or_else(|| guess_content_type(&q.key)),
            h.content_length().unwrap_or(0),
            h.last_modified()
                .map(|d| d.fmt(aws_smithy_types::date_time::Format::HttpDate).unwrap_or_default())
                .unwrap_or_default(),
            h.e_tag().unwrap_or("").trim_matches('"').to_string(),
        ),
        Err(_) => {
            return Err(AppError::NotFound(format!("object not found: {}", q.key)));
        }
    };

    if !is_text_key(&q.key, &content_type) {
        return Err(AppError::BadRequest(format!(
            "object is not text-editable: {content_type}"
        )));
    }

    let get = state
        .s3
        .client
        .get_object()
        .bucket(&bucket)
        .key(&q.key)
        .send()
        .await?;

    let mut buf: Vec<u8> = Vec::with_capacity(EDIT_FETCH_LIMIT.min(64 * 1024));
    let mut total = 0usize;
    let mut stream = get.body.into_async_read();
    let mut tmp = [0u8; 8192];
    loop {
        if total >= EDIT_FETCH_LIMIT {
            break;
        }
        let to_read = (EDIT_FETCH_LIMIT - total).min(tmp.len());
        match stream.read(&mut tmp[..to_read]).await {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                total += n;
            }
            Err(e) => return Err(AppError::Io(e)),
        }
    }
    let text_content = String::from_utf8_lossy(&buf).to_string();

    let base = crate::state::public_base_url(&headers);
    let enc = urlencoding::encode(&q.key).into_owned();
    let name = q
        .key
        .rsplit_once('/')
        .map(|(_, b)| b.to_string())
        .unwrap_or_else(|| q.key.clone());
    let short_modified: String = last_modified.get(..19).unwrap_or("").to_string();
    let crumb_segments = render_crumbs(&q.key);
    let page = EditPage {
        key: q.key.clone(),
        name,
        content_type,
        size_label: human_size(size),
        last_modified: last_modified.clone(),
        short_modified: short_modified.clone(),
        has_modified: !last_modified.is_empty(),
        etag,
        text_content: html_escape(&text_content),
        save_url: format!("{}/api/objects/{}", base, enc),
        preview_url: format!("{}/preview/{}", base, enc),
        download_url: format!("{}/files/{}?download=1", base, enc),
        copy_url: format!("{}/copy?from={}", base, enc),
        crumb_segments,
    };
    template_into_response(&page).map_err(Into::into)
}

#[derive(Debug, Deserialize)]
pub struct PutQuery {
    #[serde(default)]
    pub content_type: Option<String>,
}

pub async fn put_object(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
    Query(q): Query<PutQuery>,
    body: Body,
) -> AppResult<Response> {
    if key.is_empty() || key.contains("..") {
        return Err(AppError::BadRequest("invalid key".into()));
    }
    let max = state.s3.config.max_upload_bytes;
    let bucket = state.s3.config.bucket.clone();
    let content_type = q
        .content_type
        .or_else(|| {
            headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| guess_content_type(&key));

    let content_length: Option<i64> = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    if let Some(len) = content_length {
        if (len as usize) > max {
            return Err(AppError::PayloadTooLarge(len as usize));
        }
    }

    let bytes = axum::body::to_bytes(body, max)
        .await
        .map_err(|e| AppError::Multipart(format!("reading body: {e}")))?;
    if bytes.len() > max {
        return Err(AppError::PayloadTooLarge(bytes.len()));
    }
    let byte_stream = aws_sdk_s3::primitives::ByteStream::from(bytes);

    let mut req = state
        .s3
        .client
        .put_object()
        .bucket(&bucket)
        .key(&key)
        .content_type(&content_type)
        .body(byte_stream);
    if let Some(len) = content_length {
        req = req.content_length(len);
    }
    let out = req.send().await?;
    let etag = out.e_tag().unwrap_or("").trim_matches('"').to_string();

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
                "key": key,
                "etag": etag,
                "content_type": content_type,
            })),
        )
            .into_response());
    }
    let back = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("/preview/{}", urlencoding::encode(&key).into_owned()));
    let loc = HeaderValue::from_str(&back).unwrap_or_else(|_| HeaderValue::from_static("/"));
    Ok((
        StatusCode::SEE_OTHER,
        [(header::LOCATION, loc)],
        format!("saved {key}"),
    )
        .into_response())
}

#[allow(dead_code)]
fn _stream_unused() {
    let _ = futures::stream::empty::<Result<bytes::Bytes, std::io::Error>>().next();
}
