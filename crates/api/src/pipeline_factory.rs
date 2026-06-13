//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Wires compiled strategy plugins into a `PipelineRunner`.
//!
//! This is the single place in the binary that knows about the concrete plugin
//! types.  The API and backtest endpoints call `build_pipeline()` rather than
//! constructing plugins directly, keeping the rest of the codebase decoupled
//! from plugin implementation details.

use anyhow::anyhow;
use economind_db::StrategyConfigRow;
use economind_strategy::{
    config::{CompositionMode, ExecutionMode, PluginSpec, StrategyConfig},
    pipeline::{PipelineRunner, PipelineRunnerBuilder},
};
use std::collections::HashMap;
use strategy_atr_sizer::AtrSizer;
use strategy_kelly_sizer::KellySizer;
use strategy_mean_reversion::MeanReversionTimer;
use strategy_momentum::MomentumIdentifier;
use strategy_regime::RegimeIdentifier;
use strategy_trend_follow::TrendFollowTimer;

/// Build a `PipelineRunner` from a `StrategyConfig`.
///
/// Iterates the config's `plugins` list and instantiates each named plugin.
/// Unknown plugin names return an error so the caller can surface it to the API
/// client rather than silently skipping plugins.
///
/// # Errors
/// Returns an error if a plugin name is not recognised.  The caller is
/// responsible for validating configs before they reach this function.
pub fn build_pipeline(config: &StrategyConfig) -> anyhow::Result<PipelineRunner> {
    let mut builder = PipelineRunnerBuilder::new();
    let mut has_sizer = false;

    for spec in &config.plugins {
        match (spec.role.as_str(), spec.name.as_str()) {
            ("identifier", "momentum") => {
                builder = builder.identifier(MomentumIdentifier::new(&config.parameters));
            }
            ("identifier", "regime") => {
                builder = builder.identifier(RegimeIdentifier::new(&config.parameters));
            }
            ("timer", "mean-reversion") => {
                builder = builder.timer(MeanReversionTimer::new(&config.parameters));
            }
            ("timer", "trend-follow") => {
                builder = builder.timer(TrendFollowTimer::new(&config.parameters));
            }
            ("sizer", "atr-sizer") => {
                builder = builder.sizer(AtrSizer::new(&config.parameters));
                has_sizer = true;
            }
            ("sizer", "kelly-sizer") => {
                builder = builder.sizer(KellySizer::new(&config.parameters));
                has_sizer = true;
            }
            (role, name) => {
                return Err(anyhow!("unknown plugin: role={role}, name={name}"));
            }
        }
    }

    if !has_sizer {
        return Err(anyhow!("pipeline has no Sizer plugin configured"));
    }

    Ok(builder.build())
}

/// Reconstruct a domain `StrategyConfig` from the persisted database row.
pub fn strategy_config_from_row(row: StrategyConfigRow) -> anyhow::Result<StrategyConfig> {
    let plugins: Vec<PluginSpec> = serde_json::from_str(&row.plugins_json)
        .map_err(|e| anyhow!("invalid plugins JSON for strategy {}: {e}", row.id))?;
    let parameters: HashMap<String, String> = serde_json::from_str(&row.parameters_json)
        .map_err(|e| anyhow!("invalid parameters JSON for strategy {}: {e}", row.id))?;
    let composition = match row.composition.as_str() {
        "pipeline" => CompositionMode::Pipeline,
        "voting" => CompositionMode::Voting,
        "ensemble" => CompositionMode::Ensemble,
        other => return Err(anyhow!("unknown composition for strategy {}: {other}", row.id)),
    };

    Ok(StrategyConfig {
        id: row.id,
        name: row.name,
        description: row.description,
        composition,
        plugins,
        parameters,
        enabled: row.enabled,
        auto_execute: row.auto_execute,
        execution_mode: ExecutionMode::parse_lossy(&row.execution_mode),
        version: row.version,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}
