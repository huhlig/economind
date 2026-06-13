//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `StrategyStack` — a self-contained Identifier + Timer(s) + Sizer unit.
//!
//! A `StrategyStack` is the building block for multi-mode composition.
//! Each stack is a complete mini-pipeline: one or more Identifiers, one or
//! more Timers, and exactly one Sizer.  The `VotingRunner` and
//! `EnsembleRunner` each manage a collection of stacks and combine their
//! outputs differently.
//!
//! A `StrategyStack` on its own behaves identically to a `PipelineRunner`.

use crate::context::StrategyContext;
use crate::pipeline::TradeSignal;
use crate::traits::{
    Candidate, Identifier, PositionSize, Sizer, Timer, TimingSignal, TradeDirection,
};

use rust_decimal::Decimal;

// ── StrategyStack ─────────────────────────────────────────────────────────────

/// A named, self-contained strategy pipeline.
///
/// Contains one or more Identifiers, zero or more Timers, and exactly one
/// Sizer.  The `name` is used in composite rationale strings so logs and
/// signal metadata identify which stack contributed.
pub struct StrategyStack {
    /// Human-readable name for this stack (e.g. "momentum-reversion").
    pub name: String,
    pub identifiers: Vec<Box<dyn Identifier>>,
    pub timers: Vec<Box<dyn Timer>>,
    pub sizer: Box<dyn Sizer>,
    /// Minimum averaged Timer score for a candidate to pass (0.0–1.0).
    pub score_threshold: f64,
}

impl Default for StrategyStack {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            identifiers: vec![],
            timers: vec![],
            sizer: Box::new(PassthroughSizer),
            score_threshold: 0.5,
        }
    }
}

impl StrategyStack {
    /// Run this stack's pipeline and return `TradeSignal`s for candidates
    /// that meet `score_threshold`.
    pub async fn run(&self, ctx: &StrategyContext) -> Vec<TradeSignal> {
        // Stage 1: Identification
        let mut candidates: Vec<Candidate> = ctx
            .universe
            .iter()
            .map(|s| Candidate {
                symbol: s.clone(),
                score: 1.0,
                metadata: Default::default(),
            })
            .collect();

        for id in &self.identifiers {
            candidates = id.identify(ctx).await;
            if candidates.is_empty() {
                return vec![];
            }
        }

        // Stage 2: Timing + Stage 3: Sizing
        let mut signals: Vec<TradeSignal> = Vec::new();

        for candidate in &candidates {
            if self.timers.is_empty() {
                let timing = TimingSignal {
                    candidate: candidate.clone(),
                    score: candidate.score,
                    direction: TradeDirection::Long,
                    rationale: format!("[{}] No timer — passthrough", self.name),
                };
                let size = self.sizer.size(&timing, ctx).await;
                signals.push(TradeSignal { timing, size });
                continue;
            }

            let mut timer_outputs = Vec::with_capacity(self.timers.len());
            for timer in &self.timers {
                timer_outputs.push(timer.score(candidate, ctx).await);
            }

            let avg_score =
                timer_outputs.iter().map(|s| s.score).sum::<f64>() / timer_outputs.len() as f64;

            if avg_score < self.score_threshold {
                continue;
            }

            let mut rep = timer_outputs.into_iter().next().unwrap();
            rep.score = avg_score;
            rep.rationale = format!("[{}] {}", self.name, rep.rationale);

            let size = self.sizer.size(&rep, ctx).await;
            signals.push(TradeSignal { timing: rep, size });
        }

        signals
    }
}

// ── StrategyStackBuilder ──────────────────────────────────────────────────────

/// Fluent builder for `StrategyStack`.
pub struct StrategyStackBuilder {
    name: String,
    identifiers: Vec<Box<dyn Identifier>>,
    timers: Vec<Box<dyn Timer>>,
    sizer: Option<Box<dyn Sizer>>,
    score_threshold: Option<f64>,
}

impl StrategyStackBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            identifiers: vec![],
            timers: vec![],
            sizer: None,
            score_threshold: None,
        }
    }

    pub fn identifier(mut self, i: impl Identifier + 'static) -> Self {
        self.identifiers.push(Box::new(i));
        self
    }

    pub fn timer(mut self, t: impl Timer + 'static) -> Self {
        self.timers.push(Box::new(t));
        self
    }

    pub fn sizer(mut self, s: impl Sizer + 'static) -> Self {
        self.sizer = Some(Box::new(s));
        self
    }

    pub fn score_threshold(mut self, t: f64) -> Self {
        self.score_threshold = Some(t);
        self
    }

    /// Build the `StrategyStack`.
    ///
    /// # Panics
    /// Panics if no Sizer has been registered.
    pub fn build(self) -> StrategyStack {
        StrategyStack {
            name: self.name,
            identifiers: self.identifiers,
            timers: self.timers,
            sizer: self.sizer.expect("StrategyStack requires a Sizer"),
            score_threshold: self.score_threshold.unwrap_or(0.5),
        }
    }
}

// ── PassthroughSizer ──────────────────────────────────────────────────────────
//
// Internal zero-size sizer used by Default. Not exported.

struct PassthroughSizer;

#[async_trait::async_trait]
impl Sizer for PassthroughSizer {
    fn name(&self) -> &str {
        "passthrough"
    }
    async fn size(&self, signal: &TimingSignal, _ctx: &StrategyContext) -> PositionSize {
        PositionSize {
            symbol: signal.candidate.symbol.clone(),
            shares: Decimal::ZERO,
            notional: Decimal::ZERO,
            portfolio_fraction: Decimal::ZERO,
        }
    }
}
