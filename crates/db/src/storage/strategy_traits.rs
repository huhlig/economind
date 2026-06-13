//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Storage traits for strategy configs, runs, signals, and backtest results (Phases 2 & 4).
//!
//! These traits are implemented by `DuckDatabase` and forwarded through `DataStore`.

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

/// Trait for reading and writing macro series data (written by the ingest crate in Phase 3).
#[allow(async_fn_in_trait)]
pub trait MacroStorage: Send + Sync {
    /// Upsert a batch of macro series observations (insert or update on conflict).
    async fn upsert_macro_series(&self, points: &[MacroSeriesPoint]) -> StorageResult<()>;

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

/// A symbol on the user's watch list.
#[derive(Debug, Clone)]
pub struct WatchItem {
    pub symbol: Symbol,
    pub added_at: DateTime<Utc>,
}

/// Trait for reading portfolio state (written by the broker / execution layer).
#[allow(async_fn_in_trait)]
pub trait PortfolioStorage: Send + Sync {
    /// Load the current portfolio state for building StrategyContext.
    async fn load_portfolio_state(&self) -> StorageResult<PortfolioState>;

    /// Fetch a single open position by its UUID. Returns `None` if not found or closed.
    async fn get_open_position(&self, id: Uuid) -> StorageResult<Option<OpenPosition>>;

    /// Open a new long or short position.
    async fn open_position(
        &self,
        symbol: &Symbol,
        shares: Decimal,
        entry_price: Decimal,
        entry_at: DateTime<Utc>,
    ) -> StorageResult<OpenPosition>;

    /// Close an open position by ID.
    async fn close_position(
        &self,
        id: Uuid,
        exit_price: Decimal,
        exit_at: DateTime<Utc>,
    ) -> StorageResult<()>;

    /// Set the available cash balance in today's portfolio equity snapshot.
    async fn set_cash(&self, cash: Decimal) -> StorageResult<()>;

    // ── Watchlist ─────────────────────────────────────────────────────────────

    async fn add_watch(&self, symbol: &Symbol) -> StorageResult<WatchItem>;
    async fn remove_watch(&self, symbol: &Symbol) -> StorageResult<()>;
    async fn list_watches(&self) -> StorageResult<Vec<WatchItem>>;
    async fn get_watch(&self, symbol: &Symbol) -> StorageResult<Option<WatchItem>>;
}

// ── StrategyStorage ───────────────────────────────────────────────────────────

/// Serialised strategy config row (matches `strategy.configs`).
#[derive(Debug, Clone)]
pub struct StrategyConfigRow {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub composition: String,
    pub plugins_json: String,    // JSON array of PluginSpec
    pub parameters_json: String, // JSON object
    pub enabled: bool,
    pub auto_execute: bool,
    /// "signal_only" | "paper" | "live"
    pub execution_mode: String,
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

// ── BacktestStorage ───────────────────────────────────────────────────────────

/// Serialised backtest run row (matches `backtest.runs`).
#[derive(Debug, Clone)]
pub struct BacktestRunRow {
    pub id: Uuid,
    pub config_id: Uuid,
    /// JSON snapshot of the StrategyConfig at the time of the run.
    pub config_snapshot_json: String,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    pub initial_capital: Decimal,
    pub final_capital: Option<Decimal>,
    // ── Performance metrics ───────────────────────────────────────────────────
    pub cagr: Option<Decimal>,
    pub sharpe_ratio: Option<Decimal>,
    pub sortino_ratio: Option<Decimal>,
    pub max_drawdown: Option<Decimal>,
    pub max_drawdown_days: Option<i32>,
    pub win_rate: Option<Decimal>,
    pub profit_factor: Option<Decimal>,
    pub expectancy: Option<Decimal>,
    pub total_trades: Option<i32>,
    pub avg_hold_days: Option<Decimal>,
    // ── Run lifecycle ─────────────────────────────────────────────────────────
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

/// Serialised backtest trade row (matches `backtest.trades`).
#[derive(Debug, Clone)]
pub struct BacktestTradeRow {
    pub id: Uuid,
    pub run_id: Uuid,
    pub symbol: String,
    /// `"long"` or `"short"`.
    pub direction: String,
    pub entry_date: NaiveDate,
    pub entry_price: Decimal,
    pub exit_date: Option<NaiveDate>,
    pub exit_price: Option<Decimal>,
    pub shares: Decimal,
    pub gross_pnl: Option<Decimal>,
    pub commission: Decimal,
    pub net_pnl: Option<Decimal>,
    pub hold_days: Option<i32>,
}

/// A single point on the equity curve (matches `backtest.equity_curve`).
#[derive(Debug, Clone)]
pub struct EquityCurvePoint {
    pub run_id: Uuid,
    pub date: NaiveDate,
    pub portfolio_value: Decimal,
    pub cash: Decimal,
    /// Drawdown from peak expressed as a fraction (0.0 – 1.0).
    pub drawdown: Decimal,
}

/// Trait for persisting and reading backtest runs, trades, and equity curves.
#[allow(async_fn_in_trait)]
pub trait BacktestStorage: Send + Sync {
    // ── Runs ──────────────────────────────────────────────────────────────────

