//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! Comprehensive Chart Pattern Detection Engine
//!
//! Patterns Supported:
//! CONTINUATION
//! - Ascending Triangle (Bullish)
//! - Descending Triangle (Bearish)
//! - Bullish / Bearish Symmetrical Triangle
//! - Rising Wedge (Bearish)
//! - Falling Wedge (Bullish)
//! - Bullish / Bearish Flag
//!
//! REVERSALS
//! - Double / Triple Bottom
//! - Double / Triple Top
//! - Head & Shoulders
//! - Inverted Head & Shoulders

use economind_datamodel::model::{
    CandleEntry,
    analysis::{PatternDetection, PatternType, Pivot, PivotType, Trendline},
};
use rust_decimal::prelude::ToPrimitive;
use std::f64;

/// ------------------------------------------------
/// Main Pattern Scanner
/// ------------------------------------------------
pub fn scan_patterns(data: &[CandleEntry]) -> Vec<PatternDetection> {
    let pivots = detect_pivots(data, 5);
    let highs: Vec<_> = pivots
        .iter()
        .filter(|p| p.pivot_type == PivotType::High)
        .cloned()
        .collect();
    let lows: Vec<_> = pivots
        .iter()
        .filter(|p| p.pivot_type == PivotType::Low)
        .cloned()
        .collect();

    let mut patterns = Vec::new();

    // ---------- Triangles ----------
    if let (Some(r), Some(s)) = (fit_trendline(&highs), fit_trendline(&lows)) {
        let apex = if (r.slope - s.slope).abs() > 1e-6 {
            let x = (s.intercept - r.intercept) / (r.slope - s.slope);
            // We use the time increment from data if available, or just index
            // Assuming data[1].timestamp() - data[0].timestamp() is the interval
            if data.len() >= 2 {
                let t0 = data[0].timestamp.clone();
                let t1 = data[1].timestamp.clone();
                let dt = (t1.timestamp() - t0.timestamp()) as f64;
                chrono::NaiveDateTime::from_timestamp_opt(
                    (t0.timestamp() as f64 + x * dt) as i64,
                    0,
                )
                .unwrap_or_else(|| highs.last().unwrap().timestamp.clone())
            } else {
                highs.last().unwrap().timestamp.clone()
            }
        } else {
            highs.last().unwrap().timestamp.clone()
        };

        if flat(r.slope) && s.slope > 0.0 {
            patterns.push(PatternDetection {
                pattern: PatternType::AscendingTriangle,
                start_time: lows.first().unwrap().timestamp.clone(),
                apex_time: apex.clone(),
                end_time: highs.last().unwrap().timestamp.clone(),
                confidence: confidence(&r, &s),
            });
        }

        if flat(s.slope) && r.slope < 0.0 {
            patterns.push(PatternDetection {
                pattern: PatternType::DescendingTriangle,
                start_time: highs.first().unwrap().timestamp.clone(),
                apex_time: apex.clone(),
                end_time: lows.last().unwrap().timestamp.clone(),
                confidence: confidence(&r, &s),
            });
        }

        if r.slope < 0.0 && s.slope > 0.0 {
            patterns.push(PatternDetection {
                pattern: PatternType::BullishSymTriangle,
                start_time: lows.first().unwrap().timestamp.clone(),
                apex_time: apex.clone(),
                end_time: highs.last().unwrap().timestamp.clone(),
                confidence: confidence(&r, &s),
            });
        }

        if r.slope > 0.0 && s.slope < 0.0 {
            patterns.push(PatternDetection {
                pattern: PatternType::BearishSymTriangle,
                start_time: highs.first().unwrap().timestamp.clone(),
                apex_time: apex.clone(),
                end_time: lows.last().unwrap().timestamp.clone(),
                confidence: confidence(&r, &s),
            });
        }

        // ---------- Wedges ----------
        if r.slope > 0.0 && s.slope > 0.0 && s.slope > r.slope {
            patterns.push(PatternDetection {
                pattern: PatternType::RisingWedge,
                start_time: lows.first().unwrap().timestamp.clone(),
                apex_time: apex.clone(),
                end_time: highs.last().unwrap().timestamp.clone(),
                confidence: confidence(&r, &s),
            });
        }

        if r.slope < 0.0 && s.slope < 0.0 && s.slope > r.slope {
            patterns.push(PatternDetection {
                pattern: PatternType::FallingWedge,
                start_time: highs.first().unwrap().timestamp.clone(),
                apex_time: apex,
                end_time: lows.last().unwrap().timestamp.clone(),
                confidence: confidence(&r, &s),
            });
        }
    }

    // ---------- Double / Triple Bottom ----------
    if lows.len() >= 3 {
        let l1 = &lows[lows.len() - 3];
        let l2 = &lows[lows.len() - 2];
        let l3 = &lows[lows.len() - 1];

        if approx_equal(l1.price, l2.price, 0.02) {
            patterns.push(PatternDetection {
                pattern: PatternType::DoubleBottom,
                start_time: l1.timestamp.clone(),
                apex_time: l2.timestamp.clone(),
                end_time: l2.timestamp.clone(),
                confidence: 0.75,
            });
        }

        if approx_equal(l1.price, l2.price, 0.02) && approx_equal(l2.price, l3.price, 0.02) {
            patterns.push(PatternDetection {
                pattern: PatternType::TripleBottom,
                start_time: l1.timestamp.clone(),
                apex_time: l3.timestamp.clone(),
                end_time: l3.timestamp.clone(),
                confidence: 0.85,
            });
        }
    }

    // ---------- Double / Triple Top ----------
    if highs.len() >= 3 {
        let h1 = &highs[highs.len() - 3];
        let h2 = &highs[highs.len() - 2];
        let h3 = &highs[highs.len() - 1];

        if approx_equal(h1.price, h2.price, 0.02) {
            patterns.push(PatternDetection {
                pattern: PatternType::DoubleTop,
                start_time: h1.timestamp.clone(),
                apex_time: h2.timestamp.clone(),
                end_time: h2.timestamp.clone(),
                confidence: 0.75,
            });
        }

        if approx_equal(h1.price, h2.price, 0.02) && approx_equal(h2.price, h3.price, 0.02) {
            patterns.push(PatternDetection {
                pattern: PatternType::TripleTop,
                start_time: h1.timestamp.clone(),
                apex_time: h3.timestamp.clone(),
                end_time: h3.timestamp.clone(),
                confidence: 0.85,
            });
        }
    }

    // ---------- Head & Shoulders ----------
    if highs.len() >= 3 {
        let a = &highs[highs.len() - 3];
        let b = &highs[highs.len() - 2];
        let c = &highs[highs.len() - 1];

        if b.price > a.price && b.price > c.price && approx_equal(a.price, c.price, 0.03) {
            patterns.push(PatternDetection {
                pattern: PatternType::HeadAndShoulders,
                start_time: a.timestamp.clone(),
                apex_time: c.timestamp.clone(),
                end_time: c.timestamp.clone(),
                confidence: 0.9,
            });
        }
    }

    // ---------- Inverted Head & Shoulders ----------
    if lows.len() >= 3 {
        let a = &lows[lows.len() - 3];
        let b = &lows[lows.len() - 2];
        let c = &lows[lows.len() - 1];

        if b.price < a.price && b.price < c.price && approx_equal(a.price, c.price, 0.03) {
            patterns.push(PatternDetection {
                pattern: PatternType::InvertedHeadAndShoulders,
                start_time: a.timestamp.clone(),
                apex_time: c.timestamp.clone(),
                end_time: c.timestamp.clone(),
                confidence: 0.9,
            });
        }
    }

    patterns
}

