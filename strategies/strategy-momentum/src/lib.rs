//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `strategy-momentum` — Identifier plugin (§2.C.1)
//!
//! Ranks instruments by **risk-adjusted momentum**: rolling return over a
//! configurable lookback window divided by rolling annualised volatility.
//! Returns the top-N candidates by score.
//!
//! # Parameters (read from `ctx.parameters`)
//!
//! | Key             | Default | Description                            |
//! |-----------------|---------|----------------------------------------|
//! | `lookback_days` | `90`    | Rolling return / volatility window     |
//! | `top_n`         | `20`    | Maximum number of candidates to emit   |
//! | `min_bars`      | `30`    | Minimum bar count to consider a symbol |
//!
//! Symbols with fewer than `min_bars` data points are silently skipped.

use async_trait::async_trait;
use economind_core::model::DailyCandleEntry;
use economind_strategy::{Candidate, Identifier, StrategyContext};
use rust_decimal::prelude::*;
use std::collections::HashMap;

// ── MomentumIdentifier ────────────────────────────────────────────────────────

pub struct MomentumIdentifier {
    lookback_days: usize,
    top_n: usize,
    min_bars: usize,
}

impl MomentumIdentifier {
    pub fn new(parameters: &HashMap<String, String>) -> Self {
        Self {
            lookback_days: parameters
                .get("lookback_days")
                .and_then(|v| v.parse().ok())
                .unwrap_or(90),
            top_n: parameters
                .get("top_n")
                .and_then(|v| v.parse().ok())
                .unwrap_or(20),
            min_bars: parameters
                .get("min_bars")
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
        }
    }
}

#[async_trait]
impl Identifier for MomentumIdentifier {
    fn name(&self) -> &str {
        "momentum"
    }

    async fn identify(&self, ctx: &StrategyContext) -> Vec<Candidate> {
        let mut scored: Vec<(String, f64)> = Vec::new();

        for (symbol, bars) in &ctx.bars {
            // Need at least min_bars data points.
            if bars.len() < self.min_bars {
                continue;
            }

            // Use the most recent `lookback_days` bars (sorted oldest → newest).
            let n = bars.len();
            let start = n.saturating_sub(self.lookback_days);
            let window_bars: &[DailyCandleEntry] = &bars[start..];

            if window_bars.len() < 2 {
                continue;
            }

            let score = risk_adjusted_momentum(window_bars);
            if score.is_finite() {
                scored.push((symbol.as_str().to_string(), score));
            }
        }

        if scored.is_empty() {
            return vec![];
        }

        // Sort descending by score.
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Normalise scores to [0, 1] relative to the top/bottom within the top-N window.
        let top_slice = scored
            .iter()
            .take(self.top_n)
            .map(|(_, s)| *s)
            .collect::<Vec<_>>();

        let max_score = top_slice.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min_score = top_slice.iter().cloned().fold(f64::INFINITY, f64::min);
        let range = (max_score - min_score).max(1e-10);

        scored
            .into_iter()
            .take(self.top_n)
            .map(|(sym, raw_score)| {
                let normalised = ((raw_score - min_score) / range).clamp(0.0, 1.0);
                Candidate {
                    symbol: economind_core::model::Symbol::new(&sym),
                    score: normalised,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("raw_momentum".to_string(), format!("{raw_score:.6}"));
                        m
                    },
                }
            })
            .collect()
    }
}

// ── Indicator functions ───────────────────────────────────────────────────────

/// Risk-adjusted momentum: total_return / annualised_volatility.
///
/// Returns `f64::NEG_INFINITY` for series that are flat or too short.
fn risk_adjusted_momentum(bars: &[DailyCandleEntry]) -> f64 {
    if bars.len() < 2 {
        return f64::NEG_INFINITY;
    }

    let first_close = bars.first().unwrap().close;
    let last_close = bars.last().unwrap().close;

    if first_close.is_zero() {
        return f64::NEG_INFINITY;
    }

    // Total return over the window.
    let total_return = ((last_close - first_close) / first_close)
        .to_f64()
        .unwrap_or(0.0);

    // Daily log returns for volatility.
    let daily_returns: Vec<f64> = bars
        .windows(2)
        .filter_map(|w| {
            let prev = w[0].close.to_f64()?;
            let curr = w[1].close.to_f64()?;
            if prev > 0.0 {
                Some((curr / prev).ln())
            } else {
                None
            }
        })
        .collect();

    if daily_returns.len() < 2 {
        return total_return;
    }

    let vol = annualised_volatility(&daily_returns);
    if vol < 1e-10 {
        return f64::NEG_INFINITY;
    }

    total_return / vol
}

/// Annualised volatility from daily log returns (√252 scaling).
fn annualised_volatility(returns: &[f64]) -> f64 {
    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    variance.sqrt() * 252.0_f64.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use rust_decimal::Decimal;

    fn make_bars(prices: &[f64]) -> Vec<DailyCandleEntry> {
        prices
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                let d = Decimal::try_from(p).unwrap();
                DailyCandleEntry {
                    date: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
                        + chrono::Duration::days(i as i64),
                    open: d,
                    high: d,
                    low: d,
                    close: d,
                    volume: 1_000_000,
                }
            })
            .collect()
    }

    #[test]
    fn test_positive_momentum() {
        let prices: Vec<f64> = (100..=190).map(|i| i as f64).collect();
        let score = risk_adjusted_momentum(&make_bars(&prices));
        assert!(score > 0.0, "Expected positive momentum, got {score}");
    }

    #[test]
    fn test_negative_momentum() {
        let prices: Vec<f64> = (100..=190).rev().map(|i| i as f64).collect();
        let score = risk_adjusted_momentum(&make_bars(&prices));
        assert!(score < 0.0, "Expected negative momentum, got {score}");
    }

    #[test]
    fn test_flat_returns_neg_infinity() {
        let prices = vec![100.0_f64; 30];
        let score = risk_adjusted_momentum(&make_bars(&prices));
        assert_eq!(score, f64::NEG_INFINITY);
    }

    #[test]
    fn test_annualised_vol_constant_returns() {
        // Constant daily return of 1% → vol = 0.01 * sqrt(252).
        let returns = vec![0.01_f64; 50];
        let vol = annualised_volatility(&returns);
        assert!(
            vol < 1e-10,
            "Constant returns should give near-zero vol: {vol}"
        );
    }
}
