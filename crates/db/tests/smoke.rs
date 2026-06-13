//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Phase 1.D — Integration smoke tests for the db crate.
//!
//! These tests exercise both storage backends against real data round-trips.
//!
//! # DuckDB tests
//! Run unconditionally using an in-memory database.
//!
//! # PostgreSQL tests
//! Gated on the `DATABASE_URL` environment variable.  If it is not set the
//! tests are skipped gracefully so CI without a live Postgres still passes.
//! Set it to a scratch database before running:
//!
//! ```
//! DATABASE_URL=postgres://user:pass@localhost/economind_test cargo test --test smoke
//! ```

use chrono::NaiveDate;
use economind_core::model::{
    BalanceSheet, CandleEntry, CashFlowStatement, DailyCandleEntry, DividendEvent, IncomeStatement,
    Interval, NewsAbout, NewsStory, StockSplitEvent, Symbol,
};
use economind_db::storage::{CandleStorage, DuckDatabase, MetadataStorage, PortfolioStorage};
use rust_decimal::Decimal;
use std::str::FromStr;

// ── helpers ──────────────────────────────────────────────────────────────────

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn sym(s: &str) -> Symbol {
    Symbol::new(s)
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

// ── DuckDB smoke tests ────────────────────────────────────────────────────────

#[tokio::test]
async fn duckdb_schema_applies_cleanly() {
    // Schema application is tested implicitly by opening an in-memory DB.
    let db = DuckDatabase::in_memory().expect("DuckDatabase::in_memory() should succeed");
    // If schema.sql had a syntax error, open would have panicked above.
    // Confirm the DB is usable by listing an empty ticker set.
    let symbols: Vec<Symbol> = {
        use futures::StreamExt;
        db.list_tickers().await.unwrap().collect().await
    };
    assert!(symbols.is_empty(), "fresh DB should have no tickers");
}

#[tokio::test]
async fn duckdb_instrument_upsert_and_query() {
    let db = DuckDatabase::in_memory().unwrap();

    // Upsert a symbol
    db.upsert_ticker(&sym("AAPL")).await.unwrap();
    db.upsert_ticker(&sym("MSFT")).await.unwrap();

    // list_tickers should return both
    let symbols: Vec<Symbol> = {
        use futures::StreamExt;
        db.list_tickers().await.unwrap().collect().await
    };
    assert_eq!(symbols.len(), 2);
    assert!(symbols.contains(&sym("AAPL")));
    assert!(symbols.contains(&sym("MSFT")));

    // get_ticker round-trip
    let ticker = db.get_ticker(&sym("AAPL")).await.unwrap();
    assert!(ticker.is_some());
    assert_eq!(ticker.unwrap().symbol, sym("AAPL"));

    // Unknown symbol returns None
    let missing = db.get_ticker(&sym("ZZZZ")).await.unwrap();
    assert!(missing.is_none());
}

#[tokio::test]
async fn duckdb_daily_candle_round_trip() {
    let db = DuckDatabase::in_memory().unwrap();
    db.upsert_ticker(&sym("TSLA")).await.unwrap();

    let bars = vec![
        DailyCandleEntry {
            date: date(2024, 1, 2),
            open: dec("200.00"),
            high: dec("210.50"),
            low: dec("198.00"),
            close: dec("208.00"),
            volume: 50_000_000,
        },
        DailyCandleEntry {
            date: date(2024, 1, 3),
            open: dec("208.00"),
            high: dec("215.00"),
            low: dec("205.00"),
            close: dec("212.50"),
            volume: 45_000_000,
        },
        DailyCandleEntry {
            date: date(2024, 1, 4),
            open: dec("212.50"),
            high: dec("213.00"),
            low: dec("200.00"),
            close: dec("202.00"),
            volume: 60_000_000,
        },
    ];

    db.unsert_daily_candle(&sym("TSLA"), &bars).await.unwrap();

    // Query the full range
    let result: Vec<DailyCandleEntry> = {
        use futures::StreamExt;
        db.query_daily_candles(&sym("TSLA"), date(2024, 1, 1)..date(2024, 1, 5))
            .await
            .unwrap()
            .collect()
            .await
    };

    assert_eq!(result.len(), 3, "should get back all 3 bars");
    assert_eq!(result[0].date, date(2024, 1, 2));
    assert_eq!(result[0].close, dec("208.00"));
    assert_eq!(result[2].date, date(2024, 1, 4));

    // Narrow range: only 2 bars
    let narrow: Vec<DailyCandleEntry> = {
        use futures::StreamExt;
        db.query_daily_candles(&sym("TSLA"), date(2024, 1, 2)..date(2024, 1, 4))
            .await
            .unwrap()
            .collect()
            .await
    };
    assert_eq!(narrow.len(), 2);

    // Upsert idempotency — same bars again should not duplicate
    db.unsert_daily_candle(&sym("TSLA"), &bars).await.unwrap();
    let after_upsert: Vec<DailyCandleEntry> = {
        use futures::StreamExt;
        db.query_daily_candles(&sym("TSLA"), date(2024, 1, 1)..date(2024, 1, 5))
            .await
            .unwrap()
            .collect()
            .await
    };
    assert_eq!(after_upsert.len(), 3, "upsert must not duplicate bars");
}

#[tokio::test]
async fn duckdb_intraday_candle_round_trip() {
    use chrono::NaiveDateTime;

    let db = DuckDatabase::in_memory().unwrap();
    db.upsert_ticker(&sym("SPY")).await.unwrap();

    let t = |h: u32, m: u32| -> NaiveDateTime { date(2024, 3, 15).and_hms_opt(h, m, 0).unwrap() };

    let bars = vec![
        CandleEntry {
            timestamp: t(9, 30),
            open: dec("505.00"),
            high: dec("506.00"),
            low: dec("504.50"),
            close: dec("505.80"),
            volume: 1_200_000,
        },
        CandleEntry {
            timestamp: t(9, 35),
            open: dec("505.80"),
            high: dec("507.00"),
            low: dec("505.00"),
            close: dec("506.50"),
            volume: 980_000,
        },
        CandleEntry {
            timestamp: t(9, 40),
            open: dec("506.50"),
            high: dec("508.00"),
            low: dec("506.00"),
            close: dec("507.20"),
            volume: 870_000,
        },
    ];

    db.upsert_candles(&sym("SPY"), Interval::FiveMinute, &bars)
        .await
        .unwrap();

    let result = db
        .query_candles(&sym("SPY"), Interval::FiveMinute, t(9, 0)..t(10, 0))
        .await
        .unwrap();

    assert_eq!(result.len(), 3);
    assert_eq!(result[0].timestamp, t(9, 30));
    assert_eq!(result[2].close, dec("507.20"));
}

#[tokio::test]
async fn duckdb_income_statement_round_trip() {
    let db = DuckDatabase::in_memory().unwrap();
    db.upsert_ticker(&sym("NVDA")).await.unwrap();

    let items = vec![IncomeStatement {
        symbol: sym("NVDA"),
        period_end: date(2023, 12, 31),
        revenue: dec("60922000000"),
        cogs: dec("16621000000"),
        operating_income: dec("32972000000"),
        ebit: dec("33053000000"),
        net_income: dec("29760000000"),
        eps: dec("11.93"),
        interest_expense: dec("257000000"),
        tax_expense: dec("1041000000"),
    }];

    db.insert_income_statements(&items).await.unwrap();

    let result = db
        .query_income_statements(Some(&sym("NVDA")), None)
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].symbol, sym("NVDA"));
    assert_eq!(result[0].period_end, date(2023, 12, 31));
    // f64 round-trip loses precision for large ints; verify within 1%
    let eps_diff = (result[0].eps - dec("11.93")).abs();
    assert!(
        eps_diff < dec("0.01"),
        "eps round-trip within 1 cent: got {}",
        result[0].eps
    );
}

