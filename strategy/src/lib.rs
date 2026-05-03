//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Economind Strategy Engine
//!
//! Defines the core strategy traits (`Identifier`, `Timer`, `Sizer`), the
//! pipeline composition engine, strategy configuration types, run result types,
//! and the top-level `run_strategy` orchestration function.
//!
//! Strategy plugin crates implement the traits; this crate hosts and orchestrates them.

pub mod config;
pub mod context;
pub mod orchestrator;
pub mod pipeline;
pub mod run;
pub mod traits;

// Flat re-exports of the most commonly used items.
pub use self::config::{CompositionMode, PluginSpec, StrategyConfig};
pub use self::orchestrator::run_strategy;
pub use self::pipeline::{PipelineRunner, PipelineRunnerBuilder, TradeSignal};
pub use self::run::{PersistedSignal, RunStatus, StrategyRunResult};
pub use self::context::StrategyContext;
pub use self::traits::{Candidate, Identifier, PositionSize, Sizer, TimingSignal, Timer, TradeDirection};
