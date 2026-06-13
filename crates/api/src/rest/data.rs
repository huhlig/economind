//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Data endpoints (§5.B.6):
//!
//! - `GET /api/v1/data/bars`          — OHLCV bars for a symbol
//! - `GET /api/v1/data/fundamentals`  — income / balance sheet / cashflow
//! - `GET /api/v1/data/macro`         — macro series values

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::NaiveDate;
use economind_core::model::Symbol;
use economind_db::{CandleStorage, MacroStorage, MetadataStorage};
use futures::StreamExt;
use serde::Deserialize;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/data/bars", get(get_bars))
        .route("/data/fundamentals", get(get_fundamentals))
        .route("/data/macro", get(get_macro))
}

// ── Query params ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BarsQuery {
    pub symbol: String,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub interval: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FundamentalsQuery {
    pub symbol: String,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
pub struct MacroQuery {
    /// Comma-separated list of FRED series IDs, e.g. `DGS10,UNRATE`.
    pub series: Option<String>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /api/v1/data/bars?symbol=AAPL&from=2024-01-01&to=2024-12-31`
async fn get_bars(
    State(state): State<AppState>,
    Query(q): Query<BarsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let symbol = Symbol::new(&q.symbol);
    let from = q
        .from
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(2020, 1, 1).unwrap());
    let to = q.to.unwrap_or_else(|| chrono::Utc::now().date_naive());

    let mut stream = state
        .store()
        .query_daily_candles(&symbol, from..to)
        .await
        .map_err(ApiError::Storage)?;

    let mut bars = Vec::new();
    while let Some(b) = stream.next().await {
        bars.push(serde_json::json!({
            "date": b.date,
            "open": b.open,
            "high": b.high,
            "low": b.low,
            "close": b.close,
            "volume": b.volume,
        }));
    }

    Ok(Json(serde_json::json!({
        "symbol": q.symbol,
        "from": from,
        "to": to,
        "interval": q.interval,
        "count": bars.len(),
        "bars": bars,
    })))
}

/// `GET /api/v1/data/fundamentals?symbol=AAPL`
async fn get_fundamentals(
    State(state): State<AppState>,
    Query(q): Query<FundamentalsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let symbol = Symbol::new(&q.symbol);
    let from = q
        .from
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(2018, 1, 1).unwrap());
    let to = q.to.unwrap_or_else(|| chrono::Utc::now().date_naive());

    let date_range = Some(from..to);

    let income = state
        .store()
        .query_income_statements(Some(&symbol), date_range.clone())
        .await
        .map_err(ApiError::Storage)?;

    let balance = state
        .store()
        .query_balance_sheets(Some(&symbol), date_range.clone())
        .await
        .map_err(ApiError::Storage)?;

    let cashflow = state
        .store()
        .query_cash_flow_statements(Some(&symbol), date_range)
        .await
        .map_err(ApiError::Storage)?;

    Ok(Json(serde_json::json!({
        "symbol": q.symbol,
        "income_statements": income.len(),
        "balance_sheets": balance.len(),
        "cash_flow_statements": cashflow.len(),
    })))
}

/// `GET /api/v1/data/macro?series=DGS10,UNRATE&from=2020-01-01`
async fn get_macro(
    State(state): State<AppState>,
    Query(q): Query<MacroQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let default_series = "DGS10,T10Y2Y,CPIAUCSL,UNRATE,VIXCLS,M2SL".to_string();
    let series_str = q.series.as_deref().unwrap_or(&default_series);
    let series_ids: Vec<&str> = series_str.split(',').map(str::trim).collect();

    let from = q
        .from
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(2020, 1, 1).unwrap());
    let to = q.to.unwrap_or_else(|| chrono::Utc::now().date_naive());

    let mut result = serde_json::Map::new();
    for series_id in &series_ids {
        let points = state
            .store()
            .query_macro_series(series_id, from..to)
            .await
            .map_err(ApiError::Storage)?;

        let pts: Vec<_> = points
            .iter()
            .map(|p| {
                serde_json::json!({
                    "date": p.date,
                    "value": p.value,
                })
            })
            .collect();
        result.insert(series_id.to_string(), serde_json::Value::Array(pts));
    }

    Ok(Json(serde_json::json!({
        "series": result,
        "from": from,
        "to": to,
    })))
}
