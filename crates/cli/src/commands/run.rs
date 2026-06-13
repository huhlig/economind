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
use economind_strategy::{run_strategy, CompositionMode, PipelineRunnerBuilder, StrategyConfig};
use strategy_atr_sizer::AtrSizer;
use strategy_mean_reversion::MeanReversionTimer;
use strategy_momentum::MomentumIdentifier;
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
    let store = DataStore::open(duckdb_path)
        .context("Failed to open DataStore")?;

    // Load strategy config from DB.
    let config_row = store
        .get_strategy_config(args.config)
        .await
        .context("Failed to query strategy config")?
        .with_context(|| format!("Strategy config {} not found", args.config))?;

    // Deserialise to StrategyConfig.
    let config: StrategyConfig = serde_json::from_str(&config_row.parameters_json)
        .or_else(|_| {
            // Fall back: reconstruct a minimal config from the row fields.
            Ok::<StrategyConfig, serde_json::Error>(StrategyConfig::new(
                &config_row.name,
                match config_row.composition.as_str() {
                    "voting" => CompositionMode::Voting,
                    "ensemble" => CompositionMode::Ensemble,
                    _ => CompositionMode::Pipeline,
                },
                vec![],
                serde_json::from_str(&config_row.parameters_json).unwrap_or_default(),
            ))
        })
        .context("Failed to deserialise strategy config")?;

    if !config_row.enabled {
        bail!("Strategy config {} is disabled", args.config);
    }

    // Preload hot tables into memory before running strategy.
    store
        .preload(args.lookback_days)
        .await
        .context("Failed to preload hot data into memory")?;

    // Build pipeline from plugin specs.
    // In Phase 2 the known plugins are wired by name.  Phase 5 will introduce a
    // proper plugin registry loaded from the config's `plugins` JSON field.
    let mut builder = PipelineRunnerBuilder::new();

    // Instantiate plugins declared in the config (by role+name).
    let params = &config.parameters;

    let mut has_sizer = false;

    for spec in &config.plugins {
        match (spec.role.as_str(), spec.name.as_str()) {
            ("identifier", "momentum") => {
                builder = builder.identifier(MomentumIdentifier::new(params));
            }
            ("timer", "mean-reversion") => {
                builder = builder.timer(MeanReversionTimer::new(params));
            }
            ("sizer", "atr-sizer") => {
                builder = builder.sizer(AtrSizer::new(params));
                has_sizer = true;
            }
            (role, name) => {
                eprintln!("Warning: unknown plugin {role}:{name} — skipping");
            }
        }
    }

    // Require at least a sizer.
    if !has_sizer {
        bail!(
            "Config {} has no sizer plugin — cannot build pipeline",
            args.config
        );
    }

    // Set score threshold from params.
    let threshold: f64 = params
        .get("score_threshold")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.5);
    let runner = builder.score_threshold(threshold).build();

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
