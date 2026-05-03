//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use crate::model::Interval;
use crate::model::types::Symbol;
use chrono::{ NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DailyCandleSeries {
    pub symbol: Symbol,
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: u64,
    pub entries: Vec<DailyCandleEntry>,
}

impl DailyCandleSeries {
    pub fn from_iter(
        symbol: Symbol,
        entries: impl IntoIterator<Item = DailyCandleEntry>,
    ) -> Option<Self> {
        Self::from_vec(symbol, entries.into_iter().collect())
    }
    pub fn from_vec(symbol: Symbol, mut entries: Vec<DailyCandleEntry>) -> Option<Self> {
        if entries.is_empty() {
            return None;
        }

        entries.sort_by_key(|e| e.date);

        let start = entries.first().unwrap().date;
        let end = entries.last().unwrap().date;
        let open = entries.first().unwrap().open;
        let close = entries.last().unwrap().close;
        let mut high = Decimal::MIN;
        let mut low = Decimal::MAX;
        let mut volume = 0;

        for entry in &entries {
            if entry.high > high {
                high = entry.high;
            }
            if entry.low < low {
                low = entry.low;
            }
            volume += entry.volume;
        }

        Some(Self {
            symbol,
            start,
            end,
            open,
            high,
            low,
            close,
            volume,
            entries,
        })
    }
}

/// Candle Chart Entry
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DailyCandleEntry {
    pub date: NaiveDate,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CandleSeries {
    pub symbol: Symbol,
    pub interval: Interval,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: u64,
    pub entries: Vec<CandleEntry>,
}

impl CandleSeries {
    pub fn from_iter(
        symbol: Symbol,
        interval: Interval,
        entries: impl IntoIterator<Item = CandleEntry>,
    ) -> Option<Self> {
        Self::from_vec(symbol, interval, entries.into_iter().collect())
    }
    pub fn from_vec(symbol: Symbol, interval: Interval, entries: Vec<CandleEntry>) -> Option<Self> {
        let mut entries: Vec<CandleEntry> = entries.into_iter().collect();
        if entries.is_empty() {
            return None;
        }

        entries.sort_by_key(|e| e.timestamp.clone());

        let start = entries.first().unwrap().timestamp.clone();
        let end = entries.last().unwrap().timestamp.clone();
        let open = entries.first().unwrap().open;
        let close = entries.last().unwrap().close;
        let mut high = Decimal::MIN;
        let mut low = Decimal::MAX;
        let mut volume = 0;

        for entry in &entries {
            if entry.high > high {
                high = entry.high;
            }
            if entry.low < low {
                low = entry.low;
            }
            volume += entry.volume;
        }

        Some(Self {
            symbol,
            interval,
            start,
            end,
            open,
            high,
            low,
            close,
            volume,
            entries,
        })
    }
}

/// Candle Chart Entry
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CandleEntry {
    pub timestamp: NaiveDateTime,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: u64,
}

/// A Single Trade Tick
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TradeTick {
    /// Ticker Symbol
    pub symbol: Symbol,
    /// Trade Timestamp
    pub timestamp: NaiveDateTime,
    /// Trade Price
    pub price: Decimal,
    /// Trade Size (Volume)
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{Industry, Sector, Symbol};
    use chrono::NaiveDate;

    #[test]
    fn test_daily_candle_series_from_entries() {
        let symbol = Symbol::new("AAPL");
        let entries = vec![
            DailyCandleEntry {
                date: NaiveDate::from_ymd_opt(2023, 1, 2).unwrap(),
                open: Decimal::from_str_radix("101.0", 10).unwrap(),
                high: Decimal::from_str_radix("105.0", 10).unwrap(),
                low: Decimal::from_str_radix("99.0", 10).unwrap(),
                close: Decimal::from_str_radix("102.0", 10).unwrap(),
                volume: 1000,
            },
            DailyCandleEntry {
                date: NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
                open: Decimal::from_str_radix("100.0", 10).unwrap(),
                high: Decimal::from_str_radix("110.0", 10).unwrap(),
                low: Decimal::from_str_radix("90.0", 10).unwrap(),
                close: Decimal::from_str_radix("105.0", 10).unwrap(),
                volume: 500,
            },
        ];

        let series = DailyCandleSeries::from_iter(symbol.clone(), entries).unwrap();

        assert_eq!(series.symbol, symbol);
        assert_eq!(series.start, NaiveDate::from_ymd_opt(2023, 1, 1).unwrap());
        assert_eq!(series.end, NaiveDate::from_ymd_opt(2023, 1, 2).unwrap());
        assert_eq!(series.open, Decimal::from_str_radix("100.0", 10).unwrap());
        assert_eq!(series.close, Decimal::from_str_radix("102.0", 10).unwrap());
        assert_eq!(series.high, Decimal::from_str_radix("110.0", 10).unwrap());
        assert_eq!(series.low, Decimal::from_str_radix("90.0", 10).unwrap());
        assert_eq!(series.volume, 1500);
        assert_eq!(series.entries.len(), 2);
        assert_eq!(
            series.entries[0].date,
            NaiveDate::from_ymd_opt(2023, 1, 1).unwrap()
        );
    }

    #[test]
    fn test_candle_series_from_entries() {
        let symbol = Symbol::new("AAPL");
        let interval = Interval::OneMinute;
        let t1 = NaiveDateTime::from_timestamp_opt(1000, 0).unwrap();
        let t2 = NaiveDateTime::from_timestamp_opt(2000, 0).unwrap();
        let entries = vec![
            CandleEntry {
                timestamp: t2,
                open: Decimal::from_str_radix("101.0", 10).unwrap(),
                high: Decimal::from_str_radix("105.0", 10).unwrap(),
                low: Decimal::from_str_radix("99.0", 10).unwrap(),
                close: Decimal::from_str_radix("102.0", 10).unwrap(),
                volume: 1000,
            },
            CandleEntry {
                timestamp: t1,
                open: Decimal::from_str_radix("100.0", 10).unwrap(),
                high: Decimal::from_str_radix("110.0", 10).unwrap(),
                low: Decimal::from_str_radix("90.0", 10).unwrap(),
                close: Decimal::from_str_radix("105.0", 10).unwrap(),
                volume: 500,
            },
        ];

        let series = CandleSeries::from_iter(symbol.clone(), interval, entries).unwrap();

        assert_eq!(series.symbol, symbol);
        assert_eq!(series.start, t1);
        assert_eq!(series.end, t2);
        assert_eq!(series.open, Decimal::from_str_radix("100.0", 10).unwrap());
        assert_eq!(series.close, Decimal::from_str_radix("102.0", 10).unwrap());
        assert_eq!(series.high, Decimal::from_str_radix("110.0", 10).unwrap());
        assert_eq!(series.low, Decimal::from_str_radix("90.0", 10).unwrap());
        assert_eq!(series.volume, 1500);
        assert_eq!(series.entries.len(), 2);
        assert_eq!(series.entries[0].timestamp, t1);
    }
}
