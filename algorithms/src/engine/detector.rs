//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! # Pattern Detection Utilities
//!
//! This module provides a suite of tools for validating chart patterns and identifying
//! critical market events like breakouts. It includes technical indicators tailored
//! for pattern analysis, such as ATR and Volume SMA, as well as logic for sliding
//! window scans and multi-timeframe confluence.
//!
//! ## Key Components:
//! - **Volatility & Volume**: ATR for price range and Volume SMA for liquidity analysis.
//! - **Pattern Validation**: Logic to verify structural components like flagpoles.
//! - **Breakout Detection**: Identifying when price moves beyond a defined base with momentum.
//! - **Target Projection**: Estimating potential price targets based on pattern height.
//! - **Scanning**: Sliding window mechanism to find patterns across a historical dataset.
//! - **Confluence**: Combining detections from multiple timeframes to increase confidence.

use crate::engine::EngineConfig;
use economind_datamodel::model::{CandleEntry, PatternDetection, PatternType};
use std::collections::HashMap;

/// ----------------------------------------
/// ATR Calculation
/// ----------------------------------------
/// Calculates the Average True Range (ATR) over a given period.
/// ATR is used to measure market volatility by decomposing the entire range of an asset price for that period.
pub fn atr(data: &[CandleEntry], period: usize) -> Vec<f32> {
    let mut out = vec![0.0; data.len()];
    for i in period..data.len() {
        let mut tr_sum = 0.0f32;
        for j in i - period + 1..=i {
            let high = data[j].high.to_string().parse::<f32>().unwrap_or(0.0);
            let low = data[j].low.to_string().parse::<f32>().unwrap_or(0.0);
            let prev_close = data[j - 1].close.to_string().parse::<f32>().unwrap_or(0.0);
            let tr = (high - low)
                .max((high - prev_close).abs())
                .max((low - prev_close).abs());
            tr_sum += tr;
        }
        out[i] = tr_sum / period as f32;
    }
    out
}

/// ----------------------------------------
/// Volume SMA
/// ----------------------------------------
/// Calculates the Simple Moving Average of volume over a given period.
/// Used to detect volume contraction or expansion relative to historical averages.
pub fn volume_sma(data: &[CandleEntry], period: usize) -> Vec<f32> {
    let mut out = vec![0.0; data.len()];
    for i in period..data.len() {
        let sum: f32 = data[i - period..i]
            .iter()
            .map(|c| c.volume as f32)
            .sum();
        out[i] = sum / period as f32;
    }
    out
}

/// ----------------------------------------
/// Flagpole Validation
/// ----------------------------------------
/// Validates if a price move qualifies as a "flagpole" for patterns like flags or pennants.
/// A flagpole is defined by a rapid price change over a specific percentage threshold.
pub fn validate_flagpole(data: &[CandleEntry], start: usize, end: usize, min_pct: f32) -> bool {
    let start_price = data[start].close.to_string().parse::<f32>().unwrap_or(0.0);
    let end_price = data[end].close.to_string().parse::<f32>().unwrap_or(0.0);
    if start_price == 0.0 { return false; }
    let pct = (end_price - start_price) / start_price;
    pct.abs() >= min_pct
}

/// ----------------------------------------
/// Breakout Detection
/// ----------------------------------------
/// Scans forward from `end_index` to see if the price breaks out from the `base` price.
/// - `lookahead`: Number of candles to check for a breakout.
/// - `direction`: `1` for bullish (upward) breakout, `-1` for bearish (downward) breakout.
/// Returns the index of the breakout candle if found.
pub fn detect_breakout(
    data: &[CandleEntry],
    end_index: usize,
    lookahead: usize,
    direction: i8,
) -> Option<usize> {
    let base = data[end_index].close.to_string().parse::<f32>().unwrap_or(0.0);

    for i in end_index + 1..=(end_index + lookahead).min(data.len() - 1) {
        let close = data[i].close.to_string().parse::<f32>().unwrap_or(0.0);
        if base == 0.0 { continue; }
        let delta = (close - base) / base;
        if direction > 0 && delta > 0.01 {
            return Some(i);
        }
        if direction < 0 && delta < -0.01 {
            return Some(i);
        }
    }
    None
}

/// ----------------------------------------
/// Target Projection
/// ----------------------------------------
/// Projects a price target based on the pattern height and breakout price.
/// Usually, the target is the breakout price plus/minus the height of the pattern.
pub fn project_target(pattern_height: f32, breakout_price: f32, direction: i8) -> f32 {
    breakout_price + direction as f32 * pattern_height
}

/// ----------------------------------------
/// Sliding Window Pattern Scan
/// ----------------------------------------
/// Performs a sliding window scan over the provided data using a specific `detector` function.
/// The window size is defined in the `EngineConfig`.
/// This approach allows detecting patterns that might appear at any point in history.
pub fn scan_sliding_windows(
    data: &[CandleEntry],
    config: &EngineConfig,
    detector: fn(&[CandleEntry]) -> Vec<PatternDetection>,
) -> Vec<PatternDetection> {
    let mut out = Vec::new();

    for start in 0..data.len().saturating_sub(config.window_size) {
        let window = &data[start..start + config.window_size];
        let patterns = detector(window);

        for p in patterns {
            // In a sliding window, we don't add timestamps,
            // the detector already returns absolute timestamps from the window.
            out.push(p);
        }
    }

    out
}

/// ----------------------------------------
/// Multi-Timeframe Confluence
/// ----------------------------------------
/// Result of multi-timeframe confluence analysis.
#[derive(Debug, Clone)]
pub struct ConfluenceResult {
    /// The pattern type that was found in multiple timeframes.
    pub pattern: PatternType,
    /// List of timeframes where the pattern was detected (e.g., ["1m", "5m"]).
    pub timeframes: Vec<String>,
    /// A boost factor for the confidence score based on the number of concurring timeframes.
    pub confidence_boost: f32,
}

/// Identifies patterns that appear across multiple timeframes at the same time.
///
/// Confluence increases the probability of a pattern's success. This function
/// filters for patterns appearing in at least 2 timeframes and calculates
/// a confidence boost.
pub fn multi_timeframe_confluence(
    tf_patterns: &HashMap<String, Vec<PatternDetection>>,
) -> Vec<ConfluenceResult> {
    let mut map: HashMap<PatternType, Vec<String>> = HashMap::new();

    for (tf, patterns) in tf_patterns {
        for p in patterns {
            map.entry(p.pattern.clone()).or_default().push(tf.clone());
        }
    }

    map.into_iter()
        .filter(|(_, tfs): &(PatternType, Vec<String>)| tfs.len() >= 2)
        .map(|(pattern, tfs): (PatternType, Vec<String>)| ConfluenceResult {
            pattern,
            confidence_boost: 0.15 * tfs.len() as f32,
            timeframes: tfs,
        })
        .collect()
}
