//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! DuckDB in-process analytical storage.
//!
//! Single embedded database for all data: market data, fundamentals, strategy
//! configs, backtest results, and live/paper portfolio positions.
//!
//! `preload(days)` materialises hot tables into an attached `:memory:` schema so
//! strategy reads are served entirely from RAM.  All writes always go to the main
//! (on-disk) connection.
//!
//! All async methods wrap DuckDB's synchronous API via `tokio::task::spawn_blocking`.

use tracing::{debug, instrument, warn};

use crate::storage::{
    BacktestRunRow, BacktestStorage, BacktestTradeRow, CandleStorage, ChatMessageRow,
    ChatSessionRow, ChatStorage, EquityCurvePoint, MacroSeriesPoint, MacroStorage,
    MetadataStorage, OpenPosition, PortfolioState, PortfolioStorage, StrategyConfigRow,
    StrategyRunRow, StrategySignalRow, StrategyStorage, TickStorage, TickerQuery, WatchItem,
};
use crate::StorageError;
use crate::StorageResult;
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use duckdb::{params, Connection};
use economind_core::model::{
    BalanceSheet, CandleEntry, CashFlowStatement, DailyCandleEntry, DividendEvent, Exchange,
    IncomeStatement, Interval, NewsAbout, NewsStory, StockSplitEvent, Symbol, Ticker, TradeTick,
};
use futures_core::stream::BoxStream;
use rust_decimal::Decimal;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// ── DuckDatabase ─────────────────────────────────────────────────────────────

/// DuckDB-backed storage for the entire Economind platform.
///
/// After calling `preload(days)`, hot tables (instruments, recent bars,
/// macro_series, strategy_configs, open positions) are materialised into an
/// attached `:memory:` schema and all reads are served from RAM.
#[derive(Clone)]
pub struct DuckDatabase {
    path: PathBuf,
    conn: Arc<Mutex<Connection>>,
    preloaded: Arc<AtomicBool>,
}

impl DuckDatabase {
    /// Open (or create) a DuckDB database at `path` and apply the schema.
    pub fn open<P: AsRef<Path>>(path: P) -> StorageResult<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)?;
        conn.execute_batch(include_str!("schema.sql"))?;
        Ok(Self {
            path,
            conn: Arc::new(Mutex::new(conn)),
            preloaded: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Open an in-memory DuckDB instance (tests / backtesting).
    pub fn in_memory() -> StorageResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(include_str!("schema.sql"))?;
        Ok(Self {
            path: PathBuf::from(":memory:"),
            conn: Arc::new(Mutex::new(conn)),
            preloaded: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Path to the underlying database file (`":memory:"` for in-memory instances).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Execute a raw SQL batch.
    pub fn execute_batch(&self, sql: &str) -> StorageResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| StorageError::Provider(e.to_string()))?;
        conn.execute_batch(sql)?;
        Ok(())
    }

    /// Whether the hot in-memory cache is active.
    pub fn is_preloaded(&self) -> bool {
        self.preloaded.load(Ordering::Relaxed)
    }

    /// Read a runtime setting from DuckDB.
    pub async fn get_setting(&self, key: &str) -> StorageResult<Option<String>> {
        let conn = self.conn.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Option<String>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare("SELECT value FROM app_settings WHERE key=?")?;
            Ok(stmt
                .query_map([&key], |row| row.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .next())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    /// Upsert a runtime setting in DuckDB.
    pub async fn set_setting(&self, key: &str, value: &str) -> StorageResult<()> {
        let conn = self.conn.clone();
        let key = key.to_string();
        let value = value.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            conn.execute(
                "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, ?) \
                 ON CONFLICT (key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
                params![key, value, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    /// Materialise hot tables into an attached `:memory:` schema.
    ///
    /// Call once before each strategy run.  Subsequent calls refresh the cache.
    /// Hot tables: instruments (active), bars (1d, last N days), macro_series,
    /// strategy_configs (enabled), portfolio_positions (open).
    pub async fn preload(&self, lookback_days: u32) -> StorageResult<()> {
        let conn = self.conn.clone();
        let preloaded = self.preloaded.clone();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            // ATTACH ':memory:' once; silently ignore if already attached.
            let _ = conn.execute_batch("ATTACH ':memory:' AS hot;");
            conn.execute_batch(&format!(
                r#"
                CREATE OR REPLACE TABLE hot.instruments AS
                    SELECT * FROM instruments WHERE active = TRUE;

                CREATE OR REPLACE TABLE hot.bars AS
                    SELECT * FROM bars
                    WHERE interval = '1d'
                      AND time >= CURRENT_DATE - INTERVAL '{lookback_days} days';

                CREATE OR REPLACE TABLE hot.macro_series AS
                    SELECT * FROM macro_series;

                CREATE OR REPLACE TABLE hot.strategy_configs AS
                    SELECT * FROM strategy_configs WHERE enabled = TRUE;

                CREATE OR REPLACE TABLE hot.portfolio_positions AS
                    SELECT * FROM portfolio_positions WHERE status = 'open';
                "#
            ))?;
            preloaded.store(true, Ordering::Relaxed);
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))??;
        Ok(())
    }

    // ── table name routing (hot cache vs. on-disk) ────────────────────────────

    fn t_instruments(&self) -> &'static str {
        if self.preloaded.load(Ordering::Relaxed) {
            "hot.instruments"
        } else {
            "instruments"
        }
    }
    fn t_bars(&self) -> &'static str {
        if self.preloaded.load(Ordering::Relaxed) {
            "hot.bars"
        } else {
            "bars"
        }
    }
    fn t_macro(&self) -> &'static str {
        if self.preloaded.load(Ordering::Relaxed) {
            "hot.macro_series"
        } else {
            "macro_series"
        }
    }
    fn t_strategy_configs(&self) -> &'static str {
        if self.preloaded.load(Ordering::Relaxed) {
            "hot.strategy_configs"
        } else {
            "strategy_configs"
        }
    }
}

// ── Interval helper ──────────────────────────────────────────────────────────

fn interval_str(interval: Interval) -> &'static str {
    match interval {
        Interval::OneMinute => "1m",
        Interval::FiveMinute => "5m",
        Interval::FifteenMinute => "15m",
        Interval::OneHour => "1h",
        Interval::OneDay => "1d",
    }
}

// ── MetadataStorage ──────────────────────────────────────────────────────────

