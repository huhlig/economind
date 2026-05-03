//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! `DataFeedManager` — orchestrates all scheduled ingestion jobs.
//!
//! Holds references to every active connector.  Each job is identified by a
//! string name so it can be triggered on-demand from the CLI
//! (`economind ingest bars`, etc.) or called programmatically.
//!
//! # Job names
//!
//! | Name            | Connector       | Description                      |
//! |-----------------|-----------------|----------------------------------|
//! | `bars`          | Yahoo Finance   | Daily OHLCV for all instruments  |
//! | `macro`         | FRED            | Macro time series                |
//! | `fundamentals`  | EDGAR + SimFin  | Annual IS/BS/CF statements       |

use crate::{
    EdgarConnector, FredConnector, FundamentalsProvider, SimFinConnector, YahooFinanceConnector,
};
use chrono::NaiveDate;
use economind_core::model::Symbol;
use economind_db::storage::{CandleStorage, MacroStorage, MetadataStorage};
use std::sync::Arc;

// ── Config ────────────────────────────────────────────────────────────────────

/// Runtime configuration for the `DataFeedManager`.
#[derive(Debug, Clone)]
pub struct DataFeedManagerConfig {
    /// How far back to backfill bars when no `since` date is provided.
    pub bars_backfill_days: u32,
    /// How far back to backfill macro series when no `since` date is provided.
    pub macro_backfill_days: u32,
    /// FRED series IDs to fetch. Falls back to `crate::FRED_DEFAULT_SERIES` if `None`.
    pub fred_series: Option<Vec<String>>,
}

impl Default for DataFeedManagerConfig {
    fn default() -> Self {
        Self {
            bars_backfill_days: 365,
            macro_backfill_days: 365 * 5,
            fred_series: None,
        }
    }
}

// ── DataFeedManager ───────────────────────────────────────────────────────────

/// Orchestrates all ingestion connectors.
///
/// All connectors are optional.  If a connector is absent, the corresponding
/// job logs a skip and returns an `IngestResult` with `skipped: true`.
#[derive(Clone)]
pub struct DataFeedManager {
    pub config: DataFeedManagerConfig,
    pub yahoo: Option<Arc<YahooFinanceConnector>>,
    pub fred: Option<Arc<FredConnector>>,
    pub edgar: Option<Arc<EdgarConnector>>,
    pub simfin: Option<Arc<SimFinConnector>>,
}

impl DataFeedManager {
    /// Create a manager with no connectors.  Use the builder methods to add them.
    pub fn new(config: DataFeedManagerConfig) -> Self {
        Self {
            config,
            yahoo: None,
            fred: None,
            edgar: None,
            simfin: None,
        }
    }

    pub fn with_yahoo(mut self, c: YahooFinanceConnector) -> Self {
        self.yahoo = Some(Arc::new(c));
        self
    }

    pub fn with_fred(mut self, c: FredConnector) -> Self {
        self.fred = Some(Arc::new(c));
        self
    }

    pub fn with_edgar(mut self, c: EdgarConnector) -> Self {
        self.edgar = Some(Arc::new(c));
        self
    }

    pub fn with_simfin(mut self, c: SimFinConnector) -> Self {
        self.simfin = Some(Arc::new(c));
        self
    }

    // ── bars ──────────────────────────────────────────────────────────────────

    /// Download and store daily OHLCV bars for all instruments in the DataStore.
    pub async fn run_bars<S>(&self, store: &S, since: Option<NaiveDate>) -> IngestResult
    where
        S: MetadataStorage + CandleStorage,
    {
        let yahoo = match &self.yahoo {
            Some(y) => y.clone(),
            None => {
                return IngestResult::skipped("bars", "No Yahoo Finance connector configured")
            }
        };

        let today = chrono::Utc::now().date_naive();
        let start = since.unwrap_or_else(|| {
            today - chrono::Duration::days(self.config.bars_backfill_days as i64)
        });

        let symbols: Vec<Symbol> = collect_symbols(store).await;
        if symbols.is_empty() {
            return IngestResult {
                job: "bars",
                symbols_ok: 0,
                symbols_err: 0,
                records: 0,
                skipped: false,
                message: Some("No instruments in universe — add symbols first".into()),
            };
        }

        let results = yahoo.fetch_all(&symbols, start..today).await;
        let mut ok = 0usize;
        let mut err = 0usize;
        let mut records = 0usize;

        for (sym, bars) in results {
            if bars.is_empty() {
                err += 1;
                continue;
            }
            match store.unsert_daily_candle(&sym, &bars).await {
                Ok(()) => {
                    records += bars.len();
                    ok += 1;
                }
                Err(e) => {
                    eprintln!("[manager:bars] store error for {}: {e}", sym.as_str());
                    err += 1;
                }
            }
        }

        IngestResult { job: "bars", symbols_ok: ok, symbols_err: err, records, skipped: false, message: None }
    }

    // ── macro ─────────────────────────────────────────────────────────────────

    /// Fetch macro time series from FRED and store them.
    pub async fn run_macro<S>(&self, store: &S, since: Option<NaiveDate>) -> IngestResult
    where
        S: MacroStorage,
    {
        let fred = match &self.fred {
            Some(f) => f.clone(),
            None => return IngestResult::skipped("macro", "No FRED connector configured"),
        };

        let backfill_since = since.or_else(|| {
            let today = chrono::Utc::now().date_naive();
            Some(today - chrono::Duration::days(self.config.macro_backfill_days as i64))
        });

        let series_refs: Option<Vec<&str>> = self
            .config
            .fred_series
            .as_deref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());

