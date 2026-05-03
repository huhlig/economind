//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Storage traits for strategy configs, runs, and signals (Phase 2).
//!
//! These traits are implemented by `PostgresStorage` (authoritative writes) and
//! forwarded through `DataStore`.  The `DataStore` routes writes to Postgres and
//! reads to Postgres as well (strategy data is small enough that DuckDB mirroring
//! is not needed until Phase 4 backtest work begins).

use crate::StorageResult;
use chrono::{DateTime, NaiveDate, Utc};
use economind_core::model::Symbol;
use rust_decimal::Decimal;
use uuid::Uuid;

// ── MacroSeries ───────────────────────────────────────────────────────────────

/// A single observation from a macro time series (e.g. FRED).
#[derive(Debug, Clone)]
pub struct MacroSeriesPoint {
    pub series_id: String,
    pub date: NaiveDate,
    pub value: Option<Decimal>,
    pub fetched_at: DateTime<Utc>,
}

/// Trait for reading macro series data (written by the ingest crate in Phase 3).
#[allow(async_fn_in_trait)]
pub trait MacroStorage: Send + Sync {
    /// Return the latest available value for each of the given series IDs.
    async fn get_latest_macro_values(
        &self,
        series_ids: &[&str],
    ) -> StorageResult<Vec<MacroSeriesPoint>>;

    /// Return all observations for a series within a date range.
    async fn query_macro_series(
        &self,
        series_id: &str,
        date_range: std::ops::Range<NaiveDate>,
    ) -> StorageResult<Vec<MacroSeriesPoint>>;
}

// ── PortfolioState ────────────────────────────────────────────────────────────

/// Current open position snapshot loaded for StrategyContext.
#[derive(Debug, Clone)]
pub struct OpenPosition {
    pub id: Uuid,
    pub symbol: Symbol,
    /// Positive = long, negative = short.
    pub shares: Decimal,
    pub entry_price: Decimal,
    pub entry_at: DateTime<Utc>,
}

/// Summary of the portfolio state loaded at the start of a strategy run.
#[derive(Debug, Clone)]
pub struct PortfolioState {
    pub open_positions: Vec<OpenPosition>,
    /// Sum of (current_price * shares) for all open positions + cash.
    pub portfolio_value: Decimal,
    pub available_cash: Decimal,
    /// Drawdown from peak (0.0 – 1.0).
    pub current_drawdown: Decimal,
}

/// Trait for reading portfolio state (written by the broker / execution layer).
#[allow(async_fn_in_trait)]
pub trait PortfolioStorage: Send + Sync {
    /// Load the current portfolio state for building StrategyContext.
    async fn load_portfolio_state(&self) -> StorageResult<PortfolioState>;
}

// ── StrategyStorage ───────────────────────────────────────────────────────────

/// Serialised strategy config row (matches `strategy.configs`).
#[derive(Debug, Clone)]
pub struct StrategyConfigRow {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub composition: String,
    pub plugins_json: String,   // JSON array of PluginSpec
    pub parameters_json: String, // JSON object
    pub enabled: bool,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Serialised strategy run row (matches `strategy.runs`).
#[derive(Debug, Clone)]
pub struct StrategyRunRow {
    pub id: Uuid,
    pub config_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub signal_count: i32,
    pub error_message: Option<String>,
    pub config_snapshot_json: String,
}

/// Serialised signal row (matches `strategy.signals`).
#[derive(Debug, Clone)]
pub struct StrategySignalRow {
    pub id: Uuid,
    pub run_id: Uuid,
    pub config_id: Uuid,
    pub symbol: String,
    pub direction: String,
    pub identifier_score: Decimal,
    pub timing_score: Decimal,
    pub position_shares: Option<Decimal>,
    pub position_notional: Option<Decimal>,
    pub portfolio_fraction: Option<Decimal>,
    pub rationale: Option<String>,
    pub analysis_brief: Option<String>,
    pub emitted_at: DateTime<Utc>,
}

/// Trait for persisting and reading strategy configs, runs, and signals.
#[allow(async_fn_in_trait)]
pub trait StrategyStorage: Send + Sync {
    // ── Configs ───────────────────────────────────────────────────────────────

    async fn insert_strategy_config(&self, row: &StrategyConfigRow) -> StorageResult<()>;
    async fn get_strategy_config(&self, id: Uuid) -> StorageResult<Option<StrategyConfigRow>>;
    async fn list_strategy_configs(&self) -> StorageResult<Vec<StrategyConfigRow>>;
    async fn update_strategy_config(&self, row: &StrategyConfigRow) -> StorageResult<()>;

    // ── Runs ──────────────────────────────────────────────────────────────────

    /// Insert a new run record (status = 'running').
    async fn insert_strategy_run(&self, row: &StrategyRunRow) -> StorageResult<()>;
    /// Update run status, completed_at, signal_count, and error_message.
    async fn complete_strategy_run(&self, row: &StrategyRunRow) -> StorageResult<()>;
    async fn get_strategy_run(&self, id: Uuid) -> StorageResult<Option<StrategyRunRow>>;
    async fn list_strategy_runs(
        &self,
        config_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<StrategyRunRow>>;

    // ── Signals ───────────────────────────────────────────────────────────────

    async fn insert_strategy_signals(&self, rows: &[StrategySignalRow]) -> StorageResult<()>;
    async fn query_strategy_signals(
        &self,
        run_id: Option<Uuid>,
        config_id: Option<Uuid>,
        symbol: Option<&Symbol>,
        since: Option<NaiveDate>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<StrategySignalRow>>;
    async fn get_strategy_signal(&self, id: Uuid) -> StorageResult<Option<StrategySignalRow>>;
}
