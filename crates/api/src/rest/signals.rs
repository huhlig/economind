//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Signal endpoints (§5.B.2):
//!
//! - `GET /api/v1/signals`      — paginated, filterable list
//! - `GET /api/v1/signals/:id`  — single signal by ID

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::NaiveDate;
use economind_core::model::Symbol;
use economind_db::StrategyStorage;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/signals", get(list_signals))
        .route("/signals/{id}", get(get_signal))
}

// ── Query params ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SignalQuery {
    pub strategy: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub symbol: Option<String>,
    pub since: Option<NaiveDate>,
    pub limit: Option<u32>,
    pub min_score: Option<f64>,
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SignalResponse {
    pub id: Uuid,
    pub run_id: Uuid,
    pub config_id: Uuid,
    pub symbol: String,
    pub direction: String,
    pub identifier_score: String,
    pub timing_score: String,
    pub position_shares: Option<String>,
    pub position_notional: Option<String>,
    pub portfolio_fraction: Option<String>,
    pub rationale: Option<String>,
    pub analysis_brief: Option<String>,
    pub emitted_at: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /api/v1/signals`
async fn list_signals(
    State(state): State<AppState>,
    Query(q): Query<SignalQuery>,
) -> ApiResult<Json<Vec<SignalResponse>>> {
    let symbol = q.symbol.as_deref().map(Symbol::new);
    let rows = state
        .store()
        .query_strategy_signals(
            q.run_id,
            q.strategy,
            symbol.as_ref(),
            q.since,
            q.limit.or(Some(100)),
        )
        .await
        .map_err(ApiError::Storage)?;

    let mut signals: Vec<SignalResponse> = rows
        .into_iter()
        .map(|r| SignalResponse {
            id: r.id,
            run_id: r.run_id,
            config_id: r.config_id,
            symbol: r.symbol,
            direction: r.direction,
            identifier_score: r.identifier_score.to_string(),
            timing_score: r.timing_score.to_string(),
            position_shares: r.position_shares.map(|d| d.to_string()),
            position_notional: r.position_notional.map(|d| d.to_string()),
            portfolio_fraction: r.portfolio_fraction.map(|d| d.to_string()),
            rationale: r.rationale,
            analysis_brief: r.analysis_brief,
            emitted_at: r.emitted_at.to_rfc3339(),
        })
        .collect();

    // Optional client-side score filter.
    if let Some(min) = q.min_score {
        signals.retain(|s| {
            s.timing_score
                .parse::<f64>()
                .map(|v| v >= min)
                .unwrap_or(false)
        });
    }

    Ok(Json(signals))
}

/// `GET /api/v1/signals/:id`
async fn get_signal(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SignalResponse>> {
    let row = state
        .store()
        .get_strategy_signal(id)
        .await
        .map_err(ApiError::Storage)?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(SignalResponse {
        id: row.id,
        run_id: row.run_id,
        config_id: row.config_id,
        symbol: row.symbol,
        direction: row.direction,
        identifier_score: row.identifier_score.to_string(),
        timing_score: row.timing_score.to_string(),
        position_shares: row.position_shares.map(|d| d.to_string()),
        position_notional: row.position_notional.map(|d| d.to_string()),
        portfolio_fraction: row.portfolio_fraction.map(|d| d.to_string()),
        rationale: row.rationale,
        analysis_brief: row.analysis_brief,
        emitted_at: row.emitted_at.to_rfc3339(),
    }))
}
