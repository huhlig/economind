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
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub name: String,
    pub country: String,
    pub industry: Industry,
    pub sector: Sector,
    pub ipoyear: String,
    pub marketcap: Decimal,
    pub description: String,
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
