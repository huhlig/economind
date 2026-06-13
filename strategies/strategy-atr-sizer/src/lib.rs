//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `strategy-atr-sizer` — Sizer plugin (§2.C.3)
//!
//! Computes position size using ATR-based volatility normalization:
//!
//! ```text
//! position_value = (portfolio_value × risk_per_trade) / ATR(period)
//! shares         = position_value / current_price
//! ```
//!
//! Position value is then capped at `max_position_pct × portfolio_value`.
//! This ensures each trade risks approximately the same dollar amount
//! regardless of the instrument's price level.
//!
//! # Parameters (read from `ctx.parameters`)
//!
//! | Key               | Default | Description                                     |
//! |-------------------|---------|-------------------------------------------------|
//! | `risk_per_trade`  | `0.01`  | Fraction of portfolio to risk per trade (1%)    |
//! | `max_position_pct`| `0.05`  | Max position size as fraction of portfolio (5%) |
//! | `atr_period`      | `14`    | ATR look-back period                            |

use async_trait::async_trait;
use economind_core::model::DailyCandleEntry;
use economind_strategy::{PositionSize, Sizer, StrategyContext, TimingSignal};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use std::collections::HashMap;

// ── AtrSizer ──────────────────────────────────────────────────────────────────

pub struct AtrSizer {
    risk_per_trade: f64,
    max_position_pct: f64,
    atr_period: usize,
}

impl AtrSizer {
    pub fn new(parameters: &HashMap<String, String>) -> Self {
        Self {
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
impl Sizer for AtrSizer {
    fn name(&self) -> &str {
        "atr-sizer"
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

        let bars = match ctx.bars.get(&signal.candidate.symbol) {
            Some(b) if b.len() > self.atr_period => b,
            _ => return zero_size,
        };

        let current_price = match bars.last().and_then(|b| b.close.to_f64()) {
            Some(p) if p > 0.0 => p,
            _ => return zero_size,
        };

        let atr = compute_atr(bars, self.atr_period);
        if atr < 1e-10 {
            return zero_size;
        }

        // Dollar risk per trade.
        let dollar_risk = portfolio_value_f * self.risk_per_trade;

        // Position value: how much we can buy such that a 1×ATR move = dollar_risk.
        let position_value = dollar_risk * (current_price / atr);

        // Cap at max_position_pct of portfolio.
        let max_value = portfolio_value_f * self.max_position_pct;
        let capped_value = position_value.min(max_value);

        // Convert to shares (round down to 2 decimal places for fractional shares).
        let raw_shares = capped_value / current_price;
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

// ── ATR calculation ───────────────────────────────────────────────────────────

/// Wilder's Average True Range over `period` bars.
///
/// True Range = max(high−low, |high−prev_close|, |low−prev_close|)
pub fn compute_atr(bars: &[DailyCandleEntry], period: usize) -> f64 {
    if bars.len() < period + 1 {
        return 0.0;
    }

    let n = bars.len();
    let recent = &bars[n - period - 1..];

    // Calculate True Range for each bar (needs previous close).
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

    // Simple average (Wilder's first ATR; smoothed EMA for subsequent updates would
    // require state — for a stateless indicator simple average is standard for Phase 2).
    true_ranges.iter().sum::<f64>() / true_ranges.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn make_bar(i: usize, open: f64, high: f64, low: f64, close: f64) -> DailyCandleEntry {
        let to_dec = |v: f64| Decimal::try_from(v).unwrap();
        DailyCandleEntry {
            date: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap() + chrono::Duration::days(i as i64),
            open: to_dec(open),
            high: to_dec(high),
            low: to_dec(low),
            close: to_dec(close),
            volume: 1_000_000,
        }
    }

    #[test]
    fn test_atr_flat_price() {
        // All bars with same OHLC → true range per bar is 0.
        let bars: Vec<DailyCandleEntry> = (0..20)
            .map(|i| make_bar(i, 100.0, 101.0, 99.0, 100.0))
            .collect();
        let atr = compute_atr(&bars, 14);
        // TR = high - low = 2.0 for every bar.
        assert!((atr - 2.0).abs() < 0.01, "Expected ATR ≈ 2.0, got {atr}");
    }

    #[test]
    fn test_atr_increasing_range() {
        // Increasing daily ranges.
        let bars: Vec<DailyCandleEntry> = (0..20)
            .map(|i| {
                let range = (i + 1) as f64;
                make_bar(i, 100.0, 100.0 + range, 100.0 - range, 100.0)
            })
            .collect();
        let atr = compute_atr(&bars, 14);
        assert!(atr > 0.0, "ATR should be positive: {atr}");
    }

    #[test]
    fn test_atr_not_enough_bars() {
        let bars: Vec<DailyCandleEntry> = (0..5)
            .map(|i| make_bar(i, 100.0, 101.0, 99.0, 100.0))
            .collect();
        let atr = compute_atr(&bars, 14);
        assert_eq!(atr, 0.0, "Not enough bars should return 0.0");
    }

    #[test]
    fn test_position_size_with_portfolio() {
        // Manual check: 100k portfolio, 1% risk, price 100, ATR 2.
        // dollar_risk = 1000, position_value = 1000 * (100/2) = 50_000,
        // max_value = 5% * 100_000 = 5_000 → capped at 5_000.
        // shares = 5000 / 100 = 50.
        let bars: Vec<DailyCandleEntry> = (0..20)
            .map(|i| make_bar(i, 100.0, 101.0, 99.0, 100.0))
            .collect();

        // Build a minimal context using rust_decimal.
        use economind_core::model::Symbol;
        use economind_strategy::{Candidate, TradeDirection};
        let sym = Symbol::new("TEST");
        let signal = TimingSignal {
            candidate: Candidate {
                symbol: sym.clone(),
                score: 0.8,
                metadata: Default::default(),
            },
            score: 0.8,
            direction: TradeDirection::Long,
            rationale: "test".to_string(),
        };
        let sizer = AtrSizer {
            risk_per_trade: 0.01,
            max_position_pct: 0.05,
            atr_period: 14,
        };

        let mut bar_map = std::collections::HashMap::new();
        bar_map.insert(sym.clone(), bars);

        let ctx = economind_strategy::StrategyContext {
            universe: vec![sym.clone()],
            bars: bar_map,
            fundamentals: Default::default(),
            macro_data: Default::default(),
            open_positions: Default::default(),
            portfolio_value: Decimal::from(100_000u32),
            available_cash: Decimal::from(100_000u32),
            current_drawdown: Decimal::ZERO,
            regime: None,
            parameters: Default::default(),
        };

        let result = futures::executor::block_on(sizer.size(&signal, &ctx));
        assert_eq!(result.shares, Decimal::try_from(50.0).unwrap());
        assert_eq!(result.notional, Decimal::from(5000u32));
    }
}
