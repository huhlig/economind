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

#![allow(dead_code)]

use crate::engine::EngineConfig;
use economind_core::model::{CandleEntry, PatternDetection, PatternType};
use std::collections::HashMap;

/// ----------------------------------------
/// ATR Calculation
/// ----------------------------------------
/// Calculates the Average True Range (ATR) over a given period.
/// ATR is used to measure market volatility by decomposing the entire range of an asset price for that period.
pub fn atr(data: &[CandleEntry], period: usize) -> Vec<f32> {
    let mut out = vec![0.0; data.len()];
    #[allow(clippy::needless_range_loop)]
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
    #[allow(clippy::needless_range_loop)]
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
///
/// Returns the index of the breakout candle if found.
pub fn detect_breakout(
    data: &[CandleEntry],
    end_index: usize,
    lookahead: usize,
    direction: i8,
) -> Option<usize> {
    let base = data[end_index].close.to_string().parse::<f32>().unwrap_or(0.0);

    #[allow(clippy::needless_range_loop)]
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

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use economind_core::model::{CandleEntry, PatternDetection, PatternType};
    use chrono::NaiveDateTime;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal { Decimal::from_str(s).unwrap() }

    fn ts(secs: i64) -> NaiveDateTime {
        NaiveDateTime::from_timestamp_opt(86400 * secs, 0).unwrap()
    }

    fn candle(secs: i64, high: &str, low: &str, close: &str) -> CandleEntry {
        CandleEntry {
            timestamp: ts(secs),
            open: dec(close),
            high: dec(high),
            low: dec(low),
            close: dec(close),
            volume: 10_000,
        }
    }

    /// Flat series: every bar at exactly `price` with tiny spread.
    fn flat_series(n: usize, price: f64) -> Vec<CandleEntry> {
        (0..n as i64).map(|i| {
            let p = format!("{:.2}", price);
            let hi = format!("{:.2}", price + 0.01);
            let lo = format!("{:.2}", price - 0.01);
            CandleEntry {
                timestamp: ts(i),
                open:  dec(&p),
                high:  dec(&hi),
                low:   dec(&lo),
                close: dec(&p),
                volume: 5_000,
            }
        }).collect()
    }

    /// Rising series: close = base + i, high = close + 0.5, low = close - 0.5.
    fn rising_series(n: usize, base: f64) -> Vec<CandleEntry> {
        (0..n as i64).map(|i| {
            let c = base + i as f64;
            candle(i, &format!("{:.2}", c + 0.5), &format!("{:.2}", c - 0.5), &format!("{:.2}", c))
        }).collect()
    }

    // ── atr ───────────────────────────────────────────────────────────────────

    #[test]
    fn atr_output_length_equals_input() {
        let data = rising_series(30, 50.0);
        assert_eq!(atr(&data, 14).len(), 30);
    }

    #[test]
    fn atr_zero_before_window_fills() {
        let data = rising_series(30, 50.0);
        let result = atr(&data, 14);
        for i in 0..14 {
            assert_eq!(result[i], 0.0, "atr[{i}] should be 0 before window fills");
        }
    }

    #[test]
    fn atr_positive_once_window_fills() {
        let data = rising_series(30, 50.0);
        let result = atr(&data, 14);
        assert!(result[14] > 0.0, "ATR should be positive after window fills");
    }

    #[test]
    fn atr_small_for_flat_series() {
        let data = flat_series(30, 100.0);
        let result = atr(&data, 14);
        for i in 14..30 {
            assert!(result[i] < 0.1, "atr[{i}]={} for flat series", result[i]);
        }
    }

    #[test]
    fn atr_larger_for_volatile_series() {
        // Volatile: high=close+5, low=close-5
        let data: Vec<CandleEntry> = (0..30i64)
            .map(|i| candle(i, "105.00", "95.00", "100.00"))
            .collect();
        let result = atr(&data, 14);
        // ATR should reflect the 10-point daily range
        assert!(result[14] > 5.0, "ATR={} should reflect high volatility", result[14]);
    }

    // ── volume_sma ────────────────────────────────────────────────────────────

    #[test]
    fn volume_sma_output_length_equals_input() {
        let data = rising_series(30, 50.0);
        assert_eq!(volume_sma(&data, 10).len(), 30);
    }

    #[test]
    fn volume_sma_correct_for_uniform_volume() {
        // All bars have volume 10_000 → SMA should be 10_000 after window fills
        let data = rising_series(30, 50.0);
        let result = volume_sma(&data, 10);
        for i in 10..30 {
            assert!((result[i] - 10_000.0).abs() < 1.0, "volume_sma[{i}]={}", result[i]);
        }
    }

    #[test]
    fn volume_sma_zero_before_window_fills() {
        let data = rising_series(30, 50.0);
        let result = volume_sma(&data, 10);
        for i in 0..10 {
            assert_eq!(result[i], 0.0, "volume_sma[{i}] should be 0 before window fills");
        }
    }

    // ── validate_flagpole ─────────────────────────────────────────────────────

    #[test]
    fn flagpole_valid_when_move_exceeds_threshold() {
        // close[0]=100, close[4]=110 → 10% move, threshold 5% → true
        let mut data = rising_series(5, 100.0);
        data[4].close = dec("110.00");
        assert!(validate_flagpole(&data, 0, 4, 0.05));
    }

    #[test]
    fn flagpole_invalid_when_move_below_threshold() {
        // rising_series: 100→104 over 5 bars = 4% < 10% threshold → false
        let data = rising_series(5, 100.0);
        assert!(!validate_flagpole(&data, 0, 4, 0.10));
    }

    #[test]
    fn flagpole_invalid_when_start_price_is_zero() {
        let data = flat_series(5, 0.0);
        assert!(!validate_flagpole(&data, 0, 4, 0.01));
    }

    #[test]
    fn flagpole_works_for_bearish_move() {
        // Price drops from 100 to 88 → -12% absolute → meets 0.10 threshold
        let mut data = flat_series(5, 100.0);
        data[4].close = dec("88.00");
        assert!(validate_flagpole(&data, 0, 4, 0.10));
    }

    // ── detect_breakout ───────────────────────────────────────────────────────

    #[test]
    fn breakout_bullish_detected() {
        // base=100, next bar closes at 102 → +2% > 1% threshold
        let data = vec![
            candle(0, "101", "99", "100"),
            candle(1, "103", "101", "102"),
        ];
        assert_eq!(detect_breakout(&data, 0, 5, 1), Some(1));
    }

    #[test]
    fn breakout_bearish_detected() {
        let data = vec![
            candle(0, "101", "99", "100"),
            candle(1, "99", "97", "98"),   // -2% → bearish breakout
        ];
        assert_eq!(detect_breakout(&data, 0, 5, -1), Some(1));
    }

    #[test]
    fn breakout_none_when_flat() {
        let data = flat_series(10, 100.0);
        assert!(detect_breakout(&data, 0, 8, 1).is_none());
        assert!(detect_breakout(&data, 0, 8, -1).is_none());
    }

    #[test]
    fn breakout_respects_lookahead_limit() {
        // Breakout happens at index 5, but lookahead=3 from index 0 → should not find it
        let mut data = flat_series(10, 100.0);
        data[5].close = dec("102.00"); // +2% at index 5
        assert!(detect_breakout(&data, 0, 3, 1).is_none());
    }

    // ── project_target ────────────────────────────────────────────────────────

    #[test]
    fn project_target_bullish_adds_height() {
        assert!((project_target(5.0, 100.0, 1) - 105.0).abs() < 1e-5);
    }

    #[test]
    fn project_target_bearish_subtracts_height() {
        assert!((project_target(5.0, 100.0, -1) - 95.0).abs() < 1e-5);
    }

    #[test]
    fn project_target_zero_height() {
        assert!((project_target(0.0, 50.0, 1) - 50.0).abs() < 1e-5);
    }

    // ── multi_timeframe_confluence ────────────────────────────────────────────

    fn make_pattern(pattern: PatternType) -> PatternDetection {
        let t = NaiveDateTime::from_timestamp_opt(0, 0).unwrap();
        PatternDetection { pattern, start_time: t, apex_time: t, end_time: t, confidence: 0.8 }
    }

    #[test]
    fn confluence_empty_input() {
        let result = multi_timeframe_confluence(&HashMap::new());
        assert!(result.is_empty());
    }

    #[test]
    fn confluence_single_timeframe_no_result() {
        let mut map = HashMap::new();
        map.insert("1d".to_string(), vec![make_pattern(PatternType::DoubleBottom)]);
        assert!(multi_timeframe_confluence(&map).is_empty());
    }

    #[test]
    fn confluence_two_timeframes_detected() {
        let mut map = HashMap::new();
        map.insert("1d".to_string(), vec![make_pattern(PatternType::HeadAndShoulders)]);
        map.insert("1w".to_string(), vec![make_pattern(PatternType::HeadAndShoulders)]);
        let results = multi_timeframe_confluence(&map);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].timeframes.len(), 2);
        assert!((results[0].confidence_boost - 0.30).abs() < 1e-5,
            "confidence_boost={}", results[0].confidence_boost);
    }

    #[test]
    fn confluence_three_timeframes_higher_boost() {
        let mut map = HashMap::new();
        for tf in ["1d", "1w", "1m"] {
            map.insert(tf.to_string(), vec![make_pattern(PatternType::DoubleTop)]);
        }
        let results = multi_timeframe_confluence(&map);
        assert_eq!(results.len(), 1);
        assert!((results[0].confidence_boost - 0.45).abs() < 1e-5,
            "3 TF boost should be 0.45, got {}", results[0].confidence_boost);
    }

    #[test]
    fn confluence_different_patterns_no_cross_match() {
        let mut map = HashMap::new();
        map.insert("1d".to_string(), vec![make_pattern(PatternType::DoubleBottom)]);
        map.insert("1w".to_string(), vec![make_pattern(PatternType::DoubleTop)]);
        // Two different patterns, each appearing in only 1 TF → no confluence
        assert!(multi_timeframe_confluence(&map).is_empty());
    }
}
