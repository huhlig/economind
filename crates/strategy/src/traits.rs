//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Core strategy traits: Identifier, Timer, Sizer.

use crate::context::StrategyContext;
use async_trait::async_trait;
use economind_core::model::Symbol;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Output types ─────────────────────────────────────────────────────────────

/// An instrument candidate produced by an Identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub symbol: Symbol,
    /// Identifier score (0.0 = weakest, 1.0 = strongest).
    pub score: f64,
    /// Arbitrary key-value metadata the Identifier may attach.
    pub metadata: HashMap<String, String>,
}

/// A timing signal produced by a Timer for a single Candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingSignal {
    pub candidate: Candidate,
    /// Timing score (0.0 = poor entry, 1.0 = optimal entry).
    pub score: f64,
    /// Suggested trade direction.
    pub direction: TradeDirection,
    /// Human-readable rationale for this signal.
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeDirection {
    Long,
    Short,
}

/// A position size recommendation produced by a Sizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionSize {
    pub symbol: Symbol,
    /// Number of shares / units to trade.
    pub shares: Decimal,
    /// Notional value of the position.
    pub notional: Decimal,
    /// Fraction of total portfolio capital this represents.
    pub portfolio_fraction: Decimal,
}

// ── Strategy traits ───────────────────────────────────────────────────────────

/// Filters the instrument universe down to trade candidates.
#[async_trait]
pub trait Identifier: Send + Sync {
    fn name(&self) -> &str;
    async fn identify(&self, ctx: &StrategyContext) -> Vec<Candidate>;
}

/// Scores each candidate for entry timing attractiveness.
#[async_trait]
pub trait Timer: Send + Sync {
    fn name(&self) -> &str;
    async fn score(&self, candidate: &Candidate, ctx: &StrategyContext) -> TimingSignal;
}

/// Computes a position size for a timing signal.
#[async_trait]
pub trait Sizer: Send + Sync {
    fn name(&self) -> &str;
    async fn size(&self, signal: &TimingSignal, ctx: &StrategyContext) -> PositionSize;
}
