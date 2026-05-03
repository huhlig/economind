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
use economind_strategy::{
    config::StrategyConfig,
    pipeline::{PipelineRunner, PipelineRunnerBuilder},
};
use strategy_atr_sizer::AtrSizer;
use strategy_mean_reversion::MeanReversionTimer;
use strategy_momentum::MomentumIdentifier;

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
            ("timer", "mean-reversion") => {
                builder = builder.timer(MeanReversionTimer::new(&config.parameters));
            }
            ("sizer", "atr-sizer") => {
                builder = builder.sizer(AtrSizer::new(&config.parameters));
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
