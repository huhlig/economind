//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! # Pattern Testing & Validation
//!
//! This module provides tools for backtesting detected patterns against historical data.
//! It evaluates the predictive power of patterns by checking for breakouts in the
//! expected direction following the pattern completion.
//!
//! ## Logic:
//! 1. **Alignment**: Maps detected patterns to the corresponding points in the historical dataset.
//! 2. **Evaluation**: For each pattern, it looks ahead a specific number of candles (defined in `EngineConfig`)
//!    to see if a price breakout occurs in the predicted direction.
//! 3. **Statistics**: Aggregates results by `PatternType` to provide success rates (win/loss ratios).

#![allow(dead_code)]

use crate::engine::EngineConfig;
use crate::engine::detector::detect_breakout;
use economind_core::model::{CandleEntry, PatternDetection, PatternType};
use std::collections::HashMap;

/// ----------------------------------------
/// Backtesting
/// ----------------------------------------
/// Statistics for backtesting results.
#[derive(Debug, Clone)]
pub struct BacktestStats {
    /// Total number of patterns tested.
    pub total: usize,
    /// Number of patterns that resulted in a successful breakout.
    pub wins: usize,
    /// Number of patterns that did not result in a breakout within the lookahead period.
    pub losses: usize,
    /// Percentage of successful breakouts (wins / total).
    pub win_rate: f64,
}

/// Backtests a set of detected patterns against historical data.
///
/// For each pattern, it determines the expected breakout direction based on the `PatternType`
/// and then uses `detect_breakout` to see if the price moved as predicted within the
/// configured lookahead window.
pub fn backtest_patterns(
    data: &[CandleEntry],
    patterns: &[PatternDetection],
    config: &EngineConfig,
) -> HashMap<PatternType, BacktestStats> {
    let mut stats: HashMap<PatternType, BacktestStats> = HashMap::new();

    for p in patterns {
        let entry_index = data
            .iter()
            .position(|c| c.timestamp == p.end_time)
            .unwrap_or(0);
        let dir = match p.pattern {
            PatternType::DescendingTriangle
            | PatternType::BearishFlag
            | PatternType::BearishSymTriangle
            | PatternType::HeadAndShoulders
            | PatternType::DoubleTop
            | PatternType::TripleTop
            | PatternType::RisingWedge => -1,
            _ => 1,
        };

        let breakout = detect_breakout(data, entry_index, config.breakout_lookahead, dir);
        let win = breakout.is_some();

        let entry = stats.entry(p.pattern.clone()).or_insert(BacktestStats {
            total: 0,
            wins: 0,
            losses: 0,
            win_rate: 0.0,
        });

        entry.total += 1;
        if win {
            entry.wins += 1;
        } else {
            entry.losses += 1;
        }
        entry.win_rate = entry.wins as f64 / entry.total as f64;
    }

    stats
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineConfig;
    use economind_core::model::{CandleEntry, PatternDetection, PatternType};
    use chrono::NaiveDateTime;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal { Decimal::from_str(s).unwrap() }

    fn ts(secs: i64) -> NaiveDateTime {
        NaiveDateTime::from_timestamp_opt(86400 * secs, 0).unwrap()
    }

    fn candle(secs: i64, close: &str) -> CandleEntry {
        CandleEntry {
            timestamp: ts(secs),
            open: dec(close),
            high: dec(close),
            low: dec(close),
            close: dec(close),
            volume: 1_000,
        }
    }

    fn pattern_at(pattern: PatternType, end_secs: i64, bullish: bool) -> PatternDetection {
        let t = ts(end_secs);
        PatternDetection {
            pattern,
            start_time: t,
            apex_time: t,
            end_time: t,
            confidence: if bullish { 0.8 } else { 0.7 },
        }
    }

    #[test]
    fn backtest_empty_patterns_returns_empty_stats() {
        let data: Vec<CandleEntry> = (0..10).map(|i| candle(i, "100")).collect();
        let config = EngineConfig::default();
        let stats = backtest_patterns(&data, &[], &config);
        assert!(stats.is_empty());
    }

    #[test]
    fn backtest_bullish_pattern_with_subsequent_breakout_is_win() {
        // 5 flat bars at 100, then bar 5 closes at 103 (+3%) → bullish breakout
        let mut data: Vec<CandleEntry> = (0..10).map(|i| candle(i, "100")).collect();
        data[5].close = dec("103.00"); // +3% → breakout
        // Pattern ends at bar 0; lookahead looks at bars 1..=5
        let pattern = pattern_at(PatternType::AscendingTriangle, 0, true);
        let config = EngineConfig { breakout_lookahead: 8, ..Default::default() };
        let stats = backtest_patterns(&data, &[pattern], &config);
        let s = &stats[&PatternType::AscendingTriangle];
        assert_eq!(s.total, 1);
        assert_eq!(s.wins, 1);
        assert_eq!(s.losses, 0);
        assert!((s.win_rate - 1.0).abs() < 1e-9);
    }

    #[test]
    fn backtest_bearish_pattern_with_drop_is_win() {
        let mut data: Vec<CandleEntry> = (0..10).map(|i| candle(i, "100")).collect();
        data[3].close = dec("98.00"); // -2% → bearish breakout
        let pattern = pattern_at(PatternType::HeadAndShoulders, 0, false);
        let config = EngineConfig { breakout_lookahead: 8, ..Default::default() };
        let stats = backtest_patterns(&data, &[pattern], &config);
        let s = &stats[&PatternType::HeadAndShoulders];
        assert_eq!(s.wins, 1);
        assert!((s.win_rate - 1.0).abs() < 1e-9);
    }

    #[test]
    fn backtest_no_breakout_is_loss() {
        // Flat data — no 1% move ever
        let data: Vec<CandleEntry> = (0..10).map(|i| candle(i, "100")).collect();
        let pattern = pattern_at(PatternType::DoubleBottom, 0, true);
        let config = EngineConfig { breakout_lookahead: 8, ..Default::default() };
        let stats = backtest_patterns(&data, &[pattern], &config);
        let s = &stats[&PatternType::DoubleBottom];
        assert_eq!(s.wins, 0);
        assert_eq!(s.losses, 1);
        assert!((s.win_rate - 0.0).abs() < 1e-9);
    }

    #[test]
    fn backtest_win_rate_aggregates_correctly() {
        // Two of same pattern: one win (breakout at bar 3), one loss (flat through bar 0)
        // We'll use two patterns ending at different indices
        let mut data: Vec<CandleEntry> = (0..15).map(|i| candle(i, "100")).collect();
        data[3].close = dec("102.00");  // +2% → win for pattern ending at bar 0
        // Pattern ending at bar 8 — bars 9..14 are flat → loss
        let p_win  = pattern_at(PatternType::FallingWedge, 0, true);
        let p_loss = pattern_at(PatternType::FallingWedge, 8, true);
        let config = EngineConfig { breakout_lookahead: 5, ..Default::default() };
        let stats = backtest_patterns(&data, &[p_win, p_loss], &config);
        let s = &stats[&PatternType::FallingWedge];
        assert_eq!(s.total, 2);
        assert_eq!(s.wins, 1);
        assert_eq!(s.losses, 1);
        assert!((s.win_rate - 0.5).abs() < 1e-9);
    }
}
