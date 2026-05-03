//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! StrategyContext — the read-only snapshot passed to every strategy call.

use economind_core::model::{DailyCandleEntry, FactorSnapshot, Symbol};
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Read-only market snapshot passed to every Identifier, Timer, and Sizer call.
///
/// All data is pre-loaded from the DataStore before a strategy run begins.
/// Strategies must not perform I/O; they read from this context only.
#[derive(Debug, Clone)]
pub struct StrategyContext {
    /// The universe of instruments this run considers.
    pub universe: Vec<Symbol>,

    /// Daily bar history keyed by symbol. Inner vec is sorted oldest → newest.
    pub bars: HashMap<Symbol, Vec<DailyCandleEntry>>,

    /// Latest fundamental factor snapshot per instrument, if available.
    pub fundamentals: HashMap<Symbol, FactorSnapshot>,

    /// Latest values of tracked macro series (FRED series ID → value).
    pub macro_data: HashMap<String, Decimal>,

    /// Current open positions: symbol → shares held (negative = short).
    pub open_positions: HashMap<Symbol, Decimal>,

    /// Total portfolio value (cash + mark-to-market of open positions).
    pub portfolio_value: Decimal,

    /// Available cash for new positions.
    pub available_cash: Decimal,

    /// Current drawdown from peak portfolio value (0.0 – 1.0).
    pub current_drawdown: Decimal,

    /// Regime label from the regime classifier, if configured (e.g. "trending-up").
    pub regime: Option<String>,

    /// Strategy-specific parameters (key → value strings, parsed by each plugin).
    pub parameters: HashMap<String, String>,
}