impl MetadataStorage for DuckDatabase {
    async fn list_tickers(&self) -> StorageResult<BoxStream<'static, Symbol>> {
        let conn = self.conn.clone();
        let table = self.t_instruments();
        let symbols = tokio::task::spawn_blocking(move || -> StorageResult<Vec<Symbol>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(&format!(
                "SELECT symbol FROM {table} WHERE active = TRUE ORDER BY symbol"
            ))?;
            let symbols: Vec<Symbol> = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .map(|s| Symbol::new(&s))
                .collect();
            Ok(symbols)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))??;
        Ok(Box::pin(futures::stream::iter(symbols)))
    }

    async fn query_tickers<'a>(
        &'a self,
        _query: TickerQuery,
    ) -> StorageResult<BoxStream<'a, Ticker>> {
        let conn = self.conn.clone();
        let table = self.t_instruments();
        let tickers = tokio::task::spawn_blocking(move || -> StorageResult<Vec<Ticker>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(&format!(
                "SELECT symbol, exchange, name, country, industry, sector, \
                        ipoyear, marketcap, description, active \
                 FROM {table} WHERE active = TRUE ORDER BY symbol"
            ))?;
            let tickers = stmt
                .query_map([], duck_row_to_ticker)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(tickers)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))??;
        Ok(Box::pin(futures::stream::iter(tickers)))
    }

    async fn get_ticker(&self, symbol: &Symbol) -> StorageResult<Option<Ticker>> {
        let conn = self.conn.clone();
        let sym = symbol.as_str().to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Option<Ticker>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT symbol, exchange, name, country, industry, sector, \
                        ipoyear, marketcap, description, active \
                 FROM instruments WHERE symbol = ?",
            )?;
            let mut rows = stmt.query_map([&sym], duck_row_to_ticker)?;
            Ok(rows.next().and_then(|r| r.ok()))
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn upsert_ticker(&self, symbol: &Symbol) -> StorageResult<()> {
        let conn = self.conn.clone();
        let sym = symbol.as_str().to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            conn.execute(
                "INSERT INTO instruments (symbol) VALUES (?) \
                 ON CONFLICT (symbol) DO NOTHING",
                params![sym],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_news(&self, items: &[NewsStory]) -> StorageResult<()> {
        let conn = self.conn.clone();
        let items: Vec<NewsStory> = items.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO news (symbol, headline, summary, story, url, evaluation, published_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (url) DO NOTHING",
            )?;
            for item in &items {
                let symbol_str: Option<String> = match &item.about {
                    NewsAbout::Symbol(s) => Some(s.as_str().to_string()),
                    _ => None,
                };
                let published = item.published_at.to_string();
                stmt.execute(params![
                    symbol_str, item.headline, item.summary,
                    item.story, item.url, item.evaluation, published,
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn query_news<'a>(
        &'a self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<BoxStream<'a, NewsStory>> {
        let conn = self.conn.clone();
        let sym = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end = time_range.map(|r| r.end.to_string());

        let stories = tokio::task::spawn_blocking(move || -> StorageResult<Vec<NewsStory>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let stories = match (&sym, &start, &end) {
                (Some(s), Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, headline, summary, story, url, evaluation, \
                                published_at::VARCHAR, fetched_at::VARCHAR FROM news \
                         WHERE symbol = ? AND published_at >= ? AND published_at < ? \
                         ORDER BY published_at DESC",
                    )?;
                    stmt.query_map(params![s, st, e], duck_row_to_news)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                (Some(s), None, _) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, headline, summary, story, url, evaluation, \
                                published_at::VARCHAR, fetched_at::VARCHAR FROM news \
                         WHERE symbol = ? ORDER BY published_at DESC",
                    )?;
                    stmt.query_map(params![s], duck_row_to_news)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                (None, Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, headline, summary, story, url, evaluation, \
                                published_at::VARCHAR, fetched_at::VARCHAR FROM news \
                         WHERE published_at >= ? AND published_at < ? \
                         ORDER BY published_at DESC",
                    )?;
                    stmt.query_map(params![st, e], duck_row_to_news)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                _ => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, headline, summary, story, url, evaluation, \
                                published_at::VARCHAR, fetched_at::VARCHAR FROM news \
                         ORDER BY published_at DESC",
                    )?;
                    stmt.query_map([], duck_row_to_news)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
            };
            Ok(stories)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))??;
        Ok(Box::pin(futures::stream::iter(stories)))
    }

    async fn insert_income_statements(&self, items: &[IncomeStatement]) -> StorageResult<()> {
        let conn = self.conn.clone();
        let items: Vec<IncomeStatement> = items.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO income_statements \
                    (symbol, period_end, period_type, revenue, cogs, operating_income, \
                     ebit, net_income, eps, interest_expense, tax_expense) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (symbol, period_end, period_type) DO UPDATE SET \
                    revenue=EXCLUDED.revenue, cogs=EXCLUDED.cogs, \
                    operating_income=EXCLUDED.operating_income, ebit=EXCLUDED.ebit, \
                    net_income=EXCLUDED.net_income, eps=EXCLUDED.eps, \
                    interest_expense=EXCLUDED.interest_expense, \
                    tax_expense=EXCLUDED.tax_expense",
            )?;
            for item in &items {
                stmt.execute(params![
                    item.symbol.as_str(),
                    item.period_end.to_string(),
                    item.period_type,
                    d2f(item.revenue),
                    d2f(item.cogs),
                    d2f(item.operating_income),
                    d2f(item.ebit),
                    d2f(item.net_income),
                    d2f(item.eps),
                    d2f(item.interest_expense),
                    d2f(item.tax_expense),
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn query_income_statements(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<IncomeStatement>> {
        let conn = self.conn.clone();
        let sym = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end = time_range.map(|r| r.end.to_string());
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<IncomeStatement>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let results: Vec<IncomeStatement> = match (&sym, &start, &end) {
                (Some(s), Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, period_type, revenue, cogs, operating_income, \
                                ebit, net_income, eps, interest_expense, tax_expense \
                         FROM income_statements \
                         WHERE symbol=? AND period_end>=? AND period_end<? \
                         ORDER BY period_end DESC",
                    )?;
                    stmt.query_map(params![s, st, e], duck_row_to_income)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                (Some(s), None, _) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, period_type, revenue, cogs, operating_income, \
                                ebit, net_income, eps, interest_expense, tax_expense \
                         FROM income_statements WHERE symbol=? ORDER BY period_end DESC",
                    )?;
                    stmt.query_map(params![s], duck_row_to_income)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                (None, Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, period_type, revenue, cogs, operating_income, \
                                ebit, net_income, eps, interest_expense, tax_expense \
                         FROM income_statements \
                         WHERE period_end>=? AND period_end<? ORDER BY period_end DESC",
                    )?;
                    stmt.query_map(params![st, e], duck_row_to_income)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                _ => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, period_type, revenue, cogs, operating_income, \
                                ebit, net_income, eps, interest_expense, tax_expense \
                         FROM income_statements ORDER BY symbol, period_end DESC",
                    )?;
                    stmt.query_map([], duck_row_to_income)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
            };
            Ok(results)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_balance_sheets(&self, items: &[BalanceSheet]) -> StorageResult<()> {
        let conn = self.conn.clone();
        let items: Vec<BalanceSheet> = items.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO balance_sheets \
                    (symbol, period_end, period_type, total_assets, total_debt, total_equity, cash) \
                 VALUES (?, ?, 'annual', ?, ?, ?, ?) \
                 ON CONFLICT (symbol, period_end, period_type) DO UPDATE SET \
                    total_assets=EXCLUDED.total_assets, total_debt=EXCLUDED.total_debt, \
                    total_equity=EXCLUDED.total_equity, cash=EXCLUDED.cash",
            )?;
            for item in &items {
                stmt.execute(params![
                    item.symbol.as_str(), item.period_end.to_string(),
                    d2f(item.total_assets), d2f(item.total_debt),
                    d2f(item.total_equity), d2f(item.cash),
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn query_balance_sheets(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<BalanceSheet>> {
        let conn = self.conn.clone();
        let sym = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end = time_range.map(|r| r.end.to_string());
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<BalanceSheet>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let results =
                match (&sym, &start, &end) {
                    (Some(s), Some(st), Some(e)) => {
                        let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, total_assets, total_debt, total_equity, cash \
                         FROM balance_sheets WHERE symbol=? AND period_end>=? AND period_end<? \
                         ORDER BY period_end DESC")?;
                        stmt.query_map(params![s, st, e], duck_row_to_balance)?
                            .filter_map(|r| r.ok())
                            .collect()
                    }
                    (Some(s), None, _) => {
                        let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, total_assets, total_debt, total_equity, cash \
                         FROM balance_sheets WHERE symbol=? ORDER BY period_end DESC")?;
                        stmt.query_map(params![s], duck_row_to_balance)?
                            .filter_map(|r| r.ok())
                            .collect()
                    }
                    (None, Some(st), Some(e)) => {
                        let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, total_assets, total_debt, total_equity, cash \
                         FROM balance_sheets WHERE period_end>=? AND period_end<? \
                         ORDER BY period_end DESC")?;
                        stmt.query_map(params![st, e], duck_row_to_balance)?
                            .filter_map(|r| r.ok())
                            .collect()
                    }
                    _ => {
                        let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, total_assets, total_debt, total_equity, cash \
                         FROM balance_sheets ORDER BY symbol, period_end DESC")?;
                        stmt.query_map([], duck_row_to_balance)?
                            .filter_map(|r| r.ok())
                            .collect()
                    }
                };
            Ok(results)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_cash_flow_statements(&self, items: &[CashFlowStatement]) -> StorageResult<()> {
        let conn = self.conn.clone();
        let items: Vec<CashFlowStatement> = items.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO cash_flow_statements \
                    (symbol, period_end, period_type, operating_cash_flow, capex, free_cash_flow) \
                 VALUES (?, ?, 'annual', ?, ?, ?) \
                 ON CONFLICT (symbol, period_end, period_type) DO UPDATE SET \
                    operating_cash_flow=EXCLUDED.operating_cash_flow, \
                    capex=EXCLUDED.capex, free_cash_flow=EXCLUDED.free_cash_flow",
            )?;
            for item in &items {
                stmt.execute(params![
                    item.symbol.as_str(),
                    item.period_end.to_string(),
                    d2f(item.operating_cash_flow),
                    d2f(item.capex),
                    d2f(item.operating_cash_flow - item.capex),
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn query_cash_flow_statements(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<CashFlowStatement>> {
        let conn = self.conn.clone();
        let sym = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end = time_range.map(|r| r.end.to_string());
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<CashFlowStatement>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let results = match (&sym, &start, &end) {
                (Some(s), Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, operating_cash_flow, capex \
                         FROM cash_flow_statements \
                         WHERE symbol=? AND period_end>=? AND period_end<? \
                         ORDER BY period_end DESC",
                    )?;
                    stmt.query_map(params![s, st, e], duck_row_to_cash_flow)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                (Some(s), None, _) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, operating_cash_flow, capex \
                         FROM cash_flow_statements WHERE symbol=? ORDER BY period_end DESC",
                    )?;
                    stmt.query_map(params![s], duck_row_to_cash_flow)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                (None, Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, operating_cash_flow, capex \
                         FROM cash_flow_statements \
                         WHERE period_end>=? AND period_end<? ORDER BY period_end DESC",
                    )?;
                    stmt.query_map(params![st, e], duck_row_to_cash_flow)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                _ => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end::VARCHAR, operating_cash_flow, capex \
                         FROM cash_flow_statements ORDER BY symbol, period_end DESC",
                    )?;
                    stmt.query_map([], duck_row_to_cash_flow)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
            };
            Ok(results)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_dividend_report(&self, items: &[DividendEvent]) -> StorageResult<()> {
        let conn = self.conn.clone();
        let items: Vec<DividendEvent> = items.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO dividends (symbol, ex_date, payment_date, amount) \
                 VALUES (?, ?, ?, ?) \
                 ON CONFLICT (symbol, ex_date) DO UPDATE SET \
                    payment_date=EXCLUDED.payment_date, amount=EXCLUDED.amount",
            )?;
            for item in &items {
                stmt.execute(params![
                    item.symbol.as_str(),
                    item.ex_date.to_string(),
                    item.payment_date.to_string(),
                    d2f(item.amount),
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn query_dividend_report(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<DividendEvent>> {
        let conn = self.conn.clone();
        let sym = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end = time_range.map(|r| r.end.to_string());
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<DividendEvent>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let results = match (&sym, &start, &end) {
                (Some(s), Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, ex_date::VARCHAR, payment_date::VARCHAR, amount FROM dividends \
                         WHERE symbol=? AND ex_date>=? AND ex_date<? ORDER BY ex_date DESC",
                    )?;
                    stmt.query_map(params![s, st, e], duck_row_to_dividend)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                (Some(s), None, _) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, ex_date::VARCHAR, payment_date::VARCHAR, amount FROM dividends \
                         WHERE symbol=? ORDER BY ex_date DESC",
                    )?;
                    stmt.query_map(params![s], duck_row_to_dividend)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                (None, Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, ex_date::VARCHAR, payment_date::VARCHAR, amount FROM dividends \
                         WHERE ex_date>=? AND ex_date<? ORDER BY ex_date DESC",
                    )?;
                    stmt.query_map(params![st, e], duck_row_to_dividend)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                _ => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, ex_date::VARCHAR, payment_date::VARCHAR, amount FROM dividends \
                         ORDER BY symbol, ex_date DESC",
                    )?;
                    stmt.query_map([], duck_row_to_dividend)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
            };
            Ok(results)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_stock_split(&self, items: &[StockSplitEvent]) -> StorageResult<()> {
        let conn = self.conn.clone();
        let items: Vec<StockSplitEvent> = items.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO stock_splits (symbol, date, ratio) VALUES (?, ?, ?) \
                 ON CONFLICT (symbol, date) DO UPDATE SET ratio=EXCLUDED.ratio",
            )?;
            for item in &items {
                stmt.execute(params![
                    item.symbol.as_str(),
                    item.date.to_string(),
                    d2f(item.ratio),
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn query_stock_split(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<StockSplitEvent>> {
        let conn = self.conn.clone();
        let sym = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end = time_range.map(|r| r.end.to_string());
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<StockSplitEvent>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let results = match (&sym, &start, &end) {
                (Some(s), Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, date::VARCHAR, ratio FROM stock_splits \
                         WHERE symbol=? AND date>=? AND date<? ORDER BY date DESC",
                    )?;
                    stmt.query_map(params![s, st, e], duck_row_to_split)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                (Some(s), None, _) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, date::VARCHAR, ratio FROM stock_splits \
                         WHERE symbol=? ORDER BY date DESC",
                    )?;
                    stmt.query_map(params![s], duck_row_to_split)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                (None, Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, date::VARCHAR, ratio FROM stock_splits \
                         WHERE date>=? AND date<? ORDER BY date DESC",
                    )?;
                    stmt.query_map(params![st, e], duck_row_to_split)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
                _ => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, date::VARCHAR, ratio FROM stock_splits \
                         ORDER BY symbol, date DESC",
                    )?;
                    stmt.query_map([], duck_row_to_split)?
                        .filter_map(|r| r.ok())
                        .collect()
                }
            };
            Ok(results)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }
}

// ── CandleStorage ────────────────────────────────────────────────────────────

impl CandleStorage for DuckDatabase {
    async fn upsert_candles(
        &self,
        symbol: &Symbol,
        interval: Interval,
        bars: &[CandleEntry],
    ) -> StorageResult<()> {
        let conn = self.conn.clone();
        let sym = symbol.as_str().to_string();
        let ivl = interval_str(interval).to_string();
        let bars: Vec<CandleEntry> = bars.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO bars (symbol, interval, time, open, high, low, close, volume) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (symbol, interval, time) DO UPDATE SET \
                    open=EXCLUDED.open, high=EXCLUDED.high, \
                    low=EXCLUDED.low, close=EXCLUDED.close, volume=EXCLUDED.volume",
            )?;
            for bar in &bars {
                stmt.execute(params![
                    sym,
                    ivl,
                    bar.timestamp.to_string(),
                    d2f(bar.open),
                    d2f(bar.high),
                    d2f(bar.low),
                    d2f(bar.close),
                    bar.volume as i64,
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn query_candles(
        &self,
        symbol: &Symbol,
        interval: Interval,
        time_range: Range<NaiveDateTime>,
    ) -> StorageResult<Vec<CandleEntry>> {
        let conn = self.conn.clone();
        let sym = symbol.as_str().to_string();
        let ivl = interval_str(interval).to_string();
        let start = time_range.start.to_string();
        let end = time_range.end.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<CandleEntry>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT time::VARCHAR, open, high, low, close, volume FROM bars \
                 WHERE symbol=? AND interval=? AND time>=? AND time<? ORDER BY time ASC",
            )?;
            let results = stmt
                .query_map(params![sym, ivl, start, end], duck_row_to_candle)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(results)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn unsert_daily_candle(
        &self,
        symbol: &Symbol,
        bars: &[DailyCandleEntry],
    ) -> StorageResult<()> {
        let conn = self.conn.clone();
        let sym = symbol.as_str().to_string();
        let bars: Vec<DailyCandleEntry> = bars.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO bars (symbol, interval, time, open, high, low, close, volume) \
                 VALUES (?, '1d', ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (symbol, interval, time) DO UPDATE SET \
                    open=EXCLUDED.open, high=EXCLUDED.high, \
                    low=EXCLUDED.low, close=EXCLUDED.close, volume=EXCLUDED.volume",
            )?;
            for bar in &bars {
                let ts = bar.date.and_hms_opt(0, 0, 0).unwrap().to_string();
                stmt.execute(params![
                    sym,
                    ts,
                    d2f(bar.open),
                    d2f(bar.high),
                    d2f(bar.low),
                    d2f(bar.close),
                    bar.volume as i64,
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn query_daily_candles<'a>(
        &'a self,
        symbol: &Symbol,
        date_range: Range<NaiveDate>,
    ) -> StorageResult<BoxStream<'a, DailyCandleEntry>> {
        let conn = self.conn.clone();
        let sym = symbol.as_str().to_string();
        let start = date_range.start.and_hms_opt(0, 0, 0).unwrap().to_string();
        let end = date_range.end.and_hms_opt(0, 0, 0).unwrap().to_string();
        let table = self.t_bars();
        let entries =
            tokio::task::spawn_blocking(move || -> StorageResult<Vec<DailyCandleEntry>> {
                let conn = conn
                    .lock()
                    .map_err(|e| StorageError::Provider(e.to_string()))?;
                let mut stmt = conn.prepare(&format!(
                    "SELECT time::VARCHAR, open, high, low, close, volume FROM {table} \
                     WHERE symbol=? AND interval='1d' AND time>=? AND time<? ORDER BY time ASC"
                ))?;
                let results = stmt
                    .query_map(params![sym, start, end], duck_row_to_daily_candle)?
                    .filter_map(|r| r.ok())
                    .collect();
                Ok(results)
            })
            .await
            .map_err(|e| StorageError::Provider(e.to_string()))??;
        Ok(Box::pin(futures::stream::iter(entries)))
    }
}

// ── TickStorage ──────────────────────────────────────────────────────────────

impl TickStorage for DuckDatabase {
    async fn insert_ticks(&self, _ticks: &[TradeTick]) -> StorageResult<()> {
        // Tick storage is out of scope for the 1-day to 1-month trading horizon.
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

// ── MacroStorage ─────────────────────────────────────────────────────────────

impl MacroStorage for DuckDatabase {
    async fn upsert_macro_series(&self, points: &[MacroSeriesPoint]) -> StorageResult<()> {
        let conn = self.conn.clone();
        let points: Vec<MacroSeriesPoint> = points.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO macro_series (series_id, date, value, fetched_at) \
                 VALUES (?, ?, ?, ?) \
                 ON CONFLICT (series_id, date) DO UPDATE SET \
                    value=EXCLUDED.value, fetched_at=EXCLUDED.fetched_at",
            )?;
            for p in &points {
                stmt.execute(params![
                    p.series_id,
                    p.date.to_string(),
                    p.value.map(d2f),
                    p.fetched_at.to_rfc3339(),
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn get_latest_macro_values(
        &self,
        series_ids: &[&str],
    ) -> StorageResult<Vec<MacroSeriesPoint>> {
        let conn = self.conn.clone();
        let table = self.t_macro();
        let ids: Vec<String> = series_ids.iter().map(|s| s.to_string()).collect();
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<MacroSeriesPoint>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut results = Vec::new();
            for id in &ids {
                let mut stmt = conn.prepare(&format!(
                    "SELECT series_id, date::VARCHAR, value, fetched_at::VARCHAR FROM {table} \
                     WHERE series_id=? ORDER BY date DESC LIMIT 1"
                ))?;
                if let Some(row) = stmt
                    .query_map([id], duck_row_to_macro)?
                    .filter_map(|r| r.ok())
                    .next()
                {
                    results.push(row);
                }
            }
            Ok(results)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn query_macro_series(
        &self,
        series_id: &str,
        date_range: Range<NaiveDate>,
    ) -> StorageResult<Vec<MacroSeriesPoint>> {
        let conn = self.conn.clone();
        let table = self.t_macro();
        let id = series_id.to_string();
        let start = date_range.start.to_string();
        let end = date_range.end.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<MacroSeriesPoint>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(&format!(
                "SELECT series_id, date::VARCHAR, value, fetched_at::VARCHAR FROM {table} \
                 WHERE series_id=? AND date>=? AND date<? ORDER BY date ASC"
            ))?;
            Ok(stmt
                .query_map(params![id, start, end], duck_row_to_macro)?
                .filter_map(|r| r.ok())
                .collect())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }
}

// ── PortfolioStorage ──────────────────────────────────────────────────────────

impl PortfolioStorage for DuckDatabase {
    #[instrument(skip(self), name = "db.load_portfolio_state")]
    async fn load_portfolio_state(&self) -> StorageResult<PortfolioState> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> StorageResult<PortfolioState> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;

            // Load open positions.
            let mut stmt = conn.prepare(
                "SELECT id, symbol, shares, entry_price, CAST(entry_at AS VARCHAR) \
                 FROM portfolio_positions WHERE status='open'",
            )?;
            let open_positions: Vec<OpenPosition> = stmt
                .query_map([], |row| {
                    let id_str: String = row.get(0)?;
                    let sym: String = row.get(1)?;
                    let shares: f64 = row.get(2)?;
                    let entry_price: f64 = row.get(3)?;
                    let entry_str: String = row.get(4)?;
                    Ok((id_str, sym, shares, entry_price, entry_str))
                })?
                .filter_map(|r| r.ok())
                .map(
                    |(id_str, sym, shares, entry_price, entry_str)| OpenPosition {
                        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
                        symbol: Symbol::new(&sym),
                        shares: f2d(shares),
                        entry_price: f2d(entry_price),
                        entry_at: parse_datetime_utc(&entry_str),
                    },
                )
                .collect();

            // Batch price lookup: one query for all open positions.
            let mut position_value = 0.0f64;
            debug!(position_count = open_positions.len(), "pricing open positions");
            if !open_positions.is_empty() {
                let symbols: Vec<String> = open_positions
                    .iter()
                    .map(|p| p.symbol.as_str().to_string())
                    .collect();
                let placeholders = symbols.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                let price_sql = format!(
                    "SELECT DISTINCT ON (symbol) symbol, close \
                     FROM bars WHERE symbol IN ({placeholders}) AND interval='1d' \
                     ORDER BY symbol, time DESC"
                );
                let sym_refs: Vec<&dyn duckdb::ToSql> =
                    symbols.iter().map(|s| s as &dyn duckdb::ToSql).collect();
                let mut price_stmt = conn.prepare(&price_sql)?;
                let price_map: std::collections::HashMap<String, f64> = price_stmt
                    .query_map(sym_refs.as_slice(), |r| {
                        let sym: String = r.get(0)?;
                        let close: f64 = r.get(1)?;
                        Ok((sym, close))
                    })?
                    .filter_map(|r| r.ok())
                    .collect();
                for pos in &open_positions {
                    let price = match price_map.get(pos.symbol.as_str()) {
                        Some(&p) => p,
                        None => {
                            warn!(symbol = pos.symbol.as_str(), "no bar data; using entry_price for valuation");
                            d2f(pos.entry_price)
                        }
                    };
                    position_value += price * d2f(pos.shares);
                }
            }

            // Read peak value and cash from the last equity snapshot (for drawdown).
            let (peak, available_cash_f): (f64, f64) = {
                let mut s = conn.prepare(
                    "SELECT COALESCE(MAX(peak_value), 0.0), \
                            COALESCE((SELECT cash FROM portfolio_equity ORDER BY date DESC LIMIT 1), 0.0) \
                     FROM portfolio_equity",
                )?;
                s.query_map([], |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?)))?
                    .filter_map(|r| r.ok())
                    .next()
                    .unwrap_or((0.0, 0.0))
            };

            let portfolio_value = position_value;
            let new_peak = f64::max(peak, portfolio_value);

            // Persist today's equity snapshot so drawdown tracking stays current.
            let today = Utc::now().date_naive().to_string();
            let _ = conn.execute(
                "INSERT INTO portfolio_equity (date, portfolio_value, cash, peak_value) \
                 VALUES (?, ?, ?, ?) \
                 ON CONFLICT (date) DO UPDATE SET \
                    portfolio_value=EXCLUDED.portfolio_value, \
                    peak_value=GREATEST(portfolio_equity.peak_value, EXCLUDED.peak_value)",
                params![today, portfolio_value, available_cash_f, new_peak],
            );

            let current_drawdown = if new_peak > 0.0 {
                f2d(((new_peak - portfolio_value) / new_peak).max(0.0))
            } else {
                Decimal::ZERO
            };

            Ok(PortfolioState {
                open_positions,
                portfolio_value: f2d(portfolio_value),
                available_cash: f2d(available_cash_f),
                current_drawdown,
            })
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    #[instrument(skip(self), name = "db.get_open_position")]
    async fn get_open_position(&self, id: Uuid) -> StorageResult<Option<OpenPosition>> {
        let conn = self.conn.clone();
        let id_str = id.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Option<OpenPosition>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT id, symbol, shares, entry_price, CAST(entry_at AS VARCHAR) \
                 FROM portfolio_positions WHERE id=? AND status='open'",
            )?;
            Ok(stmt
                .query_map([&id_str], |row| {
                    let id_s: String = row.get(0)?;
                    let sym: String = row.get(1)?;
                    let shares: f64 = row.get(2)?;
                    let entry_price: f64 = row.get(3)?;
                    let entry_str: String = row.get(4)?;
                    Ok((id_s, sym, shares, entry_price, entry_str))
                })?
                .filter_map(|r| r.ok())
                .next()
                .map(|(id_s, sym, shares, entry_price, entry_str)| OpenPosition {
                    id: Uuid::parse_str(&id_s).unwrap_or_else(|_| Uuid::nil()),
                    symbol: Symbol::new(&sym),
                    shares: f2d(shares),
                    entry_price: f2d(entry_price),
                    entry_at: parse_datetime_utc(&entry_str),
                }))
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    #[instrument(skip(self), fields(symbol = symbol.as_str()), name = "db.open_position")]
    async fn open_position(
        &self,
        symbol: &Symbol,
        shares: Decimal,
        entry_price: Decimal,
        entry_at: DateTime<Utc>,
    ) -> StorageResult<OpenPosition> {
        let conn = self.conn.clone();
        let sym = symbol.as_str().to_string();
        let shares_f = shares.to_string().parse::<f64>().unwrap_or(0.0);
        let price_f = entry_price.to_string().parse::<f64>().unwrap_or(0.0);
        let entry_str = entry_at.to_rfc3339();
        tokio::task::spawn_blocking(move || -> StorageResult<OpenPosition> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let id = Uuid::new_v4();
            let id_str = id.to_string();
            conn.execute(
                "INSERT INTO portfolio_positions (id, symbol, shares, entry_price, entry_at, status) \
                 VALUES (?, ?, ?, ?, ?, 'open')",
                params![id_str, sym, shares_f, price_f, entry_str],
            )?;
            Ok(OpenPosition {
                id,
                symbol: Symbol::new(&sym),
                shares: f2d(shares_f),
                entry_price: f2d(price_f),
                entry_at,
            })
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    #[instrument(skip(self), name = "db.close_position")]
    async fn close_position(
        &self,
        id: Uuid,
        exit_price: Decimal,
        exit_at: DateTime<Utc>,
    ) -> StorageResult<()> {
        let conn = self.conn.clone();
        let id_str = id.to_string();
        let price_f = exit_price.to_string().parse::<f64>().unwrap_or(0.0);
        let exit_str = exit_at.to_rfc3339();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let rows = conn.execute(
                "UPDATE portfolio_positions \
                 SET status='closed', exit_price=?, exit_at=? \
                 WHERE id=? AND status='open'",
                params![price_f, exit_str, id_str],
            )?;
            if rows == 0 {
                return Err(StorageError::Provider(format!(
                    "Position {id_str} not found or already closed"
                )));
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn add_watch(&self, symbol: &Symbol) -> StorageResult<WatchItem> {
        let conn = self.conn.clone();
        let sym = symbol.as_str().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        tokio::task::spawn_blocking(move || -> StorageResult<WatchItem> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            conn.execute(
                "INSERT INTO portfolio_watchlist (symbol, added_at) VALUES (?, ?) \
                 ON CONFLICT (symbol) DO NOTHING",
                params![sym, now_str],
            )?;
            Ok(WatchItem {
                symbol: Symbol::new(&sym),
                added_at: now,
            })
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn remove_watch(&self, symbol: &Symbol) -> StorageResult<()> {
        let conn = self.conn.clone();
        let sym = symbol.as_str().to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            conn.execute(
                "DELETE FROM portfolio_watchlist WHERE symbol=?",
                params![sym],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn list_watches(&self) -> StorageResult<Vec<WatchItem>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<WatchItem>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT symbol, CAST(added_at AS VARCHAR) FROM portfolio_watchlist ORDER BY added_at DESC",
            )?;
            let items: Vec<WatchItem> = stmt
                .query_map([], |row| {
                    let sym: String = row.get(0)?;
                    let ts: String = row.get(1)?;
                    Ok((sym, ts))
                })?
                .filter_map(|r| r.ok())
                .map(|(sym, ts)| WatchItem {
                    symbol: Symbol::new(&sym),
                    added_at: parse_datetime_utc(&ts),
                })
                .collect();
            Ok(items)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn get_watch(&self, symbol: &Symbol) -> StorageResult<Option<WatchItem>> {
        let conn = self.conn.clone();
        let sym = symbol.as_str().to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Option<WatchItem>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT symbol, CAST(added_at AS VARCHAR) FROM portfolio_watchlist WHERE symbol=?",
            )?;
            let item = stmt
                .query_map([sym.as_str()], |row| {
                    let s: String = row.get(0)?;
                    let ts: String = row.get(1)?;
                    Ok((s, ts))
                })?
                .filter_map(|r| r.ok())
                .next()
                .map(|(s, ts)| WatchItem {
                    symbol: Symbol::new(&s),
                    added_at: parse_datetime_utc(&ts),
                });
            Ok(item)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }
}

// ── StrategyStorage ───────────────────────────────────────────────────────────

impl StrategyStorage for DuckDatabase {
    #[instrument(skip(self, row), fields(id = %row.id, name = %row.name), name = "db.insert_strategy_config")]
    async fn insert_strategy_config(&self, row: &StrategyConfigRow) -> StorageResult<()> {
        let conn = self.conn.clone();
        let row = row.clone();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            conn.execute(
                "INSERT INTO strategy_configs \
                    (id, name, description, composition, plugins_json, parameters_json, \
                     enabled, auto_execute, execution_mode, version, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (id) DO NOTHING",
                params![
                    row.id.to_string(),
                    row.name,
                    row.description,
                    row.composition,
                    row.plugins_json,
                    row.parameters_json,
                    row.enabled,
                    row.auto_execute,
                    row.execution_mode,
                    row.version as i32,
                    row.created_at.to_rfc3339(),
                    row.updated_at.to_rfc3339(),
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn get_strategy_config(&self, id: Uuid) -> StorageResult<Option<StrategyConfigRow>> {
        let conn = self.conn.clone();
        let table = self.t_strategy_configs();
        let id_str = id.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Option<StrategyConfigRow>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(&format!(
                "SELECT id, name, description, composition, plugins_json, parameters_json, \
                        enabled, auto_execute, execution_mode, version, created_at::VARCHAR, updated_at::VARCHAR \
                 FROM {table} WHERE id=?"
            ))?;
            Ok(stmt
                .query_map([&id_str], duck_row_to_strategy_config)?
                .filter_map(|r| r.ok())
                .next())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn list_strategy_configs(&self) -> StorageResult<Vec<StrategyConfigRow>> {
        let conn = self.conn.clone();
        let table = self.t_strategy_configs();
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<StrategyConfigRow>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(&format!(
                "SELECT id, name, description, composition, plugins_json, parameters_json, \
                        enabled, auto_execute, execution_mode, version, created_at::VARCHAR, updated_at::VARCHAR \
                 FROM {table} ORDER BY name"
            ))?;
            Ok(stmt
                .query_map([], duck_row_to_strategy_config)?
                .filter_map(|r| r.ok())
                .collect())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn update_strategy_config(&self, row: &StrategyConfigRow) -> StorageResult<()> {
        let conn = self.conn.clone();
        let row = row.clone();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            conn.execute(
                "UPDATE strategy_configs SET \
                    name=?, description=?, composition=?, plugins_json=?, parameters_json=?, \
                    enabled=?, auto_execute=?, execution_mode=?, version=?, updated_at=? \
                 WHERE id=?",
                params![
                    row.name,
                    row.description,
                    row.composition,
                    row.plugins_json,
                    row.parameters_json,
                    row.enabled,
                    row.auto_execute,
                    row.execution_mode,
                    row.version as i32,
                    row.updated_at.to_rfc3339(),
                    row.id.to_string(),
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    #[instrument(skip(self, row), fields(id = %row.id, config_id = %row.config_id), name = "db.insert_strategy_run")]
    async fn insert_strategy_run(&self, row: &StrategyRunRow) -> StorageResult<()> {
        let conn = self.conn.clone();
        let row = row.clone();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            conn.execute(
                "INSERT INTO strategy_runs \
                    (id, config_id, started_at, status, signal_count, \
                     error_message, config_snapshot_json) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![
                    row.id.to_string(),
                    row.config_id.to_string(),
                    row.started_at.to_rfc3339(),
                    row.status,
                    row.signal_count,
                    row.error_message,
                    row.config_snapshot_json,
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn complete_strategy_run(&self, row: &StrategyRunRow) -> StorageResult<()> {
        let conn = self.conn.clone();
        let row = row.clone();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            conn.execute(
                "UPDATE strategy_runs SET \
                    completed_at=?, status=?, signal_count=?, error_message=? \
                 WHERE id=?",
                params![
                    row.completed_at.map(|t| t.to_rfc3339()),
                    row.status,
                    row.signal_count,
                    row.error_message,
                    row.id.to_string(),
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn get_strategy_run(&self, id: Uuid) -> StorageResult<Option<StrategyRunRow>> {
        let conn = self.conn.clone();
        let id_str = id.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Option<StrategyRunRow>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT id, config_id, started_at::VARCHAR, completed_at::VARCHAR, status, signal_count, \
                        error_message, config_snapshot_json \
                 FROM strategy_runs WHERE id=?",
            )?;
            Ok(stmt
                .query_map([&id_str], duck_row_to_strategy_run)?
                .filter_map(|r| r.ok())
                .next())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn list_strategy_runs(
        &self,
        config_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<StrategyRunRow>> {
        let conn = self.conn.clone();
        let cid = config_id.map(|id| id.to_string());
        let lim = limit.unwrap_or(50) as i64;
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<StrategyRunRow>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let sql = match &cid {
                Some(id) => format!(
                    "SELECT id, config_id, started_at::VARCHAR, completed_at::VARCHAR, status, signal_count, \
                            error_message, config_snapshot_json \
                     FROM strategy_runs WHERE config_id='{}' \
                     ORDER BY started_at DESC LIMIT {}",
                    id, lim
                ),
                None => format!(
                    "SELECT id, config_id, started_at::VARCHAR, completed_at::VARCHAR, status, signal_count, \
                            error_message, config_snapshot_json \
                     FROM strategy_runs ORDER BY started_at DESC LIMIT {}",
                    lim
                ),
            };
            let mut stmt = conn.prepare(&sql)?;
            Ok(stmt
                .query_map([], duck_row_to_strategy_run)?
                .filter_map(|r| r.ok())
                .collect())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    #[instrument(skip(self, rows), fields(count = rows.len()), name = "db.insert_strategy_signals")]
    async fn insert_strategy_signals(&self, rows: &[StrategySignalRow]) -> StorageResult<()> {
        let conn = self.conn.clone();
        let rows: Vec<StrategySignalRow> = rows.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO strategy_signals \
                    (id, run_id, config_id, symbol, direction, identifier_score, timing_score, \
                     position_shares, position_notional, portfolio_fraction, \
                     rationale, analysis_brief, emitted_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )?;
            for r in &rows {
                stmt.execute(params![
                    r.id.to_string(),
                    r.run_id.to_string(),
                    r.config_id.to_string(),
                    r.symbol,
                    r.direction,
                    d2f(r.identifier_score),
                    d2f(r.timing_score),
                    r.position_shares.map(d2f),
                    r.position_notional.map(d2f),
                    r.portfolio_fraction.map(d2f),
                    r.rationale,
                    r.analysis_brief,
                    r.emitted_at.to_rfc3339(),
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    #[instrument(skip(self), name = "db.query_strategy_signals")]
    async fn query_strategy_signals(
        &self,
        run_id: Option<Uuid>,
        config_id: Option<Uuid>,
        symbol: Option<&Symbol>,
        since: Option<NaiveDate>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<StrategySignalRow>> {
        let conn = self.conn.clone();
        let rid = run_id.map(|id| id.to_string());
        let cid = config_id.map(|id| id.to_string());
        let sym = symbol.map(|s| s.as_str().to_string());
        let since_str = since.map(|d| d.to_string());
        let lim = limit.unwrap_or(200) as i64;
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<StrategySignalRow>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut condition_parts: Vec<&str> = Vec::new();
            let mut param_strings: Vec<String> = Vec::new();
            if let Some(id) = &rid {
                condition_parts.push("run_id=?");
                param_strings.push(id.clone());
            }
            if let Some(id) = &cid {
                condition_parts.push("config_id=?");
                param_strings.push(id.clone());
            }
            if let Some(s) = &sym {
                condition_parts.push("symbol=?");
                param_strings.push(s.clone());
            }
            if let Some(d) = &since_str {
                condition_parts.push("emitted_at>=?");
                param_strings.push(d.clone());
            }
            let where_clause = if condition_parts.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", condition_parts.join(" AND "))
            };
            let sql = format!(
                "SELECT id, run_id, config_id, symbol, direction, identifier_score, \
                        timing_score, position_shares, position_notional, portfolio_fraction, \
                        rationale, analysis_brief, emitted_at::VARCHAR \
                 FROM strategy_signals {where_clause} ORDER BY emitted_at DESC LIMIT {lim}"
            );
            let mut stmt = conn.prepare(&sql)?;
            let param_refs: Vec<&dyn duckdb::ToSql> =
                param_strings.iter().map(|s| s as &dyn duckdb::ToSql).collect();
            Ok(stmt
                .query_map(param_refs.as_slice(), duck_row_to_signal)?
                .filter_map(|r| r.ok())
                .collect())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn get_strategy_signal(&self, id: Uuid) -> StorageResult<Option<StrategySignalRow>> {
        let conn = self.conn.clone();
        let id_str = id.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Option<StrategySignalRow>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT id, run_id, config_id, symbol, direction, identifier_score, \
                        timing_score, position_shares, position_notional, portfolio_fraction, \
                        rationale, analysis_brief, emitted_at::VARCHAR \
                 FROM strategy_signals WHERE id=?",
            )?;
            Ok(stmt
                .query_map([&id_str], duck_row_to_signal)?
                .filter_map(|r| r.ok())
                .next())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }
}

// ── BacktestStorage ───────────────────────────────────────────────────────────

impl BacktestStorage for DuckDatabase {
    async fn insert_backtest_run(&self, row: &BacktestRunRow) -> StorageResult<()> {
        let conn = self.conn.clone();
        let row = row.clone();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            conn.execute(
                "INSERT INTO backtest_runs \
                    (id, config_id, config_snapshot_json, from_date, to_date, \
                     initial_capital, status, started_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    row.id.to_string(),
                    row.config_id.to_string(),
                    row.config_snapshot_json,
                    row.from_date.to_string(),
                    row.to_date.to_string(),
                    d2f(row.initial_capital),
                    row.status,
                    row.started_at.to_rfc3339(),
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn complete_backtest_run(&self, row: &BacktestRunRow) -> StorageResult<()> {
        let conn = self.conn.clone();
        let row = row.clone();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            conn.execute(
                "UPDATE backtest_runs SET \
                    final_capital=?, cagr=?, sharpe_ratio=?, sortino_ratio=?, \
                    max_drawdown=?, max_drawdown_days=?, win_rate=?, profit_factor=?, \
                    expectancy=?, total_trades=?, avg_hold_days=?, \
                    status=?, completed_at=?, error_message=? \
                 WHERE id=?",
                params![
                    row.final_capital.map(d2f),
                    row.cagr.map(d2f),
                    row.sharpe_ratio.map(d2f),
                    row.sortino_ratio.map(d2f),
                    row.max_drawdown.map(d2f),
                    row.max_drawdown_days,
                    row.win_rate.map(d2f),
                    row.profit_factor.map(d2f),
                    row.expectancy.map(d2f),
                    row.total_trades,
                    row.avg_hold_days.map(d2f),
                    row.status,
                    row.completed_at.map(|t| t.to_rfc3339()),
                    row.error_message,
                    row.id.to_string(),
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn get_backtest_run(&self, id: Uuid) -> StorageResult<Option<BacktestRunRow>> {
        let conn = self.conn.clone();
        let id_str = id.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Option<BacktestRunRow>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT id, config_id, config_snapshot_json, from_date::VARCHAR, to_date::VARCHAR, \
                        initial_capital, final_capital, cagr, sharpe_ratio, sortino_ratio, \
                        max_drawdown, max_drawdown_days, win_rate, profit_factor, expectancy, \
                        total_trades, avg_hold_days, status, started_at::VARCHAR, completed_at::VARCHAR, error_message \
                 FROM backtest_runs WHERE id=?",
            )?;
            Ok(stmt
                .query_map([&id_str], duck_row_to_backtest_run)?
                .filter_map(|r| r.ok())
                .next())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn list_backtest_runs(
        &self,
        config_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<BacktestRunRow>> {
        let conn = self.conn.clone();
        let cid = config_id.map(|id| id.to_string());
        let lim = limit.unwrap_or(50) as i64;
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<BacktestRunRow>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let sql = match &cid {
                Some(id) => format!(
                    "SELECT id, config_id, config_snapshot_json, from_date::VARCHAR, to_date::VARCHAR, \
                            initial_capital, final_capital, cagr, sharpe_ratio, sortino_ratio, \
                            max_drawdown, max_drawdown_days, win_rate, profit_factor, expectancy, \
                            total_trades, avg_hold_days, status, started_at::VARCHAR, completed_at::VARCHAR, error_message \
                     FROM backtest_runs WHERE config_id='{}' ORDER BY started_at DESC LIMIT {}",
                    id, lim
                ),
                None => format!(
                    "SELECT id, config_id, config_snapshot_json, from_date::VARCHAR, to_date::VARCHAR, \
                            initial_capital, final_capital, cagr, sharpe_ratio, sortino_ratio, \
                            max_drawdown, max_drawdown_days, win_rate, profit_factor, expectancy, \
                            total_trades, avg_hold_days, status, started_at::VARCHAR, completed_at::VARCHAR, error_message \
                     FROM backtest_runs ORDER BY started_at DESC LIMIT {}",
                    lim
                ),
            };
            let mut stmt = conn.prepare(&sql)?;
            Ok(stmt
                .query_map([], duck_row_to_backtest_run)?
                .filter_map(|r| r.ok())
                .collect())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_backtest_trades(&self, rows: &[BacktestTradeRow]) -> StorageResult<()> {
        let conn = self.conn.clone();
        let rows: Vec<BacktestTradeRow> = rows.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO backtest_trades \
                    (id, run_id, symbol, direction, entry_date, entry_price, \
                     exit_date, exit_price, shares, gross_pnl, commission, net_pnl, hold_days) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )?;
            for r in &rows {
                stmt.execute(params![
                    r.id.to_string(),
                    r.run_id.to_string(),
                    r.symbol,
                    r.direction,
                    r.entry_date.to_string(),
                    d2f(r.entry_price),
                    r.exit_date.map(|d| d.to_string()),
                    r.exit_price.map(d2f),
                    d2f(r.shares),
                    r.gross_pnl.map(d2f),
                    d2f(r.commission),
                    r.net_pnl.map(d2f),
                    r.hold_days,
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn get_backtest_trades(&self, run_id: Uuid) -> StorageResult<Vec<BacktestTradeRow>> {
        let conn = self.conn.clone();
        let id_str = run_id.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<BacktestTradeRow>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT id, run_id, symbol, direction, entry_date::VARCHAR, entry_price, \
                        exit_date::VARCHAR, exit_price, shares, gross_pnl, commission, net_pnl, hold_days \
                 FROM backtest_trades WHERE run_id=? ORDER BY entry_date ASC",
            )?;
            Ok(stmt
                .query_map([&id_str], duck_row_to_backtest_trade)?
                .filter_map(|r| r.ok())
                .collect())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_equity_curve(&self, points: &[EquityCurvePoint]) -> StorageResult<()> {
        let conn = self.conn.clone();
        let points: Vec<EquityCurvePoint> = points.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO backtest_equity_curve (run_id, date, portfolio_value, cash, drawdown) \
                 VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT (run_id, date) DO UPDATE SET \
                    portfolio_value=EXCLUDED.portfolio_value, cash=EXCLUDED.cash, \
                    drawdown=EXCLUDED.drawdown",
            )?;
            for p in &points {
                stmt.execute(params![
                    p.run_id.to_string(),
                    p.date.to_string(),
                    d2f(p.portfolio_value),
                    d2f(p.cash),
                    d2f(p.drawdown),
                ])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn get_equity_curve(&self, run_id: Uuid) -> StorageResult<Vec<EquityCurvePoint>> {
        let conn = self.conn.clone();
        let id_str = run_id.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<EquityCurvePoint>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT run_id, date::VARCHAR, portfolio_value, cash, drawdown \
                 FROM backtest_equity_curve WHERE run_id=? ORDER BY date ASC",
            )?;
            Ok(stmt
                .query_map([&id_str], duck_row_to_equity_point)?
                .filter_map(|r| r.ok())
                .collect())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }
}

// ── ChatStorage ──────────────────────────────────────────────────────────────

impl ChatStorage for DuckDatabase {
    async fn upsert_chat_session(&self, row: &ChatSessionRow) -> StorageResult<()> {
        let conn = self.conn.clone();
        let row = row.clone();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            conn.execute(
                "INSERT INTO chat_sessions \
                    (id, title, persona_id, depth, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (id) DO UPDATE SET \
                    title=excluded.title, persona_id=excluded.persona_id, depth=excluded.depth, \
                    updated_at=excluded.updated_at",
                params![
                    row.id.to_string(),
                    row.title,
                    row.persona_id,
                    row.depth,
                    row.created_at.to_rfc3339(),
                    row.updated_at.to_rfc3339(),
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn list_chat_sessions(&self, limit: Option<u32>) -> StorageResult<Vec<ChatSessionRow>> {
        let conn = self.conn.clone();
        let limit = limit.unwrap_or(50).min(200) as i64;
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<ChatSessionRow>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT id, title, persona_id, depth, created_at::VARCHAR, updated_at::VARCHAR \
                 FROM chat_sessions ORDER BY updated_at DESC LIMIT ?",
            )?;
            Ok(stmt
                .query_map([limit], duck_row_to_chat_session)?
                .filter_map(|r| r.ok())
                .collect())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn get_chat_session(&self, id: Uuid) -> StorageResult<Option<ChatSessionRow>> {
        let conn = self.conn.clone();
        let id_str = id.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Option<ChatSessionRow>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT id, title, persona_id, depth, created_at::VARCHAR, updated_at::VARCHAR \
                 FROM chat_sessions WHERE id=?",
            )?;
            Ok(stmt
                .query_map([&id_str], duck_row_to_chat_session)?
                .filter_map(|r| r.ok())
                .next())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_chat_messages(&self, rows: &[ChatMessageRow]) -> StorageResult<()> {
        let conn = self.conn.clone();
        let rows = rows.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            for row in rows {
                conn.execute(
                    "INSERT INTO chat_messages \
                        (id, session_id, ordinal, role, content, created_at) \
                     VALUES (?, ?, ?, ?, ?, ?) \
                     ON CONFLICT (session_id, ordinal) DO UPDATE SET \
                        role=excluded.role, content=excluded.content, created_at=excluded.created_at",
                    params![
                        row.id.to_string(),
                        row.session_id.to_string(),
                        row.ordinal,
                        row.role,
                        row.content,
                        row.created_at.to_rfc3339(),
                    ],
                )?;
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn list_chat_messages(&self, session_id: Uuid) -> StorageResult<Vec<ChatMessageRow>> {
        let conn = self.conn.clone();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<ChatMessageRow>> {
            let conn = conn
                .lock()
                .map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT id, session_id, ordinal, role, content, created_at::VARCHAR \
                 FROM chat_messages WHERE session_id=? ORDER BY ordinal ASC",
            )?;
            Ok(stmt
                .query_map([&session_id], duck_row_to_chat_message)?
                .filter_map(|r| r.ok())
                .collect())
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Decimal → f64 for DuckDB binding (DuckDB stores financials as DOUBLE).
fn d2f(d: Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

/// f64 → Decimal for reading back from DuckDB.
fn f2d(v: f64) -> Decimal {
    Decimal::from_str(&format!("{v:.10}")).unwrap_or_default()
}

fn parse_date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap())
}

fn parse_datetime(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .unwrap_or_else(|_| {
            NaiveDate::from_ymd_opt(1970, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
        })
}

// ── Row mappers ───────────────────────────────────────────────────────────────

fn duck_row_to_ticker(row: &duckdb::Row) -> duckdb::Result<Ticker> {
    Ok(Ticker {
        symbol: Symbol::new(&row.get::<_, String>(0)?),
        exchange: row.get::<_, Option<String>>(1)?.map(|s| Exchange::new(&s)),
        name: row.get::<_, Option<String>>(2)?,
        country: row.get::<_, Option<String>>(3)?,
        industry: None,
        sector: None,
        ipoyear: row.get::<_, Option<String>>(6)?,
        marketcap: row.get::<_, Option<f64>>(7)?.map(f2d),
        description: row.get::<_, Option<String>>(8)?,
        active: row.get::<_, bool>(9).unwrap_or(true),
    })
}

fn duck_row_to_news(row: &duckdb::Row) -> duckdb::Result<NewsStory> {
    let symbol: Option<String> = row.get(0)?;
    let about = match symbol {
        Some(s) => NewsAbout::Symbol(Symbol::new(&s)),
        None => NewsAbout::Sector("Market".to_string()),
    };
    let pub_str: Option<String> = row.get(6)?;
    let fet_str: Option<String> = row.get(7)?;
    let epoch = || NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
    Ok(NewsStory {
        about,
        headline: row.get::<_, String>(1)?,
        summary: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        story: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        url: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        evaluation: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        published_at: pub_str.as_deref().map(parse_date).unwrap_or_else(epoch),
        fetched_at: fet_str.as_deref().map(parse_date).unwrap_or_else(epoch),
    })
}

fn duck_row_to_income(row: &duckdb::Row) -> duckdb::Result<IncomeStatement> {
    let period_str: String = row.get(1)?;
    Ok(IncomeStatement {
        symbol: Symbol::new(&row.get::<_, String>(0)?),
        period_end: parse_date(&period_str),
        period_type: row.get::<_, String>(2).unwrap_or_else(|_| "annual".to_string()),
        revenue: f2d(row.get::<_, f64>(3).unwrap_or(0.0)),
        cogs: f2d(row.get::<_, f64>(4).unwrap_or(0.0)),
        operating_income: f2d(row.get::<_, f64>(5).unwrap_or(0.0)),
        ebit: f2d(row.get::<_, f64>(6).unwrap_or(0.0)),
        net_income: f2d(row.get::<_, f64>(7).unwrap_or(0.0)),
        eps: f2d(row.get::<_, f64>(8).unwrap_or(0.0)),
        interest_expense: f2d(row.get::<_, f64>(9).unwrap_or(0.0)),
        tax_expense: f2d(row.get::<_, f64>(10).unwrap_or(0.0)),
    })
}

fn duck_row_to_balance(row: &duckdb::Row) -> duckdb::Result<BalanceSheet> {
    let period_str: String = row.get(1)?;
    Ok(BalanceSheet {
        symbol: Symbol::new(&row.get::<_, String>(0)?),
        period_end: parse_date(&period_str),
        total_assets: f2d(row.get::<_, f64>(2).unwrap_or(0.0)),
        total_debt: f2d(row.get::<_, f64>(3).unwrap_or(0.0)),
        total_equity: f2d(row.get::<_, f64>(4).unwrap_or(0.0)),
        cash: f2d(row.get::<_, f64>(5).unwrap_or(0.0)),
    })
}

fn duck_row_to_cash_flow(row: &duckdb::Row) -> duckdb::Result<CashFlowStatement> {
    let period_str: String = row.get(1)?;
    Ok(CashFlowStatement {
        symbol: Symbol::new(&row.get::<_, String>(0)?),
        period_end: parse_date(&period_str),
        operating_cash_flow: f2d(row.get::<_, f64>(2).unwrap_or(0.0)),
        capex: f2d(row.get::<_, f64>(3).unwrap_or(0.0)),
    })
}

fn duck_row_to_dividend(row: &duckdb::Row) -> duckdb::Result<DividendEvent> {
    let ex_str: String = row.get(1)?;
    let pay_str: String = row.get(2)?;
    Ok(DividendEvent {
        symbol: Symbol::new(&row.get::<_, String>(0)?),
        ex_date: parse_date(&ex_str),
        payment_date: parse_date(&pay_str),
        amount: f2d(row.get::<_, f64>(3).unwrap_or(0.0)),
    })
}

fn duck_row_to_split(row: &duckdb::Row) -> duckdb::Result<StockSplitEvent> {
    let date_str: String = row.get(1)?;
    Ok(StockSplitEvent {
        symbol: Symbol::new(&row.get::<_, String>(0)?),
        date: parse_date(&date_str),
        ratio: f2d(row.get::<_, f64>(2).unwrap_or(1.0)),
    })
}

fn duck_row_to_candle(row: &duckdb::Row) -> duckdb::Result<CandleEntry> {
    let ts_str: String = row.get(0)?;
    Ok(CandleEntry {
        timestamp: parse_datetime(&ts_str),
        open: f2d(row.get::<_, f64>(1)?),
        high: f2d(row.get::<_, f64>(2)?),
        low: f2d(row.get::<_, f64>(3)?),
        close: f2d(row.get::<_, f64>(4)?),
        volume: row.get::<_, i64>(5)? as u64,
    })
}

fn duck_row_to_daily_candle(row: &duckdb::Row) -> duckdb::Result<DailyCandleEntry> {
    let ts_str: String = row.get(0)?;
    Ok(DailyCandleEntry {
        date: parse_datetime(&ts_str).date(),
        open: f2d(row.get::<_, f64>(1)?),
        high: f2d(row.get::<_, f64>(2)?),
        low: f2d(row.get::<_, f64>(3)?),
        close: f2d(row.get::<_, f64>(4)?),
        volume: row.get::<_, i64>(5)? as u64,
    })
}

fn duck_row_to_macro(row: &duckdb::Row) -> duckdb::Result<MacroSeriesPoint> {
    let date_str: String = row.get(1)?;
    let fetched_str: String = row.get(3)?;
    Ok(MacroSeriesPoint {
        series_id: row.get(0)?,
        date: parse_date(&date_str),
        value: row.get::<_, Option<f64>>(2)?.map(f2d),
        fetched_at: parse_datetime_utc(&fetched_str),
    })
}

fn duck_row_to_strategy_config(row: &duckdb::Row) -> duckdb::Result<StrategyConfigRow> {
    let id_str: String = row.get(0)?;
    let created_str: String = row.get(10)?;
    let updated_str: String = row.get(11)?;
    Ok(StrategyConfigRow {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
        name: row.get(1)?,
        description: row.get(2)?,
        composition: row.get(3)?,
        plugins_json: row.get(4)?,
        parameters_json: row.get(5)?,
        enabled: row.get(6)?,
        auto_execute: row.get(7)?,
        execution_mode: row.get(8)?,
        version: row.get::<_, i32>(9)? as u32,
        created_at: parse_datetime_utc(&created_str),
        updated_at: parse_datetime_utc(&updated_str),
    })
}

fn duck_row_to_strategy_run(row: &duckdb::Row) -> duckdb::Result<StrategyRunRow> {
    let id_str: String = row.get(0)?;
    let cid_str: String = row.get(1)?;
    let started_str: String = row.get(2)?;
    let completed_str: Option<String> = row.get(3)?;
    Ok(StrategyRunRow {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
        config_id: Uuid::parse_str(&cid_str).unwrap_or_else(|_| Uuid::nil()),
        started_at: parse_datetime_utc(&started_str),
        completed_at: completed_str.as_deref().map(parse_datetime_utc),
        status: row.get(4)?,
        signal_count: row.get(5)?,
        error_message: row.get(6)?,
        config_snapshot_json: row.get(7)?,
    })
}

fn duck_row_to_signal(row: &duckdb::Row) -> duckdb::Result<StrategySignalRow> {
    let id_str: String = row.get(0)?;
    let rid_str: String = row.get(1)?;
    let cid_str: String = row.get(2)?;
    let emitted_str: String = row.get(12)?;
    Ok(StrategySignalRow {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
        run_id: Uuid::parse_str(&rid_str).unwrap_or_else(|_| Uuid::nil()),
        config_id: Uuid::parse_str(&cid_str).unwrap_or_else(|_| Uuid::nil()),
        symbol: row.get(3)?,
        direction: row.get(4)?,
        identifier_score: f2d(row.get::<_, f64>(5).unwrap_or(0.0)),
        timing_score: f2d(row.get::<_, f64>(6).unwrap_or(0.0)),
        position_shares: row.get::<_, Option<f64>>(7)?.map(f2d),
        position_notional: row.get::<_, Option<f64>>(8)?.map(f2d),
        portfolio_fraction: row.get::<_, Option<f64>>(9)?.map(f2d),
        rationale: row.get(10)?,
        analysis_brief: row.get(11)?,
        emitted_at: parse_datetime_utc(&emitted_str),
    })
}

fn duck_row_to_backtest_run(row: &duckdb::Row) -> duckdb::Result<BacktestRunRow> {
    let id_str: String = row.get(0)?;
    let cid_str: String = row.get(1)?;
    let from_str: String = row.get(3)?;
    let to_str: String = row.get(4)?;
    let started_str: String = row.get(18)?;
    let completed_str: Option<String> = row.get(19)?;
    Ok(BacktestRunRow {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
        config_id: Uuid::parse_str(&cid_str).unwrap_or_else(|_| Uuid::nil()),
        config_snapshot_json: row.get(2)?,
        from_date: parse_date(&from_str),
        to_date: parse_date(&to_str),
        initial_capital: f2d(row.get::<_, f64>(5).unwrap_or(0.0)),
        final_capital: row.get::<_, Option<f64>>(6)?.map(f2d),
        cagr: row.get::<_, Option<f64>>(7)?.map(f2d),
        sharpe_ratio: row.get::<_, Option<f64>>(8)?.map(f2d),
        sortino_ratio: row.get::<_, Option<f64>>(9)?.map(f2d),
        max_drawdown: row.get::<_, Option<f64>>(10)?.map(f2d),
        max_drawdown_days: row.get(11)?,
        win_rate: row.get::<_, Option<f64>>(12)?.map(f2d),
        profit_factor: row.get::<_, Option<f64>>(13)?.map(f2d),
        expectancy: row.get::<_, Option<f64>>(14)?.map(f2d),
        total_trades: row.get(15)?,
        avg_hold_days: row.get::<_, Option<f64>>(16)?.map(f2d),
        status: row.get(17)?,
        started_at: parse_datetime_utc(&started_str),
        completed_at: completed_str.as_deref().map(parse_datetime_utc),
        error_message: row.get(20)?,
    })
}

fn duck_row_to_backtest_trade(row: &duckdb::Row) -> duckdb::Result<BacktestTradeRow> {
    let id_str: String = row.get(0)?;
    let rid_str: String = row.get(1)?;
    let entry_str: String = row.get(4)?;
    let exit_str: Option<String> = row.get(6)?;
    Ok(BacktestTradeRow {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
        run_id: Uuid::parse_str(&rid_str).unwrap_or_else(|_| Uuid::nil()),
        symbol: row.get(2)?,
        direction: row.get(3)?,
        entry_date: parse_date(&entry_str),
        entry_price: f2d(row.get::<_, f64>(5).unwrap_or(0.0)),
        exit_date: exit_str.as_deref().map(parse_date),
        exit_price: row.get::<_, Option<f64>>(7)?.map(f2d),
        shares: f2d(row.get::<_, f64>(8).unwrap_or(0.0)),
        gross_pnl: row.get::<_, Option<f64>>(9)?.map(f2d),
        commission: f2d(row.get::<_, f64>(10).unwrap_or(0.0)),
        net_pnl: row.get::<_, Option<f64>>(11)?.map(f2d),
        hold_days: row.get(12)?,
    })
}

fn duck_row_to_equity_point(row: &duckdb::Row) -> duckdb::Result<EquityCurvePoint> {
    let rid_str: String = row.get(0)?;
    let date_str: String = row.get(1)?;
    Ok(EquityCurvePoint {
        run_id: Uuid::parse_str(&rid_str).unwrap_or_else(|_| Uuid::nil()),
        date: parse_date(&date_str),
        portfolio_value: f2d(row.get::<_, f64>(2).unwrap_or(0.0)),
        cash: f2d(row.get::<_, f64>(3).unwrap_or(0.0)),
        drawdown: f2d(row.get::<_, f64>(4).unwrap_or(0.0)),
    })
}

fn duck_row_to_chat_session(row: &duckdb::Row) -> duckdb::Result<ChatSessionRow> {
    let id_str: String = row.get(0)?;
    let created_str: String = row.get(4)?;
    let updated_str: String = row.get(5)?;
    Ok(ChatSessionRow {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
        title: row.get(1)?,
        persona_id: row.get(2)?,
        depth: row.get(3)?,
        created_at: parse_datetime_utc(&created_str),
        updated_at: parse_datetime_utc(&updated_str),
    })
}

fn duck_row_to_chat_message(row: &duckdb::Row) -> duckdb::Result<ChatMessageRow> {
    let id_str: String = row.get(0)?;
    let session_id_str: String = row.get(1)?;
    let created_str: String = row.get(5)?;
    Ok(ChatMessageRow {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
        session_id: Uuid::parse_str(&session_id_str).unwrap_or_else(|_| Uuid::nil()),
        ordinal: row.get(2)?,
        role: row.get(3)?,
        content: row.get(4)?,
        created_at: parse_datetime_utc(&created_str),
    })
}

fn parse_datetime_utc(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .map(|ndt| Utc.from_utc_datetime(&ndt))
        })
        .unwrap_or_else(|_| Utc.timestamp_opt(0, 0).unwrap())
}
