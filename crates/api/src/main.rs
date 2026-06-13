//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Economind API Server — REST + GraphQL + WebSocket + embedded dashboard.

mod auth;
mod error;
mod events;
mod graphql;
mod pipeline_factory;
mod rest;
mod scheduler;
mod state;
mod ws;

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
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::auth::require_api_key;
use crate::state::AppState;
use economind_config::EconomindConfig;

/// Embedded SvelteKit build output.
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
        // SPA fallback — let the client-side router handle unknown paths.
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let cfg = EconomindConfig::load()?;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "economind_api=debug,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

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
        .allow_origin([
            "http://localhost:8080"
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
            "http://127.0.0.1:8080"
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        ])
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let protected = Router::new()
        .nest("/api/v1", rest::router())
        .merge(graphql::router(gql_schema))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_key,
        ));

    let app = Router::new()
        .merge(protected)
        .merge(ws::router())
        .route(
            "/health",
            get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }),
        )
        // Embedded SvelteKit dashboard — catch-all, must be last.
        .fallback(serve_dashboard)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state.clone());

    // Start background scheduler (nightly ingestion + strategy runs).
    scheduler::start(state);

    tracing::info!("Economind API listening on {bind_addr}");
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
