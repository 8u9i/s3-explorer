use std::time::Duration;

use aws_sdk_s3::presigning::PresigningConfig;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio_util::io::ReaderStream;

use crate::error::{AppError, AppResult};
use crate::routes::guess_content_type;
use crate::state::{public_base_url, AppState};

#[derive(Debug, Deserialize)]
pub struct PresignQuery {
    pub key: String,
    #[serde(default)]
    pub download: Option<bool>,
    #[serde(default)]
    pub ttl: Option<u64>,
    #[serde(default)]
    pub method: Option<String>,
}

pub async fn presign_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PresignQuery>,
) -> AppResult<Response> {
    if q.key.is_empty() {
        return Err(AppError::BadRequest("missing key".into()));
    }
    let bucket = state.s3.config.bucket.clone();
    let ttl = q
        .ttl
        .map(Duration::from_secs)
        .unwrap_or(state.s3.config.presign_ttl);

    let is_put = q
        .method
        .as_deref()
        .map(|m| m.eq_ignore_ascii_case("put"))
        .unwrap_or(false);
    if is_put {
        return presign_put_inner(&state, &bucket, &q.key, ttl, &headers).await;
    }

    let mut req = state.s3.client.get_object().bucket(&bucket).key(&q.key);
    if matches!(q.download, Some(true)) {
        let fname = q
            .key
            .rsplit_once('/')
            .map(|(_, b)| b)
            .unwrap_or(&q.key)
            .to_string();
        let encoded = percent_encode_filename(&fname);
        req = req.response_content_disposition(format!(
            "attachment; filename=\"{}\"; filename*=UTF-8''{}",
            fname.replace('"', "_"),
            encoded
        ));
    }

    let presigned = req.presigned(PresigningConfig::expires_in(ttl).map_err(|e| {
        AppError::Internal(format!("presign ttl: {e}"))
    })?).await?;
    let url = presigned.uri().to_string();

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
                "url": url,
                "expires_in_secs": ttl.as_secs(),
                "key": q.key,
            })),
        )
            .into_response());
    }

    Ok((
        StatusCode::FOUND,
        [(header::LOCATION, HeaderValue::from_str(&url).unwrap())],
        "",
    )
        .into_response())
}

async fn presign_put_inner(
    state: &AppState,
    bucket: &str,
    key: &str,
    ttl: Duration,
    headers: &HeaderMap,
) -> AppResult<Response> {
    if key.is_empty() || key.contains("..") {
        return Err(AppError::BadRequest("invalid key".into()));
    }
    let content_type = headers
        .get("x-content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| guess_content_type(key));

    let mut req = state
        .s3
        .client
        .put_object()
        .bucket(bucket)
        .key(key)
        .content_type(&content_type);

    if let Some(max) = headers
        .get("x-content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
    {
        req = req.content_length(max);
    }

    let presigned = req
        .presigned(PresigningConfig::expires_in(ttl).map_err(|e| {
            AppError::Internal(format!("presign ttl: {e}"))
        })?)
        .await?;
    let url = presigned.uri().to_string();

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        axum::Json(serde_json::json!({
            "url": url,
            "method": "PUT",
            "key": key,
            "content_type": content_type,
            "expires_in_secs": ttl.as_secs(),
            "headers": {
                "Content-Type": content_type,
            },
        })),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct ProxyQuery {
    #[serde(default)]
    pub download: Option<bool>,
}

pub async fn proxy_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
    Query(q): Query<ProxyQuery>,
) -> AppResult<Response> {
    if key.is_empty() {
        return Err(AppError::BadRequest("missing key".into()));
    }
    if key.contains("..") {
        return Err(AppError::BadRequest("invalid key".into()));
    }
    let bucket = state.s3.config.bucket.clone();
    let mut req = state.s3.client.get_object().bucket(&bucket).key(&key);
    if let Some(rng) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        req = req.range(rng);
    }

    let out = req.send().await?;
    let ct = out
        .content_type()
        .map(|s| s.to_string())
        .unwrap_or_else(|| guess_content_type(&key));
    let length = out.content_length().map(|l| l as u64);
    let etag = out.e_tag().unwrap_or("").to_string();
    let last_modified = out
        .last_modified()
        .map(|d| d.fmt(aws_smithy_types::date_time::Format::HttpDate).unwrap_or_default())
        .unwrap_or_default();

    let disposition = if matches!(q.download, Some(true)) {
        let fname = key.rsplit_once('/').map(|(_, b)| b).unwrap_or(&key).to_string();
        format!(
            "attachment; filename=\"{}\"; filename*=UTF-8''{}",
            fname.replace('"', "_"),
            percent_encode_filename(&fname)
        )
    } else {
        "inline".to_string()
    };

    let byte_stream = out.body;
    let async_read = byte_stream.into_async_read();
    let stream = ReaderStream::new(async_read);

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&ct).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    if !etag.is_empty() {
        if let Ok(v) = HeaderValue::from_str(&etag) {
            response_headers.insert(header::ETAG, v);
        }
    }
    if !last_modified.is_empty() {
        if let Ok(v) = HeaderValue::from_str(&last_modified) {
            response_headers.insert(header::LAST_MODIFIED, v);
        }
    }
    if let Some(l) = length {
        if let Ok(v) = HeaderValue::from_str(&l.to_string()) {
            response_headers.insert(header::CONTENT_LENGTH, v);
        }
    }
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).unwrap_or_else(|_| HeaderValue::from_static("inline")),
    );

    let body = Body::from_stream(stream);
    Ok((StatusCode::OK, response_headers, body).into_response())
}

pub async fn presign_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> AppResult<Response> {
    if key.is_empty() || key.contains("..") {
        return Err(AppError::BadRequest("invalid key".into()));
    }
    let bucket = state.s3.config.bucket.clone();
    let ttl = state.s3.config.presign_ttl;
    let content_type = headers
        .get("x-content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| guess_content_type(&key));

    let req = state
        .s3
        .client
        .put_object()
        .bucket(&bucket)
        .key(&key)
        .content_type(&content_type);

    let presigned = req
        .presigned(PresigningConfig::expires_in(ttl).map_err(|e| {
            AppError::Internal(format!("presign ttl: {e}"))
        })?)
        .await?;
    let url = presigned.uri().to_string();
    let base = public_base_url(&headers);

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        axum::Json(serde_json::json!({
            "url": url,
            "method": "PUT",
            "key": key,
            "content_type": content_type,
            "expires_in_secs": ttl.as_secs(),
            "headers": {
                "Content-Type": content_type,
            },
            "callback": format!("{}/api/upload/callback", base.trim_end_matches('/')),
        })),
    )
        .into_response())
}

fn percent_encode_filename(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'~') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}
