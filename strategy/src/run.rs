//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Strategy run result types.
//!
//! A `StrategyRunResult` is returned by `run_strategy()` and contains the run
//! metadata plus the list of `TradeSignal`s produced by the pipeline.  These
//! types mirror the `strategy.runs` and `strategy.signals` DB tables (§1.B.3).

use crate::config::StrategyConfig;
use crate::traits::{PositionSize, TimingSignal, TradeDirection};
use chrono::{DateTime, Utc};
use economind_core::model::Symbol;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Run status ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
}

// ── PersistedSignal ───────────────────────────────────────────────────────────

/// A complete trade recommendation ready for DB persistence and downstream use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSignal {
    pub id: Uuid,
    pub run_id: Uuid,
    pub config_id: Uuid,
    pub symbol: Symbol,
    pub direction: TradeDirection,
    /// Score from the Identifier stage (0.0–1.0).
    pub identifier_score: f64,
    /// Averaged score from the Timer stage(s) (0.0–1.0).
    pub timing_score: f64,
    /// Computed position size.
    pub shares: Option<Decimal>,
    pub notional: Option<Decimal>,
    pub portfolio_fraction: Option<Decimal>,
    /// Human-readable rationale from the winning Timer.
    pub rationale: Option<String>,
    /// Populated by the agentic layer in Phase 7.
    pub analysis_brief: Option<String>,
    pub emitted_at: DateTime<Utc>,
}

impl PersistedSignal {
    /// Build a `PersistedSignal` from a timing signal + position size.
    pub fn from_pipeline(
        run_id: Uuid,
        config_id: Uuid,
        timing: &TimingSignal,
        size: &PositionSize,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            run_id,
            config_id,
            symbol: timing.candidate.symbol.clone(),
            direction: timing.direction,
            identifier_score: timing.candidate.score,
            timing_score: timing.score,
            shares: Some(size.shares),
            notional: Some(size.notional),
            portfolio_fraction: Some(size.portfolio_fraction),
            rationale: Some(timing.rationale.clone()),
            analysis_brief: None,
            emitted_at: Utc::now(),
        }
    }
}

// ── StrategyRunResult ─────────────────────────────────────────────────────────

/// The complete outcome of a single strategy run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRunResult {
    pub run_id: Uuid,
    pub config_id: Uuid,
    /// Snapshot of the config parameters as-of this run.
    pub config_snapshot: StrategyConfig,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub status: RunStatus,
    pub signals: Vec<PersistedSignal>,
    /// Set when `status == Failed`.
    pub error_message: Option<String>,
}

impl StrategyRunResult {
    pub fn signal_count(&self) -> usize {
        self.signals.len()
    }

    /// Return only long signals.
    pub fn longs(&self) -> impl Iterator<Item = &PersistedSignal> {
        self.signals
            .iter()
            .filter(|s| s.direction == TradeDirection::Long)
    }

    /// Return only short signals.
    pub fn shorts(&self) -> impl Iterator<Item = &PersistedSignal> {
        self.signals
            .iter()
            .filter(|s| s.direction == TradeDirection::Short)
    }
}
