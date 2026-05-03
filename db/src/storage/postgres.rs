//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! PostgreSQL storage implementation backed by sqlx + TimescaleDB.
//!
//! Implements MetadataStorage (instruments, news, fundamentals) and
//! CandleStorage (market.bars) against the schema created by migration 001/002.

use crate::{StorageError, StorageResult};
use crate::storage::traits::{
    CandleStorage, MetadataStorage, TickStorage, TickerQuery,
};
use economind_core::model::{
    BalanceSheet, CandleEntry, CashFlowStatement, DailyCandleEntry, DividendEvent,
    IncomeStatement, Interval, NewsStory, StockSplitEvent, Symbol, Ticker, TradeTick,
};
use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};
use futures_core::stream::BoxStream;
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::ops::Range;

// ── Connection ───────────────────────────────────────────────────────────────

/// PostgreSQL-backed storage using sqlx with async connection pooling.
///
/// Requires a running PostgreSQL + TimescaleDB instance with migrations applied.
/// Construct via [`PostgresStorage::connect`] passing a `DATABASE_URL`.
#[derive(Clone)]
pub struct PostgresStorage {
    pool: PgPool,
}

impl PostgresStorage {
    /// Connect to PostgreSQL using the provided connection URL.
    ///
    /// # Example
    /// ```ignore
    /// let db = PostgresStorage::connect("postgres://user:pass@localhost/economind").await?;
    /// ```
    pub async fn connect(database_url: &str) -> StorageResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    /// Return a reference to the underlying pool (useful for raw queries).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

// ── Interval helper ──────────────────────────────────────────────────────────

fn interval_to_str(interval: Interval) -> &'static str {
    match interval {
        Interval::OneMinute     => "1m",
        Interval::FiveMinute    => "5m",
        Interval::FifteenMinute => "15m",
        Interval::OneHour       => "1h",
        Interval::OneDay        => "1d",
    }
}

// ── MetadataStorage ──────────────────────────────────────────────────────────

impl MetadataStorage for PostgresStorage {
    async fn list_tickers(&self) -> StorageResult<BoxStream<'static, Symbol>> {
        let rows = sqlx::query(
            "SELECT symbol FROM market.instruments WHERE active = TRUE ORDER BY symbol",
        )
        .fetch_all(&self.pool)
        .await?;

        let symbols: Vec<Symbol> = rows
            .into_iter()
            .map(|r| Symbol::new(r.get::<&str, _>("symbol")))
            .collect();

