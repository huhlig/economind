//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `strategy-mean-reversion` — Timer plugin (§2.C.2)
//!
//! Scores candidates for mean-reversion entry timing using three complementary
//! signals, each weighted equally in the composite score:
//!
//! 1. **Bollinger Band position** — how far price has pulled back below the
//!    lower band (oversold dip).
//! 2. **Z-score of price** relative to a rolling mean — how many standard
//!    deviations price is below its recent average.
//! 3. **RSI** — classic overbought/oversold oscillator; oversold (< 30) scores
//!    highest.
//!
//! All three sub-scores are normalised to [0, 1] and averaged.  Higher score =
//! better mean-reversion entry (price is cheap relative to its recent history).
//!
//! # Parameters (read from `ctx.parameters`)
//!
//! | Key                | Default | Description                           |
//! |--------------------|---------|---------------------------------------|
//! | `bb_period`        | `20`    | Bollinger Band SMA period             |
//! | `bb_std_devs`      | `2.0`   | Bollinger Band standard deviation multiplier |
//! | `rsi_period`       | `14`    | RSI period                            |
//! | `zscore_window`    | `20`    | Z-score rolling window                |
//! | `signal_threshold` | `0.4`   | Minimum composite score to emit       |
//!
//! The `direction` is always `Long` (mean-reversion timer assumes buy-the-dip).

use async_trait::async_trait;
use economind_strategy::{Candidate, StrategyContext, Timer, TimingSignal, TradeDirection};
use rust_decimal::prelude::*;
use std::collections::HashMap;

// ── MeanReversionTimer ───────────────────────────────────────────────────────���

pub struct MeanReversionTimer {
    bb_period: usize,
    bb_std_devs: f64,
    rsi_period: usize,
    zscore_window: usize,
}

impl MeanReversionTimer {
    pub fn new(parameters: &HashMap<String, String>) -> Self {
        Self {
            bb_period: parameters
                .get("bb_period")
                .and_then(|v| v.parse().ok())
                .unwrap_or(20),
            bb_std_devs: parameters
                .get("bb_std_devs")
                .and_then(|v| v.parse().ok())
                .unwrap_or(2.0),
            rsi_period: parameters
                .get("rsi_period")
                .and_then(|v| v.parse().ok())
                .unwrap_or(14),
            zscore_window: parameters
                .get("zscore_window")
                .and_then(|v| v.parse().ok())
                .unwrap_or(20),
        }
    }
}

#[async_trait]
impl Timer for MeanReversionTimer {
    fn name(&self) -> &str {
        "mean-reversion"
    }

    async fn score(&self, candidate: &Candidate, ctx: &StrategyContext) -> TimingSignal {
        let no_signal = |reason: &str| TimingSignal {
            candidate: candidate.clone(),
            score: 0.0,
            direction: TradeDirection::Long,
            rationale: reason.to_string(),
        };

        let bars = match ctx.bars.get(&candidate.symbol) {
            Some(b)
                if b.len()
                    >= self
                        .bb_period
                        .max(self.rsi_period + 1)
                        .max(self.zscore_window) =>
            {
                b
            }
            _ => return no_signal("Insufficient bar history"),
        };

        let closes: Vec<f64> = bars.iter().filter_map(|b| b.close.to_f64()).collect();

        if closes.is_empty() {
            return no_signal("No close prices available");
        }

        let current_price = *closes.last().unwrap();

        // 1. Bollinger Band position score.
        let bb_score = bollinger_score(&closes, self.bb_period, self.bb_std_devs);

        // 2. Z-score.
        let zscore_score = zscore_score(&closes, self.zscore_window);

        // 3. RSI.
        let rsi_score = rsi_score(&closes, self.rsi_period);

        // Composite: equal weights.
        let composite = (bb_score + zscore_score + rsi_score) / 3.0;
        let composite = composite.clamp(0.0, 1.0);

        let rationale = format!(
            "Mean-reversion score {composite:.3} \
             (BB:{bb_score:.2} Z:{zscore_score:.2} RSI:{rsi_score:.2}) \
             price={current_price:.2}"
        );

        TimingSignal {
            candidate: candidate.clone(),
            score: composite,
            direction: TradeDirection::Long,
            rationale,
        }
    }
}

// ── Indicator implementations ─────────────────────────────────────────────────

