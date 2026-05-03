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