        match fred.fetch_and_store(store, series_refs.as_deref(), backfill_since).await {
            Ok(stats) => IngestResult {
                job: "macro",
                symbols_ok: stats.series_ok,
                symbols_err: stats.series_err,
                records: stats.points_stored,
                skipped: false,
                message: None,
            },
            Err(e) => IngestResult {
                job: "macro",
                symbols_ok: 0,
                symbols_err: 1,
                records: 0,
                skipped: false,
                message: Some(e.to_string()),
            },
        }
    }

    // ── fundamentals ──────────────────────────────────────────────────────────

    /// Fetch fundamentals from EDGAR and/or SimFin for all tracked instruments.
    pub async fn run_fundamentals<S>(&self, store: &S) -> IngestResult
    where
        S: MetadataStorage,
    {
        if self.edgar.is_none() && self.simfin.is_none() {
            return IngestResult::skipped(
                "fundamentals",
                "No fundamentals connector configured (EDGAR or SimFin)",
            );
        }

        let symbols: Vec<Symbol> = collect_symbols(store).await;
        if symbols.is_empty() {
            return IngestResult {
                job: "fundamentals",
                symbols_ok: 0,
                symbols_err: 0,
                records: 0,
                skipped: false,
                message: Some("No instruments in universe".into()),
            };
        }

        let edgar = self.edgar.clone();
        let simfin = self.simfin.clone();
        let mut ok = 0usize;
        let mut err = 0usize;
        let mut records = 0usize;

        for sym in &symbols {
            let ticker = sym.as_str();
            let mut fetched = false;

            // EDGAR
            if let Some(ref e) = edgar {
                if let Ok(items) = e.income_statements(ticker).await {
                    if !items.is_empty() {
                        records += items.len();
                        let _ = store.insert_income_statements(&items).await;
                        fetched = true;
                    }
                }
                if let Ok(items) = e.balance_sheets(ticker).await {
                    if !items.is_empty() {
                        records += items.len();
                        let _ = store.insert_balance_sheets(&items).await;
                        fetched = true;
                    }
                }
                if let Ok(items) = e.cash_flows(ticker).await {
                    if !items.is_empty() {
                        records += items.len();
                        let _ = store.insert_cash_flow_statements(&items).await;
                        fetched = true;
                    }
                }
            }

            // SimFin (supplement or fallback)
            if let Some(ref s) = simfin {
                if let Ok(items) = s.income_statements(ticker).await {
                    if !items.is_empty() {
                        records += items.len();
                        let _ = store.insert_income_statements(&items).await;
                        fetched = true;
                    }
                }
                if let Ok(items) = s.balance_sheets(ticker).await {
                    if !items.is_empty() {
                        records += items.len();
                        let _ = store.insert_balance_sheets(&items).await;
                        fetched = true;
                    }
                }
                if let Ok(items) = s.cash_flows(ticker).await {
                    if !items.is_empty() {
                        records += items.len();
                        let _ = store.insert_cash_flow_statements(&items).await;
                        fetched = true;
                    }
                }
            }

            if fetched { ok += 1; } else { err += 1; }
        }

        IngestResult { job: "fundamentals", symbols_ok: ok, symbols_err: err, records, skipped: false, message: None }
    }

    // ── dispatch by name ──────────────────────────────────────────────────────

    /// Run a job by name.  Valid names: `"bars"`, `"macro"`, `"fundamentals"`.
    pub async fn run_job_by_name<S>(
        &self,
        name: &str,
        store: &S,
        since: Option<NaiveDate>,
    ) -> Result<IngestResult, String>
    where
        S: MetadataStorage + CandleStorage + MacroStorage,
    {
        match name {
            "bars" => Ok(self.run_bars(store, since).await),
            "macro" => Ok(self.run_macro(store, since).await),
            "fundamentals" => Ok(self.run_fundamentals(store).await),
            other => Err(format!(
                "Unknown ingest job: '{other}'. Valid: bars, macro, fundamentals"
            )),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn collect_symbols<S: MetadataStorage>(store: &S) -> Vec<Symbol> {
    use futures::StreamExt;
    match store.list_tickers().await {
        Ok(stream) => stream.collect::<Vec<_>>().await,
        Err(e) => {
            eprintln!("[manager] failed to list tickers: {e}");
            vec![]
        }
    }
}

// ── IngestResult ──────────────────────────────────────────────────────────────

/// Summary of a single ingestion job run.
#[derive(Debug)]
pub struct IngestResult {
    pub job: &'static str,
    pub symbols_ok: usize,
    pub symbols_err: usize,
    pub records: usize,
    /// True if the job was a no-op because no connector was configured.
    pub skipped: bool,
    pub message: Option<String>,
}

impl IngestResult {
    fn skipped(job: &'static str, reason: &str) -> Self {
        eprintln!("[manager:{job}] skipped — {reason}");
        Self {
            job,
            symbols_ok: 0,
            symbols_err: 0,
            records: 0,
            skipped: true,
            message: Some(reason.to_string()),
        }
    }
}

impl std::fmt::Display for IngestResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.skipped {
            write!(
                f,
                "[{}] SKIPPED — {}",
                self.job,
                self.message.as_deref().unwrap_or("")
            )
        } else {
            write!(
                f,
                "[{}] ok={} err={} records={}{}",
                self.job,
                self.symbols_ok,
                self.symbols_err,
                self.records,
                self.message
                    .as_deref()
                    .map(|m| format!("  ({m})"))
                    .unwrap_or_default(),
            )
        }
    }
}
