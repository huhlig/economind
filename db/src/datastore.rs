//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `DataStore` — unified facade over PostgreSQL (durable writes) and DuckDB
//! (fast analytical reads).
//!
//! The plan (§1.C.7) calls for a single interface that:
//! - **Writes** go to PostgreSQL (durable, ACID)
//! - **Reads** are served from DuckDB (fast columnar, in-process)
//! - A `sync()` call refreshes DuckDB from PostgreSQL before each strategy run
//!
//! Callers never need to know which backend is in use — they call DataStore methods
//! and the facade routes appropriately.

use crate::{StorageError, StorageResult};
use crate::storage::{
    CandleStorage, DuckDatabase, MacroSeriesPoint, MacroStorage, MetadataStorage,
    PortfolioState, PortfolioStorage, PostgresStorage,
    StrategyConfigRow, StrategyRunRow, StrategySignalRow, StrategyStorage,
    TickStorage, TickerQuery,
};
use economind_core::model::{
    BalanceSheet, CandleEntry, CashFlowStatement, DailyCandleEntry, DividendEvent,
    IncomeStatement, Interval, NewsStory, StockSplitEvent, Symbol, Ticker, TradeTick,
};
use chrono::{NaiveDate, NaiveDateTime};
use futures_core::stream::BoxStream;
use std::ops::Range;
use uuid::Uuid;

// ── DataStore ────────────────────────────────────────────────────────────────

/// Unified data access facade.
///
/// Writes are durably committed to PostgreSQL.  Reads are served from DuckDB
/// after `sync()` has been called.  For most use-cases (strategy runs, backtest)
/// call `sync()` once at startup, then read freely from the fast columnar store.
///
/// When neither a `PostgresStorage` nor a `DATABASE_URL` is available the
/// `DataStore` can be constructed with `from_duck_only()` for offline / testing
/// use — in that case writes go directly to DuckDB and reads come from the same.
#[derive(Clone)]
pub struct DataStore {
    duck: DuckDatabase,
    pg: Option<PostgresStorage>,
}

impl DataStore {
    /// Connect to both PostgreSQL and DuckDB.
    ///
    /// The DuckDB path may be `":memory:"` for purely in-process use.
    pub async fn connect(database_url: &str, duckdb_path: &str) -> StorageResult<Self> {
        let pg = PostgresStorage::connect(database_url).await?;
        let duck = if duckdb_path == ":memory:" {
            DuckDatabase::in_memory()?
        } else {
            DuckDatabase::open(duckdb_path)?
        };
        Ok(Self { duck, pg: Some(pg) })
    }

    /// DuckDB-only mode — no PostgreSQL connection.
    /// Useful for backtesting against a pre-populated DuckDB snapshot
    /// or for integration tests that don't need a live database.
    pub fn from_duck_only(duck: DuckDatabase) -> Self {
        Self { duck, pg: None }
    }

    /// Whether a live PostgreSQL connection is available.
    pub fn has_postgres(&self) -> bool {
        self.pg.is_some()
    }

    /// Return a reference to the underlying DuckDB instance.
    pub fn duck(&self) -> &DuckDatabase {
        &self.duck
    }

    /// Return a reference to the PostgreSQL storage, if connected.
    pub fn postgres(&self) -> Option<&PostgresStorage> {
        self.pg.as_ref()
    }

    // ── Sync: PostgreSQL → DuckDB ─────────────────────────────────────────────

