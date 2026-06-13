//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `strategy-kelly-sizer` — Sizer plugin (§8.C.2)
//!
//! Computes position size using the Fractional Kelly Criterion:
//!
//! ```text
//! kelly_f  = win_rate - (1 - win_rate) / odds_ratio
//! fraction = kelly_fraction × kelly_f          (default kelly_fraction = 0.25)
//! position = fraction × portfolio_value / current_price
//! ```
//!
//! where `odds_ratio = avg_win / avg_loss`.
//!
//! **History source:** Win rate and avg win/loss are read from the
//! `StrategyContext` parameters under the keys `kelly_win_rate`,
//! `kelly_avg_win`, and `kelly_avg_loss`.  The orchestrator is responsible for
//! injecting these values (queried from `backtest.trades`) before a run.  If
//! the keys are absent or the trade count is below `min_trades` the sizer
//! **falls back to ATR-based sizing** (identical to `strategy-atr-sizer`).
//!
//! This design keeps the plugin dependency-free (no DB access inside the
//! Sizer trait) while still enabling data-driven sizing.
//!
//! # Parameters (read from `ctx.parameters`)
//!
//! | Key                | Default | Description                                          |
//! |--------------------|---------|------------------------------------------------------|
//! | `kelly_fraction`   | `0.25`  | Fractional Kelly multiplier                          |
//! | `min_trades`       | `30`    | Minimum completed trades before Kelly is used        |
//! | `kelly_win_rate`   | —       | Historical win rate (0.0–1.0); injected by orchestrator |
//! | `kelly_avg_win`    | —       | Average winning trade return; injected by orchestrator  |
//! | `kelly_avg_loss`   | —       | Average losing trade return (positive); injected        |
//! | `kelly_trade_count`| —       | Number of completed trades used; injected               |
//! | `risk_per_trade`   | `0.01`  | ATR fallback: fraction of portfolio to risk per trade |
//! | `max_position_pct` | `0.05`  | Max position size as fraction of portfolio            |
//! | `atr_period`       | `14`    | ATR fallback: look-back period                        |

use async_trait::async_trait;
use economind_core::model::DailyCandleEntry;
use economind_strategy::{PositionSize, Sizer, StrategyContext, TimingSignal};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use std::collections::HashMap;

// ── KellySizer ────────────────────────────────────────────────────────────────

pub struct KellySizer {
    kelly_fraction: f64,
    min_trades: u32,
    // ATR fallback parameters.
    risk_per_trade: f64,
    max_position_pct: f64,
    atr_period: usize,
}

impl KellySizer {
    pub fn new(parameters: &HashMap<String, String>) -> Self {
        Self {
            kelly_fraction: parameters
                .get("kelly_fraction")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.25),
            min_trades: parameters
                .get("min_trades")
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            risk_per_trade: parameters
                .get("risk_per_trade")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.01),
            max_position_pct: parameters
                .get("max_position_pct")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.05),
            atr_period: parameters
                .get("atr_period")
                .and_then(|v| v.parse().ok())
                .unwrap_or(14),
        }
    }
}

#[async_trait]
impl Sizer for KellySizer {
    fn name(&self) -> &str {
        "kelly-sizer"
    }