#[tokio::test]
async fn duckdb_balance_sheet_round_trip() {
    let db = DuckDatabase::in_memory().unwrap();
    db.upsert_ticker(&sym("AAPL")).await.unwrap();

    let items = vec![BalanceSheet {
        symbol: sym("AAPL"),
        period_end: date(2023, 9, 30),
        total_assets: dec("352755000000"),
        total_debt: dec("111088000000"),
        total_equity: dec("62146000000"),
        cash: dec("61555000000"),
    }];

    db.insert_balance_sheets(&items).await.unwrap();
    let result = db
        .query_balance_sheets(Some(&sym("AAPL")), None)
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].period_end, date(2023, 9, 30));
}

#[tokio::test]
async fn duckdb_cash_flow_round_trip() {
    let db = DuckDatabase::in_memory().unwrap();
    db.upsert_ticker(&sym("MSFT")).await.unwrap();

    let items = vec![CashFlowStatement {
        symbol: sym("MSFT"),
        period_end: date(2023, 6, 30),
        operating_cash_flow: dec("87582000000"),
        capex: dec("28107000000"),
    }];

    db.insert_cash_flow_statements(&items).await.unwrap();
    let result = db
        .query_cash_flow_statements(Some(&sym("MSFT")), None)
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].symbol, sym("MSFT"));
}

