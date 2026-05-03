//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Economind API Server
//!
//! Axum-based REST + GraphQL + WebSocket server.
//! Also serves the embedded SvelteKit web dashboard (Phase 6).
//!
//! # Usage
//! ```
//! ECONOMIND_API_KEY=secret DATABASE_URL=postgres://... economind-serve
//! ```

mod auth;
mod error;
mod events;
mod graphql;
mod pipeline_factory;
mod rest;
mod state;
mod ws;

use axum::{middleware, routing::get, Router};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::auth::require_api_key;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    // Initialise structured logging.
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "economind_api=debug,tower_http=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Configuration from environment.
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let duckdb_path = std::env::var("DUCKDB_PATH").unwrap_or_else(|_| ":memory:".to_string());
    let api_key = std::env::var("ECONOMIND_API_KEY")
        .map_err(|_| anyhow::anyhow!("ECONOMIND_API_KEY must be set"))?;
    let bind_addr: SocketAddr = std::env::var("ECONOMIND_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid ECONOMIND_BIND address: {e}"))?;

    tracing::info!("Connecting to database…");
    let state = AppState::new(&database_url, &duckdb_path, api_key).await?;

    // Build GraphQL schema.
    let gql_schema = graphql::build_schema(state.clone());

    // CORS — localhost only for self-hosted deployment.
    let cors = CorsLayer::new()
        .allow_origin([
            "http://localhost:8080".parse::<axum::http::HeaderValue>().unwrap(),
            "http://127.0.0.1:8080".parse::<axum::http::HeaderValue>().unwrap(),
        ])
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    // Protected routes — REST + GraphQL require a valid API key.
    let protected = Router::new()
        .nest("/api/v1", rest::router())
        .merge(graphql::router(gql_schema))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_api_key));

    let app = Router::new()
        .merge(protected)
        // WebSocket: auth is checked inside the handler.
        .merge(ws::router())
        // Health check — no auth required.
        .route("/health", get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    tracing::info!("Economind API listening on {bind_addr}");
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