    async fn size(&self, signal: &TimingSignal, ctx: &StrategyContext) -> PositionSize {
        let zero_size = PositionSize {
            symbol: signal.candidate.symbol.clone(),
            shares: Decimal::ZERO,
            notional: Decimal::ZERO,
            portfolio_fraction: Decimal::ZERO,
        };

        let portfolio_value_f = match ctx.portfolio_value.to_f64() {
            Some(v) if v > 0.0 => v,
            _ => return zero_size,
        };

        // Try to read pre-computed Kelly stats from context parameters.
        let trade_count: u32 = ctx
            .parameters
            .get("kelly_trade_count")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let kelly_position_fraction = if trade_count >= self.min_trades {
            // Kelly stats available — compute fractional Kelly.
            let win_rate = ctx
                .parameters
                .get("kelly_win_rate")
                .and_then(|v| v.parse::<f64>().ok());
            let avg_win = ctx
                .parameters
                .get("kelly_avg_win")
                .and_then(|v| v.parse::<f64>().ok());
            let avg_loss = ctx
                .parameters
                .get("kelly_avg_loss")
                .and_then(|v| v.parse::<f64>().ok());

            match (win_rate, avg_win, avg_loss) {
                (Some(wr), Some(aw), Some(al)) if al > 0.0 && aw > 0.0 => {
                    let odds_ratio = aw / al;
                    let full_kelly = wr - (1.0 - wr) / odds_ratio;
                    // Never size into a negative Kelly (negative expectancy).
                    let fraction = (full_kelly * self.kelly_fraction).max(0.0);
                    // Hard cap at max_position_pct regardless of Kelly output.
                    fraction.min(self.max_position_pct)
                }
                _ => {
                    // Stats present but malformed — fall back to ATR.
                    return self.atr_fallback(signal, ctx, portfolio_value_f, &zero_size);
                }
            }
        } else {
            // Insufficient history — ATR fallback.
            return self.atr_fallback(signal, ctx, portfolio_value_f, &zero_size);
        };

        if kelly_position_fraction <= 0.0 {
            return zero_size;
        }

        // Get current price from bars.
        let bars = match ctx.bars.get(&signal.candidate.symbol) {
            Some(b) if !b.is_empty() => b,
            _ => return zero_size,
        };

        let current_price = match bars.last().and_then(|b| b.close.to_f64()) {
            Some(p) if p > 0.0 => p,
            _ => return zero_size,
        };

        let position_value = portfolio_value_f * kelly_position_fraction;
        let raw_shares = position_value / current_price;
        let shares = (raw_shares * 100.0).floor() / 100.0; // 2dp truncation

        if shares < 0.01 {
            return zero_size;
        }

        let notional = shares * current_price;
        let portfolio_fraction = notional / portfolio_value_f;

        let to_dec = |v: f64| Decimal::try_from(v).unwrap_or(Decimal::ZERO);

        PositionSize {
            symbol: signal.candidate.symbol.clone(),
            shares: to_dec(shares),
            notional: to_dec(notional),
            portfolio_fraction: to_dec(portfolio_fraction),
        }
    }
}

impl KellySizer {
    /// ATR-based fallback sizing when Kelly stats are unavailable.
    fn atr_fallback(
        &self,
        signal: &TimingSignal,
        ctx: &StrategyContext,
        portfolio_value_f: f64,
        zero_size: &PositionSize,
    ) -> PositionSize {
        let bars = match ctx.bars.get(&signal.candidate.symbol) {
            Some(b) if b.len() > self.atr_period => b,
            _ => return zero_size.clone(),
        };

        let current_price = match bars.last().and_then(|b| b.close.to_f64()) {
            Some(p) if p > 0.0 => p,
            _ => return zero_size.clone(),
        };

        let atr = compute_atr(bars, self.atr_period);
        if atr < 1e-10 {
            return zero_size.clone();
        }

        let dollar_risk = portfolio_value_f * self.risk_per_trade;
        let position_value = dollar_risk * (current_price / atr);
        let max_value = portfolio_value_f * self.max_position_pct;
        let capped_value = position_value.min(max_value);

        let raw_shares = capped_value / current_price;
        let shares = (raw_shares * 100.0).floor() / 100.0;

        if shares < 0.01 {
            return zero_size.clone();
        }

        let notional = shares * current_price;
        let portfolio_fraction = notional / portfolio_value_f;

        let to_dec = |v: f64| Decimal::try_from(v).unwrap_or(Decimal::ZERO);

        PositionSize {
            symbol: signal.candidate.symbol.clone(),
            shares: to_dec(shares),
            notional: to_dec(notional),
            portfolio_fraction: to_dec(portfolio_fraction),
        }
    }
}

// ── Kelly stats computation helper ────────────────────────────────────────────
//
// Called by the orchestrator layer (economind-strategy / CLI) to pre-compute
// Kelly stats from backtest trade history and inject them into ctx.parameters.

/// Pre-computed Kelly statistics derived from completed backtest trades.
#[derive(Debug, Clone)]
pub struct KellyStats {
    /// Number of completed (closed) trades in the sample.
    pub trade_count: u32,
    /// Fraction of trades that were profitable.
    pub win_rate: f64,
    /// Average return of winning trades (positive fraction, e.g. 0.05 = 5%).
    pub avg_win: f64,
    /// Average return of losing trades (positive magnitude, e.g. 0.03 = 3%).
    pub avg_loss: f64,
}

impl KellyStats {
    /// Compute Kelly statistics from a list of net P&L values (as fractions of
    /// position notional).
    ///
    /// Returns `None` if `pnl_fractions` is empty.
    pub fn from_pnl_fractions(pnl_fractions: &[f64]) -> Option<Self> {
        if pnl_fractions.is_empty() {
            return None;
        }
        let wins: Vec<f64> = pnl_fractions.iter().copied().filter(|&x| x > 0.0).collect();
        let losses: Vec<f64> = pnl_fractions.iter().copied().filter(|&x| x < 0.0).collect();

        let n = pnl_fractions.len() as u32;
        let win_rate = wins.len() as f64 / n as f64;
        let avg_win = if wins.is_empty() {
            0.0
        } else {
            wins.iter().sum::<f64>() / wins.len() as f64
        };
        let avg_loss = if losses.is_empty() {
            0.0
        } else {
            losses.iter().map(|x| x.abs()).sum::<f64>() / losses.len() as f64
        };

        Some(Self {
            trade_count: n,
            win_rate,
            avg_win,
            avg_loss,
        })
    }