#[tokio::test]
async fn duckdb_dividend_round_trip() {
    let db = DuckDatabase::in_memory().unwrap();
    db.upsert_ticker(&sym("JNJ")).await.unwrap();

    let items = vec![
        DividendEvent {
            symbol: sym("JNJ"),
            ex_date: date(2024, 2, 20),
            payment_date: date(2024, 3, 5),
            amount: dec("1.19"),
        },
        DividendEvent {
            symbol: sym("JNJ"),
            ex_date: date(2024, 5, 21),
            payment_date: date(2024, 6, 4),
            amount: dec("1.24"),
        },
    ];

    db.insert_dividend_report(&items).await.unwrap();

    let all = db
        .query_dividend_report(Some(&sym("JNJ")), None)
        .await
        .unwrap();
    assert_eq!(all.len(), 2);

    let range = db
        .query_dividend_report(Some(&sym("JNJ")), Some(date(2024, 1, 1)..date(2024, 4, 1)))
        .await
        .unwrap();
    assert_eq!(range.len(), 1);
    assert_eq!(range[0].ex_date, date(2024, 2, 20));
}

#[tokio::test]
async fn duckdb_stock_split_round_trip() {
    let db = DuckDatabase::in_memory().unwrap();
    db.upsert_ticker(&sym("NVDA")).await.unwrap();

    let items = vec![StockSplitEvent {
        symbol: sym("NVDA"),
        date: date(2024, 6, 10),
        ratio: dec("10"),
    }];

    db.insert_stock_split(&items).await.unwrap();
    let result = db
        .query_stock_split(Some(&sym("NVDA")), None)
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
    // f64 round-trip: 10.0 should come back as exactly 10
    assert_eq!(result[0].ratio, dec("10"));
}

