//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Economind API server library.
//!
//! The [`serve`] function is the single entrypoint for both the standalone
//! binary and the Tauri desktop app.  Callers are responsible for:
//!
//! - Calling `dotenvy::dotenv().ok()` before constructing config
//! - Initialising `tracing_subscriber` before calling `serve`

pub mod auth;
pub mod error;
pub mod events;
pub mod graphql;
pub mod pipeline_factory;
pub mod rest;
pub mod scheduler;
pub mod state;
pub mod ws;

use axum::{
    body::Body,
    http::{header, Response, StatusCode},
    middleware,
    response::IntoResponse,
    routing::get,
    Router,
};
use include_dir::{include_dir, Dir};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::require_api_key;
use crate::state::AppState;
use economind_config::EconomindConfig;

/// Embedded SvelteKit build output — compiled in at build time by `build.rs`.
static DASHBOARD: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../dashboard/build");

/// Serve a file from the embedded dashboard, with SPA fallback to `index.html`.
async fn serve_dashboard(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(file) = DASHBOARD.get_file(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(file.contents()))
            .unwrap()
    } else {
        if let Some(index) = DASHBOARD.get_file("index.html") {
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(Body::from(index.contents()))
                .unwrap()
        } else {
            Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Body::from(
                    "Dashboard not built. Run `npm run build` in dashboard/.",
                ))
                .unwrap()
        }
    }
}

/// Start the Economind API server.
///
/// Binds the TCP listener first, then — if `ready` is `Some` — sends the
/// bound [`SocketAddr`] on the channel before entering the serve loop.
/// This lets callers (e.g. the Tauri app) open the webview only after the
/// server is actually listening.
///
/// # Tracing
/// The caller must initialise a `tracing_subscriber` before calling this
/// function.  This function does not call `tracing_subscriber::init()`.
pub async fn serve(
    cfg: EconomindConfig,
    ready: Option<tokio::sync::oneshot::Sender<SocketAddr>>,
) -> anyhow::Result<()> {
    let duckdb_path = cfg.database.effective_duckdb_path();
    let api_key = std::env::var("ECONOMIND_API_KEY")
        .map_err(|_| anyhow::anyhow!("ECONOMIND_API_KEY must be set"))?;
    let bind_addr: SocketAddr = cfg
        .server
        .effective_bind()
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid bind address: {e}"))?;

    tracing::info!("Opening database at {duckdb_path}…");
    let state = AppState::new(&duckdb_path, api_key).await?;

    let gql_schema = graphql::build_schema(state.clone());

    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let protected = Router::new()
        .merge(rest::router())
        .merge(graphql::router(gql_schema))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_key,
        ));

    let api = Router::new()
        .merge(protected)
        .merge(ws::router())
        .route(
            "/health",
            get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }),
        );

    let app = Router::new()
        .nest("/api/v1", api)
        .fallback(serve_dashboard)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state.clone());

    scheduler::start(state);

    tracing::info!("Economind API listening on {bind_addr}");
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    let bound = listener.local_addr()?;

    // Notify the caller that the server is ready.
    if let Some(tx) = ready {
        let _ = tx.send(bound);
    }

    axum::serve(listener, app).await?;
    Ok(())
}
