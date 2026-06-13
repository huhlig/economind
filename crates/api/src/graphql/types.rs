//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! GraphQL output types — thin wrappers over the DB row types.

use async_graphql::{SimpleObject, ID};

// ── Instrument ────────────────────────────────────────────────────────────────

#[derive(SimpleObject)]
pub struct GqlInstrument {
    pub symbol: String,
    pub name: Option<String>,
    pub exchange: Option<String>,
    pub sector: Option<String>,
}

// ── Signal ────────────────────────────────────────────────────────────────────

#[derive(SimpleObject)]
pub struct GqlSignal {
    pub id: ID,
    pub run_id: ID,
    pub config_id: ID,
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

// ── Strategy config ───────────────────────────────────────────────────────────

#[derive(SimpleObject)]
pub struct GqlStrategyConfig {
    pub id: ID,
    pub name: String,
    pub description: Option<String>,
    pub composition: String,
    pub plugins: String,    // JSON string — clients can parse it
    pub parameters: String, // JSON string
    pub enabled: bool,
    pub version: i32,
    pub created_at: String,
    pub updated_at: String,
}

// ── Backtest run ──────────────────────────────────────────────────────────────

#[derive(SimpleObject)]
pub struct GqlBacktestRun {
    pub id: ID,
    pub config_id: ID,
    pub status: String,
    pub from_date: String,
    pub to_date: String,
    pub initial_capital: Option<String>,
    pub final_capital: Option<String>,
    pub cagr: Option<String>,
    pub sharpe_ratio: Option<String>,
    pub sortino_ratio: Option<String>,
    pub max_drawdown: Option<String>,
    pub max_drawdown_days: Option<i32>,
    pub total_trades: Option<i32>,
    pub win_rate: Option<String>,
    pub profit_factor: Option<String>,
    pub expectancy: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

// ── Position ──────────────────────────────────────────────────────────────────

#[derive(SimpleObject)]
pub struct GqlPosition {
    pub id: ID,
    pub symbol: String,
    pub shares: String,
    pub entry_price: String,
    pub entry_at: String,
}

// ── Portfolio ─────────────────────────────────────────────────────────────────

#[derive(SimpleObject)]
pub struct GqlPortfolio {
    pub portfolio_value: String,
    pub available_cash: String,
    pub current_drawdown: String,
    pub open_positions: Vec<GqlPosition>,
}

// ── Mutation responses ────────────────────────────────────────────────────────

#[derive(SimpleObject)]
pub struct TriggerRunResult {
    pub run_id: ID,
    pub config_id: ID,
    pub status: String,
    pub started_at: String,
}

#[derive(SimpleObject)]
pub struct AddInstrumentResult {
    pub symbol: String,
    pub status: String,
}
