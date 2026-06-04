use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;

use crate::state::AppState;

pub async fn require_auth(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if !state.auth.is_enabled() {
        return next.run(req).await;
    }
    let user = state.auth.user.as_deref().unwrap_or("");
    let pass = state.auth.pass.as_deref().unwrap_or("");

    if check_header(req.headers(), user, pass) {
        return next.run(req).await;
    }

    let mut resp = (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        "Basic realm=\"s3-explorer\", charset=\"UTF-8\""
            .parse()
            .unwrap(),
    );
    resp
}

fn check_header(headers: &HeaderMap, expected_user: &str, expected_pass: &str) -> bool {
    let Some(h) = headers.get(header::AUTHORIZATION) else {
        return false;
    };
    let Some(s) = h.to_str().ok() else {
        return false;
    };
    let Some(rest) = s.strip_prefix("Basic ") else {
        return false;
    };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(rest.trim()) else {
        return false;
    };
    let Ok(s) = std::str::from_utf8(&decoded) else {
        return false;
    };
    let Some((u, p)) = s.split_once(':') else {
        return false;
    };
    constant_time_eq(u.as_bytes(), expected_user.as_bytes())
        && constant_time_eq(p.as_bytes(), expected_pass.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut x = 0u8;
    for (i, j) in a.iter().zip(b.iter()) {
        x |= i ^ j;
    }
    x == 0
}
