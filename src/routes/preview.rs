use askama::Template;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use serde::Deserialize;
use tokio::io::AsyncReadExt;

use crate::error::{AppError, AppResult};
use crate::routes::{guess_content_type, html_escape, human_size, is_text_key, render_crumbs};
use crate::routes::template_into_response;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PreviewQuery {
    #[serde(default)]
    pub download: Option<bool>,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

#[derive(Template)]
#[template(path = "preview.html")]
pub struct PreviewPage {
    pub key: String,
    pub name: String,
    pub content_type: String,
    pub size_label: String,
    #[allow(dead_code)]
    pub last_modified: String,
    pub short_modified: String,
    pub has_modified: bool,
    pub preview_kind: String,
    pub preview_limit: String,
    pub text_content: String,
    pub truncated: bool,
    pub is_text: bool,
    pub download_url: String,
    pub proxy_url: String,
    pub presign_url: String,
    pub edit_url: String,
    pub copy_url: String,
    pub crumb_html: String,
}

const TEXT_PREVIEW_LIMIT: usize = 1_048_576;

pub async fn preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
    Query(q): Query<PreviewQuery>,
) -> AppResult<Response> {
    if key.is_empty() {
        return Err(AppError::BadRequest("missing key".into()));
    }
    if key.contains("..") {
        return Err(AppError::BadRequest("invalid key".into()));
    }
    let bucket = state.s3.config.bucket.clone();

    if matches!(q.download, Some(true)) {
        return super::download::proxy_get(
            State(state),
            headers,
            Path(key),
            Query(super::download::ProxyQuery { download: Some(true) }),
        )
        .await;
    }

    let head = state
        .s3
        .client
        .head_object()
        .bucket(&bucket)
        .key(&key)
        .send()
        .await;

    let (content_type, size, last_modified, _etag) = match head {
        Ok(h) => (
            h.content_type()
                .map(|s| s.to_string())
                .unwrap_or_else(|| guess_content_type(&key)),
            h.content_length().unwrap_or(0),
            h.last_modified()
                .map(|d| d.fmt(aws_smithy_types::date_time::Format::HttpDate).unwrap_or_default())
                .unwrap_or_default(),
            h.e_tag().unwrap_or("").trim_matches('"').to_string(),
        ),
        Err(_) => {
            let ct = guess_content_type(&key);
            (ct, 0, String::new(), String::new())
        }
    };

    let is_image = content_type.starts_with("image/");
    let is_pdf = content_type == "application/pdf";
    let is_video = content_type.starts_with("video/");
    let is_audio = content_type.starts_with("audio/");
    let is_text_bool = is_text_key(&key, &content_type);
    let preview_kind = if is_image {
        "image"
    } else if is_pdf {
        "pdf"
    } else if is_video {
        "video"
    } else if is_audio {
        "audio"
    } else if is_text_bool {
        "text"
    } else {
        "binary"
    }
    .to_string();

    let limit = q.max_bytes.unwrap_or(TEXT_PREVIEW_LIMIT);
    let (text_content, truncated_bool) = if is_text_bool && size <= limit as i64 {
        match fetch_text(&state, &bucket, &key, limit).await {
            Ok((s, t)) => (html_escape(&s), t),
            Err(_) => (String::new(), false),
        }
    } else {
        (String::new(), false)
    };

    let base = crate::state::public_base_url(&headers);
    let enc = urlencoding::encode(&key).into_owned();
    let short_modified: String = last_modified.get(..19).unwrap_or("").to_string();
    let name = key
        .rsplit_once('/')
        .map(|(_, b)| b.to_string())
        .unwrap_or_else(|| key.clone());
    let crumb_html = render_crumbs(&key);

    let page = PreviewPage {
        key: key.clone(),
        name,
        content_type,
        size_label: human_size(size),
        last_modified: last_modified.clone(),
        short_modified: short_modified.clone(),
        has_modified: !last_modified.is_empty(),
        preview_kind,
        preview_limit: limit.to_string(),
        text_content,
        truncated: truncated_bool,
        is_text: is_text_bool,
        download_url: format!("{}/files/{}?download=1", base, enc),
        proxy_url: format!("{}/files/{}", base, enc),
        presign_url: format!("{}/api/presign?key={}", base, enc),
        edit_url: format!("{}/edit?key={}", base, enc),
        copy_url: format!("{}/copy?from={}", base, enc),
        crumb_html,
    };

    let wants_html = headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("text/html") || s.contains("*/*"))
        .unwrap_or(true);

    if wants_html {
        return template_into_response(&page).map_err(Into::into);
    }

    let json = serde_json::json!({
        "key": page.key,
        "content_type": page.content_type,
        "size_label": page.size_label,
        "preview_kind": page.preview_kind,
        "is_text": page.is_text,
    });
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        axum::Json(json),
    )
        .into_response())
}

pub async fn raw_object(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> AppResult<Response> {
    super::download::proxy_get(
        State(state),
        headers,
        Path(key),
        Query(super::download::ProxyQuery { download: None }),
    )
    .await
}

async fn fetch_text(
    state: &AppState,
    bucket: &str,
    key: &str,
    limit: usize,
) -> AppResult<(String, bool)> {
    let out = state
        .s3
        .client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await?;

    let mut buf: Vec<u8> = Vec::with_capacity(limit.min(64 * 1024));
    let mut total = 0usize;
    let mut truncated = false;
    let mut stream = out.body.into_async_read();
    let mut tmp = [0u8; 8192];
    loop {
        if total >= limit {
            truncated = true;
            break;
        }
        let to_read = (limit - total).min(tmp.len());
        match stream.read(&mut tmp[..to_read]).await {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                total += n;
            }
            Err(e) => return Err(AppError::Io(e)),
        }
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    Ok((text, truncated))
}

#[allow(dead_code)]
fn _stream_unused() {
    let _ = futures::stream::empty::<Result<bytes::Bytes, std::io::Error>>().next();
    let _ = Body::empty();
    let _ = HeaderValue::from_static("x");
}
