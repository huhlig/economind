//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `DataStore` — thin façade over a single `DuckDatabase`.
//!
//! All data lives in DuckDB. Call `preload(days)` before each strategy run to
//! materialise hot tables into an attached `:memory:` schema for fast reads.

use crate::storage::{
    BacktestRunRow, BacktestStorage, BacktestTradeRow, CandleStorage, ChatMessageRow,
    ChatSessionRow, ChatStorage, DuckDatabase, EquityCurvePoint, MacroSeriesPoint, MacroStorage,
    MetadataStorage, OpenPosition, PortfolioState, PortfolioStorage, StrategyConfigRow,
    StrategyRunRow, StrategySignalRow, StrategyStorage, TickStorage, TickerQuery, WatchItem,
};
use crate::StorageResult;
use chrono::{NaiveDate, NaiveDateTime};
use economind_core::model::{
    BalanceSheet, CandleEntry, CashFlowStatement, DailyCandleEntry, DividendEvent, IncomeStatement,
    Interval, NewsStory, StockSplitEvent, Symbol, Ticker, TradeTick,
};
use futures_core::stream::BoxStream;
use std::ops::Range;
use uuid::Uuid;

// ── DataStore ────────────────────────────────────────────────────────────────

/// Unified data access façade backed by a single embedded DuckDB instance.
#[derive(Clone)]
pub struct DataStore {
    duck: DuckDatabase,
}

impl DataStore {
    /// Open (or create) a DuckDB database at `path`.
    pub fn open(path: &str) -> StorageResult<Self> {
        let duck = if path == ":memory:" {
            DuckDatabase::in_memory()?
        } else {
            DuckDatabase::open(path)?
        };
        Ok(Self { duck })
    }

    /// Materialise hot strategy tables into an attached `:memory:` schema.
    ///
    /// Call once before each strategy run. Subsequent calls refresh the cache.
    pub async fn preload(&self, lookback_days: u32) -> StorageResult<()> {
        self.duck.preload(lookback_days).await
    }

    /// Whether the in-memory cache is active.
    pub fn is_preloaded(&self) -> bool {
        self.duck.is_preloaded()
    }

    /// Direct access to the underlying `DuckDatabase` (for raw SQL batches).
    pub fn duck(&self) -> &DuckDatabase {
        &self.duck
    }

    /// Read a runtime setting from DuckDB.
    pub async fn get_setting(&self, key: &str) -> StorageResult<Option<String>> {
        self.duck.get_setting(key).await
    }

    /// Upsert a runtime setting in DuckDB.
    pub async fn set_setting(&self, key: &str, value: &str) -> StorageResult<()> {
        self.duck.set_setting(key, value).await
    }
}

// ── MetadataStorage ──────────────────────────────────────────────────────────

