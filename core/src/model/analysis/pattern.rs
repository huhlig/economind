//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use chrono::{NaiveDateTime};

/// Pattern Detection
///
/// Start Time         End Time        Apex Time
///     |------------------|---------------X
///     ^                  ^               ^
///     Pattern begins     Pattern valid   Trendlines intersect
#[derive(Debug, Clone)]
pub struct PatternDetection {
    /// Pattern Type Detected
    pub pattern: PatternType,
    /// Start Time
    ///
    /// Moment when the pattern begins forming
    /// * The moment the pattern begins forming:
    /// * First pivot that participates in the pattern
    /// * Start of compression / consolidation
    ///
    /// Examples:
    /// * First higher low in an ascending triangle
    /// * First lower high in a wedge
    /// * First shoulder in H&S
    pub start_time: NaiveDateTime,
    /// Apex Time
    /// The theoretical convergence point of the pattern’s trendlines. The timestamp where the
    /// support and resistance trendlines would intersect if extended forward.
    ///
    /// Key properties:
    /// * Often future relative to the last candle
    /// * Represents maximum compression
    /// * Breakouts statistically occur before the apex (≈ 60–90%)
    ///
    /// So apex time is:
    /// * A prediction boundary
    /// * A deadline for pattern validity
    /// * A time-based risk metric
    pub apex_time: NaiveDateTime,
    /// End Time
    /// The last candle used to identify the pattern so far.
    /// * The most recent pivot or bar that confirms the geometry
    /// * Not a breakout
    /// * Not necessarily the apex
    pub end_time: NaiveDateTime,
    /// Confidence Score
    pub confidence: f64,
}

/// Type of chart pattern detected.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PatternType {
    /// Ascending Triangle
    ///
    /// A bullish continuation pattern characterized by a flat resistance line
    /// and an ascending support line.
    AscendingTriangle,
    /// Descending Triangle
    ///
    /// A bearish continuation pattern characterized by a flat support line
    /// and a descending resistance line.
    DescendingTriangle,
    /// Bullish Symmetrical Triangle
    ///
    /// A continuation pattern where both resistance and support trendlines
    /// converge towards each other, typically breaking out in the direction
    /// of the prior bullish trend.
    BullishSymTriangle,
    /// Bearish Symmetrical Triangle
    ///
    /// A continuation pattern where both resistance and support trendlines
    /// converge towards each other, typically breaking out in the direction
    /// of the prior bearish trend.
    BearishSymTriangle,
    /// Rising Wedge
    ///
    /// A bearish reversal pattern characterized by two upward sloping but
    /// converging trendlines, where the support line is steeper than the
    /// resistance line.
    RisingWedge,
    /// Falling Wedge
    ///
    /// A bullish reversal pattern characterized by two downward sloping but
    /// converging trendlines, where the resistance line is steeper than the
    /// support line.
    FallingWedge,
    /// Bullish Flag
    ///
    /// A short-term bullish continuation pattern that looks like a small
    /// downward sloping rectangle following a sharp upward move.
    BullishFlag,
    /// Bearish Flag
    ///
    /// A short-term bearish continuation pattern that looks like a small
    /// upward sloping rectangle following a sharp downward move.
    BearishFlag,
    /// Double Bottom
    ///
    /// A bullish reversal pattern that occurs after an extended downtrend,
    /// characterized by two consecutive lows at roughly the same price level.
    DoubleBottom,
    /// Triple Bottom
    ///
    /// A bullish reversal pattern similar to a double bottom but with three
    /// consecutive lows at roughly the same price level.
    TripleBottom,
    /// Double Top
    ///
    /// A bearish reversal pattern that occurs after an extended uptrend,
    /// characterized by two consecutive highs at roughly the same price level.
    DoubleTop,
    /// Triple Top
    ///
    /// A bearish reversal pattern similar to a double top but with three
    /// consecutive highs at roughly the same price level.
    TripleTop,
    /// Head and Shoulders
    ///
    /// A bearish reversal pattern characterized by a peak (left shoulder),
    /// followed by a higher peak (head), and then another lower peak (right shoulder).
    HeadAndShoulders,
    /// Inverted Head and Shoulders
    ///
    /// A bullish reversal pattern characterized by a trough (left shoulder),
    /// followed by a lower trough (head), and then another higher trough (right shoulder).
    InvertedHeadAndShoulders,
}