    /// Inject these stats as string parameters into a parameter map, so they
    /// can be read back by `KellySizer` via `ctx.parameters`.
    pub fn inject_into_params(&self, params: &mut HashMap<String, String>) {
        params.insert("kelly_trade_count".to_string(), self.trade_count.to_string());
        params.insert("kelly_win_rate".to_string(), self.win_rate.to_string());
        params.insert("kelly_avg_win".to_string(), self.avg_win.to_string());
        params.insert("kelly_avg_loss".to_string(), self.avg_loss.to_string());
    }

    /// Compute the fractional Kelly position size fraction.
    ///
    /// Returns 0.0 for negative or zero Kelly (avoid the trade).
    pub fn fractional_kelly(&self, fraction: f64) -> f64 {
        if self.avg_loss < 1e-12 || self.avg_win < 1e-12 {
            return 0.0;
        }
        let odds = self.avg_win / self.avg_loss;
        let full_kelly = self.win_rate - (1.0 - self.win_rate) / odds;
        (full_kelly * fraction).max(0.0)
    }
}

// ── ATR calculation ───────────────────────────────────────────────────────────

fn compute_atr(bars: &[DailyCandleEntry], period: usize) -> f64 {
    if bars.len() < period + 1 {
        return 0.0;
    }
    let n = bars.len();
    let recent = &bars[n - period - 1..];
    let true_ranges: Vec<f64> = recent
        .windows(2)
        .filter_map(|w| {
            let prev_close = w[0].close.to_f64()?;
            let high = w[1].high.to_f64()?;
            let low = w[1].low.to_f64()?;
            let tr = (high - low)
                .max((high - prev_close).abs())
                .max((low - prev_close).abs());
            Some(tr)
        })
        .collect();
    if true_ranges.is_empty() {
        return 0.0;
    }
    true_ranges.iter().sum::<f64>() / true_ranges.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kelly_stats_from_pnl() {
        // 6 wins of 5%, 4 losses of 3%.
        let mut pnls = vec![0.05_f64; 6];
        pnls.extend(vec![-0.03_f64; 4]);
        let stats = KellyStats::from_pnl_fractions(&pnls).unwrap();
        assert_eq!(stats.trade_count, 10);
        assert!((stats.win_rate - 0.6).abs() < 1e-9);
        assert!((stats.avg_win - 0.05).abs() < 1e-9);
        assert!((stats.avg_loss - 0.03).abs() < 1e-9);
    }

    #[test]
    fn test_fractional_kelly_positive_expectancy() {
        // win_rate=0.6, avg_win=0.05, avg_loss=0.03
        // odds = 5/3, kelly = 0.6 - 0.4/(5/3) = 0.6 - 0.24 = 0.36
        // fractional (0.25) = 0.09
        let stats = KellyStats {
            trade_count: 100,
            win_rate: 0.6,
            avg_win: 0.05,
            avg_loss: 0.03,
        };
        let fk = stats.fractional_kelly(0.25);
        assert!((fk - 0.09).abs() < 1e-6, "Expected ~0.09, got {fk:.6}");
    }

    #[test]
    fn test_fractional_kelly_negative_expectancy() {
        // Losing strategy: win_rate=0.3, avg_win=0.02, avg_loss=0.05
        let stats = KellyStats {
            trade_count: 100,
            win_rate: 0.3,
            avg_win: 0.02,
            avg_loss: 0.05,
        };
        let fk = stats.fractional_kelly(0.25);
        // Kelly is negative → should return 0.0.
        assert_eq!(fk, 0.0, "Negative expectancy should return 0.0");
    }

    #[test]
    fn test_inject_and_read_params() {
        let stats = KellyStats {
            trade_count: 50,
            win_rate: 0.55,
            avg_win: 0.04,
            avg_loss: 0.025,
        };
        let mut params = HashMap::new();
        stats.inject_into_params(&mut params);
        assert_eq!(params.get("kelly_trade_count").unwrap(), "50");
        assert!(params.contains_key("kelly_win_rate"));
        assert!(params.contains_key("kelly_avg_win"));
        assert!(params.contains_key("kelly_avg_loss"));
    }

    #[test]
    fn test_empty_pnl_returns_none() {
        assert!(KellyStats::from_pnl_fractions(&[]).is_none());
    }
}
