//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

mod error;
pub mod model;

pub use self::error::*;

#[cfg(test)]
mod tests {
    use super::model::*;
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    // ── Symbol ────────────────────────────────────────────────────────────────

    #[test]
    fn symbol_equality() {
        assert_eq!(Symbol::new("AAPL"), Symbol::new("AAPL"));
        assert_ne!(Symbol::new("AAPL"), Symbol::new("GOOG"));
    }

    #[test]
    fn symbol_as_str_roundtrip() {
        let sym = Symbol::new("MSFT");
        assert_eq!(sym.as_str(), "MSFT");
    }

    #[test]
    fn symbol_ordering() {
        let a = Symbol::new("AAPL");
        let b = Symbol::new("ZZZZ");
        assert!(a < b);
    }

    #[test]
    fn symbol_clone_equality() {
        let s = Symbol::new("TSLA");
        assert_eq!(s.clone(), s);
    }

    #[test]
    fn symbol_hashable_in_map() {
        use std::collections::HashMap;
        let mut map: HashMap<Symbol, i32> = HashMap::new();
        map.insert(Symbol::new("AAPL"), 1);
        map.insert(Symbol::new("GOOG"), 2);
        assert_eq!(map[&Symbol::new("AAPL")], 1);
        assert_eq!(map[&Symbol::new("GOOG")], 2);
    }

    // ── Exchange ──────────────────────────────────────────────────────────────

    #[test]
    fn exchange_as_str() {
        let ex = Exchange::new("NASDAQ");
        assert_eq!(ex.as_str(), "NASDAQ");
    }

    #[test]
    fn exchange_equality() {
        assert_eq!(Exchange::new("NYSE"), Exchange::new("NYSE"));
        assert_ne!(Exchange::new("NYSE"), Exchange::new("NASDAQ"));
    }

    // ── Interval ──────────────────────────────────────────────────────────────

    #[test]
    fn interval_as_str() {
        assert_eq!(Interval::OneMinute.as_str(), "1m");
        assert_eq!(Interval::FiveMinute.as_str(), "5m");
        assert_eq!(Interval::FifteenMinute.as_str(), "15m");
        assert_eq!(Interval::OneHour.as_str(), "1h");
        assert_eq!(Interval::OneDay.as_str(), "daily");
    }

    #[test]
    fn interval_copy_and_equality() {
        let i = Interval::OneDay;
        let j = i;
        assert_eq!(i, j);
    }

    // ── DailyCandleEntry ──────────────────────────────────────────────────────

    #[test]
    fn daily_candle_entry_fields() {
        let dec = |s: &str| Decimal::from_str(s).unwrap();
        let entry = DailyCandleEntry {
            date: NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
            open: dec("100.00"),
            high: dec("110.00"),
            low: dec("95.00"),
            close: dec("108.50"),
            volume: 1_234_567,
        };
        assert_eq!(entry.close, dec("108.50"));
        assert!(entry.high >= entry.low);
        assert!(entry.high >= entry.open);
        assert!(entry.high >= entry.close);
    }

    // ── DailyCandleSeries ─────────────────────────────────────────────────────

    #[test]
    fn daily_candle_series_empty_returns_none() {
        assert!(DailyCandleSeries::from_iter(Symbol::new("AAPL"), vec![]).is_none());
    }

    #[test]
    fn daily_candle_series_aggregates_correctly() {
        let dec = |s: &str| Decimal::from_str(s).unwrap();
        let entries = vec![
            DailyCandleEntry {
                date: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                open: dec("100"),
                high: dec("105"),
                low: dec("98"),
                close: dec("103"),
                volume: 1_000,
            },
            DailyCandleEntry {
                date: NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
                open: dec("103"),
                high: dec("112"),
                low: dec("102"),
                close: dec("110"),
                volume: 2_000,
            },
        ];
        let series = DailyCandleSeries::from_iter(Symbol::new("TEST"), entries).unwrap();
        assert_eq!(series.open, dec("100")); // first bar open
        assert_eq!(series.close, dec("110")); // last bar close
        assert_eq!(series.high, dec("112")); // max high
        assert_eq!(series.low, dec("98")); // min low
        assert_eq!(series.volume, 3_000); // sum
        assert_eq!(series.entries.len(), 2);
    }

    #[test]
    fn daily_candle_series_sorted_oldest_first() {
        let dec = |s: &str| Decimal::from_str(s).unwrap();
        // Insert out of order.
        let entries = vec![
            DailyCandleEntry {
                date: NaiveDate::from_ymd_opt(2024, 1, 3).unwrap(),
                open: dec("110"),
                high: dec("115"),
                low: dec("108"),
                close: dec("112"),
                volume: 500,
            },
            DailyCandleEntry {
                date: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                open: dec("100"),
                high: dec("105"),
                low: dec("98"),
                close: dec("103"),
                volume: 1_000,
            },
        ];
        let series = DailyCandleSeries::from_iter(Symbol::new("TEST"), entries).unwrap();
        assert!(
            series.entries[0].date < series.entries[1].date,
            "Should be sorted oldest first"
        );
    }

    // ── Ticker ────────────────────────────────────────────────────────────────

    #[test]
    fn ticker_active_by_default() {
        let ticker = Ticker {
            symbol: Symbol::new("NVDA"),
            exchange: None,
            name: Some("NVIDIA Corp".to_string()),
            country: Some("US".to_string()),
            industry: None,
            sector: None,
            ipoyear: Some("1999".to_string()),
            marketcap: Some(Decimal::from_str("1000000000000").unwrap()),
            description: None,
            active: true,
        };
        assert!(ticker.active);
        assert_eq!(ticker.symbol.as_str(), "NVDA");
        assert_eq!(ticker.name.as_deref(), Some("NVIDIA Corp"));
    }
}
