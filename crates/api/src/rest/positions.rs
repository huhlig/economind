//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Position and watchlist endpoints:
//!
//! - `GET  /api/v1/positions`          — current open positions
//! - `POST /api/v1/positions/buy`      — open a new position
//! - `POST /api/v1/positions/{id}/sell`— close an open position
//! - `GET  /api/v1/positions/history`  — closed positions (stub until broker integration)
//! - `GET  /api/v1/positions/{id}`     — single position by ID
//! - `GET  /api/v1/watchlist`          — list watched symbols
//! - `POST /api/v1/watchlist`          — add a symbol to the watchlist
//! - `DELETE /api/v1/watchlist/{symbol}` — remove a symbol from the watchlist

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use economind_db::PortfolioStorage;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/positions", get(list_open_positions))
        .route("/positions/buy", post(buy_position))
        .route("/positions/history", get(position_history))
        .route("/positions/{id}", get(get_position))
        .route("/positions/{id}/sell", post(sell_position))
        .route("/watchlist", get(list_watchlist))
        .route("/watchlist", post(add_watchlist))
        .route("/watchlist/{symbol}", delete(remove_watchlist))
}

// ── Response / Request types ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PositionResponse {
    pub id: Uuid,
    pub symbol: String,
    pub shares: String,
    pub entry_price: String,
    pub entry_at: String,
}

#[derive(Debug, Serialize)]
pub struct PortfolioSummary {
    pub portfolio_value: String,
    pub available_cash: String,
    pub current_drawdown: String,
    pub open_positions: Vec<PositionResponse>,
}

#[derive(Debug, Deserialize)]
pub struct BuyRequest {
    pub symbol: String,
    pub shares: String,
    pub entry_price: String,
    /// ISO 8601. Defaults to now if omitted.
    pub entry_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SellRequest {
    pub exit_price: String,
    /// ISO 8601. Defaults to now if omitted.
    pub exit_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddWatchRequest {
    pub symbol: String,
}

#[derive(Debug, Serialize)]
pub struct WatchResponse {
    pub symbol: String,
    pub added_at: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /api/v1/positions`
async fn list_open_positions(State(state): State<AppState>) -> ApiResult<Json<PortfolioSummary>> {
    let portfolio = state
        .store()
        .load_portfolio_state()
        .await
        .map_err(ApiError::Storage)?;

    let positions = portfolio
        .open_positions
        .iter()
        .map(|p| PositionResponse {
            id: p.id,
            symbol: p.symbol.as_str().to_string(),
            shares: p.shares.to_string(),
            entry_price: p.entry_price.to_string(),
            entry_at: p.entry_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(PortfolioSummary {
        portfolio_value: portfolio.portfolio_value.to_string(),
        available_cash: portfolio.available_cash.to_string(),
        current_drawdown: portfolio.current_drawdown.to_string(),
        open_positions: positions,
    }))
}

/// `POST /api/v1/positions/buy`
async fn buy_position(
    State(state): State<AppState>,
    Json(req): Json<BuyRequest>,
) -> ApiResult<Json<PositionResponse>> {
    let symbol = economind_core::model::Symbol::new(&req.symbol);

    let shares = Decimal::from_str(&req.shares)
        .map_err(|_| ApiError::BadRequest("Invalid shares value".to_string()))?;
    if shares <= Decimal::ZERO {
        return Err(ApiError::BadRequest("shares must be positive".to_string()));
    }

    let entry_price = Decimal::from_str(&req.entry_price)
        .map_err(|_| ApiError::BadRequest("Invalid entry_price value".to_string()))?;
    if entry_price <= Decimal::ZERO {
        return Err(ApiError::BadRequest("entry_price must be positive".to_string()));
    }

    let entry_at: DateTime<Utc> = req
        .entry_at
        .as_deref()
        .map(|s| DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc)))
        .transpose()
        .map_err(|_| ApiError::BadRequest("Invalid entry_at timestamp".to_string()))?
        .unwrap_or_else(Utc::now);

    let pos = state
        .store()
        .open_position(&symbol, shares, entry_price, entry_at)
        .await
        .map_err(ApiError::Storage)?;

    Ok(Json(PositionResponse {
        id: pos.id,
        symbol: pos.symbol.as_str().to_string(),
        shares: pos.shares.to_string(),
        entry_price: pos.entry_price.to_string(),
        entry_at: pos.entry_at.to_rfc3339(),
    }))
}

/// `POST /api/v1/positions/{id}/sell`
async fn sell_position(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<SellRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let exit_price = Decimal::from_str(&req.exit_price)
        .map_err(|_| ApiError::BadRequest("Invalid exit_price value".to_string()))?;
    if exit_price <= Decimal::ZERO {
        return Err(ApiError::BadRequest("exit_price must be positive".to_string()));
    }

    let exit_at: DateTime<Utc> = req
        .exit_at
        .as_deref()
        .map(|s| DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc)))
        .transpose()
        .map_err(|_| ApiError::BadRequest("Invalid exit_at timestamp".to_string()))?
        .unwrap_or_else(Utc::now);

    state
        .store()
        .close_position(id, exit_price, exit_at)
        .await
        .map_err(ApiError::Storage)?;

    Ok(Json(serde_json::json!({ "status": "closed", "id": id })))
}

/// `GET /api/v1/positions/history`
async fn position_history(State(_state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "positions": [],
        "note": "closed position history available after broker integration (Phase 8)"
    })))
}

/// `GET /api/v1/positions/{id}`
async fn get_position(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PositionResponse>> {
    let portfolio = state
        .store()
        .load_portfolio_state()
        .await
        .map_err(ApiError::Storage)?;

    let pos = portfolio
        .open_positions
        .iter()
        .find(|p| p.id == id)
        .ok_or(ApiError::NotFound)?;

    Ok(Json(PositionResponse {
        id: pos.id,
        symbol: pos.symbol.as_str().to_string(),
        shares: pos.shares.to_string(),
        entry_price: pos.entry_price.to_string(),
        entry_at: pos.entry_at.to_rfc3339(),
    }))
}

// ── Watchlist handlers ────────────────────────────────────────────────────────

/// `GET /api/v1/watchlist`
async fn list_watchlist(State(state): State<AppState>) -> ApiResult<Json<Vec<WatchResponse>>> {
    let watches = state
        .store()
        .list_watches()
        .await
        .map_err(ApiError::Storage)?;

    Ok(Json(
        watches
            .into_iter()
            .map(|w| WatchResponse {
                symbol: w.symbol.as_str().to_string(),
                added_at: w.added_at.to_rfc3339(),
            })
            .collect(),
    ))
}

/// `POST /api/v1/watchlist`
async fn add_watchlist(
    State(state): State<AppState>,
    Json(req): Json<AddWatchRequest>,
) -> ApiResult<Json<WatchResponse>> {
    let symbol = economind_core::model::Symbol::new(&req.symbol);
    let item = state
        .store()
        .add_watch(&symbol)
        .await
        .map_err(ApiError::Storage)?;

    Ok(Json(WatchResponse {
        symbol: item.symbol.as_str().to_string(),
        added_at: item.added_at.to_rfc3339(),
    }))
}

/// `DELETE /api/v1/watchlist/{symbol}`
async fn remove_watchlist(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let sym = economind_core::model::Symbol::new(&symbol);
    state
        .store()
        .remove_watch(&sym)
        .await
        .map_err(ApiError::Storage)?;

    Ok(Json(serde_json::json!({ "status": "removed", "symbol": symbol })))
}
