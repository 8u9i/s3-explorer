use askama::Template;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response, Redirect};
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde::Deserialize;

use crate::state::AppState;

const SESSION_COOKIE: &str = "s3_session";
const SESSION_TTL_SECS: i64 = 86400; // 24 hours

// ── Askama template ─────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginPage {
    pub error: String,
    pub redirect: String,
    pub display_url: String,
}

// ── Login / Logout handlers ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LoginQuery {
    #[serde(default)]
    pub next: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

pub async fn login_page(
    Query(q): Query<LoginQuery>,
) -> Response {
    let page = LoginPage {
        error: String::new(),
        redirect: q.next.clone(),
        display_url: urlencoding::encode(&q.next).into_owned(),
    };
    match page.render() {
        Ok(html) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("template error: {e}"),
        ).into_response(),
    }
}

pub async fn login_submit(
    State(state): State<AppState>,
    Query(q): Query<LoginQuery>,
    form: axum::extract::Form<LoginForm>,
) -> Response {
    let user = state.auth.user.as_deref().unwrap_or("");
    let pass = state.auth.pass.as_deref().unwrap_or("");

    if constant_time_eq(form.username.as_bytes(), user.as_bytes())
        && constant_time_eq(form.password.as_bytes(), pass.as_bytes())
    {
        let token = make_token(user, &state.auth.pass.clone().unwrap_or_default());
        let redirect_to = if q.next.is_empty() { "/browse".to_string() } else { q.next.clone() };
        let cookie = format!(
            "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECS}"
        );
        return (
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, redirect_to),
                (header::SET_COOKIE, cookie),
            ],
        ).into_response();
    }

    let page = LoginPage {
        error: "Invalid username or password".to_string(),
        redirect: q.next.clone(),
        display_url: urlencoding::encode(&q.next).into_owned(),
    };
    match page.render() {
        Ok(html) => (
            StatusCode::UNAUTHORIZED,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("template error: {e}"),
        ).into_response(),
    }
}

pub async fn logout() -> Response {
    let cookie = format!(
        "{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"
    );
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, "/login".to_string()),
            (header::SET_COOKIE, cookie),
        ],
    ).into_response()
}

// ── Auth middleware ──────────────────────────────────────────────────────

pub async fn require_auth(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if !state.auth.is_enabled() {
        return next.run(req).await;
    }

    // Check session cookie first
    if let Some(cookie_header) = req.headers().get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        if let Some(token) = parse_cookie(cookie_header, SESSION_COOKIE) {
            let secret = state.auth.pass.as_deref().unwrap_or("");
            if validate_token(token, secret) {
                return next.run(req).await;
            }
        }
    }

    // Fallback: check Basic Auth header (for API consumers)
    let user = state.auth.user.as_deref().unwrap_or("");
    let pass = state.auth.pass.as_deref().unwrap_or("");
    if check_basic_auth(req.headers(), user, pass) {
        return next.run(req).await;
    }

    // For API/AJAX requests, return 401 JSON. For browser requests, redirect to login.
    let wants_json = req
        .headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json"))
        .unwrap_or(false);

    let is_xhr = req
        .headers()
        .get("x-requested-with")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("XMLHttpRequest"))
        .unwrap_or(false);

    if wants_json || is_xhr {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error":"authentication required","status":401}"#,
        ).into_response();
    }

    // Browser redirect to login page
    let path = req.uri().path_and_query()
        .map(|pq| pq.to_string())
        .unwrap_or_else(|| "/browse".to_string());
    let login_url = format!("/login?next={}", urlencoding::encode(&path));
    Redirect::temporary(&login_url).into_response()
}

// ── Session token helpers ───────────────────────────────────────────────

type HmacSha256 = Hmac<Sha256>;

fn make_token(user: &str, secret: &str) -> String {
    let expiry = chrono::Utc::now().timestamp() + SESSION_TTL_SECS;
    let payload = format!("{user}:{expiry}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key size");
    mac.update(payload.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    let raw = format!("{payload}:{sig}");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes())
}

fn validate_token(token: &str, secret: &str) -> bool {
    let Ok(decoded) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token) else {
        return false;
    };
    let Ok(raw) = std::str::from_utf8(&decoded) else {
        return false;
    };
    let parts: Vec<&str> = raw.splitn(3, ':').collect();
    if parts.len() != 3 {
        return false;
    }
    let (user, expiry_str, sig) = (parts[0], parts[1], parts[2]);

    // Check expiry
    let Ok(expiry) = expiry_str.parse::<i64>() else {
        return false;
    };
    if chrono::Utc::now().timestamp() > expiry {
        return false;
    }

    // Verify HMAC
    let payload = format!("{user}:{expiry}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key size");
    mac.update(payload.as_bytes());
    let Ok(expected_sig) = hex::decode(sig) else {
        return false;
    };
    mac.verify_slice(&expected_sig).is_ok()
}

// ── Cookie parsing ──────────────────────────────────────────────────────

fn parse_cookie<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(name).and_then(|s| s.strip_prefix('=')) {
            return Some(value);
        }
    }
    None
}

// ── Basic Auth (kept for API consumers) ─────────────────────────────────

fn check_basic_auth(headers: &HeaderMap, expected_user: &str, expected_pass: &str) -> bool {
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
