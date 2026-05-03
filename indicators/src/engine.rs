//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//


//! Advanced Pattern Engine
//!
//! Adds:
//! - Sliding window detection
//! - Flagpole validation
//! - ATR & volume contraction
//! - Breakout confirmation
//! - Breakout prediction & targets
//! - Backtesting & success statistics
//! - Multi-timeframe confluence

mod pattern;
mod detector;
mod testing;

/// ----------------------------------------
/// Configuration
/// ----------------------------------------
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub window_size: usize,
    pub atr_period: usize,
    pub volume_period: usize,
    pub breakout_lookahead: usize,
    pub min_flagpole_pct: f64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            window_size: 120,
            atr_period: 14,
            volume_period: 20,
            breakout_lookahead: 20,
            min_flagpole_pct: 0.03,
        }
    }
}
