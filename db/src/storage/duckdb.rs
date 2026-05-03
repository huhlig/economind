//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! DuckDB in-process analytical storage.
//!
//! DuckDB is used as a fast, embedded analytics engine for strategy runs and
//! backtesting. It mirrors a subset of the PostgreSQL operational data and is
//! refreshed nightly by the `db.duckdb_sync` scheduled job.
//!
//! All async methods wrap DuckDB's synchronous API via `tokio::task::spawn_blocking`
//! so they compose safely with the rest of the async runtime.

use crate::StorageResult;
use crate::storage::{
    CandleStorage, MetadataStorage, TickStorage, TickerQuery,
};
use crate::StorageError;
use economind_core::model::{
    BalanceSheet, CandleEntry, CashFlowStatement, DailyCandleEntry, DividendEvent,
    IncomeStatement, Interval, NewsStory, StockSplitEvent, Symbol, Ticker, TradeTick,
    NewsAbout, Exchange,
};
use chrono::{NaiveDate, NaiveDateTime};
use duckdb::{params, Connection};
use futures_core::stream::BoxStream;
use rust_decimal::Decimal;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

// ── DuckDatabase ─────────────────────────────────────────────────────────────

/// DuckDB-backed analytical storage.
///
/// Wraps a connection behind an `Arc<Mutex<Connection>>` so it can be
/// cloned and shared across tasks. DuckDB supports a single writer at a time;
/// the mutex ensures sequential access.
#[derive(Clone)]
pub struct DuckDatabase {
    path: PathBuf,
    conn: Arc<Mutex<Connection>>,
}

impl DuckDatabase {
    /// Open (or create) a DuckDB database at the given path and apply the schema.
    pub fn open<P: AsRef<Path>>(path: P) -> StorageResult<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)?;
        conn.execute_batch(include_str!("schema.sql"))?;
        Ok(Self { path, conn: Arc::new(Mutex::new(conn)) })
    }

    /// Open an in-memory DuckDB instance (useful for backtesting / tests).
    pub fn in_memory() -> StorageResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(include_str!("schema.sql"))?;
        Ok(Self { path: PathBuf::from(":memory:"), conn: Arc::new(Mutex::new(conn)) })
    }

    /// Path to the underlying database file (`":memory:"` for in-memory instances).
    pub fn path(&self) -> &Path { &self.path }

    /// Execute a raw SQL batch (useful for the sync job inserting bulk data).
    pub fn execute_batch(&self, sql: &str) -> StorageResult<()> {
        let conn = self.conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
        conn.execute_batch(sql)?;
        Ok(())
    }
}

// ── Interval helper ──────────────────────────────────────────────────────────

fn interval_str(interval: Interval) -> &'static str {
    match interval {
        Interval::OneMinute     => "1m",
        Interval::FiveMinute    => "5m",
        Interval::FifteenMinute => "15m",
        Interval::OneHour       => "1h",
        Interval::OneDay        => "1d",
    }
}

// ── MetadataStorage ──────────────────────────────────────────────────────────

impl MetadataStorage for DuckDatabase {
    async fn list_tickers(&self) -> StorageResult<BoxStream<'static, Symbol>> {
        let conn = self.conn.clone();
        let symbols = tokio::task::spawn_blocking(move || -> StorageResult<Vec<Symbol>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT symbol FROM instruments WHERE active = TRUE ORDER BY symbol",
            )?;
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
        let tickers = tokio::task::spawn_blocking(move || -> StorageResult<Vec<Ticker>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT symbol, exchange, name, country, industry, sector, \
                        ipoyear, marketcap, description, active \
                 FROM instruments WHERE active = TRUE ORDER BY symbol",
            )?;
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
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
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
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
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
        let conn  = self.conn.clone();
        let sym   = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end   = time_range.map(|r| r.end.to_string());

