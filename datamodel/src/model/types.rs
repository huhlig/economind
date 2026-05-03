//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Exchange(String);

impl Exchange {
    pub fn new(name: &str) -> Self {
        Exchange(name.to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Symbol(String);

impl Symbol {
    pub fn new(symbol: &str) -> Self {
        Symbol(symbol.to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Industry {
    // TODO: List Industries
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Sector {
    // TODO: List Sectors
}

/// Ticker Interval
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Interval {
    OneMinute,
    FiveMinute,
    FifteenMinute,
    OneHour,
    OneDay,
}

impl Interval {
    pub fn as_str(&self) -> &'static str {
        match self {
            Interval::OneMinute => "1m",
            Interval::FiveMinute => "5m",
            Interval::FifteenMinute => "15m",
            Interval::OneHour => "1h",
            Interval::OneDay => "daily",
        }
    }
}