    /// Synchronise DuckDB from PostgreSQL.
    ///
    /// Fetches instruments, bars (for all active instruments, last `days` of
    /// daily bars), income statements, balance sheets, cash flow statements,
    /// dividends, and stock splits from PostgreSQL and upserts them into DuckDB.
    ///
    /// Call this once before each strategy run.  The operation is idempotent
    /// (all inserts use `ON CONFLICT DO UPDATE / DO NOTHING`).
    ///
    /// # Errors
    /// Returns `StorageError::Provider("no postgres connection")` if the
    /// DataStore was created with `from_duck_only()`.
    pub async fn sync(&self, days: u32) -> StorageResult<()> {
        let pg = self.pg.as_ref().ok_or_else(|| {
            StorageError::Provider("sync() requires a PostgreSQL connection".to_string())
        })?;

        let cutoff = {
            let today = chrono::Utc::now().date_naive();
            today - chrono::Duration::days(days as i64)
        };

        // 1. Sync instrument list
        {
            use futures::StreamExt;
            let symbols: Vec<Symbol> = pg.list_tickers().await?.collect().await;
            for sym in &symbols {
                self.duck.upsert_ticker(sym).await?;
            }
        }

        // 2. Sync daily bars for every active instrument
        {
            use futures::StreamExt;
            let symbols: Vec<Symbol> = self.duck.list_tickers().await?.collect().await;
            let end = chrono::Utc::now().date_naive() + chrono::Duration::days(1);
            for sym in &symbols {
                let bars: Vec<DailyCandleEntry> = pg
                    .query_daily_candles(sym, cutoff..end)
                    .await?
                    .collect()
                    .await;
                if !bars.is_empty() {
                    self.duck.unsert_daily_candle(sym, &bars).await?;
                }
            }
        }

        // 3. Sync fundamentals
        {
            let fund_cutoff_range = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()
                ..chrono::Utc::now().date_naive();

            let stmts = pg.query_income_statements(None, Some(fund_cutoff_range.clone())).await?;
            if !stmts.is_empty() {
                self.duck.insert_income_statements(&stmts).await?;
            }

            let bs = pg.query_balance_sheets(None, Some(fund_cutoff_range.clone())).await?;
            if !bs.is_empty() {
                self.duck.insert_balance_sheets(&bs).await?;
            }

            let cf = pg.query_cash_flow_statements(None, Some(fund_cutoff_range.clone())).await?;
            if !cf.is_empty() {
                self.duck.insert_cash_flow_statements(&cf).await?;
            }

            let divs = pg.query_dividend_report(None, Some(fund_cutoff_range.clone())).await?;
            if !divs.is_empty() {
                self.duck.insert_dividend_report(&divs).await?;
            }

            let splits = pg.query_stock_split(None, Some(fund_cutoff_range)).await?;
            if !splits.is_empty() {
                self.duck.insert_stock_split(&splits).await?;
            }
        }

        Ok(())
    }
}

// ── MetadataStorage — writes to PG, reads from Duck ──────────────────────────

impl MetadataStorage for DataStore {
    /// List all active ticker symbols. Served from DuckDB.
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

    /// Upsert a ticker — writes to PostgreSQL (if available) and DuckDB.
    async fn upsert_ticker(&self, symbol: &Symbol) -> StorageResult<()> {
        if let Some(pg) = &self.pg {
            pg.upsert_ticker(symbol).await?;
        }
        self.duck.upsert_ticker(symbol).await
    }

    /// Insert news — written to PostgreSQL for durability.
    async fn insert_news(&self, items: &[NewsStory]) -> StorageResult<()> {
        if let Some(pg) = &self.pg {
            pg.insert_news(items).await?;
        }
        // Also mirror to Duck so queries work without a re-sync
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
        if let Some(pg) = &self.pg {
            pg.insert_income_statements(items).await?;
        }
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
        if let Some(pg) = &self.pg {
            pg.insert_balance_sheets(items).await?;
        }
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
        if let Some(pg) = &self.pg {
            pg.insert_cash_flow_statements(items).await?;
        }
        self.duck.insert_cash_flow_statements(items).await
    }

