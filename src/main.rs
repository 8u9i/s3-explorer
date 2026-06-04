mod auth;
mod error;
mod routes;
mod s3;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::auth::require_auth;
use crate::routes::copy as copy_route;
use crate::routes::delete as delete_route;
use crate::routes::download as download_route;
use crate::routes::edit as edit_route;
use crate::routes::health as health_route;
use crate::routes::list as list_route;
use crate::routes::preview as preview_route;
use crate::routes::thumb as thumb_route;
use crate::routes::upload as upload_route;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    init_tracing();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "starting s3-explorer"
    );

    let s3 = s3::build_context().await.context("initialising S3 client")?;
    let auth = Arc::new(state::AuthConfig::from_env());
    if auth.is_enabled() {
        tracing::info!("basic auth enabled");
    } else {
        tracing::warn!(
            "EXPLORER_USER / EXPLORER_PASS not set — explorer is publicly accessible. \
             This is fine only when behind Railway Private Networking."
        );
    }
    let app_state = AppState { s3, auth };

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();

    let app = build_router(app_state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    tracing::info!(%addr, "listening");
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server crashed")?;
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,s3_explorer=debug,tower_http=info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .compact(),
        )
        .init();
}

fn build_router(state: AppState) -> Router {
    let public = Router::new()
        .route("/", get(redirect_to_browse))
        .route("/health", get(health_route::health))
        .route("/ready", get(health_route::ready))
        .route("/version", get(health_route::version))
        .route("/static/{*path}", get(serve_static))
        .route("/favicon.ico", get(serve_favicon));

    let api = Router::new()
        .route("/api/objects", get(list_route::list_objects))
        .route("/api/objects/{*key}", get(preview_route::raw_object))
        .route("/api/objects/{*key}", put(edit_route::put_object))
        .route("/api/objects/{*key}", delete(delete_route::delete_object))
        .route("/api/thumb/{*key}", get(thumb_route::thumb))
        .route("/api/upload", post(upload_route::upload_multipart))
        .route("/api/upload/stream", put(upload_route::upload_stream))
        .route("/api/presign", get(download_route::presign_get))
        .route("/api/presign/{*key}", get(download_route::presign_put))
        .route("/api/copy", post(copy_route::copy_object))
        .route("/api/move", post(copy_route::move_object))
        .route("/api/delete-prefix/{*prefix}", delete(delete_route::delete_prefix));

    let browse = Router::new()
        .route("/browse", get(list_route::list_objects))
        .route("/upload", post(upload_route::upload_multipart))
        .route("/files/{*key}", get(download_route::proxy_get))
        .route("/preview/{*key}", get(preview_route::preview))
        .route("/edit", get(edit_route::edit_form))
        .route("/replace", post(upload_route::upload_form))
        .route("/copy", get(copy_route::copy_form))
        .route("/copy", post(copy_route::copy_object));

    let merged = public
        .merge(browse)
        .merge(api)
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ));

    merged
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::header::HeaderName::from_static("x-content-type-options"),
            axum::http::HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::header::HeaderName::from_static("referrer-policy"),
            axum::http::HeaderValue::from_static("no-referrer"),
        ))
        .with_state(state)
}

async fn serve_static(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use axum::http::header;
    use axum::response::IntoResponse;
    let p = path.trim_start_matches('/');
    if p.is_empty() || p.contains("..") {
        return Err(axum::http::StatusCode::NOT_FOUND);
    }
    let bytes = match p {
        "app.js" => include_bytes!("../static/app.js").to_vec(),
        "app.css" => include_bytes!("../static/app.css").to_vec(),
        "favicon.svg" => include_bytes!("../static/favicon.svg").to_vec(),
        _ => return Err(axum::http::StatusCode::NOT_FOUND),
    };
    let ct = if p.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if p.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if p.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    };
    let mut resp = bytes.into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        ct.parse().unwrap(),
    );
    Ok(resp)
}

async fn serve_favicon() -> impl axum::response::IntoResponse {
    let bytes = include_bytes!("../static/favicon.svg").to_vec();
    (
        [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
        bytes,
    )
}

async fn redirect_to_browse() -> impl axum::response::IntoResponse {
    axum::response::Redirect::permanent("/browse")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        term.recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("ctrl_c received, shutting down"),
        _ = terminate => tracing::info!("SIGTERM received, shutting down"),
    }
}
