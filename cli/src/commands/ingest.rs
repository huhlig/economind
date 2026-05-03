//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! `economind ingest` — on-demand data ingestion commands.
//!
//! # Usage
//! ```text
//! economind ingest bars          [--since <YYYY-MM-DD>]
//! economind ingest macro         [--since <YYYY-MM-DD>]
//! economind ingest fundamentals
//! ```
//!
//! Each subcommand instantiates the relevant connector from environment
//! variables and runs the corresponding `DataFeedManager` job against
//! the configured DataStore.
//!
//! # Environment variables
//!
//! | Variable          | Required for     |
//! |-------------------|------------------|
//! | DATABASE_URL      | All subcommands  |
//! | FRED_API_KEY      | `macro`          |
//! | SIMFIN_API_KEY    | `fundamentals`   |

use anyhow::Context;
use chrono::NaiveDate;
use clap::{Args, Subcommand};
use economind_db::DataStore;
use economind_ingest::{
    DataFeedManager, DataFeedManagerConfig, EdgarConnector, FredConnector, SimFinConnector,
    YahooFinanceConnector,
};

// ── Top-level args ────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct IngestArgs {
    #[command(subcommand)]
    pub command: IngestCommand,
}

#[derive(Subcommand)]
pub enum IngestCommand {
    /// Download and store daily OHLCV bars for all tracked instruments.
    Bars(BarsArgs),
    /// Fetch macro time series from FRED.
    Macro(MacroArgs),
    /// Fetch annual fundamental statements (IS/BS/CF) from EDGAR and SimFin.
    Fundamentals(FundamentalsArgs),
}

#[derive(Args)]
pub struct BarsArgs {
    /// Fetch bars from this date onward (YYYY-MM-DD).
    /// Defaults to today minus the configured backfill window.
    #[arg(long)]
    pub since: Option<NaiveDate>,

    /// Parallel download concurrency (default: 4).
    #[arg(long, default_value = "4")]
    pub concurrency: usize,
}

#[derive(Args)]
pub struct MacroArgs {
    /// Fetch observations from this date onward (YYYY-MM-DD).
    #[arg(long)]
    pub since: Option<NaiveDate>,

    /// Comma-separated FRED series IDs to fetch (defaults to standard set).
    /// Example: --series DGS10,T10Y2Y,CPIAUCSL
    #[arg(long, value_delimiter = ',')]
    pub series: Option<Vec<String>>,
}

#[derive(Args)]
pub struct FundamentalsArgs {
    /// Use only EDGAR (skip SimFin).
    #[arg(long)]
    pub edgar_only: bool,

    /// Use only SimFin (skip EDGAR).
    #[arg(long)]
    pub simfin_only: bool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn execute(
    args: IngestArgs,
    database_url: &str,
    duckdb_path: &str,
) -> anyhow::Result<()> {
    let store = DataStore::connect(database_url, duckdb_path)
        .await
        .context("Failed to connect to DataStore")?;

    match args.command {
        IngestCommand::Bars(a) => run_bars(a, &store).await,
        IngestCommand::Macro(a) => run_macro(a, &store).await,
        IngestCommand::Fundamentals(a) => run_fundamentals(a, &store).await,
    }
}

// ── bars ──────────────────────────────────────────────────────────────────────

async fn run_bars(args: BarsArgs, store: &DataStore) -> anyhow::Result<()> {
    let yahoo = YahooFinanceConnector::new().with_concurrency(args.concurrency);

    let manager = DataFeedManager::new(DataFeedManagerConfig::default()).with_yahoo(yahoo);

    println!("Starting bar ingestion{}…",
        args.since.map(|d| format!(" since {d}")).unwrap_or_default());

    let result = manager.run_bars(store, args.since).await;
    println!("{result}");

    if result.symbols_err > 0 && result.symbols_ok == 0 {
        anyhow::bail!("Bar ingestion failed for all symbols");
    }
    Ok(())
}

// ── macro ─────────────────────────────────────────────────────────────────────

async fn run_macro(args: MacroArgs, store: &DataStore) -> anyhow::Result<()> {
    let fred = FredConnector::from_env()
        .context("Failed to create FRED connector — is FRED_API_KEY set?")?;

    let config = DataFeedManagerConfig {
        fred_series: args.series.clone(),
        ..Default::default()
    };
    let manager = DataFeedManager::new(config).with_fred(fred);

    let series_desc = args.series
        .as_deref()
        .map(|v| v.join(", "))
        .unwrap_or_else(|| "default series".to_string());

    println!("Fetching FRED macro series ({series_desc})…");

    let result = manager.run_macro(store, args.since).await;
    println!("{result}");

    if result.symbols_err > 0 && result.symbols_ok == 0 {
        anyhow::bail!("Macro ingestion failed for all series");
    }
    Ok(())
}

// ── fundamentals ──────────────────────────────────────────────────────────────

async fn run_fundamentals(args: FundamentalsArgs, store: &DataStore) -> anyhow::Result<()> {
    let mut manager = DataFeedManager::new(DataFeedManagerConfig::default());

    if !args.simfin_only {
        manager = manager.with_edgar(EdgarConnector::new());
    }

    if !args.edgar_only {
        match SimFinConnector::from_env() {
            Ok(sf) => {
                manager = manager.with_simfin(sf);
            }
            Err(_) => {
                if args.simfin_only {
                    anyhow::bail!("SIMFIN_API_KEY not set — cannot run SimFin-only ingestion");
                }
                eprintln!("Note: SIMFIN_API_KEY not set — SimFin connector skipped");
            }
        }
    }

    println!("Fetching fundamental statements (IS/BS/CF)…");

    let result = manager.run_fundamentals(store).await;
    println!("{result}");

    if result.symbols_err > 0 && result.symbols_ok == 0 {
        anyhow::bail!("Fundamentals ingestion failed for all instruments");
    }
    Ok(())
}
