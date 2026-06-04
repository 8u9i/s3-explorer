use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::routes::guess_content_type;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct UploadResult {
    pub key: String,
    pub size: u64,
    pub content_type: String,
    pub etag: String,
}

pub async fn upload_multipart(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let max = state.s3.config.max_upload_bytes;
    let bucket = state.s3.config.bucket.clone();
    let mut last_result: Option<UploadResult> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Multipart(format!("reading multipart field: {e}")))?
    {
        let name = field.name().unwrap_or("file").to_string();
        if name != "file" && name != "files[]" {
            tracing::debug!(field = %name, "skipping non-file field");
            let _ = field.bytes().await;
            continue;
        }

        let original = field
            .file_name()
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::BadRequest("missing filename".into()))?;

        let key = resolve_upload_key(&headers, &original);

        let content_type = field
            .content_type()
            .map(|s| s.to_string())
            .unwrap_or_else(|| guess_content_type(&key));

        let content_length = field
            .bytes()
            .await
            .map_err(|e| AppError::Multipart(format!("reading field bytes: {e}")))?;
        let size = content_length.len();
        if size > max {
            return Err(AppError::PayloadTooLarge(size));
        }

        let resp = state
            .s3
            .client
            .put_object()
            .bucket(&bucket)
            .key(&key)
            .content_type(&content_type)
            .content_length(size as i64)
            .body(content_length.into())
            .send()
            .await?;

        let etag = resp.e_tag().unwrap_or("").trim_matches('"').to_string();

        last_result = Some(UploadResult {
            key: key.clone(),
            size: size as u64,
            content_type,
            etag,
        });
        tracing::info!(key = %key, size, "uploaded object");
    }

    let Some(result) = last_result else {
        return Err(AppError::BadRequest(
            "no file field in multipart body".into(),
        ));
    };

    let is_ajax = headers
        .get("x-requested-with")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("xmlhttprequest"))
        .unwrap_or(false)
        || headers
            .get("accept")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.contains("application/json"))
            .unwrap_or(false);

    if is_ajax {
        return Ok((
            StatusCode::CREATED,
            [(header::CONTENT_TYPE, "application/json")],
            axum::Json(serde_json::json!({
                "ok": true,
                "uploaded": [result],
            })),
        )
            .into_response());
    }

    let back = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("/");
    let loc = HeaderValue::from_str(back).unwrap_or_else(|_| HeaderValue::from_static("/"));
    Ok((
        StatusCode::SEE_OTHER,
        [(header::LOCATION, loc)],
        format!("uploaded {}", result.key),
    )
        .into_response())
}

#[derive(Debug, serde::Deserialize)]
pub struct UploadQuery {
    #[serde(default)]
    pub key: Option<String>,
}

pub async fn upload_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UploadQuery>,
    body: Body,
) -> AppResult<Response> {
    let max = state.s3.config.max_upload_bytes;
    let bucket = state.s3.config.bucket.clone();
    let key = q
        .key
        .ok_or_else(|| AppError::BadRequest("missing ?key=".into()))?;
    if key.is_empty() || key.contains("..") {
        return Err(AppError::BadRequest("invalid key".into()));
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
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

    let resp = req.send().await?;
    let etag = resp.e_tag().unwrap_or("").trim_matches('"').to_string();
    Ok((
        StatusCode::CREATED,
        [(header::CONTENT_TYPE, "application/json")],
        axum::Json(serde_json::json!({
            "ok": true,
            "key": key,
            "etag": etag,
            "content_type": content_type,
        })),
    )
        .into_response())
}

fn resolve_upload_key(headers: &HeaderMap, original: &str) -> String {
    let prefix = headers
        .get("x-upload-prefix")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let raw = if prefix.is_empty() {
        original.to_string()
    } else {
        let mut p = prefix.to_string();
        if !p.ends_with('/') {
            p.push('/');
        }
        p + original
    };
    sanitize_key(&raw)
}

pub fn sanitize_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for (i, seg) in key.split('/').enumerate() {
        if i > 0 {
            out.push('/');
        }
        if seg.is_empty() || seg == "." {
            continue;
        }
        let cleaned: String = seg
            .chars()
            .filter(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(
                        c,
                        '.' | '-'
                            | '_'
                            | '~'
                            | ' '
                            | '('
                            | ')'
                            | '['
                            | ']'
                            | '+'
                            | ','
                            | '@'
                            | '!'
                            | '\''
                    )
            })
            .collect();
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            out.push_str("_");
        } else {
            out.push_str(trimmed);
        }
    }
    if out.starts_with('/') {
        out.remove(0);
    }
    if out.is_empty() {
        "_".to_string()
    } else {
        out
    }
}

pub async fn upload_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_key): Path<String>,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let max = state.s3.config.max_upload_bytes;
    let bucket = state.s3.config.bucket.clone();
    if target_key.is_empty() || target_key.contains("..") {
        return Err(AppError::BadRequest("invalid target key".into()));
    }
    let mut last_key = target_key.clone();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Multipart(format!("reading multipart field: {e}")))?
    {
        let data = field
            .bytes()
            .await
            .map_err(|e| AppError::Multipart(format!("reading field bytes: {e}")))?;
        let size = data.len();
        if size > max {
            return Err(AppError::PayloadTooLarge(size));
        }
        let ct = guess_content_type(&target_key);
        state
            .s3
            .client
            .put_object()
            .bucket(&bucket)
            .key(&target_key)
            .content_type(&ct)
            .content_length(size as i64)
            .body(data.into())
            .send()
            .await?;
        tracing::info!(key = %target_key, size, "replaced object");
        last_key = target_key.clone();
    }
    let back = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("/preview/{}", urlencoding::encode(&last_key).into_owned()));
    let loc = HeaderValue::from_str(&back).unwrap_or_else(|_| HeaderValue::from_static("/"));
    Ok((
        StatusCode::SEE_OTHER,
        [(header::LOCATION, loc)],
        "replaced",
    )
        .into_response())
}
