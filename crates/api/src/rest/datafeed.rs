//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Provider-specific datafeed fetch endpoints.

use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use economind_ingest::{RReichelFeed, TiingoFeed};
use serde::Serialize;

use tracing::info;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/datafeed/rreichel", post(fetch_rreichel))
        .route(
            "/datafeed/tiingo/{ticker}/metadata",
            post(fetch_tiingo_metadata),
        )
        .route("/datafeed/tiingo/{ticker}/prices", post(fetch_tiingo_prices))
}

#[derive(Debug, Serialize)]
pub struct DatafeedFetchResponse {
    pub status: String,
    pub provider: String,
    pub action: String,
    pub ticker: Option<String>,
    pub message: String,
}

async fn fetch_rreichel(State(state): State<AppState>) -> ApiResult<Json<DatafeedFetchResponse>> {
    info!("Datafeed: fetching RReichel ticker universe");
    let feed = RReichelFeed::new(state.store().duck().clone());
    feed.upsert_tickers()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    info!("Datafeed: RReichel fetch completed");

    Ok(Json(DatafeedFetchResponse {
        status: "completed".to_string(),
        provider: "rreichel".to_string(),
        action: "upsert_tickers".to_string(),
        ticker: None,
        message: "RReichel datafeed fetch completed.".to_string(),
    }))
}

async fn fetch_tiingo_metadata(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
) -> ApiResult<Json<DatafeedFetchResponse>> {
    info!("Datafeed: fetching Tiingo metadata for {ticker}");
    let feed = tiingo_feed(state.store().duck().clone())?;
    feed.fetch_ticker_metadata(&ticker)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    info!("Datafeed: Tiingo metadata fetch completed for {ticker}");

    Ok(Json(DatafeedFetchResponse {
        status: "completed".to_string(),
        provider: "tiingo".to_string(),
        action: "metadata".to_string(),
        ticker: Some(ticker.clone()),
        message: format!("Tiingo metadata fetch completed for {ticker}."),
    }))
}

async fn fetch_tiingo_prices(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
) -> ApiResult<Json<DatafeedFetchResponse>> {
    info!("Datafeed: fetching Tiingo prices for {ticker}");
    let feed = tiingo_feed(state.store().duck().clone())?;
    feed.fetch_ticker_prices(&ticker, None)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    info!("Datafeed: Tiingo prices fetch completed for {ticker}");

    Ok(Json(DatafeedFetchResponse {
        status: "completed".to_string(),
        provider: "tiingo".to_string(),
        action: "prices".to_string(),
        ticker: Some(ticker.clone()),
        message: format!("Tiingo price fetch completed for {ticker}."),
    }))
}

fn tiingo_feed(db: economind_db::DuckDatabase) -> ApiResult<TiingoFeed> {
    let api_key = std::env::var("TIINGO_API_KEY")
        .map_err(|_| ApiError::BadRequest("TIINGO_API_KEY must be set".to_string()))?;
    Ok(TiingoFeed::new(db, api_key))
}
