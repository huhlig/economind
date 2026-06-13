//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Backtest endpoints (§5.B.5):
//!
//! - `POST /api/v1/backtest/run`   — run a backtest (sync, returns results)
//! - `GET  /api/v1/backtest/:id`   — fetch results for a completed run
//! - `GET  /api/v1/backtest`       — list backtest runs

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::NaiveDate;
use economind_backtest::BacktestRunner;
use economind_db::{BacktestStorage, StrategyStorage};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/backtest", get(list_backtests))
        .route("/backtest/run", post(run_backtest))
        .route("/backtest/{id}", get(get_backtest))
}

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RunBacktestRequest {
    pub config_id: Uuid,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    #[serde(default = "default_capital")]
    pub initial_capital: Decimal,
    #[serde(default = "default_slippage")]
    pub slippage_bps: u32,
    #[serde(default = "default_commission")]
    pub commission_per_trade: Decimal,
    #[serde(default = "default_max_hold")]
    pub max_hold_days: u32,
    #[serde(default = "default_entry_threshold")]
    pub entry_score_threshold: f64,
    #[serde(default = "default_max_position")]
    pub max_position_pct: Decimal,
}

fn default_capital() -> Decimal {
    Decimal::new(100_000, 0)
}
fn default_slippage() -> u32 {
    5
}
fn default_commission() -> Decimal {
    Decimal::new(1, 0)
}
fn default_max_hold() -> u32 {
    30
}
fn default_entry_threshold() -> f64 {
    0.6
}
fn default_max_position() -> Decimal {
    Decimal::new(5, 2)
}

#[derive(Debug, Deserialize)]
pub struct ListBacktestQuery {
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct BacktestRunSummary {
    pub run_id: Uuid,
    pub config_id: Uuid,
    pub from_date: String,
    pub to_date: String,
    pub initial_capital: String,
    pub final_capital: String,
    pub cagr: String,
    pub sharpe_ratio: String,
    pub sortino_ratio: String,
    pub max_drawdown: String,
    pub max_drawdown_days: i32,
    pub total_trades: i32,
    pub win_rate: String,
    pub profit_factor: String,
    pub expectancy: String,
    pub run_at: String,
}

#[derive(Debug, Serialize)]
pub struct BacktestListItem {
    pub run_id: Uuid,
    pub config_id: Uuid,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub status: String,
    pub total_trades: i32,
    pub cagr: Option<String>,
    pub sharpe_ratio: Option<String>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `POST /api/v1/backtest/run`
async fn run_backtest(
    State(state): State<AppState>,
    Json(req): Json<RunBacktestRequest>,
) -> ApiResult<Json<BacktestRunSummary>> {
    // Load the strategy config.
    let config_row = state
        .store()
        .get_strategy_config(req.config_id)
        .await
        .map_err(ApiError::Storage)?
        .ok_or(ApiError::NotFound)?;

    let strategy_config = crate::pipeline_factory::strategy_config_from_row(config_row)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Build the pipeline from registered plugins.
    let pipeline = crate::pipeline_factory::build_pipeline(&strategy_config)
        .map_err(|e| ApiError::BadRequest(format!("failed to build pipeline: {e}")))?;

    let runner = BacktestRunner::builder()
        .strategy_config(strategy_config)
        .pipeline(pipeline)
        .from_date(req.from_date)
        .to_date(req.to_date)
        .initial_capital(req.initial_capital)
        .slippage_bps(req.slippage_bps)
        .commission_per_trade(req.commission_per_trade)
        .max_hold_days(req.max_hold_days)
        .entry_score_threshold(req.entry_score_threshold)
        .max_position_pct(req.max_position_pct)
        .build();

    let result = runner
        .run(state.store())
        .await
        .map_err(|e| ApiError::Internal(format!("backtest failed: {e}")))?;

    let m = &result.metrics;
    Ok(Json(BacktestRunSummary {
        run_id: result.run_id,
        config_id: req.config_id,
        from_date: req.from_date.to_string(),
        to_date: req.to_date.to_string(),
        initial_capital: m.initial_capital.to_string(),
        final_capital: m.final_capital.to_string(),
        cagr: m.cagr.to_string(),
        sharpe_ratio: m.sharpe_ratio.to_string(),
        sortino_ratio: m.sortino_ratio.to_string(),
        max_drawdown: m.max_drawdown.to_string(),
        max_drawdown_days: m.max_drawdown_days,
        total_trades: m.total_trades,
        win_rate: m.win_rate.to_string(),
        profit_factor: m.profit_factor.to_string(),
        expectancy: m.expectancy.to_string(),
        run_at: chrono::Utc::now().to_rfc3339(),
    }))
}

/// `GET /api/v1/backtest`
async fn list_backtests(
    State(state): State<AppState>,
    Query(q): Query<ListBacktestQuery>,
) -> ApiResult<Json<Vec<BacktestListItem>>> {
    let rows = state
        .store()
        .list_backtest_runs(None, q.limit.or(Some(50)))
        .await
        .map_err(ApiError::Storage)?;

    let items = rows
        .into_iter()
        .map(|r| BacktestListItem {
            run_id: r.id,
            config_id: r.config_id,
            started_at: r.started_at.to_rfc3339(),
            completed_at: r.completed_at.map(|d| d.to_rfc3339()),
            status: r.status,
            total_trades: r.total_trades.unwrap_or(0),
            cagr: r.cagr.map(|d| d.to_string()),
            sharpe_ratio: r.sharpe_ratio.map(|d| d.to_string()),
        })
        .collect();

    Ok(Json(items))
}

/// `GET /api/v1/backtest/:id`
async fn get_backtest(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let run = state
        .store()
        .get_backtest_run(id)
        .await
        .map_err(ApiError::Storage)?
        .ok_or(ApiError::NotFound)?;

    let trades = state
        .store()
        .get_backtest_trades(id)
        .await
        .map_err(ApiError::Storage)?;

    let equity_curve = state
        .store()
        .get_equity_curve(id)
        .await
        .map_err(ApiError::Storage)?;

    Ok(Json(serde_json::json!({
        "run_id": run.id,
        "config_id": run.config_id,
        "started_at": run.started_at,
        "completed_at": run.completed_at,
        "status": run.status,
        "initial_capital": run.initial_capital,
        "final_capital": run.final_capital,
        "cagr": run.cagr,
        "sharpe_ratio": run.sharpe_ratio,
        "sortino_ratio": run.sortino_ratio,
        "max_drawdown": run.max_drawdown,
        "max_drawdown_days": run.max_drawdown_days,
        "total_trades": run.total_trades,
        "win_rate": run.win_rate,
        "profit_factor": run.profit_factor,
        "expectancy": run.expectancy,
        "trade_count": trades.len(),
        "trades": trades.iter().take(500).map(|t| serde_json::json!({
            "id": t.id,
            "symbol": t.symbol,
            "direction": t.direction,
            "entry_date": t.entry_date,
            "exit_date": t.exit_date,
            "entry_price": t.entry_price,
            "exit_price": t.exit_price,
            "shares": t.shares,
            "realized_pnl": t.net_pnl,
            "hold_days": t.hold_days,
        })).collect::<Vec<_>>(),
        "equity_curve_points": equity_curve.len(),
        "equity_curve": equity_curve.iter().take(1000).map(|p| serde_json::json!({
            "date": p.date,
            "value": p.portfolio_value,
        })).collect::<Vec<_>>(),
    })))
}