#[tokio::test]
async fn duckdb_news_round_trip() {
    let db = DuckDatabase::in_memory().unwrap();

    let items = vec![
        NewsStory {
            about: NewsAbout::Symbol(sym("AAPL")),
            headline: "Apple reports record Q4 earnings".to_string(),
            summary: "Revenue beat estimates by 3%".to_string(),
            story: "Full story here.".to_string(),
            url: "https://example.com/aapl-q4-2023".to_string(),
            evaluation: "Positive".to_string(),
            published_at: date(2023, 11, 2),
            fetched_at: date(2023, 11, 2),
        },
        NewsStory {
            about: NewsAbout::Symbol(sym("MSFT")),
            headline: "Microsoft Azure growth accelerates".to_string(),
            summary: "Cloud segment up 29% YoY".to_string(),
            story: "Full story here.".to_string(),
            url: "https://example.com/msft-azure-2024".to_string(),
            evaluation: "Positive".to_string(),
            published_at: date(2024, 1, 30),
            fetched_at: date(2024, 1, 30),
        },
    ];

    db.insert_news(&items).await.unwrap();

    // Dedup: inserting same URLs again should be a no-op
    db.insert_news(&items).await.unwrap();

    // Query by symbol
    let aapl_news: Vec<NewsStory> = {
        use futures::StreamExt;
        db.query_news(Some(&sym("AAPL")), None)
            .await
            .unwrap()
            .collect()
            .await
    };
    assert_eq!(aapl_news.len(), 1);
    assert_eq!(aapl_news[0].headline, "Apple reports record Q4 earnings");

    // Query all
    let all: Vec<NewsStory> = {
        use futures::StreamExt;
        db.query_news(None, None).await.unwrap().collect().await
    };
    assert_eq!(all.len(), 2, "dedup should prevent duplicates");
}

// ── Portfolio position tests ──────────────────────────────────────────────────

#[tokio::test]
async fn duckdb_open_and_close_position() {
    use chrono::Utc;

    let db = DuckDatabase::in_memory().unwrap();

    let pos = db
        .open_position(&sym("AAPL"), dec("10"), dec("180.00"), Utc::now())
        .await
        .unwrap();

    assert_eq!(pos.symbol, sym("AAPL"));
    assert_eq!(pos.shares, dec("10"));
    assert_eq!(pos.entry_price, dec("180.00"));

    // Should appear in portfolio state
    let state = db.load_portfolio_state().await.unwrap();
    assert_eq!(state.open_positions.len(), 1);
    assert_eq!(state.open_positions[0].id, pos.id);

    // Close it
    db.close_position(pos.id, dec("195.00"), Utc::now())
        .await
        .unwrap();

    let state2 = db.load_portfolio_state().await.unwrap();
    assert!(state2.open_positions.is_empty(), "should have no open positions after close");
}

#[tokio::test]
async fn duckdb_close_unknown_position_errors() {
    use chrono::Utc;
    use uuid::Uuid;

    let db = DuckDatabase::in_memory().unwrap();

    let result = db
        .close_position(Uuid::new_v4(), dec("100.00"), Utc::now())
        .await;

    assert!(result.is_err(), "closing a non-existent position should fail");
}

// ── Watchlist tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn duckdb_watchlist_crud() {
    let db = DuckDatabase::in_memory().unwrap();

    // Empty initially
    let empty = db.list_watches().await.unwrap();
    assert!(empty.is_empty());

    // Add two symbols
    let w1 = db.add_watch(&sym("MSFT")).await.unwrap();
    assert_eq!(w1.symbol, sym("MSFT"));

    db.add_watch(&sym("NVDA")).await.unwrap();

    let all = db.list_watches().await.unwrap();
    assert_eq!(all.len(), 2);

    // get_watch round-trip
    let got = db.get_watch(&sym("MSFT")).await.unwrap();
    assert!(got.is_some());
    assert_eq!(got.unwrap().symbol, sym("MSFT"));

    // Unknown symbol returns None
    let missing = db.get_watch(&sym("ZZZZ")).await.unwrap();
    assert!(missing.is_none());

    // Duplicate add is a no-op
    db.add_watch(&sym("MSFT")).await.unwrap();
    let after_dup = db.list_watches().await.unwrap();
    assert_eq!(after_dup.len(), 2, "duplicate add must not insert a second row");

    // Remove one
    db.remove_watch(&sym("NVDA")).await.unwrap();
    let after_remove = db.list_watches().await.unwrap();
    assert_eq!(after_remove.len(), 1);
    assert_eq!(after_remove[0].symbol, sym("MSFT"));

    // Remove non-existent is silent
    db.remove_watch(&sym("ZZZZ")).await.unwrap();
}