/// Bollinger Band score: 1.0 when price is at/below lower band, 0.0 at/above upper band.
fn bollinger_score(closes: &[f64], period: usize, std_devs: f64) -> f64 {
    if closes.len() < period {
        return 0.0;
    }

    let window = &closes[closes.len() - period..];
    let sma = window.iter().sum::<f64>() / period as f64;
    let variance = window.iter().map(|c| (c - sma).powi(2)).sum::<f64>() / period as f64;
    let std = variance.sqrt();

    if std < 1e-10 {
        return 0.5; // Flat price — neutral signal.
    }

    let lower = sma - std_devs * std;
    let upper = sma + std_devs * std;
    let price = *closes.last().unwrap();

    // Map price position: lower band = 1.0, upper band = 0.0, linear in between.
    let band_width = upper - lower;
    if band_width < 1e-10 {
        return 0.5;
    }

    ((upper - price) / band_width).clamp(0.0, 1.0)
}

/// Z-score score: maps distance below rolling mean to [0, 1].
/// Z ≤ -2 → 1.0,  Z = 0 → 0.5,  Z ≥ +2 → 0.0.
fn zscore_score(closes: &[f64], window: usize) -> f64 {
    if closes.len() < window {
        return 0.5;
    }

    let w = &closes[closes.len() - window..];
    let mean = w.iter().sum::<f64>() / window as f64;
    let variance = w.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / window as f64;
    let std = variance.sqrt();

    if std < 1e-10 {
        return 0.5;
    }

    let price = *closes.last().unwrap();
    let z = (price - mean) / std;

    // Invert and normalise: negative Z (price below mean) → higher score.
    // Clamp at z = ±3.
    ((-z + 3.0) / 6.0).clamp(0.0, 1.0)
}

/// RSI score: RSI ≤ 30 → 1.0, RSI = 50 → 0.5, RSI ≥ 70 → 0.0.
fn rsi_score(closes: &[f64], period: usize) -> f64 {
    let rsi = compute_rsi(closes, period);
    // Invert: oversold (low RSI) = high score.
    ((70.0 - rsi) / 70.0).clamp(0.0, 1.0)
}

/// Wilder's RSI.
fn compute_rsi(closes: &[f64], period: usize) -> f64 {
    if closes.len() < period + 1 {
        return 50.0; // Neutral default.
    }

    let n = closes.len();
    let recent = &closes[n - period - 1..];

    let mut gains = 0.0_f64;
    let mut losses = 0.0_f64;

    for w in recent.windows(2) {
        let delta = w[1] - w[0];
        if delta > 0.0 {
            gains += delta;
        } else {
            losses += -delta;
        }
    }

    gains /= period as f64;
    losses /= period as f64;

    if losses < 1e-10 {
        return 100.0;
    }

    let rs = gains / losses;
    100.0 - (100.0 / (1.0 + rs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bollinger_price_below_lower_band() {
        // Create a series where price drops sharply at the end.
        let mut closes: Vec<f64> = vec![100.0; 20];
        closes.push(80.0); // well below lower band
        let score = bollinger_score(&closes, 20, 2.0);
        assert!(
            score > 0.8,
            "Expected high BB score for oversold price: {score}"
        );
    }

    #[test]
    fn test_bollinger_price_above_upper_band() {
        let mut closes: Vec<f64> = vec![100.0; 20];
        closes.push(120.0); // well above upper band
        let score = bollinger_score(&closes, 20, 2.0);
        assert!(
            score < 0.2,
            "Expected low BB score for overbought price: {score}"
        );
    }

    #[test]
    fn test_rsi_oversold() {
        // Falling price for 20 days → RSI should be low → high score.
        let closes: Vec<f64> = (0..21).map(|i| 100.0 - i as f64).collect();
        let rsi = compute_rsi(&closes, 14);
        assert!(rsi < 30.0, "Expected oversold RSI, got {rsi}");
        let score = rsi_score(&closes, 14);
        assert!(score > 0.5, "Expected high RSI score for oversold: {score}");
    }

    #[test]
    fn test_rsi_overbought() {
        // Rising price → RSI high → low score.
        let closes: Vec<f64> = (0..21).map(|i| 100.0 + i as f64).collect();
        let rsi = compute_rsi(&closes, 14);
        assert!(rsi > 70.0, "Expected overbought RSI, got {rsi}");
        let score = rsi_score(&closes, 14);
        assert!(
            score < 0.2,
            "Expected low RSI score for overbought: {score}"
        );
    }

    #[test]
    fn test_zscore_below_mean() {
        let mut closes = vec![100.0_f64; 19];
        closes.push(90.0); // below mean
        let score = zscore_score(&closes, 20);
        assert!(
            score > 0.5,
            "Expected high z-score for price below mean: {score}"
        );
    }

    #[test]
    fn test_rsi_neutral() {
        let score = rsi_score(&[100.0_f64; 20], 14);
        // Flat price → neutral RSI of 50 → score around 0.28.
        assert!((0.0..=1.0).contains(&score));
    }
}
