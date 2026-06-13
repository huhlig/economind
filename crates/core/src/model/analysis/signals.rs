//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use crate::model::types::Symbol;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct FactorSnapshot {
    pub id: Uuid,
    pub symbol: Symbol,
    pub as_of: NaiveDate,

    // Growth
    pub revenue_cagr_5y: Option<Decimal>,
    pub eps_cagr_5y: Option<Decimal>,

    // Profitability / Quality
    pub gross_margin: Option<Decimal>,
    pub operating_margin: Option<Decimal>,
    pub net_margin: Option<Decimal>,
    pub roe: Option<Decimal>,
    pub roic: Option<Decimal>,
    pub fcf_margin: Option<Decimal>,

    // Valuation
    pub pe_ratio: Option<Decimal>,
    pub ev_ebitda: Option<Decimal>,
    pub peg_ratio: Option<Decimal>,

    // Risk
    pub beta_1y: Option<Decimal>,
    pub volatility_1y: Option<Decimal>,
    pub sharpe_1y: Option<Decimal>,
    pub max_drawdown_1y: Option<Decimal>,

    // Momentum
    pub momentum_12m: Option<Decimal>,
    pub relative_strength_12m: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct RollingReturn {
    pub symbol: Symbol,
    pub window_days: u32,
    pub as_of: NaiveDate,
    pub return_pct: Decimal,
}

#[derive(Debug, Clone)]
pub struct RollingVolatility {
    pub symbol: Symbol,
    pub window_days: u32,
    pub as_of: NaiveDate,
    pub annualized_vol: Decimal,
}

#[derive(Debug, Clone)]
pub struct RollingSharpe {
    pub symbol: Symbol,
    pub window_days: u32,
    pub as_of: NaiveDate,
    pub sharpe: Decimal,
}

#[derive(Debug, Clone)]
pub struct BenchmarkReturn {
    pub benchmark: String,
    pub timestamp: DateTime<Utc>,
    pub return_pct: Decimal,
}

#[derive(Debug, Clone)]
pub struct RiskFreeRate {
    pub date: NaiveDate,
    pub annual_rate: Decimal,
}
