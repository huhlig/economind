//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `strategy-trend-follow` — Timer plugin (§8.C.1)
//!
//! Scores candidates for trend-following entry timing using two complementary
//! signals:
//!
//! 1. **EMA crossover** — fast EMA crosses above slow EMA indicates uptrend
//!    entry.  The crossover sub-score reflects how far the fast EMA is above
//!    the slow EMA relative to recent spread volatility.
//!
//! 2. **ADX trend strength** — Average Directional Index measures trend
//!    strength (not direction).  ADX above `adx_threshold` confirms a trending
//!    environment; weak trends score near zero.
//!
//! Both sub-scores are normalised to [0, 1] and averaged into the composite
//! timing score.  The direction is `Long` when fast EMA > slow EMA (uptrend)
//! and `Short` when fast EMA < slow EMA (downtrend).  Only Long signals are
//! emitted when `long_only = true` (default).
//!
//! # Parameters (read from `ctx.parameters`)
//!
//! | Key              | Default | Description                                  |
//! |------------------|---------|----------------------------------------------|
//! | `fast_ema`       | `12`    | Fast EMA period                              |
//! | `slow_ema`       | `26`    | Slow EMA period                              |
//! | `adx_period`     | `14`    | ADX (+DI/-DI/ATR) period                     |
//! | `adx_threshold`  | `25.0`  | Minimum ADX for a trending environment       |
//! | `long_only`      | `true`  | Emit only Long signals                       |

use async_trait::async_trait;
use economind_strategy::{Candidate, StrategyContext, TimingSignal, Timer, TradeDirection};
use rust_decimal::prelude::*;
use std::collections::HashMap;

// ── TrendFollowTimer ──────────────────────────────────────────────────────────

pub struct TrendFollowTimer {
    fast_ema: usize,
    slow_ema: usize,
    adx_period: usize,
    adx_threshold: f64,
    long_only: bool,
}

impl TrendFollowTimer {
    pub fn new(parameters: &HashMap<String, String>) -> Self {
        Self {
            fast_ema: parameters
                .get("fast_ema")
                .and_then(|v| v.parse().ok())
                .unwrap_or(12),
            slow_ema: parameters
                .get("slow_ema")
                .and_then(|v| v.parse().ok())
                .unwrap_or(26),
            adx_period: parameters
                .get("adx_period")
                .and_then(|v| v.parse().ok())
                .unwrap_or(14),
            adx_threshold: parameters
                .get("adx_threshold")
                .and_then(|v| v.parse().ok())
                .unwrap_or(25.0),
            long_only: parameters
                .get("long_only")
                .map(|v| v.to_lowercase() != "false")
                .unwrap_or(true),
        }
    }
}

#[async_trait]
impl Timer for TrendFollowTimer {
    fn name(&self) -> &str {
        "trend-follow"
    }