    async fn query_cash_flow_statements(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<CashFlowStatement>> {
        self.duck.query_cash_flow_statements(symbol, time_range).await
    }

    async fn insert_dividend_report(&self, items: &[DividendEvent]) -> StorageResult<()> {
        if let Some(pg) = &self.pg {
            pg.insert_dividend_report(items).await?;
        }
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
        if let Some(pg) = &self.pg {
            pg.insert_stock_split(items).await?;
        }
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

// ── CandleStorage — writes to PG, reads from Duck ────────────────────────────

impl CandleStorage for DataStore {
    async fn upsert_candles(
        &self,
        symbol: &Symbol,
        interval: Interval,
        bars: &[CandleEntry],
    ) -> StorageResult<()> {
        if let Some(pg) = &self.pg {
            pg.upsert_candles(symbol, interval, bars).await?;
        }
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
        if let Some(pg) = &self.pg {
            pg.unsert_daily_candle(symbol, bars).await?;
        }
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
    async fn insert_ticks(&self, _ticks: &[TradeTick]) -> StorageResult<()> {
        Err(StorageError::UnsupportedInterval)
    }
    async fn query_ticks(
        &self,
        _symbol: &Symbol,
        _time_range: Range<NaiveDateTime>,
    ) -> StorageResult<Vec<TradeTick>> {
        Err(StorageError::UnsupportedInterval)
    }
}

// ── MacroStorage — reads from PostgreSQL ─────────────────────────────────────
//
// Macro series are never mirrored to DuckDB in Phase 2 (small dataset, no
// analytical joins needed).  Reads go straight to PostgreSQL.

impl MacroStorage for DataStore {
    async fn get_latest_macro_values(
        &self,
        series_ids: &[&str],
    ) -> StorageResult<Vec<MacroSeriesPoint>> {
        let pg = self.pg.as_ref().ok_or_else(|| {
            StorageError::Provider("MacroStorage requires a PostgreSQL connection".to_string())
        })?;
        pg.get_latest_macro_values(series_ids).await
    }

    async fn query_macro_series(
        &self,
        series_id: &str,
        date_range: Range<NaiveDate>,
    ) -> StorageResult<Vec<MacroSeriesPoint>> {
        let pg = self.pg.as_ref().ok_or_else(|| {
            StorageError::Provider("MacroStorage requires a PostgreSQL connection".to_string())
        })?;
        pg.query_macro_series(series_id, date_range).await
    }
}

// ── PortfolioStorage ──────────────────────────────────────────────────────────

impl PortfolioStorage for DataStore {
    async fn load_portfolio_state(&self) -> StorageResult<PortfolioState> {
        let pg = self.pg.as_ref().ok_or_else(|| {
            StorageError::Provider("PortfolioStorage requires a PostgreSQL connection".to_string())
        })?;
        pg.load_portfolio_state().await
    }
}

// ── StrategyStorage — writes and reads via PostgreSQL ────────────────────────

impl StrategyStorage for DataStore {
    async fn insert_strategy_config(&self, row: &StrategyConfigRow) -> StorageResult<()> {
        self.require_pg()?.insert_strategy_config(row).await
    }
    async fn get_strategy_config(&self, id: Uuid) -> StorageResult<Option<StrategyConfigRow>> {
        self.require_pg()?.get_strategy_config(id).await
    }
    async fn list_strategy_configs(&self) -> StorageResult<Vec<StrategyConfigRow>> {
        self.require_pg()?.list_strategy_configs().await
    }
    async fn update_strategy_config(&self, row: &StrategyConfigRow) -> StorageResult<()> {
        self.require_pg()?.update_strategy_config(row).await
    }
    async fn insert_strategy_run(&self, row: &StrategyRunRow) -> StorageResult<()> {
        self.require_pg()?.insert_strategy_run(row).await
    }
    async fn complete_strategy_run(&self, row: &StrategyRunRow) -> StorageResult<()> {
        self.require_pg()?.complete_strategy_run(row).await
    }
    async fn get_strategy_run(&self, id: Uuid) -> StorageResult<Option<StrategyRunRow>> {
        self.require_pg()?.get_strategy_run(id).await
    }
    async fn list_strategy_runs(
        &self,
        config_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<StrategyRunRow>> {
        self.require_pg()?.list_strategy_runs(config_id, limit).await
    }
    async fn insert_strategy_signals(&self, rows: &[StrategySignalRow]) -> StorageResult<()> {
        self.require_pg()?.insert_strategy_signals(rows).await
    }
    async fn query_strategy_signals(
        &self,
        run_id: Option<Uuid>,
        config_id: Option<Uuid>,
        symbol: Option<&Symbol>,
        since: Option<NaiveDate>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<StrategySignalRow>> {
        self.require_pg()?
            .query_strategy_signals(run_id, config_id, symbol, since, limit)
            .await
    }
    async fn get_strategy_signal(&self, id: Uuid) -> StorageResult<Option<StrategySignalRow>> {
        self.require_pg()?.get_strategy_signal(id).await
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

impl DataStore {
    fn require_pg(&self) -> StorageResult<&PostgresStorage> {
        self.pg.as_ref().ok_or_else(|| {
            StorageError::Provider("this operation requires a PostgreSQL connection".to_string())
        })
    }
}
