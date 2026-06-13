//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `economind run` — trigger a strategy run and print the emitted signals.
//!
//! # Usage
//! ```text
//! economind run --config <uuid>  [--lookback-days 365]
//! ```
//!
//! The command:
//! 1. Loads the `StrategyConfig` from the database.
//! 2. Instantiates the correct plugins based on the config's plugin specs.
//! 3. Runs the pipeline via `run_strategy`.
//! 4. Prints a table of emitted signals.

use anyhow::{bail, Context};
use clap::Args;
use economind_db::{DataStore, StrategyStorage};
use economind_strategy::run_strategy;
use uuid::Uuid;

#[derive(Args)]
pub struct RunArgs {
    /// UUID of the strategy config to run.
    #[arg(long)]
    pub config: Uuid,

    /// Bar lookback window in days (default: 365).
    #[arg(long, default_value = "365")]
    pub lookback_days: u32,
}

pub async fn execute(args: RunArgs, duckdb_path: &str) -> anyhow::Result<()> {
    let store = DataStore::open(duckdb_path).context("Failed to open DataStore")?;

    // Load strategy config from DB.
    let config_row = store
        .get_strategy_config(args.config)
        .await
        .context("Failed to query strategy config")?
        .with_context(|| format!("Strategy config {} not found", args.config))?;

    if !config_row.enabled {
        bail!("Strategy config {} is disabled", args.config);
    }

    let config = economind_api::pipeline_factory::strategy_config_from_row(config_row)
        .context("Failed to reconstruct strategy config")?;

    // Preload hot tables into memory before running strategy.
    store
        .preload(args.lookback_days)
        .await
        .context("Failed to preload hot data into memory")?;

    let runner = economind_api::pipeline_factory::build_pipeline(&config)
        .context("Failed to build strategy pipeline")?;

    println!("Running strategy: {} ({})", config.name, args.config);

    let result = run_strategy(&config, &runner, &store).await;

    println!(
        "Run {} — status: {:?} — {} signal(s)",
        result.run_id,
        result.status,
        result.signal_count()
    );

    if let Some(err) = &result.error_message {
        eprintln!("Error: {err}");
        return Ok(());
    }

    if result.signals.is_empty() {
        println!("No signals emitted.");
        return Ok(());
    }

    // Print signal table.
    println!(
        "\n{:<8} {:<12} {:<10} {:<8} {:<10} {:<10} Rationale",
        "Symbol", "Direction", "Id.Score", "Tm.Score", "Shares", "Notional"
    );
    println!("{}", "─".repeat(100));

    for sig in &result.signals {
        println!(
            "{:<8} {:<12} {:<8.3} {:<8.3} {:<10} {:<10} {}",
            sig.symbol.as_str(),
            format!("{:?}", sig.direction),
            sig.identifier_score,
            sig.timing_score,
            sig.shares
                .map(|s| format!("{s:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            sig.notional
                .map(|n| format!("{n:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            sig.rationale.as_deref().unwrap_or("—"),
        );
    }

    Ok(())
}
