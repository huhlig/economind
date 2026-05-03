//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use crate::model::{Exchange, Industry, Sector, Symbol};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ticker {
    pub symbol: Symbol,
    pub exchange: Option<Exchange>,
    pub name: Option<String>,
    pub country: Option<String>,
    /// Industry classification. `None` until Industry enum variants are populated.
    pub industry: Option<Industry>,
    /// Sector classification. `None` until Sector enum variants are populated.
    pub sector: Option<Sector>,
    /// IPO year as a string (e.g. "2001").
    pub ipoyear: Option<String>,
    pub marketcap: Option<Decimal>,
    pub description: Option<String>,
    /// False when the instrument is delisted or removed from the universe.
    pub active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TickerProperty {
    pub symbol: Symbol,
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TickerStatistics {
    /// Ticker Symbol
    pub symbol: Symbol,
    /// Date of statistics
    pub date: NaiveDate,
    /// Closing Price on date
    pub last_price: Decimal,
    /// Closing Volume on date
    pub last_volume: Decimal,
    /// 7-day Price Average
    pub avg_price: Decimal,
    /// 7-day Price Delta
    pub avg_net_change: Decimal,
    /// 7-day Price Delta
    pub avg_pct_change: Decimal,
    /// 7-day Volume Average
    pub avg_volume: Decimal,
}