    /// Insert a new backtest run record (status = 'running').
    async fn insert_backtest_run(&self, row: &BacktestRunRow) -> StorageResult<()>;

    /// Update a run to 'completed' or 'failed', filling in all metric fields.
    async fn complete_backtest_run(&self, row: &BacktestRunRow) -> StorageResult<()>;

    /// Fetch a single backtest run by ID.
    async fn get_backtest_run(&self, id: Uuid) -> StorageResult<Option<BacktestRunRow>>;

    /// List backtest runs, optionally filtered by strategy config ID, newest first.
    async fn list_backtest_runs(
        &self,
        config_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<BacktestRunRow>>;

    // ── Trades ────────────────────────────────────────────────────────────────

    /// Bulk-insert simulated trades for a backtest run.
    async fn insert_backtest_trades(&self, rows: &[BacktestTradeRow]) -> StorageResult<()>;

    /// Fetch all trades for a backtest run.
    async fn get_backtest_trades(&self, run_id: Uuid) -> StorageResult<Vec<BacktestTradeRow>>;

    // ── Equity curve ──────────────────────────────────────────────────────────

    /// Bulk-insert equity curve points for a backtest run.
    async fn insert_equity_curve(&self, points: &[EquityCurvePoint]) -> StorageResult<()>;

    /// Fetch the full equity curve for a backtest run (sorted by date asc).
    async fn get_equity_curve(&self, run_id: Uuid) -> StorageResult<Vec<EquityCurvePoint>>;
}

// ── ChatStorage ──────────────────────────────────────────────────────────────

/// Persisted agent chat session.
#[derive(Debug, Clone)]
pub struct ChatSessionRow {
    pub id: Uuid,
    pub title: String,
    pub persona_id: Option<String>,
    pub depth: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Persisted agent chat message.
#[derive(Debug, Clone)]
pub struct ChatMessageRow {
    pub id: Uuid,
    pub session_id: Uuid,
    pub ordinal: i32,
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// Trait for persisting and reading chat sessions and their messages.
#[allow(async_fn_in_trait)]
pub trait ChatStorage: Send + Sync {
    async fn upsert_chat_session(&self, row: &ChatSessionRow) -> StorageResult<()>;
    async fn list_chat_sessions(&self, limit: Option<u32>) -> StorageResult<Vec<ChatSessionRow>>;
    async fn get_chat_session(&self, id: Uuid) -> StorageResult<Option<ChatSessionRow>>;
    async fn insert_chat_messages(&self, rows: &[ChatMessageRow]) -> StorageResult<()>;
    async fn list_chat_messages(&self, session_id: Uuid) -> StorageResult<Vec<ChatMessageRow>>;
}
