use std::sync::Arc;

use axum::extract::FromRef;
use axum::http::header::HeaderMap;

use crate::s3::S3Ctx;

#[derive(Clone)]
pub struct AppState {
    pub s3: S3Ctx,
    pub auth: Arc<AuthConfig>,
}

impl FromRef<AppState> for S3Ctx {
    fn from_ref(s: &AppState) -> Self {
        s.s3.clone()
    }
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub user: Option<String>,
    pub pass: Option<String>,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        Self {
            user: std::env::var("EXPLORER_USER").ok().filter(|s| !s.is_empty()),
            pass: std::env::var("EXPLORER_PASS").ok().filter(|s| !s.is_empty()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.user.is_some() && self.pass.is_some()
    }
}

pub fn public_base_url(headers: &HeaderMap) -> String {
    if let Ok(v) = std::env::var("PUBLIC_BASE_URL") {
        if !v.trim().is_empty() {
            return v.trim_end_matches('/').to_string();
        }
    }
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("https");
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!("{scheme}://{host}")
}