/// ------------------------------------------------
/// Pivot Detection
/// ------------------------------------------------
pub fn detect_pivots(data: &[CandleEntry], lookback: usize) -> Vec<Pivot> {
    let mut pivots = Vec::new();

    for i in lookback..data.len() - lookback {
        let high = data[i].high;
        let low = data[i].low;

        let is_high = (i - lookback..i)
            .chain(i + 1..=i + lookback)
            .all(|j| data[j].high < high);

        let is_low = (i - lookback..i)
            .chain(i + 1..=i + lookback)
            .all(|j| data[j].low > low);

        if is_high {
            pivots.push(Pivot {
                index: i,
                timestamp: data[i].timestamp.clone(),
                price: high.to_f64().unwrap_or(f64::NAN),
                pivot_type: PivotType::High,
            });
        }

        if is_low {
            pivots.push(Pivot {
                index: i,
                timestamp: data[i].timestamp.clone(),
                price: low.to_f64().unwrap_or(f64::NAN),
                pivot_type: PivotType::Low,
            });
        }
    }
    pivots
}

/// ------------------------------------------------
/// Trendline Fit
/// ------------------------------------------------
pub fn fit_trendline(pivots: &[Pivot]) -> Option<Trendline> {
    if pivots.len() < 2 {
        return None;
    }

    let n = pivots.len() as f64;
    let (sx, sy, sxy, sx2) = pivots.iter().fold((0.0, 0.0, 0.0, 0.0), |acc, p| {
        let x = p.index as f64;
        (
            acc.0 + x,
            acc.1 + p.price,
            acc.2 + x * p.price,
            acc.3 + x * x,
        )
    });

    let denom = n * sx2 - sx * sx;
    if denom.abs() < 1e-6 {
        return None;
    }

    let slope = (n * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / n;

    let error = pivots
        .iter()
        .map(|p| (p.price - (slope * p.index as f64 + intercept)).abs())
        .sum::<f64>()
        / n;

    Some(Trendline {
        slope,
        intercept,
        touches: pivots.len(),
        error,
    })
}

/// ------------------------------------------------
/// Triangle & Wedge Detection
/// ------------------------------------------------
fn flat(slope: f64) -> bool {
    slope.abs() < 0.0005
}

fn confidence(a: &Trendline, b: &Trendline) -> f64 {
    let touch_score = (a.touches + b.touches) as f64 / 10.0;
    let precision = 1.0 / (1.0 + a.error + b.error);
    (0.6f64 * touch_score + 0.4f64 * precision).clamp(0.0f64, 1.0f64)
}

/// ------------------------------------------------
/// Reversal Pattern Helpers
/// ------------------------------------------------
fn approx_equal(a: f64, b: f64, tolerance: f64) -> bool {
    ((a - b) / b).abs() < tolerance
}
