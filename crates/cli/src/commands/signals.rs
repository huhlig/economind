//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `economind signals` — query and display recent signals from the database.
//!
//! # Usage
//! ```text
//! economind signals [--since <YYYY-MM-DD>] [--limit <n>] [--symbol <sym>]
//!                   [--config <uuid>]
//! ```

use anyhow::Context;
use chrono::NaiveDate;
use clap::Args;
use economind_core::model::Symbol;
use economind_db::{DataStore, StrategyStorage};
use uuid::Uuid;

#[derive(Args)]
pub struct SignalsArgs {
    /// Filter signals emitted on or after this date (YYYY-MM-DD).
    #[arg(long)]
    pub since: Option<NaiveDate>,

    /// Maximum number of signals to display (default: 50).
    #[arg(long, default_value = "50")]
    pub limit: u32,

    /// Filter by instrument symbol (e.g. AAPL).
    #[arg(long)]
    pub symbol: Option<String>,

    /// Filter by strategy config UUID.
    #[arg(long)]
    pub config: Option<Uuid>,
}

pub async fn execute(
    args: SignalsArgs,
    duckdb_path: &str,
) -> anyhow::Result<()> {
    let store = DataStore::open(duckdb_path)
        .context("Failed to open DataStore")?;

    let symbol = args.symbol.as_deref().map(Symbol::new);

    let signals = store
        .query_strategy_signals(
            None,             // run_id
            args.config,      // config_id
            symbol.as_ref(),  // symbol
            args.since,       // since date
            Some(args.limit), // limit
        )
        .await
        .context("Failed to query signals")?;

    if signals.is_empty() {
        println!("No signals found matching the given filters.");
        return Ok(());
    }

    println!(
        "{:<8} {:<12} {:<8} {:<8} {:<12} {:<10} Rationale",
        "Symbol", "Direction", "Id.Sc", "Tm.Sc", "Emitted", "Notional"
    );
    println!("{}", "─".repeat(110));

    for sig in &signals {
        let emitted = sig.emitted_at.format("%Y-%m-%d %H:%M").to_string();
        let notional = sig
            .position_notional
            .map(|n| format!("{n:.0}"))
            .unwrap_or_else(|| "—".to_string());
        let rationale = sig
            .rationale
            .as_deref()
            .unwrap_or("—")
            .chars()
            .take(60)
            .collect::<String>();

        println!(
            "{:<8} {:<12} {:<8.3} {:<8.3} {:<12} {:<10} {}",
            sig.symbol,
            sig.direction,
            sig.identifier_score,
            sig.timing_score,
            emitted,
            notional,
            rationale,
        );
    }

    println!("\n{} signal(s) shown.", signals.len());
    Ok(())
}