        Ok(Box::pin(futures::stream::iter(symbols)))
    }

    async fn query_tickers<'a>(
        &'a self,
        _query: TickerQuery,
    ) -> StorageResult<BoxStream<'a, Ticker>> {
        // Full TickerQuery filtering is Phase 3 work (data coverage).
        // For now return all active instruments.
        let rows = sqlx::query(
            "SELECT symbol, exchange, name, country, industry, sector, \
                    ipo_year, market_cap, description, active \
             FROM market.instruments \
             WHERE active = TRUE \
             ORDER BY symbol",
        )
        .fetch_all(&self.pool)
        .await?;

        let tickers: Vec<Ticker> = rows
            .into_iter()
            .map(row_to_ticker)
            .collect();

        Ok(Box::pin(futures::stream::iter(tickers)))
    }

    async fn get_ticker(&self, symbol: &Symbol) -> StorageResult<Option<Ticker>> {
        let row = sqlx::query(
            "SELECT symbol, exchange, name, country, industry, sector, \
                    ipo_year, market_cap, description, active \
             FROM market.instruments \
             WHERE symbol = $1",
        )
        .bind(symbol.as_str())
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(row_to_ticker))
    }

    async fn upsert_ticker(&self, symbol: &Symbol) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO market.instruments (symbol) \
             VALUES ($1) \
             ON CONFLICT (symbol) DO UPDATE SET updated_at = NOW()",
        )
        .bind(symbol.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_news(&self, items: &[NewsStory]) -> StorageResult<()> {
        for item in items {
            let symbol_str: Option<&str> = match &item.about {
                economind_core::model::NewsAbout::Symbol(s) => Some(s.as_str()),
                _ => None,
            };
            // Cast NaiveDate → TIMESTAMPTZ at midnight UTC in the query
            let published_ts = item.published_at
                .and_hms_opt(0, 0, 0)
                .map(|ndt| Utc.from_utc_datetime(&ndt));
            sqlx::query(
                "INSERT INTO market.news \
                    (symbol, headline, summary, story, url, evaluation, published_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7) \
                 ON CONFLICT (url) DO NOTHING",
            )
            .bind(symbol_str)
            .bind(&item.headline)
            .bind(&item.summary)
            .bind(&item.story)
            .bind(&item.url)
            .bind(&item.evaluation)
            .bind(published_ts)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn query_news<'a>(
        &'a self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<BoxStream<'a, NewsStory>> {
        // published_at is TIMESTAMPTZ; cast to DATE for range comparisons.
        let rows = match (symbol, time_range) {
            (Some(sym), Some(range)) => {
                sqlx::query(
                    "SELECT symbol, headline, summary, story, url, evaluation, \
                             published_at, fetched_at \
                     FROM market.news \
                     WHERE symbol = $1 \
                       AND published_at::date >= $2 AND published_at::date < $3 \
                     ORDER BY published_at DESC",
                )
                .bind(sym.as_str())
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(sym), None) => {
                sqlx::query(
                    "SELECT symbol, headline, summary, story, url, evaluation, \
                             published_at, fetched_at \
                     FROM market.news \
                     WHERE symbol = $1 \
                     ORDER BY published_at DESC",
                )
                .bind(sym.as_str())
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(range)) => {
                sqlx::query(
                    "SELECT symbol, headline, summary, story, url, evaluation, \
                             published_at, fetched_at \
                     FROM market.news \
                     WHERE published_at::date >= $1 AND published_at::date < $2 \
                     ORDER BY published_at DESC",
                )
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT symbol, headline, summary, story, url, evaluation, \
                             published_at, fetched_at \
                     FROM market.news \
                     ORDER BY published_at DESC",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };

        let stories: Vec<NewsStory> = rows.into_iter().map(row_to_news).collect();
        Ok(Box::pin(futures::stream::iter(stories)))
    }

    async fn insert_income_statements(&self, items: &[IncomeStatement]) -> StorageResult<()> {
        for item in items {
            sqlx::query(
                "INSERT INTO market.income_statements \
                    (symbol, period_end, period_type, revenue, cogs, operating_income, \
                     ebit, net_income, eps, interest_expense, tax_expense) \
                 VALUES ($1, $2, 'annual', $3, $4, $5, $6, $7, $8, $9, $10) \
                 ON CONFLICT (symbol, period_end, period_type) DO UPDATE SET \
                    revenue = EXCLUDED.revenue, \
                    cogs = EXCLUDED.cogs, \
                    operating_income = EXCLUDED.operating_income, \
                    ebit = EXCLUDED.ebit, \
                    net_income = EXCLUDED.net_income, \
                    eps = EXCLUDED.eps, \
                    interest_expense = EXCLUDED.interest_expense, \
                    tax_expense = EXCLUDED.tax_expense, \
                    fetched_at = NOW()",
            )
            .bind(item.symbol.as_str())
            .bind(item.period_end)
            .bind(item.revenue)
            .bind(item.cogs)
            .bind(item.operating_income)
            .bind(item.ebit)
            .bind(item.net_income)
            .bind(item.eps)
            .bind(item.interest_expense)
            .bind(item.tax_expense)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn query_income_statements(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<IncomeStatement>> {
        let rows = match (symbol, time_range) {
            (Some(sym), Some(range)) => {
                sqlx::query(
                    "SELECT symbol, period_end, revenue, cogs, operating_income, \
                             ebit, net_income, eps, interest_expense, tax_expense \
                     FROM market.income_statements \
                     WHERE symbol = $1 AND period_end >= $2 AND period_end < $3 \
                     ORDER BY period_end DESC",
                )
                .bind(sym.as_str())
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(sym), None) => {
                sqlx::query(
                    "SELECT symbol, period_end, revenue, cogs, operating_income, \
                             ebit, net_income, eps, interest_expense, tax_expense \
                     FROM market.income_statements \
                     WHERE symbol = $1 \
                     ORDER BY period_end DESC",
                )
                .bind(sym.as_str())
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(range)) => {
                sqlx::query(
                    "SELECT symbol, period_end, revenue, cogs, operating_income, \
                             ebit, net_income, eps, interest_expense, tax_expense \
                     FROM market.income_statements \
                     WHERE period_end >= $1 AND period_end < $2 \
                     ORDER BY period_end DESC",
                )
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT symbol, period_end, revenue, cogs, operating_income, \
                             ebit, net_income, eps, interest_expense, tax_expense \
                     FROM market.income_statements \
                     ORDER BY symbol, period_end DESC",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(row_to_income_statement).collect())
    }

    async fn insert_balance_sheets(&self, items: &[BalanceSheet]) -> StorageResult<()> {
        for item in items {
            sqlx::query(
                "INSERT INTO market.balance_sheets \
                    (symbol, period_end, period_type, total_assets, total_debt, total_equity, cash) \
                 VALUES ($1, $2, 'annual', $3, $4, $5, $6) \
                 ON CONFLICT (symbol, period_end, period_type) DO UPDATE SET \
                    total_assets = EXCLUDED.total_assets, \
                    total_debt = EXCLUDED.total_debt, \
                    total_equity = EXCLUDED.total_equity, \
                    cash = EXCLUDED.cash, \
                    fetched_at = NOW()",
            )
            .bind(item.symbol.as_str())
            .bind(item.period_end)
            .bind(item.total_assets)
            .bind(item.total_debt)
            .bind(item.total_equity)
            .bind(item.cash)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn query_balance_sheets(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<BalanceSheet>> {
        let rows = match (symbol, time_range) {
            (Some(sym), Some(range)) => {
                sqlx::query(
                    "SELECT symbol, period_end, total_assets, total_debt, total_equity, cash \
                     FROM market.balance_sheets \
                     WHERE symbol = $1 AND period_end >= $2 AND period_end < $3 \
                     ORDER BY period_end DESC",
                )
                .bind(sym.as_str())
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(sym), None) => {
                sqlx::query(
                    "SELECT symbol, period_end, total_assets, total_debt, total_equity, cash \
                     FROM market.balance_sheets \
                     WHERE symbol = $1 \
                     ORDER BY period_end DESC",
                )
                .bind(sym.as_str())
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(range)) => {
                sqlx::query(
                    "SELECT symbol, period_end, total_assets, total_debt, total_equity, cash \
                     FROM market.balance_sheets \
                     WHERE period_end >= $1 AND period_end < $2 \
                     ORDER BY period_end DESC",
                )
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT symbol, period_end, total_assets, total_debt, total_equity, cash \
                     FROM market.balance_sheets \
                     ORDER BY symbol, period_end DESC",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(row_to_balance_sheet).collect())
    }

    async fn insert_cash_flow_statements(&self, items: &[CashFlowStatement]) -> StorageResult<()> {
        for item in items {
            sqlx::query(
                "INSERT INTO market.cash_flow_statements \
                    (symbol, period_end, period_type, operating_cash_flow, capex, free_cash_flow) \
                 VALUES ($1, $2, 'annual', $3, $4, $5) \
                 ON CONFLICT (symbol, period_end, period_type) DO UPDATE SET \
                    operating_cash_flow = EXCLUDED.operating_cash_flow, \
                    capex = EXCLUDED.capex, \
                    free_cash_flow = EXCLUDED.free_cash_flow, \
                    fetched_at = NOW()",
            )
            .bind(item.symbol.as_str())
            .bind(item.period_end)
            .bind(item.operating_cash_flow)
            .bind(item.capex)
            // free_cash_flow = operating_cash_flow - capex
            .bind(item.operating_cash_flow - item.capex)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn query_cash_flow_statements(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<CashFlowStatement>> {
        let rows = match (symbol, time_range) {
            (Some(sym), Some(range)) => {
                sqlx::query(
                    "SELECT symbol, period_end, operating_cash_flow, capex \
                     FROM market.cash_flow_statements \
                     WHERE symbol = $1 AND period_end >= $2 AND period_end < $3 \
                     ORDER BY period_end DESC",
                )
                .bind(sym.as_str())
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(sym), None) => {
                sqlx::query(
                    "SELECT symbol, period_end, operating_cash_flow, capex \
                     FROM market.cash_flow_statements \
                     WHERE symbol = $1 \
                     ORDER BY period_end DESC",
                )
                .bind(sym.as_str())
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(range)) => {
                sqlx::query(
                    "SELECT symbol, period_end, operating_cash_flow, capex \
                     FROM market.cash_flow_statements \
                     WHERE period_end >= $1 AND period_end < $2 \
                     ORDER BY period_end DESC",
                )
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT symbol, period_end, operating_cash_flow, capex \
                     FROM market.cash_flow_statements \
                     ORDER BY symbol, period_end DESC",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(row_to_cash_flow).collect())
    }

    async fn insert_dividend_report(&self, items: &[DividendEvent]) -> StorageResult<()> {
        for item in items {
            sqlx::query(
                "INSERT INTO market.dividends (symbol, ex_date, payment_date, amount) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (symbol, ex_date) DO UPDATE SET \
                    payment_date = EXCLUDED.payment_date, \
                    amount = EXCLUDED.amount",
            )
            .bind(item.symbol.as_str())
            .bind(item.ex_date)
            .bind(item.payment_date)
            .bind(item.amount)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn query_dividend_report(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<DividendEvent>> {
        let rows = match (symbol, time_range) {
            (Some(sym), Some(range)) => {
                sqlx::query(
                    "SELECT symbol, ex_date, payment_date, amount \
                     FROM market.dividends \
                     WHERE symbol = $1 AND ex_date >= $2 AND ex_date < $3 \
                     ORDER BY ex_date DESC",
                )
                .bind(sym.as_str())
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(sym), None) => {
                sqlx::query(
                    "SELECT symbol, ex_date, payment_date, amount \
                     FROM market.dividends \
                     WHERE symbol = $1 \
                     ORDER BY ex_date DESC",
                )
                .bind(sym.as_str())
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(range)) => {
                sqlx::query(
                    "SELECT symbol, ex_date, payment_date, amount \
                     FROM market.dividends \
                     WHERE ex_date >= $1 AND ex_date < $2 \
                     ORDER BY ex_date DESC",
                )
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT symbol, ex_date, payment_date, amount \
                     FROM market.dividends \
                     ORDER BY symbol, ex_date DESC",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(row_to_dividend).collect())
    }

    async fn insert_stock_split(&self, items: &[StockSplitEvent]) -> StorageResult<()> {
        for item in items {
            sqlx::query(
                "INSERT INTO market.stock_splits (symbol, date, ratio) \
                 VALUES ($1, $2, $3) \
                 ON CONFLICT (symbol, date) DO UPDATE SET ratio = EXCLUDED.ratio",
            )
            .bind(item.symbol.as_str())
            .bind(item.date)
            .bind(item.ratio)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn query_stock_split(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<StockSplitEvent>> {
        let rows = match (symbol, time_range) {
            (Some(sym), Some(range)) => {
                sqlx::query(
                    "SELECT symbol, date, ratio FROM market.stock_splits \
                     WHERE symbol = $1 AND date >= $2 AND date < $3 \
                     ORDER BY date DESC",
                )
                .bind(sym.as_str())
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(sym), None) => {
                sqlx::query(
                    "SELECT symbol, date, ratio FROM market.stock_splits \
                     WHERE symbol = $1 \
                     ORDER BY date DESC",
                )
                .bind(sym.as_str())
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(range)) => {
                sqlx::query(
                    "SELECT symbol, date, ratio FROM market.stock_splits \
                     WHERE date >= $1 AND date < $2 \
                     ORDER BY date DESC",
                )
                .bind(range.start)
                .bind(range.end)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT symbol, date, ratio FROM market.stock_splits \
                     ORDER BY symbol, date DESC",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(row_to_split).collect())
    }
}

// ── CandleStorage ────────────────────────────────────────────────────────────

impl CandleStorage for PostgresStorage {
    async fn upsert_candles(
        &self,
        symbol: &Symbol,
        interval: Interval,
        bars: &[CandleEntry],
    ) -> StorageResult<()> {
        let interval_str = interval_to_str(interval);
        for bar in bars {
            let ts = Utc.from_utc_datetime(&bar.timestamp);
            sqlx::query(
                "INSERT INTO market.bars (symbol, interval, time, open, high, low, close, volume) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
                 ON CONFLICT (symbol, interval, time) DO UPDATE SET \
                    open   = EXCLUDED.open, \
                    high   = EXCLUDED.high, \
                    low    = EXCLUDED.low, \
                    close  = EXCLUDED.close, \
                    volume = EXCLUDED.volume",
            )
            .bind(symbol.as_str())
            .bind(interval_str)
            .bind(ts)
            .bind(bar.open)
            .bind(bar.high)
            .bind(bar.low)
            .bind(bar.close)
            .bind(bar.volume as i64)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn query_candles(
        &self,
        symbol: &Symbol,
        interval: Interval,
        time_range: Range<NaiveDateTime>,
    ) -> StorageResult<Vec<CandleEntry>> {
        let interval_str = interval_to_str(interval);
        let start = Utc.from_utc_datetime(&time_range.start);
        let end   = Utc.from_utc_datetime(&time_range.end);

        let rows = sqlx::query(
            "SELECT time, open, high, low, close, volume \
             FROM market.bars \
             WHERE symbol = $1 AND interval = $2 \
               AND time >= $3 AND time < $4 \
             ORDER BY time ASC",
        )
        .bind(symbol.as_str())
        .bind(interval_str)
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(row_to_candle_entry).collect())
    }

    async fn unsert_daily_candle(
        &self,
        symbol: &Symbol,
        bars: &[DailyCandleEntry],
    ) -> StorageResult<()> {
        for bar in bars {
            let ts = Utc
                .from_utc_datetime(&bar.date.and_hms_opt(0, 0, 0).unwrap());
            sqlx::query(
                "INSERT INTO market.bars (symbol, interval, time, open, high, low, close, volume) \
                 VALUES ($1, '1d', $2, $3, $4, $5, $6, $7) \
                 ON CONFLICT (symbol, interval, time) DO UPDATE SET \
                    open   = EXCLUDED.open, \
                    high   = EXCLUDED.high, \
                    low    = EXCLUDED.low, \
                    close  = EXCLUDED.close, \
                    volume = EXCLUDED.volume",
            )
            .bind(symbol.as_str())
            .bind(ts)
            .bind(bar.open)
            .bind(bar.high)
            .bind(bar.low)
            .bind(bar.close)
            .bind(bar.volume as i64)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn query_daily_candles<'a>(
        &'a self,
        symbol: &Symbol,
        date_range: Range<NaiveDate>,
    ) -> StorageResult<BoxStream<'a, DailyCandleEntry>> {
        let start = Utc.from_utc_datetime(&date_range.start.and_hms_opt(0, 0, 0).unwrap());
        let end   = Utc.from_utc_datetime(&date_range.end.and_hms_opt(0, 0, 0).unwrap());

        let rows = sqlx::query(
            "SELECT time, open, high, low, close, volume \
             FROM market.bars \
             WHERE symbol = $1 AND interval = '1d' \
               AND time >= $2 AND time < $3 \
             ORDER BY time ASC",
        )
        .bind(symbol.as_str())
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await?;

        let entries: Vec<DailyCandleEntry> = rows.into_iter().map(row_to_daily_candle_entry).collect();
        Ok(Box::pin(futures::stream::iter(entries)))
    }
}

// ── TickStorage ──────────────────────────────────────────────────────────────

impl TickStorage for PostgresStorage {
    async fn insert_ticks(&self, _ticks: &[TradeTick]) -> StorageResult<()> {
        // Tick data is not stored in PostgreSQL in the current schema design.
        // The market.bars table covers OHLCV granularity down to 1-minute bars.
        // Sub-minute tick storage (if ever needed) would require a separate hypertable.
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

// ── Row mapping helpers ──────────────────────────────────────────────────────

fn row_to_ticker(row: sqlx::postgres::PgRow) -> Ticker {
    Ticker {
        symbol:      Symbol::new(row.get::<&str, _>("symbol")),
        exchange:    row.get::<Option<&str>, _>("exchange")
                        .map(economind_core::model::Exchange::new),
        name:        row.get::<Option<String>, _>("name"),
        country:     row.get::<Option<String>, _>("country"),
        // Industry/Sector enums have no variants yet; always None until Phase 3.
        industry:    None,
        sector:      None,
        ipoyear:     row.get::<Option<i16>, _>("ipo_year").map(|y| y.to_string()),
        marketcap:   row.get::<Option<Decimal>, _>("market_cap"),
        description: row.get::<Option<String>, _>("description"),
        active:      row.get::<bool, _>("active"),
    }
}

fn row_to_news(row: sqlx::postgres::PgRow) -> NewsStory {
    use economind_core::model::{NewsAbout, NewsStory};
    let symbol: Option<&str> = row.get("symbol");
    let about = match symbol {
        Some(s) => NewsAbout::Symbol(Symbol::new(s)),
        None => NewsAbout::Sector("Market".to_string()),
    };
    // Both published_at and fetched_at are stored as TIMESTAMPTZ; extract date portion.
    let published_at: NaiveDate = {
        let ts: chrono::DateTime<Utc> = row.get("published_at");
        ts.naive_utc().date()
    };
    let fetched_at: NaiveDate = {
        let ts: chrono::DateTime<Utc> = row.get("fetched_at");
        ts.naive_utc().date()
    };
    NewsStory {
        about,
        headline: row.get("headline"),
        summary: row.get::<Option<String>, _>("summary").unwrap_or_default(),
        story: row.get::<Option<String>, _>("story").unwrap_or_default(),
        url: row.get::<Option<String>, _>("url").unwrap_or_default(),
        evaluation: row.get::<Option<String>, _>("evaluation").unwrap_or_default(),
        published_at,
        fetched_at,
    }
}

fn row_to_income_statement(row: sqlx::postgres::PgRow) -> IncomeStatement {
    IncomeStatement {
        symbol: Symbol::new(row.get::<&str, _>("symbol")),
        period_end: row.get("period_end"),
        revenue: row.get::<Option<Decimal>, _>("revenue").unwrap_or_default(),
        cogs: row.get::<Option<Decimal>, _>("cogs").unwrap_or_default(),
        operating_income: row.get::<Option<Decimal>, _>("operating_income").unwrap_or_default(),
        ebit: row.get::<Option<Decimal>, _>("ebit").unwrap_or_default(),
        net_income: row.get::<Option<Decimal>, _>("net_income").unwrap_or_default(),
        eps: row.get::<Option<Decimal>, _>("eps").unwrap_or_default(),
        interest_expense: row.get::<Option<Decimal>, _>("interest_expense").unwrap_or_default(),
        tax_expense: row.get::<Option<Decimal>, _>("tax_expense").unwrap_or_default(),
    }
}

fn row_to_balance_sheet(row: sqlx::postgres::PgRow) -> BalanceSheet {
    BalanceSheet {
        symbol: Symbol::new(row.get::<&str, _>("symbol")),
        period_end: row.get("period_end"),
        total_assets: row.get::<Option<Decimal>, _>("total_assets").unwrap_or_default(),
        total_debt: row.get::<Option<Decimal>, _>("total_debt").unwrap_or_default(),
        total_equity: row.get::<Option<Decimal>, _>("total_equity").unwrap_or_default(),
        cash: row.get::<Option<Decimal>, _>("cash").unwrap_or_default(),
    }
}

fn row_to_cash_flow(row: sqlx::postgres::PgRow) -> CashFlowStatement {
    CashFlowStatement {
        symbol: Symbol::new(row.get::<&str, _>("symbol")),
        period_end: row.get("period_end"),
        operating_cash_flow: row
            .get::<Option<Decimal>, _>("operating_cash_flow")
            .unwrap_or_default(),
        capex: row.get::<Option<Decimal>, _>("capex").unwrap_or_default(),
    }
}

fn row_to_dividend(row: sqlx::postgres::PgRow) -> DividendEvent {
    DividendEvent {
        symbol: Symbol::new(row.get::<&str, _>("symbol")),
        ex_date: row.get("ex_date"),
        payment_date: row.get::<Option<NaiveDate>, _>("payment_date")
            .unwrap_or_else(|| row.get("ex_date")),
        amount: row.get("amount"),
    }
}

fn row_to_split(row: sqlx::postgres::PgRow) -> StockSplitEvent {
    StockSplitEvent {
        symbol: Symbol::new(row.get::<&str, _>("symbol")),
        date: row.get("date"),
        ratio: row.get("ratio"),
    }
}

fn row_to_candle_entry(row: sqlx::postgres::PgRow) -> CandleEntry {
    let ts: chrono::DateTime<Utc> = row.get("time");
    CandleEntry {
        timestamp: ts.naive_utc(),
        open:   row.get("open"),
        high:   row.get("high"),
        low:    row.get("low"),
        close:  row.get("close"),
        volume: row.get::<i64, _>("volume") as u64,
    }
}

fn row_to_daily_candle_entry(row: sqlx::postgres::PgRow) -> DailyCandleEntry {
    let ts: chrono::DateTime<Utc> = row.get("time");
    DailyCandleEntry {
        date:   ts.naive_utc().date(),
        open:   row.get("open"),
        high:   row.get("high"),
        low:    row.get("low"),
        close:  row.get("close"),
        volume: row.get::<i64, _>("volume") as u64,
    }
}
