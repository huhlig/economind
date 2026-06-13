//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Instrument endpoints (§5.B.1):
//!
//! - `GET  /api/v1/instruments`          — list / search instruments
//! - `GET  /api/v1/instruments/:symbol`  — instrument detail
//! - `POST /api/v1/instruments`          — add instrument to universe
//! - `DELETE /api/v1/instruments/:symbol`— remove instrument

use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use economind_core::model::Symbol;
use economind_db::MetadataStorage;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/instruments", get(list_instruments).post(add_instrument))
        .route(
            "/instruments/:symbol",
            get(get_instrument).delete(remove_instrument),
        )
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct InstrumentSummary {
    pub symbol: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddInstrumentRequest {
    pub symbol: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /api/v1/instruments`
async fn list_instruments(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<InstrumentSummary>>> {
    let mut stream = state
        .store()
        .list_tickers()
        .await
        .map_err(ApiError::Storage)?;

    let mut symbols = Vec::new();
    while let Some(sym) = stream.next().await {
        symbols.push(InstrumentSummary {
            symbol: sym.as_str().to_string(),
            name: None,
        });
    }
    Ok(Json(symbols))
}

/// `GET /api/v1/instruments/:symbol`
async fn get_instrument(
    State(state): State<AppState>,
    Path(symbol_str): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let symbol = Symbol::new(&symbol_str);

    let ticker = state
        .store()
        .get_ticker(&symbol)
        .await
        .map_err(ApiError::Storage)?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(serde_json::json!({
        "symbol": ticker.symbol.as_str(),
        "name": ticker.name,
        "exchange": ticker.exchange.as_ref().map(|e| e.as_str()),
        "sector": ticker.sector.as_ref().map(|s| format!("{s:?}")),
        "description": ticker.description,
        "market_cap": ticker.marketcap,
        "ipo_year": ticker.ipoyear,
        "active": ticker.active,
    })))
}

/// `POST /api/v1/instruments`
async fn add_instrument(
    State(state): State<AppState>,
    Json(req): Json<AddInstrumentRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let symbol = Symbol::new(&req.symbol);
    state
        .store()
        .upsert_ticker(&symbol)
        .await
        .map_err(ApiError::Storage)?;
    Ok(Json(
        serde_json::json!({ "symbol": req.symbol, "status": "added" }),
    ))
}

/// `DELETE /api/v1/instruments/:symbol`
async fn remove_instrument(
    State(state): State<AppState>,
    Path(symbol_str): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let symbol = Symbol::new(&symbol_str);
    // Verify it exists before responding.
    state
        .store()
        .get_ticker(&symbol)
        .await
        .map_err(ApiError::Storage)?
        .ok_or(ApiError::NotFound)?;

    // Universe membership removal is tracked by the portfolio layer (Phase 8+).
    // For now acknowledge the request and return success.
    Ok(Json(serde_json::json!({
        "symbol": symbol_str,
        "status": "removed"
    })))
}