        let stories = tokio::task::spawn_blocking(move || -> StorageResult<Vec<NewsStory>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let stories = match (&sym, &start, &end) {
                (Some(s), Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, headline, summary, story, url, evaluation, \
                                published_at, fetched_at FROM news \
                         WHERE symbol = ? AND published_at >= ? AND published_at < ? \
                         ORDER BY published_at DESC")?;
                    stmt.query_map(params![s, st, e], duck_row_to_news)?
                        .filter_map(|r| r.ok()).collect()
                }
                (Some(s), None, _) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, headline, summary, story, url, evaluation, \
                                published_at, fetched_at FROM news \
                         WHERE symbol = ? ORDER BY published_at DESC")?;
                    stmt.query_map(params![s], duck_row_to_news)?
                        .filter_map(|r| r.ok()).collect()
                }
                (None, Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, headline, summary, story, url, evaluation, \
                                published_at, fetched_at FROM news \
                         WHERE published_at >= ? AND published_at < ? \
                         ORDER BY published_at DESC")?;
                    stmt.query_map(params![st, e], duck_row_to_news)?
                        .filter_map(|r| r.ok()).collect()
                }
                _ => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, headline, summary, story, url, evaluation, \
                                published_at, fetched_at FROM news \
                         ORDER BY published_at DESC")?;
                    stmt.query_map([], duck_row_to_news)?
                        .filter_map(|r| r.ok()).collect()
                }
            };
            Ok(stories)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))??;
        Ok(Box::pin(futures::stream::iter(stories)))
    }

    async fn insert_income_statements(&self, items: &[IncomeStatement]) -> StorageResult<()> {
        let conn  = self.conn.clone();
        let items: Vec<IncomeStatement> = items.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO income_statements \
                    (symbol, period_end, period_type, revenue, cogs, operating_income, \
                     ebit, net_income, eps, interest_expense, tax_expense) \
                 VALUES (?, ?, 'annual', ?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (symbol, period_end, period_type) DO UPDATE SET \
                    revenue=EXCLUDED.revenue, cogs=EXCLUDED.cogs, \
                    operating_income=EXCLUDED.operating_income, ebit=EXCLUDED.ebit, \
                    net_income=EXCLUDED.net_income, eps=EXCLUDED.eps, \
                    interest_expense=EXCLUDED.interest_expense, \
                    tax_expense=EXCLUDED.tax_expense",
            )?;
            for item in &items {
                stmt.execute(params![
                    item.symbol.as_str(), item.period_end.to_string(),
                    d2f(item.revenue), d2f(item.cogs), d2f(item.operating_income),
                    d2f(item.ebit), d2f(item.net_income), d2f(item.eps),
                    d2f(item.interest_expense), d2f(item.tax_expense),
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
        let conn  = self.conn.clone();
        let sym   = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end   = time_range.map(|r| r.end.to_string());
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<IncomeStatement>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let results: Vec<IncomeStatement> = match (&sym, &start, &end) {
                (Some(s), Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, revenue, cogs, operating_income, \
                                ebit, net_income, eps, interest_expense, tax_expense \
                         FROM income_statements \
                         WHERE symbol=? AND period_end>=? AND period_end<? \
                         ORDER BY period_end DESC")?;
                    stmt.query_map(params![s,st,e], duck_row_to_income)?
                        .filter_map(|r| r.ok()).collect()
                }
                (Some(s), None, _) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, revenue, cogs, operating_income, \
                                ebit, net_income, eps, interest_expense, tax_expense \
                         FROM income_statements WHERE symbol=? ORDER BY period_end DESC")?;
                    stmt.query_map(params![s], duck_row_to_income)?
                        .filter_map(|r| r.ok()).collect()
                }
                (None, Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, revenue, cogs, operating_income, \
                                ebit, net_income, eps, interest_expense, tax_expense \
                         FROM income_statements \
                         WHERE period_end>=? AND period_end<? ORDER BY period_end DESC")?;
                    stmt.query_map(params![st,e], duck_row_to_income)?
                        .filter_map(|r| r.ok()).collect()
                }
                _ => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, revenue, cogs, operating_income, \
                                ebit, net_income, eps, interest_expense, tax_expense \
                         FROM income_statements ORDER BY symbol, period_end DESC")?;
                    stmt.query_map([], duck_row_to_income)?
                        .filter_map(|r| r.ok()).collect()
                }
            };
            Ok(results)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_balance_sheets(&self, items: &[BalanceSheet]) -> StorageResult<()> {
        let conn  = self.conn.clone();
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
        let conn  = self.conn.clone();
        let sym   = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end   = time_range.map(|r| r.end.to_string());
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<BalanceSheet>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let results = match (&sym, &start, &end) {
                (Some(s), Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, total_assets, total_debt, total_equity, cash \
                         FROM balance_sheets WHERE symbol=? AND period_end>=? AND period_end<? \
                         ORDER BY period_end DESC")?;
                    stmt.query_map(params![s,st,e], duck_row_to_balance)?
                        .filter_map(|r| r.ok()).collect()
                }
                (Some(s), None, _) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, total_assets, total_debt, total_equity, cash \
                         FROM balance_sheets WHERE symbol=? ORDER BY period_end DESC")?;
                    stmt.query_map(params![s], duck_row_to_balance)?
                        .filter_map(|r| r.ok()).collect()
                }
                (None, Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, total_assets, total_debt, total_equity, cash \
                         FROM balance_sheets WHERE period_end>=? AND period_end<? \
                         ORDER BY period_end DESC")?;
                    stmt.query_map(params![st,e], duck_row_to_balance)?
                        .filter_map(|r| r.ok()).collect()
                }
                _ => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, total_assets, total_debt, total_equity, cash \
                         FROM balance_sheets ORDER BY symbol, period_end DESC")?;
                    stmt.query_map([], duck_row_to_balance)?
                        .filter_map(|r| r.ok()).collect()
                }
            };
            Ok(results)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_cash_flow_statements(&self, items: &[CashFlowStatement]) -> StorageResult<()> {
        let conn  = self.conn.clone();
        let items: Vec<CashFlowStatement> = items.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO cash_flow_statements \
                    (symbol, period_end, period_type, operating_cash_flow, capex, free_cash_flow) \
                 VALUES (?, ?, 'annual', ?, ?, ?) \
                 ON CONFLICT (symbol, period_end, period_type) DO UPDATE SET \
                    operating_cash_flow=EXCLUDED.operating_cash_flow, \
                    capex=EXCLUDED.capex, free_cash_flow=EXCLUDED.free_cash_flow",
            )?;
            for item in &items {
                let ocf   = d2f(item.operating_cash_flow);
                let capex = d2f(item.capex);
                stmt.execute(params![
                    item.symbol.as_str(), item.period_end.to_string(),
                    ocf, capex, ocf - capex,
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
        let conn  = self.conn.clone();
        let sym   = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end   = time_range.map(|r| r.end.to_string());
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<CashFlowStatement>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let results = match (&sym, &start, &end) {
                (Some(s), Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, operating_cash_flow, capex \
                         FROM cash_flow_statements \
                         WHERE symbol=? AND period_end>=? AND period_end<? \
                         ORDER BY period_end DESC")?;
                    stmt.query_map(params![s,st,e], duck_row_to_cash_flow)?
                        .filter_map(|r| r.ok()).collect()
                }
                (Some(s), None, _) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, operating_cash_flow, capex \
                         FROM cash_flow_statements WHERE symbol=? ORDER BY period_end DESC")?;
                    stmt.query_map(params![s], duck_row_to_cash_flow)?
                        .filter_map(|r| r.ok()).collect()
                }
                (None, Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, operating_cash_flow, capex \
                         FROM cash_flow_statements \
                         WHERE period_end>=? AND period_end<? ORDER BY period_end DESC")?;
                    stmt.query_map(params![st,e], duck_row_to_cash_flow)?
                        .filter_map(|r| r.ok()).collect()
                }
                _ => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, period_end, operating_cash_flow, capex \
                         FROM cash_flow_statements ORDER BY symbol, period_end DESC")?;
                    stmt.query_map([], duck_row_to_cash_flow)?
                        .filter_map(|r| r.ok()).collect()
                }
            };
            Ok(results)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_dividend_report(&self, items: &[DividendEvent]) -> StorageResult<()> {
        let conn  = self.conn.clone();
        let items: Vec<DividendEvent> = items.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO dividends (symbol, ex_date, payment_date, amount) \
                 VALUES (?, ?, ?, ?) \
                 ON CONFLICT (symbol, ex_date) DO UPDATE SET \
                    payment_date=EXCLUDED.payment_date, amount=EXCLUDED.amount",
            )?;
            for item in &items {
                stmt.execute(params![
                    item.symbol.as_str(), item.ex_date.to_string(),
                    item.payment_date.to_string(), d2f(item.amount),
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
        let conn  = self.conn.clone();
        let sym   = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end   = time_range.map(|r| r.end.to_string());
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<DividendEvent>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let results = match (&sym, &start, &end) {
                (Some(s), Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, ex_date, payment_date, amount FROM dividends \
                         WHERE symbol=? AND ex_date>=? AND ex_date<? ORDER BY ex_date DESC")?;
                    stmt.query_map(params![s,st,e], duck_row_to_dividend)?
                        .filter_map(|r| r.ok()).collect()
                }
                (Some(s), None, _) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, ex_date, payment_date, amount FROM dividends \
                         WHERE symbol=? ORDER BY ex_date DESC")?;
                    stmt.query_map(params![s], duck_row_to_dividend)?
                        .filter_map(|r| r.ok()).collect()
                }
                (None, Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, ex_date, payment_date, amount FROM dividends \
                         WHERE ex_date>=? AND ex_date<? ORDER BY ex_date DESC")?;
                    stmt.query_map(params![st,e], duck_row_to_dividend)?
                        .filter_map(|r| r.ok()).collect()
                }
                _ => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, ex_date, payment_date, amount FROM dividends \
                         ORDER BY symbol, ex_date DESC")?;
                    stmt.query_map([], duck_row_to_dividend)?
                        .filter_map(|r| r.ok()).collect()
                }
            };
            Ok(results)
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn insert_stock_split(&self, items: &[StockSplitEvent]) -> StorageResult<()> {
        let conn  = self.conn.clone();
        let items: Vec<StockSplitEvent> = items.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO stock_splits (symbol, date, ratio) VALUES (?, ?, ?) \
                 ON CONFLICT (symbol, date) DO UPDATE SET ratio=EXCLUDED.ratio",
            )?;
            for item in &items {
                stmt.execute(params![
                    item.symbol.as_str(), item.date.to_string(), d2f(item.ratio),
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
        let conn  = self.conn.clone();
        let sym   = symbol.map(|s| s.as_str().to_string());
        let start = time_range.as_ref().map(|r| r.start.to_string());
        let end   = time_range.map(|r| r.end.to_string());
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<StockSplitEvent>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let results = match (&sym, &start, &end) {
                (Some(s), Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, date, ratio FROM stock_splits \
                         WHERE symbol=? AND date>=? AND date<? ORDER BY date DESC")?;
                    stmt.query_map(params![s,st,e], duck_row_to_split)?
                        .filter_map(|r| r.ok()).collect()
                }
                (Some(s), None, _) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, date, ratio FROM stock_splits \
                         WHERE symbol=? ORDER BY date DESC")?;
                    stmt.query_map(params![s], duck_row_to_split)?
                        .filter_map(|r| r.ok()).collect()
                }
                (None, Some(st), Some(e)) => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, date, ratio FROM stock_splits \
                         WHERE date>=? AND date<? ORDER BY date DESC")?;
                    stmt.query_map(params![st,e], duck_row_to_split)?
                        .filter_map(|r| r.ok()).collect()
                }
                _ => {
                    let mut stmt = conn.prepare(
                        "SELECT symbol, date, ratio FROM stock_splits \
                         ORDER BY symbol, date DESC")?;
                    stmt.query_map([], duck_row_to_split)?
                        .filter_map(|r| r.ok()).collect()
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
        let sym  = symbol.as_str().to_string();
        let ivl  = interval_str(interval).to_string();
        let bars: Vec<CandleEntry> = bars.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "INSERT INTO bars (symbol, interval, time, open, high, low, close, volume) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (symbol, interval, time) DO UPDATE SET \
                    open=EXCLUDED.open, high=EXCLUDED.high, \
                    low=EXCLUDED.low, close=EXCLUDED.close, volume=EXCLUDED.volume",
            )?;
            for bar in &bars {
                stmt.execute(params![
                    sym, ivl, bar.timestamp.to_string(),
                    d2f(bar.open), d2f(bar.high), d2f(bar.low), d2f(bar.close),
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
        let conn  = self.conn.clone();
        let sym   = symbol.as_str().to_string();
        let ivl   = interval_str(interval).to_string();
        let start = time_range.start.to_string();
        let end   = time_range.end.to_string();
        tokio::task::spawn_blocking(move || -> StorageResult<Vec<CandleEntry>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT time, open, high, low, close, volume FROM bars \
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
        let sym  = symbol.as_str().to_string();
        let bars: Vec<DailyCandleEntry> = bars.to_vec();
        tokio::task::spawn_blocking(move || -> StorageResult<()> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
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
                    sym, ts,
                    d2f(bar.open), d2f(bar.high), d2f(bar.low), d2f(bar.close),
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
        let conn  = self.conn.clone();
        let sym   = symbol.as_str().to_string();
        let start = date_range.start.and_hms_opt(0, 0, 0).unwrap().to_string();
        let end   = date_range.end.and_hms_opt(0, 0, 0).unwrap().to_string();
        let entries = tokio::task::spawn_blocking(move || -> StorageResult<Vec<DailyCandleEntry>> {
            let conn = conn.lock().map_err(|e| StorageError::Provider(e.to_string()))?;
            let mut stmt = conn.prepare(
                "SELECT time, open, high, low, close, volume FROM bars \
                 WHERE symbol=? AND interval='1d' AND time>=? AND time<? ORDER BY time ASC",
            )?;
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
        &self, _symbol: &Symbol, _time_range: Range<NaiveDateTime>,
    ) -> StorageResult<Vec<TradeTick>> {
        Err(StorageError::UnsupportedInterval)
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
        .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1970,1,1).unwrap().and_hms_opt(0,0,0).unwrap())
}

// ── Row mappers ───────────────────────────────────────────────────────────────

fn duck_row_to_ticker(row: &duckdb::Row) -> duckdb::Result<Ticker> {
    Ok(Ticker {
        symbol:      Symbol::new(&row.get::<_, String>(0)?),
        exchange:    row.get::<_, Option<String>>(1)?.map(|s| Exchange::new(&s)),
        name:        row.get::<_, Option<String>>(2)?,
        country:     row.get::<_, Option<String>>(3)?,
        industry:    None,
        sector:      None,
        ipoyear:     row.get::<_, Option<String>>(6)?,
        marketcap:   row.get::<_, Option<f64>>(7)?.map(f2d),
        description: row.get::<_, Option<String>>(8)?,
        active:      row.get::<_, bool>(9).unwrap_or(true),
    })
}

fn duck_row_to_news(row: &duckdb::Row) -> duckdb::Result<NewsStory> {
    let symbol: Option<String> = row.get(0)?;
    let about = match symbol {
        Some(s) => NewsAbout::Symbol(Symbol::new(&s)),
        None    => NewsAbout::Sector("Market".to_string()),
    };
    let pub_str: Option<String> = row.get(6)?;
    let fet_str: Option<String> = row.get(7)?;
    let epoch = || NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
    Ok(NewsStory {
        about,
        headline:     row.get::<_, String>(1)?,
        summary:      row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        story:        row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        url:          row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        evaluation:   row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        published_at: pub_str.as_deref().map(parse_date).unwrap_or_else(epoch),
        fetched_at:   fet_str.as_deref().map(parse_date).unwrap_or_else(epoch),
    })
}

fn duck_row_to_income(row: &duckdb::Row) -> duckdb::Result<IncomeStatement> {
    let period_str: String = row.get(1)?;
    Ok(IncomeStatement {
        symbol:           Symbol::new(&row.get::<_, String>(0)?),
        period_end:       parse_date(&period_str),
        revenue:          f2d(row.get::<_, f64>(2).unwrap_or(0.0)),
        cogs:             f2d(row.get::<_, f64>(3).unwrap_or(0.0)),
        operating_income: f2d(row.get::<_, f64>(4).unwrap_or(0.0)),
        ebit:             f2d(row.get::<_, f64>(5).unwrap_or(0.0)),
        net_income:       f2d(row.get::<_, f64>(6).unwrap_or(0.0)),
        eps:              f2d(row.get::<_, f64>(7).unwrap_or(0.0)),
        interest_expense: f2d(row.get::<_, f64>(8).unwrap_or(0.0)),
        tax_expense:      f2d(row.get::<_, f64>(9).unwrap_or(0.0)),
    })
}

fn duck_row_to_balance(row: &duckdb::Row) -> duckdb::Result<BalanceSheet> {
    let period_str: String = row.get(1)?;
    Ok(BalanceSheet {
        symbol:       Symbol::new(&row.get::<_, String>(0)?),
        period_end:   parse_date(&period_str),
        total_assets: f2d(row.get::<_, f64>(2).unwrap_or(0.0)),
        total_debt:   f2d(row.get::<_, f64>(3).unwrap_or(0.0)),
        total_equity: f2d(row.get::<_, f64>(4).unwrap_or(0.0)),
        cash:         f2d(row.get::<_, f64>(5).unwrap_or(0.0)),
    })
}

fn duck_row_to_cash_flow(row: &duckdb::Row) -> duckdb::Result<CashFlowStatement> {
    let period_str: String = row.get(1)?;
    Ok(CashFlowStatement {
        symbol:              Symbol::new(&row.get::<_, String>(0)?),
        period_end:          parse_date(&period_str),
        operating_cash_flow: f2d(row.get::<_, f64>(2).unwrap_or(0.0)),
        capex:               f2d(row.get::<_, f64>(3).unwrap_or(0.0)),
    })
}

fn duck_row_to_dividend(row: &duckdb::Row) -> duckdb::Result<DividendEvent> {
    let ex_str:  String = row.get(1)?;
    let pay_str: String = row.get(2)?;
    Ok(DividendEvent {
        symbol:       Symbol::new(&row.get::<_, String>(0)?),
        ex_date:      parse_date(&ex_str),
        payment_date: parse_date(&pay_str),
        amount:       f2d(row.get::<_, f64>(3).unwrap_or(0.0)),
    })
}

fn duck_row_to_split(row: &duckdb::Row) -> duckdb::Result<StockSplitEvent> {
    let date_str: String = row.get(1)?;
    Ok(StockSplitEvent {
        symbol: Symbol::new(&row.get::<_, String>(0)?),
        date:   parse_date(&date_str),
        ratio:  f2d(row.get::<_, f64>(2).unwrap_or(1.0)),
    })
}

fn duck_row_to_candle(row: &duckdb::Row) -> duckdb::Result<CandleEntry> {
    let ts_str: String = row.get(0)?;
    Ok(CandleEntry {
        timestamp: parse_datetime(&ts_str),
        open:      f2d(row.get::<_, f64>(1)?),
        high:      f2d(row.get::<_, f64>(2)?),
        low:       f2d(row.get::<_, f64>(3)?),
        close:     f2d(row.get::<_, f64>(4)?),
        volume:    row.get::<_, i64>(5)? as u64,
    })
}

fn duck_row_to_daily_candle(row: &duckdb::Row) -> duckdb::Result<DailyCandleEntry> {
    let ts_str: String = row.get(0)?;
    Ok(DailyCandleEntry {
        date:   parse_datetime(&ts_str).date(),
        open:   f2d(row.get::<_, f64>(1)?),
        high:   f2d(row.get::<_, f64>(2)?),
        low:    f2d(row.get::<_, f64>(3)?),
        close:  f2d(row.get::<_, f64>(4)?),
        volume: row.get::<_, i64>(5)? as u64,
    })
}