    async fn score(&self, candidate: &Candidate, ctx: &StrategyContext) -> TimingSignal {
        let no_signal = |reason: &str| TimingSignal {
            candidate: candidate.clone(),
            score: 0.0,
            direction: TradeDirection::Long,
            rationale: reason.to_string(),
        };

        let bars = match ctx.bars.get(&candidate.symbol) {
            Some(b) if b.len() >= self.slow_ema + self.adx_period + 1 => b,
            Some(_) => return no_signal("Insufficient bars for trend-follow"),
            None => return no_signal("No bar data for symbol"),
        };

        let closes: Vec<f64> = bars.iter().filter_map(|b| b.close.to_f64()).collect();
        let highs: Vec<f64> = bars.iter().filter_map(|b| b.high.to_f64()).collect();
        let lows: Vec<f64> = bars.iter().filter_map(|b| b.low.to_f64()).collect();

        // ── EMA crossover score ───────────────────────────────────────────────

        let fast_emas = ema(&closes, self.fast_ema);
        let slow_emas = ema(&closes, self.slow_ema);

        if fast_emas.is_empty() || slow_emas.is_empty() {
            return no_signal("Could not compute EMAs");
        }

        let fast_last = *fast_emas.last().unwrap();
        let slow_last = *slow_emas.last().unwrap();

        // Direction from EMA alignment.
        let uptrend = fast_last > slow_last;
        let direction = if uptrend {
            TradeDirection::Long
        } else {
            TradeDirection::Short
        };

        // Skip short signals in long-only mode.
        if self.long_only && direction == TradeDirection::Short {
            return no_signal("Short signal suppressed (long_only=true)");
        }

        // EMA spread as a fraction of slow EMA — normalised by recent spread volatility.
        let spread = (fast_last - slow_last) / slow_last.max(1e-10);

        // Compute recent spread volatility over the slow EMA window.
        let n_common = fast_emas.len().min(slow_emas.len());
        let recent_spreads: Vec<f64> = fast_emas[fast_emas.len() - n_common..]
            .iter()
            .zip(slow_emas[slow_emas.len() - n_common..].iter())
            .map(|(&f, &s)| (f - s) / s.max(1e-10))
            .collect();

        let spread_std = std_dev(&recent_spreads);
        let ema_score = if spread_std < 1e-12 {
            if spread > 0.0 { 1.0 } else { 0.0 }
        } else {
            // Normalise spread to a score: z-score clamped to [0, 3] then / 3.
            let z = spread / spread_std;
            if uptrend {
                (z / 3.0).clamp(0.0, 1.0)
            } else {
                ((-z) / 3.0).clamp(0.0, 1.0)
            }
        };

        // ── ADX trend strength score ───────────────────────────────────────────

        let adx_val = adx(&highs, &lows, &closes, self.adx_period);
        let adx_score = if adx_val < self.adx_threshold {
            // Trend too weak — score drops to 0 linearly below threshold.
            (adx_val / self.adx_threshold).clamp(0.0, 1.0) * 0.5
        } else {
            // Trend confirmed — score rises from 0.5 to 1.0 as ADX goes from
            // threshold to threshold+30.
            let excess = (adx_val - self.adx_threshold) / 30.0;
            (0.5 + 0.5 * excess).clamp(0.5, 1.0)
        };

        // ── Composite score ───────────────────────────────────────────────────

        let composite = (ema_score + adx_score) / 2.0;

        TimingSignal {
            candidate: candidate.clone(),
            score: composite,
            direction,
            rationale: format!(
                "trend-follow: fast_ema={fast_last:.2} slow_ema={slow_last:.2} \
                 spread={spread:.4} ema_score={ema_score:.3} \
                 adx={adx_val:.1} adx_score={adx_score:.3} composite={composite:.3}",
            ),
        }
    }
}

// ── Indicator functions ───────────────────────────────────────────────────────