impl MetadataStorage for DataStore {
    async fn list_tickers(&self) -> StorageResult<BoxStream<'static, Symbol>> {
        self.duck.list_tickers().await
    }
    async fn query_tickers<'a>(
        &'a self,
        query: TickerQuery,
    ) -> StorageResult<BoxStream<'a, Ticker>> {
        self.duck.query_tickers(query).await
    }
    async fn get_ticker(&self, symbol: &Symbol) -> StorageResult<Option<Ticker>> {
        self.duck.get_ticker(symbol).await
    }
    async fn upsert_ticker(&self, symbol: &Symbol) -> StorageResult<()> {
        self.duck.upsert_ticker(symbol).await
    }
    async fn insert_news(&self, items: &[NewsStory]) -> StorageResult<()> {
        self.duck.insert_news(items).await
    }
    async fn query_news<'a>(
        &'a self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<BoxStream<'a, NewsStory>> {
        self.duck.query_news(symbol, time_range).await
    }
    async fn insert_income_statements(&self, items: &[IncomeStatement]) -> StorageResult<()> {
        self.duck.insert_income_statements(items).await
    }
    async fn query_income_statements(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<IncomeStatement>> {
        self.duck.query_income_statements(symbol, time_range).await
    }
    async fn insert_balance_sheets(&self, items: &[BalanceSheet]) -> StorageResult<()> {
        self.duck.insert_balance_sheets(items).await
    }
    async fn query_balance_sheets(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<BalanceSheet>> {
        self.duck.query_balance_sheets(symbol, time_range).await
    }
    async fn insert_cash_flow_statements(&self, items: &[CashFlowStatement]) -> StorageResult<()> {
        self.duck.insert_cash_flow_statements(items).await
    }
    async fn query_cash_flow_statements(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<CashFlowStatement>> {
        self.duck
            .query_cash_flow_statements(symbol, time_range)
            .await
    }
    async fn insert_dividend_report(&self, items: &[DividendEvent]) -> StorageResult<()> {
        self.duck.insert_dividend_report(items).await
    }
    async fn query_dividend_report(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<DividendEvent>> {
        self.duck.query_dividend_report(symbol, time_range).await
    }
    async fn insert_stock_split(&self, items: &[StockSplitEvent]) -> StorageResult<()> {
        self.duck.insert_stock_split(items).await
    }
    async fn query_stock_split(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<StockSplitEvent>> {
        self.duck.query_stock_split(symbol, time_range).await
    }
}

// ── CandleStorage ─────────────────────────────────────────────────────────────

impl CandleStorage for DataStore {
    async fn upsert_candles(
        &self,
        symbol: &Symbol,
        interval: Interval,
        bars: &[CandleEntry],
    ) -> StorageResult<()> {
        self.duck.upsert_candles(symbol, interval, bars).await
    }
    async fn query_candles(
        &self,
        symbol: &Symbol,
        interval: Interval,
        time_range: Range<NaiveDateTime>,
    ) -> StorageResult<Vec<CandleEntry>> {
        self.duck.query_candles(symbol, interval, time_range).await
    }
    async fn unsert_daily_candle(
        &self,
        symbol: &Symbol,
        bars: &[DailyCandleEntry],
    ) -> StorageResult<()> {
        self.duck.unsert_daily_candle(symbol, bars).await
    }
    async fn query_daily_candles<'a>(
        &'a self,
        symbol: &Symbol,
        date_range: Range<NaiveDate>,
    ) -> StorageResult<BoxStream<'a, DailyCandleEntry>> {
        self.duck.query_daily_candles(symbol, date_range).await
    }
}

// ── TickStorage ───────────────────────────────────────────────────────────────

impl TickStorage for DataStore {
    async fn insert_ticks(&self, ticks: &[TradeTick]) -> StorageResult<()> {
        self.duck.insert_ticks(ticks).await
    }
    async fn query_ticks(
        &self,
        symbol: &Symbol,
        time_range: Range<NaiveDateTime>,
    ) -> StorageResult<Vec<TradeTick>> {
        self.duck.query_ticks(symbol, time_range).await
    }
}

// ── MacroStorage ──────────────────────────────────────────────────────────────

impl MacroStorage for DataStore {
    async fn upsert_macro_series(&self, points: &[MacroSeriesPoint]) -> StorageResult<()> {
        self.duck.upsert_macro_series(points).await
    }
    async fn get_latest_macro_values(
        &self,
        series_ids: &[&str],
    ) -> StorageResult<Vec<MacroSeriesPoint>> {
        self.duck.get_latest_macro_values(series_ids).await
    }
    async fn query_macro_series(
        &self,
        series_id: &str,
        date_range: Range<NaiveDate>,
    ) -> StorageResult<Vec<MacroSeriesPoint>> {
        self.duck.query_macro_series(series_id, date_range).await
    }
}

// ── PortfolioStorage ──────────────────────────────────────────────────────────

impl PortfolioStorage for DataStore {
    async fn load_portfolio_state(&self) -> StorageResult<PortfolioState> {
        self.duck.load_portfolio_state().await
    }
    async fn get_open_position(&self, id: Uuid) -> StorageResult<Option<OpenPosition>> {
        self.duck.get_open_position(id).await
    }
    async fn open_position(
        &self,
        symbol: &Symbol,
        shares: rust_decimal::Decimal,
        entry_price: rust_decimal::Decimal,
        entry_at: chrono::DateTime<chrono::Utc>,
    ) -> StorageResult<OpenPosition> {
        self.duck.open_position(symbol, shares, entry_price, entry_at).await
    }
    async fn close_position(
        &self,
        id: Uuid,
        exit_price: rust_decimal::Decimal,
        exit_at: chrono::DateTime<chrono::Utc>,
    ) -> StorageResult<()> {
        self.duck.close_position(id, exit_price, exit_at).await
    }
    async fn add_watch(&self, symbol: &Symbol) -> StorageResult<WatchItem> {
        self.duck.add_watch(symbol).await
    }
    async fn remove_watch(&self, symbol: &Symbol) -> StorageResult<()> {
        self.duck.remove_watch(symbol).await
    }
    async fn list_watches(&self) -> StorageResult<Vec<WatchItem>> {
        self.duck.list_watches().await
    }
    async fn get_watch(&self, symbol: &Symbol) -> StorageResult<Option<WatchItem>> {
        self.duck.get_watch(symbol).await
    }
}

// ── StrategyStorage ───────────────────────────────────────────────────────────

impl StrategyStorage for DataStore {
    async fn insert_strategy_config(&self, row: &StrategyConfigRow) -> StorageResult<()> {
        self.duck.insert_strategy_config(row).await
    }
    async fn get_strategy_config(&self, id: Uuid) -> StorageResult<Option<StrategyConfigRow>> {
        self.duck.get_strategy_config(id).await
    }
    async fn list_strategy_configs(&self) -> StorageResult<Vec<StrategyConfigRow>> {
        self.duck.list_strategy_configs().await
    }
    async fn update_strategy_config(&self, row: &StrategyConfigRow) -> StorageResult<()> {
        self.duck.update_strategy_config(row).await
    }
    async fn insert_strategy_run(&self, row: &StrategyRunRow) -> StorageResult<()> {
        self.duck.insert_strategy_run(row).await
    }
    async fn complete_strategy_run(&self, row: &StrategyRunRow) -> StorageResult<()> {
        self.duck.complete_strategy_run(row).await
    }
    async fn get_strategy_run(&self, id: Uuid) -> StorageResult<Option<StrategyRunRow>> {
        self.duck.get_strategy_run(id).await
    }
    async fn list_strategy_runs(
        &self,
        config_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<StrategyRunRow>> {
        self.duck.list_strategy_runs(config_id, limit).await
    }
    async fn insert_strategy_signals(&self, rows: &[StrategySignalRow]) -> StorageResult<()> {
        self.duck.insert_strategy_signals(rows).await
    }
    async fn query_strategy_signals(
        &self,
        run_id: Option<Uuid>,
        config_id: Option<Uuid>,
        symbol: Option<&Symbol>,
        since: Option<NaiveDate>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<StrategySignalRow>> {
        self.duck
            .query_strategy_signals(run_id, config_id, symbol, since, limit)
            .await
    }
    async fn get_strategy_signal(&self, id: Uuid) -> StorageResult<Option<StrategySignalRow>> {
        self.duck.get_strategy_signal(id).await
    }
}

// ── BacktestStorage ───────────────────────────────────────────────────────────

impl BacktestStorage for DataStore {
    async fn insert_backtest_run(&self, row: &BacktestRunRow) -> StorageResult<()> {
        self.duck.insert_backtest_run(row).await
    }
    async fn complete_backtest_run(&self, row: &BacktestRunRow) -> StorageResult<()> {
        self.duck.complete_backtest_run(row).await
    }
    async fn get_backtest_run(&self, id: Uuid) -> StorageResult<Option<BacktestRunRow>> {
        self.duck.get_backtest_run(id).await
    }
    async fn list_backtest_runs(
        &self,
        config_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<BacktestRunRow>> {
        self.duck.list_backtest_runs(config_id, limit).await
    }
    async fn insert_backtest_trades(&self, rows: &[BacktestTradeRow]) -> StorageResult<()> {
        self.duck.insert_backtest_trades(rows).await
    }
    async fn get_backtest_trades(&self, run_id: Uuid) -> StorageResult<Vec<BacktestTradeRow>> {
        self.duck.get_backtest_trades(run_id).await
    }
    async fn insert_equity_curve(&self, points: &[EquityCurvePoint]) -> StorageResult<()> {
        self.duck.insert_equity_curve(points).await
    }
    async fn get_equity_curve(&self, run_id: Uuid) -> StorageResult<Vec<EquityCurvePoint>> {
        self.duck.get_equity_curve(run_id).await
    }
}

// ── ChatStorage ──────────────────────────────────────────────────────────────

impl ChatStorage for DataStore {
    async fn upsert_chat_session(&self, row: &ChatSessionRow) -> StorageResult<()> {
        self.duck.upsert_chat_session(row).await
    }

    async fn list_chat_sessions(&self, limit: Option<u32>) -> StorageResult<Vec<ChatSessionRow>> {
        self.duck.list_chat_sessions(limit).await
    }

    async fn get_chat_session(&self, id: Uuid) -> StorageResult<Option<ChatSessionRow>> {
        self.duck.get_chat_session(id).await
    }

    async fn insert_chat_messages(&self, rows: &[ChatMessageRow]) -> StorageResult<()> {
        self.duck.insert_chat_messages(rows).await
    }

    async fn list_chat_messages(&self, session_id: Uuid) -> StorageResult<Vec<ChatMessageRow>> {
        self.duck.list_chat_messages(session_id).await
    }
}
