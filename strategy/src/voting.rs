//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Voting / Consensus composition engine (§8.A).
//!
//! `VotingRunner` takes a collection of independent `StrategyStack`s, runs them
//! all against the same `StrategyContext`, then tallies a binary vote per
//! instrument.  An instrument receives a signal only when the fraction of stacks
//! that voted for it meets or exceeds the configured `quorum` threshold.
//!
//! ## Vote definition
//!
//! A stack "votes" for an instrument when:
//! 1. The stack's Identifier includes the instrument in its candidate list, **and**
//! 2. The stack's Timer scores the candidate at or above the stack's
//!    `score_threshold`.
//!
//! ## Signal assembly
//!
//! For every instrument that reaches quorum, the position size is computed by
//! averaging the `PositionSize` outputs from all stacks that voted for it.
//! The consensus timing score is the simple average of those stacks' Timer scores.

use crate::context::StrategyContext;
use crate::pipeline::TradeSignal;
use crate::stack::StrategyStack;
use crate::traits::{Candidate, PositionSize, TimingSignal, TradeDirection};
use economind_core::model::Symbol;
use rust_decimal::Decimal;
use std::collections::HashMap;

// ── VotingRunner ──────────────────────────────────────────────────────────────

/// Runs multiple strategy stacks in parallel and emits consensus signals.
pub struct VotingRunner {
    /// The independent stacks to run.
    pub stacks: Vec<StrategyStack>,
    /// Fraction of stacks that must vote for an instrument (0.0–1.0].
    /// Defaults to simple majority (> 0.5).  Must be > 0.0.
    pub quorum: f64,
}

impl VotingRunner {
    /// Create a new `VotingRunner`.
    ///
    /// # Panics
    /// Panics if `quorum` is ≤ 0.0 or > 1.0, or if `stacks` is empty.
    pub fn new(stacks: Vec<StrategyStack>, quorum: f64) -> Self {
        assert!(!stacks.is_empty(), "VotingRunner requires at least one stack");
        assert!(
            quorum > 0.0 && quorum <= 1.0,
            "quorum must be in (0.0, 1.0]"
        );
        Self { stacks, quorum }
    }

    /// Run all stacks and return consensus `TradeSignal`s.
    pub async fn run(&self, ctx: &StrategyContext) -> Vec<TradeSignal> {
        let n = self.stacks.len();
        let quorum_count = (self.quorum * n as f64).ceil() as usize;

        // Collect per-instrument votes: symbol → list of (TimingSignal, PositionSize).
        let mut votes: HashMap<Symbol, Vec<(TimingSignal, PositionSize)>> = HashMap::new();

        for stack in &self.stacks {
            let stack_signals = stack.run(ctx).await;
            for ts in stack_signals {
                votes
                    .entry(ts.timing.candidate.symbol.clone())
                    .or_default()
                    .push((ts.timing, ts.size));
            }
        }

        // Emit a signal for each instrument that reached quorum.
        let mut signals: Vec<TradeSignal> = Vec::new();

        for (symbol, vote_list) in votes {
            if vote_list.len() < quorum_count {
                continue;
            }

            let vote_count = vote_list.len() as f64;

            // Average timing score across voting stacks.
            let avg_timing_score =
                vote_list.iter().map(|(t, _)| t.score).sum::<f64>() / vote_count;

            // Use the direction from the first vote (all stacks should agree for
            // long-only strategies; this is the expected common case).
            let direction = vote_list
                .first()
                .map(|(t, _)| t.direction)
                .unwrap_or(TradeDirection::Long);

            // Average identifier score.
            let avg_identifier_score = vote_list
                .iter()
                .map(|(t, _)| t.candidate.score)
                .sum::<f64>()
                / vote_count;

            // Average position size components.
            let avg_shares = average_decimal(vote_list.iter().map(|(_, s)| s.shares));
            let avg_notional = average_decimal(vote_list.iter().map(|(_, s)| s.notional));
            let avg_fraction =
                average_decimal(vote_list.iter().map(|(_, s)| s.portfolio_fraction));

            let rationale = format!(
                "Voting consensus: {}/{} stacks agreed (quorum={:.0}%); avg timing score={:.3}",
                vote_list.len(),
                n,
                self.quorum * 100.0,
                avg_timing_score,
            );

            let consensus_candidate = Candidate {
                symbol: symbol.clone(),
                score: avg_identifier_score,
                metadata: {
                    let mut m = std::collections::HashMap::new();
                    m.insert("vote_count".to_string(), vote_list.len().to_string());
                    m.insert("stack_count".to_string(), n.to_string());
                    m
                },
            };

            signals.push(TradeSignal {
                timing: TimingSignal {
                    candidate: consensus_candidate,
                    score: avg_timing_score,
                    direction,
                    rationale,
                },
                size: PositionSize {
                    symbol,
                    shares: avg_shares,
                    notional: avg_notional,
                    portfolio_fraction: avg_fraction,
                },
            });
        }

        // Sort descending by timing score for deterministic output.
        signals.sort_by(|a, b| {
            b.timing
                .score
                .partial_cmp(&a.timing.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        signals
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn average_decimal<I: Iterator<Item = Decimal>>(iter: I) -> Decimal {
    let values: Vec<Decimal> = iter.collect();
    if values.is_empty() {
        return Decimal::ZERO;
    }
    let sum: Decimal = values.iter().copied().sum();
    sum / Decimal::from(values.len())
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Fluent builder for `VotingRunner`.
#[derive(Default)]
pub struct VotingRunnerBuilder {
    stacks: Vec<StrategyStack>,
    quorum: Option<f64>,
}

impl VotingRunnerBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stack(mut self, s: StrategyStack) -> Self {
        self.stacks.push(s);
        self
    }

    /// Quorum fraction (0.0, 1.0].  Defaults to simple majority (0.5 + ε).
    pub fn quorum(mut self, q: f64) -> Self {
        self.quorum = Some(q);
        self
    }

    pub fn build(self) -> VotingRunner {
        VotingRunner::new(
            self.stacks,
            self.quorum.unwrap_or(0.5 + f64::EPSILON),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quorum_validation() {
        // Should not panic.
        let _ = VotingRunner::new(
            vec![StrategyStack::default()],
            0.5,
        );
    }

    #[test]
    #[should_panic]
    fn test_zero_quorum_panics() {
        let _ = VotingRunner::new(vec![StrategyStack::default()], 0.0);
    }
}