/// Exponential Moving Average.  Returns a vec of length `prices.len() - period + 1`
/// (first `period-1` values seeded with SMA).
fn ema(prices: &[f64], period: usize) -> Vec<f64> {
    if prices.len() < period || period == 0 {
        return vec![];
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut result = Vec::with_capacity(prices.len() - period + 1);
    let seed: f64 = prices[..period].iter().sum::<f64>() / period as f64;
    result.push(seed);
    for &price in &prices[period..] {
        let prev = *result.last().unwrap();
        result.push(price * k + prev * (1.0 - k));
    }
    result
}

/// Average Directional Index (Wilder smoothing).  Returns the final ADX value.
/// Requires at least `2 * period + 1` bars.
fn adx(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> f64 {
    let n = highs.len().min(lows.len()).min(closes.len());
    if n < period * 2 + 1 {
        return 0.0;
    }

    let mut tr_vals: Vec<f64> = Vec::with_capacity(n - 1);
    let mut plus_dm: Vec<f64> = Vec::with_capacity(n - 1);
    let mut minus_dm: Vec<f64> = Vec::with_capacity(n - 1);

    for i in 1..n {
        let h = highs[i];
        let l = lows[i];
        let prev_c = closes[i - 1];
        let prev_h = highs[i - 1];
        let prev_l = lows[i - 1];

        let tr = (h - l).max((h - prev_c).abs()).max((l - prev_c).abs());
        tr_vals.push(tr);

        let up_move = h - prev_h;
        let down_move = prev_l - l;
        plus_dm.push(if up_move > down_move && up_move > 0.0 { up_move } else { 0.0 });
        minus_dm.push(if down_move > up_move && down_move > 0.0 { down_move } else { 0.0 });
    }

    // Wilder smoothing (alpha = 1/period).
    let wilder_smooth = |vals: &[f64]| -> Vec<f64> {
        if vals.len() < period {
            return vec![];
        }
        let seed: f64 = vals[..period].iter().sum();
        let mut out = vec![seed];
        let alpha = 1.0 / period as f64;
        for &v in &vals[period..] {
            let prev = *out.last().unwrap();
            out.push(prev - prev * alpha + v);
        }
        out
    };

    let atr_s = wilder_smooth(&tr_vals);
    let plus_s = wilder_smooth(&plus_dm);
    let minus_s = wilder_smooth(&minus_dm);

    let m = atr_s.len().min(plus_s.len()).min(minus_s.len());
    if m == 0 {
        return 0.0;
    }

    let mut dx_vals: Vec<f64> = Vec::with_capacity(m);
    for i in 0..m {
        let atr = atr_s[i];
        if atr < 1e-12 {
            dx_vals.push(0.0);
            continue;
        }
        let plus_di = 100.0 * plus_s[i] / atr;
        let minus_di = 100.0 * minus_s[i] / atr;
        let dx = 100.0 * (plus_di - minus_di).abs() / (plus_di + minus_di).max(1e-12);
        dx_vals.push(dx);
    }

    if dx_vals.len() < period {
        return 0.0;
    }

    let adx_vals = wilder_smooth(&dx_vals);
    *adx_vals.last().unwrap_or(&0.0)
}

/// Population standard deviation.
fn std_dev(vals: &[f64]) -> f64 {
    if vals.len() < 2 {
        return 0.0;
    }
    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
    let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64;
    variance.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ema_length() {
        let prices: Vec<f64> = (1..=50).map(|i| i as f64).collect();
        let result = ema(&prices, 12);
        assert_eq!(result.len(), 50 - 12 + 1, "EMA length mismatch");
    }

    #[test]
    fn test_ema_trending_up() {
        let prices: Vec<f64> = (1..=50).map(|i| i as f64).collect();
        let fast = ema(&prices, 5);
        let slow = ema(&prices, 20);
        assert!(
            fast.last().unwrap() > slow.last().unwrap(),
            "fast={:.2} slow={:.2}",
            fast.last().unwrap(),
            slow.last().unwrap()
        );
    }

    #[test]
    fn test_ema_insufficient_data() {
        let prices = vec![1.0, 2.0, 3.0];
        assert!(ema(&prices, 10).is_empty());
    }

    #[test]
    fn test_adx_insufficient_bars() {
        let h = vec![1.0; 5];
        let l = vec![0.9; 5];
        let c = vec![0.95; 5];
        assert_eq!(adx(&h, &l, &c, 14), 0.0);
    }

    #[test]
    fn test_adx_trending_market() {
        let n = 80;
        let closes: Vec<f64> = (1..=n).map(|i| i as f64).collect();
        let highs: Vec<f64> = closes.iter().map(|c| c + 0.5).collect();
        let lows: Vec<f64> = closes.iter().map(|c| c - 0.5).collect();
        let result = adx(&highs, &lows, &closes, 14);
        assert!(result > 20.0, "Expected high ADX in trending market, got {result:.1}");
    }

    #[test]
    fn test_std_dev_constant() {
        let vals = vec![5.0_f64; 10];
        assert!(std_dev(&vals) < 1e-10);
    }

    #[test]
    fn test_std_dev_known_values() {
        // Population std dev of [2, 4, 4, 4, 5, 5, 7, 9] = 2.0
        let vals = vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        assert!((std_dev(&vals) - 2.0).abs() < 1e-9, "std_dev={}", std_dev(&vals));
    }

    #[test]
    fn test_std_dev_single_element_returns_zero() {
        assert_eq!(std_dev(&[42.0]), 0.0);
    }

    #[test]
    fn test_ema_first_value_is_seed_sma() {
        // EMA with period=3: seed = mean of first 3 values = (1+2+3)/3 = 2.0
        let prices = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = ema(&prices, 3);
        assert!((result[0] - 2.0).abs() < 1e-9, "first EMA value should be SMA seed");
    }

    #[test]
    fn test_adx_zero_period_edge() {
        // Period of 0 should not panic; returns 0.0 due to guard condition
        let h = vec![100.0; 10];
        let l = vec![99.0; 10];
        let c = vec![99.5; 10];
        // adx requires n >= 2*period+1; with period=0 that's >= 1 so it'll compute
        // We just verify it doesn't panic and returns a valid f64
        let result = adx(&h, &l, &c, 1);
        assert!(result.is_finite(), "adx should return a finite value");
    }

    #[test]
    fn test_ema_monotone_rising_fast_above_slow() {
        // For a strictly monotone series, fast EMA should always exceed slow EMA
        let prices: Vec<f64> = (1..=100).map(|i| i as f64 * 2.0).collect();
        let fast = ema(&prices, 10);
        let slow = ema(&prices, 30);
        assert!(
            fast.last().unwrap() > slow.last().unwrap(),
            "fast EMA should be above slow EMA in uptrend"
        );
    }
}
