//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Position endpoints (§5.B.3):
//!
//! - `GET /api/v1/positions`          — current open positions
//! - `GET /api/v1/positions/history`  — closed positions (future: requires broker integration)
//! - `GET /api/v1/positions/:id`      — single position by ID

use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use economind_db::PortfolioStorage;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/positions", get(list_open_positions))
        .route("/positions/history", get(position_history))
        .route("/positions/:id", get(get_position))
}

// ── Response types ────────────────────────────────────────────────────────────

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

/// `GET /api/v1/positions/history`
/// Returns a placeholder — full closed-position history requires the broker
/// execution layer (Phase 8+).
async fn position_history(State(_state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "positions": [],
        "note": "closed position history available after broker integration (Phase 8)"
    })))
}

/// `GET /api/v1/positions/:id`
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
