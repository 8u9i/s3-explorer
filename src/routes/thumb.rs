use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::error::{AppError, AppResult};
use crate::routes::guess_content_type;
use crate::state::AppState;

pub async fn thumb(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> AppResult<Response> {
    if key.is_empty() || key.contains("..") {
        return Err(AppError::BadRequest("invalid key".into()));
    }
    let bucket = state.s3.config.bucket.clone();
    let ct = guess_content_type(&key);
    if !ct.starts_with("image/") {
        return Err(AppError::BadRequest("not an image".into()));
    }
    if !matches!(ct.as_str(), "image/jpeg" | "image/png" | "image/webp" | "image/gif") {
        return Err(AppError::BadRequest(format!(
            "unsupported image format for thumbnail: {ct}"
        )));
    }

    let out = state
        .s3
        .client
        .get_object()
        .bucket(&bucket)
        .key(&key)
        .send()
        .await?;

    let mut bytes = Vec::new();
    let mut stream = out.body.into_async_read();
    let mut tmp = [0u8; 16 * 1024];
    loop {
        match stream.read(&mut tmp).await {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&tmp[..n]),
            Err(e) => return Err(AppError::Io(e)),
        }
        if bytes.len() > 20 * 1024 * 1024 {
            return Err(AppError::PayloadTooLarge(bytes.len()));
        }
    }
    drop(stream);

    let resized = resize_image(&bytes, &ct, 320)?;
    let hash = hex::encode(Sha256::digest(&resized));
    let etag = format!("\"{hash}\"");

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/webp"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    headers.insert(header::ETAG, HeaderValue::from_str(&etag).unwrap());
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&resized.len().to_string()).unwrap(),
    );

    Ok((StatusCode::OK, headers, resized).into_response())
}

fn resize_image(input: &[u8], ct: &str, max_dim: u32) -> AppResult<Vec<u8>> {
    use image::ImageReader;
    use std::io::Cursor;

    let img = ImageReader::new(Cursor::new(input))
        .with_guessed_format()
        .map_err(|e| AppError::Internal(format!("image format: {e}")))?
        .decode()
        .map_err(|e| AppError::Internal(format!("image decode: {e}")))?;

    let w = img.width();
    let h = img.height();
    let scale = (max_dim as f32 / w.max(h) as f32).min(1.0);
    let nw = ((w as f32 * scale).round() as u32).max(1);
    let nh = ((h as f32 * scale).round() as u32).max(1);
    let resized = img.resize(nw, nh, image::imageops::FilterType::Triangle);

    let mut out = Vec::new();
    let mut cursor = Cursor::new(&mut out);
    resized
        .write_to(&mut cursor, image::ImageFormat::WebP)
        .map_err(|e| AppError::Internal(format!("image encode: {e}")))?;
    let _ = ct;
    Ok(out)
}

#[allow(dead_code)]
fn _stream_unused() {
    let _ = futures::stream::empty::<Result<bytes::Bytes, std::io::Error>>().next();
}
