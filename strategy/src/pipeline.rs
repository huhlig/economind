//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Pipeline composition engine — runs Identifiers → Timers → Sizers in sequence.
//!
//! Stage behaviour (§2.B.1):
//! - Identifiers run in series; each narrows the candidate list.
//! - Multiple Timers each score every surviving candidate; scores are **averaged**.
//! - The single Sizer computes position size for each averaged signal above threshold.

use crate::context::StrategyContext;
use crate::traits::{Candidate, Identifier, PositionSize, Sizer, TimingSignal, Timer, TradeDirection};

// ── TradeSignal ───────────────────────────────────────────────────────────────

/// An in-memory trade recommendation produced by one pipeline run.
///
/// Callers convert this to `PersistedSignal` via `PersistedSignal::from_pipeline`
/// before writing to the database.
#[derive(Debug, Clone)]
pub struct TradeSignal {
    pub timing: TimingSignal,
    pub size: PositionSize,
}

// ── PipelineRunner ────────────────────────────────────────────────────────────

/// Runs a sequence of Identifiers, Timers, and Sizers against a StrategyContext.
///
/// # Usage
/// ```ignore
/// let runner = PipelineRunnerBuilder::new()
///     .identifier(MomentumIdentifier::new(&params))
///     .timer(MeanReversionTimer::new(&params))
///     .sizer(AtrSizer::new(&params))
///     .score_threshold(0.55)
///     .build();
///
/// let signals = runner.run(&ctx).await;
/// ```
pub struct PipelineRunner {
    pub identifiers: Vec<Box<dyn Identifier>>,
    pub timers: Vec<Box<dyn Timer>>,
    pub sizer: Box<dyn Sizer>,
    /// Minimum averaged Timer score required to pass through to sizing (0.0–1.0).
    pub score_threshold: f64,
}

impl PipelineRunner {
    /// Execute the full Identifier → Timer → Sizer pipeline.
    ///
    /// Returns `TradeSignal`s for every candidate whose averaged Timer score
    /// meets or exceeds `score_threshold`.
    pub async fn run(&self, ctx: &StrategyContext) -> Vec<TradeSignal> {
        // ── Stage 1: Identification ───────────────────────────────────────────
        //
        // Seed with the full universe (score 1.0, no metadata).
        // Each Identifier replaces the candidate list with its own filtered output.
        let mut candidates: Vec<Candidate> = ctx
            .universe
            .iter()
            .map(|s| Candidate {
                symbol: s.clone(),
                score: 1.0,
                metadata: Default::default(),
            })
            .collect();

        for identifier in &self.identifiers {
            candidates = identifier.identify(ctx).await;
            if candidates.is_empty() {
                return vec![];
            }
        }

        // ── Stage 2: Timing ───────────────────────────────────────────────────
        let mut signals: Vec<TradeSignal> = Vec::new();

        for candidate in &candidates {
            if self.timers.is_empty() {
                // No Timers configured — pass all candidates at full identifier score.
                let timing = TimingSignal {
                    candidate: candidate.clone(),
                    score: candidate.score,
                    direction: TradeDirection::Long,
                    rationale: "No timer configured — passthrough".to_string(),
                };
                let size = self.sizer.size(&timing, ctx).await;
                signals.push(TradeSignal { timing, size });
                continue;
            }

            let timer_outputs: Vec<TimingSignal> = {
                let mut v = Vec::with_capacity(self.timers.len());
                for timer in &self.timers {
                    v.push(timer.score(candidate, ctx).await);
                }
                v
            };

            let avg_score = timer_outputs.iter().map(|s| s.score).sum::<f64>()
                / timer_outputs.len() as f64;

            if avg_score < self.score_threshold {
                continue;
            }

            // Use the first Timer's direction/rationale as the representative;
            // replace its score with the averaged value.
            let mut representative = timer_outputs.into_iter().next().unwrap();
            representative.score = avg_score;

            // ── Stage 3: Sizing ───────────────────────────────────────────────
            let size = self.sizer.size(&representative, ctx).await;

            signals.push(TradeSignal {
                timing: representative,
                size,
            });
        }

        signals
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Fluent builder for `PipelineRunner`.
#[derive(Default)]
pub struct PipelineRunnerBuilder {
    identifiers: Vec<Box<dyn Identifier>>,
    timers: Vec<Box<dyn Timer>>,
    sizer: Option<Box<dyn Sizer>>,
    score_threshold: Option<f64>,
}

impl PipelineRunnerBuilder {
    pub fn new() -> Self {
        Self::default()
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

    pub fn score_threshold(mut self, threshold: f64) -> Self {
        self.score_threshold = Some(threshold);
        self
    }

    /// Build the `PipelineRunner`.
    ///
    /// # Panics
    /// Panics if no Sizer has been registered.
    pub fn build(self) -> PipelineRunner {
        PipelineRunner {
            identifiers: self.identifiers,
            timers: self.timers,
            sizer: self.sizer.expect("PipelineRunner requires a Sizer"),
            score_threshold: self.score_threshold.unwrap_or(0.5),
        }
    }
}
